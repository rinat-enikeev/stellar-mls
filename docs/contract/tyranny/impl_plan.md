# Implementation plan: `contracts/sep-tyranny/`

Fifth in the per-type Soroban contract family. Mirrors the structure of `docs/contract/{democracy,oligarchy,anarchy,oneonone}/impl_plan.md`.

## Source-of-truth references

- Design doc: [`docs/tyranny-update-testnet-design.md`](../../tyranny-update-testnet-design.md)
- Postmortem context: [`docs/postmortem-deactivate-group-frontrun.md`](../../postmortem-deactivate-group-frontrun.md)
- Contract code: `contracts/sep-tyranny/src/lib.rs`
- ABI pin: `contracts/sep-tyranny/test-vectors.json`
- Inline tests: `contracts/sep-tyranny/src/test.rs`

## Surface

9 entrypoints (1 constructor + 8 user-callable):

| Entrypoint | Auth | Notes |
|---|---|---|
| `__constructor` | admin | 9 VKs (3 Membership × 3 tiers + 3 Create × 3 + 3 Update × 3) |
| `update_vk(kind, tier, new_vk)` | admin | kind ∈ {Membership, Create, Update}; tier ≤ 2 |
| `set_restricted_mode(restricted)` | admin | |
| `bump_group_ttl(group_id)` | none | Permissionless |
| `create_group(caller, group_id, commitment, tier, admin_pubkey_commitment, proof, public_inputs)` | caller + restricted gate | Verifies under Create VK; epoch=0 |
| `update_commitment(group_id, proof, public_inputs)` | none (proof IS auth) | Verifies under Update VK; admin_pubkey_commitment is contract-supplied |
| `verify_membership(group_id, proof, public_inputs) → bool` | none | Read-only |
| `get_commitment(group_id) → CommitmentEntry` | none | Read-only |
| `get_history(group_id, max_entries) → Vec<CommitmentEntry>` | none | Read-only rolling window |

No `deactivate_group` (postmortem #153). No `update_admin` in v0.

## Invariants (cross-reference: proof_of_correctness.md)

| ID | Invariant |
|---|---|
| I1 | `admin_pubkey_commitment` is invariant under `update_commitment` |
| I2 | Epoch increments by exactly 1 per successful update; `checked_add` overflow → `InvalidEpoch` |
| I3 | `GroupCount(tier)` is monotonic increment-only (no deactivate path) |
| I4 | Replay protection via global `UsedProof(proof_hash)` ~30-day TTL |
| I5 | All stored VKs have correct IC count (3/4/5) and subgroup-valid points |
| I6 | Stored commitments + admin_pubkey_commitment are canonical Fr |
| I7 | Admin auth required for VK rotation + restricted-mode toggle |

## What carries forward from sep-anarchy

Verbatim helpers: `validate_vk_points`, `validate_proof_points`, `is_canonical_fr`, `u64_to_u256_be`, `proof_hash`, `check_proof_replay`, `record_proof`, `archive_entry`, `bump_group`, `RestrictedModeChanged` event shape, `LEDGER_THRESHOLD` / `LEDGER_BUMP` / `HISTORY_WINDOW` constants. The 3-IC `verify_membership_proof` is also verbatim from sep-anarchy.

## What's specifically new

- `admin_pubkey_commitment` field on `CommitmentEntry` — Poseidon hash of admin's BLS pubkey, pinned at create.
- 4-IC `verify_create_proof` — public inputs `(commitment, epoch, admin_pubkey_commitment)`.
- 5-IC `verify_update_proof` — public inputs `(c_old, epoch_old, c_new, admin_pubkey_commitment)`. The 4th input is contract-supplied from `current.admin_pubkey_commitment`, NOT on the wire.
- Three VK families per tier: Membership (3-IC) + Create (4-IC) + Update (5-IC). 9 VKs at constructor time.
- `CreatePublicInputs` (3 fields) wire type, distinct from `MembershipPublicInputs` (2 fields).

## What's specifically dropped

- `deactivate_group` entrypoint + `GroupDeactivated` event (postmortem #153).
- `active: bool` field on `CommitmentEntry`.
- `member_count`, `occupancy_commitment`, `threshold_numerator`, `admin_root` fields (Tyranny is value-agnostic to count, no quorum, no admin tree).
- `GroupInactive`, `OneOnOneImmutable`, `Invalid1v1Tier`, `MissingAdminRoot`, `AdminRootMismatch`, `InvalidThreshold`, `MemberCountMismatch`, `MemberCountOutOfRange`, `GroupTypeMismatch`, `InvalidInitialMembership`, `Reserved3`, `Reserved22`, `GroupStillActive` error variants.

## Phases

| Phase | Status |
|---|---|
| Phase A — TyrannyCreateCircuit + TyrannyUpdateCircuit | **Open** (real Phase A blocker; circuits NEW vs. keyset-v2). Membership circuit may reuse keyset-v2's standard 3-IC membership circuit. |
| Phase B — `generateTyrannyCreateProof` + `generateTyrannyUpdateProof` FFI | Open (after Phase A) |
| Phase C — contract crate | **PR-Y (this work).** Done. |
| Phase D — clients (`swift-mls`, iOS, Android wiring) | Out of scope |
| Phase E — production ceremony | Resolves during Phase A |

## Verification

- `cargo test --manifest-path contracts/sep-tyranny/Cargo.toml --lib` — 37/37 passing.
- `stellar contract build --manifest-path contracts/sep-tyranny/Cargo.toml` — produces `sep_tyranny_contract.wasm`.
- `bash -n scripts/deploy_sep_tyranny_testnet.sh` — clean syntax.
- `jq . contracts/sep-tyranny/test-vectors.json` — valid JSON.

## Test coverage

41 inline tests, mapped 1:1 to entries in `test-vectors.json#tests_to_implement.tests`:

- 4 initialization (happy + 3 IC-arity rejections).
- 11 `create_group` (happy, invalid tier, duplicate, non-canonical commitment, non-canonical admin_pubkey_commitment, invalid proof, restricted-mode {non-admin rejected, admin allowed}, group-count cap, replay protection, public-input mismatch).
- 8 `update_commitment` (happy, stale c_old, wrong epoch_old, non-canonical c_new, replayed proof, unknown group, admin_pubkey_commitment invariance, epoch overflow).
- 4 `verify_membership` (happy, wrong commitment, wrong epoch, unknown group).
- 6 admin entrypoints (update_vk_requires_auth, set_restricted_mode_requires_auth, rotates Membership/Create/Update, invalid_tier).
- 7 queries (get_commitment {happy, unknown}, bump_group_ttl {happy, unknown}, get_history {most-recent slice, full-when-max-exceeds, unknown}).
- 1 ABI pin (`test_vectors_consistency`).

## LOC

Order of magnitude (counts drift slightly with comment rewrites):

- `src/lib.rs`: ~1000 LOC (vs. sep-anarchy ~975, sep-democracy ~1000, sep-oligarchy ~1250, sep-oneonone ~590).
- `src/test.rs`: ~1050 LOC.
- `test-vectors.json`: ~270 lines.

## Out of scope

- Phase A circuit work (`TyrannyCreateCircuit`, `TyrannyUpdateCircuit`).
- Phase B FFI proof-generation paths.
- Phase D client wiring.
- `update_admin` (admin rotation) — v1 follow-up if needed.
- Member-count hiding (`occupancy_commitment`) — v1 follow-up if privacy requirements emerge.
