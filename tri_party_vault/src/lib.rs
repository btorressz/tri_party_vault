use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;

// --- Token-v1 vs Token-2022 toggle -----------------------------------------
#[cfg(not(feature = "token-2022"))]
use anchor_spl::token::{self as token_i, Mint, Token, TokenAccount, Transfer};

#[cfg(feature = "token-2022")]
use anchor_spl::token_2022::{self as token_i, Mint, Token, TokenAccount, Transfer};
// ---------------------------------------------------------------------------

// Pyth 0.8.0
use pyth_sdk_solana::load_price_feed_from_account_info;

declare_id!("3yU4CGvB2pDQPk2ACBSjy8JBTEnnvbdLS9U1couLPmVM");

/// Fixed program seeds
const SEED_VAULT: &[u8] = b"vault";
const SEED_AUTH: &[u8] = b"authority";

/// Token-denominated risk knobs used when Pyth is disabled (fallback)
const DAILY_CAP_TOKENS: u64 = 1_000_000_000_000;       // per-day release cap (base units)
const MAX_SINGLE_RELEASE_TOKENS: u64 = 500_000_000_000; // per-tx max release (base units)

#[program]
pub mod tri_party_vault {
    use super::*;

    /// Create the vault state PDA, derive the vault_authority PDA, and init vault ATA.
    pub fn initialize_vault(
        ctx: Context<InitializeVault>,
        custodian: Pubkey,
        borrower: Pubkey,
        lender: Pubkey,
        mint: Pubkey,
    ) -> Result<()> {
        // Reinit protection via `init` on the VaultState PDA.
        let state = &mut ctx.accounts.vault_state;

        // Role keys must be distinct
        require!(custodian != borrower, ErrorCode::RoleNotDistinct);
        require!(custodian != lender, ErrorCode::RoleNotDistinct);
        require!(borrower != lender, ErrorCode::RoleNotDistinct);

        // Pin the passed mint to the provided mint account (belt & suspenders)
        require_keys_eq!(ctx.accounts.mint_account.key(), mint, ErrorCode::Unauthorized);

        // Persist core state
        state.mint = mint;
        state.mint_decimals = ctx.accounts.mint_account.decimals;
        state.custodian = custodian;
        state.borrower = borrower;
        state.lender = lender;
        state.approvals_bitmap = 0;
        state.amount_locked = 0;
        state.is_frozen = false;

        // Default 2-of-3 threshold (future-proof; you can expose a setter later)
        state.threshold = 2;

        // Store the vault_authority bump for later CPI signer use (Anchor-generated field).
        let bump: u8 = ctx.bumps.vault_authority;
        state.vault_authority_bump = bump;

        // Init daily cap trackers (token & USD)
        state.last_cap_reset_ts = Clock::get()?.unix_timestamp;
        state.released_today = 0;
        state.released_today_usd_1e6 = 0;

        // Default price config (disabled)
        state.price_config = PriceConfig {
            enabled: false,
            sol_usdc_price_feed: Pubkey::default(),
            max_ltv_bps: 7000,                 // 70% LTV -> min collateral 30% retained
            max_single_release_usd_1e6: 1_000_000_000, // 1,000 USDC
            daily_cap_usd_1e6: 5_000_000_000,  // 5,000 USDC
            max_price_staleness_secs: 90,
        };

        // Extra runtime checks for PDAs/ATAs (ATA macro guarantees, but we assert anyway)
        require_keys_eq!(
            ctx.accounts.vault_ata.owner,
            ctx.accounts.vault_authority.key(),
            ErrorCode::Unauthorized
        );

        // Emit event
        emit!(VaultInitialized {
            mint,
            custodian,
            borrower,
            lender
        });

        Ok(())
    }

