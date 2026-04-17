# Update Circuit Binding — Design Doc

**Date:** 2026-04-17
**Status:** Draft
**Author:** Internal protocol review
**Version:** 0.1
**Supersedes:** (none — new circuit)
**Related:** [`vuln-unbound-new-commitment.md`](vuln-unbound-new-commitment.md), [`implementation-plan-update-circuit-binding.md`](implementation-plan-update-circuit-binding.md), [`postmortem-unbound-new-commitment.md`](postmortem-unbound-new-commitment.md), [`proof-of-soundness.md`](proof-of-soundness.md), [`sep.md`](sep.md)

---

## 1. Background

Stellar-MLS uses a single Groth16 circuit — `MembershipCircuit` at `src/circuit/mod.rs:48-74` — to authorise every ZK-gated contract operation. A proof from this circuit establishes that the prover knows a BLS12-381 secret key whose Poseidon-hashed leaf sits in a Poseidon Merkle tree with root `root`, and that `commitment = Poseidon(Poseidon(root, epoch), salt)` for the public commitment binding. The public inputs are `(commitment, epoch)`; the witnesses are `(secret_key, poseidon_root, salt, merkle_path, leaf_index)`.

Four contract entry points consume proofs from this circuit:

- `create_group` at `contracts/sep-xxxx/src/lib.rs:378-445` — proves membership in the initial tree at epoch 0.
- `update_commitment` at `:459-523` — proves membership at the current epoch; writes a new commitment for epoch+1.
- `verify_membership` at `:530-565` — read-only membership check.
- `deactivate_group` at `:574-617` — proves membership and freezes the group.

All four call `verify_groth16_proof` (at `:780-823`) with the same 2-public-input shape, against a per-tier verification key stored under `DataKey::VK(tier)`.

### 1.1 The `update_commitment` semantics

`update_commitment` takes five arguments beyond the environment:

- `group_id: BytesN<32>` — the group being advanced.
- `new_commitment: BytesN<32>` — the commitment to write for the next epoch.
- `new_epoch: u64` — must equal `current.epoch + 1`.
- `proof: Groth16Proof` — the ZK proof.
- `public_inputs: PublicInputs` — the claimed `(commitment, epoch)` the proof is over.

After a check cascade (epoch monotonicity, public-inputs-match-current-state, proof-replay, Groth16 verification), the contract writes `new_commitment` to storage as the group's next `CommitmentEntry`.

### 1.2 The relayer

A separate `relayer` service (Axum, `relayer/src/**`) lets users submit operations without paying Stellar fees themselves. For `update_commitment` the relayer branch at `relayer/src/handler.rs:229-235` accepts a JSON body of the form:

```json
{
  "groupID": "...",
  "newCommitment": "...",
  "newEpoch": 1,
  "proof": "...",
  "publicInputs": { "commitment": "...", "epoch": 0 }
}
```

and invokes the Stellar CLI with `--new-commitment`, `--new-epoch`, `--proof-file-path`, and `--public-inputs-file-path`. The relayer is explicitly **not** trusted with any secret material — today's trust model says it sees the proof (which is zero-knowledge) and the opaque group id. The design premise is that even a malicious relayer cannot harm the group because proofs authorise state transitions.

---

## 2. Problem

The design premise is false.

`new_commitment` is a function parameter to `update_commitment` but it is **not** part of the proof's public inputs and does not appear in any R1CS constraint in the circuit. The Groth16 pairing check is therefore entirely insensitive to it:

```
verify_groth16_proof(&vk, &proof, &current.commitment, current.epoch)
                                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
                                   two scalars go into the MSM;
                                   new_commitment is nowhere
```

(`contracts/sep-xxxx/src/lib.rs:492` and `:812-815`.)

The consequence: any party in the write path — a compromised relayer, a MITM on the relayer request, a Stellar mempool observer — can replace `new_commitment` with a value of their choosing while keeping `proof` and `public_inputs` byte-identical. The contract accepts the attack because the proof is numerically valid for its declared public inputs.

Two concrete impacts:

- **Silent hijack.** Attacker chooses `new_commitment = Poseidon(Poseidon(root_attacker, epoch+1), salt_attacker)` for a tree only they know. At the next epoch, no legitimate member can produce a valid membership proof — the group is captured.
- **Bricking.** Attacker picks any 32-byte value; no member can produce a preimage; the group is permanently stuck.

Full writeup in [`vuln-unbound-new-commitment.md`](vuln-unbound-new-commitment.md).

This is a protocol-level defect. The circuit, the contract, the prover SDK, and the relayer are each locally consistent. The gap is between the operation's security property and the statement the proof commits to. The [proof-of-soundness](proof-of-soundness.md) document proves knowledge-soundness for `(C_old, e_old)` — a correct proof of the wrong theorem.

---

## 3. Goals

1. Make the proof cryptographically bind `new_commitment` so that no party in the message path can substitute it without invalidating the proof.
2. Preserve the existing properties: membership privacy, constant verification cost, epoch monotonicity, proof-replay protection.
3. Preserve the existing `create_group` / `verify_membership` / `deactivate_group` paths with minimal or zero change.
4. Keep the fix scope tight — no scope creep into adjacent properties (nullifier, caller binding, new-tree membership).
5. Surface the fix in the documentation layer, not just the code: every reader of [`proof-of-soundness.md`](proof-of-soundness.md) should be able to tell which operation each theorem covers.

---

## 4. Non-goals

