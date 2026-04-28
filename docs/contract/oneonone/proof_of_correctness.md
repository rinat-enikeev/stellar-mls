# Proof of Correctness — `contracts/sep-oneonone`

**Scope:** the per-type 1v1 Soroban contract at `contracts/sep-oneonone/src/lib.rs`. Mirrors the structure of `docs/contract/{democracy,oligarchy,anarchy}/proof_of_correctness.md` but scoped to the smaller 1v1 surface.

## What "correct" means here

The contract implements the spec in `docs/oneonone-update-testnet-design.md`:

- Stores an immutable Poseidon commitment representing a 2-party group at creation time.
- Verifies a 3-IC Create-circuit proof against `(commitment, epoch=0)` at create.
- Verifies a 3-IC Membership-circuit proof against the stored `(commitment, epoch=0)` at read time.
- Allows only admin-gated VK rotation and admin-gated restricted-mode toggling.
- Has no entrypoint that mutates a stored `CommitmentEntry` (immutability).
- Has no entrypoint that admits a `deactivate_group`-style irreversible side effect (postmortem #153).

The `test_vectors_consistency` inline test enforces that the contract's `Error` enum, IC-point constants, and `MAX_GROUPS` ceiling match `test-vectors.json` byte-for-byte.

## Spec → impl map

| Spec section | Impl site |
|---|---|
| §5 Entrypoint surface (6 user-callable + constructor) | `lib.rs::SepOneOnOneContract` impl block |
| §5.1 Two distinct VKs (Membership + Create), same 3-IC public-input shape | `VkKind` enum + `MembershipVK` / `CreateVK` `DataKey` variants + shared `verify_proof` helper |
| §5.2 `caller` is a pure auth gate, not bound into the proof | `create_group` calls `caller.require_auth()` then `verify_proof(&vk, &proof, &commitment, 0)` — the verifier ignores `caller` |
| §6 Storage layout: `CommitmentEntry { commitment, epoch, timestamp }`; no `tier`, `active`, `member_count` | `CommitmentEntry` struct definition + `DataKey::Group` |
| §7 Errors (12 reachable, intentional gaps for sibling-contract alignment) | `Error` enum |
| §8 `MAX_GROUPS = 10_000` monotonic increment-only ceiling | `create_group` reads `GroupCount`, rejects if `>= MAX_GROUPS`, increments after success |
| §9 Wire format: `(commitment, epoch=0)` public inputs at both create and verify | `PublicInputs` struct + `create_group` rejects `public_inputs.epoch != 0` with `PublicInputsMismatch` |

## Invariants

| Invariant | How enforced |
|---|---|
| **I1: Immutability.** Once `Group(id)` is written, its `commitment` and `epoch` fields are never mutated. | No entrypoint writes to `DataKey::Group(id)` after `create_group`'s initial set. There is no `update_commitment` or `deactivate_group` entrypoint. |
| **I2: Epoch is always 0.** | `create_group` rejects `public_inputs.epoch != 0`. There is no path that increments `epoch`. |
| **I3: Group count never decreases.** | `GroupCount` is only ever incremented (in `create_group`); no decrement path exists. |
| **I4: Replay protection.** A proof byte-string used at one `create_group` cannot be reused at another within the `LEDGER_BUMP` (~30 days) TTL. | `check_proof_replay` checks `UsedProof(hash)` before verification; `record_proof` writes `UsedProof(hash)` after verification. Hash is `sha256(proof.a || proof.b || proof.c)` over the 384-byte uncompressed proof. |
| **I5: Both stored VKs have IC count = 3.** | `__constructor` and `update_vk` reject `vk.ic.len() != 3` with `InvalidVkLength`. |
| **I6: All stored VK points are subgroup-valid.** | `validate_vk_points` runs in `__constructor` and `update_vk`. |
| **I7: Stored commitment is canonical Fr.** | `create_group` calls `is_canonical_fr(&commitment)` and rejects with `InvalidCommitmentEncoding`. |
| **I8: Admin auth is required for all rotation / config entrypoints.** | `update_vk` and `set_restricted_mode` both call `admin.require_auth()` after loading the stored admin address. |
| **I9: Read-only verifier semantics.** `verify_membership` returns `Ok(false)` on a verification failure, never `Err(InvalidProof)`. | `verify_membership` calls `verify_proof(...)` and returns the `bool` directly inside `Ok(_)`. |

## Soundness of the post-#153 framing

The postmortem-#153 attack on `deactivate_group` was: any observer of a read-only `verify_membership` call could replay the leaked proof bytes as `deactivate_group`, irreversibly freezing the group. The mitigation in `sep-democracy` / `sep-oligarchy` / `sep-anarchy` was to remove `deactivate_group` entirely — leaving the leaked proof with no state-mutating membership-VK-keyed entrypoint to attack.

In `sep-oneonone`, the same framing applies but is even stronger by construction: the **only** state-mutating entrypoint that consumes a Groth16 proof is `create_group`, and that entrypoint requires a fresh `group_id` (with no prior `Group(id)` entry, else `GroupAlreadyExists`). A leaked Membership-VK proof from `verify_membership` cannot mutate any existing group's state because:

1. There is no entrypoint that accepts a Membership-VK proof to mutate state.
2. `create_group` verifies under the **Create** VK, not the Membership VK. A Membership-VK proof would not pass the Create-VK pairing check (different `alpha`, `beta`, `gamma`, `delta` points).

A leaked Create-VK proof can be replayed by an attacker at `create_group`, but only with the same `commitment` the prover authored, against a different `group_id`. That's benign griefing (same shape as create-time front-running on the other three per-type contracts) — see §5.2 of the design doc and the `front_running_surface` block in `test-vectors.json`.

## What's NOT proved here

- That the `OneOnOneCreateCircuit` itself enforces "exactly 2 non-zero leaves at founding." That's a circuit-level property, not a contract-level one. The contract trusts the Create VK to encode the constraint.
- That the `MembershipCircuit` doesn't leak information about the witness. Standard Groth16 ZK property; out of scope for this contract.
- That clients pick unique `group_id` values. App-level concern.

## Test enforcement

- `test_vectors_consistency` pins the `Error` codes, IC counts, and `MAX_GROUPS`.
- `test_initialize` + `test_invalid_{membership,create}_vk_length_rejected` enforce I5.
- `test_create_group_rejects_non_canonical_commitment` enforces I7.
- `test_create_group_rejects_replayed_proof` enforces I4.
- `test_create_group_rejects_non_zero_epoch` enforces I2 (creation side).
- `test_create_group_enforces_group_count_limit` enforces I3 (ceiling side).
- `test_update_vk_requires_auth` enforces I8 (a sample; `set_restricted_mode` follows the same pattern and uses identical `require_auth` plumbing).
- `test_verify_membership_returns_false_on_invalid_proof` + `test_verify_membership_happy_path` enforce I9.
- I1 (immutability) and I6 (subgroup-valid stored points) are proved by **absence**: no test covers a mutation path or a non-subgroup-valid storage path because no such path exists. The corresponding negative tests are `test_invalid_{membership,create}_vk_length_rejected` (subgroup check sits behind length check; an invalid-subgroup VK with correct length triggers `InvalidPoint`, see Anarchy's analogous test) and the absence of an `update_commitment` entrypoint in the contract surface.
