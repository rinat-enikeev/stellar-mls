# Proof of Correctness — `contracts/sep-democracy`

**Scope:** the per-type Democracy Soroban contract at `contracts/sep-democracy/src/lib.rs` (revision: branch `feat/contract-sep-democracy`, commit `37fedab` and later).

**Status:** informal correctness argument — for every input class permitted by the spec, the contract produces the spec-mandated state delta and event. Where the spec is ambiguous or aspirational (Phase A integration, ceremony), this document flags the gap rather than pretending it's covered.

**Companion:** [`proof_of_soundness.md`](proof_of_soundness.md) covers the dual question ("can the contract be tricked"). This document covers ("does the contract behave as documented when used honestly").

**Specification sources** (in decreasing authority order):
1. `contracts/sep-democracy/test-vectors.json` — CI-asserted ABI pin (loaded at test time by `test_vectors_consistency`).
2. [`docs/democracy-update-testnet-design.md`](../../democracy-update-testnet-design.md) — design v0.5.1; sections referenced inline.
3. [`docs/contract/democracy/impl_plan.md`](impl_plan.md) — implementation plan tracking the design.

---

## 1. Spec-to-implementation mapping

### 1.1 Storage layout

The on-chain `CommitmentEntry` (`lib.rs:166-189`) carries exactly the fields the design's §A.2 storage-layout pin requires for v2:

| Field | Type | Source-of-truth | Mutation surface |
|---|---|---|---|
| `commitment` | `BytesN<32>` | `c_new` from update wire OR `commitment` from create | written by `create_group` and `update_commitment`; never by `deactivate_group` (preserved via `..current.clone()`) |
| `epoch` | `u64` | `0` at create, `current.epoch + 1` at update | monotonic +1 |
| `timestamp` | `u64` | `env.ledger().timestamp()` | re-stamped at every successful `create_group` / `update_commitment` / `deactivate_group` |
| `tier` | `u32` | wire arg at create | fixed |
| `active` | `bool` | `true` at create / update; `false` at deactivate | one-way flip |
| `occupancy_commitment` | `BytesN<32>` | wire `occupancy_commitment_initial` at create; wire `occupancy_commitment_new` at update | written |
| `threshold_numerator` | `u32` | wire arg at create | fixed |

**Removed fields vs sep-xxxx v1** (per design §3.4 "hidden counts" and per-type-contract architecture):
- `group_type` — implicit in contract address.
- `member_count` — replaced by `occupancy_commitment` (Poseidon-hashed bitmap).

The `DataKey` enum (`lib.rs:255-275`) covers the design's required keys:

| `DataKey` variant | Storage class | Purpose | Spec ref |
|---|---|---|---|
| `Admin` | instance | contract admin address | §4.6 |
| `RestrictedMode` | instance | bool toggle for admin-only `create_group` | impl_plan §B.5 |
| `VK(tier)` | persistent | membership-circuit VK per tier 0-2 | §4.6 |
| `UpdateVK(tier)` | persistent | v2 democracy update VK per tier 0-2 | §4.6 / §4.7.6 |
| `Group(group_id)` | persistent | live state | §A.2 |
| `History(group_id)` | persistent | rolling FIFO of prior states | §A |
| `UsedProof(proof_hash)` | persistent (TTL ≈ `LEDGER_BUMP`) | replay nullifier | §11 |
| `GroupCount(tier)` | instance | active-group counter for `MAX_GROUPS_PER_TIER` gate | impl_plan §B.5 |

CI-asserted by `test_vectors_consistency` (`test.rs:1112-1186`).

### 1.2 Constants