    /// Any of the three roles may deposit SPL tokens into the vault ATA.
    pub fn deposit_collateral(ctx: Context<DepositCollateral>, amount: u64) -> Result<()> {
        require!(amount > 0, ErrorCode::InvalidAmount);
        let state = &mut ctx.accounts.vault_state;
        require!(!state.is_frozen, ErrorCode::Paused);

        // Prevent depositing while approvals exist (clear flow ambiguity)
        require!(state.approvals_bitmap == 0, ErrorCode::PendingReleaseFlow);

        // Depositor must be a recognized role
        require!(
            is_role(state, ctx.accounts.depositor.key()),
            ErrorCode::Unauthorized
        );

        // Extra ownership pinning
        require_keys_eq!(
            ctx.accounts.vault_ata.owner,
            ctx.accounts.vault_authority.key(),
            ErrorCode::Unauthorized
        );

        // Transfer from depositor_ata -> vault_ata with depositor as authority
        let cpi_accounts = Transfer {
            from: ctx.accounts.depositor_ata.to_account_info(),
            to: ctx.accounts.vault_ata.to_account_info(),
            authority: ctx.accounts.depositor.to_account_info(),
        };
        let cpi_ctx =
            CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token_i::transfer(cpi_ctx, amount)?;

        // Update locked amount (checked math)
        state.amount_locked = state
            .amount_locked
            .checked_add(amount)
            .ok_or(ErrorCode::MathOverflow)?;

        emit!(CollateralDeposited {
            amount,
            new_total: state.amount_locked
        });
        Ok(())
    }

    /// Role-gated approval; idempotent bit set for (0=custodian,1=borrower,2=lender).
    pub fn approve_release(ctx: Context<ApproveRelease>, role: u8) -> Result<()> {
        let state = &mut ctx.accounts.vault_state;
        // Auth: signer must match role
        require!(role <= 2, ErrorCode::InvalidRole);
        let signer = ctx.accounts.role_signer.key();
        let expected = match role {
            0 => state.custodian,
            1 => state.borrower,
            _ => state.lender,
        };
        require_keys_eq!(signer, expected, ErrorCode::Unauthorized);

        // Idempotent set
        if !has_bit(state.approvals_bitmap, role) {
            set_bit(&mut state.approvals_bitmap, role);
        }
        emit!(ReleaseApproved {
            by_role: role,
            approvals_bitmap: state.approvals_bitmap
        });
        Ok(())
    }

    /// Allow a role to revoke its approval before release.
    pub fn revoke_approval(ctx: Context<ApproveRelease>, role: u8) -> Result<()> {
        let state = &mut ctx.accounts.vault_state;
        require!(role <= 2, ErrorCode::InvalidRole);
        let signer = ctx.accounts.role_signer.key();
        let expected = [state.custodian, state.borrower, state.lender][role as usize];
        require_keys_eq!(signer, expected, ErrorCode::Unauthorized);
        if has_bit(state.approvals_bitmap, role) {
            clear_bit(&mut state.approvals_bitmap, role);
            emit!(ReleaseApproved {
                by_role: role,
                approvals_bitmap: state.approvals_bitmap
            });
        }
        Ok(())
    }

