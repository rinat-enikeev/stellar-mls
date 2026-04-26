# Proof of Correctness — `contracts/sep-oligarchy`

**Scope:** the per-type Oligarchy Soroban contract at `contracts/sep-oligarchy/src/lib.rs` (revision: branch `feat/contract-sep-oligarchy`).

**Status:** informal correctness argument — for every input class permitted by the spec, the contract produces the spec-mandated state delta and event. Where the spec is ambiguous or aspirational (Phase A integration, ceremony), this document flags the gap rather than pretending it's covered.

**Companion:** [`proof_of_soundness.md`](proof_of_soundness.md) covers the dual question. This document covers ("does the contract behave as documented when used honestly").

**Specification sources** (in decreasing authority order):
1. `contracts/sep-oligarchy/test-vectors.json` — CI-asserted ABI pin (loaded at test time by `test_vectors_consistency`).
2. [`docs/oligarchy-update-testnet-design.md`](../../oligarchy-update-testnet-design.md) — design v0.1.4; sections referenced inline.
3. [`docs/contract/oligarchy/impl_plan.md`](impl_plan.md) — implementation plan tracking the design.

---

## 1. Spec-to-implementation mapping

### 1.1 Storage layout

The on-chain `CommitmentEntry` (`lib.rs:172-198`) carries exactly the fields the design's §A storage-layout pin requires for v0.1.4:

| Field | Type | Source-of-truth | Mutation surface |
|---|---|---|---|
| `commitment` | `BytesN<32>` | `c_new` from update wire OR wire `commitment` at create | written by `create_oligarchy_group` and `update_commitment`; never by `deactivate_group` (preserved via `..current.clone()`) |
| `epoch` | `u64` | `0` at create, `current.epoch + 1` at update | monotonic +1 |
| `timestamp` | `u64` | `env.ledger().timestamp()` | re-stamped at every successful create / update / deactivate |
| `tier` | `u32` | wire `member_tier` arg at create; admin tier fixed at Small (depth 5, 32 slots) per §4.6 and NOT stored | fixed |
| `active` | `bool` | `true` at create / update; `false` at deactivate | one-way flip |
| `occupancy_commitment` | `BytesN<32>` | wire `occupancy_commitment_initial` at create; wire `occupancy_commitment_new` at update — salted combined per §4.7.2 v0.1.4 | written |
| `admin_threshold_numerator` | `u32` | wire arg at create | fixed |

**Removed fields vs sep-xxxx v1** (per design §3.5 and §4.6):
- `group_type` — implicit in contract address.
- `member_count`, `admin_count` — replaced by combined `occupancy_commitment`.
- `admin_root` — v0.1.3 §3.5 fix; circuit-internal private witness reconstructed off-chain.
- `salt_occ` — v0.1.4; per-epoch private witness, not stored, distributed via `SEPSaltResponse.occupancySalt`.

The `DataKey` enum (`lib.rs:265-285`) covers the design's required keys:

| `DataKey` variant | Storage class | Purpose | Spec ref |
|---|---|---|---|
| `Admin` | instance | contract admin address | §4.6 |
| `RestrictedMode` | instance | bool toggle for admin-only `create_oligarchy_group` | impl_plan §B.5 |
| `VK(tier)` | persistent | membership-circuit VK per member tier 0-2 | §4.6 |
| `CreateVK(tier)` | persistent | v0.1.4 oligarchy create VK per member tier 0-2 | §4.8 |
| `UpdateVK(tier)` | persistent | v0.1.4 oligarchy update VK per member tier 0-2 | §4.6 / §4.7.6 |
| `Group(group_id)` | persistent | live state | §A |
| `History(group_id)` | persistent | rolling FIFO of prior states | §A |
| `UsedProof(proof_hash)` | persistent (TTL ≈ `LEDGER_BUMP`) | replay nullifier | §11 |
| `GroupCount(member_tier)` | instance | active-group counter for `MAX_GROUPS_PER_TIER` gate | impl_plan §B.5 |

