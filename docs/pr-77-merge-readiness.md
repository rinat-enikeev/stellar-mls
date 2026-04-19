## Preamble

```
Title:   PR #77 Merge-Readiness Audit
Author:  @rinat-enikeev + Claude (governance rollout review)
Status:  Ready to merge — one follow-up test-coverage PR recommended
Created: 2026-04-19
Scope:   PR https://github.com/rinat-enikeev/stellar-mls/pull/77
         (branch monstr/governance, ~100 changed files, +55k / -227)
Related: issue #78 (developer rollout), docs/group-governance-types-design.md,
         docs/democracy-circuit-ceremony.md
```

This document audits PR #77 (`governance: DemocracyUpdate + AdminUpdate circuits
+ VOTE_CAST v2`) across three dimensions — **soundness**, **correctness**,
and **cleanness** — and gives a merge recommendation.

---

## 1. Verdict

**Ready to merge.** The PR is `MERGEABLE` with `mergeStateStatus: CLEAN`, CI
is green (android-build ✓, ios-build ✓), all three code-review threads have
been materially addressed, and the already-deployed testnet rollout
(contract `CBIW2N…BZWT`) has passed an end-to-end smoke that exercises
every new dispatcher arm. The only outstanding items are **test-coverage
deepening in the Democracy circuit** (three scenarios currently covered
only implicitly) — these are belt-and-suspenders and do not block Phase D
of the rollout, which needs the Phase 2 trusted-setup ceremony before
mainnet anyway.

Three recommendations — none blocking:

1. **Follow-up PR:** add direct tests for Democracy `Insert` / `Remove`
   delta variants, plus explicit reject-path tests for `K = 0` and
   descending signer indices. §5.2.
2. **Minor doc polish:** reconcile the `tier_capacity` doc-comment
   ("behaviour for out-of-range tiers is undefined") with the actual
   return-0 implementation. §7.1.
3. **Operational:** before mainnet Phase D, re-run the Phase 2 ceremony
   per `docs/democracy-circuit-ceremony.md` to replace the dev VKs. The
   current testnet VKs are explicitly marked `DEV`. §7.2.

---

## 2. State at a glance

| Dimension | Status |
|-----------|--------|
| PR state | `OPEN` |
| Mergeability | `MERGEABLE` / `CLEAN` |
| Review decision | Comments only (no blocking change requests) |
| CI — android-build | ✓ SUCCESS |
| CI — ios-build | ✓ SUCCESS |
| Contract `cargo test` | ✓ 106 passed, 0 failed |
| Circuit `cargo test` (democracy) | ✓ 6 passed, 0 failed |
| Contract `stellar contract build` | ✓ WASM produced cleanly |
| Testnet smoke (`scripts/smoke-governance-testnet.sh`) | ✓ 6/6 checks |
| Reviewed by | `@gramyzer` (×2), `@releaseng` (×1) |

---

## 3. Soundness

### 3.1 Protocol-review coverage

The second `@gramyzer` review (the hard-critique protocol-soundness pass)
raised six items. All six are resolved in the working tree as of commit
`75536e8` / `eb31294`:

| # | Item | Resolution | Where |
|---|------|------------|-------|
| 1 | `K ≥ 1` constraint missing from `DemocracyUpdateCircuit` | Added `active_bits[0].enforce_equal(&Boolean::TRUE)` — equivalent to `K ≥ 1` given the prefix-monotonicity constraint | `src/circuit/democracy.rs:464` |
| 2 | Depth-bit range check on field-element index diff — soundness not documented | Added explicit comment (~l.381–394) explaining the `r ≈ 2²⁵⁵` vs `2^depth ≤ 2¹¹` margin and why descending indices cannot wrap into the accepted range | `src/circuit/democracy.rs` |
| 3 | Oligarchy member-update timeline unclear | Design doc §6.4.3 now carries an explicit v1 rollout table: `update_commitment` returns `UnknownGroupType` for `group_type == 3`; only admin-set rotation ships; member rekey deferred to a follow-up `OligarchyUpdateCircuit` | `docs/group-governance-types-design.md` |
| 4 | VK domain-separation not documented in ceremony runbook | Added `docs/democracy-circuit-ceremony.md §3.1` with three-layer defence (R1CS shape, contract storage key, public-input schedule) and operator verification commands (sha256sum divergence, `ceremony-tool inspect-vk`) | `docs/democracy-circuit-ceremony.md` |
| 5 | `UsedProof` storage-cost model not acknowledged | Design doc §8 T5 now carries storage-cost subsection with `30·G·r` formula, sample sizing, and a note on rent-scaling | `docs/group-governance-types-design.md` |
| 6 | Cross-layer coupling risk if `member_count = 0` were ever stored | Closed by fix #1 — circuit now rejects any all-zero active bitmap regardless of stored `member_count` | Same as #1 |

The reviewer's verdict was *"Conditionally Sound — 3 design concerns,
2 circuit-level issues, 1 architectural gap."* All six actionable items
are closed.

### 3.2 Independent cross-checks

Beyond the reviewer's list, I verified:

- **Dispatcher isolation.** `update_commitment` (Anarchy) at `lib.rs:945–949`
  explicitly rejects `group_type ∈ {1, 2, 3}` with `OneOnOneImmutable` /
  `UnknownGroupType`. `update_commitment_democracy` at `lib.rs:1050–1052`
  rejects `group_type != 2`. `update_admin_commitment` at `lib.rs:1180–1182`
  rejects `group_type != 3`. No proof verified against a wrong-type group
  is routable through any dispatcher, and none of these functions share a
  VK slot.
- **V2 source-of-truth binding for `member_count`.** `update_commitment_democracy`
  at `lib.rs:1063–1067` binds `public_inputs.member_count_old` to the
  on-chain `current.member_count` via strict equality *before* VK lookup.
  This is the §6.4.2 pre-check that guards against a malicious caller
  lying about the quorum denominator.
- **Groth16 proof replay.** `record_proof` is called *after* successful
  verification (`lib.rs:1112`, `1233`), so a bogus proof that fails
  `#7 InvalidProof` is not recorded. This is exploited by the smoke script
  to run the bogus-proof check idempotently (§6).