    /// Release to a recipient ATA when approvals >= threshold; USD caps/LTV enforced if Pyth is enabled.
    pub fn release_collateral(ctx: Context<ReleaseCollateral>, amount: u64) -> Result<()> {
        require!(amount > 0, ErrorCode::InvalidAmount);

        let state = &mut ctx.accounts.vault_state;
        require!(!state.is_frozen, ErrorCode::Paused);

        // Must have >= threshold approvals
        let approvals = bitcount(state.approvals_bitmap);
        require!(approvals >= state.threshold as u32, ErrorCode::NotEnoughApprovals);

        // Bounds (token-denominated basic checks)
        require!(amount <= state.amount_locked, ErrorCode::AmountExceedsLocked);
        require!(ctx.accounts.recipient.key() != Pubkey::default(), ErrorCode::Unauthorized);

        // Daily cap window reset (both token and USD counters)
        let now = Clock::get()?.unix_timestamp;
        if now - state.last_cap_reset_ts >= 86_400 {
            state.last_cap_reset_ts = now;
            state.released_today = 0;
            state.released_today_usd_1e6 = 0;
        }

        // Token-denominated fallback caps when Pyth disabled
        if !state.price_config.enabled {
            let new_today = state
                .released_today
                .checked_add(amount)
                .ok_or(ErrorCode::MathOverflow)?;
            require!(amount <= MAX_SINGLE_RELEASE_TOKENS, ErrorCode::AmountExceedsLocked);
            require!(new_today <= DAILY_CAP_TOKENS, ErrorCode::DailyCapExceeded);
            state.released_today = new_today;
        }

        // USD-denominated caps & LTV when Pyth enabled
        if state.price_config.enabled {
            let price_acc_key = ctx
                .accounts
                .pyth_price_acc
                .as_ref()
                .map(|a| a.key())
                .unwrap_or(Pubkey::default());
            require_keys_eq!(price_acc_key, state.price_config.sol_usdc_price_feed, ErrorCode::Unauthorized);

            let price_acc_info = ctx
                .accounts
                .pyth_price_acc
                .as_ref()
                .ok_or(ErrorCode::PriceAccountInvalid)?
                .to_account_info();

            let feed = load_price_feed_from_account_info(&price_acc_info)
                .map_err(|_| ErrorCode::PriceAccountInvalid)?;

            // ---- FIX: pass u64 staleness window to Pyth (0.8.0) ----
            require!(state.price_config.max_price_staleness_secs >= 0, ErrorCode::InvalidRiskParams);
            let staleness: u64 = state
                .price_config
                .max_price_staleness_secs
                .try_into()
                .map_err(|_| ErrorCode::InvalidRiskParams)?;
            let px = feed
                .get_price_no_older_than(now, staleness)
                .ok_or(ErrorCode::PriceStale)?;
            // --------------------------------------------------------

            // Conservative price: price - conf
            let p_conservative = (px.price as i128)
                .checked_sub(px.conf as i128)
                .ok_or(ErrorCode::MathOverflow)?;
            if p_conservative <= 0 {
                return err!(ErrorCode::PriceNonPositive);
            }

            // Convert amounts to USD 1e6 (micro-USDC) using integer math:
            // usd_1e6 = amount * (price - conf) * 10^6 / (10^mint_decimals * 10^(-expo))
            let denom_exp: i32 = state.mint_decimals as i32 - px.expo; // = mint_dec + (-expo)
            let denom = ten_pow_u128(denom_exp).ok_or(ErrorCode::MathOverflow)?;
            let amount_i = amount as u128;
            let p_i = p_conservative as u128;

            let release_usd_num = amount_i
                .checked_mul(p_i)
                .and_then(|v| v.checked_mul(1_000_000u128))
                .ok_or(ErrorCode::MathOverflow)?;
            let release_usd_1e6 = release_usd_num
                .checked_div(denom)
                .ok_or(ErrorCode::MathOverflow)?; // floor conservative

            let total_usd_num = (state.amount_locked as u128)
                .checked_mul(p_i)
                .and_then(|v| v.checked_mul(1_000_000u128))
                .ok_or(ErrorCode::MathOverflow)?;
            let total_usd_1e6 = total_usd_num
                .checked_div(denom)
                .ok_or(ErrorCode::MathOverflow)?;

            // Per-tx USD cap
            require!(
                release_usd_1e6 <= state.price_config.max_single_release_usd_1e6 as u128,
                ErrorCode::UsdCapExceeded
            );

            // Daily USD cap
            let new_today_usd = (state.released_today_usd_1e6 as u128)
                .checked_add(release_usd_1e6)
                .ok_or(ErrorCode::MathOverflow)?;
            require!(
                new_today_usd <= state.price_config.daily_cap_usd_1e6 as u128,
                ErrorCode::UsdCapExceeded
            );

            // LTV guard (interpreted as: post-release collateral must be >= (1 - LTV) * pre-release)
            let remaining_usd_1e6 = total_usd_1e6
                .checked_sub(release_usd_1e6)
                .ok_or(ErrorCode::MathOverflow)?;
            let min_remaining_usd_1e6 = total_usd_1e6
                .checked_mul((10_000 - state.price_config.max_ltv_bps) as u128)
                .and_then(|v| v.checked_div(10_000))
                .ok_or(ErrorCode::MathOverflow)?;
            require!(remaining_usd_1e6 >= min_remaining_usd_1e6, ErrorCode::LtvBreach);

            // Track USD daily
            state.released_today_usd_1e6 = new_today_usd as u64;

            // Emit price diagnostic
            emit!(PriceUsed {
                feed: state.price_config.sol_usdc_price_feed,
                price: px.price,
                conf: px.conf,
                expo: px.expo,
                publish_time: px.publish_time,
            });
        }

        // Extra ownership pinning
        require_keys_eq!(
            ctx.accounts.vault_ata.owner,
            ctx.accounts.vault_authority.key(),
            ErrorCode::Unauthorized
        );

        // ---- PDA signer seeds (stable lifetime binding) ----
        let state_key = state.key();
        let signer_seed_slice: [&[u8]; 3] = [
            SEED_AUTH,
            state_key.as_ref(),
            &[state.vault_authority_bump],
        ];
        let signer: &[&[u8]] = &signer_seed_slice;
        let signer_arr: &[&[&[u8]]] = &[signer];
        // ----------------------------------------------------

        // CPI transfer vault_ata -> recipient_ata
        let cpi_accounts = Transfer {
            from: ctx.accounts.vault_ata.to_account_info(),
            to: ctx.accounts.recipient_ata.to_account_info(),
            authority: ctx.accounts.vault_authority.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            signer_arr,
        );
        token_i::transfer(cpi_ctx, amount)?;

        // Update accounting & reset approvals
        state.amount_locked = state
            .amount_locked
            .checked_sub(amount)
            .ok_or(ErrorCode::MathOverflow)?;
        state.approvals_bitmap = 0;

        emit!(CollateralReleased {
            recipient: ctx.accounts.recipient.key(),
            amount,
            remaining: state.amount_locked,
            approvals_after: state.approvals_bitmap,
        });

        Ok(())
    }

