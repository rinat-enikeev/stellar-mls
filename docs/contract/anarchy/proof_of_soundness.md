# Proof of Soundness — `contracts/sep-anarchy`

**Scope:** the per-type Anarchy Soroban contract at `contracts/sep-anarchy/src/lib.rs` (revision: branch `feat/contract-sep-anarchy`).

**Status:** informal soundness argument. Not a machine-checked proof. Cryptographic core (Groth16, Poseidon, BLS12-381) relied upon as a black box; trust assumptions explicit below.

**Companion:** [`proof_of_correctness.md`](proof_of_correctness.md) covers spec→impl alignment.

---

## 1. Threat model

| Actor | Capability | Trust |
|---|---|---|
| **Admin** | `__constructor`; rotate VKs (`update_vk`); toggle `set_restricted_mode` | Trusted to install honest VKs; ceremony-bound for production |
| **Member** | Submits Groth16 proofs at `create_group`, `update_commitment`, `verify_membership`, `deactivate_group`. Has the secret key behind a member leaf | Untrusted from the contract's point of view; the proof is the auth |
| **Observer** | Reads chain state and event log | Untrusted; can read `commitment` / `epoch` / `member_count` (informational) / events |

**Out-of-scope adversarial capabilities:**
- Forging Groth16 proofs (relies on Groth16's discrete-log assumption + trusted setup).
- Breaking BLS12-381 pairing or Poseidon collision resistance.
- Compromising Soroban's host runtime, BLS host functions, or persistent-storage integrity.
- Compromising the admin key (covered by ceremony for production).
- Pre-inclusion mempool replay (acknowledged in the design §11; circuit-level operation-tag binding would close it).
- Cross-group replay against Anarchy MembershipCircuit when both groups share root + epoch (sep-xxxx Audit Finding #1; deferred per design §11 — pure extraction does not change the v1 circuit).

---

## 2. Trust assumptions

1. **Soroban BLS host functions** — `g1_add`, `g1_msm`, `pairing_check`, `is_in_subgroup`, `Fr::from_bytes`, `Fr::to_bytes`, `Fr::from_u256`, `U256::from_be_bytes` — implement BLS12-381 group operations correctly (Stellar SEP-0046 / soroban-sdk 25.3.0).
2. **Groth16 verifier formula** as implemented in `verify_membership_proof` and `verify_update_proof` — `e(-π_A, π_B) · e(α, β) · e(vk_x, γ) · e(π_C, δ) = 1_GT` is sound under the q-PKE / generic-group assumption used by Groth16, given a trusted setup.
3. **Poseidon collision-resistance / preimage-resistance** is used by the *prover* (off-chain). The contract is value-agnostic to Poseidon — it sees only 32-byte field elements.
4. **`env.crypto().sha256`** is collision-resistant (used for the proof-replay nullifier).
5. **The admin's installed VKs correspond to a real ceremony / dev-keyset** that fixes the circuit constraint system. A malicious admin who installs a VK with known toxic-waste secrets can forge proofs.
6. **Stellar's transaction ordering and persistent-storage atomicity** — reads and writes within a single Soroban host invocation are atomic; failed entrypoints revert all writes.

---

## 3. Soundness invariants

### 3.1 State-binding invariants

**SI-1: `current.commitment` is bound by a Groth16 proof against the membership VK at `current.tier` (at create) and against the update VK at every subsequent epoch.**
Established by the verifier call inside `create_group`. Re-established by the verifier call inside `update_commitment`.

**SI-2: `c_old` and `epoch_old` on the wire equal `current.commitment` and `current.epoch` at the moment of `update_commitment`.**
Enforced by the chaining gate inside `update_commitment`. A wire payload that doesn't bind to current state is rejected with `PublicInputsMismatch` before the proof is loaded.

**SI-3: `member_count` is informational and never mutated by the contract.**
Set at `create_group` from the wire. Preserved verbatim at `update_commitment` via `member_count: current.member_count` in the new entry's struct literal. Preserved verbatim at `deactivate_group` via `..current.clone()`. The contract is value-agnostic to this field; clients track the actual count off-chain. **NOT a soundness invariant per se** (the contract makes no claim about the field's accuracy); SI-3 documents the field's contract-side immutability post-create.

### 3.2 Cryptographic invariants

**SI-4: All curve points stored in any of the 6 VKs are subgroup-valid.**
`validate_vk_points` runs `is_in_subgroup` on `α_g1`, `β_g2`, `γ_g2`, `δ_g2`, and every `IC[i]`. Called at `__constructor` for all 6 VKs and at `update_vk`. A small-subgroup VK is rejected with `Error::InvalidPoint`.

**SI-5: All proof points are subgroup-valid.**
`validate_proof_points` runs `is_in_subgroup` on `proof.a`, `proof.b`, `proof.c`. Called at the start of `verify_membership_proof` and `verify_update_proof`. A small-subgroup proof returns `false` (→ `Error::InvalidProof` upstream).

**SI-6: All field-element commitments use canonical Fr encoding.**
`is_canonical_fr` round-trips `Fr::from_bytes ∘ Fr::to_bytes` and rejects any 32-byte value whose `Fr::from_bytes` reduces. Called at every entrypoint for every wire scalar AND defensively inside each verifier helper. Non-canonical inputs → `InvalidCommitmentEncoding`. Closes the malleability hole where `Fr` reduction can produce two distinct 32-byte preimages of the same field element.

**SI-7: VK IC-vector length matches each circuit's public-input arity.**
`MEMBERSHIP_IC_POINTS = 3`, `UPDATE_IC_POINTS = 4`. Enforced at `__constructor` and `update_vk` and re-asserted inside `verify_membership_proof` and `verify_update_proof`. A wrong-arity VK can never be installed.

### 3.3 Replay protection

**SI-8: A proof's bytes (`a || b || c`) cannot be reused at any state-changing entrypoint within `LEDGER_BUMP` (~30 days).**
`proof_hash` = `sha256(a.to_array() || b.to_array() || c.to_array())`. `check_proof_replay` is called BEFORE proof verification at `create_group`, `update_commitment`, and `deactivate_group`. `record_proof` is called only after the proof verifies, so a failed entrypoint does not consume the nullifier (transactional revert).

The scope is **contract-global** — not per-group. Two distinct groups cannot accept byte-identical proofs.

`verify_membership` does NOT call `check_proof_replay` — read-only, no nullifier consumption. The same proof bytes can be re-submitted to that entrypoint indefinitely. Documented in `verify_membership`'s doc-comment (rationale: read-only verifier semantics; high-frequency repeats are a metering concern, not a soundness one).

### 3.4 Authorization

**SI-9: Admin-gated entrypoints require Soroban auth from the stored admin address.**
`update_vk` and `set_restricted_mode` both load `Admin` from instance storage and call `admin.require_auth()`. Without a matching auth, Soroban panics with "Unauthorized" before any state mutation.

**SI-10: `create_group` requires `caller.require_auth()` and, in restricted mode, `caller == admin`.**
The `caller.require_auth()` call inside `create_group` prevents one address from creating a group on behalf of another. In restricted mode, `caller != admin` returns `AdminOnly` (restricted-mode gate in `create_group`).

**SI-11: `update_commitment`, `verify_membership`, `deactivate_group` do NOT require Soroban-level auth.**
The proof IS the authorization. Documented in `update_commitment`'s doc-comment. An attacker without a valid proof cannot construct a passing `verify_*_proof` call. The only attack surface is a valid proof that's been replayed, which SI-8 closes.

### 3.5 State-machine invariants

**SI-12: Epoch is monotonic and increments by exactly 1 per successful `update_commitment`.**
Enforced inside `update_commitment` via `current.epoch.checked_add(1)`, with the resulting `new_epoch` written to storage. The `checked_add` returns `InvalidEpoch` if `current.epoch == u64::MAX`.

**SI-13: A group transitions `active=true → false` exactly once and irreversibly.**
`create_group` writes `active: true`. `update_commitment` preserves `active: true` on the new entry. `deactivate_group` rejects if `current.active == false` and sets `active: false`.

**SI-14: `tier` is fixed at create and never mutated.**
`update_commitment`'s new entry inherits `tier: current.tier`; `deactivate_group`'s deactivated entry uses `..current.clone()`. No entrypoint takes `tier` as a wire input post-create.

**SI-15: `GroupCount(tier)` is bounded by `MAX_GROUPS_PER_TIER = 10000`.**
`create_group` reads the count and rejects with `TierGroupLimitReached` if `count >= MAX_GROUPS_PER_TIER`. `deactivate_group` decrements with an underflow guard.

**SI-16: `__constructor` is callable exactly once.**
`do_initialize` returns `AlreadyInitialized` if `DataKey::Admin` is already set. Called at deploy time by Soroban.

### 3.6 History invariants

**SI-17: History is a rolling FIFO window of at most `HISTORY_WINDOW = 64` entries.**
`archive_entry` appends and prunes from the front. Test pin: `test_archive_entry_appends_and_prunes`.

**SI-18: History captures the prior active state, not the post-update state.**
`update_commitment` calls `archive_entry(env, &group_id, &current)` BEFORE writing the new entry. `deactivate_group` likewise archives `&current` before flipping inactive.

---

## 4. Per-entrypoint soundness argument

For each state-changing entrypoint, soundness reduces to "every successful return implies the corresponding storage delta is justified by a valid Groth16 proof + auth + storage gates."

### 4.1 `create_group`

A successful return requires (in source order): initialized; `caller.require_auth()`; restricted-mode gate; `tier ≤ 2`; public_inputs match supplied commitment + epoch=0; group not already exists; canonical Fr commitment; `GroupCount(tier) < MAX_GROUPS_PER_TIER`; replay-fresh; membership proof verifies under `VK(tier)` against `(commitment, 0)`. Storage delta on success: `Group(group_id) := CommitmentEntry{...}`, `History(group_id) := []`, `GroupCount(tier) += 1`, `UsedProof(proof_hash) := true`.

**Soundness statement:** the only way to populate `Group(group_id)` is to produce a Groth16 proof against the membership circuit, against a `commitment` that is canonical-Fr-valid, with `epoch=0`. Computationally infeasible without knowledge of a witness satisfying the circuit's R1CS — which reduces to "knowing a secret key behind a member leaf in the Merkle tree rooted at `commitment`." `member_count` is wire-supplied and stored verbatim; the contract makes no soundness claim about its accuracy.

### 4.2 `update_commitment`

A successful return requires: initialized; `Group(group_id)` exists and `active == true`; `current.epoch + 1` doesn't overflow; SI-2 chaining; `c_new` canonical Fr; replay-fresh; update proof verifies under `UpdateVK(current.tier)` with public inputs `(c_old, epoch_old, c_new)`. Storage delta on success: `Group(group_id) := CommitmentEntry{c_new, current.epoch+1, ..., active: true, member_count: current.member_count}`; `History(group_id)` ← prior `current`; `UsedProof(proof_hash) := true`.

**Soundness statement:** the contract stores only states connected by a chain of valid update proofs from `create_group`'s initial state. Forking the chain (writing a `c_new` not derived from a witness against `c_old`) is infeasible. `member_count` is preserved verbatim — the contract has no Poseidon host to recompute it.

### 4.3 `verify_membership`

Read-only. Returns `Ok(true)` iff `(public_inputs.commitment == current.commitment) ∧ (public_inputs.epoch == current.epoch) ∧ (proof verifies under VK(current.tier))`. Returns `Ok(false)` on bad proof, `Err(...)` on storage / chain mismatch.

**Note**: Does NOT check `state.active`. Post-deactivation attestations against the frozen final state remain verifiable forever. Documented in `verify_membership`'s doc-comment.

### 4.4 `deactivate_group`

A successful return requires SI-1's witness (current member proof) plus SI-2 chaining at `current.epoch`. Replay-fresh. State delta: `Group(group_id).active = false`, `History(group_id)` ← prior `current`, `GroupCount(current.tier) -= 1`, `UsedProof(proof_hash) := true`.

**Soundness statement:** any current member can deactivate. Once `active == false`, no further `update_commitment` or `deactivate_group` succeeds.

### 4.5 `update_vk`

Admin-only. State delta: `VK(tier)` or `UpdateVK(tier)` replaced. **Soundness statement:** see admin-trust assumption (T-5). After rotation, subsequent operations against the affected (tier, kind) verify under the new VK.

### 4.6 `set_restricted_mode`

Admin-only. Toggles a single bool; emits `RestrictedModeChanged`. No cryptographic effect.

### 4.7 `bump_group_ttl`

Permissionless. Requires `require_initialized`. Extends storage TTL on `Group(group_id)` and `History(group_id)`; does NOT touch `UsedProof(...)`. No state-value mutation.

### 4.8 `get_commitment` / `get_history`

Read-only. No state mutation.

---

## 5. Out-of-scope (acknowledged, not closed by this contract)

- **Pre-inclusion mempool replay.** Circuit-level operation-tag binding would close this; deferred per design §11.
- **Admin-key compromise.** No admin-rotation entrypoint — same parity as sep-democracy / sep-oligarchy. Admin-key loss requires fresh contract redeploy.
- **Cross-group replay against Anarchy MembershipCircuit** (sep-xxxx Audit Finding #1 — `group_id` not bound). Deferred per design §11.
- **Resource exhaustion.** Bounded by Soroban's per-tx fee model.
- **`member_count` accuracy.** Informational by design; the contract makes no claim about the field's correspondence to the actual member set.

---

## 6. Test pins

| Invariant | Test |
|---|---|
| SI-1 | `test_create_group_happy_path`, `test_create_group_rejects_invalid_proof`, `test_update_commitment_happy_path` |
| SI-2 | `test_update_commitment_rejects_stale_c_old`, `test_update_commitment_rejects_wrong_epoch_old` |
| SI-3 | `test_update_commitment_does_not_mutate_member_count` |
| SI-4 | (subgroup checks at constructor — implicit via `test_initialize` using `hash_to_g{1,2}` valid mocks) |
| SI-5 | (subgroup checks at proof verify — implicit via mock_proof) |
| SI-6 | `test_create_group_rejects_non_canonical_commitment`, `test_update_commitment_rejects_non_canonical_c_new` |
| SI-7 | `test_invalid_membership_vk_length_rejected`, `test_invalid_update_vk_length_rejected` |
| SI-8 | `test_update_commitment_rejects_replayed_proof` |
| SI-9 | `test_update_vk_requires_auth` |
| SI-10 | `test_create_group_restricted_mode_rejects_non_admin` |
| SI-12 | `test_update_commitment_rejects_wrong_epoch_old`, `test_update_commitment_happy_path` |
| SI-13 | `test_deactivate_already_inactive_group`, `test_update_commitment_rejects_inactive_group` |
| SI-14 | `test_get_commitment_returns_current_state` |
| SI-15 | `test_create_group_enforces_tier_group_limit` |
| SI-16 | (constructor idempotency — covered by `test_initialize` semantics) |
| SI-17 | `test_archive_entry_appends_and_prunes` |
| SI-18 | (covered by SI-17) |

`test_vectors_consistency` additionally CI-asserts that `test-vectors.json`'s pinned error codes, tier capacities, IC-point counts, and `MAX_GROUPS_PER_TIER` agree with the contract source.

---

## 7. Verification gaps

1. **Real Groth16 proofs against the v1 Anarchy circuits**. Tests use mock proofs (subgroup-valid points that fail pairing). The full success branches are exercised by v1 sep-xxxx integration tests already, since the Anarchy circuit and proof shapes are unchanged.
2. **Cross-platform vector agreement**. The v1 Anarchy 3-scalar payload is already pinned in `docs/cross-platform-test-vectors.json`; the per-type contract reuses these without modification.
3. **End-to-end testnet deploy** with `keyset-v2/` is the smoke-test gate; deferred until clients land Phase D routing to the new contract address.

---

## 8. Change-control

Edits to `contracts/sep-anarchy/src/lib.rs`, `test.rs`, or `test-vectors.json` MUST keep `test_vectors_consistency` green. Spec drift (new error code, new entrypoint, changed IC ordering) MUST be reflected in `test-vectors.json`, `impl_plan.md`, and these proof docs.
