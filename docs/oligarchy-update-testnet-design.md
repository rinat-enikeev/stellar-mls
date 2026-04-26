# Oligarchy Group Updates — Testnet-Gated Implementation Design

**Date:** 2026-04-26
**Status:** Draft (Proposal — pre-implementation)
**Author:** Onym contributors
**Version:** 0.1.1 — addresses @gramyzer REQUEST_CHANGES on v0.1: §4.4 admin_root claim corrected (Poseidon is one-way; admin_root is stored separately, not derived from c_old; wire payload extended to 9 scalars to carry admin_root_old/new explicitly); §4.2 single-signer cutoff generalized for non-default thresholds (parallel to Democracy v0.5.1 fix); §7.1 constraint reference renumbered to #12; §6 Phase A LOC arithmetic corrected to ~1190; §4.8 zero-member create disallowed (would brick on first update); §10.2 Q6 self-removal workaround corrected to four steps (was three, but skipped a step that violated the member floor); §4.7.7 "Defeats:" renamed to "Threat-model limitation"
**Supersedes:** (none — first iteration)
**Related:**
- [`democracy-update-testnet-design.md`](democracy-update-testnet-design.md) — sibling design; many mechanisms (slot-index convention, occupancy commitment, polymorphic dispatch, testnet gating, ceremony) are inherited verbatim and cross-referenced rather than duplicated below.
- [`group-governance-types-design.md`](group-governance-types-design.md) — parent design that introduces `groupType ∈ {Anarchy=0, OneOnOne=1, Democracy=2, Oligarchy=3}`.
- [`democracy-circuit-ceremony.md`](democracy-circuit-ceremony.md) — extends to cover Oligarchy circuits as part of the same Phase 2 ceremony.
- `contracts/sep-xxxx/src/lib.rs:197-200` (`MissingAdminRoot=19`, `AdminRootMismatch=20`), `:1154-1248` (`create_oligarchy_group`), `:1293-1300` (`get_admin_root`). Updates against Oligarchy groups currently have no contract entrypoint at all — the missing piece this design specifies.

---

## 1. Background

Oligarchy groups (`groupType = 3`) extend the membership model with a privileged **admin subset** — a second Merkle-committed set whose holders are the only parties authorized to propose state changes. The contract today supports oligarchy *creation* via `create_oligarchy_group(group_id, admin_root, …)` (`lib.rs:1154`), which seeds the admin set as a Poseidon-committed tree alongside the standard membership tree. **There is no Oligarchy update entrypoint**. Any attempt to change the membership of an Oligarchy group fails the same way Democracy does today — the `update_commitment` arm hard-rejects non-Anarchy types — but with no per-type entrypoint to fall back to.

`swift-mls/Sources/SwiftMLS/ContractClient.swift:88-91` exposes `createOligarchyGroup` but no `updateCommitmentOligarchy`. The gap is symmetric to Democracy's pre-v0.5 state, with one structural addition: Oligarchy carries **two trees** (member set + admin set) instead of one, and any update must prove an admin authorized it without revealing which admin.

This design fills the gap. It commits to the same direct-to-v2 trajectory as Democracy: hidden counts via occupancy commitment, polymorphic `update_commitment` dispatch, dev-key ceremony for all three tiers, fresh contract redeploy, no backward compatibility required. The "no metadata leakage" privacy floor extends from members to admins — admin count, admin-set trajectory, and which admin co-signed any update are all hidden.

---

## 2. Problem

Oligarchy groups can be created on chain but never modified. There's no `update_commitment` arm that accepts `group_type == 3`, no proving function, no FFI export, no client dispatch. The contract knows about `admin_root` storage but has no read path that consumes it (apart from `get_admin_root`, which is a passive getter).

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
| The group is Oligarchy-typed | `group_type=3` from `create_oligarchy_group_v2` argument and contract storage | Same publicness class as Democracy's `group_type=2`. Polymorphic dispatch (§4.7.4) keeps it out of the per-call selector. **Residual**: the Oligarchy public-inputs payload size differs from Democracy and Anarchy (9 scalars vs 5 vs 3) — observable from call-payload analysis. Oligarchy is the most-distinguishable variant on payload size. Same partial-solution as Democracy: structural fix is uniform-shape padding via dummy circuit-side commitments, deferred. |
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
| `DOMAIN_ADMIN_OCCUPANCY` | `Fr::from(5)` | **New.** Admin-tree occupancy commitment: `Poseidon(DOMAIN_ADMIN_OCCUPANCY, fold(admin_bitmap))`. |