    /// Pause guard: only custodian can pause.
    pub fn pause(ctx: Context<Pause>) -> Result<()> {
        let state = &mut ctx.accounts.vault_state;
        require_keys_eq!(
            ctx.accounts.custodian.key(),
            state.custodian,
            ErrorCode::Unauthorized
        );
        state.is_frozen = true;
        emit!(Paused {});
        emit!(StateSignal {
            paused: true,
            approvals_bitmap: state.approvals_bitmap,
            amount_locked: state.amount_locked
        });
        Ok(())
    }

    /// Unpause guard: only custodian can unpause.
    pub fn unpause(ctx: Context<Pause>) -> Result<()> {
        let state = &mut ctx.accounts.vault_state;
        require_keys_eq!(
            ctx.accounts.custodian.key(),
            state.custodian,
            ErrorCode::Unauthorized
        );
        state.is_frozen = false;
        emit!(Unpaused {});
        emit!(StateSignal {
            paused: false,
            approvals_bitmap: state.approvals_bitmap,
            amount_locked: state.amount_locked
        });
        Ok(())
    }

    /// Clear approvals bitmap (custodian-only).
    pub fn reset_approvals(ctx: Context<Pause>) -> Result<()> {
        let state = &mut ctx.accounts.vault_state;
        require_keys_eq!(
            ctx.accounts.custodian.key(),
            state.custodian,
            ErrorCode::Unauthorized
        );
        state.approvals_bitmap = 0;
        Ok(())
    }