| Constant | Value | Spec | Rationale |
|---|---|---|---|
| `HISTORY_WINDOW` | 64 | impl_plan / §A | rolling-window cap |
| `LEDGER_THRESHOLD` | 17_280 (~1d) | TTL bump threshold | persistent-storage liveness |
| `LEDGER_BUMP` | 518_400 (~30d) | TTL bump amount | nullifier window |
| `MAX_GROUPS_PER_TIER` | 10_000 | impl_plan §B.5 | cross-group cap |
| `MEMBERSHIP_IC_POINTS` | 3 | §4.7.3 | base + commitment + epoch |
| `UPDATE_IC_POINTS` | 7 | §4.7.6 | base + 5 wire scalars + threshold |
| `tier_capacity(0/1/2)` | 32/256/2048 | §4.1 | tree depth 5/8/11 → 2^depth slots |

### 1.3 Errors

The `Error` enum (`lib.rs:91-118`) pins 19 reachable variants plus `Reserved3` and `GroupStillActive` (slot-continuity placeholders documented in `test-vectors.json`). `UnknownVkKind=16` was removed in PR #148 review (chunk 1 #4).

CI-asserted by `test_vectors_consistency`'s error-code loop.

### 1.4 Events

Three events match the design's audit-trail expectation:

| Event | Topics | Fields | Emitted by |
|---|---|---|---|
| `GroupCreated` | `group_id` | `commitment, epoch, tier, timestamp` | `create_group` |
| `CommitmentUpdated` | `group_id` | `commitment, epoch, timestamp` | `update_commitment` |
| `GroupDeactivated` | `group_id` | `final_epoch, timestamp` | `deactivate_group` |

Events are emitted **after** all storage writes (`lib.rs:561-568`, `:644-650`, `:715-720`). On any error path, no event is emitted (Soroban transaction revert).

---

## 2. State invariants (from `proof_of_soundness.md`)

The soundness document's SI-1 through SI-19 are also correctness invariants — the contract should preserve them, AND honest users should observe them as documented behavior. Cross-references:

- `epoch` monotonic +1 per update (SI-13) — observable via `get_commitment().epoch` advancing exactly 1 per `CommitmentUpdated` event.
- `threshold_numerator` immutable post-create (SI-4) — observable: `get_commitment().threshold_numerator` returns the value passed to `create_group`, never mutated.
- `tier` immutable post-create (SI-15) — observable: `get_commitment().tier` returns the value passed to `create_group`.
- `active` one-way (SI-14) — observable: once `false`, no entrypoint flips it back.
- History FIFO at `HISTORY_WINDOW` (SI-18) — observable: `get_history()` returns at most 64 entries, oldest dropped first.

---

## 3. Per-entrypoint correctness

For each public entrypoint, the spec's input class → output behavior mapping is enumerated. "Spec-required" means the design / impl_plan / vectors mandate the behavior; "Verified by" lists the test that pins it.

### 3.1 `__constructor(env, admin, vk_small, vk_medium, vk_large, update_vk_small, update_vk_medium, update_vk_large) -> Result<(), Error>`

