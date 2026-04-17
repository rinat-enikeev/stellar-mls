# Update-Circuit Binding — Implementation Plan

**Date:** 2026-04-17
**Based on:** [update-circuit-binding-design.md](update-circuit-binding-design.md)
**Motivated by:** [vuln-unbound-new-commitment.md](vuln-unbound-new-commitment.md)
**Status:** Draft
**Version:** 1.0
**Target release:** keyset-v2 / contract v2

---

## Overview

This document turns the `UpdateCircuit` binding design into a concrete,
phase-by-phase execution plan. It enumerates every file touched, the shape of
the change, the verification step that must pass before the phase is considered
done, and the prerequisite phases each step depends on.

The core problem (full treatment in
[vuln-unbound-new-commitment.md](vuln-unbound-new-commitment.md)): the
`update_commitment` contract entry point accepts an attacker-controlled
`new_commitment: BytesN<32>` alongside a Groth16 proof, but the proof's public
inputs cover only `(C_old, epoch_old)`. Nothing cryptographically binds the
authenticated operation ("I am a current member and I authorize this epoch
transition") to the specific `new_commitment` value that gets persisted. A
malicious relayer, a Stellar mempool front-runner, or any actor between the
prover and the ledger can swap `new_commitment` and the proof will still verify.

The fix introduces a **separate `UpdateCircuit`** with four public inputs
`(C_old, epoch_old, C_new, epoch_new)` bound by an in-circuit equality
constraint `C_new = Poseidon(Poseidon(root_new, epoch_new), salt_new)`. Read
paths (`create_group`, `verify_membership`, `deactivate_group`) continue to use
the existing `MembershipCircuit` unchanged, because their statements really are
"prove membership in the current tree" — no transition.

This plan covers thirteen phases (Phase 0 through Phase 12) across the Rust
core, Soroban contract, relayer, Swift SDK, Kotlin SDK, trusted-setup
artifacts, cross-platform test vectors, documentation, and rollout.

### Design inputs

| Input | Source |
|-------|--------|
| Circuit relation `R_Update` | `update-circuit-binding-design.md` §5 |
| Public-input order and encoding | `update-circuit-binding-design.md` §5.6 |
| Storage / ABI shape | `update-circuit-binding-design.md` §6 |
| Threat model boundary | `update-circuit-binding-design.md` §8 |
| Canonical-bytes reduction risk | `vuln-unbound-new-commitment.md` §9.3 |
| Constraint budget per tier | `update-circuit-binding-design.md` Appendix A |

### Out of scope

Per the design doc, the following are **deliberately deferred** and are *not*
implementation targets in this plan:

- Nullifier / double-spend protection for the acting member.
- Cryptographic binding of the acting member to the transaction submitter.
- Proof of "acting member is a member of the new tree" (governance question).
- Multi-party co-authorization of a group update.
- Re-randomization hardening (Groth16 malleability at the pairing level).
- Unified circuit collapsing create / update / verify / deactivate.

---

## Dependency DAG

Phases are ordered by technical dependency. Parallel tracks are explicit where
they exist. Status indicators: ☐ not started, 🔧 in progress, ✅ done.

```
Phase 0 (scaffolding)
    │
Phase 1 (circuit constraints 1–4)
    │
Phase 2 (prover API)
    │
Phase 3 (FFI / JNI surface)
    │      ╲                 ╲
    │       ╲                 ╲
    ▼        ▼                 ▼
Phase 4 (contract)   Phase 6 (Swift SDK)   Phase 7 (Kotlin SDK)
    │
    ▼
Phase 5 (relayer)
    ╲         ╱          ╱
     ╲       ╱          ╱
      ▼     ▼          ▼
      Phase 8 (keyset-v2 / trusted setup)
                │
                ▼
      Phase 9 (cross-platform vectors)
                │
                ▼
      Phase 10 (documentation update)
                │
                ▼
      Phase 11 (testnet rollout + attack simulation)
                │
                ▼
      Phase 12 (mainnet rollout)
```

Critical path length: Phase 0 → 1 → 2 → 3 → 4 → 5 → 8 → 9 → 11 → 12
(ten phases serial, plus Phase 10 floating alongside 8–9).

Parallelizable: Phases 6 and 7 (Swift and Kotlin SDKs) can proceed concurrently
once Phase 3 exposes the FFI surface. Phase 4 (contract) likewise proceeds in
parallel with Phases 6 and 7.

---

## Phase 0 — Setup & scaffolding

**Goal:** Introduce the new module skeleton and type stubs so that subsequent
phases add constraints rather than rewiring modules. This phase must leave the
build green but produce no behavioural change.

**Status:** ☐

### Files

| File | Action |
|------|--------|
| `src/circuit/mod.rs` | Add `pub mod update;` declaration |
| `src/circuit/update.rs` | **NEW** — empty module skeleton |
| `src/circuit/update/tests.rs` | **NEW** — test scaffolding |
| `Cargo.toml` | No change (arkworks deps already present) |

### Changes

1. Create `src/circuit/update.rs` with the bare scaffolding:

```rust
//! UpdateCircuit — Groth16 circuit that binds a membership proof at the old
//! epoch to the specific C_new / epoch_new being authorized on-chain.
//!
//! See docs/update-circuit-binding-design.md §5 for the formal relation.

use ark_bls12_381::Fr;
use ark_r1cs_std::alloc::AllocationMode;
use ark_r1cs_std::fields::fp::FpVar;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use crate::circuit::MerkleAuthPath;

pub struct UpdateCircuit {
    // Public inputs (allocated in fixed order — see §5.6 of the design doc).
    pub c_old: Option<Fr>,
    pub epoch_old: Option<Fr>,
    pub c_new: Option<Fr>,
    pub epoch_new: Option<Fr>,

    // Witnesses.
    pub sk: Option<Fr>,
    pub salt_old: Option<Fr>,
    pub root_new: Option<Fr>,
    pub salt_new: Option<Fr>,
    pub auth_path: Option<MerkleAuthPath>,
    pub tree_depth: usize,
}

impl ConstraintSynthesizer<Fr> for UpdateCircuit {
    fn generate_constraints(
        self,
        _cs: ConstraintSystemRef<Fr>,
    ) -> Result<(), SynthesisError> {
        // Populated in Phase 1.
        Ok(())
    }
}
```

2. Extend `src/circuit/mod.rs`:

```rust
pub mod update;
```

3. Seed the test file with a compile-only smoke test:

```rust
#[test]
fn update_circuit_compiles_with_no_constraints() {
    use ark_relations::r1cs::ConstraintSystem;
    let cs = ConstraintSystem::<ark_bls12_381::Fr>::new_ref();
    let circuit = super::UpdateCircuit {
        c_old: None, epoch_old: None, c_new: None, epoch_new: None,
        sk: None, salt_old: None, root_new: None, salt_new: None,
        auth_path: None, tree_depth: 5,
    };
    <super::UpdateCircuit as ark_relations::r1cs::ConstraintSynthesizer<_>>::generate_constraints(circuit, cs.clone()).unwrap();
    assert_eq!(cs.num_constraints(), 0);
}
```

4. Stub a module-level `prove_update` that returns `Err("not implemented")` to
   reserve the symbol that Phase 2 will fill in:

```rust
pub fn prove_update(_: ()) -> Result<Vec<u8>, &'static str> {
    Err("prove_update: not implemented (Phase 2)")
}
```

### Verification

- `cargo build --lib` succeeds.
- `cargo test --lib circuit::update::tests::update_circuit_compiles_with_no_constraints` passes.
- `cargo test --lib` returns same pre-existing count plus 1.
- `git grep -n "UpdateCircuit"` matches only `src/circuit/update.rs` and its test module.

### Dependencies

None — Phase 0 is the root of the DAG.

---

## Phase 1 — Circuit constraints (1)–(4)

**Goal:** Implement the four R1CS constraints that define `R_Update`. This
phase makes the circuit semantically correct. No prover API yet; tests exercise
the circuit via the raw `ConstraintSynthesizer` interface.

**Status:** ☐

### Files

| File | Action |
|------|--------|
| `src/circuit/update.rs` | Populate `generate_constraints` |
| `src/circuit/update/tests.rs` | Add positive / negative / order tests |
| `src/circuit/mod.rs` | Re-export `poseidon_hash_two_gadget` (if not already `pub(crate)`) |

### Changes

1. **Public input allocation (pinned order).** Inside `generate_constraints`,
   allocate public inputs in the exact order `(c_old, epoch_old, c_new,
   epoch_new)`. Allocation order IS the serialisation order — no sorting later
   will fix a mistake here, because the Groth16 IC vector indexes by allocation
   order:

```rust
let c_old_var      = FpVar::<Fr>::new_input(cs.clone(), || {
    self.c_old.ok_or(SynthesisError::AssignmentMissing)
})?;
let epoch_old_var  = FpVar::<Fr>::new_input(cs.clone(), || {
    self.epoch_old.ok_or(SynthesisError::AssignmentMissing)
})?;
let c_new_var      = FpVar::<Fr>::new_input(cs.clone(), || {
    self.c_new.ok_or(SynthesisError::AssignmentMissing)
})?;
let epoch_new_var  = FpVar::<Fr>::new_input(cs.clone(), || {
    self.epoch_new.ok_or(SynthesisError::AssignmentMissing)
})?;
```

2. **Constraint (1): Membership leaf.** Allocate `sk` as witness, compute
   `leaf = Poseidon(sk)`, fold the Merkle auth path to obtain `root_old_var`.
   Port verbatim from `MembershipCircuit` — no structural changes.

3. **Constraint (2): Old commitment.** Allocate `salt_old` as witness, compute
   `c_old_derived = Poseidon(Poseidon(root_old_var, epoch_old_var), salt_old)`,
   enforce `c_old_derived.enforce_equal(&c_old_var)`. This binds the proof to
   the specific old epoch state.

4. **Constraint (3): Epoch monotonicity.** Enforce `epoch_new_var = epoch_old_var + 1`
   using `FpVar`-level arithmetic. This prevents the relayer from jumping
   epochs forward or rewinding history, which in turn prevents history-forking
   attacks described in `vuln-unbound-new-commitment.md` §6.4.

5. **Constraint (4): New commitment binding — the core fix.** Allocate
   `root_new` and `salt_new` as witnesses, compute
   `c_new_derived = Poseidon(Poseidon(root_new_var, epoch_new_var), salt_new_var)`,
   enforce `c_new_derived.enforce_equal(&c_new_var)`. This is the constraint
   whose absence produced the vulnerability.

6. **Cross-reference the design doc in a comment** at the top of each
   constraint block: `// See update-circuit-binding-design.md §5.2, constraint (N).`
   Keep comments minimal — the WHY is in the design doc, not inline.

### Tests

Add to `src/circuit/update/tests.rs`:

```rust
#[test]
fn update_circuit_accepts_valid_transition() { /* ... */ }

#[test]
fn update_circuit_rejects_wrong_c_new() { /* swap salt_new but not c_new */ }

#[test]
fn update_circuit_rejects_non_monotonic_epoch() { /* epoch_new = epoch_old + 2 */ }

#[test]
fn update_circuit_rejects_rewound_epoch() { /* epoch_new = epoch_old */ }

#[test]
fn update_circuit_rejects_wrong_auth_path() { /* leaf not in root_old */ }

#[test]
fn update_circuit_public_input_order_pinned() {
    // Build the circuit, run `ConstraintSystem::num_instance_variables`,
    // assert it equals 1 (constant) + 4 (our public inputs).
    // Then extract allocated values in instance-index order and assert
    // they equal (c_old, epoch_old, c_new, epoch_new).
}
```

### Verification

- `cargo test --lib circuit::update` — all tests pass.
- `cargo test --lib circuit` — existing `MembershipCircuit` tests unaffected.
- Constraint count landed within design doc estimates per tier (Appendix A:
  depth-5 ~9,000; depth-8 ~14,400; depth-11 ~19,800).

### Dependencies

Phase 0.

---

## Phase 2 — Prover API

**Goal:** Expose a Rust-level API that takes high-level inputs (secret key,
member list, old epoch, new member list, salts) and returns a serialised
Groth16 proof plus the four public-input scalars. This phase makes the circuit
usable from application code without reaching into arkworks primitives.

**Status:** ☐

### Files

| File | Action |
|------|--------|
| `src/prover/mod.rs` | Add `pub mod update;` |
| `src/prover/update.rs` | **NEW** — high-level API |
| `src/prover/update/tests.rs` | **NEW** — round-trip tests |
| `src/public_inputs.rs` | Add `UpdatePublicInputs` struct |

### Changes

1. Define `UpdatePublicInputs`:

```rust
pub struct UpdatePublicInputs {
    pub c_old: [u8; 32],      // big-endian canonical Fr
    pub epoch_old: u64,
    pub c_new: [u8; 32],      // big-endian canonical Fr
    pub epoch_new: u64,
}

impl UpdatePublicInputs {
    pub const VERSION: u8 = 2;
    pub fn serialize(&self) -> Vec<u8> {
        // 1 byte version || 32 bytes c_old || 8 bytes epoch_old (BE)
        // || 32 bytes c_new || 8 bytes epoch_new (BE)
    }
    pub fn deserialize(bytes: &[u8]) -> Result<Self, DeserializeError> { ... }
    pub fn to_scalars(&self) -> [Fr; 4] { ... }  // in allocation order
}
```

2. Define prover input type:

```rust
pub struct UpdateProverInput<'a> {
    pub tier: Tier,
    pub sk: &'a [u8; 32],
    pub members_old: &'a [[u8; 48]],   // canonical member order (BLS G1 compressed)
    pub epoch_old: u64,
    pub salt_old: &'a [u8; 32],
    pub members_new: &'a [[u8; 48]],
    pub salt_new: &'a [u8; 32],
    pub proving_key: &'a Groth16ProvingKey,
}
```

3. Implement `prove_update`:

```rust
pub fn prove_update(input: UpdateProverInput<'_>) -> Result<(Vec<u8>, UpdatePublicInputs), ProveError> {
    // 1. Canonicalize member_old -> Poseidon leaves -> tree -> root_old.
    // 2. Locate sk's leaf index; build auth path.
    // 3. Canonicalize member_new -> tree -> root_new.
    // 4. Derive c_old = Poseidon(Poseidon(root_old, epoch_old), salt_old).
    // 5. epoch_new = epoch_old + 1; derive c_new.
    // 6. Instantiate UpdateCircuit; Groth16::prove.
    // 7. Serialise proof (G1 || G2 || G1 compressed) and inputs.
}
```

4. Add a convenience wrapper `prove_update_from_members` that wraps the
   canonical-ordering + Poseidon hashing steps so callers don't reimplement them.

### Tests

```rust
#[test]
fn prove_update_round_trip() {
    // Generate a small tier with 3 members, prove, verify locally with
    // Groth16::verify against the same VK. Must succeed.
}

#[test]
fn prove_update_rejects_stale_epoch() {
    // Build members_old at epoch 5; call prove_update; assert epoch_new == 6.
}

#[test]
fn prove_update_canonical_member_order() {
    // Present members_new in non-canonical order; prove; decode c_new;
    // assert it matches the canonical-order commitment.
}

#[test]
fn prove_update_wrong_sk_fails() {
    // sk not in members_old → prove should fail with MembershipProof error.
}

#[test]
fn update_public_inputs_serde_round_trip() {
    // serialize -> deserialize -> same bytes; leading version byte == 2.
}
```

### Verification

- `cargo test --lib prover::update` — all pass.
- `cargo test --lib public_inputs` — `UpdatePublicInputs` serde tests pass.
- `UpdatePublicInputs::serialize` output length is exactly `1 + 32 + 8 + 32 + 8 = 81` bytes.

### Dependencies

Phase 1.

---

## Phase 3 — FFI / JNI surface

**Goal:** Expose `prove_update` to Swift (via the C FFI) and Kotlin (via JNI).
Add `PUBLIC_INPUTS_VERSION = 2` byte to the wire format so platforms can detect
version skew loudly instead of misparsing a legacy payload.

**Status:** ☐

### Files

| File | Action |
|------|--------|
| `src/ffi/mod.rs` | Add `pub mod update;` |
| `src/ffi/update.rs` | **NEW** — `sep_prove_update` C entry point |
| `src/jni/mod.rs` | Add `pub mod update;` |
| `src/jni/update.rs` | **NEW** — `Java_com_stellarmls_native_ProverJNI_sepProveUpdate` |
| `src/ffi/error.rs` | Add `ErrorCode::UpdateProof*` variants |
| `include/stellar_mls.h` | Add `sep_prove_update` prototype, bump header version |

### Changes

1. C FFI signature:

```c
int32_t sep_prove_update(
    uint8_t tier,                    // 0 = small, 1 = medium, 2 = large
    const uint8_t* sk,               // 32 bytes
    const uint8_t* members_old,      // 48 * n_old bytes
    uint32_t n_old,
    uint64_t epoch_old,
    const uint8_t* salt_old,         // 32 bytes
    const uint8_t* members_new,      // 48 * n_new bytes
    uint32_t n_new,
    const uint8_t* salt_new,         // 32 bytes
    const uint8_t* proving_key_bytes,
    size_t proving_key_len,
    uint8_t* out_proof,              // caller-allocated, size from sep_proof_size()
    size_t out_proof_cap,
    size_t* out_proof_len,
    uint8_t* out_public_inputs,      // caller-allocated, size 81
    size_t out_public_inputs_cap,
    size_t* out_public_inputs_len
);
```

2. JNI signature mirrors the C FFI but returns a Kotlin data class
   `UpdateProofBundle(proof: ByteArray, publicInputs: ByteArray)`.

3. **Wire-format version byte.** The first byte of `out_public_inputs` must be
   `0x02`. Callers are required to check this byte and fail loudly on anything
   other than `0x02`. Rationale: prevents a legacy (v1, 40-byte `commitment ||
   epoch`) payload from being silently parsed as the leading half of a v2
   payload — a confused-deputy risk at the FFI boundary.

4. Update `include/stellar_mls.h` header with:

```c
#define STELLAR_MLS_PUBLIC_INPUTS_VERSION 2
```

### Tests

- `tests/ffi/update_round_trip.rs` — C-level round-trip via `libloading`.
- `tests/jni/update_round_trip.rs` — JNI round-trip via `j4rs`.
- `tests/ffi/update_version_mismatch.rs` — pass a v1-shaped buffer; assert
  error code `UpdateProofVersionMismatch`.
- `tests/ffi/update_buffer_too_small.rs` — pass `out_public_inputs_cap = 80`;
  assert error code `UpdateProofBufferTooSmall`.

### Verification

- `cargo test --lib ffi::update` passes.
- `cargo test --lib jni::update` passes.
- Header file diffs confined to additive changes (no symbol rename).

### Dependencies

Phase 2.

---

## Phase 4 — Contract

**Goal:** Extend the Soroban contract to store an `UpdateVK` per tier, verify
the 4-input update proof on `update_commitment`, enforce canonical bytes on
`new_commitment`, and reject the prior 2-input ABI shape.

**Status:** ☐

### Files

| File | Action |
|------|--------|
| `contracts/sep-xxxx/src/lib.rs` | Major — add types, storage keys, entry points, verification |
| `contracts/sep-xxxx/src/errors.rs` | Add `InvalidCommitmentEncoding`, `UnknownVkKind` |
| `contracts/sep-xxxx/src/test.rs` | Add attack-simulation test |
| `contracts/sep-xxxx/src/testutils.rs` | Add `UpdatePublicInputs` constructor helpers |

### Changes

1. **New types** (§6 of the design doc):

```rust
#[contracttype]
#[derive(Clone, Debug)]
pub struct UpdatePublicInputs {
    pub c_old: BytesN<32>,
    pub epoch_old: u64,
    pub c_new: BytesN<32>,
    pub epoch_new: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VkKind {
    Membership,
    Update,
}
```

2. **New storage keys**:

```rust
#[contracttype]
pub enum DataKey {
    // ... existing variants ...
    UpdateVK(Tier),
}
```

3. **Extended `initialize`.** The contract is alpha; `initialize` is safe to
   widen. It now takes six VKs (three tiers × two kinds):

```rust
pub fn initialize(
    env: Env,
    admin: Address,
    membership_vk_small: BytesN<VK_LEN>,
    membership_vk_medium: BytesN<VK_LEN>,
    membership_vk_large: BytesN<VK_LEN>,
    update_vk_small: BytesN<VK_LEN>,
    update_vk_medium: BytesN<VK_LEN>,
    update_vk_large: BytesN<VK_LEN>,
);
```

4. **`update_vk(kind, tier, vk)`** — replaces the hardcoded rotation path:

```rust
pub fn update_vk(env: Env, kind: VkKind, tier: Tier, new_vk: BytesN<VK_LEN>) {
    Self::require_admin(&env);
    let key = match kind {
        VkKind::Membership => DataKey::MembershipVK(tier),
        VkKind::Update     => DataKey::UpdateVK(tier),
    };
    env.storage().persistent().set(&key, &new_vk);
}
```

5. **`verify_groth16_proof_update`** — mirrors
   `verify_groth16_proof` (`contracts/sep-xxxx/src/lib.rs:780-823`) but MSM
   over 4 IC points rather than 3:

```rust
fn verify_groth16_proof_update(
    env: &Env,
    tier: Tier,
    proof: &Groth16Proof,
    pi: &UpdatePublicInputs,
) -> Result<bool, Error> {
    // 1. Canonical-bytes check on c_old and c_new
    if !is_canonical_fr_bytes(&pi.c_old)  { return Err(Error::InvalidCommitmentEncoding); }
    if !is_canonical_fr_bytes(&pi.c_new)  { return Err(Error::InvalidCommitmentEncoding); }
    // 2. Load UpdateVK(tier)
    // 3. Build scalars = [c_old_fr, epoch_old_fr, c_new_fr, epoch_new_fr]
    // 4. vk_x = IC[0] + MSM([IC[1..=4]], scalars)
    // 5. Pairing: e(A, B) == e(alpha, beta) * e(vk_x, gamma) * e(C, delta)
    ...
}
```

6. **Rewrite `update_commitment`**:

```rust
pub fn update_commitment(
    env: Env,
    group_id: BytesN<32>,
    proof: Groth16Proof,
    public_inputs: UpdatePublicInputs,
) -> Result<(), Error> {
    let state = Self::require_active_group(&env, &group_id)?;

    // Canonical-bytes guard on the stored value.
    if !is_canonical_fr_bytes(&public_inputs.c_new) {
        return Err(Error::InvalidCommitmentEncoding);
    }

    // Bind to stored state.
    if state.commitment != public_inputs.c_old { return Err(Error::CommitmentMismatch); }
    if state.epoch != public_inputs.epoch_old  { return Err(Error::EpochMismatch); }

    // Monotonicity (also enforced in-circuit — defense in depth at the boundary).
    if public_inputs.epoch_new != public_inputs.epoch_old + 1 {
        return Err(Error::EpochNotMonotonic);
    }

    // Proof verification.
    if !Self::verify_groth16_proof_update(&env, state.tier, &proof, &public_inputs)? {
        return Err(Error::InvalidProof);
    }

    // Replay protection (unchanged from prior impl).
    Self::mark_proof_consumed(&env, &proof);

    // Write new state.
    let new_state = GroupState {
        commitment: public_inputs.c_new.clone(),
        epoch: public_inputs.epoch_new,
        ..state
    };
    env.storage().persistent().set(&DataKey::Group(group_id), &new_state);
    Ok(())
}
```

7. **Delete the old signature.** The prior parameters `new_commitment:
   BytesN<32>` and `new_epoch: u64` are removed. `public_inputs` is now the
   authoritative carrier for both.

8. **Errors.** Add variants:

```rust
InvalidCommitmentEncoding = 17,
UnknownVkKind             = 18,
```

### Tests

Add to `contracts/sep-xxxx/src/test.rs`:

```rust
#[test]
fn test_update_commitment_accepts_valid_proof() { /* baseline */ }

#[test]
fn test_update_commitment_rejects_rebinding() {
    // 1. Prover generates valid (proof, public_inputs) for c_new = X.
    // 2. Attacker swaps public_inputs.c_new -> Y, keeps proof.
    // 3. Contract::update_commitment must return Err(InvalidProof).
}

#[test]
fn test_update_commitment_rejects_non_canonical_new_commitment() {
    // Set c_new to r (the BLS12-381 scalar field modulus) encoded big-endian.
    // Fr::from_bytes_mod_order would silently reduce to 0.
    // Contract must return Err(InvalidCommitmentEncoding) before touching the VK.
}

#[test]
fn test_update_commitment_rejects_epoch_jump() {
    // epoch_old = 5, epoch_new = 7. Must fail monotonicity.
}

#[test]
fn test_update_commitment_rejects_rewound_epoch() {
    // epoch_new == epoch_old. Must fail monotonicity.
}

#[test]
fn test_vk_length_mismatch_update() {
    // initialize with a truncated update_vk_small; any update_commitment call
    // must return Err(InvalidVkLength).
}

#[test]
fn test_update_vk_routing() {
    // update_vk(Membership, Small, VK_A)  then  update_vk(Update, Small, VK_B)
    // must store in two distinct slots; neither overwrites the other.
}

#[test]
fn test_create_group_still_uses_membership_vk() {
    // Regression — create_group path unaffected by update-circuit work.
}

#[test]
fn test_deactivate_group_still_uses_membership_vk() {
    // Regression.
}
```

### Verification

- `stellar contract build --manifest-path contracts/sep-xxxx/Cargo.toml` succeeds.
- `cargo test --manifest-path contracts/sep-xxxx/Cargo.toml` — all pass.
- Contract wasm size delta < +10% vs. pre-change (MSM over 4 scalars vs. 3
  adds a few hundred bytes).

### Dependencies

Phase 3 (needs final `UpdatePublicInputs` serialisation shape to pin
`UpdatePublicInputs` contract type to match).

---

## Phase 5 — Relayer

**Goal:** Forward the new public-inputs shape to the contract. Stop treating
`new_commitment` as a top-level HTTP request field.

**Status:** ☐

### Files

| File | Action |
|------|--------|
| `relayer/src/handler.rs` | Update the `update_commitment` branch |
| `relayer/src/types.rs` | New `UpdatePublicInputs` request/response types |
| `relayer/src/tests.rs` | Branch-level tests with stub contract |

### Changes

1. **Remove top-level `newCommitment` field.** At
   `relayer/src/handler.rs:229-235`, the HTTP request for `update_commitment`
   currently reads `new_commitment` as a top-level JSON field. Drop it. The
   authoritative source is now `public_inputs.c_new`.

2. **Split `add_public_inputs_arg`** (currently at
   `relayer/src/handler.rs:346-375`) into two variants:

```rust
fn add_public_inputs_arg_membership(
    args: &mut Vec<ScVal>,
    inputs: &MembershipPublicInputs,
) { ... }

fn add_public_inputs_arg_update(
    args: &mut Vec<ScVal>,
    inputs: &UpdatePublicInputs,
) { ... }
```

   The existing unified helper has been a minor source of type confusion
   already; separating by op is safer and makes each call site's expectation
   explicit at the type level.

3. **Branch update.** The `update_commitment` branch now reads a single
   `UpdatePublicInputs` blob from the request body, no longer merges a separate
   `new_commitment` into the Soroban argument vector, and uses
   `add_public_inputs_arg_update`.

4. **Version gate.** If the incoming `public_inputs` buffer has a leading byte
   other than `0x02`, the relayer rejects with HTTP 400 `wrong_public_inputs_version`.

### Tests

- `tests/update_commitment_happy_path.rs` — end-to-end with a stubbed contract
  transport.
- `tests/update_commitment_rejects_v1_payload.rs` — request body shaped as
  legacy v1 (containing `newCommitment` top-level) returns 400.
- `tests/update_commitment_rejects_wrong_version_byte.rs` — leading byte `0x01`
  returns 400.
- `tests/update_commitment_forwards_canonical_payload.rs` — asserts the exact
  ScVal vector sent to Soroban matches the contract's expected argument shape.

### Verification

- `cargo test --manifest-path relayer/Cargo.toml` passes.
- A `curl`-based smoke test against the local relayer succeeds for v2 payloads
  and fails with 400 for v1.

### Dependencies

Phase 4 (needs the finalised contract ABI shape).

---

## Phase 6 — Swift SDK

**Goal:** Mirror the new `UpdatePublicInputs` type on iOS/macOS, remove the
top-level `newCommitment` from `SEPUpdateCommitmentRequest`, and thread the
version byte end-to-end.

**Status:** ☐

### Files

| File | Action |
|------|--------|
| `swift-mls/Sources/SwiftMLS/PublicInputs.swift` | Add `UpdatePublicInputs` |
| `swift-mls/Sources/SwiftMLS/UpdateCommitment.swift` | **NEW** — high-level API |
| `swift-mls/Sources/SwiftMLS/SEPUpdateCommitmentRequest.swift` | Drop `newCommitment`; add `publicInputs` + `publicInputsVersion: UInt8` |
| `swift-mls/Sources/SwiftMLS/Prover.swift` | Add `proveUpdate(...) -> UpdateProofBundle` |
| `swift-mls/Tests/SwiftMLSTests/UpdateCircuitTests.swift` | **NEW** |
| `clients/ios/StellarChat/StellarChatTests/CrossPlatformVectorTests.swift` | Update vector consumers |
| `clients/ios/StellarChat/StellarChat/GroupMutation.swift` | Call `proveUpdate` instead of `proveMembership` on the update path |

### Changes

1. **`UpdatePublicInputs` Swift mirror:**

```swift
public struct UpdatePublicInputs: Codable, Equatable, Sendable {
    public static let version: UInt8 = 2
    public let cOld: Data           // 32 bytes
    public let epochOld: UInt64
    public let cNew: Data           // 32 bytes
    public let epochNew: UInt64

    public func serialize() -> Data {
        var out = Data()
        out.append(Self.version)
        out.append(cOld)
        var eo = epochOld.bigEndian;  withUnsafeBytes(of: &eo)  { out.append(contentsOf: $0) }
        out.append(cNew)
        var en = epochNew.bigEndian;  withUnsafeBytes(of: &en)  { out.append(contentsOf: $0) }
        return out
    }
}
```

2. **Drop `newCommitment` from `SEPUpdateCommitmentRequest`.** Callers now pass
   the full `publicInputs` blob; `cNew` lives inside it.

3. **High-level `proveUpdate`:**

```swift
public static func proveUpdate(
    tier: Tier,
    sk: Data,
    membersOld: [Data],
    epochOld: UInt64,
    saltOld: Data,
    membersNew: [Data],
    saltNew: Data
) throws -> (proof: Data, publicInputs: UpdatePublicInputs)
```

4. **FFI call site.** Invokes `sep_prove_update` (Phase 3 symbol).

### Tests

- `UpdateCircuitTests.testRoundTrip` — prove + locally-verify round trip.
- `UpdateCircuitTests.testPublicInputsVersionByte` — serialised buffer starts
  with `0x02`.
- `UpdateCircuitTests.testRejectLegacyV1Buffer` — attempt to deserialise a 40-byte
  v1 buffer; assert it throws.
- `CrossPlatformVectorTests.testUpdateVectors` — consume the new fixtures
  produced in Phase 9.

### Verification

- `swift test` passes across `swift-mls` and `clients/ios/StellarChat`.
- iOS simulator smoke test: create → update → verify works end-to-end against
  a locally running relayer.

### Dependencies

Phase 3 (FFI surface), Phase 4 (finalised contract ABI shape for the HTTP
request wire format consumed downstream).

---

## Phase 7 — Kotlin SDK

**Goal:** Android mirror of Phase 6. Kept as a separate phase to allow
independent review and to reflect different JNI integration surface.

**Status:** ☐

### Files

| File | Action |
|------|--------|
| `kotlin-mls/src/main/kotlin/com/stellarmls/mls/PublicInputs.kt` | Add `UpdatePublicInputs` |
| `kotlin-mls/src/main/kotlin/com/stellarmls/mls/UpdateCommitment.kt` | **NEW** |
| `kotlin-mls/src/main/kotlin/com/stellarmls/mls/SEPUpdateCommitmentRequest.kt` | Drop `newCommitment`; add `publicInputs: ByteArray` |
| `kotlin-mls/src/main/kotlin/com/stellarmls/mls/Prover.kt` | Add `proveUpdate` |
| `kotlin-mls/src/test/kotlin/com/stellarmls/mls/UpdateCircuitTest.kt` | **NEW** |
| `clients/android/.../CrossPlatformVectorTest.kt` | Update vector consumers |
| `clients/android/.../GroupMutation.kt` | Call `proveUpdate` on update path |

### Changes

1. `UpdatePublicInputs` Kotlin mirror:

```kotlin
data class UpdatePublicInputs(
    val cOld: ByteArray,     // 32 bytes
    val epochOld: Long,      // u64 — callers must treat as unsigned
    val cNew: ByteArray,     // 32 bytes
    val epochNew: Long,
) {
    companion object {
        const val VERSION: Byte = 0x02
    }

    fun serialize(): ByteArray {
        val buf = ByteBuffer.allocate(1 + 32 + 8 + 32 + 8).order(ByteOrder.BIG_ENDIAN)
        buf.put(VERSION)
        buf.put(cOld)
        buf.putLong(epochOld)
        buf.put(cNew)
        buf.putLong(epochNew)
        return buf.array()
    }
}
```

2. `proveUpdate` entry invokes the JNI symbol `sepProveUpdate` from Phase 3.

3. Remove `newCommitment` field from `SEPUpdateCommitmentRequest`. The Retrofit
   service now sends `publicInputs: ByteArray` base64-encoded in the JSON body.

### Tests

- Equivalent set to the Swift tests (`testRoundTrip`, `testPublicInputsVersionByte`,
  `testRejectLegacyV1Buffer`, `testUpdateVectors`).
- Uses `j4rs` or direct JNI loading for local native tests.

### Verification

- `./gradlew test` in `kotlin-mls` and `clients/android` both pass.
- Android emulator smoke test: create → update → verify works end-to-end.

### Dependencies

Phase 3 (JNI surface), Phase 4 (contract ABI).

---

## Phase 8 — Trusted setup / keyset-v2

**Goal:** Produce Groth16 proving and verifying keys for *both* circuits
(`MembershipCircuit` re-run at unchanged relation, `UpdateCircuit` fresh) for
each of the three tiers. Publish as `keyset-v2/` artifacts with
`schemaVersion: 2`.

**Status:** ☐

**Warning: this is the single highest-risk phase operationally.** A leaked
toxic-waste value from the Phase 2 ceremony lets an attacker forge proofs. Do
not shortcut.

### Files

| File | Action |
|------|--------|
| `scripts/generate-keyset.sh` | Parameterise over circuit kind |
| `scripts/generate-keyset-v2.sh` | **NEW** — orchestrates Phase 2 ceremony for both circuits |
| `scripts/generate-mainnet-vks.sh` | Update to publish six VKs (was three) |
| `src/ceremony/phase2.rs` | Support running Phase 2 over `UpdateCircuit` |
| `src/ceremony/mpc.rs` | No structural change — arithmetic already generic |
| `keyset-v2/metadata.json` | **NEW** |
| `keyset-v2/membership-small/{pk,vk}.bin` | **NEW** |
| `keyset-v2/membership-medium/{pk,vk}.bin` | **NEW** |
| `keyset-v2/membership-large/{pk,vk}.bin` | **NEW** |
| `keyset-v2/update-small/{pk,vk}.bin` | **NEW** |
| `keyset-v2/update-medium/{pk,vk}.bin` | **NEW** |
| `keyset-v2/update-large/{pk,vk}.bin` | **NEW** |
| `docs/trusted-setup-ceremony-phase2-participant-playbook.md` | Add update-circuit instructions |

### Changes

1. **Ceremony strategy.** Phase 1 (powers-of-tau, universal) is reusable
   verbatim from the existing keyset-v1 contributions — no re-run needed.
   Phase 2 (circuit-specific) must be re-run for each of the six
   (circuit, tier) pairs. Contributions can be parallelised per pair.

2. **`keyset-v2/metadata.json` shape:**

```json
{
  "schemaVersion": 2,
  "contractVersion": "2.0.0",
  "circuits": {
    "membership": {
      "small":  { "pkHash": "...", "vkHash": "...", "constraints": 5000 },
      "medium": { "pkHash": "...", "vkHash": "...", "constraints": 8000 },
      "large":  { "pkHash": "...", "vkHash": "...", "constraints": 11000 }
    },
    "update": {
      "small":  { "pkHash": "...", "vkHash": "...", "constraints": 9000 },
      "medium": { "pkHash": "...", "vkHash": "...", "constraints": 14400 },
      "large":  { "pkHash": "...", "vkHash": "...", "constraints": 19800 }
    }
  },
  "phase1Source": "keyset-v1/phase1/",
  "phase2Ceremony": {
    "date": "2026-04-??",
    "coordinator": "...",
    "participants": [ "..." ],
    "transcriptHash": "..."
  }
}
```

3. **Pinned public-input order unit test.** Add
   `src/ceremony/tests.rs::test_update_vk_public_input_order_pinned` — fails
   loudly if the VK's IC vector length is not exactly 5 (constant + 4 inputs).
   This catches a class of silent ordering footgun where a developer swaps the
   order of `FpVar::new_input` calls and everything still compiles.

### Verification

- All six `pk.bin` / `vk.bin` pairs round-trip prove/verify for test vectors.
- `shasum -a 256 keyset-v2/**/*.bin` matches the hashes in `metadata.json`.
- Constraint counts per tier match design doc Appendix A within ±5%.
- `test_update_vk_public_input_order_pinned` passes.
- Transcript hash can be independently recomputed from the MPC logs.

### Dependencies

Phases 4, 5, 6, 7 (final circuit shape must be frozen before the ceremony — a
late constraint change invalidates every contribution).

---

## Phase 9 — Cross-platform vectors

**Goal:** Regenerate the shared test fixture so iOS, Android, and contract
tests all agree on the exact byte-level encoding of `UpdatePublicInputs`.

**Status:** ☐

### Files

| File | Action |
|------|--------|
| `docs/cross-platform-test-vectors.json` | Bump `schemaVersion` to 2; add `update` vectors |
| `scripts/generate-test-vectors.sh` | Emit both membership and update fixtures |
| `src/testvectors/generate.rs` | Add update-circuit vector generator |

### Changes

1. **Vector structure** (additive):

```json
{
  "schemaVersion": 2,
  "membership": [ ... ],         // unchanged shape
  "update": [
    {
      "name": "small_tier_3_to_4_members",
      "tier": "small",
      "sk": "hex",
      "membersOld": ["hex", ...],
      "epochOld": 0,
      "saltOld": "hex",
      "membersNew": ["hex", ...],
      "saltNew": "hex",
      "publicInputs": "81-byte hex",     // version || cOld || epochOld || cNew || epochNew
      "proof": "192-byte hex",
      "expectedResult": "valid"
    },
    { "name": "rebinding_attack_1", "expectedResult": "InvalidProof", ... },
    { "name": "non_canonical_c_new", "expectedResult": "InvalidCommitmentEncoding", ... },
    { "name": "epoch_jump", "expectedResult": "EpochNotMonotonic", ... }
  ]
}
```

2. At least one positive and three negative vectors per tier (×3 tiers = 12
   update vectors total).

### Verification

- Each vector round-trips through:
  - Rust `verify_update_proof` — expected result matches.
  - Swift `UpdatePublicInputs` + Prover — expected result matches.
  - Kotlin `UpdatePublicInputs` + Prover — expected result matches.
  - Soroban contract `update_commitment` — expected result matches.
- All four consumers agree on every vector.

### Dependencies

Phase 8 (needs the keyset-v2 PK/VKs to generate real proofs).

---

## Phase 10 — Documentation update

**Goal:** Bring the normative spec, soundness proof, architecture overview,
and relayer design doc into alignment with the new circuit. This phase is not
strictly a blocker for rollout but must precede public announcement.

**Status:** ☐

### Files

| File | Section to update |
|------|-------------------|
| `docs/sep.md` | Add `UpdateCircuit` relation; update `update_commitment` ABI section; bump SEP-XXXX version |
| `docs/proof-of-soundness.md` | New section proving knowledge soundness for the **operation-level** statement `(C_old, e_old, C_new, e_new)`; retire the prior §257-259 theorem that proved soundness for the wrong statement |
| `docs/design-doc.md` | Refresh architecture diagram to show two VKs per tier |
| `docs/relay-design-doc.md` | Document `publicInputs` wire format v2; drop top-level `newCommitment` from the request schema diagram |
| `docs/real-world-gap-analysis.md` | Tick off the newly-closed gap; add the lesson about "statement ≠ operation" |
| `docs/audit-critical.md` | Add cross-reference to the new vuln report |
| `README.md` | No change required if kept tier-level |

### Changes

1. **`docs/sep.md`.** Add a new subsection under the ZK-circuits chapter
   describing `R_Update`, its public inputs (fixed order), and the constraint
   graph. Update the ABI section to show the new `update_commitment` signature.

2. **`docs/proof-of-soundness.md`.** The existing theorem at lines 257-259
   proves knowledge soundness for `(C_old, e_old)`, which is insufficient. Add
   Theorem 4 (or whatever the next ordinal is) stating:

   > **Theorem (Operation-level soundness for update).** For all PPT adversaries
   > $\mathcal{A}$ producing a tuple $(\pi, \text{pi})$ accepted by the
   > Soroban verifier for `update_commitment`, there exists an extractor
   > $\mathcal{E}$ producing a witness $(\text{sk}, \text{salt}_{old},
   > \text{auth\_path}, \text{root}_{new}, \text{salt}_{new})$ satisfying
   > constraints (1)–(4) of `R_Update` over the public inputs
   > $(C_{old}, e_{old}, C_{new}, e_{new})$ *as bound to storage by the
   > `update_commitment` entry point*.

   Mark the prior theorem as "correct statement about the wrong relation;
   superseded" rather than deleting it, since the audit trail matters.

### Verification

- `markdown-link-check docs/**/*.md` — no broken cross-links.
- Manual review: every mention of "`new_commitment`" as a top-level argument is
  either removed or explicitly annotated as legacy-v1.

### Dependencies

Can start in parallel with Phases 4–9; must be finished before Phase 11.

---

## Phase 11 — Testnet rollout and attack simulation

**Goal:** Deploy the new contract to Stellar testnet, wire the relayer and
both SDKs to it, and run the full attack scenario end-to-end to prove the fix
holds in practice.

**Status:** ☐

### Files

| File | Action |
|------|--------|
| `docs/testnet-deployment.md` | Add v2 deployment section |
| `scripts/deploy-testnet.sh` | Pass six VKs to `initialize` |
| `tests/e2e/attack_simulation.rs` | **NEW** — scripted end-to-end exploit attempt |
| `relayer/.env.testnet` | Update contract ID |
| `clients/ios/.../Config.swift` | Testnet contract ID |
| `clients/android/.../Config.kt` | Testnet contract ID |

### Changes

1. Deploy contract wasm with the keyset-v2 artifacts as `initialize` arguments.
2. Record deployment outputs (contract ID, deployer, hash) in
   `docs/testnet-deployment.md`.

### Attack simulation

The end-to-end test must demonstrate that the rebinding attack from the vuln
report (§6.1) is now impossible. Steps:

1. A prover generates `(proof, publicInputs)` for a legitimate update with
   `c_new = X`.
2. An attacker (simulated relayer) substitutes `publicInputs.cNew = Y` in the
   HTTP body, keeps the proof bytes unchanged, and submits to the relayer.
3. The relayer forwards to Soroban.
4. Assertion: the on-chain transaction fails with `InvalidProof` (not
   `InvalidCommitmentEncoding` — we want the *proof-level* rejection on
   record, proving the cryptographic binding is doing the work).
5. A second attack attempt substitutes `c_new = 0xff..ff` (non-canonical).
6. Assertion: the relayer OR contract rejects with `InvalidCommitmentEncoding`.
7. A third attack attempt replays the proof against a different `group_id`.
8. Assertion: fails with `CommitmentMismatch` (state binding at the contract level).

All three failure modes must be reproducible from the test.

### Verification

- `cargo test --features e2e tests::attack_simulation::*` passes against live testnet.
- `create_group → update_commitment × N → verify_membership → deactivate_group`
  happy path succeeds.
- Testnet ledger shows six successful operations with the expected contract
  version tags.

### Dependencies

Phases 4, 5, 6, 7, 8, 9.

---

## Phase 12 — Mainnet rollout

**Goal:** Publish the production contract, update the SDK bundles to point at
the new contract ID and keyset-v2, and retire keyset-v1 from distribution.

**Status:** ☐

**Pre-flight checklist:**

- [ ] Phase 11 attack simulation ran green for at least one week on testnet
      with no regressions.
- [ ] External audit sign-off on `UpdateCircuit` relation, constraints, and
      contract changes.
- [ ] Security disclosure timeline (see `vuln-unbound-new-commitment.md` §12)
      completed.

### Files

| File | Action |
|------|--------|
| `docs/mainnet-deployment.md` | Add v2 section, contract hash, deployer, date |
| `scripts/deploy-mainnet.sh` | Pass six VKs at `initialize` |
| `clients/ios/.../Config.swift` | Mainnet contract ID |
| `clients/android/.../Config.kt` | Mainnet contract ID |
| `README.md` | Update quick-start to reference keyset-v2 |
| `keyset-v1/` | Add `DEPRECATED.md`; keep artifacts for forensic access |

### Changes

1. Deploy with mainnet admin signer.
2. Publish contract ID + wasm hash in `docs/mainnet-deployment.md`, signed by
   the release key.
3. Mark keyset-v1 as deprecated; do not delete.
4. Publish a post-mortem announcement referencing
   `docs/postmortem-unbound-new-commitment.md`.

### Verification

- Create → update → verify → deactivate round trip succeeds on mainnet with
  real user-sized groups.
- `stellar contract invoke … --fn version` returns `"2.0.0"`.
- Fleet telemetry shows zero clients still sending v1-shaped requests after
  the SDK hotfix window closes.

### Dependencies

Phase 11.

---

## Files summary

Consolidated table across all phases — useful when assigning reviewers and
scoping PRs.

| Area | File | Phases |
|------|------|--------|
| Rust core | `src/circuit/mod.rs` | 0, 1 |
| Rust core | `src/circuit/update.rs` | 0, 1 |
| Rust core | `src/prover/update.rs` | 2 |
| Rust core | `src/public_inputs.rs` | 2 |
| Rust core | `src/ffi/update.rs` | 3 |
| Rust core | `src/jni/update.rs` | 3 |
| Rust core | `include/stellar_mls.h` | 3 |
| Contract | `contracts/sep-xxxx/src/lib.rs` | 4 |
| Contract | `contracts/sep-xxxx/src/errors.rs` | 4 |
| Contract | `contracts/sep-xxxx/src/test.rs` | 4, 11 |
| Relayer | `relayer/src/handler.rs` | 5 |
| Relayer | `relayer/src/types.rs` | 5 |
| Swift | `swift-mls/Sources/SwiftMLS/PublicInputs.swift` | 6 |
| Swift | `swift-mls/Sources/SwiftMLS/UpdateCommitment.swift` | 6 |
| Swift | `swift-mls/Sources/SwiftMLS/SEPUpdateCommitmentRequest.swift` | 6 |
| Swift | `clients/ios/StellarChat/StellarChatTests/CrossPlatformVectorTests.swift` | 6, 9 |
| Kotlin | `kotlin-mls/src/main/kotlin/com/stellarmls/mls/PublicInputs.kt` | 7 |
| Kotlin | `kotlin-mls/src/main/kotlin/com/stellarmls/mls/UpdateCommitment.kt` | 7 |
| Kotlin | `kotlin-mls/src/main/kotlin/com/stellarmls/mls/SEPUpdateCommitmentRequest.kt` | 7 |
| Kotlin | `clients/android/.../CrossPlatformVectorTest.kt` | 7, 9 |
| Ceremony | `scripts/generate-keyset-v2.sh` | 8 |
| Ceremony | `src/ceremony/phase2.rs` | 8 |
| Ceremony | `keyset-v2/**` | 8 |
| Vectors | `docs/cross-platform-test-vectors.json` | 9 |
| Docs | `docs/sep.md` | 10 |
| Docs | `docs/proof-of-soundness.md` | 10 |
| Docs | `docs/design-doc.md` | 10 |
| Docs | `docs/relay-design-doc.md` | 10 |
| Docs | `docs/real-world-gap-analysis.md` | 10 |
| Deployment | `docs/testnet-deployment.md` | 11 |
| Deployment | `docs/mainnet-deployment.md` | 12 |

---

## Rollback strategy

Each phase is designed so that its output is **additive** through Phase 10.
Phases 11 and 12 are the only ones that change on-chain state; both are
reversible by redeploying the prior contract version and pointing SDK config
back at the v1 contract ID.

| Phase | Rollback action |
|-------|-----------------|
| 0–3 | Revert commits. No external surface touched. |
| 4 | Revert contract commits; keyset-v1 still usable. |
| 5 | Revert relayer commits; clients still compatible with v1 shape. |
| 6, 7 | Revert SDK commits; release patch `1.x.y+1` pinning to v1 wire format. |
| 8 | Stop distributing keyset-v2 tarball; keyset-v1 already cached by clients. |
| 9 | Revert vector JSON; consumers default back to v1 fixtures. |
| 10 | Revert doc commits. |
| 11 | Redeploy v1 contract to testnet; flip contract-ID config back. |
| 12 | Redeploy v1 contract to mainnet. **Note:** any state written between the v2 deploy and rollback is stranded; communicate the rollback window to users. |

---

## Acceptance criteria

The implementation is considered **shippable to mainnet** when, and only when,
all of the following are true:

1. All twelve phases have status ✅.
2. `cargo test --workspace` green across Rust core, contract, and relayer.
3. `swift test` green across `swift-mls/` and `clients/ios/`.
4. `./gradlew test` green across `kotlin-mls/` and `clients/android/`.
5. The Phase 11 attack simulation proves *the specific rebinding scenario from
   `vuln-unbound-new-commitment.md` §6.1* is rejected on testnet.
6. External audit review has landed, with findings addressed or explicitly
   accepted.
7. `docs/proof-of-soundness.md` contains the new operation-level theorem.
8. Keyset-v2 `metadata.json` signed by at least two release maintainers.
9. Post-mortem document
   ([postmortem-unbound-new-commitment.md](postmortem-unbound-new-commitment.md))
   published and cross-linked.

---

## Open questions

Questions that are known and will be resolved during execution. Logged here so
they aren't forgotten:

1. **Parallel contributor count for Phase 8 Phase 2 ceremony.** keyset-v1 had
   three participants per tier; does `UpdateCircuit` warrant the same bar, or
   higher given it's the security-critical fix?
2. **SDK major-version bump.** iOS 2.0.0 and Android 2.0.0 on publication?
   The wire format is breaking, so semver says yes.
3. **Gas cost delta.** MSM over 4 scalars vs. 3 will raise per-update gas.
   Measure on testnet; if the delta is large (> 20%), consider squeezing the
   canonical-bytes check path.
4. **Should `verify_membership` also transition to a 4-input form for
   consistency?** Current answer: no — its statement truly is single-epoch and
   the extra work is pointless. But this is a call worth confirming during
   Phase 10 doc review.

---

## Related documents

- [vuln-unbound-new-commitment.md](vuln-unbound-new-commitment.md) — the
  underlying vulnerability.
- [update-circuit-binding-design.md](update-circuit-binding-design.md) — the
  design being implemented.
- [postmortem-unbound-new-commitment.md](postmortem-unbound-new-commitment.md) —
  lessons from the discovery.
- [implementation_plan.md](implementation_plan.md) — style reference for
  phased plans.
- [proof-of-soundness.md](proof-of-soundness.md) — to be amended in Phase 10
  with operation-level theorem.
- [sep.md](sep.md) — normative SEP-XXXX specification; to be amended in Phase 10.
