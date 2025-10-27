# 🔐 Tri-Party Vault - Solana Smart Contract(program)

A secure multi-signature vault system for Solana that enables three-party collateral management with approval-based releases and built-in safety mechanisms.

- **NOTE** This is a proof of concept that was developed in solana playground and i am still making changes and working on the tests

## 📖 Overview

The Tri-Party Vault is a Solana program designed for scenarios requiring trustless collateral management between three parties: a Custodian, Borrower, and Lender. The vault enforces a configurable threshold approval system (default 2-of-3) before any collateral can be released, with additional safety features including daily caps, per-transaction limits, and pause functionality.

## ✨ Key Features

### ✅ Multi-Party Approval System
- **Three Roles**: Custodian, Borrower, and Lender
- **Threshold-Based Releases**: Requires 2 out of 3 approvals by default
- **Idempotent Approvals**: Each party can approve or revoke their approval before release
- **Role Rotation**: Governance mechanism to change role holders (requires threshold approvals)

### 🛡️ Safety Mechanisms
- **Daily Release Cap**: 1,000,000,000,000 base units per 24-hour period
- **Per-Transaction Maximum**: 500,000,000,000 base units per release
- **Pause/Unpause**: Custodian can freeze all operations
- **Overflow Protection**: Checked arithmetic throughout
- **Deposit Protection**: Prevents deposits while approvals are pending

### 💱 Token Support
- **SPL Token & Token-2022**: Toggle via feature flag (token-2022)
- **Associated Token Accounts**: Automatic ATA management
- **Mint-Specific Vaults**: Each vault is tied to a specific token mint

## 🏗️ Program Architecture

### 📛 Program ID
The program is deployed at address: 3yU4CGvB2pDQPk2ACBSjy8JBTEnnvbdLS9U1couLPmVM 
- **devnet**:(https://explorer.solana.com/address/3yU4CGvB2pDQPk2ACBSjy8JBTEnnvbdLS9U1couLPmVM?cluster=devnet)

### 🧬 PDA Seeds
- **Vault State**: Derived from the seed components "vault", mint address, custodian address, borrower address, and lender address
- **Vault Authority**: Derived from the seed components "authority" and the vault state account key

### 🧠 State Structure

The VaultState account stores the following fields:

- **mint**: A Pubkey that stores the token mint address for the collateral token
- **vault_authority_bump**: A single byte that stores the PDA bump seed for the vault authority
- **custodian**: A Pubkey representing the custodian role holder
- **borrower**: A Pubkey representing the borrower role holder
- **lender**: A Pubkey representing the lender role holder
- **approvals_bitmap**: A single byte using bit flags to track which roles have approved (bit 0 for custodian, bit 1 for borrower, bit 2 for lender)
- **amount_locked**: An unsigned 64-bit integer tracking the total amount of collateral currently locked in the vault
- **is_frozen**: A boolean flag indicating whether the vault is currently paused
- **threshold**: A single byte storing the number of required approvals, which defaults to 2 for a 2-of-3 setup
- **last_cap_reset_ts**: A signed 64-bit integer storing the Unix timestamp of when the daily cap was last reset
- **released_today**: An unsigned 64-bit integer tracking the amount of tokens released in the current 24-hour period

## 🧾 Instructions

### 1. Initialize Vault
Creates a new vault with three parties and a specific token mint.

**Parameters:**
- custodian: The public key of the custodian
- borrower: The public key of the borrower
- lender: The public key of the lender
- mint: The public key of the token mint

**Accounts:**
- vault_state: The initialized PDA account
- vault_authority: The PDA signer account
- mint_account: The token mint account
- vault_ata: The vault's associated token account
- payer: The transaction fee payer

### 2. Deposit Collateral
Any of the three parties can deposit tokens into the vault.

**Parameters:**
- amount: An unsigned 64-bit integer representing the amount to deposit

**Requirements:**
- Signer must be custodian, borrower, or lender
- No pending approvals (approvals_bitmap must be 0)
- Vault not frozen
- Amount > 0

### 3. Approve Release
A party signals approval for the next release.

**Parameters:**
- role: A single byte value where 0 represents custodian, 1 represents borrower, and 2 represents lender

**Requirements:**
- Signer matches the specified role

### 4. Revoke Approval
A party can revoke their approval before release is executed.

**Parameters:**
- role: A single byte value representing the role

**Requirements:**
- Signer matches the specified role

### 5. Release Collateral
Transfer tokens from vault to a recipient when threshold approvals are met.

**Parameters:**
- amount: An unsigned 64-bit integer representing the amount to release

**Requirements:**
- Approvals >= threshold (default 2)
- Amount <= locked balance
- Amount <= MAX_SINGLE_RELEASE
- Daily cap not exceeded
- Vault not frozen

**Effects:**
- Resets all approvals to 0
- Updates daily cap tracking
- Transfers tokens to recipient

### 6. Pause
Custodian-only: freeze all deposits and releases.

### 7. Unpause
Custodian-only: unfreeze the vault.

### 8. Reset Approvals
Custodian-only: clear all pending approvals.

### 9. Rotate Role
Change a role holder (governance action requiring threshold approvals).

**Parameters:**
- role: A single byte value representing which role to rotate
- new_key: The public key of the new role holder

### 10. Close Vault
Close the vault account when fully drained (amount_locked = 0).

## 📢 Events

- **VaultInitialized**: Emitted on vault creation
- **CollateralDeposited**: Tracks deposits
- **ReleaseApproved**: Records approval actions
- **CollateralReleased**: Logs successful releases
- **Paused / Unpaused**: State change notifications
- **StateSignal**: General state broadcast

## ⚙️ Configuration

### Risk Parameters (Configurable in Code)
The daily cap constant is set to 1,000,000,000,000 base units, which represents the maximum amount that can be released per 24-hour period. The max single release constant is set to 500,000,000,000 base units, which represents the maximum amount per individual transaction.


## ❗ Error Codes

- **InvalidRole**: Invalid role index (must be 0-2)
- **Unauthorized**: Signer doesn't match required role/ownership
- **NotEnoughApprovals**: Insufficient approvals for action
- **Paused**: Operation attempted while vault is frozen
- **AmountExceedsLocked**: Release amount exceeds available balance or max
- **MathOverflow**: Arithmetic overflow detected
- **InvalidAmount**: Amount must be > 0
- **DailyCapExceeded**: Daily release limit reached
- **PendingReleaseFlow**: Cannot deposit while approvals exist

## 🔐 Security Considerations

1. **PDA Ownership**: All token accounts are owned by the vault_authority PDA
2. **Role Verification**: Every privileged action verifies signer identity
3. **Reentrancy Protection**: State updates occur before external calls
4. **Overflow Safety**: All arithmetic uses checked operations
5. **Mint Validation**: Ensures all operations use the correct token mint
6. **ATA Verification**: Validates associated token account ownership

## Usage Example

To use this vault system, you would first initialize a vault by providing the public keys of all three parties and the token mint. Any of the three parties can then deposit collateral into the vault. When it's time to release funds, at least two of the three parties must approve the release by calling the approve release instruction with their respective role. Once the threshold is met, anyone can execute the release collateral instruction to transfer the tokens to the designated recipient. The custodian has special privileges to pause or unpause the vault in case of emergencies, and all three parties can participate in governance actions like rotating role holders when the approval threshold is met.

