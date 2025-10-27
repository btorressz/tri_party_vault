# tri_party_vault

### ***🔐 Tri-Party Vault - Solana Smart Contract

A secure multi-signature vault system for Solana that enables three-party collateral management with approval-based releases and built-in safety mechanisms.

---


## 🌐 Overview

The **Tri-Party Vault** is a Solana program designed for secure, trustless **collateral management between three roles**:

- 🛡️ **Custodian**
- 🧑‍💼 **Borrower**
- 🏦 **Lender**

This vault system enforces a **configurable multi-signature threshold** (default **2-of-3**) before allowing collateral releases. It also incorporates several **security and governance features** for production-grade usage.

---

## ✨ Key Features

### ✅ Multi-Party Approval System

- **Three Roles**: Custodian, Borrower, and Lender
- **Threshold-Based Release**: Default 2-of-3 approval requirement
- **Idempotent Approvals**: Parties can approve/revoke before finalization
- **Governance-Based Rotation**: Role holders can be updated via multisig approvals

---

### 🛡️ Safety Mechanisms

- **🔒 Daily Release Cap**: `1,000,000,000,000` base units per 24 hours
- **📦 Per-Transaction Maximum**: `500,000,000,000` base units
- **⏸️ Pause/Unpause**: Custodian can halt operations during emergencies
- **🧮 Overflow Protection**: All math operations are checked
- **🚫 Deposit Lock**: New deposits are disallowed while approvals are pending

---

### 💸 Token Support

- **SPL Token & Token-2022 Compatible**: Toggle via feature flag `token-2022`
- **ATA Integration**: Automatic Associated Token Account creation/management
- **Mint-Specific Vaults**: Each vault is bound to a single token mint

---

### 🏗️ Program Architecture
## 📛 Program ID
- 3yU4CGvB2pDQPk2ACBSjy8JBTEnnvbdLS9U1couLPmVM
- **devnet**:(https://explorer.solana.com/address/3yU4CGvB2pDQPk2ACBSjy8JBTEnnvbdLS9U1couLPmVM?cluster=devnet)

## 📦 PDA Seeds

- Vault State: ["vault", mint, custodian, borrower, lender]

- Vault Authority: ["authority", vault_state_key]


## 🧠 State Structure
- The VaultState account maintains all core state data for the tri-party collateral vault:

# Token Mint Address (mint)
- Identifies the specific SPL token this vault is associated with.

# Vault Authority Bump (vault_authority_bump)
- The bump used to derive the PDA (vault_authority) that controls vault token accounts.

  # Custodian Role (custodian)
- Public key representing the party assigned as the Custodian.

  # Borrower Role (borrower)
- Public key representing the party designated as the Borrower.

# Lender Role (lender)
- Public key representing the party serving as the Lender.

# Approvals Bitmap (approvals_bitmap)
- A bit-flag (3 bits) indicating which of the three roles have approved the next release.

# Amount Locked (amount_locked)
- Total amount of tokens currently held in the vault.

# Pause State (is_frozen)
- Boolean flag indicating whether the vault is paused (frozen) or active.

# Approval Threshold (threshold)
- The number of approvals required to authorize a token release (default: 2).

# Daily Cap Timestamp (last_cap_reset_ts)
- Unix timestamp of when the daily release limit was last reset.

# Released Today (released_today)
- Tracks the total amount of tokens released during the current 24-hour window.