### 3.3 Open soundness questions (non-blocking)

- **Admin-∈-member cross-check in `AdminUpdateCircuit`.** By design
  (§6.4.4, §8 T10) the circuit does not prove the rotating admin is also
  a group member — that invariant is UI-enforced. Tightening it would
  ~2× the constraint count. Documented.
- **`admin_root` at `create_oligarchy_group` is creator-attested.** The
  contract validates canonical Fr encoding but not the tree's structural
  validity. Damage from a malformed root is confined to self-sabotage
  (the group becomes immutable for admin ops). §8 T10.
- **Client-only Democracy finalise fallback.** Until the Phase 2 ceremony
  VK lands on mainnet, `GroupListViewModel.finalizeBallot` on both
  clients broadcasts a standard `update_commitment` against the Anarchy
  VK. This is the §3.2 quorum-bypass gap that closes at Phase D.
  Documented in the ceremony playbook §6.

---

## 4. Correctness

### 4.1 The `verify_membership` V2 bug (first review)

A reviewer flagged that `verify_membership` read legacy `DataKey::Group`
only and would return `GroupNotFound` for every V2-native group. **Fixed
and regression-tested.**

- `verify_membership` at `lib.rs:1284` now uses `load_group_v2(&env, &group_id)`
  — the V2-aware loader that tries `DataKey::GroupV2` first and falls
  back to legacy.
- Every other read/write path is consistent: `deactivate_group`,
  `update_commitment`, `update_commitment_democracy`,
  `update_admin_commitment`, `get_state`, `get_state_v2`, `get_history`
  all use the same V2-aware pattern. No dispatcher reads stale legacy
  state.
- Regression test `test_verify_membership_resolves_v2_native_group`
  (and a matching snapshot) exercises the V2-only path.

### 4.2 iOS v2 ballot-cast detection (first review)

The first review noted *"iOS `VoteProposalCard.myChoice` at `ChatView.swift:700`
matches against `VOTE_CAST::v1::` prefix only — it won't detect the
user's own v2 cast."* **Stale — already fixed at HEAD.**

`ChatView.swift:686–687` now contains:
```swift
let prefixV1 = "VOTE_CAST::v1::\(parsed.ballotID)::"
let prefixV2 = "VOTE_CAST::v2::\(parsed.ballotID)::"
```
…and both platforms accept both prefixes in tally + "my cast" detection.
No action needed.

