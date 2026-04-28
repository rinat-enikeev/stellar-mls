# Plan: implement `contracts/sep-oligarchy` from scratch (per-type Soroban contract)

> **Amendment (postmortem #153 phase 1):** The `deactivate_group` entrypoint described below was removed after a front-running vulnerability surfaced — see `docs/postmortem-deactivate-group-frontrun.md`. References to it are retained for historical accuracy; the live contract has one fewer entrypoint and three fewer tests. The Membership VK is used at `verify_membership` only. The `active` field on `CommitmentEntry` and the `GroupInactive` error variant remain as defense-in-depth for any future re-introduction of deactivation but are unreachable in current production code paths.

## Context

The user is shipping per-type Soroban contracts — one per governance type. `contracts/sep-democracy` already landed (PR #148, branch `feat/contract-sep-democracy`). This plan covers the second per-type contract: **sep-oligarchy**, the Oligarchy governance type. The design source is [`docs/oligarchy-update-testnet-design.md`](../../oligarchy-update-testnet-design.md), v0.1.4.

User direction (PR-Y planning turn):
- **Mirror sep-democracy's contract surface verbatim** — same admin entrypoints, same VK shapes, same restricted-mode gate, no `group_type` field, no polymorphic dispatch. Rename `threshold_numerator` → `admin_threshold_numerator`.
- **Block on design v0.1.4 first** — the brute-force-from-public-priors privacy gap (raised in PR #147 review) closed in the design before the contract is implemented. v0.1.4 added per-epoch salting on the combined occupancy commitment.
- **Verbose `create_oligarchy_group` signature per design §4.8** — contract receives `member_root`, `admin_root`, `salt_initial` explicitly so the create proof binds them as public inputs. Requires a new `CreateVK(tier)` family with 7 IC points (base + 6 bound public inputs), separate from the 3-IC-point membership VK and the 7-IC-point update VK.

The work happened in two sequential PRs:
1. **PR #147**: design v0.1.4 — adds the salting fix.
2. **PR-Y** (this contract): `contracts/sep-oligarchy/` from scratch, parallel to `sep-democracy/`. Branched off `design/oligarchy-update-testnet` to inherit v0.1.4 for cross-references; rebased onto main once PR #147 merges.

---

## Per-type architecture adaptations vs. design v0.1.4 assumptions

Design v0.1.4 is written assuming one shared contract serves all governance types via polymorphic `update_commitment` dispatch. The per-type-contract architecture (already adopted for sep-democracy) inverts this:

| Design v0.1.4 assumption | Per-type adaptation in this implementation |
|---|---|
| Single redeployed contract for Anarchy + Democracy + Oligarchy | One crate per type. `contracts/sep-oligarchy/` has Oligarchy entrypoints only. |
| Polymorphic `update_commitment` dispatches by `current.group_type` (§4.7.4) | No polymorphic dispatch. The contract address IS the discriminator. |
| `group_type=3` stored on `CommitmentEntry` | No `group_type` field. Always Oligarchy. |
| Type-confusion guard rejects an Oligarchy proof submitted to a Democracy group | No guard needed; the wrong-type proof would simply fail verification at the wrong VK family (or not load — different contract address). |
| §3.4 residual "Oligarchy is most-distinguishable on payload size" | Closed differently: the contract address itself reveals the type to chain observers (same publicness class as `tier`). The wire payload still matches Democracy's 5-scalar shape so on-chain calls to two different contracts are indistinguishable on payload bytes alone (only the contract-address dimension differentiates). |
| Phase C lands `contracts/sep-xxxx/src/lib.rs` Oligarchy entrypoints (~280 LOC into the monolith) | This implementation creates `contracts/sep-oligarchy/src/lib.rs` from scratch (~870 LOC standalone). |
| `scripts/install-governance-vks-testnet.sh` covers Oligarchy + Democracy in one install | Each per-type contract has its own deploy script. `scripts/deploy_sep_oligarchy_testnet.sh` (sibling of the sep-democracy script). |

---

## Repo touch list

| Path | Action | Notes |
|---|---|---|
| `contracts/sep-oligarchy/Cargo.toml` | Created | `crate-type = ["cdylib"]`, `soroban-sdk = "=25.3.0"`, `lib.name = "sep_oligarchy_contract"`. Mirrors sep-democracy's Cargo.toml. |
| `contracts/sep-oligarchy/Cargo.lock` | Copied from sep-democracy | Pins transitive deps to the same versions; avoids the 25.3.1 / rustc 1.91 trap. |
| `contracts/sep-oligarchy/test-vectors.json` | Created FIRST | Canonical contract-ABI pin. CI-asserted by `test_vectors_consistency`. |
| `contracts/sep-oligarchy/src/lib.rs` | Created | The contract (~870 LOC). |
| `contracts/sep-oligarchy/src/test.rs` | Created | Inline tests (~750 LOC, 48 tests). |
| `contracts/sep-oligarchy/test_snapshots/test/` | Auto-generated | `#[contracttest]` snapshots; checked in. |
| `docs/contract/oligarchy/impl_plan.md` | Created | This plan. |
| `docs/contract/oligarchy/proof_of_soundness.md` | Created | Mirrors `docs/contract/democracy/proof_of_soundness.md` shape; Oligarchy-specific SI-N invariants. |
| `docs/contract/oligarchy/proof_of_correctness.md` | Created | Mirrors `docs/contract/democracy/proof_of_correctness.md` shape; Oligarchy-specific spec→impl mapping. |
| `scripts/deploy_sep_oligarchy_testnet.sh` | Created | Sibling of `scripts/deploy_sep_democracy_testnet.sh`. Reads VKs from `keyset-oligarchy-dev/` (created in Phase A4). |

No workspace at root — each contract crate is independent.

---

## A. `test-vectors.json` — written first

Canonical contract-ABI pin. CI-asserted by `test_vectors_consistency` (`#[test]` in `src/test.rs`). Mirrors sep-democracy's `test-vectors.json` shape with Oligarchy-specific values.

Sections:
1. **`error_codes`** — slimmed enum: `NotInitialized=1`, `AlreadyInitialized=2`, `Reserved3=3`, `GroupAlreadyExists=4`, `GroupNotFound=5`, `GroupInactive=6`, `InvalidProof=7`, `InvalidTier=8`, `InvalidVkLength=9`, `PublicInputsMismatch=10`, `InvalidEpoch=11`, `ProofReplay=12`, `TierGroupLimitReached=13`, `AdminOnly=14`, `InvalidCommitmentEncoding=15`, `InvalidPoint=26`, `GroupStillActive=27`, `InvalidThreshold=28`, `InvalidInitialMembership=30` (reserved per §4.8).
2. **`tier`** — member tier 0/1/2 mapping (Small=32 / Medium=256 / Large=2048). Admin tier always Small (depth 5, 32 slots) — pinned as a constant.
3. **`max_groups_per_tier`** — 10000.
4. **`vk_kind_enum`** — three kinds: `Membership` (3 IC, member-tree opening), `Create` (7 IC, binds verbose §4.8 tuple), `Update` (7 IC, with admin_threshold).
5. **`create_oligarchy_group_validation`** — full validation rule set.
6. **`domain_tags`** — six values per v0.1.3+ §4.1.1.
7. **`occupancy_bitmap_fold`** — member tier scaling 1/2/9 scalars; admin tier always 1 scalar.
8. **`update_commitment_public_inputs_wire_format`** — single Oligarchy variant, byte-identical to Democracy.
9. **`create_public_inputs_wire_format`** — new section vs. sep-democracy. 5 wire fields + epoch=0 supplied by contract.
10. **`storage_layout`** — `CommitmentEntry { commitment, epoch, timestamp, tier, active, occupancy_commitment, admin_threshold_numerator: u32 }`. Three VK families (Membership / Create / Update).
11. **`admin_entrypoints`** — `update_vk` (rotates Membership / Create / Update VK at a tier), `set_restricted_mode`.
12. **`queries`** — `get_commitment`, `get_history`, `verify_membership`, `bump_group_ttl`.
13. **`epoch_invariant`** — same monotonic-by-1 rule.
14. **`proof_replay_protection`** — same `sha256(proof.a || proof.b || proof.c)` global-nullifier scope, `LEDGER_BUMP` TTL.
15. **`tests_to_implement`** — 48 inline-test names that map 1:1 to `#[test] fn`s in `src/test.rs`.

`test_vectors_consistency` loads the JSON via `serde_json::from_str(include_str!("../test-vectors.json"))` and asserts:
- Every error code matches the `Error::Variant as u32`.
- Every tier mapping matches `tier_capacity(tier)`.
- The IC-point counts match `MEMBERSHIP_IC_POINTS=3`, `CREATE_IC_POINTS=7`, `UPDATE_IC_POINTS=7`.
- `MAX_GROUPS_PER_TIER=10000`.

---

## B. `src/lib.rs` structure

Mirrors sep-democracy with three additions:
1. `CreateVK(tier)` storage family + `verify_create_proof` + `load_create_vk`.
2. Verbose `create_oligarchy_group` signature (10 args + proof + public_inputs).
3. `Error::InvalidInitialMembership=30` reservation.

### B.1 Constants
- `HISTORY_WINDOW = 64`
- `LEDGER_THRESHOLD = 17_280`
- `LEDGER_BUMP = 518_400`
- `MAX_GROUPS_PER_TIER = 10_000`
- `MEMBERSHIP_IC_POINTS = 3`, `CREATE_IC_POINTS = 7`, `UPDATE_IC_POINTS = 7`
- `tier_capacity(0/1/2)` returns 32 / 256 / 2048 — member-tier slot capacity (admin tier fixed at depth 5, 32 slots, NOT a contract argument).

### B.2 Types
- `CommitmentEntry { commitment, epoch, timestamp, tier, active, occupancy_commitment, admin_threshold_numerator: u32 }`
- `PublicInputs { commitment, epoch }` — 3 IC (membership)
- `CreatePublicInputs { commitment, epoch, occupancy_commitment, member_root, admin_root, salt_initial }` — 7 IC (create)
- `UpdateCommitmentPublicInputs { c_old, epoch_old, c_new, occupancy_commitment_old, occupancy_commitment_new }` — 5 wire scalars
- `VkKind { Membership, Create, Update }`
- `DataKey { Admin, RestrictedMode, VK(tier), CreateVK(tier), UpdateVK(tier), Group, History, UsedProof, GroupCount(tier) }`

### B.3 Entrypoints

| Entrypoint | Auth | Notes |
|---|---|---|
| `__constructor(env, admin, vk_small/medium/large, create_vk_small/medium/large, update_vk_small/medium/large)` | admin.require_auth() | Atomic deploy. Validates IC arities (3/7/7), runs subgroup checks, stores all 9 VKs + admin. |
| `update_vk(env, kind: VkKind, tier, new_vk)` | admin.require_auth() | Rotates Membership / Create / Update VK at `tier ∈ {0,1,2}`. |
| `set_restricted_mode(env, restricted)` | admin.require_auth() | Toggles `RestrictedMode` bool. |
| `bump_group_ttl(env, group_id)` | none | Permissionless TTL bump on `Group` + `History`. |
| `create_oligarchy_group(env, caller, group_id, commitment, member_tier, admin_threshold_numerator, occupancy_commitment_initial, member_root, admin_root, salt_initial, proof, public_inputs: CreatePublicInputs)` | caller.require_auth() + RestrictedMode gate | Validates per §A rules; verifies proof against `CreateVK(member_tier)` with public inputs `(commitment, 0, occupancy_commitment, member_root, admin_root, salt_initial)`; on success writes `CommitmentEntry`, increments `GroupCount(tier)`, records proof, emits `GroupCreated`. |
| `update_commitment(env, group_id, proof, public_inputs: UpdateCommitmentPublicInputs)` | none (proof IS auth) | State-chains via `c_old / epoch_old / occupancy_commitment_old`. Verifies proof against `UpdateVK(current.tier)` with public inputs `(c_old, epoch_old, c_new, occ_old, occ_new, current.admin_threshold_numerator)`. Archives prior state, advances epoch, emits `CommitmentUpdated`. |
| `verify_membership(env, group_id, proof, public_inputs: PublicInputs) -> bool` | none | Read-only. Verifies proof against `VK(current.tier)` (the standard 3-IC-point membership VK, opening against the **member tree**). |
| `deactivate_group(env, group_id, proof, public_inputs: PublicInputs)` | none | Member-tree membership proof. Archives prior state, sets `active = false`, decrements `GroupCount`, emits `GroupDeactivated`. |
| `get_commitment(env, group_id) -> CommitmentEntry` | none | Read-only. |
| `get_history(env, group_id, max_entries) -> Vec<CommitmentEntry>` | none | Read-only; tail-truncated. |

### B.4 Verifiers

Three verifier helpers parallel sep-democracy's two:
- `verify_membership_proof` — 3-IC, public inputs `(commitment, epoch)`. Verbatim from sep-democracy.
- `verify_create_proof` — 7-IC, public inputs `(commitment, epoch, occupancy_commitment, member_root, admin_root, salt_initial)`. New for sep-oligarchy.
- `verify_update_proof` — 7-IC, public inputs `(c_old, epoch_old, c_new, occ_old, occ_new, admin_threshold_numerator)`. Renamed `threshold_numerator` → `admin_threshold_numerator`.

### B.5 Internal helpers

Verbatim from sep-democracy:
- `proof_hash`, `check_proof_replay`, `record_proof`
- `archive_entry`, `bump_group`, `group_exists`, `load_group`
- `load_vk`, `load_update_vk`, plus new `load_create_vk`
- `require_initialized`
- `validate_vk_points`, `validate_proof_points`, `is_canonical_fr`, `u64_to_u256_be`, `u32_to_fr`

---

## C. Inline tests (`src/test.rs`)

Mirrors sep-democracy's pattern: `setup_env()` helper, `mock_*` constructors, `inject_group`, `#[contracttest]` snapshots. The `tests_to_implement` JSON section is the canonical list.

### C.1 Test count: 51 (post-PR-#149-review-chunk-2 expansion)

| Category | Test count |
|---|---|
| Initialization | 4 (initialize + 3 IC-arity rejection) |
| `create_oligarchy_group` | 16 (happy path + 15 rejection / threshold / canonical-Fr / restricted-mode / tier-cap) |
| `update_commitment` | 11 (3 over-claiming "v2 threshold" tests collapsed into 1 IC[6]-inspection per chunk-2 review) |
| `verify_membership` | 4 |
| `deactivate_group` | 3 |
| `update_vk` | 6 (requires_auth + 3 rotation paths + invalid-tier + create-vk-arity-mismatch) |
| Queries | 6 (get_commitment {happy, GroupNotFound} + get_history {happy, GroupNotFound} + archive_entry + bump_group_ttl {happy, GroupNotFound}) |
| ABI pin | 1 (`test_vectors_consistency`) |

Note: `test_archive_entry_appends_and_prunes` is folded into the Queries row (it exercises the History write path that `update_commitment` and `deactivate_group` both rely on). Total: 50 named entries in `tests_to_implement.tests` + `test_vectors_consistency` = 51 inline `#[test] fn`s.

### C.2 Mock proof strategy

Same as sep-democracy: `valid_g1` / `valid_g2` via `hash_to_g{1,2}` for subgroup-valid points. Mock proofs cannot pass `pairing_check`, so happy-path tests assert `InvalidProof` (verifier reached) rather than success. The full positive round-trip is gated on Phase A's real `prove_oligarchy_v2`.

### C.3 Budget reset for setup

The constructor runs subgroup checks on 9 VKs (vs sep-democracy's 6); the default test budget can't cover it. `setup_env` calls `env.cost_estimate().budget().reset_unlimited()` before `env.register(...)` to keep test runs fast.

---

## D. Verification

After implementation:

1. **Build**: `stellar contract build --manifest-path contracts/sep-oligarchy/Cargo.toml` produces `sep_oligarchy_contract.wasm` (31KB, 10 entrypoints including `__constructor`). Asserts the crate compiles to Soroban WASM.
2. **Native test suite**: `cargo test --manifest-path contracts/sep-oligarchy/Cargo.toml`. All 48 tests pass; snapshot files appear under `test_snapshots/test/`.
3. **Snapshot stability**: re-run; no snapshot diffs.
4. **ABI drift**: `test_vectors_consistency` is the canonical CI gate.
5. **Manual deploy smoke** (post-PR, gated on Phase A4 producing dev VKs in `keyset-oligarchy-dev/`): `bash scripts/deploy_sep_oligarchy_testnet.sh` against testnet. Capture deployed contract address. Smoke-test `create_oligarchy_group → update_commitment` round-trip via stellar CLI once Phase A's `prove_oligarchy_v2` lands.

---

## E. Out of scope for this PR

- **Phase A v0.1.4 oligarchy circuit + prover** (`src/circuit/oligarchy_v2.rs`, `src/prover/mod.rs::prove_oligarchy_v2`) — design §6 Phase A. Independent workstream; not blocking the contract impl since the contract uses mock proofs in tests.
- **Phase A4 dev VK generation** — generates `keyset-oligarchy-dev/`. Required before the deploy script's smoke-test can run, but not before the contract can be tested locally with mock proofs.
- **Phase B FFI + Swift/Kotlin bindings** — separate workstream.
- **Phase D iOS/Android client dispatch** — separate workstream; clients route to the new contract address.
- **Phase E ceremony** — production VKs for mainnet release.
- **Cross-platform test vectors** — `docs/cross-platform-test-vectors.json` extension lives in Phase A6.
- **Anarchy / OneOnOne sibling contracts** — separate per-type-contract PRs.
- **Migrating existing testnet groups** — no live users; design's §4.6 specifies "fresh contract address, old contract abandoned."
