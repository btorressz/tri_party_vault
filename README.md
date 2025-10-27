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