### 4.3 Backward compatibility

Verified by inspection and test snapshots:

- **Contract V2 ↔ V1 lazy migration.** `get_state` projects V2 records
  back to V1 shape for legacy clients; legacy `DataKey::Group` entries
  are synthesized into V2 shape on read. Test
  `test_v2_takes_precedence_when_both_entries_present` confirms
  precedence rules.
- **`InviteCode` wire format.** Both platforms use `decodeIfPresent` /
  `optInt` / `optJSONArray` for new `groupTypeRawValue` and `adminPubkeys`
  fields, defaulting to Anarchy / empty. Older invite codes decode
  correctly; older clients tolerate unknown keys.
- **Android DB migration.** Version bumps 14 → 15 → 16, both migrations
  registered (`StellarChatDatabase.kt:22`), additive ALTERs with defaults.
- **iOS SwiftData.** New fields are optional; pre-existing rows load with
  defaults.

### 4.4 Testnet smoke (end-to-end correctness)

On testnet contract `CBIW2NWBZKKDUK64SOSJGKKOZK3GSWJWSMMZIGTSCV5AMFYGH7SABZWT`
(fresh deploy + Democracy VKs + AdminUpdate VK installed):

| # | Action | Expected | Observed |
|---|--------|----------|----------|
| 1 | `create_group_v2` Democracy tier 0 member_count 2 | ✓ | ✓ |
| 2 | `update_commitment_democracy` bogus proof | `#7 InvalidProof` | `#7` |
| 3 | `create_oligarchy_group` tier 0 member_count 2 | ✓ | ✓ |
| 4 | `get_admin_epoch` fresh Oligarchy | `0` | `0` |
| 5 | `update_admin_commitment` bogus proof | `#7 InvalidProof` | `#7` |
| 6 | `update_admin_commitment` on Democracy group | `#18 UnknownGroupType` | `#18` |

The bogus-proof checks distinguish "VK slot populated + pairing rejected"
(`#7`) from "VK slot empty" (which would panic at lookup, not return a
neat enum error). Both slots prove populated.

Reproduce:
```
./scripts/smoke-governance-testnet.sh
```

---

## 5. Test coverage

### 5.1 What's covered

**Contract (`contracts/sep-xxxx/src/lib.rs`) — 106 tests pass.**

New coverage spans every dispatcher arm:

- `test_create_group_v2_{accepts_democracy,rejects_oligarchy,rejects_unknown}_type`
- `test_create_democracy_rejects_member_count_{zero,one,exceeds_small_capacity,exceeds_large_capacity}`
- `test_create_oligarchy_{rejects_duplicate_group_id,rejects_invalid_tier,rejects_non_canonical_admin_root}`
- `test_update_commitment_democracy_{rejects_non_democracy_group,rejects_wrong_c_old,rejects_member_count_mismatch,rejects_delta_too_large,rejects_drop_below_quorum_floor,rejects_when_vk_missing}`
- `test_update_admin_commitment_{rejects_non_oligarchy_group,rejects_missing_admin_root,rejects_wrong_admin_c_old,rejects_wrong_admin_epoch_old,rejects_non_canonical_c_new,rejects_when_vk_missing}`
- `test_update_vk_{admin_update_ignores_tier,admin_update_rejects_wrong_ic_length,democracy_rejects_wrong_ic_length,rejects_unknown_group_type_for_update_by_type}`
- `test_verify_membership_resolves_v2_native_group` (the regression test for §4.1)
- V2/V1 coexistence: `test_legacy_group_read_via_get_state_v2_defaults_to_anarchy`,
  `test_v2_takes_precedence_when_both_entries_present`,
  `test_get_state_projects_v2_to_v1_shape`, etc.

Plus full test-snapshot coverage (75+ new `.json` traces under
`contracts/sep-xxxx/test_snapshots/test/`) for reproducible ledger effects.

**Circuit (`src/circuit/democracy.rs`) — 6 tests pass.**

- `democracy_circuit_accepts_valid_replace` (positive path)
- `democracy_circuit_rejects_under_quorum`
- `democracy_circuit_rejects_wrong_c_new`
- `democracy_circuit_rejects_mismatched_member_count_delta`
- `democracy_circuit_rejects_out_of_range_count_delta`
- `democracy_circuit_empty_for_keygen` (ceremony-safety check)