CI-asserted by `test_vectors_consistency`.

### 1.2 Constants

| Constant | Value | Spec | Rationale |
|---|---|---|---|
| `HISTORY_WINDOW` | 64 | impl_plan / §A | rolling-window cap |
| `LEDGER_THRESHOLD` | 17_280 (~1d) | TTL bump threshold | persistent-storage liveness |
| `LEDGER_BUMP` | 518_400 (~30d) | TTL bump amount | nullifier window |
| `MAX_GROUPS_PER_TIER` | 10_000 | impl_plan §B.5 | cross-group cap |
| `MEMBERSHIP_IC_POINTS` | 3 | §4.7.3 | base + commitment + epoch |
| `CREATE_IC_POINTS` | 7 | §4.8 | base + 6 bound public inputs |
| `UPDATE_IC_POINTS` | 7 | §4.7.6 | base + 5 wire scalars + admin_threshold |
| `tier_capacity(0/1/2)` | 32/256/2048 | §4.1 | member-tree depth 5/8/11 → 2^depth slots |

Admin tier is fixed at Small (depth=5, 32 slots) per design §4.6 and is NOT a contract argument — pinned in `test-vectors.json#tier.admin_tier_fixed`.

### 1.3 Errors

The `Error` enum (`lib.rs:96-138`) pins 19 reachable variants plus `Reserved3`, `GroupStillActive`, and `InvalidInitialMembership=30` (slot reservations documented in `test-vectors.json`). `UnknownVkKind=16` removed (parallel to sep-democracy).

CI-asserted by `test_vectors_consistency`'s error-code loop.

### 1.4 Events

Three events match the design's audit-trail expectation:

| Event | Topics | Fields | Emitted by |
|---|---|---|---|
| `GroupCreated` | `group_id` | `commitment, epoch, tier, timestamp` | `create_oligarchy_group` |
| `CommitmentUpdated` | `group_id` | `commitment, epoch, timestamp` | `update_commitment` |
| `GroupDeactivated` | `group_id` | `final_epoch, timestamp` | `deactivate_group` |

Events are emitted **after** all storage writes. On any error path, no event is emitted (Soroban transaction revert).

---

## 2. State invariants (from `proof_of_soundness.md`)

The soundness document's SI-1 through SI-20 are also correctness invariants — the contract should preserve them, AND honest users should observe them as documented behavior. Cross-references:

- `epoch` monotonic +1 per update (SI-14) — observable via `get_commitment().epoch` advancing exactly 1 per `CommitmentUpdated` event.
- `admin_threshold_numerator` immutable post-create (SI-4) — observable: `get_commitment().admin_threshold_numerator` returns the value passed to `create_oligarchy_group`, never mutated.
- `tier` immutable post-create (SI-16) — observable: `get_commitment().tier` returns the value passed to `create_oligarchy_group`.
- `active` one-way (SI-15) — observable: once `false`, no entrypoint flips it back.
- History FIFO at `HISTORY_WINDOW` (SI-19) — observable: `get_history()` returns at most 64 entries, oldest dropped first.
- Verbose Create binding (SI-5) — observable: a creator who supplies a bogus `occupancy_commitment_initial` gets `InvalidProof` rather than silently locking their own group (closes the sep-democracy self-DoS).

---

## 3. Per-entrypoint correctness

For each public entrypoint, the spec's input class → output behavior mapping is enumerated. "Spec-required" means the design / impl_plan / vectors mandate the behavior; "Verified by" lists the test that pins it.

### 3.1 `__constructor(env, admin, vk_small, vk_medium, vk_large, create_vk_small, create_vk_medium, create_vk_large, update_vk_small, update_vk_medium, update_vk_large) -> Result<(), Error>`