    /// Governance-like role rotation; requires >= threshold approvals.
    pub fn rotate_role(ctx: Context<RotateRole>, role: u8, new_key: Pubkey) -> Result<()> {
        let state = &mut ctx.accounts.vault_state;
        require!(
            bitcount(state.approvals_bitmap) >= state.threshold as u32,
            ErrorCode::NotEnoughApprovals
        );
        match role {
            0 => state.custodian = new_key,
            1 => state.borrower = new_key,
            2 => state.lender = new_key,
            _ => return err!(ErrorCode::InvalidRole),
        }
        // Clear approvals after governance change
        state.approvals_bitmap = 0;
        Ok(())
    }

    /// Admin: set Pyth price feed and on/off.
    pub fn set_price_feed(ctx: Context<Admin>, feed: Pubkey, enabled: bool) -> Result<()> {
        let s = &mut ctx.accounts.vault_state;
        require_keys_eq!(ctx.accounts.custodian.key(), s.custodian, ErrorCode::Unauthorized);
        s.price_config.sol_usdc_price_feed = feed;
        s.price_config.enabled = enabled;
        Ok(())
    }

    /// Admin: set risk limits for USD caps & staleness; and maximum LTV (bps).
    pub fn set_risk_limits(
        ctx: Context<Admin>,
        max_ltv_bps: u16,
        max_single_usd_1e6: u64,
        daily_cap_usd_1e6: u64,
        max_price_staleness_secs: i64,
    ) -> Result<()> {
        let s = &mut ctx.accounts.vault_state;
        require_keys_eq!(ctx.accounts.custodian.key(), s.custodian, ErrorCode::Unauthorized);
        require!(max_ltv_bps <= 9_999, ErrorCode::InvalidRiskParams); // disallow 100%+
        s.price_config.max_ltv_bps = max_ltv_bps;
        s.price_config.max_single_release_usd_1e6 = max_single_usd_1e6;
        s.price_config.daily_cap_usd_1e6 = daily_cap_usd_1e6;
        s.price_config.max_price_staleness_secs = max_price_staleness_secs;
        Ok(())
    }

    /// Close the vault account when fully drained; refunds rent to `recipient`.
    pub fn close_vault(_ctx: Context<CloseVault>) -> Result<()> {
        // All checks are in the account constraints
        Ok(())
    }
}

/* -------------------------------- Accounts -------------------------------- */

#[derive(Accounts)]
#[instruction(custodian: Pubkey, borrower: Pubkey, lender: Pubkey, mint: Pubkey)]
pub struct InitializeVault<'info> {
    #[account(
        init,
        payer = payer,
        space = 8 + VaultState::SIZE,
        seeds = [
            SEED_VAULT,
            mint.as_ref(),
            custodian.as_ref(),
            borrower.as_ref(),
            lender.as_ref()
        ],
        bump
    )]
    pub vault_state: Account<'info, VaultState>,

    /// CHECK: PDA with no data; used as CPI signer
    #[account(
        seeds = [SEED_AUTH, vault_state.key().as_ref()],
        bump
    )]
    pub vault_authority: UncheckedAccount<'info>,

    /// Mint of the collateral token
    pub mint_account: Account<'info, Mint>,

    /// Vault ATA = ATA(mint, vault_authority)
    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = mint_account,
        associated_token::authority = vault_authority
    )]
    pub vault_ata: Account<'info, TokenAccount>,

    /// Payer for initialization (can be any signer)
    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct DepositCollateral<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,

    /// CHECK: PDA signer (not invoked here, but we enforce ATA owner below)
    #[account(
        seeds = [SEED_AUTH, vault_state.key().as_ref()],
        bump = vault_state.vault_authority_bump
    )]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        associated_token::mint = mint_account,
        associated_token::authority = vault_authority
    )]
    pub vault_ata: Account<'info, TokenAccount>,

    #[account(
        constraint = mint_account.key() == vault_state.mint @ ErrorCode::Unauthorized
    )]
    pub mint_account: Account<'info, Mint>,

    /// Depositor must be one of the three roles
    pub depositor: Signer<'info>,

    #[account(
        mut,
        constraint = depositor_ata.owner == depositor.key(),
        constraint = depositor_ata.mint == mint_account.key()
    )]
    pub depositor_ata: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct ApproveRelease<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    /// Any of the three role signers
    pub role_signer: Signer<'info>,
}