§10 Q1 (the cross-circuit domain-tag audit) extends to cover values 4 and 5. All five values must be unique not just across this circuit but across `MembershipCircuit`, `UpdateCircuit`, `DemocracyUpdateCircuit`, and any future circuit family.

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

A new variant on the polymorphic `UpdateCommitmentPublicInputs` enum:

```swift
public enum UpdateCommitmentPublicInputs: Codable, Equatable, Sendable {
    case anarchy(cOld: Data, epochOld: UInt64, cNew: Data)                  // 3 scalars
    case democracy(                                                          // 5 scalars
        cOld: Data, epochOld: UInt64, cNew: Data,
        occupancyCommitmentOld: Data, occupancyCommitmentNew: Data
    )
    case oligarchy(                                                          // 9 scalars
        cOld: Data, epochOld: UInt64, cNew: Data,
        memberOccupancyCommitmentOld: Data, memberOccupancyCommitmentNew: Data,
        adminOccupancyCommitmentOld: Data, adminOccupancyCommitmentNew: Data,
        adminRootOld: Data, adminRootNew: Data    // see Note below: `admin_root` stored separately, updated in lockstep with `commitment`
    )
}
```

Note on `admin_root` storage: the contract continues to store `admin_root` as a separate field (preserved across the v2 redeploy so the existing `get_admin_root` getter keeps working as a read-only query path). **`admin_root` is NOT a derivative of `c_old`** — Poseidon is one-way, so the bundled commitment doesn't recover the unbundled root. Instead, `admin_root` is stored redundantly alongside `commitment` (= `c_old`); both must be updated in lockstep on every successful `update_commitment`.

To make the in-lockstep update verifiable, the wire payload carries `admin_root_old` and `admin_root_new` as **separate fields** (not bundled into `c_old`/`c_new`). The contract on receive: (1) checks `admin_root_old == current.admin_root` (same kind of state-binding as `c_old == current.commitment`), (2) supplies both `admin_root_old` and `admin_root_new` as Groth16 public inputs to the verifier — the circuit binds them into `c_old`/`c_new` via Poseidon and verifies the bundling matches the wire's `c_old`/`c_new` claims, (3) on success writes both `admin_root = admin_root_new` and `commitment = c_new` to storage atomically. The wire-format `oligarchy` variant therefore carries 9 scalars (the 7 from earlier in this section plus `admin_root_old`, `admin_root_new`), not 7. Updated:

```swift
case oligarchy(
    cOld: Data, epochOld: UInt64, cNew: Data,
    memberOccupancyCommitmentOld: Data, memberOccupancyCommitmentNew: Data,
    adminOccupancyCommitmentOld: Data, adminOccupancyCommitmentNew: Data,
    adminRootOld: Data, adminRootNew: Data
)
```

§3.4 entrypoint-selector residual: Oligarchy is now 9 scalars on the wire (vs. Anarchy's 3 and Democracy's 5). The same "uniform-shape padding" structural fix that's deferred for Democracy/Anarchy applies; Oligarchy is the most-distinguishable variant on payload size. §9 risks track this. §4.7.3 R1CS estimate gets a small bump for the additional public-input range checks (~30 constraints; absorbed into the existing ~9060 estimate without changing the order-of-magnitude).

