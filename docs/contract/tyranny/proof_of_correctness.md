# Proof of Correctness — `contracts/sep-tyranny`

**Scope:** the per-type Tyranny Soroban contract at `contracts/sep-tyranny/src/lib.rs`. Mirrors `docs/contract/{democracy,oligarchy,anarchy,oneonone}/proof_of_correctness.md`.

## What "correct" means here

The contract implements the spec in `docs/tyranny-update-testnet-design.md`:

- Stores a Poseidon commitment representing a member tree, plus a Poseidon commitment to the admin's BLS pubkey, pinned at creation.
- Verifies a 4-IC Create-circuit proof against `(commitment, epoch=0, admin_pubkey_commitment)` at create.
- Verifies a 5-IC Update-circuit proof against `(c_old, epoch_old, c_new, admin_pubkey_commitment)` at each `update_commitment`, where `admin_pubkey_commitment` is contract-supplied from `current.admin_pubkey_commitment` (not on the wire).
- Verifies a 3-IC Membership-circuit proof against the stored `(commitment, epoch)` at read time.
- Allows only admin-gated VK rotation and admin-gated restricted-mode toggling.
- Has no `deactivate_group` (postmortem #153).
- Has no `update_admin` — `admin_pubkey_commitment` is invariant for a group's lifetime.

The `test_vectors_consistency` inline test enforces that the contract's `Error` enum, IC-point constants, tier capacities, and `MAX_GROUPS_PER_TIER` match `test-vectors.json` byte-for-byte.

## Spec → impl map

| Spec section | Impl site |
|---|---|
| §5 Entrypoint surface (8 + constructor) | `lib.rs::SepTyrannyContract` impl block |
| §5.1 Three VKs (3/4/5 IC, distinct circuits) | `VkKind` enum, three `verify_*_proof` helpers, three storage keys (`VK`, `CreateVK`, `UpdateVK`) |
| §5.2 `caller` is auth gate, not bound into the proof | `create_group` calls `caller.require_auth()` then `verify_create_proof(...)` — verifier ignores `caller` |
| §6 Storage layout: `CommitmentEntry { commitment, epoch, timestamp, tier, admin_pubkey_commitment }` | `CommitmentEntry` struct |
| §7 Errors (14 reachable, intentional gaps) | `Error` enum |
| §8 `MAX_GROUPS_PER_TIER = 10_000` monotonic increment-only | `create_group` reads `GroupCount(tier)`, rejects if `>= MAX_GROUPS_PER_TIER`, increments after success |
| §9 Wire formats: 3-input Membership, 3-input Create (epoch=0 enforced), 3-input Update (admin_pubkey_commitment supplied by contract) | `MembershipPublicInputs`, `CreatePublicInputs`, `UpdatePublicInputs` types + the per-entrypoint check sequences |

## Invariants

| ID | Invariant | How enforced |
|---|---|---|
| I1 | `admin_pubkey_commitment` is invariant under `update_commitment`. | `update_commitment` writes `new_entry.admin_pubkey_commitment = current.admin_pubkey_commitment`. The wire payload `UpdatePublicInputs` does not carry an `admin_pubkey_commitment` field — it cannot be supplied by the caller. |
| I2 | Epoch monotonic-by-1. | `update_commitment` rejects `wire.epoch_old != current.epoch` with `PublicInputsMismatch`; computes `new_epoch = current.epoch.checked_add(1)` and surfaces overflow as `InvalidEpoch`. |
| I3 | `GroupCount(tier)` is monotonic increment-only. | `create_group` increments after success; no entrypoint decrements. |
| I4 | Replay protection. | `check_proof_replay` reads `UsedProof(hash)` before verification; `record_proof` writes after verification. |
| I5 | All stored VKs have correct IC count + subgroup-valid points. | `__constructor` and `update_vk` check `vk.ic.len()` against `expected_ic` and call `validate_vk_points(vk)`. |
| I6 | Stored commitments + admin_pubkey_commitment are canonical Fr. | `create_group` checks `is_canonical_fr` on both before persistence; `update_commitment` checks `is_canonical_fr` on `c_new` before verification (`c_old` doesn't need re-checking — it was already canonical when stored). |
| I7 | Admin auth required for rotation/config. | `update_vk` and `set_restricted_mode` both load `Admin` and call `admin.require_auth()`. |
| I8 | Read-only verifier semantics. | `verify_membership` returns `Ok(false)` on verify failure, never `Err(InvalidProof)`. |

## Soundness of the post-#153 framing

`deactivate_group`'s post-#153 attack pattern was: any observer of an honest member's `verify_membership` call could replay the leaked proof bytes as `deactivate_group` against the same `(commitment, epoch)` public inputs.

Tyranny ships without `deactivate_group`. A leaked Membership-VK proof (3-IC) cannot be replayed at `update_commitment` (5-IC, different VK family) — different `alpha`/`beta`/`gamma`/`delta` and a different IC layout. The post-#153 attack surface is empty by construction.

A leaked Create-VK proof can be replayed at `create_group`, but only with the same `commitment` + `admin_pubkey_commitment` the prover authored, against a different `group_id` (the contract requires fresh `group_id`; existing entry triggers `GroupAlreadyExists`). Benign griefing — same shape as create-time front-running on the other per-type contracts.

A leaked Update-VK proof binds a specific `(c_old, epoch_old, c_new)` and is locked to a specific `current.admin_pubkey_commitment`. An attacker replaying it against a different group fails because `current.commitment != c_old` (PublicInputsMismatch) or `current.admin_pubkey_commitment` differs (verify fails). Replaying against the same group fails because `record_proof` consumed the nullifier post-verify.

## What's NOT proved here

- **Circuit-level soundness of `TyrannyCreateCircuit`** (proves admin's BLS pubkey hash matches `admin_pubkey_commitment` AND the prover knows the secret key). The contract trusts the Create VK to encode this constraint.
- **Circuit-level soundness of `TyrannyUpdateCircuit`** (proves admin secret key + new tree differs from old by ≤1 leaf). Trusted at the VK.
- **Privacy of the witness** — standard Groth16 ZK property.
- **Soroban-host correctness.**
- **Operational soundness of `MAX_GROUPS_PER_TIER`.** Documented as a known capacity follow-up.

## Test enforcement (cross-reference: `src/test.rs`)

- `test_vectors_consistency` pins `Error` codes, IC counts, `tier_capacity()`, `MAX_GROUPS_PER_TIER`, test count.
- `test_initialize` + `test_invalid_{membership,create,update}_vk_length_rejected` enforce I5.
- `test_create_group_rejects_non_canonical_{commitment,admin_pubkey_commitment}` + `test_update_commitment_rejects_non_canonical_c_new` enforce I6.
- `test_create_group_rejects_replayed_proof` + `test_update_commitment_rejects_replayed_proof` enforce I4.
- `test_update_commitment_rejects_wrong_epoch_old` enforces I2 (mismatch path); the overflow path (`InvalidEpoch`) is structurally covered by `checked_add` + the same `wire.epoch_old != current.epoch` rejection on the boundary case.
- `test_create_group_enforces_tier_group_limit` enforces I3 (ceiling side); no decrement test because no decrement path exists.
- `test_update_vk_requires_auth` + `test_set_restricted_mode_requires_auth` enforce I7.
- `test_update_commitment_does_not_mutate_admin_pubkey_commitment` enforces I1 — sets up a group, attempts an `update_commitment` (fails at the verifier with mock proofs), reads back state, asserts `admin_pubkey_commitment` unchanged. The same assertion would hold under a successful update per the code at `update_commitment`'s `new_entry` construction.
- `test_verify_membership_happy_path` + `test_verify_membership_returns_false_on_invalid_proof`-style behavior pin I8 (verify_membership doesn't `Err(InvalidProof)`; happy path with mock proof returns `Ok(false)`).
