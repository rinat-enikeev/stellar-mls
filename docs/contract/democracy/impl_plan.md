# Plan: implement `contracts/sep-democracy` from scratch

## Context

The user is moving from one monolithic `contracts/sep-xxxx` (Anarchy + 1v1 + Democracy + Oligarchy multiplexed via a `group_type` discriminator) to **per-type contracts** — eventually four independent Soroban crates, one per governance type. This task creates the Democracy one.

The Democracy v2 design lives in [`docs/democracy-update-testnet-design.md`](../../democracy-update-testnet-design.md) (currently v0.5.1). Hidden member counts via occupancy commitment, configurable threshold, 5-scalar wire payload, threshold supplied by contract from storage at verify time, single-signer subset (multi-signer is Phase F follow-up).

The new contract is **clean-slate v2** — not a migration of the existing v1 democracy logic. No `member_count` storage, no `group_type` discriminator (the contract's address IS the discriminator now that types are separated), no polymorphic dispatch. The single-type architecture also means the §4.7.4 "polymorphic `update_commitment`" privacy benefit collapses (the contract address itself reveals group type to chain observers); the §3.4 residual table already accepted this kind of leak as same-publicness-class as `tier`.

Order is **vectors → contract → tests → PR**. Per the user's `no backward compatibility, redeploy is acceptable` framing: ship the v2 wire shape directly, no v1 intermediate.

## Scope cut from the existing sep-xxxx baseline

The existing crate has ~5,472 LOC of `lib.rs`. The Democracy-only sibling drops:
- 1v1 entrypoints, `Invalid1v1Tier` error, `OneOnOneImmutable` error
- All Oligarchy entrypoints (`create_oligarchy_group`, `update_admin_commitment`, `get_admin_root`, `get_admin_epoch`, `AdminSetCommitment`/`AdminTreeEpoch` storage, `OligarchyAdminSetUpdated` event, `OligarchyAdminUpdatePublicInputs`), `MissingAdminRoot`/`AdminRootMismatch`/`Reserved22` errors
- Anarchy entrypoints (separate contract eventually)
- `group_type` field from storage entries (always Democracy here)
- `UnknownGroupType` error (single-type contract)
- v1 democracy artifacts (`member_count` storage field, `MemberCountMismatch`/`MemberCountOutOfRange` errors, `DemocracyUpdatePublicInputs` v1 type)
- ~900 LOC of inline tests for non-Democracy types

What lands instead is the v2 design: occupancy-commitment storage, parameterized threshold, 5-scalar wire shape.

## Repo touch list

| Path | Action | Notes |
|---|---|---|
| `contracts/sep-democracy/Cargo.toml` | Create | `crate-type = ["cdylib"]`, `soroban-sdk = "25.3.0"` (matches existing), `lib.name = "sep_democracy_contract"`, same `[profile.release]` as `sep-xxxx` |
| `contracts/sep-democracy/test-vectors.json` | **Create FIRST** | Per-contract vectors (user choice). Mirror `docs/soroban-contract-test-vectors.json` shape but Democracy-only. See §A below. |
| `contracts/sep-democracy/src/lib.rs` | Create | The contract. ~1,500–2,000 LOC. See §B. |
| `contracts/sep-democracy/test_snapshots/test/` | Auto-generated | `#[contracttest]` snapshots emitted by the test runs in §C; checked in. |
| `scripts/deploy_sep_democracy_testnet.sh` | Create | Sibling of `scripts/deploy_sep_xxxx_testnet.sh` — points at `contracts/sep-democracy/Cargo.toml`, uses `keyset-democracy-dev/` for VK fixtures, only installs Democracy update VKs (3 tiers). |
| `docs/democracy-update-testnet-design.md` | Update §1 + §6 | One paragraph noting that per-type contract architecture lands first; §4.7.4 polymorphic-dispatch becomes informational (no contract has it; contract address is the discriminator). |
| Repo root | No change | No workspace at root — each contract crate is independent (confirmed via exploration). |

## A. `contracts/sep-democracy/test-vectors.json` — write this first

This file is the **canonical contract-ABI pin**. Every value is derivable from the design doc §4.6 + §4.7 or from this contract's source. The contract implementation must conform; tests assert against it.

Sections (mirrors `docs/soroban-contract-test-vectors.json` but stripped to Democracy):

1. **`error_codes`** — slimmed enum:
   - `NotInitialized=1`, `AlreadyInitialized=2`, `Reserved3=3`, `GroupAlreadyExists=4`, `GroupNotFound=5`, `GroupInactive=6`, `InvalidProof=7`, `InvalidTier=8`, `InvalidVkLength=9`, `PublicInputsMismatch=10`, `InvalidEpoch=11`, `ProofReplay=12`, `TierGroupLimitReached=13`, `AdminOnly=14`, `InvalidCommitmentEncoding=15`, `InvalidPoint=26`, `GroupStillActive=27`, `InvalidThreshold=28` (per design v0.5.1)
   - **Drop**: `UnknownVkKind=16` (VkKind exhaustively matched; unreachable — removed in PR #148 review chunk 1 #4), `OneOnOneImmutable=17`, `UnknownGroupType=18`, `MissingAdminRoot=19`, `AdminRootMismatch=20`, `DemocracyInputMissing=21`, `Reserved22`, `Invalid1v1Tier=23`, `MemberCountMismatch=24`, `MemberCountOutOfRange=25`, `GroupTypeMismatch=29` (no polymorphic dispatch in single-type contract)
2. **`tier`** — same Small/Medium/Large mapping as the shared file; `rejected_tiers` produces `InvalidTier`.
3. **`vk_kind_enum`** — only two: `Membership` (3 IC points: `[base, commitment, epoch]`) and `Update` (the v2 democracy update — 7 IC points: `[base, c_old, epoch_old, c_new, occupancy_commitment_old, occupancy_commitment_new, threshold_numerator]`). No `UpdateByType` enum since group_type is constant for this contract.
4. **`create_group_validation`** — single arm (no per-type table): `tier ∈ {0,1,2}`; `threshold_numerator ∈ [1,100]` per §4.7.6; rejection table for `0`, `101`, `255`. No `member_count` field at all.
5. **`domain_tags`** — same as the shared file: `DOMAIN_MEMBER=1`, `DOMAIN_TOMBSTONE=2`, `DOMAIN_OCCUPANCY=3`. Pinned per §10.1 Phase-A blocker.
6. **`occupancy_bitmap_fold`** — verbatim copy of the shared file's section. `BITS_PER_FELT=252`, scalar count by tier (1/2/9), bit-packing rule, tier-0/1/2 examples (already verified accurate against the contract source).
7. **`update_commitment_public_inputs_wire_format`** — single variant (Democracy-only): `wire_fields = [c_old, epoch_old, c_new, occupancy_commitment_old, occupancy_commitment_new]` (5 scalars on the wire). `groth16_public_inputs = [c_old, epoch_old, c_new, occupancy_commitment_old, occupancy_commitment_new, threshold_numerator]` (6, the 6th from contract storage). VK kind: `Update (7 IC points)`. Pin the canonical ordering.
8. **`storage_layout`** — `CommitmentEntry { commitment: BytesN<32>, epoch: u64, timestamp: u64, tier: u32, active: bool, occupancy_commitment: BytesN<32>, threshold_numerator: u32 }`. **No `group_type` field** (always Democracy). **No `member_count` field** (replaced by `occupancy_commitment`). DataKeys: `Admin`, `RestrictedMode` (bool, instance), `VK(tier)`, `UpdateVK(tier)` (no group_type sub-discriminator since contract is Democracy-only), `Group(group_id)`, `History(group_id)`, `UsedProof(proof_hash)`, `GroupCount(tier)`. The cross-group cap is `MAX_GROUPS_PER_TIER=10000` (separate from per-tier slot capacity 32/256/2048).
9. **`epoch_invariant`** — same monotonic-by-1 rule.
10. **`proof_replay_protection`** — same `sha256(proof.a || proof.b || proof.c)` global-nullifier scope, `LEDGER_BUMP` TTL.
11. **`membership_count_constraints`** — verbatim copy of the v2 popcount table (floor `≥2`, single-leaf delta `≤1`, ceiling implicit, quorum `100·K ≥ T·m_old` with the corrected single-signer-by-threshold table). The single-signer cutoff table from design v0.5.1 §4.2 is the definitive entry here.
12. **`tests_to_implement`** — list of test names that the inline `#[cfg(test)] mod test` will cover (see §C for the full list). This makes the vectors file the planning anchor for §C — every named test in this section MUST have a matching inline test.

## B. `contracts/sep-democracy/src/lib.rs`

Mirrors `contracts/sep-xxxx/src/lib.rs` patterns precisely (idiomatic Soroban): `#![no_std]`, `#[contract]`, `#[contracterror]`, `#[contracttype]`, `#[contractimpl]`. Single struct `SepDemocracyContract`.

Modules (all inline in one file, matching the existing convention):

1. **Constants** — `MEMBERSHIP_IC_POINTS = 3`, `UPDATE_IC_POINTS = 7`, `MAX_TIER = 2`, `LEDGER_BUMP_AMOUNT`, etc.
2. **`Error` enum** — slimmed per §A.1.
3. **Storage types** —
   - `pub struct CommitmentEntry { commitment, epoch, timestamp, tier, active, occupancy_commitment, threshold_numerator }`
   - `pub struct UpdateCommitmentPublicInputs { c_old: BytesN<32>, epoch_old: u64, c_new: BytesN<32>, occupancy_commitment_old: BytesN<32>, occupancy_commitment_new: BytesN<32> }` — the wire payload (5 fields).
   - `pub struct VerifyMembershipPublicInputs { commitment: BytesN<32>, epoch: u64 }`
   - `pub struct Groth16Proof { a: BytesN<96>, b: BytesN<192>, c: BytesN<96> }` (compressed)
   - `pub struct VerifyingKey { vk_alpha_g1, vk_beta_g2, vk_gamma_g2, vk_delta_g2, ic: Vec<BytesN<96>> }`
4. **`DataKey` enum** — `Admin`, `VK(u32)`, `UpdateVK(u32)`, `Group(BytesN<32>)`, `History(BytesN<32>)`, `UsedProof(BytesN<32>)`, `TierCount(u32)`. **No `UpdateVK(tier, group_type)` — the `group_type` second key is dropped since contract is Democracy-only.**
5. **`SepDemocracyContract` impl** — entrypoints:

   - `__constructor(env, admin, vk_small, vk_medium, vk_large, uvk_small, uvk_medium, uvk_large)` — exactly parallel to existing constructor, but only Democracy update VKs (no Oligarchy `OligarchyUpdateVK`, no separate Membership/Anarchy types). Validates each VK's IC length (3 for membership, 7 for update), runs the BLS subgroup check on each G1/G2 element, stores in persistent storage. Sets admin. Atomic constructor is the only init path — no separate `initialize` entrypoint (dropped in PR #148 review chunk 2 #11; redundant on Soroban ≥21).
   - `update_vk(env, kind, tier, vk)` — admin-only (panics via `require_auth(admin)`). `kind ∈ {Membership, Update}`; rotates the VK at `(tier)`. Used for testnet dev-VK rotations per design §4.6 fingerprint-coordination triplet.
   - `set_restricted_mode(env, restricted)` — admin-only. When `restricted=true`, `create_group` rejects non-admin callers with `Error::AdminOnly` (after `caller.require_auth()`). Default is unrestricted. Stored in `DataKey::RestrictedMode` (instance).
   - `create_group(env, group_id, commitment, tier, threshold_numerator, occupancy_commitment_initial, proof, public_inputs)` — validates `tier ≤ 2`, `threshold_numerator ∈ [1, 100]`, group_id not already used, `commitment` and `occupancy_commitment_initial` are canonical Fr encodings, public_inputs match the proof's claim, proof verifies under the membership VK at `tier`, proof is fresh (replay check), tier capacity not exceeded. On success: write `CommitmentEntry`, append to `History`, increment `TierCount`, record proof. Emits `GroupCreated` event.
   - `update_commitment(env, group_id, proof, public_inputs: UpdateCommitmentPublicInputs)` — validates: group exists and active; `epoch_old == current.epoch`; `c_old == current.commitment`; `occupancy_commitment_old == current.occupancy_commitment`; `c_new` and `occupancy_commitment_new` are canonical Fr; epoch overflow check; proof not replayed; **reads `current.threshold_numerator` from storage** and supplies it as the 6th Groth16 public input (alongside the 5 wire-supplied scalars in canonical order); proof verifies under the update VK at `tier`. On success: bump epoch by 1, write new `commitment` + `occupancy_commitment` (threshold unchanged — fixed at create), append to `History`, record proof. Emits `CommitmentUpdated` event.
   - `verify_membership(env, group_id, proof, public_inputs: VerifyMembershipPublicInputs)` — read-only. Validates `commitment` and `epoch` match `current`, proof verifies under membership VK at `tier`. Returns `Result<(), Error>`.
   - `deactivate_group(env, group_id, proof, public_inputs: VerifyMembershipPublicInputs)` — like `verify_membership` but writes `active = false`. Verifies proof of membership in the **current** group state (any member can deactivate, per the existing convention from `sep-xxxx`). On success appends the prior active state to history before flipping the live entry inactive (mirrors `update_commitment`'s archive-then-write pattern; bug fix from PR #148 review chunk 2 #1).
   - `get_commitment(env, group_id) -> Result<CommitmentEntry, Error>` — read-only getter.
   - `get_history(env, group_id) -> Result<Vec<CommitmentEntry>, Error>` — read-only getter.
   - `bump_group_ttl(env, group_id)` — keep the `UsedProof` nullifier alive for long-lived groups (existing pattern).
6. **Internal helpers** —
   - `proof_hash(proof) -> BytesN<32>` — `sha256(proof.a || proof.b || proof.c)`.
   - `tier_capacity(tier) -> u32` — 32 / 256 / 2048.
   - `verify_groth16_proof(env, vk, proof, public_inputs)` — calls Soroban's BLS host functions (pairing check). Same as existing.
   - `record_proof(env, proof_hash)` / `check_proof_replay(env, proof_hash)`.
   - `load_group(env, group_id)`, `store_group(env, group_id, entry)`.
   - `is_canonical_fr(bytes) -> bool` — `Fr::from_bytes ∘ Fr::to_bytes` round-trip check.

## C. Inline tests (`#[cfg(test)] mod test`)

Mirrors the `sep-xxxx` test pattern: inline `mod test` block at the bottom of `lib.rs`, `setup_env()` helper, `mock_*` constructors. Snapshots (`test_snapshots/test/test_*.1.json`) auto-generated by `#[contracttest]`.

The `test-vectors.json` file's `tests_to_implement` section IS the test list. Each test name there gets one matching inline test:

- `test_initialize` — constructor accepts valid VKs, stores admin.
- `test_invalid_membership_vk_length_rejected` — IC length ≠ 3 panics.
- `test_invalid_update_vk_length_rejected` — IC length ≠ 7 panics.
- `test_create_group_happy_path` — valid create succeeds, storage entry present, TierCount incremented.
- `test_create_group_rejects_invalid_tier` — tier > 2 → `InvalidTier`.
- `test_create_group_rejects_invalid_threshold_zero` — `threshold_numerator = 0` → `InvalidThreshold`.
- `test_create_group_rejects_invalid_threshold_above_100` — `threshold_numerator = 101` → `InvalidThreshold`.
- `test_create_group_accepts_threshold_50` (default).
- `test_create_group_accepts_threshold_67` (supermajority).
- `test_create_group_accepts_threshold_100` (unanimous).
- `test_create_group_rejects_duplicate_group_id` — second create on same id → `GroupAlreadyExists`.
- `test_create_group_rejects_non_canonical_commitment` — invalid Fr encoding → `InvalidCommitmentEncoding`.
- `test_create_group_rejects_non_canonical_occupancy_commitment`.
- `test_create_group_rejects_invalid_proof` — bad proof → `InvalidProof`.
- `test_create_group_restricted_mode_rejects_non_admin` — admin enables restricted mode; non-admin caller hits the `caller != admin` branch → `AdminOnly` (sibling of `test_update_vk_requires_auth`'s no-auth panic gate).
- `test_create_group_enforces_tier_group_limit` — `GroupCount(tier) ≥ MAX_GROUPS_PER_TIER (10000)` → `TierGroupLimitReached`.
- `test_update_commitment_happy_path` — valid update advances epoch, writes new commitment + occupancy_commitment, threshold_numerator unchanged.
- `test_update_commitment_rejects_stale_c_old` — `c_old != current.commitment` → `PublicInputsMismatch`.
- `test_update_commitment_rejects_stale_occupancy_commitment_old`.
- `test_update_commitment_rejects_wrong_epoch_old` — `epoch_old != current.epoch` → `PublicInputsMismatch`.
- `test_update_commitment_rejects_non_canonical_c_new`.
- `test_update_commitment_rejects_non_canonical_occupancy_commitment_new`.
- `test_update_commitment_rejects_replayed_proof` — same proof twice → `ProofReplay`.
- `test_update_commitment_rejects_inactive_group` — `active = false` → `GroupInactive`.
- `test_update_commitment_rejects_unknown_group` → `GroupNotFound`.
- `test_update_commitment_threshold_supplied_from_storage` — explicit verification that the contract reads `current.threshold_numerator` and includes it as the 6th public input. Sets up a group with `threshold_numerator = 67`, submits a proof valid only under that threshold; assert verify passes. Then a second case with `threshold_numerator = 50` and the same proof; assert verify fails (the chain-supplied threshold differs from what the proof was constructed for).
- `test_update_commitment_threshold_mismatch_rejected_v2` (the §6 step C2 contract test from design v0.5.1) — equivalent of the above mismatch case, with the test name design v0.5.1 §7.1 explicitly cites for cross-platform discoverability.
- `test_update_commitment_threshold_tie_passes_v2` — design v0.5.1 §4.7.6 tie semantics: `threshold = 50, m_old = 4, K = 2` (200 ≥ 200 passes). Companion to the prover-side tie test.
- `test_verify_membership_happy_path`.
- `test_verify_membership_rejects_wrong_commitment` → `PublicInputsMismatch`.
- `test_verify_membership_rejects_wrong_epoch`.
- `test_verify_membership_rejects_inactive_group`.
- `test_deactivate_group_happy_path` — flips `active = false`, history records the freeze.
- `test_deactivate_group_rejects_non_member_proof`.
- `test_deactivate_already_inactive_group` → `GroupInactive`.
- `test_update_vk_requires_auth` — `admin.require_auth()` panics when no auth was granted by the test caller (renamed from `test_update_vk_admin_only` in PR #148 review chunk 2 #4 — the original name was misleading because the harness can't construct "non-admin caller WITH valid auth" under `mock_all_auths`; the value-equality `caller != admin` branch is covered by `test_create_group_restricted_mode_rejects_non_admin`).
- `test_update_vk_rotates_membership_vk` — admin updates Membership VK at tier; subsequent membership proofs verify under the new VK.
- `test_update_vk_rotates_update_vk` — admin updates Update VK at tier; subsequent democracy updates verify under the new VK. Critical for the design §4.6 fingerprint-rotation triplet.
- `test_get_commitment_returns_current_state`.
- `test_get_history_returns_chronological_entries`.
- `test_archive_entry_appends_and_prunes` — drives `archive_entry` directly via `env.as_contract`, exceeds `HISTORY_WINDOW=64`, asserts FIFO prune. Covers the write path that `update_commitment` and `deactivate_group` rely on (those paths are mock-proof-blocked, per PR #148 review chunk 2 #6).
- `test_bump_group_ttl_extends_used_proof_lifetime`.
- `test_vectors_consistency` — load `test-vectors.json` via `serde_json` and assert error codes / tier mapping / IC point counts match the contract's `Error` enum + `tier_capacity` + `MEMBERSHIP_IC_POINTS`/`UPDATE_IC_POINTS` constants. Pins the JSON as single source of truth.

Helpers (mirror existing): `mock_membership_vk(env)` (3 IC points), `mock_update_vk(env)` (7 IC points), `mock_proof(env)`, `mock_canonical_fr(env, byte)` (deterministic test commitments), `mock_public_inputs_create(env)`, `mock_public_inputs_update(env)`.

The reference for IC layouts and field validations is `test-vectors.json` — every test that asserts a rejected error matches a `expected_error` entry there.

## D. Verification (end-to-end)

After implementation:

1. **Build**: `cargo build --manifest-path contracts/sep-democracy/Cargo.toml --target wasm32-unknown-unknown --release` produces `sep_democracy_contract.wasm` (target dir). Asserts the crate compiles to Soroban WASM.
2. **Native test suite**: `cargo test --manifest-path contracts/sep-democracy/Cargo.toml`. Asserts all tests in §C pass; snapshot files appear under `test_snapshots/test/`.
3. **Snapshot stability**: re-run tests; assert no snapshot diffs (idempotent runs).
4. **Vectors / contract drift assertion**: the `test_vectors_consistency` test in §C is the load-bearing check. It loads `contracts/sep-democracy/test-vectors.json` via `serde_json::from_str(include_str!("../test-vectors.json"))` and asserts:
   - Every error code in the JSON matches the `Error` enum variant numeric value.
   - Every tier mapping matches `tier_capacity`.
   - The IC point counts match `MEMBERSHIP_IC_POINTS` / `UPDATE_IC_POINTS` constants.
   This pins the JSON as the single source of truth and CI-asserts the contract against it.
5. **Manual deploy smoke (optional, post-PR)**: `bash scripts/deploy_sep_democracy_testnet.sh` against testnet. Capture the new contract address. Verify `create_group → update_commitment` round-trip via `stellar contract invoke` with synthetic test-vector inputs. Out of PR scope; documented in deploy script comments.

## E. Branch + PR (last step)

1. `git checkout main && git pull --rebase`
2. `git checkout -b feat/contract-sep-democracy`
3. Commit in stages so the audit trail reflects the spec-first ordering:
   - `feat(sep-democracy): impl plan + test-vectors.json (canonical contract ABI)`
   - `feat(sep-democracy): Cargo.toml + lib.rs scaffold`
   - `feat(sep-democracy): __constructor + storage + error enum`
   - `feat(sep-democracy): create_group + update_commitment + verify_membership`
   - `feat(sep-democracy): deactivate_group + getters + bump_group_ttl + update_vk`
   - `test(sep-democracy): inline tests covering test-vectors.json#tests_to_implement`
   - `feat(sep-democracy): deploy_sep_democracy_testnet.sh`
   - `docs(democracy): note per-type-contract architecture in design doc §1 + §6`
4. `git push -u origin feat/contract-sep-democracy`
5. `gh pr create --title "feat: per-type Soroban contract — sep-democracy v0 from scratch" --body …` with PR body covering: scope, the per-type-contract architectural choice + its impact on §3.4 leak profile (group type observable from contract address but no per-call leak), test-vectors-first methodology, what's intentionally out of scope (Anarchy/1v1/Oligarchy contracts; cross-platform vectors; ceremony work — those land in their own PRs).

PR checkboxes: vectors file matches design doc; tests cover every `tests_to_implement` entry; build green; snapshot drift = 0; deploy script runs locally to a stellar testnet sandbox without errors; no new dependency on `crates/` or other workspace paths (contract stays self-contained).

## Files referenced from existing code (to mirror, not import)

- `contracts/sep-xxxx/src/lib.rs` (entire file) — patterns for `#[contract]`, `__constructor`, `update_vk`, `create_group`, `update_commitment_democracy`, IC verification, replay protection, history append. **Mirror** the patterns; do NOT add a path-dependency.
- `contracts/sep-xxxx/Cargo.toml` — exact `soroban-sdk` version, profile shapes, `crate-type`. Copy verbatim with name changes.
- `scripts/deploy_sep_xxxx_testnet.sh` — flow: generate fixtures → deploy with `--admin` and per-tier VK file paths → invoke `update_vk` for any post-deploy rotations. Sibling script differs by manifest path and the VK fixture set.
- `docs/soroban-contract-test-vectors.json` — sections 1–6 are reusable; sections 7–10 (polymorphic dispatch, membership_count_validation_v2 single-signer table) get pruned to single-Democracy form.
- [`docs/democracy-update-testnet-design.md`](../../democracy-update-testnet-design.md) — `§4.6` (contract redeploy + VK install), `§4.7` (heart of v2), `§4.7.6` (configurable threshold + tie semantics + IC ordering pin), `§A.2` for storage layout pin.
- `keyset-democracy-dev/` — VK fixtures consumed by the new deploy script. Read-only here; not bundled into the contract crate.

## Out of scope for this plan

- Anarchy / 1v1 / Oligarchy sibling contracts (the user's "eventually four contracts" framing — separate planning effort each).
- Phase A v2 democracy circuit + prover (lives in `src/circuit/democracy_v2.rs`, `src/prover/mod.rs` per design §6 Phase A — independent of this contract crate).
- Phase B FFI + Swift/Kotlin bindings.
- Phase D iOS/Android client dispatch (clients route to the new contract address; that's a separate PR).
- Phase E ceremony.
- Cross-platform test vectors (`docs/cross-platform-test-vectors.json` extension lives in Phase A6).
- Migrating existing testnet groups (no live users; design's §4.6 already specifies "fresh contract address, old contract abandoned").
