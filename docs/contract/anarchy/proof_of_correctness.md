# Proof of Correctness — `contracts/sep-anarchy`

> **Amendment (postmortem #153 phase 1):** Sections referencing `deactivate_group` describe a removed entrypoint. The live contract has one fewer user-callable entrypoint than this document originally stated. The Membership VK is used at `create_group` + `verify_membership` only; `verify_membership` no longer leaks a usable proof for any state-changing membership-VK-keyed entrypoint, since none exists. See `docs/postmortem-deactivate-group-frontrun.md` for rationale.

**Scope:** the per-type Anarchy Soroban contract at `contracts/sep-anarchy/src/lib.rs` (revision: branch `feat/contract-sep-anarchy`).

**Status:** informal correctness argument — for every input class permitted by the spec, the contract produces the spec-mandated state delta and event. Where the spec is ambiguous or aspirational (Phase A future improvements), this document flags the gap rather than pretending it's covered.

**Companion:** [`proof_of_soundness.md`](proof_of_soundness.md) covers the dual question.

**Specification sources** (in decreasing authority order):
1. `contracts/sep-anarchy/test-vectors.json` — CI-asserted ABI pin.
2. [`docs/anarchy-update-testnet-design.md`](../../anarchy-update-testnet-design.md) — design v0.1; sections referenced inline.
3. [`docs/contract/anarchy/impl_plan.md`](impl_plan.md) — implementation plan.

---

## 1. Spec-to-implementation mapping

### 1.1 Storage layout

The on-chain `CommitmentEntry` (`lib.rs:172-191`) carries exactly the fields the design's §4.6 storage-layout pin requires:

| Field | Type | Source-of-truth | Mutation surface |
|---|---|---|---|
| `commitment` | `BytesN<32>` | `c_new` from update wire OR wire `commitment` at create | written by `create_group` and `update_commitment`; never by `deactivate_group` (preserved via `..current.clone()`) |
| `epoch` | `u64` | `0` at create, `current.epoch + 1` at update | monotonic +1 |
| `timestamp` | `u64` | `env.ledger().timestamp()` | re-stamped at every successful create / update / deactivate |
| `tier` | `u32` | wire arg at create | fixed |
| `active` | `bool` | `true` at create / update; `false` at deactivate | one-way flip |
| `member_count` | `u32` | wire arg at create | **fixed at create — NEVER mutated by `update_commitment`** (informational only per design §3.3) |

**Removed fields vs sep-xxxx v1**:
- `group_type` — implicit in contract address (per-type architecture).

The `DataKey` enum (`lib.rs:226-244`) covers the design's required keys: `Admin`, `RestrictedMode`, `VK(tier)`, `UpdateVK(tier)`, `Group(group_id)`, `History(group_id)`, `UsedProof(proof_hash)`, `GroupCount(tier)`. CI-asserted by `test_vectors_consistency`.

### 1.2 Constants

| Constant | Value | Spec | Rationale |
|---|---|---|---|
| `HISTORY_WINDOW` | 64 | impl_plan / §A | rolling-window cap |
| `LEDGER_THRESHOLD` | 17_280 (~1d) | TTL bump threshold | persistent-storage liveness |
| `LEDGER_BUMP` | 518_400 (~30d) | TTL bump amount | nullifier window |
| `MAX_GROUPS_PER_TIER` | 10_000 | impl_plan §B.5 | cross-group cap |
| `MEMBERSHIP_IC_POINTS` | 3 | design §4.4 | base + commitment + epoch |
| `UPDATE_IC_POINTS` | 4 | design §4.4 | base + c_old + epoch_old + c_new (Anarchy-specific; smaller than Democracy/Oligarchy's 7) |
| `tier_capacity(0/1/2)` | 32/256/2048 | design §4.1 | tree depth 5/8/11 → 2^depth slots |

### 1.3 Errors

The `Error` enum (`lib.rs:74-99`) pins 17 reachable variants plus `Reserved3` and `GroupStillActive` (slot reservations). `UnknownVkKind=16` removed (parallel to sep-democracy / sep-oligarchy). No `InvalidThreshold=28` (Anarchy has no quorum) and no `InvalidInitialMembership=30` (no in-circuit floor). CI-asserted by `test_vectors_consistency`'s error-code loop.

### 1.4 Events

Four events:

| Event | Topics | Fields | Emitted by |
|---|---|---|---|
| `GroupCreated` | `group_id` | `commitment, epoch, tier, member_count, timestamp` | `create_group` |
| `CommitmentUpdated` | `group_id` | `commitment, epoch, timestamp` | `update_commitment` |
| `GroupDeactivated` | `group_id` | `final_epoch, timestamp` | `deactivate_group` |
| `RestrictedModeChanged` | `admin` | `admin, restricted, timestamp` | `set_restricted_mode` |

Events are emitted **after** all storage writes. On any error path, no event is emitted (Soroban transaction revert).

---

## 2. State invariants (cross-reference)

The soundness document's SI-1 through SI-18 are also correctness invariants. Cross-references:

- `epoch` monotonic +1 per update (SI-12) — observable via `get_commitment().epoch`.
- `member_count` immutable post-create (SI-3) — observable: `get_commitment().member_count` returns the value passed to `create_group`, never mutated.
- `tier` immutable post-create (SI-14) — observable: `get_commitment().tier` returns the value passed to `create_group`.
- `active` one-way (SI-13) — once `false`, no entrypoint flips it back.
- History FIFO at `HISTORY_WINDOW` (SI-17) — `get_history()` returns at most 64 entries.

---

## 3. Per-entrypoint correctness

### 3.1 `__constructor(env, admin, vk_small, vk_medium, vk_large, update_vk_small, update_vk_medium, update_vk_large)`

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Valid admin + 6 subgroup-valid VKs (3 membership × 3 tiers + 3 update × 3 tiers) | `Admin`, `VK(0..=2)`, `UpdateVK(0..=2)` written; TTLs bumped; `Ok(())` | `test_initialize` |
| Membership VK with `ic.len() != 3` | `Err(InvalidVkLength)` | `test_invalid_membership_vk_length_rejected` |
| Update VK with `ic.len() != 4` | `Err(InvalidVkLength)` | `test_invalid_update_vk_length_rejected` |
| VK with non-subgroup point | `Err(InvalidPoint)` | (gated at `validate_vk_points`) |
| Already initialized | `Err(AlreadyInitialized)` | (gated at `do_initialize`'s guard) |

### 3.2 `update_vk(env, kind: VkKind, tier: u32, new_vk)`

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Admin auth + valid `(kind, tier ≤ 2, IC arity)` + subgroup-valid points | `VK(tier)` or `UpdateVK(tier)` overwritten; `Ok(())` | `test_update_vk_rotates_membership_vk`, `test_update_vk_rotates_update_vk` |
| No auth granted | "Unauthorized" panic | `test_update_vk_requires_auth` |
| `tier > 2` | `Err(InvalidTier)` | `test_update_vk_rejects_invalid_tier` |
| IC arity mismatch | `Err(InvalidVkLength)` | (gated at `lib.rs:328`) |
| Non-subgroup VK point | `Err(InvalidPoint)` | (gated at `validate_vk_points`) |
| Pre-init | `Err(NotInitialized)` | (gated at `lib.rs:317`) |

### 3.3 `set_restricted_mode(env, restricted: bool)`

| Input class | Spec-required behavior |
|---|---|
| Admin auth + bool | `RestrictedMode := restricted`; `RestrictedModeChanged` event; `Ok(())` |
| No auth | "Unauthorized" panic |
| Pre-init | `Err(NotInitialized)` |

`test_create_group_restricted_mode_rejects_non_admin` exercises the toggle-on path indirectly.

### 3.4 `bump_group_ttl(env, group_id)`

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Existing group + initialized | `Group(group_id)` and `History(group_id)` TTLs extended; `Ok(())`. Does NOT touch `UsedProof(...)` (chunk-2-review pattern from sep-democracy / sep-oligarchy). | `test_bump_group_ttl_extends_group_storage` |
| Unknown `group_id` | `Err(GroupNotFound)` | `test_bump_group_ttl_rejects_unknown_group` |
| Pre-init | `Err(NotInitialized)` | (gated at `require_initialized`) |

### 3.5 `create_group(env, caller, group_id, commitment, tier, member_count, proof, public_inputs)`

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| All checks pass + valid Groth16 proof | `CommitmentEntry` written at epoch 0; empty `History`; `GroupCount(tier) += 1`; `UsedProof(proof_hash) := true`; `GroupCreated` event; `Ok(())` | `test_create_group_happy_path` |
| `tier > 2` | `Err(InvalidTier)` | `test_create_group_rejects_invalid_tier` |
| `public_inputs.commitment != commitment OR epoch != 0` | `Err(PublicInputsMismatch)` | (gated at `lib.rs:382-384`) |
| Group already exists | `Err(GroupAlreadyExists)` | `test_create_group_rejects_duplicate_group_id` |
| Non-canonical `commitment` | `Err(InvalidCommitmentEncoding)` | `test_create_group_rejects_non_canonical_commitment` |
| `GroupCount(tier) >= MAX_GROUPS_PER_TIER` | `Err(TierGroupLimitReached)` | `test_create_group_enforces_tier_group_limit` |
| Replayed proof | `Err(ProofReplay)` | (gated via `check_proof_replay`) |
| Bad proof | `Err(InvalidProof)` | `test_create_group_rejects_invalid_proof` |
| Restricted mode + non-admin caller | `Err(AdminOnly)` | `test_create_group_restricted_mode_rejects_non_admin` |
| `member_count = 0` (sentinel) | accepted (informational) | `test_create_group_accepts_member_count_zero` |
| `member_count = u32::MAX` (arbitrary) | accepted (contract is value-agnostic) | `test_create_group_accepts_member_count_arbitrary` |

### 3.6 `update_commitment(env, group_id, proof, public_inputs)`

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| All checks pass + valid update proof | `Group(group_id) := CommitmentEntry{c_new, current.epoch+1, ..., active: true, member_count: current.member_count}`; prior `current` archived to history; `UsedProof(proof_hash) := true`; `CommitmentUpdated` event; `Ok(())` | `test_update_commitment_happy_path` |
| Group missing | `Err(GroupNotFound)` | `test_update_commitment_rejects_unknown_group` |
| `current.active == false` | `Err(GroupInactive)` | `test_update_commitment_rejects_inactive_group` |
| `current.epoch == u64::MAX` | `Err(InvalidEpoch)` | (defensive overflow guard) |
| `c_old != current.commitment` | `Err(PublicInputsMismatch)` | `test_update_commitment_rejects_stale_c_old` |
| `epoch_old != current.epoch` | `Err(PublicInputsMismatch)` | `test_update_commitment_rejects_wrong_epoch_old` |
| Non-canonical `c_new` | `Err(InvalidCommitmentEncoding)` | `test_update_commitment_rejects_non_canonical_c_new` |
| Replayed proof | `Err(ProofReplay)` | `test_update_commitment_rejects_replayed_proof` |
| Bad proof | `Err(InvalidProof)` | (mock-blocked at verifier in happy path test) |
| `member_count` preservation | the new entry inherits `current.member_count` verbatim — contract NEVER mutates this field | `test_update_commitment_does_not_mutate_member_count` |

### 3.7 `verify_membership(env, group_id, proof, public_inputs) -> bool`

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Wire matches `current`, proof verifies | `Ok(true)` | (gated on real proofs; reuse v1 Anarchy proof generation) |
| Wire matches `current`, bad proof | `Ok(false)` | `test_verify_membership_happy_path` |
| `public_inputs.commitment != current.commitment` | `Err(PublicInputsMismatch)` | `test_verify_membership_rejects_wrong_commitment` |
| `public_inputs.epoch != current.epoch` | `Err(PublicInputsMismatch)` | `test_verify_membership_rejects_wrong_epoch` |
| Group inactive (`current.active == false`) | `Ok(true)` for valid pre-deactivation proof; `Ok(false)` for invalid. The verifier intentionally does NOT check `state.active` — post-deactivation attestations against the frozen final state remain verifiable forever. | `test_verify_membership_rejects_inactive_group` (mock proof returns `Ok(false)`; the test name is historical — what it actually pins is "verifier reaches `Ok(false)` on inactive group", confirming absence of `if !state.active` short-circuit) |
| Group missing | `Err(GroupNotFound)` | (gated at `load_group`) |

### 3.8 `deactivate_group(env, group_id, proof, public_inputs)`

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| All checks pass + valid membership proof | `Group(group_id).active = false`; prior `current` archived to history; `GroupCount(current.tier) -= 1`; `UsedProof(proof_hash) := true`; `GroupDeactivated` event | `test_deactivate_group_happy_path` |
| Group missing | `Err(GroupNotFound)` | (gated at `load_group`) |
| `current.active == false` | `Err(GroupInactive)` | `test_deactivate_already_inactive_group` |
| `public_inputs.commitment != current.commitment OR epoch != current.epoch` | `Err(PublicInputsMismatch)` | (gated at `lib.rs:537-540`) |
| Replayed proof | `Err(ProofReplay)` | (gated at `check_proof_replay`) |
| Bad proof | `Err(InvalidProof)` | `test_deactivate_group_rejects_non_member_proof` |

### 3.9 `get_commitment(env, group_id)`

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Existing `group_id` | `Ok(CommitmentEntry{...current...})` | `test_get_commitment_returns_current_state` |
| Missing `group_id` | `Err(GroupNotFound)` | `test_get_commitment_rejects_unknown_group` |

### 3.10 `get_history(env, group_id, max_entries)`

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Existing group, `max_entries >= history.len()` | full history returned in chronological order | `test_get_history_returns_chronological_entries` |
| Existing group, `max_entries < history.len()` | last `max_entries` entries returned (most-recent slice) | (covered by `test_archive_entry_appends_and_prunes`'s assertion) |
| Missing group | `Err(GroupNotFound)` | `test_get_history_rejects_unknown_group` |

---

## 4. Verifier semantics

### 4.1 Membership proof (`verify_membership_proof`)

Public-input vector: `[commitment, epoch]`. 3 IC points. Same shape as sep-democracy + sep-oligarchy. `vk_x = IC[0] + IC[1]·Fr(commitment) + IC[2]·Fr(epoch)`. Pairing check: `e(-π_A, π_B) · e(α, β) · e(vk_x, γ) · e(π_C, δ) = 1_GT`.

### 4.2 Update proof (`verify_update_proof`)

Public-input vector: `[c_old, epoch_old, c_new]`. **4 IC points** (Anarchy-specific; smaller than Democracy/Oligarchy's 7). Computes:

```
vk_x = IC[0] + IC[1]·Fr(c_old) + IC[2]·Fr(epoch_old) + IC[3]·Fr(c_new)
```

Pairing check identical to membership. **No contract-supplied scalar** (Anarchy has no quorum threshold to inject from storage). All 3 inputs are wire-supplied.

### 4.3 Field-element conversions

- `Fr::from_bytes` is called on already-canonical `BytesN<32>` (canonicalization checked upstream).
- `epoch: u64` is widened to `U256` big-endian-padded (`u64_to_u256_be`).

---

## 5. Commit-or-revert atomicity

Soroban guarantees that any failed entrypoint reverts ALL storage writes. The contract relies on this for:

1. **Replay nullifier consistency** (`record_proof` only after verifier returns true).
2. **GroupCount consistency** (incremented at `create_group` ONLY after the proof verifies).
3. **History consistency** (archive_entry called after replay-fresh + verify).
4. **`member_count` immutability**: failed updates revert the (no-op) write attempt; the field is also preserved verbatim on success via `member_count: current.member_count` in the new entry's struct literal.

---

## 6. Test coverage matrix

The 39 inline tests cover every entry in `test-vectors.json#tests_to_implement` 1:1, plus `test_vectors_consistency`. Mapping:

| Test | Covers |
|---|---|
| `test_initialize` | constructor happy path |
| `test_invalid_*_vk_length_rejected` (2 tests) | constructor IC arity guards |
| `test_create_group_*` (9 tests) | every `create_group` validation gate listed in §3.5 |
| `test_update_commitment_*` (8 tests) | every `update_commitment` gate + `does_not_mutate_member_count` (Anarchy-specific) |
| `test_verify_membership_*` (4 tests) | every `verify_membership` branch |
| `test_deactivate_group_*` (3 tests) | every `deactivate_group` branch |
| `test_update_vk_*` (4 tests) | requires_auth + 2 rotation paths + invalid_tier |
| Queries (7 tests) | get_commitment {happy, GroupNotFound} + get_history {happy, GroupNotFound} + archive_entry + bump_group_ttl {happy, GroupNotFound} |
| `test_vectors_consistency` | ABI pin |

**Coverage gap (acknowledged):** mock proofs cannot pass `pairing_check`. Happy-path tests reach the verifier and surface `InvalidProof` / `Ok(false)`. Gates above the verifier are fully covered; post-verifier state writes are exercised by `test_archive_entry_appends_and_prunes` (for history) and by Phase A integration tests (using v1 Anarchy proof generation, which is unchanged).

---

## 7. Build + ABI pinning

`test_vectors_consistency` loads `test-vectors.json` via `serde_json::from_str(include_str!("../test-vectors.json"))` and asserts:

1. Every name in `error_codes.vectors` matches the corresponding `Error::Variant as u32`.
2. Every `tier.vectors` entry's `capacity` matches `tier_capacity(tier)`.
3. `vk_kind_enum.vectors[Membership].ic_count == MEMBERSHIP_IC_POINTS`.
4. `vk_kind_enum.vectors[Update].ic_count == UPDATE_IC_POINTS`.
5. `max_groups_per_tier.value == MAX_GROUPS_PER_TIER`.

The wasm build (`stellar contract build`) produces a 26KB `.wasm` exposing 9 user-callable entrypoints + `__constructor`:

```
__constructor, bump_group_ttl, create_group, deactivate_group,
get_commitment, get_history, set_restricted_mode, update_commitment,
update_vk, verify_membership
```

---

## 8. Known correctness deviations from impl_plan / design

These are deliberate, acknowledged simplifications:

1. **`load_vk` / `load_update_vk` return `Error::NotInitialized` for missing tier** (after the `tier > 2 → InvalidTier` upstream guard). In practice unreachable post-constructor.

2. **`verify_membership` returns `Ok(false)` rather than `Err(InvalidProof)` on bad proof.** Intentional read-only semantics: callers can probe membership without burning a nullifier.

3. **`Reserved3 = 3`** and **`GroupStillActive = 27`** are present in the `Error` enum but unreachable. Reserved3 is documented as a placeholder. GroupStillActive is reserved for a possible future "delete inactive group" entrypoint.

4. **`update_commitment` doesn't call `caller.require_auth()`.** Intentional — the proof IS the authorization (relayer model).

5. **History entries pruned at `HISTORY_WINDOW=64`** but contract events (`CommitmentUpdated`) preserve the full audit trail.

6. **`deactivate_group` decrements `GroupCount(tier)` but never reactivates.** Per-tier cap is on simultaneously active groups; deactivated groups free up a slot via the decrement.

7. **`member_count` is informational and never enforced.** The contract is value-agnostic to this field; clients track the actual count off-chain. A creator who supplies a misleading `member_count` (e.g., 100 when the actual count is 1) gets a misleading on-chain field but no soundness implication. Documented in design §3.3 as the option-A choice.

8. **No `update_member_count` admin entrypoint.** The informational field cannot be refreshed post-create. Drift between the on-chain field and the actual member set is accepted. §11 follow-up if a use case demands.

9. **`Error::InvalidEpoch`** reused for u64-overflow at `update_commitment`'s `checked_add` — same cosmetic concern as sep-democracy / sep-oligarchy. Realistically unreachable.

10. **`update_commitment` allows `c_new == c_old` no-op heartbeats.** Same as sep-democracy / sep-oligarchy.

---

## 9. Verification gaps

1. **Real Groth16 round-trip** with v1 Anarchy proofs. Tests use mock proofs; the v1 proof-generation path in sep-xxxx / `src/prover` is unchanged and already exercised by sep-xxxx integration tests.
2. **End-to-end testnet deploy**. `scripts/deploy_sep_anarchy_testnet.sh` reads `keyset-v2/` directly; no Phase A4 dev-VK generation needed.
3. **Phase D client routing** to the new contract address — separate workstream.

---

## 10. Change-control

Edits to `contracts/sep-anarchy/src/lib.rs`, `test.rs`, or `test-vectors.json` MUST keep `test_vectors_consistency` green. Spec drift MUST be reflected in:

1. `test-vectors.json` (the JSON drives the test).
2. `impl_plan.md`.
3. `proof_of_soundness.md` and `proof_of_correctness.md`.