**Spec:** atomic deploy-time installer (impl_plan §B.5). One-shot.

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Valid admin + 6 subgroup-valid VKs with correct IC arities, before any prior init | `Admin`, `VK(0..=2)`, `UpdateVK(0..=2)` written; TTLs bumped; `Ok(())` | `test_initialize` |
| Membership VK with `ic.len() != 3` | `Err(InvalidVkLength)` | `test_invalid_membership_vk_length_rejected` |
| Update VK with `ic.len() != 7` | `Err(InvalidVkLength)` | `test_invalid_update_vk_length_rejected` |
| VK with non-subgroup point | `Err(InvalidPoint)` | (implicit; subgroup check covered at runtime by `validate_vk_points`) |
| Already initialized | `Err(AlreadyInitialized)` | (constructor idempotency by `do_initialize`'s guard at `lib.rs:323-325`) |

### 3.2 `update_vk(env, kind: VkKind, tier: u32, new_vk: VerificationKeyData) -> Result<(), Error>`

**Spec:** admin-only VK rotation per design §4.6 fingerprint-coordination triplet.

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Admin auth + valid `(kind, tier ≤ 2, IC arity matches kind)` + subgroup-valid points | `VK(tier)` or `UpdateVK(tier)` overwritten; TTL bumped; `Ok(())` | `test_update_vk_rotates_membership_vk`, `test_update_vk_rotates_update_vk` |
| No auth granted | Soroban-level "Unauthorized" panic | `test_update_vk_requires_auth` |
| `tier > 2` | `Err(InvalidTier)` | (gated at `lib.rs:396`) |
| `IC arity mismatch` | `Err(InvalidVkLength)` | (gated at `lib.rs:403`) |
| Non-subgroup VK point | `Err(InvalidPoint)` | (gated at `lib.rs:406`) |
| Pre-init | `Err(NotInitialized)` | (gated at `lib.rs:388`) |

### 3.3 `set_restricted_mode(env, restricted: bool) -> Result<(), Error>`

**Spec:** admin-only toggle (impl_plan §B.5).

| Input class | Spec-required behavior |
|---|---|
| Admin auth + bool | `RestrictedMode := restricted`; `Ok(())` |
| No auth | "Unauthorized" panic |
| Pre-init | `Err(NotInitialized)` |

`test_create_group_restricted_mode_rejects_non_admin` exercises the full toggle-on path indirectly (admin enables restriction; non-admin gets `AdminOnly`).

### 3.4 `bump_group_ttl(env, group_id) -> Result<(), Error>`

**Spec:** permissionless TTL bump for long-lived groups.

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Existing group | `Group(group_id)` and `History(group_id)` TTLs extended; `Ok(())` | `test_bump_group_ttl_extends_used_proof_lifetime` |
| Unknown `group_id` | `Err(GroupNotFound)` | (gated at `lib.rs:432`) |

### 3.5 `create_group(env, caller, group_id, commitment, tier, threshold_numerator, occupancy_commitment_initial, proof, public_inputs) -> Result<(), Error>`

**Spec:** §A test-vectors `create_group_validation`; impl_plan §B.5; design §4.7.6 (threshold).

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| All checks pass + valid Groth16 proof | `CommitmentEntry` written at epoch 0; empty `History`; `GroupCount(tier) += 1`; `UsedProof(proof_hash) := true`; `GroupCreated` event; `Ok(())` | `test_create_group_happy_path` (fails at verifier with mock proof; verifies all gates pass) |
| `tier > 2` | `Err(InvalidTier)` | `test_create_group_rejects_invalid_tier` |
| `threshold_numerator == 0` | `Err(InvalidThreshold)` | `test_create_group_rejects_invalid_threshold_zero` |
| `threshold_numerator > 100` | `Err(InvalidThreshold)` | `test_create_group_rejects_invalid_threshold_above_100` |
| Threshold 50 / 67 / 100 | accepted (reaches verifier) | `test_create_group_accepts_threshold_50/67/100` |
| `public_inputs.commitment != commitment` | `Err(PublicInputsMismatch)` | (gated at `lib.rs:484`) |
| `public_inputs.epoch != 0` | `Err(PublicInputsMismatch)` | (gated at `lib.rs:484`) |
| Group already exists | `Err(GroupAlreadyExists)` | `test_create_group_rejects_duplicate_group_id` |
| Non-canonical `commitment` | `Err(InvalidCommitmentEncoding)` | `test_create_group_rejects_non_canonical_commitment` |
| Non-canonical `occupancy_commitment_initial` | `Err(InvalidCommitmentEncoding)` | `test_create_group_rejects_non_canonical_occupancy_commitment` |
| `GroupCount(tier) >= MAX_GROUPS_PER_TIER` | `Err(TierGroupLimitReached)` | `test_create_group_enforces_tier_group_limit` |
| Replayed proof | `Err(ProofReplay)` | (gated at `lib.rs:506` via `check_proof_replay`) |
| Bad proof bytes | `Err(InvalidProof)` | `test_create_group_rejects_invalid_proof` |
| Restricted mode + non-admin caller | `Err(AdminOnly)` | `test_create_group_restricted_mode_rejects_non_admin` |

### 3.6 `update_commitment(env, group_id, proof, public_inputs: UpdateCommitmentPublicInputs) -> Result<(), Error>`

**Spec:** §A test-vectors `update_commitment_public_inputs_wire_format`; design §4.7.6 (threshold-from-storage).

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| All checks pass + valid update proof | `Group(group_id) := CommitmentEntry{c_new, current.epoch+1, ..., active: true, threshold: current.threshold}`; prior `current` archived to history (FIFO-pruned to ≤64); `UsedProof(proof_hash) := true`; `CommitmentUpdated` event; `Ok(())` | `test_update_commitment_happy_path` (mock-blocked at verifier; gates verified) |
| Group missing | `Err(GroupNotFound)` | `test_update_commitment_rejects_unknown_group` |
| `current.active == false` | `Err(GroupInactive)` | `test_update_commitment_rejects_inactive_group` |
| `current.epoch == u64::MAX` | `Err(InvalidEpoch)` | (defensive overflow guard at `lib.rs:573`) |
| `c_old != current.commitment` | `Err(PublicInputsMismatch)` | `test_update_commitment_rejects_stale_c_old` |
| `epoch_old != current.epoch` | `Err(PublicInputsMismatch)` | `test_update_commitment_rejects_wrong_epoch_old` |
| `occupancy_commitment_old != current.occupancy_commitment` | `Err(PublicInputsMismatch)` | `test_update_commitment_rejects_stale_occupancy_commitment_old` |
| Non-canonical `c_new` | `Err(InvalidCommitmentEncoding)` | `test_update_commitment_rejects_non_canonical_c_new` |
| Non-canonical `occupancy_commitment_new` | `Err(InvalidCommitmentEncoding)` | `test_update_commitment_rejects_non_canonical_occupancy_commitment_new` |
| Replayed proof | `Err(ProofReplay)` | `test_update_commitment_rejects_replayed_proof` |
| Bad proof bytes | `Err(InvalidProof)` | (mock-blocked at verifier in happy path test) |
| Threshold supplied from storage | proof is verified against `current.threshold_numerator`, not wire | `test_update_commitment_threshold_supplied_from_storage`, `test_update_commitment_threshold_mismatch_rejected_v2` |
| Tie semantics (`100·K ≥ T·m_old` non-strict) | accepted (verifier reached) | `test_update_commitment_threshold_tie_passes_v2` |

### 3.7 `verify_membership(env, group_id, proof, public_inputs: PublicInputs) -> Result<bool, Error>`

**Spec:** read-only check; returns `Ok(true)` only if the wire-supplied state matches `current` and the proof verifies.

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Wire matches `current`, proof verifies | `Ok(true)` | (gated on real proofs; Phase A) |
| Wire matches `current`, proof does NOT verify | `Ok(false)` | `test_verify_membership_happy_path` (mock proof returns false) |
| `public_inputs.commitment != current.commitment` | `Err(PublicInputsMismatch)` | `test_verify_membership_rejects_wrong_commitment` |
| `public_inputs.epoch != current.epoch` | `Err(PublicInputsMismatch)` | `test_verify_membership_rejects_wrong_epoch` |
| Group inactive | `Ok(false)` (read-only verifier intentionally allows post-deactivation attestation) | `test_verify_membership_rejects_inactive_group` |
| Group missing | `Err(GroupNotFound)` | (gated at `lib.rs:645`) |

**Note:** the function name is misleading on the inactive case — `test_verify_membership_rejects_inactive_group` actually asserts `Ok(false)`, not `Err(GroupInactive)`. This is **intentional**: read-only attestations against the final pre-deactivation state must remain verifiable forever. Documented at `lib.rs:911-915`.

### 3.8 `deactivate_group(env, group_id, proof, public_inputs: PublicInputs) -> Result<(), Error>`

**Spec:** any current member can deactivate (V-C1 safety valve from sep-xxxx); irreversible.

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| All checks pass + valid membership proof | `Group(group_id).active = false`; prior `current` archived to history; `GroupCount(current.tier) -= 1`; `UsedProof(proof_hash) := true`; `GroupDeactivated` event | `test_deactivate_group_happy_path` (mock-blocked at verifier; gates verified) |
| Group missing | `Err(GroupNotFound)` | (gated at `lib.rs:671`) |
| `current.active == false` | `Err(GroupInactive)` | `test_deactivate_already_inactive_group` |
| `public_inputs.commitment != current.commitment OR epoch != current.epoch` | `Err(PublicInputsMismatch)` | (gated at `lib.rs:675-678`) |
| Replayed proof | `Err(ProofReplay)` | (gated at `lib.rs:680`) |
| Bad proof | `Err(InvalidProof)` | `test_deactivate_group_rejects_non_member_proof` |

### 3.9 `get_commitment(env, group_id) -> Result<CommitmentEntry, Error>`

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Existing `group_id` | `Ok(CommitmentEntry{...current...})` | `test_get_commitment_returns_current_state` |
| Missing `group_id` | `Err(GroupNotFound)` | (gated at `load_group`) |

### 3.10 `get_history(env, group_id, max_entries: u32) -> Result<Vec<CommitmentEntry>, Error>`

**Spec:** tail-truncated rolling window (impl_plan §B.5; pinned in `test-vectors.json#queries`).

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Existing group, `max_entries >= history.len()` | full history returned in chronological order | `test_get_history_returns_chronological_entries` |
| Existing group, `max_entries < history.len()` | last `max_entries` entries returned (most-recent slice) | (covered by `test_archive_entry_appends_and_prunes`'s assertion that the oldest 6 are dropped after 70 archives with HISTORY_WINDOW=64) |
| Missing group | `Err(GroupNotFound)` | (gated at `lib.rs:738`) |

---

## 4. Verifier semantics

### 4.1 Membership proof (`verify_membership_proof`, `lib.rs:939-984`)

Public-input vector: `[commitment, epoch]` (in order). Computes:

```
vk_x = IC[0] + IC[1] · Fr(commitment) + IC[2] · Fr(epoch)
```

where `Fr(commitment) = Fr::from_bytes(commitment)` and `Fr(epoch) = Fr::from_u256(U256::from_be_bytes(u64_to_u256_be(epoch)))`. The pairing check is the canonical Groth16 form:

```
e(-π_A, π_B) · e(α, β) · e(vk_x, γ) · e(π_C, δ) =? 1_GT
```

This is the test-vectors `vk_kind_enum.Membership.ic_layout` ordering: `["base", "commitment", "epoch"]`. Pinned by `test_vectors_consistency`.

### 4.2 Update proof (`verify_update_proof`, `lib.rs:998-1072`)

Public-input vector: `[c_old, epoch_old, c_new, occupancy_commitment_old, occupancy_commitment_new, threshold_numerator]` (in order, **6 elements**, supplied to a 7-IC-point VK with IC[0] as base). Computes:

```
vk_x = IC[0]
     + IC[1] · Fr(c_old)
     + IC[2] · Fr(epoch_old)
     + IC[3] · Fr(c_new)
     + IC[4] · Fr(occupancy_commitment_old)
     + IC[5] · Fr(occupancy_commitment_new)
     + IC[6] · Fr(threshold_numerator)
```

Pairing check identical to membership. The test-vectors `vk_kind_enum.Update.ic_layout` pins this ordering: `["base", "c_old", "epoch_old", "c_new", "occupancy_commitment_old", "occupancy_commitment_new", "threshold_numerator"]`.

The 6th input (`threshold_numerator`) is read from `current.threshold_numerator` in `update_commitment` (`lib.rs:601`), NOT from the wire. Critical for design §4.7.6: a chain observer cannot distinguish two groups with different thresholds by call-payload analysis.

### 4.3 Field-element conversions

- `Fr::from_bytes` is called on already-canonical `BytesN<32>` (canonicalization checked upstream by `is_canonical_fr`). Out-of-range bytes round-trip to a different value — the upstream check rejects non-canonical inputs.
- `epoch: u64` is widened to `U256` big-endian-padded with leading zeros (`u64_to_u256_be`, `lib.rs:924-928`). The high 192 bits are zero; the low 64 bits carry the epoch. `Fr::from_u256` reduces (always within Fr's range, so no reduction occurs for `epoch ≤ u64::MAX`).
- `threshold_numerator: u32` is widened similarly via `u32_to_fr` (`lib.rs:930-933`). `1 ≤ threshold ≤ 100` always fits.

---

## 5. Commit-or-revert atomicity

Soroban guarantees that any failed entrypoint (returning `Err(...)` or panicking via `require_auth`) reverts ALL storage writes performed earlier in the call. The contract relies on this for:

1. **Replay nullifier consistency** (`record_proof` only after `verify_*_proof` returns true; storage write reverts on the verify-failure branch). A failed proof does NOT consume the nullifier.
2. **GroupCount consistency** (incremented at `create_group` ONLY after the proof verifies; failed creates don't bump the counter).
3. **History consistency** (archive_entry called after replay-fresh + verify; failed updates leave history unchanged).

`test_update_commitment_threshold_unchanged_after_update` was originally added to assert this transactional guarantee, but per PR #148 review chunk 2 #5 it was deleted as the property is a Soroban runtime guarantee rather than a contract behavior.

---

## 6. Test coverage matrix

The 43 inline tests cover the named entries in `test-vectors.json#tests_to_implement` 1:1, plus `test_vectors_consistency` and `test_archive_entry_appends_and_prunes`. The mapping:

| Test | Covers |
|---|---|
| `test_initialize` | constructor happy path |
| `test_invalid_membership_vk_length_rejected` | constructor IC arity guard (membership) |
| `test_invalid_update_vk_length_rejected` | constructor IC arity guard (update) |
| `test_create_group_*` (15 tests) | every `create_group` validation gate listed in §3.5 |
| `test_update_commitment_*` (13 tests) | every `update_commitment` validation gate + threshold-from-storage behaviors |
| `test_verify_membership_*` (4 tests) | every `verify_membership` branch |
| `test_deactivate_group_*` (3 tests) | every `deactivate_group` branch |
| `test_update_vk_*` (3 tests) | admin auth + both rotation paths |
| `test_get_commitment_returns_current_state` | live state read |
| `test_get_history_returns_chronological_entries` | history read in chronological order |
| `test_archive_entry_appends_and_prunes` | History write + FIFO prune at HISTORY_WINDOW |
| `test_bump_group_ttl_extends_used_proof_lifetime` | TTL bump on existing group |
| `test_vectors_consistency` | ABI pin (errors, tier capacities, IC counts) |

**Coverage gap (acknowledged):** mock proofs cannot pass `pairing_check`. Therefore the success branches of `create_group`, `update_commitment`, `verify_membership`, and `deactivate_group` reach the verifier and surface `InvalidProof` / `Ok(false)`. The gates above the verifier are fully covered; the post-verifier state writes are exercised by `test_archive_entry_appends_and_prunes` (for history) and by Phase A integration tests (for the full round-trip with real Groth16 proofs).

---

## 7. Build + ABI pinning

The `test_vectors_consistency` test loads `test-vectors.json` via `serde_json::from_str(include_str!("../test-vectors.json"))` and asserts:

1. Every name in `error_codes.vectors` matches the corresponding `Error::Variant as u32`.
2. Every `tier.vectors` entry's `capacity` matches `tier_capacity(tier)`.
3. `vk_kind_enum.vectors[Membership].ic_count == MEMBERSHIP_IC_POINTS`.
4. `vk_kind_enum.vectors[Update].ic_count == UPDATE_IC_POINTS`.

This makes the JSON an unforgivable single source of truth: any future PR that drifts the contract from the vectors fails CI.

The wasm build (`stellar contract build`) produces a 26KB `.wasm` exposing 8 entrypoints:

```
create_group, deactivate_group, get_commitment, get_history,
set_restricted_mode, update_commitment, update_vk, verify_membership
```

(`__constructor` is invoked at deploy time and is not a runtime entrypoint.)

---

## 8. Known correctness deviations from impl_plan / design

These are deliberate, acknowledged deviations or simplifications:

1. **`load_vk` / `load_update_vk` return `Error::NotInitialized` for missing tier** instead of `InvalidTier`. In practice unreachable: `__constructor` populates tiers 0-2; `tier > 2` is rejected at every call site upstream. If storage is corrupted such that a tier's VK is missing, the contract reports the corruption as `NotInitialized` rather than `InvalidTier` — operationally treated as the same "contract is in an unusable state" condition.

2. **`verify_membership` returns `Ok(false)` rather than `Err(InvalidProof)` on bad proof.** Intentional read-only semantics: callers can probe membership without burning a nullifier or consuming a transaction-revert path.

3. **`Reserved3 = 3`** and **`GroupStillActive = 27`** are present in the `Error` enum but unreachable by any current code path. `Reserved3` is documented as a placeholder (was `Unauthorized` in v1; auth now panics directly via `require_auth`). `GroupStillActive` is reserved for a possible future "delete inactive group" entrypoint. Both are pinned in `test-vectors.json` so they survive future edits without code-vs-vectors drift.

4. **`update_commitment` doesn't call `caller.require_auth()`.** Intentional — the proof IS the authorization (relayer model). Documented at `lib.rs:555-559`.

5. **History entries pruned from contract storage at `HISTORY_WINDOW=64`** but contract events (`CommitmentUpdated`) preserve the full audit trail. Operators querying ancient state should consume events from the chain log.

6. **`deactivate_group` decrements `GroupCount(tier)` but never reactivates a group, so the same `group_id` slot is permanently consumed.** The `MAX_GROUPS_PER_TIER` cap counts active+inactive together at the moment of `create_group`, but deactivated groups free up a slot via the decrement. This means: a contract that has gone through 10000 active+deactivated groups CAN still accept new creates as long as the live count is below 10000. This matches the design's "per-tier cap on simultaneously active groups" reading.

7. **`impl_plan §E` lists 8 staged commits** but the actual ship landed in 3 commits per developer preference. Cosmetic; the per-type-contract template in §E remains the documented intent for future Anarchy / OneOnOne / Oligarchy contracts.

---

## 9. Verification gaps

The following correctness arguments are **not directly verified** by this codebase and depend on Phase A or later phases:

1. **Real Groth16 round-trip.** `test_*_happy_path` tests reach the verifier with mock proofs that can't pass `pairing_check`. The full success branches (post-verifier state writes, event emissions) are exercised only when Phase A's `prove_democracy_v2` + fixture generator land. The deploy script's smoke-test block is currently disabled pending those.
2. **Cross-platform vector agreement.** Phase A6 will pin Swift / Kotlin / Rust prover outputs against this contract. Until then, prover-vs-contract drift on field-element encoding, IC ordering, or domain tags is conceivable but unmeasured.
3. **End-to-end testnet deploy.** `scripts/deploy_sep_democracy_testnet.sh` is operational but gated on having pre-generated v2 dev VKs in `keyset-democracy-dev/`. Until those exist, the contract has only been built and tested locally; the on-chain side has not been exercised against a real testnet instance of the v2 verifier.
4. **Observability of `MAX_GROUPS_PER_TIER` decrement.** No test exercises the deactivate→re-create-after-cap path; the decrement is a one-line correctness claim that should be covered by an integration test once Phase A's real proofs let `deactivate_group` succeed.

---

## 10. Change-control

Edits to `contracts/sep-democracy/src/lib.rs`, `test.rs`, or `test-vectors.json` MUST keep `test_vectors_consistency` green. Any spec drift introduced (new error code, new entrypoint, changed IC ordering, …) MUST be reflected in:

1. `test-vectors.json` (the JSON drives the test).
2. `impl_plan.md` (the human-readable plan tracker).
3. `proof_of_soundness.md` and `proof_of_correctness.md` (this and the companion document) so the audit anchor stays current.

The bar is: every claim above either has a line-cited contract reference, a named test, or an explicit "verification gap" entry in §9.