1. **Caller / tx binding.** The protocol is intentionally identity-free on chain (N-14 comment at `contracts/sep-xxxx/src/lib.rs:453-458`). A mempool observer who copies `(proof, public_inputs)` verbatim lands the exact state transition the user wanted — that is a privacy/fee-attribution question, not a soundness one.
2. **Per-epoch nullifiers.** Under epoch monotonicity + proof-hash replay (`:726-749`), a nullifier would impose "≤ 1 update per member per epoch" governance. That is a policy choice, not a soundness fix, and is out of scope here.
3. **"Updater remains a member of the new tree."** Any member can already author an update that removes everyone else — this is the pre-existing governance model, documented in [`sep.md`](sep.md). A new-tree-membership constraint would change governance and is not required for the vulnerability.
4. **Retroactive migration.** The project is in alpha. No mainnet traffic uses `update_commitment` yet. Breaking ABI is acceptable; VK re-derivation is acceptable; keyset rotation is handled by redeploy rather than by per-group VK migration.
5. **Relayer auth hardening.** Orthogonal. Even a perfectly authenticated relayer does not close the mempool-observer variant; the fix must work regardless of relayer trust.

---

## 5. Scope

### 5.1 In scope

- New `UpdateCircuit` module and VK.
- Two VK slots per tier (membership + update) in contract storage.
- `update_commitment` ABI change — drop standalone `new_commitment` and `new_epoch`; derive from `public_inputs` and `current.epoch + 1`.
- Canonical-bytes check on `public_inputs.new_commitment` before storage (mirrors the existing check on `commitment` at `:802-808`).
- New `Error::InvalidCommitmentEncoding` variant.
- FFI / JNI new entry points with a public-inputs schema version byte.
- Swift and Kotlin SDK mirrors of the new type + version handling.
- Relayer handler: drop top-level `newCommitment`; serialise it inside `publicInputs`.
- Trusted setup: Phase 2 MPC re-run for both circuits.
- Cross-platform test vectors: regenerate with `schemaVersion: 2`.
- Documentation: update [`sep.md`](sep.md), [`proof-of-soundness.md`](proof-of-soundness.md), [`design-doc.md`](design-doc.md), [`relay-design-doc.md`](relay-design-doc.md), [`real-world-gap-analysis.md`](real-world-gap-analysis.md).
- New contract tests covering the attack (rebinding rejection) and the new canonical check.

### 5.2 Out of scope

- Nullifier (see §4.2).
- Caller binding (§4.1).
- New-tree-membership constraint (§4.3).
- Changes to `MembershipCircuit` beyond adding a sibling module file.
- Changes to `create_group`, `verify_membership`, `deactivate_group`.
- Any change to the relayer's transport / auth model.
- Per-group VK overrides.
- Recursion / proof aggregation.

---

## 6. Design

### 6.1 The relation proved by `UpdateCircuit`

Public inputs:

- `commitment: Fr` — the current commitment, same meaning as `MembershipCircuit`.
- `epoch: Fr` — the current epoch, same meaning as `MembershipCircuit`.
- `new_commitment: Fr` — the commitment being written for epoch+1. New.

Witnesses:

- `secret_key: Fr` — prover's BLS12-381 scalar, same as `MembershipCircuit`.
- `poseidon_root: Fr` — current tree root, same as `MembershipCircuit`.
- `salt: [u8; 32]` — current salt, same as `MembershipCircuit`.
- `merkle_path: Vec<Fr>` — current-tree opening, same as `MembershipCircuit`.
- `leaf_index: usize` — current-tree index, same as `MembershipCircuit`.
- `new_poseidon_root: Fr` — new tree root. New.
- `new_salt: [u8; 32]` — new salt. New.

Constraints:

```
(1) leaf = Poseidon(secret_key)                                        [unchanged]
(2) MerklePoseidonOpen(leaf, merkle_path, leaf_index, poseidon_root)   [unchanged]
(3) commitment       == Poseidon(Poseidon(poseidon_root,     epoch),     salt)       [unchanged]
(4) new_commitment   == Poseidon(Poseidon(new_poseidon_root, epoch + 1), new_salt)   [NEW]
```

Constraints (1)–(3) are the same as `MembershipCircuit` (lines 183–245 of `src/circuit/mod.rs`). Constraint (4) is the new binding. The `epoch + 1` term inside the inner Poseidon is a linear combination over the existing `epoch_var` — no addition gadget needed.

New constraint count (per tier, approximate):

- Two Poseidon-2 calls: ~360 constraints each, depth-independent.
- One equality: ~1 constraint.
- Total delta: ~720 constraints per tier.

Measured post-implementation; header comment updated to reflect.

### 6.2 Statement in formal notation

Let `𝔽` be the BLS12-381 scalar field, `Poseidon` be the canonical Poseidon hash in `src/poseidon/mod.rs`, `𝓜` be the Poseidon Merkle tree function with a fixed tier depth. The relation `R_Update` is:

```
R_Update( (C_old, e_old, C_new), (sk, root_old, s_old, π_old, i_old, root_new, s_new) ) = 1
  iff
    Poseidon(sk) = 𝓜_leaf(root_old, π_old, i_old)
    C_old = Poseidon(Poseidon(root_old, e_old),     s_old)
    C_new = Poseidon(Poseidon(root_new, e_old + 1), s_new)
```

The `UpdateCircuit` implements `R_Update`. Soundness of `R_Update` together with Groth16 knowledge-soundness yields:

> No PPT adversary can produce an accepting proof π for `(C_old, e_old, C_new)` without knowing a witness satisfying `R_Update`, except with negligible probability.

In particular: an adversary who sees a proof `π` that verifies for `(C_old, e_old, C_new_user)` cannot produce a proof that verifies for `(C_old, e_old, C_new_attacker)` unless they know the witness — i.e. unless they are a member of the old tree and chose their own `(root_new, s_new)`. That is the exact property missing today.

### 6.3 Why a separate `UpdateCircuit` (not an extended `MembershipCircuit`)

Four call sites share the membership VK today. If we extend `MembershipCircuit` with the new public input, the other three call sites (`create_group`, `verify_membership`, `deactivate_group`) must either:

- supply a sentinel `C_new` (`0` or `commitment`) — making the 3rd public input carry different semantics per call site, a reviewing hazard that future code will mishandle;
- or add branching at each call site to detect "is this a transition?" and adjust the IC vector — which Groth16 does not support on a per-call basis (the IC vector is part of the CRS, fixed at setup).