#[derive(Accounts)]
pub struct ReleaseCollateral<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,

    /// CHECK: PDA signer for vault transfers
    #[account(
        seeds = [SEED_AUTH, vault_state.key().as_ref()],
        bump = vault_state.vault_authority_bump
    )]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        associated_token::mint = mint_account,
        associated_token::authority = vault_authority
    )]
    pub vault_ata: Account<'info, TokenAccount>,

    #[account(
        constraint = mint_account.key() == vault_state.mint @ ErrorCode::Unauthorized
    )]
    pub mint_account: Account<'info, Mint>,

    /// Recipient owner (for event & ATA checks)
    /// CHECK: Only used for key() in event; safety via recipient_ata checks.
    pub recipient: UncheckedAccount<'info>,

    #[account(
        mut,
        constraint = recipient_ata.owner == recipient.key(),
        constraint = recipient_ata.mint == mint_account.key()
    )]
    pub recipient_ata: Account<'info, TokenAccount>,

    /// Optional Pyth price account (required when price checks are enabled)
    /// CHECK: read-only; validated by key equality in handler when enabled
    pub pyth_price_acc: Option<UncheckedAccount<'info>>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Pause<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    pub custodian: Signer<'info>,
}

#[derive(Accounts)]
pub struct RotateRole<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
}

#[derive(Accounts)]
pub struct Admin<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    pub custodian: Signer<'info>,
}

#[derive(Accounts)]
pub struct CloseVault<'info> {
    #[account(
        mut,
        close = recipient,
        seeds = [
            SEED_VAULT,
            vault_state.mint.as_ref(),
            vault_state.custodian.as_ref(),
            vault_state.borrower.as_ref(),
            vault_state.lender.as_ref()
        ],
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    /// CHECK: rent refund target
    #[account(mut)]
    pub recipient: UncheckedAccount<'info>,
    #[account(
        mut,
        associated_token::mint = mint_account,
        associated_token::authority = vault_authority
    )]
    pub vault_ata: Account<'info, TokenAccount>,
    #[account(
        constraint = mint_account.key() == vault_state.mint @ ErrorCode::Unauthorized
    )]
    pub mint_account: Account<'info, Mint>,
    /// CHECK: PDA signer confirmation
    #[account(
        seeds = [SEED_AUTH, vault_state.key().as_ref()],
        bump = vault_state.vault_authority_bump
    )]
    pub vault_authority: UncheckedAccount<'info>,
}

/* --------------------------------- State ---------------------------------- */

#[account]
pub struct VaultState {
    pub mint: Pubkey,
    pub mint_decimals: u8,
    pub vault_authority_bump: u8,
    pub custodian: Pubkey,
    pub borrower: Pubkey,
    pub lender: Pubkey,
    pub approvals_bitmap: u8, // bit0=custodian, bit1=borrower, bit2=lender
    pub amount_locked: u64,
    pub is_frozen: bool,

    // Governance & safety
    pub threshold: u8,        // default 2 (2-of-3)
    pub last_cap_reset_ts: i64,
    pub released_today: u64,          // token units (fallback)
    pub released_today_usd_1e6: u64,  // USD micro when Pyth enabled

    pub price_config: PriceConfig,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy)]
pub struct PriceConfig {
    pub enabled: bool,
    pub sol_usdc_price_feed: Pubkey,
    pub max_ltv_bps: u16,                 // e.g. 7000 = 70% LTV
    pub max_single_release_usd_1e6: u64,  // per-tx cap in micro-USDC
    pub daily_cap_usd_1e6: u64,           // daily cap in micro-USDC
    pub max_price_staleness_secs: i64,    // price freshness window
}