`SEPGroupMemberLeaf.slotIndex: UInt32?` carries forward unchanged (per [Democracy §4.4](democracy-update-testnet-design.md#44-wire--data-model-additions)). Same Codable wire-compat rules; same normative-leaf-hash rule (slotIndex NOT part of the leaf hash).

`BootstrapPayload` (`SEPBootstrapPayload`) for Oligarchy gains:

- `members: [SEPGroupMemberLeaf]` — required; standard member list with member-tree slot indices.
- `admins: [SEPGroupMemberLeaf]?` — required (non-nil) for Oligarchy bootstrap; MUST be nil for other governance types. Each entry's `slotIndex` is the **admin-tree** slot, not the member-tree slot. An admin who is also a member appears in both arrays with potentially different slot indices.
- `adminThresholdNumerator: UInt8?` — required (non-nil) for Oligarchy. Validated `1..=100` per §4.7.6 (parallel to Democracy's `thresholdNumerator`). MUST be nil for other types.
- `memberThresholdNumerator: UInt8?` — MUST be nil for Oligarchy. (Democracy uses this; Oligarchy uses only the admin threshold — the member set has no quorum of its own. The admin set authorizes member changes.)

`SEPSaltResponse.members: [SEPGroupMemberLeaf]?` carries forward; an additional `admins: [SEPGroupMemberLeaf]?` is added for Oligarchy salt responses so the offline-recovery flow ([§4.1.3](democracy-update-testnet-design.md#413-slot-index-back-channel-to-the-joiner)) covers admin-tree state too. Padded to tier-uniform size combining both arrays' max possible footprint.

`SEPGroupStateUpdate` for Oligarchy carries both `members` and `admins`. The receiver applies whichever changed (the §4.7.7 target-tree witness is private to the prover, but post-broadcast the receiver sees both new arrays and diffs against its persisted state to know which changed — same diff is observable to any group member, but not to a chain observer).

### 4.5 Relayer dispatch (inherited)

The polymorphic `update_commitment` entrypoint already covers Anarchy + Democracy + (now) Oligarchy. The relayer doesn't need a per-type dispatch arm — the variant is in the public-inputs payload, the contract reads `current.group_type` from storage to discriminate.

### 4.6 Contract redeploy + admin set + admin tier cap

This design inherits [Democracy §4.6](democracy-update-testnet-design.md#46-contract-redeploy--vk-installation)'s contract redeploy. The Oligarchy entrypoints land in the same redeployed contract as Democracy's — one fresh address covers all three governance types' v2 paths.

`create_oligarchy_group_v2(group_id, member_root, member_occupancy_commitment, admin_root, admin_occupancy_commitment, member_tier, admin_threshold_numerator, …)`. Storage gains, per Oligarchy group:

- `admin_root: BytesN<32>` — Poseidon root of the admin tree. (Existing field, preserved; computation now bundles into `commitment` per §4.7.2.)
- `admin_occupancy_commitment: BytesN<32>` — new field, parallel to `member_occupancy_commitment`.
- `admin_threshold_numerator: u8` — new field, range `1..=100`. Validated at `create_oligarchy_group_v2`.
- `admin_count: u8` — implicit from `popcount(admin_bitmap)` but cached for getter convenience. **Caveat**: caching `admin_count` on chain *as a separate readable field* would partially defeat the §3.5 hiding property — a chain observer reading the cached count gets exact admin-set size. So `admin_count` is NOT stored as a separate field; it's only computable in-circuit. Public getters for "is this group still alive" use `admin_occupancy_commitment ≠ commitment_to_empty_bitmap` instead.

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

Three Poseidon calls instead of Democracy's two. The inner `Poseidon(root_member, root_admin)` packs both roots into a single field element before the existing `Poseidon(combined, epoch)` step. Adds ~300 R1CS constraints over Democracy's `c_old`/`c_new` derivation (one extra Poseidon per side).

Occupancy commitments are exposed as separate public inputs (per the §4.4 enum), one per tree:

```
member_occupancy_commitment = Poseidon(DOMAIN_OCCUPANCY, fold(member_bitmap))
admin_occupancy_commitment  = Poseidon(DOMAIN_ADMIN_OCCUPANCY, fold(admin_bitmap))
```

The admin bitmap (32 bits) packs into a single `Fr` scalar (252-bit capacity), so admin's `fold` is trivial — one scalar in, one Poseidon call out. Member side scales with tier as in Democracy (1 / 2 / 9 scalars).

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
10. **Occupancy commitments** (member + admin, both sides). Two Poseidon calls per side per tree → ~1500 R1CS for tier-1 (member) + ~600 (admin).
11. **`c_old`, `c_new` derivations** (with the bundled-roots formula). ~700 R1CS for two sides × three Poseidon calls each.
12. **Existing constraints** unchanged: signer Merkle openings against `root_admin_old`, ascending leaf-index ordering, etc. ~3500.

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
| Single-leaf-delta dispatcher (`target_tree` witness + non-target-no-change constraints) | ~150 |
| Threshold range check | ~10 |
| Member occupancy commitment Poseidon (2 hashes × 2 sides) | ~1200 |
| Admin occupancy commitment Poseidon (1 hash × 2 sides) | ~600 |
| `c_old`/`c_new` bundled-roots (3 hashes × 2 sides × ~300 each) | ~1800 |
| Existing constraints unchanged | ~3500 |
| **Total tier-1** | **~9060** |

Larger than Democracy's ~6298 — the second tree adds about 50% to the constraint count. Proving time for Oligarchy at tier-1 is roughly 2.5× v1-Anarchy's ~150ms → ~400ms. Still well within budget for an interactive epoch transition.

Tier scaling:
- Tier-0 (member depth 5 / 32 slots): admin overhead unchanged; member overhead drops proportional to slot count. ~7800 constraints.
- Tier-2 (member depth 11 / 2048 slots): member overhead scales 8×. ~21000 constraints.

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

- The same wire-format public-inputs payload (9 scalars: c_old, epoch_old, c_new, member_occ_old, member_occ_new, admin_occ_old, admin_occ_new, admin_root_old, admin_root_new).
- The same proof byte length (192 bytes canonical Groth16).
- The same on-chain operation (`update_commitment` polymorphic entrypoint, same selector).

The only observable signals are the occupancy-commitment values themselves (which change per update) and the bundled `c_new` (which advances per epoch). A chain observer sees occupancy commitments updating but cannot distinguish:

- A member-add update from an admin-promotion update.
- An admin-demotion from a member-remove.
- Any of the above from a "key rotation" (member or admin replacing their own key in place — both bitmaps unchanged but `c_new` shifts due to different leaf hashes after rotation; this is still a single-leaf delta).

**Threat-model limitation (out-of-band caveat).** A chain observer who *also* has out-of-band access to one of the trees' state — e.g., compromised one client's local storage and read its persisted member list — can infer which tree changed by comparing the new commitments against their offline computation of "what the new tree should be if my offline state is correct." This is the same threat profile as Democracy — out-of-band access to a peer's state defeats the on-chain hiding regardless of governance type. The on-chain privacy floor is what this design protects; out-of-band defenses (forward secrecy of local state, tamper-resistant storage, etc.) are layered separately and out of scope here.

### 4.8 Initial admin set, `create_oligarchy_group_v2`

```
create_oligarchy_group_v2(
    group_id,
    member_root, member_occupancy_commitment,
    admin_root, admin_occupancy_commitment,
    member_tier,
    admin_threshold_numerator,  // 1..=100
    initial_proof,              // membership proof from creator against member_root
)
```

The creator constructs `member_root` with themselves at member-slot 0 (tombstone elsewhere) and `admin_root` with themselves at admin-slot 0 (tombstone elsewhere). The `initial_proof` proves the creator knows the secret key behind both leaves (one secret key, used in two trees with different domain tags — see §4.1.1).

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
| A1 | `src/circuit/oligarchy_v2.rs` (new) | ~600 | Two-tree bitmap binding (member + admin); bundled-roots `c_old`/`c_new`; admin-quorum threshold; target-tree dispatcher witness + non-target-no-change constraints; member + admin floors; new domain tags `DOMAIN_ADMIN`, `DOMAIN_ADMIN_OCCUPANCY` declared in `src/circuit/domain_tags.rs` (extends Democracy's). |
| A2 | Tests for A1 | ~300 | Round-trip member-add, admin-promotion, admin-demotion, member-replace; tombstone-collision regression for both trees; bitmap-mismatch attack rejection per tree; threshold sweep on admin-quorum; target-tree-flip attack (witness `target_tree = 1` while presenting member-delta data — must fail); floor-violation rejection (member→0 or admin→0). |
| A3 | `src/prover/mod.rs` `prove_oligarchy_v2` + `verify_oligarchy_v2` | ~200 | Bitmap derivation for both trees, bundled-roots commitment computation, target-tree inference from rosters, admin-signer Merkle path against `admin_root_old`, `QuorumRequired` for `admin_count_old ≥ 3`. |
| A4 | `keyset-oligarchy-dev/{tier0-k1,tier1-k1,tier2-k1}/` (admin tier fixed at Small=32 across all member tiers; `_k1` reflects single-admin-signer single-K). Generated via extended `scripts/generate-democracy-vk-dev.sh` (renamed `scripts/generate-governance-vks-dev.sh`). | (binary VK files) | Three dev VKs (one per member tier; admin tree fixed at depth 5). Fingerprints checked into `keyset-oligarchy-dev/fingerprints-v2.json`. |
| A5 | `Cargo.toml` feature `oligarchy-v2-dev-vks` | ~10 | Off by default. Independent of `democracy-v2-dev-vks` (mainnet build verifies BOTH features are off). |
| A6 | Cross-platform test vectors `docs/cross-platform-test-vectors.json` | ~80 | New `oligarchy_v2` section; reference proofs for member-add, admin-promotion, admin-demotion, member-replace flows. |

**Phase A exit criteria**: `cargo test --features oligarchy-v2-dev-vks circuit::oligarchy_v2` and `cargo test prover::oligarchy_v2_round_trip` both green. R1CS constraint count for tier 1 within 10% of §4.7.3 estimate (~9060). Three dev VKs checked in with fingerprint manifest.

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
| C1 | `contracts/sep-xxxx/src/lib.rs` Oligarchy v2 entrypoints | ~280 | Polymorphic `update_commitment` Oligarchy variant. New storage fields per Oligarchy group: `admin_occupancy_commitment`, `admin_threshold_numerator`. (Existing `admin_root` repurposed; `admin_count` deliberately NOT stored as a separate field.) New `create_oligarchy_group_v2` taking the tuple from §4.8. New `Error::InvalidAdminThreshold` variant. Verifier reads `current.admin_threshold_numerator` from storage. |
| C2 | Contract tests | ~280 | Mirror existing Oligarchy tests against v2. New tests: target-tree dispatcher (member-update + admin-update both succeed against the same VK); type-confusion guard rejects an Oligarchy proof submitted to a Democracy group; `m_member_new == 0 || m_admin_new == 0` rejected per the new floors; threshold-mismatch rejected; admin-tier-cap enforcement (33rd lifetime admin add rejected). |
| C3 | Cargo crate version bump | ~5 | |
| C4 | `scripts/deploy_sep_xxxx_testnet.sh` (extends Democracy's) | (script) | Same fresh contract address as Democracy redeploy (one address covers all governance types). |
| C5 | `scripts/install-governance-vks-testnet.sh` (renamed from democracy-only script; covers all Oligarchy + Democracy VKs in one install) | ~30 | |
| C6 | Smoke test on testnet | (manual) | `create_oligarchy_group_v2` → `update_commitment` (Oligarchy variant, member-add path) → `update_commitment` (admin-promote path) round-trip via stellar CLI. |

Total: ~595 LOC.

### Phase D — Relayer + clients

| Step | Files | LOC | Output |
|---|---|---|---|
| D1 | `relayer/src/handler.rs` Oligarchy variant in the public-inputs translation | ~30 | Polymorphic dispatch already handles the new variant — minor extension to recognize the 7-scalar payload. |
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
- `prove_oligarchy_v2_member_tombstone_collision` and `prove_oligarchy_v2_admin_tombstone_collision` — paired domain-tag-disabled / enabled regression tests for both trees independently. Same shape as Democracy's `prove_democracy_v2_domain_tags_block_tombstone_collision`.
- `prove_oligarchy_v2_bundled_roots_consistency` — verifier recomputes `c_old` from `(root_member, root_admin, epoch_old, salt_old)` and confirms it matches the public-input value.

### 7.2 Cross-platform vectors

`docs/cross-platform-test-vectors.json` adds an `oligarchy_v2` section covering the four operations (member-add, admin-promote, admin-demote, member-replace) with reference proofs and expected occupancy commitments for both trees.

### 7.3 Relayer integration

Polymorphic `update_commitment` already accepts arbitrary `UpdateCommitmentPublicInputs` variants — Oligarchy adds one test asserting the relayer forwards a 7-scalar Oligarchy payload to stellar CLI without re-shaping it.

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

1. iOS creates an Oligarchy group at medium tier with `admin_threshold_numerator = 50`. Verify on Soroban testnet that `current.group_type == 3`, `current.admin_threshold_numerator == 50`, `admin_root` is set, both occupancy commitments are set.
2. iOS sends an invitation to Android.
3. Android accepts. Android sends `SEPMemberJoined`.
4. iOS as the only admin processes the SMJ, runs Oligarchy v2 path, polymorphic `update_commitment` with `target_tree=member` private witness. Verify relayer log shows `function=update_commitment` with 7-scalar Oligarchy variant payload, status 200.
5. iOS local state advances: epoch 1, 2 members (slots 0, 1), 1 admin (slot 0).
6. Android receives broadcast; persists its member-slot index = 1; sees admin set unchanged.
7. iOS chat to Android works; Android chat to iOS works (both members, BLS auth passes).
8. iOS as admin promotes Android: sends `SEPAdminPromoted`. iOS runs Oligarchy v2 path with `target_tree=admin`. Member tree unchanged; admin tree gains Android at admin-slot 1. Verify relayer log: same 7-scalar payload shape, no observable difference from step 4 except the occupancy-commitment values.
9. **Privacy verification**: read the contract storage entry for this group via Soroban testnet RPC. Assert no `admin_count` or `member_count` field is present. Compare the on-chain trace of the two `update_commitment` calls (steps 4 and 8); assert the call payloads have **identical scalar count and byte length**. A chain observer cannot distinguish "added a member" from "promoted an admin" from these calls alone.

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
| Domain-tag collision (now five values: 1–5) | Low | High | §10 Q3 cross-circuit audit covers values 4 and 5 in addition to 1/2/3. |
| Admin set leaks via timing of admin-only operations (an admin promotion is observable as "an update happened" — chain timestamps reveal cadence) | High (intrinsic to public ledger) | Low (cadence is the same residual as Democracy; Oligarchy doesn't add new cadence-class signals) | Acknowledged. Same as Democracy's epoch-counter residual. |
| Cross-tree-update demand (operators want to combine member + admin changes atomically) | Medium | Medium (UX cost — two updates instead of one, one extra epoch) | §4.10 / §11. Documented limitation; not blocking. |

---

## 10. Open Questions

### 10.1 Phase-A blockers (must ratify before A1 opens)

1. **Domain tag values (extends [Democracy §10.1 Q1](democracy-update-testnet-design.md#101-phase-a-blockers-must-ratify-before-a1-opens)).** Recommended values: `DOMAIN_MEMBER=1`, `DOMAIN_TOMBSTONE=2`, `DOMAIN_OCCUPANCY=3`, `DOMAIN_ADMIN=4`, `DOMAIN_ADMIN_OCCUPANCY=5`. Audit pass: same 1-hour grep against current `main` to confirm no collision; codified in `src/circuit/domain_tags.rs`.

2. **Admin tier cap value.** Recommended: 32 (Small tier, depth=5). Lifetime admin appointments capped at 32 per group. Operators warned. Larger admin trees are §11 follow-up if a real use case demands.

3. **Admin signer K-bound for tier-2.** Same as Democracy §10.1 Q2 — k_max for the admin signer slots. Recommended: k_max=1 for v2 single-admin-signer subset (multi-admin is Phase F). The Phase F redesign for K-of-N admin quorum will need a different VK.

### 10.2 Other open questions

4. **Should member tree and admin tree share a `target_tree`-typed dispatcher, or always emit a uniform-shape proof regardless of which tree changed?** Current design (uniform-shape via `target_tree` private witness) is the correct call for §3.5. Open: when Phase F's multi-admin-quorum redesign lands, does the dispatcher still hold? Needs a re-check at the start of Phase F.

5. **What happens when the only admin is also the only member, and they try to remove themselves?** Either floor (member or admin = 0) trips at the first removal step. The admin can't remove themselves and promote a new admin in one update (cross-tree changes are out of scope per §4.10). Workaround requires growing both trees first; the minimal sequence:
   1. Add a second member (1 → 2 members). Single-signer K=1 from the original admin authorizes (per §4.2 default-50 threshold table).
   2. Promote that member to admin (1 → 2 admins). Single-signer K=1 from the original admin again.
   3. Original creator removes self from member tree (2 → 1 members). Member floor `≥1` satisfied. Admin signer is now either the original creator OR the new admin — either suffices for K=1 since admin_count_old=2 at this step.
   4. Original creator demotes self from admin tree (2 → 1 admins). Authorized by the new (remaining) admin. K=1 from `admin_count_old=2`.

   **Four updates, four epochs.** (An earlier draft cited a three-update workaround; that version skipped step 1 and immediately tried "remove self from member tree" from a 1-member starting state — which would violate the member floor since `m_new` drops to 0. Corrected here.) UX cost is high; clients SHOULD warn when the user is about to commit to becoming a member-and-admin singleton group, since exit cost is steep.

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