### 5.2 Gaps — recommend a follow-up PR

The second-review release-engineering pass flagged that `DemocracyUpdateCircuit`
tests cover only the `Replace` delta variant. A direct audit confirms:

| Scenario | Covered by … | Direct test? |
|----------|-------------|--------------|
| `Replace` delta accepts | `democracy_circuit_accepts_valid_replace` | ✓ |
| `Insert` delta accepts | — (constraint implemented but not exercised) | ✗ |
| `Remove` delta accepts | — (constraint implemented but not exercised) | ✗ |
| `K ≥ 1` rejection (all-zero active bitmap) | Indirectly by "under-quorum" | ✗ direct |
| Descending signer indices rejected | — (sound by depth-bit argument but not tested) | ✗ |

None of these are blockers:

- `Insert` / `Remove` are constrained at `democracy.rs:569–584` and the
  constraint #8 consistency gate ties delta-kind to `member_count_new`
  arithmetic. The unit tests that reject a `count_delta = 2` scenario
  exercise the delta-kind logic indirectly.
- `K ≥ 1` is now enforced in-circuit (§3.1 item 1). The constraint runs
  for every proof generation — a regression would fail every positive-path
  test.
- Descending indices are rejected by the `depth`-bit decomposition
  mechanism whose soundness argument is now inline-commented. Adding a
  negative test is strictly belt-and-suspenders.

**Recommended follow-up:** a small PR that adds
`build_insert_scenario` + `build_remove_scenario` + two reject-path tests
(K=0, descending). ~100 LOC; 30 min of work; self-contained.

### 5.3 Positive-path on-chain proof tests

Both reviews note that `test_update_admin_commitment` / `test_update_commitment_democracy`
have no positive-path (proof-accepted) test today — those paths require
real Groth16 proofs and are deferred to **integration tests post-ceremony**.
This is the right call: a positive-path test with a mock VK would prove
nothing beyond "the CLI parser works," which the rejection tests already
exercise. A real positive-path test lands with the Phase 2 ceremony.

---

## 6. Cleanness

### 6.1 Build hygiene

- `cargo build --release` — clean.
- `stellar contract build` — clean, all expected public methods present:
  `initialize`, `create_group`, `create_group_v2`, `create_oligarchy_group`,
  `update_commitment`, `update_commitment_democracy`,
  `update_admin_commitment`, `update_vk`, `set_restricted_mode`,
  `bump_group_ttl`, `deactivate_group`, `get_state`, `get_state_v2`,
  `get_history`, `get_admin_root`, `get_admin_epoch`, `verify_membership`.
- No compiler warnings introduced by this PR (checked incrementally).
- 55k line additions are dominated by auto-generated `test_snapshots/*.json`
  trace files (correct for snapshot-testing in Soroban). Hand-written
  code delta is much smaller.

### 6.2 Docs

- `docs/group-governance-types-design.md` — source of truth for the
  four-type model. Aligned with implementation after the review-response
  commits (§6.4.3 Oligarchy v1 rollout table, §8 T5 storage-cost
  subsection).
- `docs/democracy-circuit-ceremony.md` — Phase 2 ceremony playbook.
  Added §3.1 VK domain-separation section in review response.
- `docs/protocol-soundness-analysis.md` — new artifact from review
  response; consolidates the per-review findings and their resolutions.
- Design doc and ceremony doc cross-reference; no drift.

### 6.3 Rollout scripts

All operator-facing scripts are in place and testnet-verified:

| Script | Purpose | Status |
|--------|---------|--------|
| `scripts/deploy_sep_xxxx_testnet.sh` | Contract deploy with `PERSIST_IDENTITY=1` mode for reusable admin | ✓ used for CBIW… deploy |
| `scripts/install-democracy-vks-testnet.sh` | Install `UpdateByType(2)` tier 0 + tier 1 | ✓ run against CBIW… |
| `scripts/install-adminupdate-vk-testnet.sh` | Install `AdminUpdate` (reuses `keyset-v2/vk-update-small.json`) | ✓ run against CBIW… |
| `scripts/smoke-governance-testnet.sh` | End-to-end smoke of all three new dispatcher arms (§4.4) | ✓ 6/6 pass |
| `scripts/generate-democracy-vk-dev.sh` | Regenerate dev VKs when circuit changes | ✓ used to produce keyset-democracy-dev/ |