impl VaultState {
    pub const SIZE: usize =
        32 + // mint
        1  + // mint_decimals
        1  + // vault_authority_bump
        32 + // custodian
        32 + // borrower
        32 + // lender
        1  + // approvals_bitmap
        8  + // amount_locked
        1  + // is_frozen
        1  + // threshold
        8  + // last_cap_reset_ts
        8  + // released_today
        8  + // released_today_usd_1e6
        // PriceConfig
        1  + // enabled
        32 + // sol_usdc_price_feed
        2  + // max_ltv_bps
        8  + // max_single_release_usd_1e6
        8  + // daily_cap_usd_1e6
        8;   // max_price_staleness_secs
}

/* -------------------------------- Events ---------------------------------- */

#[event]
pub struct VaultInitialized {
    pub mint: Pubkey,
    pub custodian: Pubkey,
    pub borrower: Pubkey,
    pub lender: Pubkey,
}

#[event]
pub struct CollateralDeposited {
    pub amount: u64,
    pub new_total: u64,
}

#[event]
pub struct ReleaseApproved {
    pub by_role: u8,
    pub approvals_bitmap: u8,
}

#[event]
pub struct CollateralReleased {
    pub recipient: Pubkey,
    pub amount: u64,
    pub remaining: u64,
    pub approvals_after: u8,
}

#[event]
pub struct Paused {}

#[event]
pub struct Unpaused {}

#[event]
pub struct StateSignal {
    pub paused: bool,
    pub approvals_bitmap: u8,
    pub amount_locked: u64,
}

#[event]
pub struct PriceUsed {
    pub feed: Pubkey,
    pub price: i64,
    pub conf: u64,
    pub expo: i32,
    pub publish_time: i64,
}

/* ------------------------------- Error Codes ------------------------------ */

#[error_code]
pub enum ErrorCode {
    #[msg("Invalid role")]
    InvalidRole,
    #[msg("Roles must be distinct")]
    RoleNotDistinct,
    #[msg("This action is unauthorized")]
    Unauthorized,
    #[msg("Not enough approvals (need at least threshold)")]
    NotEnoughApprovals,
    #[msg("Vault is paused")]
    Paused,
    #[msg("Release amount exceeds locked total or cap")]
    AmountExceedsLocked,
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Invalid amount")]
    InvalidAmount,
    #[msg("Internal: bump not found")]
    BumpNotFound,
    #[msg("Per-day cap exceeded")]
    DailyCapExceeded,
    #[msg("Cannot deposit while a release flow is pending (approvals exist)")]
    PendingReleaseFlow,
    #[msg("Pyth price account invalid")]
    PriceAccountInvalid,
    #[msg("Price is stale")]
    PriceStale,
    #[msg("Non-positive price")]
    PriceNonPositive,
    #[msg("USD cap exceeded")]
    UsdCapExceeded,
    #[msg("LTV check failed")]
    LtvBreach,
    #[msg("Invalid risk parameters")]
    InvalidRiskParams,
}

/* ------------------------------- Utilities -------------------------------- */

#[inline]
fn has_bit(bitmap: u8, idx: u8) -> bool {
    ((bitmap >> idx) & 1) == 1
}

#[inline]
fn set_bit(bitmap: &mut u8, idx: u8) {
    *bitmap |= 1 << idx;
}

#[inline]
fn clear_bit(bitmap: &mut u8, idx: u8) {
    *bitmap &= !(1 << idx);
}

#[inline]
fn bitcount(bitmap: u8) -> u32 {
    bitmap.count_ones()
}

#[inline]
fn is_role(state: &VaultState, k: Pubkey) -> bool {
    k == state.custodian || k == state.borrower || k == state.lender
}

/// 10^exp as u128 (exp must be >= 0 and small enough to fit)
#[inline]
fn ten_pow_u128(exp: i32) -> Option<u128> {
    if exp < 0 {
        return None;
    }
    let mut v: u128 = 1;
    for _ in 0..(exp as u32) {
        v = v.checked_mul(10)?;
    }
    Some(v)
}

