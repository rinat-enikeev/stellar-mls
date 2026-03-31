## Preamble

```
SEP: <TBD>
Title: Private Group Membership on Stellar: A Zero-Knowledge Approach
Author: @rinat-enikeev
Status: Draft
Created: 2026-03-30
Updated: 2026-03-30
Version: 0.0.1
Discussion: TBD
```

## 1. Introduction

This document accompanies SEP-XXXX and provides a high-level overview of the proposal: a standard for private group membership registries on the Stellar network. The full specification defines a Soroban smart contract interface that allows groups to manage membership on-chain without ever revealing who is in the group or who is modifying it.

The core mechanism is a combination of a cryptographic commitment scheme and a zero-knowledge proof (Groth16 over BLS12-381), leveraging the pairing-friendly curve host functions introduced in Stellar Protocol 22.

---

## 2. Background

Group-based coordination is foundational to many decentralized applications — encrypted messaging, DAO governance, credential issuance, multi-party signing. All of these require some notion of "who is in this group right now," and that notion must be shared, verifiable, and resistant to forgery.

Blockchains are a natural place to anchor group state: they provide global ordering, immutability, and permissionless auditability. Stellar's Soroban smart contract platform makes this practical with low fees, fast finality, and — since Protocol 22 — native support for BLS12-381 elliptic curve operations, which are the building block for modern zero-knowledge proof verification.

The Messaging Layer Security protocol (MLS, RFC 9420) is a particularly relevant motivating case and the primary design driver for this SEP. MLS manages group encryption keys through a tree-based ratcheting structure, where each membership change (a "Commit") produces a new cryptographic group state. This state must be ordered and consistent across all members — exactly the guarantee a blockchain provides. Anchoring MLS group state on-chain creates a tamper-proof, globally-ordered log of group transitions that removes the need for a trusted central delivery service. But this only works if the on-chain record doesn't destroy the privacy that end-to-end encryption was designed to protect. The commitment-plus-ZK-proof scheme in this SEP is designed specifically to serve as that on-chain anchor without leaking the social graph that MLS keeps hidden.

---

## 3. Problem

Existing approaches to on-chain group state expose two categories of information that should remain private:

**The membership roster.** A plaintext member list (`group → [A, B, C]`) permanently publishes the social graph of every group to any network observer. Even storing a hash of the list only helps partially — if the universe of possible members is known, the hash can be brute-forced or the list can be reconstructed from observed interactions.

**The updater's identity.** Every Soroban invocation is wrapped in a transaction signed by a Stellar account. Even if the member list is hidden behind a hash, the address that submits each `update` call is visible. Over time, an observer correlates these addresses with group activity, recovering a partial social graph through traffic analysis.

These two leaks compound. Together, they make it possible to determine not just that a group exists, but who is in it, who is active, and how the group evolves — exactly the information that private communication and anonymous coordination systems exist to protect.

---

## 4. Scope

### In scope

- A Soroban contract interface for creating, updating, verifying, and deactivating private group membership registries
- A dual-hash commitment scheme (Poseidon Merkle tree + SHA-256 outer binding) that hides the member set while remaining verifiable on-chain
- A Groth16 zero-knowledge proof circuit that proves group membership without revealing the prover's identity
- BLS12-381 native identity keys with an off-chain Ed25519 attestation for Stellar address binding
- Standard circuit tiers (32, 256, 2,048 members) with shared trusted setup ceremonies
- A normative fee decoupling specification (relayer pattern) to prevent transaction-signer correlation
- Salt distribution and recovery procedures
- Compatibility with MLS (RFC 9420) group state transitions as the primary integration target
- Security analysis and test vectors

### Out of scope

- The application-layer messaging or coordination protocol (e.g., MLS implementation details, DAO voting logic)
- Key management, device-level key storage, or wallet UX
- The relayer service implementation (expected as a companion specification)
- Network-layer anonymity (Tor integration, mixnets) — this SEP addresses on-chain privacy only
- Cross-group membership correlation (a member appearing in multiple groups is not detectable from the ledger, but may be detectable at the application layer)
- Token economics, incentive design, or fee reimbursement mechanisms for relayers
- Trusted setup ceremony logistics (tooling, participant recruitment, coordination) — the SEP specifies requirements, not operational procedures