Each script has a `DRY_RUN=1` mode for offline preview, tolerant admin-mismatch
messaging, and reads the target contract + deployer from `relayer/.env`.

### 6.4 Client changes

- **Backwards-compat wire format.** `InviteCode` decoders on both clients
  tolerate missing `groupTypeRawValue` / `adminPubkeys`.
- **Governance events.** `ADMIN_PROMOTE::v1` / `ADMIN_DEMOTE::v1` /
  `VOTE_CAST::{v1,v2}` / `DEMOCRACY_FINALIZED::v1` uniformly defined on
  both platforms.
- **UI.** GroupInfoView (iOS) + GroupInfoScreen (Android) surface
  admin lists and governance-type badges. Create Group shows all four
  type options (Anarchy / 1v1 / Democracy / Oligarchy) — per issue #78
  the Create Group UI is gated on VKs being installed first, which is
  satisfied for CBIW… testnet.

### 6.5 Non-blockers worth a follow-up touch

- `tier_capacity` doc comment says "behaviour for out-of-range tiers is
  undefined" but the code returns `0`. Minor doc-comment fix.
- `BALLOT_LIFETIME_SECONDS` / `ballotLifetime` hardcoded to 7 days on
  both clients. A `// TODO(config): per-group override` comment would
  clarify intent.
- 1v1 wrong-`member_count` returns `PublicInputsMismatch` (10) rather
  than a dedicated error. Asymmetric with `Invalid1v1Tier` (23) but
  harmless.

---

## 7. Known gaps (tracked, non-blockers)

### 7.1 Oligarchy member-update frozen in v1

Per design doc §6.4.3: `update_commitment` returns `UnknownGroupType` for
`group_type == 3`. Oligarchy groups cannot add/remove *members* until a
future `OligarchyUpdateCircuit` ships with its own Phase 2 ceremony.
Admin-set rotation (`update_admin_commitment`) works today.

### 7.2 Democracy VKs are dev-only on testnet

`keyset-democracy-dev/README.txt` states the VKs are generated by a
single-party `OsRng` setup — acceptable for testnet smoke, unacceptable
for mainnet. Before Phase D (mainnet rollout), run the Phase 2 ceremony
per `docs/democracy-circuit-ceremony.md` and replace the testnet VKs
via a fresh `update_vk` call.

### 7.3 Client-only Democracy finalise

Until the Democracy VK lands on mainnet, `finalizeBallot` broadcasts an
Anarchy-shaped `update_commitment`. The quorum gate is UI-enforced. Any
member with a local build can bypass the UI and finalise unilaterally.
This is the entire reason for the Phase D rollout — closing this gap is
the rollout's purpose, not a regression.

### 7.4 `UsedProof` storage growth

Per design doc §8 T5: each state-changing op writes one 32-byte entry
with a 30-day TTL. At the documented upper bound (30k groups × 1 op/day)
this is ~90 MB of persistent state. Soroban entries expire if not bumped;
nobody bumps `UsedProof` entries. Acceptable.

---

## 8. Merge recommendation

**Merge.** The soundness, correctness, and cleanness of this PR all hold
up under an adversarial review pass. Three belt-and-suspenders test
scenarios are worth a ~100-LOC follow-up PR (§5.2), but none block this
merge or the Phase 2 ceremony that will gate the mainnet rollout.

**Post-merge checklist** (tracked in issue #78):

- [x] Phase 1 — contract deployed to testnet (`CBIW2N…BZWT`)
- [x] Phase 2 — Democracy VKs (tier 0 + tier 1) installed
- [x] Phase 3 — AdminUpdate VK installed
- [x] Testnet smoke passes
- [ ] Phase 4 — cut iOS TestFlight + Android Play Internal with the new
      contract ID (both read `RELAYER_CONTRACT_ID` from `relayer/.env` at
      build time — no manual source edits needed)
- [ ] Phase 5 — QA hand-off on #79–#83
- [ ] Follow-up PR — Democracy circuit Insert/Remove + K=0 + descending-
      index tests (§5.2)
- [ ] Pre-mainnet — Phase 2 ceremony to replace dev Democracy VKs (§7.2)
