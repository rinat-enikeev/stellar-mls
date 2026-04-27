# Oligarchy Group Updates — Testnet-Gated Implementation Design

**Date:** 2026-04-26
**Status:** Draft (Proposal — pre-implementation)
**Author:** Onym contributors
**Version:** 0.1.4 — closes the brute-force-from-public-priors privacy gap raised on PR #147 review (https://github.com/rinat-enikeev/stellar-mls/pull/147#issuecomment-4322560271). v0.1.3's combined occupancy commitment was salt-less: a chain observer with knowledge of §4.8's publicly-fixed initial bitmaps (slot 0 occupied, rest tombstoned, both trees) could enumerate ~288 single-leaf-delta candidates per epoch (256 member-side + 32 admin-side), compute each candidate's combined commitment against the unchanged-other-intermediate, and find one match — recovering which tree changed AND which slot toggled, by induction across all epochs. The §4.7.7 "out-of-band caveat" did not cover this attack because the prior state is public chain history, not out-of-band. v0.1.4 closes the leak by adding a fresh per-epoch private-witness salt `salt_occ` to the combined commitment formula in §4.7.2; the brute-force search space becomes 252-bit and is computationally infeasible. The salt is distributed off-chain via `SEPSaltResponse.occupancySalt` (§4.4), parallel to how Democracy already distributes `c_new`'s salt. Side effects: §3.5 row "Which tree changed in any given update" stays at ✓ unconditionally (no on-chain-observer caveat); §4.7.3 R1CS estimate bumps to tier-1 ~11272 (round to ~11300), tier-0 ~8880, tier-2 ~27500 — the v0.1.4 reconciliation passes (one in this version stanza below, plus a follow-up addressing @gramyzer's PR #147 review at https://github.com/rinat-enikeev/stellar-mls/pull/147#pullrequestreview-4177288674) (a) pinned the occupancy-commitment chain to strictly 2-arity Poseidon (5 hashes per side: member intermediate + admin intermediate + combined-intermediate + salted-intermediate + DOMAIN_COMBINED_OCCUPANCY-prefix), adding ~3 hashes per side beyond Democracy's single occupancy hash, and (b) split the dispatcher row into a constant witness/selector cost (~50 R1CS) plus a non-target-no-change equality cost that scales with `max(member_bits, admin_bits)` per side. Proving time ~450ms, still interactive-budget; §7.1's `prove_oligarchy_v2_member_admin_indistinguishable_on_wire` is replaced by `prove_oligarchy_v2_salt_freshness_changes_commitment` (a property test that flips the salt and asserts the commitment changes, pinning the salt's load-bearing role directly rather than via a tautological brute-force simulation), and a new `prove_oligarchy_v2_stale_salt_reuse_rejected` covers the prover's salt-freshness honor-system path; §10.1 Q1 audit covers the new salt input position (no new domain tag — the salt is an extra Poseidon input, not a new tag value, so the six-tag set 1..=6 is unchanged).
**v0.1.3 — addresses @gramyzer REQUEST_CHANGES on v0.1.2:** the §3.5 "which tree changed" privacy claim was defeated by the v0.1.2 wire layout (per-tree occupancy commitments + separate `admin_root_old/new` exposed every update — a chain observer trivially distinguished member updates from admin updates by comparing `admin_root_old == admin_root_new` or `member_occupancy_old == member_occupancy_new`). Closed by combining reviewer's options 2 and 3: (a) drop the separate `admin_root_old/new` wire fields and verify the lockstep update entirely in-circuit (admin_root is bound into `c_old`/`c_new` via the §4.7.2 Poseidon bundling, never re-exposed); (b) fold the two per-tree occupancy commitments into a single combined commitment via new `DOMAIN_COMBINED_OCCUPANCY=6` domain tag (§4.7.2). Net effect: Oligarchy wire payload shrinks from 9 scalars to 5, byte-identical to Democracy's wire shape. This simultaneously closes the §3.4 residual ("Oligarchy is the most-distinguishable variant on payload size") for Democracy-vs-Oligarchy comparison. Side effects: §4.6 drops the separate `admin_root` storage field and the `get_admin_root` getter (the leak-on-read path); group members reconstruct `admin_root` off-chain from `BootstrapPayload.admins` / `SEPSaltResponse.admins` / `SEPGroupStateUpdate`, same shape Democracy uses for `member_root` reconstruction. §7.1 drops the now-vacuous `prove_oligarchy_v2_admin_root_old_mismatch_rejected` / `prove_oligarchy_v2_admin_root_bundling_rejected` tests and adds `prove_oligarchy_v2_member_admin_indistinguishable_on_wire` (the privacy regression the reviewer requested in §7.5-style — later replaced in v0.1.4 by a stronger value-level distinguisher test that exercises the brute-force attack). §4.1.1 "Three additional domain tags" wording is now correct (`DOMAIN_ADMIN=4`, `DOMAIN_ADMIN_OCCUPANCY=5`, `DOMAIN_COMBINED_OCCUPANCY=6` are the three new tags). §4.7.3 R1CS estimate bumps by ~600 (one extra Poseidon per side for the combine) less the ~30 dropped admin_root range checks → tier-1 ~9630. §6 D LOC unchanged (~3265, same as v0.1.2) — the wire-payload simplification touches handler/relayer code in roughly equal measure to what the dropped admin_root pass-through removed
**v0.1.2 — self-review on v0.1.1:** stale "7-scalar" references in §6 D1, §7.3, §7.5 steps 4 and 8 corrected to 9-scalar (the v0.1.1 wire-payload bump was missed in those four spots — and §7.5 step 9's "identical scalar count" privacy assertion depends on the count being right); §7.1 gains two tests for the v0.1.1 `admin_root_old/new` wire fields (later removed in v0.1.3 per the privacy regression above); §7.3 additionally asserts `admin_root_old/new` are forwarded to stellar CLI without re-shaping (later removed in v0.1.3); §4.6 D1 storage list clarifies `admin_root` is preserved in schema shape (carried over from abandoned contract design), not state (later dropped entirely in v0.1.3); §4.4 deduplicated — the redundant "Updated:" repeat of the same `case oligarchy(…)` definition is removed; §4.7.7 clarifies "192-byte proof" refers to Groth16 proof bytes, not public-input wire scalars
**v0.1.1 — addresses @gramyzer REQUEST_CHANGES on v0.1:** §4.4 admin_root claim corrected (Poseidon is one-way; admin_root is stored separately, not derived from c_old; wire payload extended to 9 scalars to carry admin_root_old/new explicitly — later collapsed back to 5 in v0.1.3 per the privacy regression); §4.2 single-signer cutoff generalized for non-default thresholds (parallel to Democracy v0.5.1 fix); §7.1 constraint reference renumbered to #12; §6 Phase A LOC arithmetic corrected to ~1190; §4.8 zero-member create disallowed (would brick on first update); §10.2 Q6 self-removal workaround corrected to four steps (was three, but skipped a step that violated the member floor); §4.7.7 "Defeats:" renamed to "Threat-model limitation"
**Supersedes:** (none — first iteration)
**Related:**
- [`democracy-update-testnet-design.md`](democracy-update-testnet-design.md) — sibling design; many mechanisms (slot-index convention, occupancy commitment, polymorphic dispatch, testnet gating, ceremony) are inherited verbatim and cross-referenced rather than duplicated below.
- [`group-governance-types-design.md`](group-governance-types-design.md) — parent design that introduces `groupType ∈ {Anarchy=0, OneOnOne=1, Democracy=2, Oligarchy=3}`.
- [`democracy-circuit-ceremony.md`](democracy-circuit-ceremony.md) — extends to cover Oligarchy circuits as part of the same Phase 2 ceremony.
- `contracts/sep-xxxx/src/lib.rs:197-200` (`MissingAdminRoot=19`, `AdminRootMismatch=20`), `:1154-1248` (`create_oligarchy_group`), `:1293-1300` (`get_admin_root`). Updates against Oligarchy groups currently have no contract entrypoint at all — the missing piece this design specifies. Per §4.4 / §4.6, the v2 redeploy abandons the existing `admin_root` storage field and `get_admin_root` getter (the chain-observer leak path closed in v0.1.3); errors `MissingAdminRoot` / `AdminRootMismatch` go away with them.

---

## 1. Background

Oligarchy groups (`groupType = 3`) extend the membership model with a privileged **admin subset** — a second Merkle-committed set whose holders are the only parties authorized to propose state changes. The contract today supports oligarchy *creation* via `create_oligarchy_group(group_id, admin_root, …)` (`lib.rs:1154`), which seeds the admin set as a Poseidon-committed tree alongside the standard membership tree. **There is no Oligarchy update entrypoint**. Any attempt to change the membership of an Oligarchy group fails the same way Democracy does today — the `update_commitment` arm hard-rejects non-Anarchy types — but with no per-type entrypoint to fall back to.

`swift-mls/Sources/SwiftMLS/ContractClient.swift:88-91` exposes `createOligarchyGroup` but no `updateCommitmentOligarchy`. The gap is symmetric to Democracy's pre-v0.5 state, with one structural addition: Oligarchy carries **two trees** (member set + admin set) instead of one, and any update must prove an admin authorized it without revealing which admin.

This design fills the gap. It commits to the same direct-to-v2 trajectory as Democracy: hidden counts via occupancy commitment, polymorphic `update_commitment` dispatch, dev-key ceremony for all three tiers, fresh contract redeploy, no backward compatibility required. The "no metadata leakage" privacy floor extends from members to admins — admin count, admin-set trajectory, and which admin co-signed any update are all hidden.

---

## 2. Problem

Oligarchy groups can be created on chain but never modified. There's no `update_commitment` arm that accepts `group_type == 3`, no proving function, no FFI export, no client dispatch. The contract knows about `admin_root` storage but has no read path that consumes it (apart from `get_admin_root`, which is a passive getter that v0.1.3 drops as a privacy-leak path).

Functionally, a 1-admin / 1-member Oligarchy group works as a static "single-person room" since no updates ever happen. As soon as you want to add a member, promote/demote an admin, or remove anyone, the group bricks the same way Democracy does pre-v0.5. The only difference: Oligarchy groups are deliberately small (admin sets are typically 1-3 people), so the "permanently stuck at creation" failure mode is more visible per-call than in Democracy.

This design specifies the work needed to make Oligarchy update flows functional and preserve Anarchy's privacy floor (no count, trajectory, or churn signals on the member set OR the admin set), accepting dev-key cryptography for the testnet phase.

---

## 3. Constraints

The five hard constraints. The first three are inherited from Democracy; the last two extend the privacy-floor commitment to the second tree.

### 3.1 The circuit is dev-only (inherited from [Democracy §3.1](democracy-update-testnet-design.md#31-the-circuit-is-dev-only))

Oligarchy's circuit will be `src/circuit/oligarchy_v2.rs`, written from scratch to match the v2 occupancy-commitment shape. Same dev-only soundness posture; same MUST-NOT-run-Phase-2-ceremony-against-this until R1CS review concludes.

### 3.2 The Phase 2 ceremony has not run

Same as Democracy. `keyset-oligarchy-dev/` does not yet exist; Phase A4 of §6 generates it. The ceremony for Oligarchy circuits joins Democracy's planned Phase E (one combined ceremony, three tiers each, two governance types — six circuits + Anarchy = seven total).

### 3.3 Single-leaf delta on each tree (inherited)

Same constraint as [Democracy §3.3](democracy-update-testnet-design.md#33-single-leaf-delta-requires-a-stable-slot-index-convention), applied independently to the member tree and the admin tree. §4.1's slot-index convention extends to both. An Oligarchy update changes **at most one tree** (member tree XOR admin tree, never both); cross-tree atomic updates are §11 follow-up.

### 3.4 No member-related metadata leakage (inherited from [Democracy §3.4](democracy-update-testnet-design.md#34-no-member-metadata-leakage-is-a-p0-requirement))

Same publicness floor: hide member count, trajectory, churn, identities. The mechanism (occupancy commitment via Poseidon over the bitmap) carries over verbatim.

### 3.5 No admin-related metadata leakage

The new constraint specific to Oligarchy. Every signal a chain observer could derive about the admin set must be hidden to the same standard:

| Metadata | Anarchy hides | Democracy hides | Oligarchy MUST hide |
|---|---|---|---|
| Exact admin count at any epoch | (n/a — no admins) | (n/a — no admins) | ✓ |
| Admin-set delta per epoch (promotions, demotions distinguishable) | (n/a) | (n/a) | ✓ |
| Lifetime cumulative admin appointments | (n/a) | (n/a) | ✓ |
| Identity of admins | (n/a) | (n/a) | ✓ (Poseidon-hashed leaves; same property as members) |
| Which admin co-signed a given update | (n/a) | (within member set, hidden — proof discloses K, not which K) | ✓ (within admin set, same property) |
| Which tree changed in any given update (member vs admin) | (n/a) | (n/a — single tree) | ✓ (private witness, see §4.7.7) |

The last row is Oligarchy-specific: every update has the same on-wire shape regardless of whether it touched the member tree or the admin tree. A chain observer cannot distinguish "iOS added a new member" from "iOS promoted Bob to admin" by inspecting the proof or the public inputs. This is the most novel privacy claim in this design and the §4.7.7 mechanism is what backs it.

#### What this design does NOT hide (acknowledged residuals)

In addition to all of [Democracy §3.4's residuals](democracy-update-testnet-design.md#what-this-design-does-not-hide-acknowledged-residual-leaks), Oligarchy adds:

| Residual | Why it leaks | Mitigation in this design |
|---|---|---|
| The group is Oligarchy-typed | `group_type=3` from `create_oligarchy_group_v2` argument and contract storage | Same publicness class as Democracy's `group_type=2`. Polymorphic dispatch (§4.7.4) keeps it out of the per-call selector. **Residual (reduced in v0.1.3)**: the Oligarchy public-inputs payload size now matches Democracy's (5 scalars vs Anarchy's 3) — Democracy and Oligarchy are byte-indistinguishable on the wire, only Anarchy remains distinguishable on payload size. Closing Anarchy's residual would still need uniform-shape padding via dummy circuit-side commitments (deferred). |
| Admin tree depth (capped at Small=32 in v0.1) | Tier choice fixed at create time | Documented; see §4.6. Operators select the member tier (Small/Medium/Large) but admin tree is always Small. |
| Existence of an admin set | Implicit from `group_type=3` | Inherent to the type. No mitigation possible without restructuring at the type level. |

**No new per-update residual**. The "which tree changed" signal is the only fresh axis of leakage Oligarchy adds, and §4.7.7 closes it.

---

## 4. Design

### 4.1 Slot-index convention (per-tree)

Inherited from [Democracy §4.1](democracy-update-testnet-design.md#41-slot-index-convention-resolves-§33). Member tree and admin tree each have their own independent slot-index assignment:

- Member slots indexed `[0, 2^D_member)` where `D_member` is the member tier's depth (5/8/11 for Small/Medium/Large).
- Admin slots indexed `[0, 2^D_admin)` with `D_admin = 5` fixed (Small tier — admin sets are capped at 32 lifetime appointments per §4.6).
- A given identity (one secret key) can appear in both trees with **independent slot assignments**: their member slot index and admin slot index need not match. A creator who is both initial member and initial admin is at member-slot 0 and admin-slot 0; subsequent joins / promotions can desynchronize the indices.
- Tombstones permanent on both trees ([§4.1.2](democracy-update-testnet-design.md#412-why-permanent-tombstones-not-just-epoch-bound-replay-protection) inherited).

#### 4.1.1 Domain separation (extends Democracy §4.1.1)

Three additional domain tags on top of the Democracy set:

| Tag | Value | Purpose |
|---|---|---|
| `DOMAIN_MEMBER` | `Fr::from(1)` | Inherited from Democracy. Member-tree leaf prefix: `Poseidon(DOMAIN_MEMBER, Poseidon(sk))`. |
| `DOMAIN_TOMBSTONE` | `Fr::from(2)` | Inherited. Shared between both trees — the tombstone constant is structurally the same in member and admin trees. The disjoint *non-tombstone* domain tags below ensure no cross-tree leaf collision is possible. |
| `DOMAIN_OCCUPANCY` | `Fr::from(3)` | Inherited. Member-tree occupancy commitment: `Poseidon(DOMAIN_OCCUPANCY, fold(member_bitmap))`. |
| `DOMAIN_ADMIN` | `Fr::from(4)` | **New.** Admin-tree leaf prefix: `Poseidon(DOMAIN_ADMIN, Poseidon(sk))`. Disjoint from `DOMAIN_MEMBER` so the same secret key produces structurally distinct leaves in the two trees. |
| `DOMAIN_ADMIN_OCCUPANCY` | `Fr::from(5)` | **New.** Admin-tree occupancy commitment intermediate: `Poseidon(DOMAIN_ADMIN_OCCUPANCY, fold(admin_bitmap))`. **Circuit-internal — not exposed on wire or in storage** (per the v0.1.3 §3.5 fix; only the §4.7.2 combined commitment is observable). |
| `DOMAIN_COMBINED_OCCUPANCY` | `Fr::from(6)` | **New (v0.1.3).** Combined occupancy commitment over both trees: `Poseidon(DOMAIN_COMBINED_OCCUPANCY, member_occ_intermediate, admin_occ_intermediate)`. This is the public-input value carried on the wire and stored on chain. Combining the two per-tree intermediates under a fresh domain tag prevents a chain observer from distinguishing "member tree changed" from "admin tree changed" — both produce a fresh combined-commitment value with no per-tree decomposition leaked. |

§10 Q1 (the cross-circuit domain-tag audit) extends to cover values 4, 5, and 6. All six values must be unique not just across this circuit but across `MembershipCircuit`, `UpdateCircuit`, `DemocracyUpdateCircuit`, and any future circuit family.

#### 4.1.2–4.1.4 (inherited)

- [Democracy §4.1.2](democracy-update-testnet-design.md#412-why-permanent-tombstones-not-just-epoch-bound-replay-protection): tombstone permanence rationale, applied independently to each tree.
- [Democracy §4.1.3](democracy-update-testnet-design.md#413-slot-index-back-channel-to-the-joiner): slot back-channel via `SEPGroupStateUpdate`. Oligarchy adds: when an admin is promoted, the new admin must learn their admin-slot index; same broadcast, same `lastBroadcastEpoch` recovery, same `SEPSaltResponse` fallback.
- [Democracy §4.1.4](democracy-update-testnet-design.md#414-concurrent-acceptor-race-two-existing-members-process-the-same-sepmemberjoined): concurrent-acceptor race applies to admin updates too. The "acceptor" is whichever admin lands the chain update first; losers roll back via the same baseline-revert flow.

### 4.2 Single-admin-signer subset (multi-admin quorum is Phase F)

Parallel to [Democracy §4.2](democracy-update-testnet-design.md#42-single-signer-subset-multi-signer-quorum-is-future-work), with admin set in place of full member set as the source of authorizing signers.

Constraint #4 in the Oligarchy circuit will be `100·K ≥ admin_threshold_numerator · admin_count_old`, where `K` is the number of *admin* signers contributing their secret keys to the proof. For a single signer (`K = 1`), this collapses to `admin_count_old ≤ 100 / admin_threshold_numerator` — same parameterized formulation as the [Democracy §4.2](democracy-update-testnet-design.md#42-single-signer-subset-multi-signer-quorum-is-future-work) cutoff but applied to the admin set. The single-signer-supported size depends on the group's chosen `admin_threshold_numerator`:

| `admin_threshold_numerator` | Max `admin_count_old` for K=1 | Covers transitions |
|---|---|---|
| 50 (default — simple admin majority) | 2 | 1→2, 2→3 admin |
| 51 (strict admin majority) | 1 | 1→2 admin only |
| 67 (admin supermajority) | 1 | 1→2 admin only |
| 75 | 1 | 1→2 admin only |
| 100 (unanimous admin consent) | 1 | 1→2 admin only |

The bootstrap case (1→2 admin) works for any `admin_threshold_numerator ≤ 100`. The 2→3 admin case (and analogously, member updates whose proof depends on `admin_count_old = 2` for the quorum check) work only at default 50%. Groups created with stricter admin thresholds therefore hit `QuorumRequired` at the second-admin-add boundary, with a UX message routing the user to the (not-yet-built) multi-admin flow.

The prover errors with `Error::QuorumRequired` when `⌈admin_threshold_numerator · admin_count_old / 100⌉ > 1`. This computation is included in the prover's preflight; no malformed proof is ever generated. Multi-admin-quorum is Phase F (shared with Democracy's same-named phase — the K-of-N collection mechanism is the same shape regardless of which set the K signers are drawn from).

Coverage at default `admin_threshold_numerator = 50`:
- 1 → 2 admin transition: K=1, single signer (the original admin) authorizes the promotion.
- 2 → 3 admin transition: K=1 satisfies `100·1 ≥ 50·2` (tie-passes per [Democracy §4.7.6](democracy-update-testnet-design.md#476-configurable-quorum-threshold-per-group-fixed-at-create) tie semantics, inherited).
- Member updates with admin_count_old ∈ {1, 2}: K=1.
- Admin or member updates with admin_count_old ≥ 3 OR with admin_count_old = 2 and admin_threshold_numerator > 50: blocked, returns `QuorumRequired`.

#### 4.2.1 Circuit invariants vs. contract invariants (parallel to Democracy §4.2.1)

The Oligarchy circuit's invariants table is structurally the same as Democracy's, with member-related rows duplicated for the admin tree:

| Invariant | Circuit-enforced | Contract-enforced | Notes |
|---|---|---|---|
| `c_old = Poseidon(Poseidon(Poseidon(root_member, root_admin), epoch_old), salt_old)` | ✓ | (binding via public input) | Bundles both roots; see §4.7.2 |
| `c_new = Poseidon(Poseidon(Poseidon(root_member_new, root_admin_new), epoch_old + 1), salt_new)` | ✓ | (binding via public input) | Same shape, advanced epoch |
| At least one signer's secret key opens to a leaf in `root_admin_old` | ✓ | — | Signer is admin, not just member |
| `K ≥ 1` (no zero-opening Commits) | ✓ | — | |
| `100·K ≥ admin_threshold_numerator · popcount(admin_bitmap_old)` (admin quorum) | ✓ | — | §4.7.6 |
| Single-leaf delta on the *target* tree (member XOR admin) | ✓ | — | §4.7.7 |
| `popcount(member_bitmap_new) ≥ 1` (member floor) | ✓ | — | Group never becomes empty of members |
| `popcount(admin_bitmap_new) ≥ 1` (admin floor) | ✓ | — | Group never loses all admins |
| Tombstone domain separation (both trees) | ✓ | — | §4.1.1 |
| `c_old == current.commitment`, `epoch_old == current.epoch` | — | ✓ | |
| `occupancy_commitment_member_old == current.occupancy_commitment_member` | — | ✓ | |
| `occupancy_commitment_admin_old == current.occupancy_commitment_admin` | — | ✓ | |
| `c_new`, both occupancy commitments canonical Fr encoding | — | ✓ | |
| Proof is fresh (replay-window check) | — | ✓ | |
| Claimed `group_type` matches `current.group_type` | — | ✓ | §4.7.4 |

All count-derived rules are circuit-only (no v1-style split enforcement); `m_new ≥ 1` floors are explicitly in-circuit.

### 4.3 Testnet-gating mechanism (inherited from [Democracy §4.3](democracy-update-testnet-design.md#43-testnet-gating-mechanism))

Same two-layer scheme:

- **Layer 1**: Cargo feature `oligarchy-v2-dev-vks` (off by default), gates the FFI export, Swift/Kotlin bindings, and Oligarchy variant in `ContractClient.updateCommitment`.
- **Layer 2**: VK fingerprint check at runtime against a hardcoded allowlist baked into client builds. No cache. Mainnet builds drop the dev allowlist entirely.

### 4.4 Wire / data-model additions

A new variant on the polymorphic `UpdateCommitmentPublicInputs` enum, byte-identical in shape to the Democracy variant:

```swift
public enum UpdateCommitmentPublicInputs: Codable, Equatable, Sendable {
    case anarchy(cOld: Data, epochOld: UInt64, cNew: Data)                  // 3 scalars
    case democracy(                                                          // 5 scalars
        cOld: Data, epochOld: UInt64, cNew: Data,
        occupancyCommitmentOld: Data, occupancyCommitmentNew: Data
    )
    case oligarchy(                                                          // 5 scalars (v0.1.3)
        cOld: Data, epochOld: UInt64, cNew: Data,
        occupancyCommitmentOld: Data, occupancyCommitmentNew: Data
    )
}
```

The Democracy and Oligarchy variants carry the same scalar count and the same field shape; only the verifying-key family (and the in-circuit semantics behind `occupancyCommitment`) differs. For Oligarchy, `occupancyCommitment` is the *combined* commitment over both trees defined in §4.7.2 — its per-tree decomposition is a circuit-internal intermediate that never reaches the wire or contract storage.

Note on `admin_root` (v0.1.3 simplification): `admin_root` is bundled into `c_old`/`c_new` via the §4.7.2 Poseidon nesting and supplied to the circuit as a private witness. It is NOT exposed on the wire and NOT stored as a separate contract field. The contract's only state-binding check is `c_old == current.commitment` (same as Democracy). Group members reconstruct `admin_root` off-chain from `BootstrapPayload.admins` (initial), `SEPSaltResponse.admins` (offline-recovery), and `SEPGroupStateUpdate.admins` (per-update broadcast) — exactly the way Democracy reconstructs `member_root` from `BootstrapPayload.members`. A chain observer with no out-of-band access cannot recover `admin_root` from `c_old` (Poseidon is one-way) and cannot read it from contract storage (it isn't there).

(Earlier drafts v0.1.1–v0.1.2 carried `admin_root_old/new` as two extra wire scalars and stored `admin_root` separately, on the rationale that Poseidon is one-way and the contract needs a way to verify the lockstep update. That worked for soundness but defeated §3.5: a chain observer trivially distinguished member updates from admin updates by comparing `admin_root_old == admin_root_new`, and likewise via `member_occupancy_old == member_occupancy_new` against the salt-less per-tree commitments. v0.1.3 closes both leaks: the wire and the storage carry only the bundled `commitment` and the combined occupancy commitment, with admin_root and per-tree occupancies confined to circuit-internal witnesses.)

§3.4 entrypoint-selector residual: Oligarchy is now 5 scalars on the wire (vs. Anarchy's 3 and Democracy's 5). Democracy and Oligarchy are byte-indistinguishable on the wire; only Anarchy still differs on payload size, and the same "uniform-shape padding" structural fix to close Anarchy's residual remains deferred.

`SEPGroupMemberLeaf.slotIndex: UInt32?` carries forward unchanged (per [Democracy §4.4](democracy-update-testnet-design.md#44-wire--data-model-additions)). Same Codable wire-compat rules; same normative-leaf-hash rule (slotIndex NOT part of the leaf hash).

`BootstrapPayload` (`SEPBootstrapPayload`) for Oligarchy gains:

- `members: [SEPGroupMemberLeaf]` — required; standard member list with member-tree slot indices.
- `admins: [SEPGroupMemberLeaf]?` — required (non-nil) for Oligarchy bootstrap; MUST be nil for other governance types. Each entry's `slotIndex` is the **admin-tree** slot, not the member-tree slot. An admin who is also a member appears in both arrays with potentially different slot indices.
- `adminThresholdNumerator: UInt8?` — required (non-nil) for Oligarchy. Validated `1..=100` per §4.7.6 (parallel to Democracy's `thresholdNumerator`). MUST be nil for other types.
- `memberThresholdNumerator: UInt8?` — MUST be nil for Oligarchy. (Democracy uses this; Oligarchy uses only the admin threshold — the member set has no quorum of its own. The admin set authorizes member changes.)

`SEPSaltResponse.members: [SEPGroupMemberLeaf]?` carries forward; an additional `admins: [SEPGroupMemberLeaf]?` is added for Oligarchy salt responses so the offline-recovery flow ([§4.1.3](democracy-update-testnet-design.md#413-slot-index-back-channel-to-the-joiner)) covers admin-tree state too. Padded to tier-uniform size combining both arrays' max possible footprint. **v0.1.4 also adds `occupancySalt: Data`** — the per-epoch private-witness salt used in the §4.7.2 combined occupancy commitment. New joiners and offline-recovery flows MUST learn this salt to recompute the prior-epoch commitment and chain forward; without it they cannot generate a valid update proof. Same opacity rules and tier-uniform padding as the existing `salt: Data` field (which already covers `c_new`'s salt).

`SEPGroupStateUpdate` for Oligarchy carries `members`, `admins`, and **`occupancySalt: Data` (v0.1.4)** — the per-epoch private-witness salt that was used to compute the just-published `occupancy_commitment`. The receiver applies whichever roster changed (the §4.7.7 target-tree witness is private to the prover, but post-broadcast the receiver sees both new arrays and diffs against its persisted state to know which changed — same diff is observable to any group member, but not to a chain observer) AND persists the new `occupancySalt` so the next update's prover can recompute the prior-epoch commitment binding.

**v0.1.4 chain-forward channel.** Per-epoch `salt_occ` distribution is the load-bearing path that lets non-acceptor members generate the *next* update. The two channels carry distinct roles:
- `SEPGroupStateUpdate.occupancySalt` — **per-update broadcast**, sender to all current members. Required path for in-band epoch-to-epoch chaining. Sender publishes the salt that bound the just-committed `occupancy_commitment`; receivers persist it and re-broadcast to any joiner via the offline path.
- `SEPSaltResponse.occupancySalt` — **offline-recovery / new-joiner**, queried out-of-band when a member's local copy is stale or absent. Carries the *current* salt (whatever's bound to `current.occupancy_commitment` on chain). Same shape as Democracy's `c_new`-salt offline-recovery channel.

Both channels are MUST-ship for Phase D operability. A group that broadcasts a new `occupancy_commitment` without the corresponding `occupancySalt` bricks all non-acceptor members on their next update attempt (see §9 risks).

### 4.5 Relayer dispatch (inherited)

The polymorphic `update_commitment` entrypoint already covers Anarchy + Democracy + (now) Oligarchy. The relayer doesn't need a per-type dispatch arm — the variant is in the public-inputs payload, the contract reads `current.group_type` from storage to discriminate.

### 4.6 Contract redeploy + admin set + admin tier cap

This design inherits [Democracy §4.6](democracy-update-testnet-design.md#46-contract-redeploy--vk-installation)'s contract redeploy. The Oligarchy entrypoints land in the same redeployed contract as Democracy's — one fresh address covers all three governance types' v2 paths.

`create_oligarchy_group_v2(group_id, member_root, admin_root, occupancy_commitment, commitment_initial, member_tier, admin_threshold_numerator, initial_proof, …)`. The creator passes both `member_root` and `admin_root` as create-time inputs (so the contract can verify `initial_proof` and bind `commitment_initial` correctly), but neither root is persisted to storage — only `commitment_initial` and `occupancy_commitment` are. Storage gains, per Oligarchy group:

- `occupancy_commitment: BytesN<32>` — the **combined** occupancy commitment from §4.7.2 (`Poseidon(DOMAIN_COMBINED_OCCUPANCY, member_occ_intermediate, admin_occ_intermediate)`). Same field name and storage shape as Democracy's `occupancy_commitment` — Democracy and Oligarchy share storage layout; the in-circuit semantics behind the value is what differs. Per-tree intermediates (`member_occupancy_intermediate`, `admin_occupancy_intermediate`) are circuit-internal witnesses, never stored.
- `admin_threshold_numerator: u8` — new field, range `1..=100`. Validated at `create_oligarchy_group_v2`.
- `admin_count: u8` — implicit from `popcount(admin_bitmap)` but cached for getter convenience. **Caveat**: caching `admin_count` on chain *as a separate readable field* would partially defeat the §3.5 hiding property — a chain observer reading the cached count gets exact admin-set size. So `admin_count` is NOT stored as a separate field; it's only computable in-circuit. There is no public getter for admin-set "alive" — the same `commitment` value is the canonical liveness signal for any governance type.

**Removed in v0.1.3 (was in v0.1.1–v0.1.2):** the separate `admin_root: BytesN<32>` storage field and the `get_admin_root` public getter. Both were leak paths under the §3.5 threat model — a chain observer reading `admin_root` directly defeated the "which tree changed" hiding regardless of the wire-layout fix. The `admin_occupancy_commitment` and `member_occupancy_commitment` separate-storage fields are likewise removed; the combined `occupancy_commitment` replaces both. Errors `MissingAdminRoot=19` / `AdminRootMismatch=20` are removed from the v2 contract. Group members reconstruct `admin_root` and the per-tree occupancies off-chain from `BootstrapPayload` / `SEPSaltResponse` / `SEPGroupStateUpdate` (same shape Democracy uses for `member_root`).

**Admin tier cap**: depth = 5 (32 slots) for the admin tree, hard-coded across all member tiers. Rationale: real-world admin sets are small (typically ≤10), 32 lifetime slots covers the expected churn comfortably for testnet, and capping the admin tier reduces ceremony scope (one admin VK shape across all member tiers) and circuit constraint count. Phase A4's three Oligarchy circuits differ only in the *member* tier; admin tier is fixed.

Operators who want larger admin sets are flagged in §11 follow-up.

The existing testnet contract `CC6N…RWWKE` is abandoned (same as Democracy). All v2 Oligarchy + Democracy + Anarchy state lives at the new address.

### 4.7 Heart of v2 — two-tree occupancy commitment + target-tree privacy

Oligarchy's circuit shape extends Democracy's by tracking both trees simultaneously and hiding which one changed in any given update.

#### 4.7.1 Bitmap shape (two bitmaps per group)

For an Oligarchy group at member tier with depth `D_m` and admin tier depth `D_a = 5`:

- **Member bitmap**: length `2^D_m` bits (32 / 256 / 2048 for Small / Medium / Large). `member_bitmap[i] = 1` iff slot `i` holds a domain-tagged active member; `0` otherwise (tombstone).
- **Admin bitmap**: length `2^D_a = 32` bits. Same semantics on the admin tree.

Both bitmaps are derived deterministically from the respective leaf array; a single normative `canonicalizeBitmap` helper per platform (§7.4 test case asserts cross-platform byte-identical output for both trees).

#### 4.7.2 Commitment formula (bundles both roots)

```
c_old = Poseidon(Poseidon(Poseidon(root_member, root_admin), epoch_old), salt_old)
c_new = Poseidon(Poseidon(Poseidon(root_member_new, root_admin_new), epoch_old + 1), salt_new)
```

Three Poseidon calls instead of Democracy's two. The inner `Poseidon(root_member, root_admin)` packs both roots into a single field element before the existing `Poseidon(combined, epoch)` step. Adds ~600 R1CS constraints over Democracy's `c_old`/`c_new` derivation (one extra Poseidon per side × two sides at ~300 R1CS each).

Occupancy commitments (v0.1.4): a single **salted, combined** commitment is exposed on the wire and in storage; per-tree intermediates and the per-epoch salt are circuit-internal. Pinned to **strictly 2-arity Poseidon** to match every other Poseidon call in this design (Democracy-inherited and the bundled-roots `c_old`/`c_new`):

```
member_occupancy_intermediate = Poseidon(DOMAIN_OCCUPANCY,        fold(member_bitmap))
admin_occupancy_intermediate  = Poseidon(DOMAIN_ADMIN_OCCUPANCY,  fold(admin_bitmap))
combined_intermediate         = Poseidon(member_occupancy_intermediate, admin_occupancy_intermediate)
salted_intermediate           = Poseidon(combined_intermediate,   salt_occ)
occupancy_commitment          = Poseidon(DOMAIN_COMBINED_OCCUPANCY, salted_intermediate)
```

Every step is a 2-input Poseidon. The 4-input snippet from earlier-v0.1.4 drafts is replaced by this strictly-2-arity chain so Phase A's prover library doesn't need a 3-arity-or-higher gadget — the doc's R1CS estimates and the reference implementation can use a single Poseidon-2 gadget throughout.

`salt_occ` is a fresh per-epoch private-witness scalar (32-byte canonical Fr, sampled by the prover at proof generation). It is NOT on the wire and NOT in contract storage. Distributed off-chain via two channels (§4.4): `SEPGroupStateUpdate.occupancySalt` (per-update broadcast, in-band, load-bearing for next-update chain-forward) and `SEPSaltResponse.occupancySalt` (offline-recovery / new-joiner). Same shape Democracy already uses for `c_new`'s salt.

The admin bitmap (32 bits) packs into a single `Fr` scalar (252-bit capacity), so admin's `fold` is trivial — one scalar in, one Poseidon call out. Member side scales with tier as in Democracy (1 / 2 / 9 scalars). The chain in v0.1.4 adds three Poseidon hashes per side beyond Democracy's single-tree occupancy: combined-intermediate + salted-intermediate + DOMAIN_COMBINED_OCCUPANCY-prefix. One full hash per side per step at ~300 R1CS each — ~1800 R1CS over both sides for the three new hashes, plus the per-tree intermediates which already existed in Democracy.

Why combine + salt:
- **Combine** (v0.1.3): per-tree intermediates are deterministic functions of the bitmap, with no per-tree salt. Exposing them as separate public inputs let a chain observer compare `member_occ_old == member_occ_new` (or the admin counterpart) and trivially infer which tree changed in any update — defeating §3.5. Bundling under `DOMAIN_COMBINED_OCCUPANCY` produces a single externally-visible value per epoch that depends on both bitmaps.
- **Salt** (v0.1.4): without a salt, the combine is still vulnerable to brute-force-from-public-priors. Per §4.8, the initial bitmaps are publicly fixed (slot 0 occupied, rest tombstoned, both trees), so `member_occupancy_intermediate_0` and `admin_occupancy_intermediate_0` are publicly computable. After update 1, an observer enumerates ~256+32 single-leaf-delta candidate bitmaps per side, computes each candidate's intermediate, combines with the unchanged-other intermediate, and checks against the on-chain `occupancy_commitment_1`. At most one candidate matches — revealing which tree changed AND which slot toggled. By induction across all epochs, the observer recovers the bitmap evolution. The salt closes this: brute-force now requires guessing both the candidate bitmap AND a 252-bit salt, which is computationally infeasible.

Trades the cost of one extra Poseidon hash (~600 R1CS over both sides — see the chained 2-arity worst case described above) for the §3.5 indistinguishability claim being unconditional rather than caveated-by-public-priors. A 3-arity-Poseidon-already-available best case would be cheaper (~50–100 R1CS over both sides for absorbing one extra input), but the design accounts the chained-2-arity figure to keep the constraint estimate independent of prover-library API choice. (See §4.7.7 for the full mechanism and remaining threat-model limitations.)

#### 4.7.3 Circuit constraints

Reads as Democracy's §4.7.3 with parallel admin-tree work added. Numbered by their position in the v2 Oligarchy constraint list:

1. **Allocates two bitmaps as private witnesses** — `2 · (2^D_m + 2^D_a)` boolean constraints. For tier-1 (medium member, small admin): `2 · (256 + 32) = 576`.
2. **Binds each bitmap to its leaf array** — the §4.7.3-step-2 corrected gadget from Democracy, applied independently to member tree and admin tree:
   ```
   (a)  d · is_active_inv = bitmap[i]
   (b)  (1 - bitmap[i]) · d = 0
   ```
   where `d = leaf[i] - tombstone_constant`. Same two-constraint pair, same load-bearing soundness role. Tier-1: `(256 + 32) · 2 sides · 2 mults each = 1152`.
3. **Computes popcount on each tree** as private witnesses (free in R1CS; bit-decomp range checks for the floor and quorum constraints add ~50).
4. **Quorum on admin set**: `100·K ≥ admin_threshold_numerator · popcount(admin_bitmap_old)`. Same gadget shape as Democracy's §4.7.6 quorum, applied to the admin bitmap. `K` is a witnessed count of active admin signers (≤ `popcount(admin_bitmap_old)`).
5. **Member-floor**: `popcount(member_bitmap_new) ≥ 1`. Bit-decomp range check.
6. **Admin-floor**: `popcount(admin_bitmap_new) ≥ 1`. Bit-decomp range check.
7. **Single-leaf delta on the target tree** (§4.7.7's `target_tree` witness picks which tree). When `target_tree = 0` (member): member-tree delta witness active, admin-tree must be unchanged (`admin_bitmap_old == admin_bitmap_new`, structurally enforced via constraint #8). When `target_tree = 1` (admin): admin-tree delta witness active, member-tree must be unchanged.
8. **Non-target-tree no-change**: the non-target tree's bitmap and root MUST be byte-identical between old and new — enforced as `bitmap_old[i] == bitmap_new[i]` for all `i` in the non-target tree, plus root equality.
9. **Threshold range**: `1 ≤ admin_threshold_numerator ≤ 100`.
10. **Occupancy commitments** (member intermediate + admin intermediate + combined-intermediate + salted-intermediate + DOMAIN_COMBINED_OCCUPANCY-prefix; both sides). ~1200 R1CS for member intermediate (tier-1, 2-scalar fold = 2 hashes × 2 sides) + ~600 for admin intermediate (1 hash × 2 sides) + ~600 for the combined-intermediate + ~600 for the salted-intermediate + ~600 for the DOMAIN_COMBINED_OCCUPANCY-prefix (each row: 1 hash × 2 sides; v0.1.4 — see §4.7.2). All five steps use strictly 2-arity Poseidon.
11. **`c_old`, `c_new` derivations** (with the bundled-roots formula). ~1800 R1CS for two sides × three Poseidon calls each at ~300 R1CS per call (matches the impact-table row below).
12. **Existing constraints** unchanged: signer Merkle openings against `root_admin_old` (private witness, bound to `c_old` via §4.7.2), ascending leaf-index ordering, etc. ~3500.

R1CS-constraint impact estimate (tier-1 member tier / depth 8 / 256 slots; admin tier depth 5 / 32 slots):

| Constraint family | v2 Oligarchy |
|---|---|
| Member bitmap booleans (2 sides × 256) | ~512 |
| Admin bitmap booleans (2 sides × 32) | ~64 |
| Member bitmap-to-leaf binding (2 sides × 256 × 2 mults) | ~1024 |
| Admin bitmap-to-leaf binding (2 sides × 32 × 2 mults) | ~128 |
| Popcount (free) | 0 |
| Quorum (admin-side) `100·K ≥ admin_thr · popcount` | ~22 |
| Member floor + admin floor (range checks) | ~50 |
| Single-leaf-delta dispatcher (`target_tree` witness + mux selectors only) | ~50 |
| Non-target-tree no-change equalities (mux-multiplexed; ≈ 2 × max(member_bits, admin_bits) × 2 sides) | ~512 (max(256,32)=256 → 2·256·2 ÷ 2 mux; tier-1 figure) |
| Threshold range check | ~10 |
| Member occupancy intermediate Poseidon (2 hashes × 2 sides) | ~1200 |
| Admin occupancy intermediate Poseidon (1 hash × 2 sides) | ~600 |
| Combined-intermediate Poseidon (1 hash × 2 sides) | ~600 |
| Salted-intermediate Poseidon (v0.1.4; 1 hash × 2 sides) | ~600 |
| DOMAIN_COMBINED_OCCUPANCY-prefix Poseidon (v0.1.4; 1 hash × 2 sides) | ~600 |
| `c_old`/`c_new` bundled-roots (3 hashes × 2 sides × ~300 each) | ~1800 |
| Existing constraints unchanged | ~3500 |
| **Total tier-1** | **~11272** |

Larger than Democracy's ~6298 — the second tree, the v0.1.4 strictly-2-arity occupancy chain (3 extra Poseidon hashes per side vs Democracy's 1), and the dispatcher's non-target-no-change scaling add about 79% to the constraint count. Proving time for Oligarchy at tier-1 is roughly 3.0× v1-Anarchy's ~150ms → ~450ms. Still well within budget for an interactive epoch transition.

Tier scaling (computed as deltas vs tier-1):
- **Tier-0** (member depth 5 / 32 slots): admin overhead unchanged; member-side rows scale down — booleans 64 vs 512 (−448), binding 128 vs 1024 (−896), intermediate Poseidon ~600 vs ~1200 since the 32-bit bitmap folds to a single scalar (−600), non-target-no-change equalities ~64 vs ~512 since `max(32,32)=32` (−448). Net ~−2392 from tier-1; **~8880 constraints**.
- **Tier-2** (member depth 11 / 2048 slots): member-side booleans (4096) and binding (8192) scale 8× tier-1 (+3584 / +7168); member intermediate Poseidon scales with the 9-scalar fold (~5 hashes × 2 sides ≈ 3000, +1800 vs tier-1); non-target-no-change scales with `max(2048,32)=2048` → ~4096 R1CS, +3584 vs tier-1. Net ~+16136 from tier-1; **~27408 constraints**. Round to **~27500**.

The v0.1.3-era figures (tier-0 ~9000, tier-2 ~22200) and the early-v0.1.4 figures (tier-0 ~8300, tier-2 ~25300) under-counted because the dispatcher row was lumped at flat ~150 with no scaling. The non-target-no-change equality count grows linearly with the larger of the two bitmap sizes (mux-multiplexed); the v0.1.4 reconciliation pass splits the dispatcher row into a witness/selector cost (~50 R1CS, constant) and an equality-mux cost that scales.

#### 4.7.4 Polymorphic dispatch (extends [Democracy §4.7.4](democracy-update-testnet-design.md#474-polymorphic-update_commitment-entrypoint-eliminates-entrypoint-selector-leak))

Oligarchy joins the polymorphic `update_commitment` dispatch table as `(group_type=3, target_tree=hidden)`. The contract reads `current.group_type` from storage to pick the VK family (Anarchy / Democracy / Oligarchy); inside the Oligarchy family, it picks by `current.member_tier` (admin tier is fixed at Small).

The `target_tree` flag is a private witness — **not exposed in the public inputs** — so the contract verifies the same proof shape regardless of whether the update changed the member tree or the admin tree. This is what closes the §3.5 "which tree changed" leak.

Type-confusion guard extends: the public-inputs variant must match `current.group_type`; specifically, an `Oligarchy` variant proof submitted against a `Democracy` group is rejected on receive.

#### 4.7.5 `SEPSaltResponse` size padding (extends [Democracy §4.7.5](democracy-update-testnet-design.md#475-sepsaltresponse-size-padding))

For Oligarchy, the salt response carries both `members: [SEPGroupMemberLeaf]?` and `admins: [SEPGroupMemberLeaf]?`. Pad to the tier-uniform maximum where the maximum accommodates both arrays at full capacity (32 admin slots fixed, member tier-dependent). Same end-to-end opacity rule.

#### 4.7.6 Configurable admin quorum threshold (parallel to [Democracy §4.7.6](democracy-update-testnet-design.md#476-configurable-quorum-threshold-per-group-fixed-at-create))

`admin_threshold_numerator ∈ [1, 100]`, fixed at create, stored on-chain, supplied to the verifier from contract storage (not on the wire). Applies to **all** Oligarchy updates regardless of which tree changes — both member updates and admin updates require the same admin-quorum approval.

Common values: 50 (simple admin majority, default), 67 (admin supermajority), 100 (unanimous admin consent — every admin must sign).

`adminThresholdNumerator` carried in `BootstrapPayload` (§4.4); joiners persist it in local state.

There is **no separate** `member_threshold_numerator` for Oligarchy. Member changes are authorized by the admin set, not by the member set. This is the structural difference from Democracy — Democracy's threshold is "what fraction of members must approve to change membership"; Oligarchy's threshold is "what fraction of admins must approve any change."

#### 4.7.7 Hiding which tree changed (the new privacy axis for Oligarchy)

Constraint #7 above lets `target_tree ∈ {0, 1}` be a private witness. Every Oligarchy update produces:

- The same wire-format public-inputs payload — 5 scalars: `c_old`, `epoch_old`, `c_new`, `occupancy_commitment_old`, `occupancy_commitment_new`. Byte-identical in shape to Democracy's payload. The combined `occupancy_commitment` (§4.7.2) bundles `member_occupancy_intermediate` and `admin_occupancy_intermediate` under `DOMAIN_COMBINED_OCCUPANCY`; the per-tree intermediates are circuit-internal and never reach the wire.
- The same proof byte length (192 bytes canonical Groth16). **Note**: this 192-byte figure refers to the Groth16 proof object itself (three group elements: `A` ∈ G1, `B` ∈ G2, `C` ∈ G1, totaling 48+96+48 bytes). It does *not* include the 5 public-input scalars carried alongside on the wire — those are separate ~32-byte fields per scalar. With v0.1.3, Democracy and Oligarchy share both proof byte length AND public-input scalar count — they are byte-indistinguishable on the wire end-to-end. Only Anarchy still differs (3 scalars vs 5).
- The same on-chain operation (`update_commitment` polymorphic entrypoint, same selector).

The only observable signals are the occupancy-commitment values themselves (which change per update) and the bundled `c_new` (which advances per epoch). A chain observer sees a single combined occupancy commitment updating but cannot distinguish:

- A member-add update from an admin-promotion update.
- An admin-demotion from a member-remove.
- Any of the above from a "key rotation" (member or admin replacing their own key in place — both bitmaps unchanged but `c_new` shifts due to different leaf hashes after rotation; this is still a single-leaf delta).

**Threat-model limitation (out-of-band caveat).** A chain observer who *also* has out-of-band access to one of the trees' state — e.g., compromised one client's local storage and read its persisted member list AND the per-epoch `salt_occ` distributed via `SEPSaltResponse.occupancySalt` — can recompute the combined commitment themselves and verify "this commitment matches the bitmaps I know with the salt I observed." This is the same threat profile as Democracy — out-of-band access to a peer's state defeats the on-chain hiding regardless of governance type. The on-chain privacy floor is what this design protects; out-of-band defenses (forward secrecy of local state, tamper-resistant storage, etc.) are layered separately and out of scope here.

**What v0.1.3 changed and why.** Earlier drafts (v0.1.1 / v0.1.2) exposed nine public-input scalars that included the per-tree occupancy commitments and `admin_root_old/new` directly. Even with the §4.7.7 "uniform proof byte length" mechanism, a chain observer could trivially compare `admin_root_old == admin_root_new` (or `member_occ_old == member_occ_new`) on each `update_commitment` call to learn which tree changed — defeating the §3.5 row "Which tree changed in any given update". v0.1.3 closes the trivial wire-comparison leak by (a) bundling the two per-tree occupancy intermediates into a single combined commitment under `DOMAIN_COMBINED_OCCUPANCY`, and (b) dropping `admin_root_old/new` from the wire entirely, with `admin_root` confined to a circuit-internal private witness that's bound into `c_old`/`c_new` via Poseidon. The combination produces a 5-scalar wire payload byte-identical to Democracy's.

**What v0.1.4 changed and why.** v0.1.3's combined commitment was salt-less, leaving the §3.5 claim vulnerable to a more sophisticated brute-force-from-public-priors attack: per §4.8, the initial bitmaps are publicly fixed (slot 0 occupied, rest tombstoned, both trees), so a chain observer can compute `member_occupancy_intermediate_0` and `admin_occupancy_intermediate_0` from chain history alone (no out-of-band access needed). After the first update, the observer enumerates ~256 candidate single-leaf-delta member bitmaps and ~32 candidate single-leaf-delta admin bitmaps, computes each candidate's intermediate, combines with the unchanged-other intermediate, and checks against the on-chain `occupancy_commitment_1`. At most one candidate matches — revealing which tree changed AND which slot toggled. By induction across all epochs, the observer recovers the full bitmap evolution. The §4.7.7 "out-of-band caveat" did NOT cover this, because public-priors come from chain history rather than out-of-band access. v0.1.4 closes the leak by adding `salt_occ` (a fresh per-epoch private-witness scalar) to the combined commitment formula in §4.7.2: `occupancy_commitment = Poseidon(Poseidon(DOMAIN_COMBINED_OCCUPANCY, member_occ, admin_occ), salt_occ)`. The brute-force search now requires guessing both the candidate bitmap AND a 252-bit salt, which is computationally infeasible. Salts are distributed off-chain via `SEPSaltResponse.occupancySalt` — same shape Democracy uses for `c_new`'s salt. The §3.5 row is now ✓ unconditionally for an on-chain-only observer (the out-of-band-with-salt threat-model limitation above remains). §7.1's privacy regression test is upgraded from a wire-shape check to a value-level distinguisher experiment that simulates the brute-force attack and asserts it fails. (Credit: the brute-force attack was identified in PR #147 review at https://github.com/rinat-enikeev/stellar-mls/pull/147#issuecomment-4322560271 and the salting fix corresponds to the reviewer's option-1 suggestion in that comment.)

### 4.8 Initial admin set, `create_oligarchy_group_v2`

```
create_oligarchy_group_v2(
    group_id,
    member_root,                   // create-time input only; not persisted to storage
    admin_root,                    // create-time input only; not persisted to storage
    occupancy_commitment,          // = Poseidon(Poseidon(DOMAIN_COMBINED_OCCUPANCY,
                                   //                     Poseidon(DOMAIN_OCCUPANCY,       fold(member_bitmap_initial)),
                                   //                     Poseidon(DOMAIN_ADMIN_OCCUPANCY, fold(admin_bitmap_initial))),
                                   //                  salt_occ_initial)
                                   //   per §4.7.2 v0.1.4 — persisted
    commitment_initial,            // = Poseidon(Poseidon(Poseidon(member_root, admin_root), 0), salt_initial) — persisted
    salt_initial,                  // create-time input; circuit-bound via initial_proof for c_old binding; not persisted
    salt_occ_initial,              // v0.1.4: create-time input; circuit-bound via initial_proof for occupancy_commitment binding; not persisted
    member_tier,
    admin_threshold_numerator,     // 1..=100
    initial_proof,                 // membership proof from creator binding member_root, admin_root, salt_initial, salt_occ_initial, occupancy_commitment, commitment_initial
)
```

The creator constructs `member_root` with themselves at member-slot 0 (tombstone elsewhere) and `admin_root` with themselves at admin-slot 0 (tombstone elsewhere). The `initial_proof` proves the creator knows the secret key behind both leaves (one secret key, used in two trees with different domain tags — see §4.1.1) AND that `commitment_initial` correctly bundles the supplied roots, `salt_initial`, and `epoch=0`, AND that `occupancy_commitment` correctly bundles `(member_bitmap_initial, admin_bitmap_initial, salt_occ_initial)` per §4.7.2 v0.1.4. After successful create, only `commitment_initial` and `occupancy_commitment` (plus `admin_threshold_numerator`, `member_tier`, `group_type`) are persisted; `member_root`, `admin_root`, `salt_initial`, and `salt_occ_initial` are consumed in verification and discarded. The same shape Democracy uses for `member_root` after create. **v0.1.4 specifically requires `salt_occ_initial` (not just `salt_initial`)**: without it the create-time `occupancy_commitment` is undefined under the §4.7.2 v0.1.4 formula, and §4.4's `SEPSaltResponse.occupancySalt` joiner-distribution channel needs a starting value to share.

**Creator MUST appear in both trees at create time.** `create_oligarchy_group_v2` rejects `member_count_initial == 0` (and analogously `admin_count_initial == 0`) — both with `Error::InvalidInitialMembership` (new error). The reason: the in-circuit floor `popcount(member_bitmap_new) ≥ 1` (§4.7.3 #5) would otherwise brick the group on first update — every update would fail the floor because `m_new` would have to grow from 0, and the single-leaf-delta witness can't insert into a "below the floor" starting state without a special-case relaxation that the v2 circuit doesn't carry. (An earlier draft of this section allowed admin-only zero-member groups; that's incompatible with §4.7.3 #5 and removed in v0.1.1.) Operators who want a "single-admin static observer" group instead use Anarchy with one member and never publish updates; Oligarchy is for groups that intend to grow.

### 4.9 Ceremony scope

Phase E (the trusted-setup ceremony) gains three new circuit families: `oligarchy_v2_tier{0,1,2}`. Combined with the three Democracy circuits and (separately) Anarchy's existing VK, the ceremony covers seven circuits total. Same MPC mechanics; same calendar (4–8 weeks per [Democracy §11](democracy-update-testnet-design.md#11-follow-up-work)'s estimate, marginal increase from adding three more circuit families to the same coordinated session).

### 4.10 Cross-tree atomic updates (NOT in v2)

A single update that changes both trees simultaneously (e.g., "promote Alice to admin AND add Bob as member" in one epoch) is **out of scope**. The constraint #7 dispatcher restricts each update to one tree. Cross-tree atomic operations are §11 follow-up — they require either a circuit that handles two single-leaf deltas in parallel (constraint count roughly +50%) or a sequence of two updates with epoch advances (simpler, slower UX).

---

## 5. Alternatives Considered

### 5.1 Use Stellar's `require_auth()` for admin authorization

Skip the ZK proof; have `update_commitment` for Oligarchy require a signature from a Stellar account in the on-chain admin allowlist.

- **Pros**: Drastically simpler; no admin-tree circuit work.
- **Cons**: Reveals which admin authorized each update (their Stellar account ID is in every transaction). Violates §3.5. Also reveals admin set membership trajectory by inspecting which Stellar accounts have ever been on the allowlist.

Rejected. Hard violation of the "no admin-related metadata leakage" P0.

### 5.2 Single tree with `isAdmin` flag per leaf

Encode admin status as a per-leaf boolean in the existing member tree. No second tree.

- **Pros**: ~30% smaller circuit; one bitmap instead of two.
- **Cons**: Promoting a member to admin is a single-leaf change in *value* (the leaf hash flips from `Poseidon(DOMAIN_MEMBER, …)` to `Poseidon(DOMAIN_MEMBER + isAdmin_flag, …)` or similar). But: the bitmap's "active" definition is now joint with admin status, so any admin promotion necessarily changes the occupancy commitment in a way that's distinguishable from a non-promotion key rotation. The §3.5 "which tree changed" property is harder to enforce. Also: the per-leaf encoding of admin-status leaks "this slot was ever an admin" forever via the leaf-hash trajectory.

Rejected. Two-tree shape is structurally cleaner for the privacy property.

### 5.3 Separate update entrypoints for member-update and admin-update

Two contract entrypoints (`update_member_oligarchy`, `update_admin_oligarchy`) instead of one polymorphic `update_commitment` with a hidden `target_tree`.

- **Pros**: Smaller circuits (one tree per circuit, not both); tighter constraint count.
- **Cons**: Reveals operation type via call selector — every update is observably-typed. Violates §3.5's "which tree changed" hiding. Also doubles the ceremony work (6 Oligarchy VKs instead of 3).

Rejected. The hidden `target_tree` witness is the main mechanism §3.5 needs.

### 5.4 Per-tier admin tree depth

Let the operator pick the admin tier independently of the member tier (Small / Medium / Large for both, 9 combinations).

- **Pros**: Bigger admin sets supportable.
- **Cons**: 9 ceremony combinations instead of 3 — 3× ceremony scope. Real-world admin sets are small; the marginal flexibility doesn't justify the cost.

Rejected. Admin tier fixed at Small (32 slots). §11 follow-up if a real use case demands more.

### 5.5 Cross-tree atomic updates in v2

Bundle member-and-admin changes into one proof.

- **Pros**: Some governance flows want this (e.g., "vote in a new admin and immediately remove the old one").
- **Cons**: Adds a second single-leaf-delta witness pair, ~50% more constraints, more complex circuit. Each operation is sequenceable as two separate updates with one epoch advance between — UX cost (one extra epoch) but no functional gap.

Rejected. v2 ships single-tree-per-update; cross-tree is §11.

---

## 6. Implementation Plan — six phases

Same phase shape as [Democracy §6](democracy-update-testnet-design.md#6-implementation-plan--six-phases). Each phase ends in a testable, mergeable state.

### Phase A — Oligarchy v2 circuit + prover

| Step | Files | LOC | Output |
|---|---|---|---|
| A1 | `src/circuit/oligarchy_v2.rs` (new) | ~600 | Two-tree bitmap binding (member + admin); bundled-roots `c_old`/`c_new`; combined occupancy commitment under `DOMAIN_COMBINED_OCCUPANCY` (§4.7.2); admin-quorum threshold; target-tree dispatcher witness + non-target-no-change constraints; member + admin floors; new domain tags `DOMAIN_ADMIN=4`, `DOMAIN_ADMIN_OCCUPANCY=5`, `DOMAIN_COMBINED_OCCUPANCY=6` declared in `src/circuit/domain_tags.rs` (extends Democracy's). |
| A2 | Tests for A1 | ~300 | Round-trip member-add, admin-promotion, admin-demotion, member-replace; tombstone-collision regression for both trees; bitmap-mismatch attack rejection per tree; threshold sweep on admin-quorum; target-tree-flip attack (witness `target_tree = 1` while presenting member-delta data — must fail); floor-violation rejection (member→0 or admin→0); §3.5 privacy regression `prove_oligarchy_v2_salt_freshness_changes_commitment` + `prove_oligarchy_v2_stale_salt_reuse_rejected` (per §7.1 — v0.1.4 property tests). |
| A3 | `src/prover/mod.rs` `prove_oligarchy_v2` + `verify_oligarchy_v2` | ~200 | Bitmap derivation for both trees, bundled-roots commitment computation, combined occupancy commitment computation (§4.7.2), target-tree inference from rosters, admin-signer Merkle path against `admin_root_old` (private witness, bound to `c_old` via Poseidon — never on the wire), `QuorumRequired` for `admin_count_old ≥ 3`. |
| A4 | `keyset-oligarchy-dev/{tier0-k1,tier1-k1,tier2-k1}/` (admin tier fixed at Small=32 across all member tiers; `_k1` reflects single-admin-signer single-K). Generated via extended `scripts/generate-democracy-vk-dev.sh` (renamed `scripts/generate-governance-vks-dev.sh`). | (binary VK files) | Three dev VKs (one per member tier; admin tree fixed at depth 5). Fingerprints checked into `keyset-oligarchy-dev/fingerprints-v2.json`. |
| A5 | `Cargo.toml` feature `oligarchy-v2-dev-vks` | ~10 | Off by default. Independent of `democracy-v2-dev-vks` (mainnet build verifies BOTH features are off). |
| A6 | Cross-platform test vectors `docs/cross-platform-test-vectors.json` | ~80 | New `oligarchy_v2` section; reference proofs for member-add, admin-promotion, admin-demotion, member-replace flows. |

**Phase A exit criteria**: `cargo test --features oligarchy-v2-dev-vks circuit::oligarchy_v2` and `cargo test prover::oligarchy_v2_round_trip` both green. R1CS constraint count for tier 1 within 10% of §4.7.3 estimate (~11300, post-v0.1.4-reconciliation; tier-0 ~8880, tier-2 ~27500 same ±10% bound). Three dev VKs checked in with fingerprint manifest.

Phase A is the load-bearing crypto step (R1CS soundness review for the two-tree dispatcher constraint #8 is the new attention area beyond Democracy's review).

### Phase B — FFI + Swift/Kotlin bridges

| Step | Files | LOC | Output |
|---|---|---|---|
| B1 | `src/ffi.rs` `sep_generate_oligarchy_update_proof_v2` | ~80 | |
| B2 | `swift-mls/.../Types.swift` Oligarchy variant on `UpdateCommitmentPublicInputs` | ~50 | |
| B3 | `swift-mls/.../RustBridge.swift` + `ProofGenerator.swift` `generateOligarchyUpdateProofV2` | ~80 | |
| B4 | `kotlin-mls/.../Types.kt` parallel | ~50 | |
| B5 | `kotlin-mls/.../RustBridge.kt` + `SEPProofGenerator.kt` parallel | ~80 | |
| B6 | XCFramework + Android NDK rebuild | (CI) | |
| B7 | Bridge round-trip tests | ~120 | |

Total: ~460 LOC. Same parallelism with Phase C as Democracy's §6.

### Phase C — Contract redeploy with Oligarchy entrypoints

| Step | Files | LOC | Output |
|---|---|---|---|
| C1 | `contracts/sep-xxxx/src/lib.rs` Oligarchy v2 entrypoints | ~280 | Polymorphic `update_commitment` Oligarchy variant. Storage shape per Oligarchy group: shared `commitment` and `occupancy_commitment` (combined per §4.7.2) with Democracy, plus per-Oligarchy `admin_threshold_numerator`. Existing `admin_root` storage field, `get_admin_root` getter, and `MissingAdminRoot=19` / `AdminRootMismatch=20` errors are removed (v0.1.3 §3.5 fix); `admin_count` deliberately NOT stored as a separate field. New `create_oligarchy_group_v2` taking the tuple from §4.8. New `Error::InvalidAdminThreshold` variant. Verifier reads `current.admin_threshold_numerator` from storage. |
| C2 | Contract tests | ~280 | Mirror existing Oligarchy tests against v2. New tests: target-tree dispatcher (member-update + admin-update both succeed against the same VK); type-confusion guard rejects an Oligarchy proof submitted to a Democracy group; `m_member_new == 0 || m_admin_new == 0` rejected per the new floors; threshold-mismatch rejected; admin-tier-cap enforcement (33rd lifetime admin add rejected). |
| C3 | Cargo crate version bump | ~5 | |
| C4 | `scripts/deploy_sep_xxxx_testnet.sh` (extends Democracy's) | (script) | Same fresh contract address as Democracy redeploy (one address covers all governance types). |
| C5 | `scripts/install-governance-vks-testnet.sh` (renamed from democracy-only script; covers all Oligarchy + Democracy VKs in one install) | ~30 | |
| C6 | Smoke test on testnet | (manual) | `create_oligarchy_group_v2` → `update_commitment` (Oligarchy variant, member-add path) → `update_commitment` (admin-promote path) round-trip via stellar CLI. |

Total: ~595 LOC.

### Phase D — Relayer + clients

| Step | Files | LOC | Output |
|---|---|---|---|
| D1 | `relayer/src/handler.rs` Oligarchy variant in the public-inputs translation | ~30 | Polymorphic dispatch already handles the new variant — Oligarchy's 5-scalar payload is byte-identical to Democracy's, so the relayer-side translation is the same shape; the only practical difference is the verifying-key family the contract picks at receive time based on `current.group_type`. |
| D2 | `swift-mls/.../ContractClient.swift` `updateCommitment` overload accepting Oligarchy variant | ~30 | |
| D3 | `kotlin-mls/.../ContractClient.kt` parallel | ~30 | |
| D4 | iOS `OnChainService.publishCommitmentUpdate` Oligarchy dispatch | ~180 | Branches on `group.groupType == .oligarchy`; assembles both bitmaps; chooses `target_tree` based on which roster changed; calls `generateOligarchyUpdateProofV2`; honors §4.3 Layer 2 fingerprint check. Slot-index assignment for both trees in `applyStateUpdate`. Rollback path extension for both bitmaps per §4.1.4. |
| D5 | iOS `applyStateUpdate` admin-promotion / demotion handlers | ~120 | New protocol message variants: `SEPAdminPromoted`, `SEPAdminDemoted`. Concurrent-acceptor and offline-recovery paths cover admin-tree state. |
| D6 | Android parallel for D4 + D5 | ~300 | |
| D7 | `RelayerDefaults` + client config | ~10 | |
| D8 | iOS UI: admin-set picker (which members are admins; promotion/demotion buttons gated on local-user-is-admin) | ~150 | |
| D9 | Android UI parallel | ~150 | |
| D10 | `SEPSaltResponse` Oligarchy padding (member + admin arrays) | ~20 | |

Total: ~1020 LOC.

### Phase E — Ceremony coordination (extends Democracy's Phase E)

Same MPC ceremony, three additional circuits (Oligarchy at tier 0/1/2). Six circuits total per ceremony (3 Democracy + 3 Oligarchy); Anarchy's existing ceremony VK is preserved. Pre-ceremony R1CS soundness review covers both circuit families; cryptography reviewers evaluate the two-tree dispatcher constraint as a new soundness surface.

Production VKs install via `scripts/install-governance-vks-mainnet.sh` (new); same fingerprint allowlist coordination as [Democracy §8.3](democracy-update-testnet-design.md#83-dev-vk-fingerprint-allowlist-still-load-bearing) but extended to cover Oligarchy fingerprints.

### Phase F — Multi-admin quorum (parallel to Democracy's Phase F)

Quorum-collection design doc (proposal lifecycle, vote tally, K-of-N partial-proof aggregation). Implements the `K ≥ ⌈admin_threshold_numerator · m_old / 100⌉` general case. Required before Oligarchy groups can have admin sets of 3+ members. **Shared design with Democracy's Phase F** — the quorum-collection logic is the same regardless of which set the K signers are drawn from. One Phase F design doc covers both governance types.

### Cumulative scope estimate

| Phase | Engineering LOC (approx) | Calendar (engineer-weeks) |
|---|---|---|
| A — circuit + prover | ~1190 (A1 ~600 + A2 ~300 + A3 ~200 + A5 ~10 + A6 ~80; A4 produces binary VK files, no source LOC) | 3–4 (R1CS review of two-tree dispatcher is the gate) |
| B — FFI + bindings | ~460 | 1 |
| C — contract redeploy | ~595 | 1 |
| D — relayer + clients | ~1020 | 2–3 |
| **A–D total (testnet-functional Oligarchy)** | **~3265** (1190 + 460 + 595 + 1020) | **7–9** |
| E — ceremony (incremental over Democracy) | (mostly process) | 1–2 (calendar; coordination only — three more circuits in the same MPC session) |
| F — multi-admin quorum | (shared with Democracy F) | not estimated |

Larger than Democracy's ~2710 — the second tree adds ~17% across A and D, with C roughly comparable. Phase E is mostly free-rider on Democracy's coordination. Implementation can run **in parallel** with Democracy: A/B/C/D for both governance types share the redeployed contract, the polymorphic `update_commitment` dispatch, and the ceremony session.

---

## 7. Test Plan

Inherits all of [Democracy §7](democracy-update-testnet-design.md#7-test-plan)'s shape. Oligarchy-specific additions:

### 7.1 Rust unit tests (Oligarchy-specific)

- `prove_oligarchy_v2_round_trip_member_add` — single admin signer authorizes a 1 → 2 member addition. Member-tree single-leaf delta; admin tree unchanged. Verifier passes.
- `prove_oligarchy_v2_round_trip_admin_promote` — single admin signer authorizes a 1 → 2 admin promotion. Admin-tree single-leaf delta; member tree unchanged.
- `prove_oligarchy_v2_round_trip_admin_demote` — single admin signer authorizes the demotion of another admin. Admin-tree single-leaf delta (Remove operation); member tree unchanged. Floor `admin_count_new ≥ 1` enforces the demoter cannot demote themselves if they're the last admin.
- `prove_oligarchy_v2_target_tree_witness_lying_rejected` — prover witnesses `target_tree = 0` (member) but supplies an admin-tree delta. The non-target-no-change constraint #8 detects the inconsistency at proof-gen and rejects. Soundness regression for §4.7.7.
- `prove_oligarchy_v2_member_floor_rejected` — last-member-removal attempt; `member_count_new = 0`. Prover rejects via in-circuit floor.
- `prove_oligarchy_v2_admin_floor_rejected` — last-admin-removal attempt; `admin_count_new = 0`. Prover rejects.
- `prove_oligarchy_v2_admin_threshold_sweep` — same shape as Democracy's threshold-sweep test, applied to admin set: 50 / 67 / 75 / 100 each pass for K satisfying, fail for K below quorum.
- `prove_oligarchy_v2_admin_signer_not_in_admin_tree_rejected` — signer's secret key opens against member tree but not admin tree. The admin-tree Merkle-opening constraint (§4.7.3 #12 "Existing constraints unchanged: signer Merkle openings against `root_admin_old`") detects the missing path. (Earlier draft cited "Constraint #1" — that's the bitmap-allocation step, not the signer opening; renumbered.)
- `prove_oligarchy_v2_admin_tier_cap_exhaustion` — 33rd lifetime admin promotion attempt. Prover rejects (admin slot space exhausted; no never-used slot available).
- `prove_oligarchy_v2_admin_tier_cap_with_tombstones` — boundary regression: 29 admin slots tombstoned + 3 active = 32 lifetime appointments reached, then attempt to promote a 33rd. Same rejection as above but exercises the slot-bookkeeping path where lifetime ≠ active. Catches off-by-one bugs that pass the simple "fill 32 actives" test but fail when tombstoned slots count toward the cap. Pinned because §11 acknowledges 32 may be tight in real deployments and the v2 design has no recovery once the cap is hit.
- `prove_oligarchy_v2_member_tombstone_collision` and `prove_oligarchy_v2_admin_tombstone_collision` — paired domain-tag-disabled / enabled regression tests for both trees independently. Same shape as Democracy's `prove_democracy_v2_domain_tags_block_tombstone_collision`.
- `prove_oligarchy_v2_bundled_roots_consistency` — verifier recomputes `c_old` from `(root_member, root_admin, epoch_old, salt_old)` and confirms it matches the public-input value. Since `admin_root` is a private witness (not on the wire), this test exercises the in-circuit binding — flipping any bit of the witnessed `admin_root` while keeping `c_old` constant must fail Groth16 verification (replaces v0.1.2's wire-side `admin_root_bundling_rejected` test).
- `prove_oligarchy_v2_combined_occupancy_consistency` — verifier recomputes `occupancy_commitment` from `(member_bitmap, admin_bitmap, salt_occ)` via the §4.7.2 nesting and confirms it matches the public-input value. Per-tree intermediates and `salt_occ` derived as private witnesses; their values cannot be guessed from the combined commitment alone.
- `prove_oligarchy_v2_salt_freshness_changes_commitment` — **the §3.5 privacy regression (v0.1.4 upgrade), property-test variant.** Replaces v0.1.3's wire-shape-only test AND the earlier-v0.1.4 simulated-search variant (which was effectively tautological — 2^20 sampled salts vs 2^252 search space made the false-positive bound vanishingly small with or without the salt being load-bearing). The property test pins the salt's role directly:
  1. Generate two valid update proofs from byte-identical prior state with byte-identical bitmaps (same `member_bitmap`, same `admin_bitmap`) but different `salt_occ` values (e.g., `salt_occ_a` ≠ `salt_occ_b`, each freshly sampled).
  2. **Assert** the two proofs produce **distinct** `occupancy_commitment` values on the wire: `occ_commit(bitmap, salt_occ_a) ≠ occ_commit(bitmap, salt_occ_b)`. This proves the salt enters the commitment in a load-bearing way; if the salt were absent or ignored by the circuit, the two commitments would collide and the assertion would fail.
  3. Additionally test the wire-shape distinguishability: construct two updates from the same prior state — (a) member-add (member-tree single-leaf delta; admin tree unchanged); (b) admin-promotion (admin-tree single-leaf delta; member tree unchanged). Generate valid proofs for both. Assert (i) both public-input vectors have exactly 5 scalars; (ii) both proofs are 192 bytes; (iii) `c_old` is byte-identical between (a) and (b) (same prior state); (iv) `c_new`, `occupancy_commitment_new`, and `epoch_old` advance the same way in both (no per-tree distinguisher in any wire field).

  **Test fails the build** if (a) the salt is dropped from the §4.7.2 formula (step 2 collides), OR (b) any future change reintroduces a per-tree distinguisher on the wire (step 3 detects it).

- `prove_oligarchy_v2_stale_salt_reuse_rejected` — **prover salt-freshness honor-system gate.** Salts are private witnesses self-distributed via the §4.4 channels; freshness is not enforceable on-chain. This test pins the prover's salt-sampling path: invokes `prove_oligarchy_v2` twice in a row from the same client-side state (same prior epoch, same target update, fresh process-local randomness) and asserts the two calls sample **distinct** `salt_occ` values. A regression where the prover caches the salt or uses a deterministic seed (which would cause cross-epoch-update commitment collisions and let an observer correlate updates) is detected here. (The contract has no recourse if a buggy prover ships a stale-salt build; this test is the prover-side preflight gate.)
- `prove_oligarchy_v2_salt_chain_forward_byte_faithful` — **load-bearing in-band chain-forward test.** v0.1.4's per-epoch salt distribution lives in `SEPGroupStateUpdate.occupancySalt` (§4.4); the §3.5 privacy guarantee depends on each member receiving the byte-faithful salt that bound the prior epoch's `occupancy_commitment`. This test simulates two members (acceptor + non-acceptor) chaining across two epochs:
  1. Member A creates the group at epoch 0 with `salt_occ_initial` known to both A and B (BootstrapPayload).
  2. Member A submits a member-add update at epoch 1 with fresh `salt_occ_1`. The prover broadcasts `SEPGroupStateUpdate { ..., occupancySalt: salt_occ_1 }` to member B.
  3. Member B persists `salt_occ_1` from the broadcast.
  4. At epoch 2, member B (not A) submits a new update. B's prover uses `salt_occ_1` to recompute `occupancy_commitment_old` (binding `current.occupancy_commitment`). The new update binds fresh `salt_occ_2` and broadcasts.
  5. **Assert** the contract accepts B's update at epoch 2 — i.e., the bytes B persisted from A's broadcast were faithful to what A's prover used. A regression (wire-format drift, lossy serialization, byte-order swap) breaks this end-to-end path.

  Without this test, a wire-format bug in `SEPGroupStateUpdate.occupancySalt` would only surface in production when a non-acceptor tries to update — at which point the group is already bricked.

### 7.2 Cross-platform vectors

`docs/cross-platform-test-vectors.json` adds an `oligarchy_v2` section covering the four operations (member-add, admin-promote, admin-demote, member-replace) with reference proofs and expected occupancy commitments for both trees.

### 7.3 Relayer integration

Polymorphic `update_commitment` already accepts arbitrary `UpdateCommitmentPublicInputs` variants — Oligarchy adds one test asserting the relayer forwards a 5-scalar Oligarchy payload to stellar CLI without re-shaping it (byte-identical in shape to the Democracy variant; only the contract-side VK family selection differs). The test additionally asserts the relayer does NOT inject any per-tree decomposition of `occupancy_commitment` into the call — preserving the §3.5 indistinguishability claim end-to-end through the relayer.

### 7.4 iOS / Android integration

- `DemocracyMembershipTests` parallel: `OligarchyMembershipTests.swift` / `OligarchyMembershipTest.kt`. Boots in-memory `OnChainService` mocks; drives 1 → 2 member-add and 1 → 2 admin-promote flows; asserts the correct variant of `UpdateCommitmentPublicInputs` is emitted, slot indices assigned and persisted to both bitmaps, the `target_tree` witness chosen correctly per operation type.
- `OligarchyAdminUiVisibilityTests` — admin-only UI controls (promote/demote buttons) are gated on `currentUser.isAdminInGroup(group)`. Asserts a non-admin viewing the chat sees no admin-management surface; an admin sees the controls.
- Feature-flag-off behavior: same dedicated test target pattern as Democracy. The Oligarchy FFI symbol is conditionally compiled out when `oligarchy-v2-dev-vks` is off; `StellarChatTests-NoOligarchy` asserts the `dlsym` fail and the strict-refusal path.
- Layer 2 fingerprint mismatch + slot back-channel + concurrent-acceptor rollback — all parallel to Democracy's tests, applied to Oligarchy.
- **Bitmap derivation determinism (both trees)** — `OligarchyBitmapDerivationTests.swift` asserts the canonicalized member bitmap AND admin bitmap are byte-identical to Rust prover and Kotlin client outputs (cross-checked via §7.2 vectors). Two trees, two assertions.
- **`UpdateCommitmentPublicInputs` discriminator serialization** — extended to cover Oligarchy variant.

### 7.5 Manual testnet end-to-end

Pre-condition: Phase C complete (fresh testnet contract; `scripts/install-governance-vks-testnet.sh` run; Oligarchy v2 dev VKs installed alongside Democracy's).

Steps (member-add + admin-promote sequenced):

1. iOS creates an Oligarchy group at medium tier with `admin_threshold_numerator = 50`. Verify on Soroban testnet that `current.group_type == 3`, `current.admin_threshold_numerator == 50`, `current.commitment` is set, `current.occupancy_commitment` (combined per §4.7.2) is set. Verify NO `admin_root` field exists in the contract storage entry (per the v0.1.3 §3.5 fix) and NO `get_admin_root` getter is callable.
2. iOS sends an invitation to Android.
3. Android accepts. Android sends `SEPMemberJoined`.
4. iOS as the only admin processes the SMJ, runs Oligarchy v2 path, polymorphic `update_commitment` with `target_tree=member` private witness. Verify relayer log shows `function=update_commitment` with 5-scalar Oligarchy variant payload (byte-identical in shape to a Democracy payload), status 200.
5. iOS local state advances: epoch 1, 2 members (slots 0, 1), 1 admin (slot 0).
6. Android receives broadcast; persists its member-slot index = 1; sees admin set unchanged.
7. iOS chat to Android works; Android chat to iOS works (both members, BLS auth passes).
8. iOS as admin promotes Android: sends `SEPAdminPromoted`. iOS runs Oligarchy v2 path with `target_tree=admin`. Member tree unchanged; admin tree gains Android at admin-slot 1. Verify relayer log: same 5-scalar payload shape as step 4, no observable difference except the values of `c_old`/`c_new` and the combined `occupancy_commitment_old`/`occupancy_commitment_new` (which advance every update regardless of which tree changed).
9. **Privacy verification**: read the contract storage entry for this group via Soroban testnet RPC. Assert no `admin_count` or `member_count` field is present, AND no `admin_root` field is present (the v0.1.3 §3.5 fix removed it), AND no `salt_occ` field is present (v0.1.4 — salt is private to the prover). Compare the on-chain trace of the two `update_commitment` calls (steps 4 and 8); assert the call payloads have **identical scalar count, identical byte length, AND no value-level distinguisher**: in particular, no public-input field at index 0..4 takes a "moved vs. unchanged" pattern that lets a chosen distinguisher decide which tree was the target. (In v0.1.2 the `admin_root_old==admin_root_new` test trivially decided this; in v0.1.3 there is no such field on the wire, but a brute-force-from-priors attack against the salt-less combined commitment still worked; v0.1.4's salt closes that.) Run the `prove_oligarchy_v2_salt_freshness_changes_commitment` property test against the captured on-chain payloads as the post-deploy gate: two updates from byte-identical bitmaps with different salts must produce distinct on-chain `occupancy_commitment` values, pinning the salt's load-bearing role. A chain observer cannot distinguish "added a member" from "promoted an admin" from these calls alone, modulo the §4.7.7 out-of-band threat-model limitation (which now also requires out-of-band access to `salt_occ`).

Failure-mode tests:

- §4.3 Layer 2 fingerprint mismatch (parallel to Democracy).
- Quorum-required after admin set reaches 3: 4th admin promotion attempt; expect `QuorumRequired`.
- Member-removal of last member: rejected (member floor).
- Admin-demotion of last admin: rejected (admin floor).
- Admin tier cap: attempt 33rd lifetime admin appointment; rejected.

### 7.6 VK-mismatch handling, end-to-end (inherited from Democracy)

7.6.1 (no Oligarchy VK installed) and 7.6.2 (wrong Oligarchy VK installed) — same shape as Democracy's tests, applied independently.

---

## 8. Rollout (cross-phase coordination)

Same gating mechanism as [Democracy §8](democracy-update-testnet-design.md#8-rollout-cross-phase-coordination). Phase A through D for Oligarchy can run **in parallel with** Democracy's same phases since they share the redeployed contract, the polymorphic `update_commitment` dispatch, and the same ceremony session.

The natural sequencing:

1. Land both design docs (Democracy v0.5 in PR #146, Oligarchy v0.1 in this PR).
2. Phase A (circuits) for both governance types — separate PRs, parallel review.
3. Phase B + C in parallel (one redeployed contract carries both Oligarchy and Democracy entrypoints).
4. Phase D (clients) — both governance types' UX in same client release.
5. Phase E (ceremony) — single MPC session covers all six circuits (3 Democracy tiers + 3 Oligarchy tiers).
6. Mainnet release covers both governance types simultaneously.

If priorities diverge, Democracy can ship D before Oligarchy (single-governance-type testnet release), and Oligarchy follows in a later client update.

---

## 9. Risks

Inherits all of [Democracy §9](democracy-update-testnet-design.md#9-risks)'s risks. Additions specific to Oligarchy:

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Two-tree dispatcher (constraint #8 non-target-no-change) has a soundness bug | Low | High (admin operations could cross-write to member tree or vice versa) | Phase A's R1CS soundness review focuses specifically on the dispatcher constraint. Test `prove_oligarchy_v2_target_tree_witness_lying_rejected` is the dedicated regression. |
| Admin tier cap (32 admins) inadequate for some use case | Medium | Low–Medium (operators warned in UX; recreate group as workaround) | Documented in §4.6 and §11. Operators choose admin-heavy use cases at their own risk. Phase F or §11 follow-up could lift the cap with additional ceremony work. |
| Self-bricking via last-admin demotion | Low | High (group permanently unmodifiable) | Admin floor `admin_count_new ≥ 1` is in-circuit (constraint #6). Tested. UX warning before the demotion confirms. |
| Self-bricking via last-member removal | Low | Medium (group has no chat capacity but can still be modified by admins) | Member floor `member_count_new ≥ 1` enforced. UX warning. (An admin-only zero-member group is technically valid but useless; if it happens, recreate.) |
| Domain-tag collision (now six values: 1–6) | Low | High | §10 Q1 cross-circuit audit covers values 4, 5, and 6 in addition to 1/2/3. |
| Admin set leaks via timing of admin-only operations (an admin promotion is observable as "an update happened" — chain timestamps reveal cadence) | High (intrinsic to public ledger) | Low (cadence is the same residual as Democracy; Oligarchy doesn't add new cadence-class signals) | Acknowledged. Same as Democracy's epoch-counter residual. |
| Cross-tree-update demand (operators want to combine member + admin changes atomically) | Medium | Medium (UX cost — two updates instead of one, one extra epoch) | §4.10 / §11. Documented limitation; not blocking. |
| **`salt_occ` loss bricks future updates (v0.1.4 risk)** — salt is private and distributed only via `SEPSaltResponse.occupancySalt` (§4.4); no on-chain copy. If a group loses its current `salt_occ` (all replicas wipe simultaneously, sync channel down for the whole group), no member can produce the next valid update proof since the in-circuit binding `current.occupancy_commitment == Poseidon(…, salt_occ_old)` cannot be satisfied without knowing `salt_occ_old`. | Low | High (group permanently unupdatable; chat still works for existing members but membership cannot change) | Documented as a Phase D operability requirement: clients MUST replicate `salt_occ` to all members on every update broadcast (§4.4 SEPGroupStateUpdate carries the new salt) and SHOULD warn users before mass-wipe. ~~The deactivate-group safety valve still works (membership proof opens against the member tree, no salt needed).~~ The deactivate-group safety valve was removed in postmortem #153; if `salt_occ` is irretrievably lost, the group is unupdatable and there is no on-chain recovery path. Operators must rely on TTL aging-out or admin VK rotation as suppression mechanisms. Same shape as Democracy's `c_new`-salt loss risk; not Oligarchy-specific in nature, but new in this design as the second salt distinct from `c_new`'s. |
| **Stale `salt_occ` reuse across epochs (v0.1.4 risk)** — salt freshness is an honor-system property of the prover; if a buggy prover caches the salt or seeds it deterministically across epochs, a chain observer can correlate updates by detecting commitment patterns that depend on the bitmap delta only. | Low | Medium (privacy regression — the salt's brute-force-blocking guarantee weakens to "fresh per build, not per epoch") | Test `prove_oligarchy_v2_stale_salt_reuse_rejected` (§7.1) pins the prover's salt-sampling path. CI gate prevents the regression from shipping. |

---

## 10. Open Questions

### 10.1 Phase-A blockers (must ratify before A1 opens)

1. **Domain tag values (extends [Democracy §10.1 Q1](democracy-update-testnet-design.md#101-phase-a-blockers-must-ratify-before-a1-opens)).** Recommended values: `DOMAIN_MEMBER=1`, `DOMAIN_TOMBSTONE=2`, `DOMAIN_OCCUPANCY=3`, `DOMAIN_ADMIN=4`, `DOMAIN_ADMIN_OCCUPANCY=5`, `DOMAIN_COMBINED_OCCUPANCY=6` (last one added in v0.1.3 per the §3.5 privacy fix). Audit pass: same 1-hour grep against current `main` to confirm no collision; codified in `src/circuit/domain_tags.rs`. **v0.1.4 note:** the new `salt_occ` private-witness salt added to the combined commitment formula in §4.7.2 is **not** a new domain tag — it is an extra Poseidon input absorbed by the existing combine hash (or a chained 2-arity hash, see §4.7.2 for implementation choice). The six-tag set 1..=6 is unchanged. The audit need only verify `salt_occ`'s position in the prover's hash-input order matches the contract verifier's expectation (parallel to the existing `c_new` salt's position in Democracy's verifier).

2. **Admin tier cap value.** Recommended: 32 (Small tier, depth=5). Lifetime admin appointments capped at 32 per group. Operators warned. Larger admin trees are §11 follow-up if a real use case demands.

3. **Admin signer K-bound for tier-2.** Same as Democracy §10.1 Q2 — k_max for the admin signer slots. Recommended: k_max=1 for v2 single-admin-signer subset (multi-admin is Phase F). The Phase F redesign for K-of-N admin quorum will need a different VK.

### 10.2 Other open questions

4. **Should member tree and admin tree share a `target_tree`-typed dispatcher, or always emit a uniform-shape proof regardless of which tree changed?** Current design (uniform-shape via `target_tree` private witness) is the correct call for §3.5. Open: when Phase F's multi-admin-quorum redesign lands, does the dispatcher still hold? Needs a re-check at the start of Phase F.

5. **What happens when the only admin is also the only member, and they try to remove themselves?** Either floor (member or admin = 0) trips at the first removal step. The admin can't remove themselves and promote a new admin in one update (cross-tree changes are out of scope per §4.10). Workaround requires growing both trees first; the minimal sequence (**at default `admin_threshold_numerator = 50`**):
   1. Add a second member (1 → 2 members). Single-signer K=1 from the original admin authorizes (per §4.2 default-50 threshold table; `100·1 ≥ 50·1`).
   2. Promote that member to admin (1 → 2 admins). Single-signer K=1 from the original admin again.
   3. Original creator removes self from member tree (2 → 1 members). Member floor `≥1` satisfied. Admin signer is now either the original creator OR the new admin — either suffices for K=1 since `admin_count_old=2` and `100·1 ≥ 50·2` (tie passes).
   4. Original creator demotes self from admin tree (2 → 1 admins). Authorized by the new (remaining) admin. K=1 from `admin_count_old=2`.

   **Four updates, four epochs.** (An earlier draft cited a three-update workaround; that version skipped step 1 and immediately tried "remove self from member tree" from a 1-member starting state — which would violate the member floor since `m_new` drops to 0. Corrected here.) UX cost is high; clients SHOULD warn when the user is about to commit to becoming a member-and-admin singleton group, since exit cost is steep.

   **At stricter thresholds (51 / 67 / 75 / 100), step 3 is blocked** (the §4.2 single-signer table caps `K=1` at `admin_count_old=1` for any threshold > 50; step 3's `admin_count_old=2` would require `K≥2`, which Phase F enables). For groups created with stricter `admin_threshold_numerator`, the only single-signer-feasible self-eviction sequence is steps 1–2 (grow to 2-member-2-admin) and stop — the original creator remains a member AND admin until Phase F's K-of-N quorum lands. Clients SHOULD also warn at create-time when a stricter-than-50 threshold is chosen, since self-eviction becomes Phase-F-gated rather than just expensive.

6. **Can a non-member be promoted to admin?** Technically yes — admin tree and member tree are independent. A "non-member admin" is someone who can authorize updates but doesn't appear in the member list (no chat capacity). Probably not useful but not blocked. Recommendation: allow it; clients surface a warning "this admin is not a member of the group; promote them as a member first?" — operator can override.

### 10.3 Resolved (folded into core design)

- Single-tree vs two-tree shape: two trees (§4.7).
- Admin status per-leaf flag vs separate tree: separate tree (§5.2 rejected).
- Per-tier admin depth: fixed at Small (§5.4 rejected).
- Cross-tree atomic updates: out of scope (§4.10).
- **Zero-member admin-only groups: disallowed.** Earlier-draft Q5 asked whether admin-only zero-member groups should be allowed. Resolved as no — `create_oligarchy_group_v2` rejects `member_count_initial == 0` per §4.8 amendment. The in-circuit floor `popcount(member_bitmap_new) ≥ 1` (§4.7.3 #5) makes any post-create update from a zero-member state impossible; allowing the create state would brick the group on its first attempted update.

---

## 11. Follow-Up Work

- **Phase F — Multi-admin quorum.** Shared design with Democracy's Phase F. Implements `K ≥ ⌈admin_threshold_numerator · m_old / 100⌉` for `m_old ≥ 3`. Required before Oligarchy groups can have admin sets of 3+ admins. Multi-month effort.
- **Cross-tree atomic updates.** A single proof that changes both trees in the same epoch (e.g., "promote Alice and add Bob"). Constraint count ~+50%. UX gain: one chain operation per governance event instead of two. Separate design doc when there's a use case.
- **Larger admin tier (Medium / Large).** Lift the 32-admin cap. New circuit per (member tier, admin tier) combination — 9 circuits instead of 3. Worth it only if a real use case demands.
- **Threshold rotation** (parallel to Democracy's threshold rotation in [§11](democracy-update-testnet-design.md#11-follow-up-work)). Letting an existing Oligarchy group vote to change `admin_threshold_numerator`. Same meta-governance design challenge — which threshold ratifies the meta-vote?
- **Member-only oligarchy** (admin set = creator only, never modified). A simpler degenerate case. Probably handled fine by the v2 design as-is (just don't promote / demote anyone); no separate shape needed.