Both options force the non-update call sites to interact with a public input that is meaningless for them. `verify_membership` is read-only and has no notion of a "next state"; forcing it to commit to one is semantically wrong and creates a migration hazard.

The alternative — a **separate** `UpdateCircuit` with its own VK — costs one extra VK per tier (3 → 6 total). The runtime cost on the non-update paths is unchanged (they keep loading the 2-public-input membership VK). The setup-time cost (one extra Phase 2 MPC run per tier) is modest given we are re-running Phase 2 anyway.

This is the chosen design.

### 6.4 Why the equality constraint (4) is load-bearing

A natural first instinct is to add `new_commitment` as a public input without any constraint involving it, reasoning that the Groth16 IC-vector folds all public inputs into the verification pairing so "swapping should break the proof." This reasoning is **incorrect**.

In Groth16, the IC vector element for a public input variable `x_i` is:

```
IC_i = (β · u_i(x) + α · v_i(x) + w_i(x)) / γ  in G1
```

where `u_i, v_i, w_i` are the columns of the R1CS matrices `A, B, C` at variable index `i`. If `x_i` appears in no constraint, then `u_i = v_i = w_i = 0` identically, so `IC_i = 0` (the identity in G1). The verifier's computation

```
vk_x = IC[0] + Σ_i x_i · IC_i
```

then has a zero contribution from `x_i`, and the pairing check is insensitive to `x_i`. Swapping `x_i` does not invalidate the proof.

The consequence: binding to `new_commitment` does **not** come from allocating it as a public input. It comes from constraint (4) — the Poseidon preimage equality — which (a) forces `new_commitment` to participate in one of the R1CS matrices, and (b) ties its value to witness values `new_poseidon_root` and `new_salt` which the prover must have known at proving time. A third party who lacks those witnesses cannot produce a proof for a different `new_commitment`, because the Groth16 zero-knowledge-argument knowledge-soundness extractor cannot output those witnesses without them.

This point is easy to miss and worth restating in the code. Constraint (4) is not there for "well-formedness cosmetics" — it is the binding mechanism.

### 6.5 What the design does and does not prove

**Does prove:**

- `new_commitment` is the Poseidon double-hash of some `(root_new, epoch+1, salt_new)` triple the prover knows (knowledge-soundness of Groth16 over `R_Update`).
- Any third party who modifies `new_commitment` in transit breaks constraint (4)'s equality and the proof fails.
- The prover is a member of the **current** tree (constraints (1)–(3), unchanged from `MembershipCircuit`).

**Does not prove:**

- `new_poseidon_root` is the root of any real member roster. `new_poseidon_root` is a witness — the prover can set it to any field element. A malicious member can commit to `root_new = F(garbage)` with a `new_salt` they invent, producing a `new_commitment` nobody can open at the next epoch. This is bricking by a legitimate member — the protocol already allows this (any member can update), and the fix does not prevent it.
- The prover remains a member of the **new** tree. Deliberately: the current scheme allows any member to author an update that removes every other member. Tying the proof to new-tree membership would change that governance.
- Membership in the new tree by any specific other party.
- Any property about non-authored updates (e.g. "if the update doesn't land, the state is unchanged" — that is separately guaranteed by contract storage atomicity).

Future governance work may add a second Merkle opening over `new_poseidon_root` as an optional constraint (`const NEW_TREE_MEMBERSHIP_REQUIRED: bool`). Out of scope for this fix.

### 6.6 Canonical-bytes check on `new_commitment`

`Fr::from_bytes` in Soroban silently reduces its 32-byte input modulo the BLS12-381 scalar field modulus `r ≈ 2^255`. That is, two distinct 32-byte strings can map to the same field element. The existing verifier at `contracts/sep-xxxx/src/lib.rs:802-808` guards against non-canonical `commitment` encodings by round-tripping through `Fr::to_bytes` and comparing:

```rust
let commitment_fr = Fr::from_bytes(commitment.clone());
let canonical_bytes: BytesN<32> = commitment_fr.to_bytes();
if canonical_bytes != *commitment {
    return false;
}
```

The same check must be applied to `public_inputs.new_commitment` before it is stored. Otherwise:

1. Attacker-author submits with `new_commitment` bytes `B` where `B` and `canonical(B)` differ (upper bits above `r`).
2. The Groth16 verifier reduces `B` to `canonical(B)` and pairs correctly.
3. The contract stores `B`, not `canonical(B)`.
4. At the next epoch, the member must produce a proof with `public_inputs.commitment` matching the stored value. But the contract's `public_inputs.commitment` canonical check rejects any non-canonical encoding. The canonical encoding is `canonical(B)`, which differs from the stored `B`. The proof fails.

The group is bricked — by a legitimate prover, using their own proof, no rebinding attack required. This is a distinct bug from the primary rebinding issue and must be fixed as part of the same change. See §8.2 below for the check in the contract code.

### 6.7 Public-input allocation order

Arkworks indexes the IC vector by the order in which public inputs are allocated via `FpVar::new_input`. For `UpdateCircuit`, the allocation order is:

1. `commitment_var`
2. `epoch_var`
3. `new_commitment_var`

The contract's `verify_groth16_proof_update` helper must pass scalars in the same order:

```rust
let msm_points:  Vec<G1Affine> = vec![env, ic1, ic2, ic3];
let msm_scalars: Vec<Fr>       = vec![env, commitment_fr, epoch_fr, new_commitment_fr];
```

The SDK `UpdatePublicInputs` struct field order must match:

```swift
// Swift
public struct UpdatePublicInputs: Codable {
    public let commitment:    Data
    public let epoch:         UInt64
    public let newCommitment: Data
}
```

A field-order mismatch between the circuit allocation, the contract IC construction, and the SDK serialisation produces proofs that verify under the wrong semantics — a silent footgun. Mitigations:

