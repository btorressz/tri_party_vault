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