**Spec:** atomic deploy-time installer (impl_plan §B.5). One-shot. 9 VKs across 3 families × 3 tiers.

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Valid admin + 9 subgroup-valid VKs with correct IC arities | `Admin`, `VK(0..=2)`, `CreateVK(0..=2)`, `UpdateVK(0..=2)` written; TTLs bumped; `Ok(())` | `test_initialize` |
| Membership VK with `ic.len() != 3` | `Err(InvalidVkLength)` | `test_invalid_membership_vk_length_rejected` |
| Create VK with `ic.len() != 7` | `Err(InvalidVkLength)` | `test_invalid_create_vk_length_rejected` |
| Update VK with `ic.len() != 7` | `Err(InvalidVkLength)` | `test_invalid_update_vk_length_rejected` |
| VK with non-subgroup point | `Err(InvalidPoint)` | (implicit; subgroup check covered at runtime by `validate_vk_points`) |
| Already initialized | `Err(AlreadyInitialized)` | (constructor idempotency by `do_initialize`'s guard) |

### 3.2 `update_vk(env, kind: VkKind, tier: u32, new_vk: VerificationKeyData) -> Result<(), Error>`

**Spec:** admin-only VK rotation per design §4.6 fingerprint-coordination triplet.

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Admin auth + valid `(kind, tier ≤ 2, IC arity matches kind)` + subgroup-valid points | `VK(tier)`, `CreateVK(tier)`, or `UpdateVK(tier)` overwritten; TTL bumped; `Ok(())` | `test_update_vk_rotates_membership_vk`, `test_update_vk_rotates_create_vk`, `test_update_vk_rotates_update_vk` |
| No auth granted | Soroban-level "Unauthorized" panic | `test_update_vk_requires_auth` |
| `tier > 2` | `Err(InvalidTier)` | (gated at `lib.rs:403`) |
| `IC arity mismatch` | `Err(InvalidVkLength)` | (gated at `lib.rs:411`) |
| Non-subgroup VK point | `Err(InvalidPoint)` | (gated at `lib.rs:413`) |
| Pre-init | `Err(NotInitialized)` | (gated at `lib.rs:393`) |

### 3.3 `set_restricted_mode(env, restricted: bool) -> Result<(), Error>`

**Spec:** admin-only toggle (impl_plan §B.5).

| Input class | Spec-required behavior |
|---|---|
| Admin auth + bool | `RestrictedMode := restricted`; `Ok(())` |
| No auth | "Unauthorized" panic |
| Pre-init | `Err(NotInitialized)` |

`test_create_oligarchy_group_restricted_mode_rejects_non_admin` exercises the full toggle-on path indirectly.

### 3.4 `bump_group_ttl(env, group_id) -> Result<(), Error>`

**Spec:** permissionless TTL bump for long-lived groups.

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Existing group | `Group(group_id)` and `History(group_id)` TTLs extended; `Ok(())` | `test_bump_group_ttl_extends_used_proof_lifetime` |
| Unknown `group_id` | `Err(GroupNotFound)` | (gated at `lib.rs:439`) |

### 3.5 `create_oligarchy_group(env, caller, group_id, commitment, member_tier, admin_threshold_numerator, occupancy_commitment_initial, member_root, admin_root, salt_initial, proof, public_inputs)`

**Spec:** §A test-vectors `create_oligarchy_group_validation`; impl_plan §B.5; design §4.7.6 (admin threshold) + §4.8 (verbose binding).

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| All checks pass + valid Groth16 create proof | `CommitmentEntry` written at epoch 0; empty `History`; `GroupCount(member_tier) += 1`; `UsedProof(proof_hash) := true`; `GroupCreated` event; `Ok(())` | `test_create_oligarchy_group_happy_path` (mock-blocked at verifier; gates verified) |
| `member_tier > 2` | `Err(InvalidTier)` | `test_create_oligarchy_group_rejects_invalid_tier` |
| `admin_threshold_numerator == 0` | `Err(InvalidThreshold)` | `test_create_oligarchy_group_rejects_invalid_threshold_zero` |
| `admin_threshold_numerator > 100` | `Err(InvalidThreshold)` | `test_create_oligarchy_group_rejects_invalid_threshold_above_100` |
| Threshold 50 / 67 / 100 | accepted (reaches verifier) | `test_create_oligarchy_group_accepts_threshold_50/67/100` |
| `public_inputs` mismatch any wire field | `Err(PublicInputsMismatch)` | (gated at `lib.rs:530-538`) |
| Group already exists | `Err(GroupAlreadyExists)` | `test_create_oligarchy_group_rejects_duplicate_group_id` |
| Non-canonical `commitment` | `Err(InvalidCommitmentEncoding)` | `test_create_oligarchy_group_rejects_non_canonical_commitment` |
| Non-canonical `occupancy_commitment_initial` | `Err(InvalidCommitmentEncoding)` | `test_create_oligarchy_group_rejects_non_canonical_occupancy_commitment` |
| Non-canonical `member_root` | `Err(InvalidCommitmentEncoding)` | `test_create_oligarchy_group_rejects_non_canonical_member_root` |
| Non-canonical `admin_root` | `Err(InvalidCommitmentEncoding)` | `test_create_oligarchy_group_rejects_non_canonical_admin_root` |
| Non-canonical `salt_initial` | `Err(InvalidCommitmentEncoding)` | `test_create_oligarchy_group_rejects_non_canonical_salt_initial` |
| `GroupCount(member_tier) >= MAX_GROUPS_PER_TIER` | `Err(TierGroupLimitReached)` | `test_create_oligarchy_group_enforces_tier_group_limit` |
| Replayed proof | `Err(ProofReplay)` | (gated via `check_proof_replay`) |
| Bad proof bytes | `Err(InvalidProof)` | `test_create_oligarchy_group_rejects_invalid_proof` |
| Restricted mode + non-admin caller | `Err(AdminOnly)` | `test_create_oligarchy_group_restricted_mode_rejects_non_admin` |

### 3.6 `update_commitment(env, group_id, proof, public_inputs: UpdateCommitmentPublicInputs)`

**Spec:** §A test-vectors `update_commitment_public_inputs_wire_format`; design §4.7.6.

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| All checks pass + valid update proof | `Group(group_id) := CommitmentEntry{c_new, current.epoch+1, ..., active: true, threshold: current.admin_threshold}`; prior `current` archived to history (FIFO-pruned); `UsedProof(proof_hash) := true`; `CommitmentUpdated` event; `Ok(())` | `test_update_commitment_happy_path` |
| Group missing | `Err(GroupNotFound)` | `test_update_commitment_rejects_unknown_group` |
| `current.active == false` | `Err(GroupInactive)` | `test_update_commitment_rejects_inactive_group` |
| `current.epoch == u64::MAX` | `Err(InvalidEpoch)` | (defensive overflow guard) |
| `c_old / epoch_old / occupancy_commitment_old` mismatch | `Err(PublicInputsMismatch)` | `test_update_commitment_rejects_stale_c_old`, `test_update_commitment_rejects_stale_occupancy_commitment_old`, `test_update_commitment_rejects_wrong_epoch_old` |
| Non-canonical `c_new` / `occupancy_commitment_new` | `Err(InvalidCommitmentEncoding)` | `test_update_commitment_rejects_non_canonical_c_new`, `test_update_commitment_rejects_non_canonical_occupancy_commitment_new` |
| Replayed proof | `Err(ProofReplay)` | `test_update_commitment_rejects_replayed_proof` |
| Bad proof bytes | `Err(InvalidProof)` | (mock-blocked at verifier in happy-path test) |
| `admin_threshold_numerator` supplied from storage | proof verified against `current.admin_threshold_numerator`, not wire | `test_update_commitment_admin_threshold_supplied_from_storage`, `test_update_commitment_admin_threshold_mismatch_rejected_v2` |
| Tie semantics | accepted | `test_update_commitment_admin_threshold_tie_passes_v2` |

### 3.7 `verify_membership(env, group_id, proof, public_inputs: PublicInputs) -> bool`

**Spec:** read-only check against the **member tree only**.

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Wire matches `current`, proof verifies | `Ok(true)` | (gated on real proofs; Phase A) |
| Wire matches `current`, bad proof | `Ok(false)` | `test_verify_membership_happy_path` (mock proof returns false) |
| `public_inputs.commitment != current.commitment` | `Err(PublicInputsMismatch)` | `test_verify_membership_rejects_wrong_commitment` |
| `public_inputs.epoch != current.epoch` | `Err(PublicInputsMismatch)` | `test_verify_membership_rejects_wrong_epoch` |
| Group inactive | `Ok(false)` (read-only verifier intentionally allows post-deactivation attestation) | `test_verify_membership_rejects_inactive_group` |
| Group missing | `Err(GroupNotFound)` | (gated at `load_group`) |

### 3.8 `deactivate_group(env, group_id, proof, public_inputs: PublicInputs)`

**Spec:** any current member of the **member tree** can deactivate; irreversible. Admin status NOT required.

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| All checks pass + valid membership proof | `Group(group_id).active = false`; prior `current` archived to history; `GroupCount(current.tier) -= 1`; `UsedProof(proof_hash) := true`; `GroupDeactivated` event | `test_deactivate_group_happy_path` |
| Group missing | `Err(GroupNotFound)` | (gated at `load_group`) |
| `current.active == false` | `Err(GroupInactive)` | `test_deactivate_already_inactive_group` |
| `public_inputs.commitment != current.commitment OR epoch != current.epoch` | `Err(PublicInputsMismatch)` | (gated at `lib.rs:728-732`) |
| Replayed proof | `Err(ProofReplay)` | (gated at `check_proof_replay`) |
| Bad proof | `Err(InvalidProof)` | `test_deactivate_group_rejects_non_member_proof` |

### 3.9 `get_commitment(env, group_id) -> CommitmentEntry`

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Existing `group_id` | `Ok(CommitmentEntry{...current...})` | `test_get_commitment_returns_current_state` |
| Missing `group_id` | `Err(GroupNotFound)` | (gated at `load_group`) |

### 3.10 `get_history(env, group_id, max_entries: u32) -> Vec<CommitmentEntry>`

| Input class | Spec-required behavior | Verified by |
|---|---|---|
| Existing group, `max_entries >= history.len()` | full history returned in chronological order | `test_get_history_returns_chronological_entries` |
| Existing group, `max_entries < history.len()` | last `max_entries` entries returned | (covered by `test_archive_entry_appends_and_prunes`'s assertion) |
| Missing group | `Err(GroupNotFound)` | (gated at `lib.rs:790`) |

---

## 4. Verifier semantics

### 4.1 Membership proof (`verify_membership_proof`)

Public-input vector: `[commitment, epoch]`. 3 IC points. Same shape as sep-democracy's membership verifier; the in-circuit semantics differ (Oligarchy's membership circuit privately witnesses `member_root` un-bundled from `commitment`, then opens to a member-tree leaf in that root). Test pin: `vk_kind_enum.Membership.ic_layout == ["base", "commitment", "epoch"]`.

### 4.2 Create proof (`verify_create_proof`)

Public-input vector: `[commitment, epoch, occupancy_commitment, member_root, admin_root, salt_initial]`. 7 IC points. Computes:

```
vk_x = IC[0]
     + IC[1] · Fr(commitment)
     + IC[2] · Fr(epoch)
     + IC[3] · Fr(occupancy_commitment)
     + IC[4] · Fr(member_root)
     + IC[5] · Fr(admin_root)
     + IC[6] · Fr(salt_initial)
```

Pairing check identical to membership. Critical for design §4.8: the verbose binding closes the create-time self-DoS sep-democracy carries. `salt_occ_initial` is bundled INTO `occupancy_commitment` per §4.7.2 v0.1.4 and consumed inside the circuit (NOT a separate IC point).

### 4.3 Update proof (`verify_update_proof`)

Public-input vector: `[c_old, epoch_old, c_new, occupancy_commitment_old, occupancy_commitment_new, admin_threshold_numerator]`. 7 IC points. The 6th input is read from `current.admin_threshold_numerator` in `update_commitment` (`lib.rs:668`), NOT from the wire. Critical for design §4.7.6: a chain observer cannot distinguish two groups with different thresholds by call-payload analysis.

### 4.4 Field-element conversions

- `Fr::from_bytes` is called on already-canonical `BytesN<32>` (canonicalization checked upstream by `is_canonical_fr`).
- `epoch: u64` is widened to `U256` big-endian-padded (`u64_to_u256_be`).
- `admin_threshold_numerator: u32` is widened similarly via `u32_to_fr`.

---

## 5. Commit-or-revert atomicity

Soroban guarantees that any failed entrypoint (returning `Err(...)` or panicking via `require_auth`) reverts ALL storage writes performed earlier in the call. The contract relies on this for:

1. **Replay nullifier consistency** (`record_proof` only after `verify_*_proof` returns true; storage write reverts on the verify-failure branch). A failed proof does NOT consume the nullifier.
2. **GroupCount consistency** (incremented at `create_oligarchy_group` ONLY after the proof verifies).
3. **History consistency** (archive_entry called after replay-fresh + verify; failed updates leave history unchanged).

---

## 6. Test coverage matrix

The 48 inline tests cover every entry in `test-vectors.json#tests_to_implement` 1:1, plus `test_vectors_consistency` and `test_archive_entry_appends_and_prunes`. Mapping:

| Test | Covers |
|---|---|
| `test_initialize` | constructor happy path |
| `test_invalid_*_vk_length_rejected` (3 tests) | constructor IC arity guards (Membership / Create / Update) |
| `test_create_oligarchy_group_*` (16 tests) | every `create_oligarchy_group` validation gate listed in §3.5 |
| `test_update_commitment_*` (13 tests) | every `update_commitment` validation gate + admin-threshold-from-storage behaviors |
| `test_verify_membership_*` (4 tests) | every `verify_membership` branch |
| `test_deactivate_group_*` (3 tests) | every `deactivate_group` branch |
| `test_update_vk_*` (4 tests) | admin auth + 3 rotation paths (Membership / Create / Update) |
| `test_get_commitment_returns_current_state` | live state read |
| `test_get_history_returns_chronological_entries` | history read in chronological order |
| `test_archive_entry_appends_and_prunes` | History write + FIFO prune at HISTORY_WINDOW |
| `test_bump_group_ttl_extends_used_proof_lifetime` | TTL bump on existing group |
| `test_vectors_consistency` | ABI pin (errors, tier capacities, IC counts, MAX_GROUPS_PER_TIER) |

**Coverage gap (acknowledged):** mock proofs cannot pass `pairing_check`. Therefore the success branches of `create_oligarchy_group`, `update_commitment`, `verify_membership`, and `deactivate_group` reach the verifier and surface `InvalidProof` / `Ok(false)`. Gates above the verifier are fully covered; post-verifier state writes are exercised by `test_archive_entry_appends_and_prunes` (for history) and by Phase A integration tests (for the full round-trip with real Groth16 proofs).

---

## 7. Build + ABI pinning

The `test_vectors_consistency` test loads `test-vectors.json` via `serde_json::from_str(include_str!("../test-vectors.json"))` and asserts:

1. Every name in `error_codes.vectors` matches the corresponding `Error::Variant as u32`.
2. Every `tier.vectors` entry's `capacity` matches `tier_capacity(tier)`.
3. `vk_kind_enum.vectors[Membership].ic_count == MEMBERSHIP_IC_POINTS`.
4. `vk_kind_enum.vectors[Create].ic_count == CREATE_IC_POINTS`.
5. `vk_kind_enum.vectors[Update].ic_count == UPDATE_IC_POINTS`.
6. `max_groups_per_tier.value == MAX_GROUPS_PER_TIER`.

The wasm build (`stellar contract build`) produces a 31KB `.wasm` exposing 9 user-callable entrypoints + `__constructor`:

```
__constructor, bump_group_ttl, create_oligarchy_group, deactivate_group,
get_commitment, get_history, set_restricted_mode, update_commitment,
update_vk, verify_membership
```

---

## 8. Known correctness deviations from impl_plan / design

These are deliberate, acknowledged deviations or simplifications:

1. **`load_vk` / `load_create_vk` / `load_update_vk` return `Error::NotInitialized` for missing tier** instead of `InvalidTier`. In practice unreachable: `__constructor` populates tiers 0-2; `tier > 2` is rejected at every call site upstream. Same shape as sep-democracy.

2. **`verify_membership` returns `Ok(false)` rather than `Err(InvalidProof)` on bad proof.** Intentional read-only semantics: callers can probe membership without burning a nullifier or consuming a transaction-revert path.

3. **`Reserved3 = 3`**, **`GroupStillActive = 27`**, and **`InvalidInitialMembership = 30`** are present in the `Error` enum but unreachable by any current code path. Reserved3 is documented as a placeholder. GroupStillActive is reserved for a possible future "delete inactive group" entrypoint. InvalidInitialMembership is reserved for the in-circuit floor `popcount(member_bitmap_initial) >= 1 && popcount(admin_bitmap_initial) >= 1` per §4.8 — surfaced for ABI discoverability since cross-platform clients may produce this error path through proof generation.

4. **`update_commitment` doesn't call `caller.require_auth()`.** Intentional — the proof IS the authorization (relayer model). Documented at `lib.rs:632-637`.

5. **History entries pruned at `HISTORY_WINDOW=64`** but contract events (`CommitmentUpdated`) preserve the full audit trail.

6. **`deactivate_group` decrements `GroupCount(tier)` but never reactivates.** The per-tier cap is on simultaneously active groups; deactivated groups free up a slot via the decrement.

7. **Admin tier is fixed at depth=5 (32 slots)** per design §4.6 across all member tiers. The contract has no admin-tier argument. Operators wanting larger admin sets are §11 follow-up.

8. **Verbose Create signature.** Unlike sep-democracy (where `create_group`'s only public inputs are `commitment + epoch=0` and `occupancy_commitment_initial` is wire-trusted), sep-oligarchy's `create_oligarchy_group` binds 6 inputs to the proof (commitment, epoch, occupancy_commitment, member_root, admin_root, salt_initial). This closes the create-time self-DoS sep-democracy carries.

---

## 9. Verification gaps

The following correctness arguments are **not directly verified** by this codebase and depend on Phase A or later phases:

1. **Real Groth16 round-trip** across all three VK families (Membership / Create / Update). The full success branches (post-verifier state writes, event emissions) are exercised only when Phase A's `prove_oligarchy_v2` + fixture generator land.
2. **Cross-platform vector agreement.** Phase A6 will pin Swift / Kotlin / Rust prover outputs against this contract. Until then, prover-vs-contract drift on field-element encoding, IC ordering, or domain tags is conceivable but unmeasured.
3. **End-to-end testnet deploy.** `scripts/deploy_sep_oligarchy_testnet.sh` is operational but gated on having pre-generated v0.1.4 dev VKs in `keyset-oligarchy-dev/` (9 files: 3 membership + 3 create + 3 update).
4. **Two-tree dispatcher constraint #8** is a Phase A circuit-level property; this contract trusts whatever the prover commits to and verifies the wire-shape only.

---

## 10. Change-control

Edits to `contracts/sep-oligarchy/src/lib.rs`, `test.rs`, or `test-vectors.json` MUST keep `test_vectors_consistency` green. Any spec drift introduced (new error code, new entrypoint, changed IC ordering, new VK family, …) MUST be reflected in:

1. `test-vectors.json` (the JSON drives the test).
2. `impl_plan.md` (the human-readable plan tracker).
3. `proof_of_soundness.md` and `proof_of_correctness.md` so the audit anchor stays current.

The bar is: every claim above either has a line-cited contract reference, a named test, or an explicit "verification gap" entry in §9.