- A pinned-order unit test in the circuit crate asserts the exact allocation order.
- A `publicInputsVersion: u8 = 2` byte at the head of SDK-side serialisation guarantees that old SDK builds fail loud with `UnsupportedVersion` rather than serialise into the v1 shape against a v2 contract.
- A cross-platform test vector (`docs/cross-platform-test-vectors.json`) captures the exact bytes of a known (`proof`, `public_inputs`) pair; all three platforms must match.

---

## 7. Alternatives considered

### 7.1 Unified `MembershipCircuit` with sentinel `C_new` for read paths

Covered in §6.3. Rejected. The sentinel value introduces ambiguity (what does `C_new = 0` mean for `verify_membership`?), contradicts the semantic distinction between "membership" and "transition," and forces future reviewers to remember a convention the code does not enforce.

### 7.2 Unconstrained `new_commitment` as a public input

Covered in §6.4. Rejected. Zero IC contribution = zero binding. The equality constraint is necessary.

### 7.3 In-circuit Schnorr signature over `new_commitment` with `sk`

The prover signs `new_commitment` with their BLS12-381 private key `sk` via Schnorr; the signature is a public input; the circuit verifies the signature.

Pros: conceptually clean — "the member authorises this specific `new_commitment`."
Cons:
- BLS12-381-native Schnorr verification in circuit requires scalar multiplication in G1, which is expensive in R1CS (~1,500 constraints for a single fixed-base scalar mult with lookup tables, much more without). Compare to the Poseidon preimage approach (~720 constraints).
- Schnorr signatures are malleable under re-randomisation unless the hash binding is domain-separated carefully — adds complexity.
- The signature value leaks `sk`-dependent information unless the Schnorr nonce is produced inside the circuit (yet more constraints).

Rejected on cost and complexity. The Poseidon preimage constraint is strictly cheaper and achieves the same binding.

### 7.4 Pedersen commit-and-prove

The prover generates a Pedersen commitment `cm = a · G + r · H` to `new_commitment` with a witness opening; the commitment is a public input; the circuit verifies the opening.

Pros: generic commit-and-prove composition.
Cons:
- Requires two EC ops per commitment in-circuit (worse than Poseidon).
- Introduces a second commitment scheme alongside Poseidon, doubling the proof-reviewer surface.
- The existing Poseidon commitment to the member tree is a natural vehicle for binding without a second primitive.

Rejected on cost and tooling complexity.

### 7.5 Pre-signed Stellar transaction envelope

The user pre-signs the entire Stellar transaction (including `new_commitment`) with their Stellar account key; the relayer only pays the fee; `require_auth` on the contract binds the envelope.

Pros: no circuit change, no trusted setup re-run.
Cons:
- Ties the protocol to Stellar; does not generalise to other ledgers or to relayer-free submission.
- Leaks the submitter's Stellar account — breaks relayer-fee-decoupling anonymity.
- Does not protect against on-ledger front-running where the attacker submits with their own envelope.
- Does not fix the fundamental cryptographic gap — the proof still does not bind `new_commitment`; authorisation is just moved to a separate ledger-level signature. Future ledger migrations would reintroduce the gap.

Rejected — moves the defect rather than closing it.

### 7.6 Commit-reveal

Two-transaction scheme: user first commits `H(new_commitment ‖ blinding)` on chain; later reveals `new_commitment` and the blinding. The reveal is bound because the commitment was.

Pros: cryptographically sound; no Groth16 change required.
Cons:
- Doubles the on-chain transactions and user friction.
- Requires new state management in the contract (pending-reveal entries, timeouts, griefing resistance).
- Trivially defeated by the same rebinding attack on the commit step unless the commit step also goes through a ZK proof — at which point we are back to fixing the ZK proof.

Rejected — not simpler.

---

## 8. Implementation

### 8.1 New Rust module — `src/circuit/update.rs`

New file at `src/circuit/update.rs` declaring `UpdateCircuit`, `ConstraintSynthesizer` impl, and tests. Reuses `poseidon_hash_two_gadget` at `src/circuit/mod.rs:271-285`. Top-level `src/circuit/mod.rs` gets a `pub mod update;` line.

Struct shape:

```rust
#[derive(Clone)]
pub struct UpdateCircuit<F: PrimeField + Absorb> {
    // Public inputs — allocation order is load-bearing
    pub commitment:       Option<F>,
    pub epoch:            Option<u64>,
    pub new_commitment:   Option<F>,

    // Witnesses — current state
    pub secret_key:       Option<F>,
    pub poseidon_root:    Option<F>,
    pub salt:             Option<[u8; 32]>,
    pub merkle_path:      Option<Vec<F>>,
    pub leaf_index:       Option<usize>,

    // Witnesses — next state (new)
    pub new_poseidon_root: Option<F>,
    pub new_salt:          Option<[u8; 32]>,

    pub depth:             usize,
    pub poseidon_config:   PoseidonConfig<F>,
}
```

Constraint generation mirrors `MembershipCircuit::generate_constraints`, with a fourth section:

```rust
// Constraint 4: binding to new commitment
let epoch_plus_one = &epoch_var + FpVar::constant(Fr::ONE);
let h1 = poseidon_hash_two_gadget(cs.clone(), &cfg, &new_poseidon_root_var, &epoch_plus_one)?;
let c_new_computed = poseidon_hash_two_gadget(cs.clone(), &cfg, &h1, &new_salt_var)?;
c_new_computed.enforce_equal(&new_commitment_var)?;
```

Tests (see §9.3).

### 8.2 Contract — `contracts/sep-xxxx/src/lib.rs`

