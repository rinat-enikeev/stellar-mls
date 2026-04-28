# Implementation plan: `contracts/sep-oneonone/`

Fourth and last in the per-type Soroban contract family. Mirrors the structure of `docs/contract/{democracy,oligarchy,anarchy}/impl_plan.md` but scoped tighter — 1v1 is the smallest of the four.

## Source-of-truth references

- Design doc: [`docs/oneonone-update-testnet-design.md`](../../oneonone-update-testnet-design.md)
- Postmortem context: [`docs/postmortem-deactivate-group-frontrun.md`](../../postmortem-deactivate-group-frontrun.md)
- Contract code: `contracts/sep-oneonone/src/lib.rs`
- ABI pin: `contracts/sep-oneonone/test-vectors.json`
- Inline tests: `contracts/sep-oneonone/src/test.rs`

## Surface

8 entrypoints total (1 constructor + 6 user-callable + 1 read-only state lookup that's grouped with queries):

| Entrypoint | Auth | Notes |
|---|---|---|
| `__constructor(env, admin, vk_membership, vk_create)` | `admin.require_auth()` | One-time. Both VKs MUST be 3-IC. |
| `update_vk(env, kind, new_vk)` | `admin.require_auth()` | Rotates Membership or Create VK. NO `tier` parameter. |
| `set_restricted_mode(env, restricted)` | `admin.require_auth()` | Toggles the create-call gate. |
| `bump_group_ttl(env, group_id)` | none | Permissionless. The only ongoing lifecycle event for a 1v1 group. |
| `create_group(env, caller, group_id, commitment, proof, public_inputs)` | `caller.require_auth()` + restricted-mode gate | Verifies under Create VK. epoch must be 0. |
| `verify_membership(env, group_id, proof, public_inputs) → bool` | none | Verifies under Membership VK. Read-only; returns `Ok(false)` on bad proof. |
| `get_commitment(env, group_id) → CommitmentEntry` | none | Read-only state lookup. |

No `update_commitment`, no `deactivate_group`, no `get_history` (history is empty by definition).

## What carries forward from sep-anarchy

- Mock-VK / mock-proof testing harness (`valid_g1` / `valid_g2` via `hash_to_g{1,2}`, then `mock_membership_vk` / `mock_create_vk`).
- `validate_vk_points`, `validate_proof_points`, `is_canonical_fr`, `u64_to_u256_be` helpers — verbatim from sep-anarchy.
- `proof_hash`, `check_proof_replay`, `record_proof` — verbatim.
- 3-IC pairing-check function — same shape as sep-anarchy's `verify_membership_proof` but re-named `verify_proof` and reused for both Membership and Create VKs (public-input shape is identical).
- `RestrictedModeChanged` event shape.
- `LEDGER_THRESHOLD` / `LEDGER_BUMP` constants.

## What's specifically dropped

- `Update` VK kind, `verify_update_proof`, `UpdateCommitmentPublicInputs`, `update_commitment` entrypoint, `CommitmentUpdated` event.
- `archive_entry` helper, `History(group_id)` data key, `get_history` entrypoint, `HISTORY_WINDOW` constant.
- `tier` parameter on entrypoints, `VK(tier)` per-tier array → single `MembershipVK` data key, `UpdateVK(tier)` array → replaced by single `CreateVK`, `GroupCount(tier)` per-tier array → single `GroupCount` instance.
- `tier`, `active`, `member_count` fields on `CommitmentEntry` (down from 6 fields to 3).
- `GroupDeactivated` event, `deactivate_group` entrypoint (postmortem #153).
- Error variants: `InvalidTier`, `GroupInactive`, `InvalidEpoch`, `Reserved3`, `OneOnOneImmutable`, `Invalid1v1Tier`, `InvalidThreshold`, `GroupTypeMismatch`, `TierGroupLimitReached` (renamed `GroupCountLimitReached`), `GroupStillActive`. 12 reachable variants vs Anarchy's 17.

## Phases

| Phase | Status |
|---|---|
| Phase A — `OneOnOneCreateCircuit` VK availability | Open. PR-Y ships with mock-VK testing wiring; deploy script accepts `FIXTURE_DIR` of dev VKs. Real ceremony decision (separate ceremony vs. add to keyset-v2) deferred. |
| Phase B — FFI / `generateOneOnOneCreateProof` | Out of scope for PR-Y. Bounded follow-up. |
| Phase C — contract crate | **PR-Y (this work).** Done. |
| Phase D — clients (`swift-mls`, iOS, Android wiring) | Out of scope. Follow-up after Phase A circuits + Phase B FFI land. |
| Phase E — production ceremony | Resolved during Phase A. |

## Verification

- `cargo test --manifest-path contracts/sep-oneonone/Cargo.toml --lib` — 26/26 passing.
- `stellar contract build --manifest-path contracts/sep-oneonone/Cargo.toml` — produces `sep_oneonone_contract.wasm`.
- `bash -n scripts/deploy_sep_oneonone_testnet.sh` — clean syntax.
- `jq . contracts/sep-oneonone/test-vectors.json` — valid JSON.

## Test coverage

26 inline tests, mapped 1:1 to entries in `test-vectors.json#tests_to_implement.tests`:

- 3 initialization (happy + 2 IC-arity rejections).
- 10 `create_group` (happy, duplicate id, non-canonical commitment, invalid proof, restricted-mode admin-only, group-count cap, replay, public-input commitment mismatch, non-zero epoch, non-canonical public-input commitment).
- 5 `verify_membership` (happy returns Ok(false) on mock proof, wrong commitment, wrong epoch, unknown group, returns-false-on-invalid-proof pin).
- 4 `update_vk` (requires_auth, rotates membership, rotates create, invalid VK length).
- 3 queries (get_commitment happy + unknown, bump_group_ttl).
- 1 ABI consistency (`test_vectors_consistency`).

## LOC

- `src/lib.rs`: 589 LOC (vs. sep-anarchy 975, sep-democracy ~1000, sep-oligarchy ~1250).
- `src/test.rs`: ~600 LOC.
- `test-vectors.json`: ~140 lines.

## Out of scope (handled elsewhere)

- Client wiring (`swift-mls` SDK, iOS, Android UI). Phase D follow-up.
- `OneOnOneCreateCircuit` definition and ceremony. Phase A.
- v1 sep-xxxx 1v1 client compatibility. None — clean-slate per-type contract.
