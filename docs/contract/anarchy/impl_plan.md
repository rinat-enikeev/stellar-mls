# Plan: implement `contracts/sep-anarchy` from scratch (per-type Soroban contract)

## Context

The user is shipping per-type Soroban contracts — one per governance type. `sep-democracy` (PR #148) and `sep-oligarchy` (PR #149) already landed. This plan covers the third per-type contract: **sep-anarchy**, the Anarchy governance type (`group_type=0` in v1 sep-xxxx). The design source is [`docs/anarchy-update-testnet-design.md`](../../anarchy-update-testnet-design.md), v0.1 (PR-X).

User direction (PR-Y):
- **Pure per-type extraction (option A)**: mirror v1 Anarchy logic verbatim. Drop only the multi-type artifacts (`group_type` field, `UnknownGroupType` error, polymorphic dispatch, type-confusion guard). Keep `member_count` even though it's informational. Same VK shapes (3-IC Membership, 4-IC Update). No circuit changes. No v2 improvements over v1.
- **Branch off main**, write a design doc (PR-X), then implement test-vectors-first (this plan, PR-Y).

The work happened in two sequential PRs:
1. **PR-X**: `docs/anarchy-update-testnet-design.md` v0.1 (single-commit design doc).
2. **PR-Y** (this contract): `contracts/sep-anarchy/` from scratch.

---

## Anarchy is structurally the simplest per-type contract

| Property | sep-anarchy | sep-democracy | sep-oligarchy |
|---|---|---|---|
| Wire payload (update) | **3 scalars** | 5 scalars | 5 scalars |
| Update VK IC count | **4** | 7 | 7 |
| Membership VK IC count | 3 | 3 | 3 |
| VK families | **2** | 2 | 3 (+ Create) |
| Phase A circuit work | **0 LOC** (reuse keyset-v2) | new v2 circuit | new v2 circuit |
| Storage `CommitmentEntry` fields | 6 | 7 | 7 |
| Hidden member counts | (n/a — informational) | ✓ via occupancy_commitment | ✓ via combined occupancy |
| Quorum threshold | (n/a) | `threshold_numerator` | `admin_threshold_numerator` |
| Admin set | (n/a) | (n/a) | second tree |
| Inline tests | 39 | 51 | 51 |
| Wasm size | 26KB | 26KB | 31KB |

Anarchy is the protocol's "null case" — pure membership proof, governance enforced 100% inside the ZK circuit.

---

## Repo touch list

| Path | Action | Notes |
|---|---|---|
| `contracts/sep-anarchy/Cargo.toml` | Created | `crate-type = ["cdylib"]`, `soroban-sdk = "=25.3.0"`. Mirror sep-democracy's Cargo.toml verbatim. |
| `contracts/sep-anarchy/Cargo.lock` | Copied from sep-democracy | Pins transitive deps; avoids 25.3.1 / rustc-1.91 trap. |
| `contracts/sep-anarchy/test-vectors.json` | Created FIRST | Canonical contract-ABI pin. CI-asserted by `test_vectors_consistency`. |
| `contracts/sep-anarchy/src/lib.rs` | Created | The contract (~1050 LOC including doc-comments / blank lines). |
| `contracts/sep-anarchy/src/test.rs` | Created | Inline tests (~990 LOC, 39 tests). |
| `contracts/sep-anarchy/test_snapshots/test/` | Auto-generated | `#[contracttest]` snapshots; checked in. |
| `docs/contract/anarchy/impl_plan.md` | Created | This plan. |
| `docs/contract/anarchy/proof_of_soundness.md` | Created | SI-N invariants. |
| `docs/contract/anarchy/proof_of_correctness.md` | Created | Spec→impl mapping. |
| `scripts/deploy_sep_anarchy_testnet.sh` | Created | Reads VKs from `keyset-v2/` (existing v2 keyset; no Phase A4 work needed). |

---

## A. `test-vectors.json` (written FIRST)

Canonical contract-ABI pin. CI-asserted by `test_vectors_consistency`. Mirrors sep-democracy + sep-oligarchy structure with Anarchy-specific values.

Sections:
1. **`error_codes`** — 17 active enum slots (15 reachable + 2 reserved: Reserved3, GroupStillActive); smaller than sep-democracy's 19 and sep-oligarchy's 19. `dropped_from_sep_xxxx` lists 13 dropped (multi-type + Democracy + Oligarchy + 1v1 artifacts).
2. **`tier`** — member tier 0/1/2 (32/256/2048).
3. **`max_groups_per_tier`** — 10000.
4. **`vk_kind_enum`** — 2 kinds: Membership (3 IC, used at create + verify_membership + deactivate_group) + Update (4 IC, used at update_commitment).
5. **`create_group_validation`** — rules: tier ≤ 2, group_id unused, canonical Fr on commitment, GroupCount cap, restricted-mode check, replay-fresh, proof verifies. `member_count` is informational; any u32 accepted.
6. **`domain_tags`** — `DOMAIN_MEMBER=1`, `DOMAIN_TOMBSTONE=2`. `DOMAIN_OCCUPANCY=3` and beyond are unused (Anarchy has no occupancy commitment / admin tree).
7. **`update_commitment_public_inputs_wire_format`** — 3 wire fields; 3 Groth16 public inputs; no contract-supplied scalar.
8. **`storage_layout`** — `CommitmentEntry { commitment, epoch, timestamp, tier, active, member_count }`. DataKeys: Admin, RestrictedMode, VK(tier), UpdateVK(tier), Group, History, UsedProof, GroupCount(tier).
9. **`admin_entrypoints`** — update_vk (Membership / Update), set_restricted_mode (emits RestrictedModeChanged).
10. **`queries`** — get_commitment, get_history, verify_membership, bump_group_ttl.
11. **`epoch_invariant`** — same monotonic-by-1 rule.
12. **`proof_replay_protection`** — same global-nullifier scope; `bump_group_ttl` does NOT touch UsedProof (per the chunk-2-review correction landed in sep-democracy + sep-oligarchy).
13. **`tests_to_implement`** — 39 inline-test names (38 named + `test_vectors_consistency`).

---

## B. `src/lib.rs` structure

### B.1 Constants
- `HISTORY_WINDOW = 64`, `LEDGER_THRESHOLD = 17_280`, `LEDGER_BUMP = 518_400`, `MAX_GROUPS_PER_TIER = 10_000`
- `MEMBERSHIP_IC_POINTS = 3`, `UPDATE_IC_POINTS = 4` (Anarchy-specific — smaller than Democracy/Oligarchy's 7)
- `tier_capacity(0/1/2)` = 32/256/2048 (`#[cfg(test)]` only — closes the chunk-2 sep-democracy review finding about WASM bloat from `#[allow(dead_code)]`)

### B.2 Types
- `CommitmentEntry { commitment, epoch, timestamp, tier, active, member_count: u32 }` — 6 fields; `member_count` informational
- `PublicInputs { commitment, epoch }` — 2 wire fields, 3 IC
- `UpdateCommitmentPublicInputs { c_old, epoch_old, c_new }` — 3 wire fields, 4 IC
- `VkKind { Membership, Update }` — 2 families
- `DataKey { Admin, RestrictedMode, VK(u32), UpdateVK(u32), Group, History, UsedProof, GroupCount(u32) }`
- `RestrictedModeChanged { admin, restricted, timestamp }` — event for audit transparency

### B.3 Entrypoints

| Entrypoint | Auth | Notes |
|---|---|---|
| `__constructor(env, admin, vk_small/medium/large, update_vk_small/medium/large)` | admin.require_auth() | 6 VKs total. |
| `update_vk(env, kind: VkKind, tier, new_vk)` | admin-only | Membership / Update rotation; tier ≤ 2 guard. |
| `set_restricted_mode(env, restricted)` | admin-only | Toggles RestrictedMode bool; emits `RestrictedModeChanged`. |
| `bump_group_ttl(env, group_id)` | none | Permissionless TTL bump. Has `require_initialized` per the chunk-2-review pattern. Bumps Group + History only — does NOT touch UsedProof. |
| `create_group(env, caller, group_id, commitment, tier, member_count, proof, public_inputs)` | caller.require_auth() + RestrictedMode gate | Validates per §A.5; verifies 3-IC membership proof against `(commitment, 0)`; stores `member_count` informationally. |
| `update_commitment(env, group_id, proof, public_inputs)` | none (proof IS auth) | State-chains via `c_old`/`epoch_old`. Verifies 4-IC update proof. **`member_count` NOT mutated** (contract has no Poseidon host to recompute). |
| `verify_membership(env, group_id, proof, public_inputs) -> bool` | none | Read-only; no replay-check (parallel to sep-democracy + sep-oligarchy rationale). Does NOT check `state.active` — post-deactivation attestations remain verifiable. |
| `deactivate_group(env, group_id, proof, public_inputs)` | none | Member proof; archives state; sets `active = false`; decrements `GroupCount`. |
| `get_commitment(env, group_id)` | none | Read-only. |
| `get_history(env, group_id, max_entries)` | none | Read-only; tail-truncated. |

### B.4 Verifiers
- `verify_membership_proof(env, vk, proof, commitment, epoch) -> bool` — verbatim from sep-democracy. 3-IC.
- `verify_update_proof(env, vk, proof, c_old, epoch_old, c_new) -> bool` — Anarchy-specific. 4-IC. No contract-supplied scalar.

### B.5 Internal helpers (verbatim from sep-democracy)
- `proof_hash`, `check_proof_replay`, `record_proof`
- `archive_entry`, `bump_group`, `group_exists`, `load_group`
- `load_vk`, `load_update_vk` — both with `tier > 2 → InvalidTier` guards (chunk-2-review pattern from sep-oligarchy)
- `require_initialized`
- `validate_vk_points`, `validate_proof_points`, `is_canonical_fr`, `u64_to_u256_be`

(No `u32_to_fr` — Anarchy doesn't supply a contract-side scalar to the verifier.)

---

## C. Inline tests (`src/test.rs`, 39 tests)

Categories (parallel to sep-democracy's pattern minus threshold-related):
- Initialization (3): happy path + 2 IC-arity rejections (membership=3, update=4)
- `create_group` (9): happy path + invalid tier + duplicate + non-canonical commitment + invalid proof + restricted-mode reject + tier-cap reject + member_count zero/arbitrary
- `update_commitment` (8): happy path + stale c_old + wrong epoch_old + non-canonical c_new + replayed proof + inactive group + unknown group + does_not_mutate_member_count
- `verify_membership` (4)
- `deactivate_group` (3)
- `update_vk` (4): requires_auth + 2 rotation paths + invalid_tier
- Queries (7): get_commitment {happy, GroupNotFound} + get_history {happy, GroupNotFound} + archive_entry direct + bump_group_ttl {happy, GroupNotFound}
- ABI pin (1): `test_vectors_consistency`

**Mock-proof strategy** parallels sep-democracy + sep-oligarchy: `valid_g1` / `valid_g2` via `hash_to_g{1,2}`; happy-path tests assert `InvalidProof` (verifier reached). `setup_env` calls `env.cost_estimate().budget().reset_unlimited()` for consistency, even though the 6-VK constructor has lower budget pressure than sep-oligarchy's 9-VK constructor.

**Anarchy-specific test:** `test_update_commitment_does_not_mutate_member_count` — pins the contract's value-agnostic posture. Inject a group with `member_count=42`; attempt update (mock fails at verifier); read state; assert `member_count` is still 42 (the field is informational and the contract preserves it across failed updates by Soroban transactional revert; on a successful update it's preserved by `member_count: current.member_count` in the new entry's struct literal).

---

## D. Verification

After implementation:

1. **Build**: `stellar contract build --manifest-path contracts/sep-anarchy/Cargo.toml` produces `sep_anarchy_contract.wasm` (26KB, 10 entrypoints including `__constructor`).
2. **Native test suite**: `cargo test --manifest-path contracts/sep-anarchy/Cargo.toml`. All 39 tests pass.
3. **Snapshot stability**: re-run produces no diff.
4. **ABI drift**: `test_vectors_consistency` is the canonical CI gate.
5. **Manual deploy smoke** (post-PR): `bash scripts/deploy_sep_anarchy_testnet.sh` against testnet. Anarchy's deploy script reads `keyset-v2/` directly — no Phase A4 dev-VK generation needed (unlike Oligarchy which needed `keyset-oligarchy-dev/`).

---

## E. Out of scope for this PR

- **v2 Anarchy improvements** (drop member_count entirely, bind group_id in MembershipCircuit per sep-xxxx Audit Finding #1, operation-tag binding) — explicitly excluded per option-A choice. §11 follow-up in the design doc.
- **Phase A circuit work**: existing `keyset-v2/` Anarchy circuits are reused; no new circuit code, fixtures, or prover work.
- **Phase B FFI / Phase D clients**: existing v1 Anarchy paths work; no new bindings or client routing in this PR.
- **Phase E ceremony**: shared with Democracy + Oligarchy.
- **OneOnOne sibling contract**: still pending; separate planning effort.