---

## 5. Solution

The SEP introduces a contract that stores, for each group, only three values: a 32-byte commitment, an epoch counter, and a circuit tier identifier. No member identity ever touches the ledger.

**Dual-hash commitment.** Member keys are organized into a Poseidon Merkle tree (a ZK-friendly hash requiring ~300 constraints per evaluation). The tree root is then bound to the epoch and a random salt via SHA-256, producing the on-chain commitment. Poseidon keeps the ZK circuit small; SHA-256 keeps on-chain verification native to Soroban.

**Zero-knowledge membership proof.** Any member can prove they belong to the committed set by presenting a Groth16 proof. The circuit verifies three things: that the prover owns a BLS12-381 private key, that the corresponding public key is a leaf in the Poseidon Merkle tree, and that the tree root hashes (with the epoch and salt) to the stored commitment. The proof is 192 bytes regardless of group size, and verification costs exactly 3 BLS12-381 pairings.

**Fee decoupling.** A relayer pattern ensures the Stellar account paying transaction fees is not the group member generating the proof. The relayer sees only an opaque proof and a group identifier — it cannot determine who submitted the update.

**Lifecycle.** Groups are created, updated through monotonically increasing epochs, and can be deactivated (with ZK-proof authorization) when no longer needed. A rolling history window bounds on-chain storage while contract events preserve the full audit trail.

---

## 6. Implementation Plan

### Phase 1 — Circuit development and testing

- Implement the Groth16 circuit for all three tiers (depth 5, 8, 11) using `circom` or `bellman`
- Validate against the SEP's test vectors
- Benchmark prover time and memory on representative client hardware (mobile, laptop)
- Publish circuit source under an open-source license

### Phase 2 — Trusted setup ceremonies

- Conduct separate MPC ceremonies for each circuit tier using the Powers of Tau format
- Recruit a minimum of 10 independent participants per ceremony
- Publish contribution hashes, attestations, and the final verification keys

### Phase 3 — Soroban contract

- Implement the contract interface (create, update, verify, deactivate, get_state, get_history)
- Integrate Groth16 verification via Protocol 22 BLS12-381 host functions
- Deploy to Stellar testnet and publish the contract address
- Gas profiling and optimization

### Phase 4 — Client SDK and relayer

- Provide a TypeScript/Rust SDK for proof generation, commitment construction, and contract interaction
- Implement a reference relayer service with a public submission endpoint
- Integrate with at least one MLS (RFC 9420) library to demonstrate the primary use case: anchoring MLS Commit state transitions on Stellar without exposing the group roster

### Phase 5 — Mainnet deployment and ecosystem adoption

- Deploy the contract and verification keys to Stellar mainnet
- Publish developer documentation and integration guides
- Coordinate with wallet and application developers for adoption

---

## Appendices

### A. Constraint budget breakdown

| Component | Constraints | Notes |
|-----------|-------------|-------|
| BLS12-381 scalar multiplication (key ownership) | ~1,000 | Native field, no emulation |
| Poseidon hash (per tree level) | ~300 | Arity 2, x⁵ S-box |
| Poseidon Merkle proof (depth 5 / 8 / 11) | ~1,500 / 2,400 / 3,300 | Logarithmic scaling |
| SHA-256 (72-byte fixed preimage) | ~25,000 | Single compression call |
| **Total (tier Small / Medium / Large)** | **~27,500 / 28,400 / 29,300** | |

SHA-256 dominates at all tiers. A future Poseidon-only variant (if Soroban adds a Poseidon host function) would reduce total constraints to ~3,000–5,000.

### B. References

- **Stellar Protocol 22** — BLS12-381 host functions for Soroban
- **RFC 9420** — The Messaging Layer Security (MLS) Protocol
- **Groth16** — Jens Groth, "On the Size of Pairing-Based Non-interactive Arguments," EUROCRYPT 2016
- **Poseidon** — Grassi et al., "Poseidon: A New Hash Function for Zero-Knowledge Proof Systems," USENIX Security 2021
- **BLS12-381** — Sean Bowe, "BLS12-381: New zk-SNARK Elliptic Curve Construction," 2017
- **ECIES** — IEEE 1363a-2004, Elliptic Curve Integrated Encryption Scheme