**New types:**

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdatePublicInputs {
    pub commitment:     BytesN<32>,
    pub epoch:          u64,
    pub new_commitment: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VkKind { Membership, Update }
```

**Storage:**

Keep `DataKey::VK(tier)` unchanged for membership. Add `DataKey::UpdateVK(tier)` for the new VK.

**`initialize`** (line 236–270) grows to take 6 VKs:

```rust
pub fn initialize(
    env: Env,
    admin: Address,
    vk_small: VerificationKeyData, vk_medium: VerificationKeyData, vk_large: VerificationKeyData,
    update_vk_small: VerificationKeyData, update_vk_medium: VerificationKeyData, update_vk_large: VerificationKeyData,
) -> Result<(), Error>
```

Length checks: membership VK `ic.len() == 3`; update VK `ic.len() == 4`.

**`update_vk`** (line 279–304) gains a `kind: VkKind` parameter, routes to the correct `DataKey` and enforces the correct IC length.

**`update_commitment`** (line 459–523) signature becomes:

```rust
pub fn update_commitment(
    env: Env,
    group_id: BytesN<32>,
    proof: Groth16Proof,
    public_inputs: UpdatePublicInputs,
) -> Result<(), Error>
```

Internal logic:

1. `require_initialized`.
2. Load `current` entry. `if !current.active → GroupInactive`.
3. Expect next epoch = `current.epoch + 1`; compare to `public_inputs.epoch + 1`. (Equivalent phrasing of existing `new_epoch == current.epoch + 1` check, but derived from the public inputs.)
4. `public_inputs.commitment == current.commitment && public_inputs.epoch == current.epoch` → else `PublicInputsMismatch`.
5. **Canonical-bytes check on `public_inputs.new_commitment`** (mirror of `:802-808`). Round-trip through `Fr::from_bytes` / `to_bytes`; reject with `Error::InvalidCommitmentEncoding` on mismatch.
6. `check_proof_replay`.
7. `let uvk = Self::load_update_vk(&env, current.tier)?;`
8. `if !verify_groth16_proof_update(&env, &uvk, &proof, &public_inputs) → InvalidProof`.
9. `record_proof`.
10. Archive current, write new `CommitmentEntry { commitment: public_inputs.new_commitment, epoch: current.epoch + 1, … }`.
11. Publish `CommitmentUpdated` event.

**`verify_groth16_proof_update`** (new, alongside `verify_groth16_proof` at `:780-823`):

```rust
fn verify_groth16_proof_update(
    env: &Env,
    vk: &VerificationKeyData,
    proof: &Groth16Proof,
    public_inputs: &UpdatePublicInputs,
) -> bool {
    // 4 IC points: ic0 (constant) + ic1 (commitment) + ic2 (epoch) + ic3 (new_commitment)
    // 3 MSM scalars: commitment_fr, epoch_fr, new_commitment_fr
    // Canonical check on commitment and new_commitment before use
    …
}
```

`verify_groth16_proof` (the 2-input membership version) is unchanged; it remains the verifier for `create_group`, `verify_membership`, `deactivate_group`.

**Error variant:**

```rust
enum Error {
    …
    InvalidCommitmentEncoding = 15,
}
```

The existing `Error` discriminants occupy `1..=14` (`contracts/sep-xxxx/src/lib.rs:56-82`); `13` is already `TierGroupLimitReached` and `14` is `AdminOnly`. The next free value is `15`.

**Comment update at `:453-458`:** Replace the N-14 comment to reflect the new binding:

```
N-14 (updated 2026-04-17): This function uses proof-based authorization only
(no caller.require_auth()) — caller identity is intentionally not bound.
However, `new_commitment` IS bound into the proof (UpdateCircuit constraint 4),
so no party in the request path can substitute it without invalidating the proof.
See update-circuit-binding-design.md for the full binding argument.
```

### 8.3 Prover — `src/prover/mod.rs`

New types alongside existing `ProverInput` and `PublicInputs`:

```rust
pub struct UpdateProverInput {
    pub secret_key:   Fr,
    pub epoch:        u64,
    pub salt:         [u8; 32],
    pub members:      Vec<CanonicalMember<Fr>>,  // current tree
    pub new_members:  Vec<CanonicalMember<Fr>>,  // next tree
    pub new_salt:     [u8; 32],
    pub depth:        usize,
}

pub struct UpdatePublicInputs {
    pub commitment:     Fr,
    pub epoch:          Fr,
    pub new_commitment: Fr,
}
```

New entry point:

```rust
pub fn prove_update<R: RngCore + CryptoRng>(
    pk: &ProvingKey,
    input: &UpdateProverInput,
    rng: &mut R,
) -> (Proof, UpdatePublicInputs)
```

Internally:

1. Build current tree from `input.members`; open `Poseidon(input.secret_key)` to get `merkle_path`, `leaf_index`, `poseidon_root`.
2. Compute `commitment = Poseidon(Poseidon(poseidon_root, input.epoch), input.salt)`.
3. Build new tree from `input.new_members`; compute `new_poseidon_root`.
4. Compute `new_commitment = Poseidon(Poseidon(new_poseidon_root, input.epoch + 1), input.new_salt)`.
5. Populate `UpdateCircuit`.
6. Run Groth16 prover.
7. Return `(proof, UpdatePublicInputs { commitment, epoch: Fr::from(input.epoch), new_commitment })`.

Lower-level helper `prove_update_with_roots(pk, sk, epoch, salt, members, merkle_path, leaf_index, root, new_root, new_salt, rng)` is exposed for callers that have already built trees — e.g. FFI wrappers.

### 8.4 FFI / JNI

**`src/ffi.rs`** gains `sep_prove_update` mirroring the existing `sep_prove` surface but accepting the extended input and returning `UpdatePublicInputs` serialised with a 1-byte `PUBLIC_INPUTS_VERSION = 2` header. Version `1` remains the 2-public-input membership shape.

**`src/jni_ffi.rs`** gains `sepProveUpdate` identically.

Serialisation layout (network byte order):

```
[0]        : u8    version (0x02)
[1..33]    : [u8;32] commitment
[33..41]   : u64   epoch (big-endian)
[41..73]   : [u8;32] new_commitment
```

SDKs decode by first reading the version byte; any value other than `2` returns an explicit `UnsupportedPublicInputsVersion` error.

### 8.5 Relayer — `relayer/src/handler.rs`

The `"update_commitment"` branch at lines 229–235 becomes:

```rust
"update_commitment" => {
    add_hex_arg(&mut cmd, "--group-id", payload, "groupID")?;
    add_proof_arg(&mut cmd, payload)?;
    add_update_public_inputs_arg(&mut cmd, payload)?;  // new helper
}
```

The top-level `newCommitment` and `newEpoch` fields are dropped from the expected request body. The new helper `add_update_public_inputs_arg` reads `publicInputs.commitment`, `publicInputs.epoch`, and `publicInputs.newCommitment` and serialises them to a JSON file the Stellar CLI consumes. The existing `add_public_inputs_arg` stays for the membership call sites (`create_group`, `verify_membership`, `deactivate_group`).

Request body for `update_commitment` (v2):

```json
{
  "groupID": "...",
  "proof": "...",
  "publicInputs": {
    "version": 2,
    "commitment": "...",
    "epoch": 0,
    "newCommitment": "..."
  }
}
```

### 8.6 SDKs — Swift and Kotlin

**Swift (`swift-mls/Sources/SwiftMLS/Types.swift`)** gets a new type:

```swift
public struct UpdatePublicInputs: Codable {
    public let version:       UInt8   // = 2
    public let commitment:    Data
    public let epoch:         UInt64
    public let newCommitment: Data
}
```

`SEPUpdateCommitmentRequest` (lines 110–123) drops the top-level `newCommitment` field; the `publicInputs` property is retyped to `UpdatePublicInputs`.

**Kotlin (`kotlin-mls/`)** mirrors the Swift change identically.

### 8.7 Trusted setup and keyset

Phase 1 (Powers-of-Tau) can be reused unchanged — it is circuit-agnostic. Phase 2 (circuit-specific) must re-run because `UpdateCircuit` is a new circuit. We also take the opportunity to re-run Phase 2 for `MembershipCircuit` with the proper MPC (today's `docs/phase-2.md:243-257` admits the current Phase 2 is a single-machine simulation).

Artifacts:

- `keyset-v2/membership/{small,medium,large}/vk.json` + `pk.bin`
- `keyset-v2/update/{small,medium,large}/vk.json` + `pk.bin`
- `keyset-v2/metadata.json` with `schemaVersion: 2`, SHA-256 digests, ceremony participants, final `beacon`, etc.

`keyset-v1/` is retired (retained for historical reference, not shipped in SDKs).

Ceremony docs updated:

- [`trusted-setup-ceremony-phase1-start.md`](trusted-setup-ceremony-phase1-start.md) — updated to reflect two circuits.
- [`trusted-setup-ceremony-phase2-participant-playbook.md`](trusted-setup-ceremony-phase2-participant-playbook.md) — run twice, once per circuit.

See [`implementation-plan-update-circuit-binding.md`](implementation-plan-update-circuit-binding.md) §Phase 8 for the operational steps.

---

## 9. Testing strategy

### 9.1 Circuit-level unit tests (`src/circuit/update.rs`)

- `test_update_circuit_roundtrip`: prove then verify for a known `(C_old, e_old, C_new)`. Expect pass.
- `test_update_rejects_modified_c_new`: generate a valid proof; verify against the same `(C_old, e_old)` but a different `C_new'`. Expect fail.
- `test_update_rejects_modified_epoch`: same pattern for `epoch`.
- `test_update_rejects_modified_c_old`: same pattern for `commitment`.
- `test_update_public_input_count`: assert `num_instance_variables == 4` (1 constant + 3 inputs).
- `test_update_public_input_allocation_order`: pin the exact allocation order of the three inputs.
- `test_update_constraint_count_small/medium/large`: measure and record the new constraint counts per tier. Fail on unexpected regressions.
- `test_update_old_and_new_salt_can_be_equal`: degenerate case — `old_salt == new_salt` should still prove and verify.

### 9.2 Prover-level tests (`src/prover/mod.rs`)

- `test_prove_update_roundtrip`: `prove_update` + verifier = pass.
- `test_prove_update_from_members_matches_low_level`: `prove_update_from_members` yields identical proof bytes to `prove_update_with_roots` when given equivalent inputs.
- `test_prove_update_rejects_nonmember_prover`: prover's `sk` is not in `members` — `Err`.

### 9.3 Contract tests (`contracts/sep-xxxx/src/lib.rs` `mod test`)

- `test_update_commitment_happy_path`: end-to-end with the new circuit.
- `test_update_commitment_rejects_rebinding`: valid proof for `(C_old, e_old, C_new_A)`, submit with `public_inputs.new_commitment = C_new_B`; expect `InvalidProof`.
- `test_update_commitment_rejects_non_canonical_new_commitment`: pass `BytesN<32>` with a byte > modulus-high; expect `InvalidCommitmentEncoding`.
- `test_update_commitment_rejects_mismatched_public_inputs`: `public_inputs.commitment ≠ current.commitment`; expect `PublicInputsMismatch`.
- `test_vk_length_mismatch_update`: initialize with an update VK of `ic.len() != 4`; expect `InvalidVkLength`.
- `test_update_vk_routing`: `update_vk(kind=Update, …)` writes to `DataKey::UpdateVK`; membership call sites still read `DataKey::VK`.
- `test_initialize_takes_six_vks`: signature regression test.
- `test_create_group_unaffected`: `create_group` still uses 2-public-input VK, unchanged behaviour.
- `test_verify_membership_unaffected`: same.
- `test_deactivate_group_unaffected`: same.

### 9.4 Cross-platform tests

- `docs/cross-platform-test-vectors.json` regenerated with `schemaVersion: 2` and a new section for `UpdateCircuit` canonical vectors.
- `clients/ios/StellarChat/StellarChatTests/CrossPlatformVectorTests.swift` consumes the new section; fails loudly on version mismatch.
- `clients/android/StellarChat/app/src/androidTest/java/chat/onym/android/CrossPlatformVectorTests.kt` — same.

### 9.5 End-to-end on testnet

Per [`testnet-deployment.md`](testnet-deployment.md):

1. Redeploy contract.
2. `initialize` with 6 VKs.
3. Create a test group, perform one happy-path `update_commitment`.
4. Attack simulation: craft a relayer call with `public_inputs.new_commitment` differing from the original proof's `new_commitment`. Expect on-chain rejection with `InvalidProof`.
5. Non-canonical simulation: craft `public_inputs.new_commitment = 0xFFFF…FF` (above modulus). Expect `InvalidCommitmentEncoding`.
6. CU budget: verify that the MSM on 3 scalars (vs 2) stays well within the Stellar host limits. Pairing cost is unchanged.

---

## 10. Threat model

### 10.1 Attacker capabilities considered

| Capability | Mitigated by |
|---|---|
| Substitute `new_commitment` in relayer request body | Constraint (4) in `UpdateCircuit`; IC vector folds `new_commitment` into pairing check |
| Substitute `new_commitment` in Stellar transaction envelope | Same — contract derives `new_commitment` from `public_inputs`, not from a separate parameter |
| Re-randomise `(a, b, c)` and submit | Re-randomisation preserves the statement, not modifies it; the modified proof still verifies against the same `(C_old, e_old, C_new)` — attacker gains nothing |
| Front-run legitimate update with copied `(proof, public_inputs)` | Benign — same state transition user wanted. Fee-attribution / authorship credit flows to attacker, but state integrity preserved. Covered by §4.1 (caller binding explicitly out of scope) |
| Non-canonical `new_commitment` encoding | §6.6 canonical-bytes check |
| Submit update for a group the attacker is not a member of | Groth16 knowledge-soundness over `R_Update` — attacker has no witness, cannot produce an accepting proof |
| Compromise trusted setup participant | Phase 2 MPC 1-of-N trust assumption — see [`phase-2.md`](phase-2.md). Partial-compromise coverage unchanged by this fix |

### 10.2 Residual risks

1. **Legitimate member brick.** A member can author an update with `new_commitment = Poseidon(Poseidon(garbage_root, e+1), garbage_salt)` where nobody else knows the preimages. The group becomes unusable. This is identical to the existing "any member can update to a tree only they know" attack — out of scope, documented in [`sep.md`](sep.md).
2. **Legitimate member hijack.** A member can write `new_commitment` for a tree containing only their own key. Same class as above.
3. **Trusted setup toxic waste.** If Phase 2 MPC has <1 honest participant, soundness is broken for both circuits. Unchanged by this fix; tracked in [`phase-2.md`](phase-2.md).
4. **Proof-system-level breakage.** Groth16 is believed secure under the AGM in BLS12-381; a break would affect every SNARK deployment. Out of scope.
5. **Implementation bugs in the new circuit.** Addressed by §9 tests; external audit recommended before mainnet.
6. **Side-channel leakage of `sk` via prover timing / memory on compromised devices.** Unchanged. Out of scope.

---

## 11. Backward compatibility

The project is in alpha. Pre-fix state:

- No mainnet traffic using `update_commitment`.
- Testnet groups under keyset-v1 exist but are transient.
- SDKs pinned to keyset-v1 artifacts.

Strategy: **hard cut**. No dual-schema period. Redeploy contract with keyset-v2; republish SDK artifacts; retire keyset-v1 references. Client apps bump their minimum SDK version; older builds fail loud on the `publicInputsVersion` byte.

Explicitly rejected alternatives:

- **Per-group VK slots.** Would allow a migration window. Rejected — adds storage complexity, not needed in alpha.
- **Dual accept paths (`update_commitment_v1` + `update_commitment_v2`).** Rejected — leaves the vulnerable v1 path callable.
- **Feature flag.** Rejected — the fix must not be disable-able.

---

## 12. Operational plan

### 12.1 Ceremony

1. Confirm Phase 1 (Powers-of-Tau) artifacts.
2. Run Phase 2 for `MembershipCircuit` with the real MPC described in [`trusted-setup-ceremony-phase2-participant-playbook.md`](trusted-setup-ceremony-phase2-participant-playbook.md).
3. Run Phase 2 for `UpdateCircuit` with the same MPC.
4. Publish transcripts and verification artifacts per existing ceremony hygiene.
5. Bundle into `keyset-v2/`.

### 12.2 Rollout

1. Testnet: redeploy with keyset-v2; run smoke + attack-simulation suite; observe for at least one cycle of create → update → verify → deactivate per tier.
2. Mainnet: redeploy with keyset-v2; retire keyset-v1 endpoints.
3. SDK release: new SDK versions pinned to keyset-v2 artifacts; old SDKs fail loud on handshake via the `publicInputsVersion` byte.

### 12.3 Monitoring

- Watch on-chain `UpdateError` events (new) for unexpected rejections — may indicate stale clients.
- Sample a fraction of accepted `CommitmentUpdated` events and verify off-chain that `public_inputs.new_commitment` matches the stored commitment — defence in depth against future implementation bugs.

---

## 13. Risks and mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| Field-order mismatch between circuit alloc and SDK struct | Medium | Pinned unit test (§9.1); version byte (§6.7); cross-platform vectors (§9.4) |
| Phase 2 MPC botched for one circuit | Low | Two independent Phase 2 runs; external verifier reproduces transcripts |
| Contract ABI migration breaks a downstream consumer | Low (alpha) | Explicit hard-cut; SDK version pin |
| CU budget regression on `update_commitment` | Very low | 3-scalar MSM vs 2-scalar — trivial delta; pairing unchanged |
| Non-canonical bytes slipping through in some input path we missed | Low | Single canonical helper used everywhere; grep audit |
| New circuit proof-size or prover-time regression unnoticed | Medium | Benchmarks in CI; fail on >10% prover-time regression |

---

## 14. Appendices

### Appendix A — Estimated R1CS constraint counts

| Tier | Depth | Membership (current) | UpdateCircuit (new) | Delta |
|------|-------|----------------------|---------------------|-------|
| Small | 5 | ~1,910 | ~2,630 | +720 |
| Medium | 8 | ~2,630 | ~3,350 | +720 |
| Large | 11 | ~3,350 | ~4,070 | +720 |

Numbers are pre-implementation estimates derived from the Poseidon-2 gadget count at `src/circuit/mod.rs:271-285` plus the existing per-tier figures at `src/circuit/mod.rs:22-24`. Final figures measured and recorded during Phase 1 of the implementation plan.

### Appendix B — Groth16 IC-vector argument, summary

For the R1CS system `A · s ∘ B · s = C · s` over field `𝔽_r` with `s = (1, x₁, …, x_l, w_{l+1}, …, w_m)` (statement `x`, witness `w`), Groth16 uses a CRS that produces per-index group elements

```
IC_i = (β u_i(x) + α v_i(x) + w_i(x)) / γ      for i ∈ {0, …, l}
```

The verifier computes `vk_x = Σ_{i=0..l} x_i · IC_i` (with `x_0 := 1`) and checks the pairing equation

```
e(A, B) = e(α, β) · e(vk_x, γ) · e(C, δ)
```

Soundness (AGM, or generic-group; see Groth 2016 §4): if a PPT adversary outputs `(A, B, C)` that satisfies the equation for `x`, then with overwhelming probability there exists a witness `w` such that `(x, w)` satisfies the R1CS system — i.e. the prover must have known a valid witness for that specific `x`.

**Key consequence for binding:** the equation ties `(A, B, C)` to `vk_x`, which depends on the full statement `x = (x₁, …, x_l)`. Changing any `x_i` changes `vk_x` by `(x_i' - x_i) · IC_i`. If `IC_i ≠ 0`, the pairing equation breaks and the proof is rejected.

**Why `IC_i` can be zero:** if variable `i` does not appear in any constraint, then `u_i(x) ≡ v_i(x) ≡ w_i(x) ≡ 0` identically, so `IC_i = 0` in G1. Swapping `x_i` has no effect on `vk_x`. The proof is insensitive to that public input. **This is the subtle point that motivates constraint (4).**

Constraint (4), `c_new_computed.enforce_equal(new_commitment_var)`, forces `new_commitment_var` to appear as a C-matrix (or A/B) entry in at least one R1CS row, giving `IC_new_commitment ≠ 0` generically and binding the proof to that public input.

### Appendix C — File-by-file change summary

| File | Change |
|---|---|
| `src/circuit/mod.rs` | `pub mod update;` line only |
| `src/circuit/update.rs` | **NEW** — `UpdateCircuit` struct, constraint synthesiser, unit tests |
| `src/prover/mod.rs` | New `UpdateProverInput`, `UpdatePublicInputs`, `prove_update`, `prove_update_from_members` |
| `src/ffi.rs` | New `sep_prove_update` entry point; version byte in output |
| `src/jni_ffi.rs` | New `sepProveUpdate` entry point; version byte in output |
| `contracts/sep-xxxx/src/lib.rs` | New `UpdatePublicInputs`, `VkKind`; `DataKey::UpdateVK`; extended `initialize`, `update_vk`; rewritten `update_commitment`; new `verify_groth16_proof_update`; new `Error::InvalidCommitmentEncoding`; updated N-14 comment |
| `relayer/src/handler.rs` | `update_commitment` branch updated; new `add_update_public_inputs_arg` helper |
| `swift-mls/Sources/SwiftMLS/Types.swift` | New `UpdatePublicInputs` type; `SEPUpdateCommitmentRequest` retyped |
| `kotlin-mls/` (mirror) | Same as Swift |
| `keyset-v2/` | **NEW** — SRS Phase 2 outputs for both circuits, 3 tiers each |
| `scripts/generate-keyset*`, `scripts/generate-mainnet-vks*` | Produce both circuits' artifacts |
| `src/ceremony/**` | Run Phase 2 for both circuits |
| `docs/cross-platform-test-vectors.json` | Regenerated with `schemaVersion: 2` |
| `docs/sep.md` | New §"Update Circuit"; ABI tables updated |
| `docs/proof-of-soundness.md` | New Theorem for `R_Update`; existing Theorem 3 scoped to membership paths |
| `docs/design-doc.md`, `docs/relay-design-doc.md`, `docs/real-world-gap-analysis.md` | Reflect the two-circuit split and the payload changes |

### Appendix D — Glossary

- **`C_old`** — the group's current commitment, stored on chain as `CommitmentEntry.commitment`.
- **`e_old`** — the current epoch.
- **`C_new`** — the commitment being written for the next epoch by `update_commitment`.
- **`root_old`**, **`s_old`** — Poseidon Merkle root and salt binding `C_old`.
- **`root_new`**, **`s_new`** — same for `C_new`.
- **`sk`** — the prover's BLS12-381 scalar private key.
- **Poseidon** — the SNARK-friendly hash used throughout; configured in `src/poseidon/mod.rs`.
- **Poseidon-2** — Poseidon with arity 2 (two field-element inputs, one field-element output).
- **MSM** — multi-scalar multiplication; used in Groth16 verification to compute `vk_x`.
- **IC vector** — the CRS-derived G1 points `{IC_0, IC_1, …, IC_l}` used by the verifier to fold public inputs into `vk_x`.
- **`MembershipCircuit`** — the existing circuit proving `(C_old, e_old)`-membership.
- **`UpdateCircuit`** — the new circuit proving `(C_old, e_old, C_new)`-transition.
- **`VkKind`** — `Membership | Update` — discriminator on `update_vk` routing.
- **`UpdatePublicInputs`** — the 3-field public-inputs struct used by `UpdateCircuit`.
- **Canonical-bytes check** — round-trip `Fr::from_bytes` / `to_bytes` to reject non-canonical 32-byte encodings that exceed the scalar modulus.
- **`PUBLIC_INPUTS_VERSION`** — u8 byte at the head of SDK public-inputs serialisations; `2` for the new layout.
- **keyset-v2** — the post-fix artifact bundle containing Phase 2 outputs for both circuits across three tiers.
