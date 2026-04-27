# Democracy Group Updates — Testnet-Gated Implementation Design

**Date:** 2026-04-26
**Status:** Draft (Proposal — pre-implementation)
**Author:** Onym contributors
**Version:** 0.5.1 — addresses @releaseng review on v0.5: §4.2 single-signer cutoff generalized for non-default thresholds (table per `threshold_numerator`); §4.6 Large-tier `k_max=256` cap spelled out (was a §10 dangling reference); §4.7.6 tie semantics pinned (`100·K ≥ T·m_old` is non-strict — ties pass at threshold=50, strict majority requires 51); §4.7.6 IC-vector public-input ordering pinned to `docs/soroban-contract-test-vectors.json`; §7.1 floor test corrected to 2→1, threshold-tie + strict-majority cases added, contract-side companion tests for chain-mismatch and tie added under §6 step C2; §7.4 BootstrapPayload threshold-validation tests added (MissingThreshold, out-of-range, default-50); §7.5 manual e2e gains a non-default-threshold sub-flow; §9 risks gain rows for IC-ordering drift and storage-layout drift, both backed by `soroban-contract-test-vectors.json`. Companion test-vectors file checked in at commit 526d7a0 (verified accurate against design).
**Supersedes:** (none — first iteration)
**Related:**
- [`group-governance-types-design.md`](group-governance-types-design.md) — the parent design that introduces `groupType ∈ {Anarchy, 1v1, Democracy, Oligarchy}`
- [`democracy-circuit-ceremony.md`](democracy-circuit-ceremony.md) — Phase 2 trusted-setup plan (gates mainnet)
- [`update-circuit-binding-design.md`](update-circuit-binding-design.md) — analogous binding work for the Anarchy `update_commitment` flow
- `contracts/sep-xxxx/src/lib.rs:1318-1338` (`update_commitment`, Anarchy-only) and `:1440-1545` (`update_commitment_democracy`, the target entrypoint). `sep-xxxx` is the actual current package name in the repo, pending SEP number assignment — not a placeholder.
- `src/circuit/democracy.rs` — the dev-only `DemocracyUpdateCircuit`
- `keyset-democracy-dev/` — placeholder dev VKs (tier0-k32, tier1-k256)

---

## 1. Background

Democracy groups (`groupType = 2`) are created on-chain via `create_group_v2` and accepted by the contract today (testnet contract `CC6NUUKG25RSFI6D57HISDQ4HRBLXFAUC3GFVZIACHQX3NLRPYTRWWKE`, observed 2026-04-26). What does **not** work is the post-creation epoch transition. When a peer joins a Democracy group, both clients drive the membership-add through `applyStateUpdate` → `publishMemberUpdate` → `ContractClient.updateCommitment`, which calls `update_commitment` on-chain. That entrypoint is hard-coded to `group_type == 0` (Anarchy):

```rust
// contracts/sep-xxxx/src/lib.rs:1334-1338
match current.group_type {
    0 => {}
    1 => return Err(Error::OneOnOneImmutable),
    _ => return Err(Error::UnknownGroupType),  // <-- Democracy lands here
}
```

The Soroban contract surfaces this as `Error::UnknownGroupType` (#18). The relayer returns HTTP 502 with the diagnostic event, the chain publish is rejected, and the iOS code path at `clients/ios/StellarChat/StellarChat/StellarChatApp.swift:1027-1034` returns `chainRejected`. Local state is not advanced; subsequent chat events from the new member fail BLS sender authentication on the existing peer (the new member is not in the local `currentMembers` list), and chat is silently broken.

This snapshot of the broken behavior is preserved here as the **historical bug-of-record** — the motivation that triggered the design. Phase C of §6 redeploys the contract with a polymorphic `update_commitment` (no `_democracy` arm exists) so the broken `match` arm above no longer runs against any post-Phase-C client. The old testnet contract `CC6N…RWWKE` retains the broken arm forever, but no Phase-D-or-later client points at it.

The contract already exposes a Democracy-specific entrypoint:

```rust
// contracts/sep-xxxx/src/lib.rs:1440
pub fn update_commitment_democracy(
    env: Env,
    group_id: BytesN<32>,
    proof: Groth16Proof,
    public_inputs: DemocracyUpdatePublicInputs,
) -> Result<(), Error>
```

with extra public inputs `member_count_old` and `member_count_new`, and a per-`(tier, group_type)` verification key loaded via `load_update_vk_by_type(tier, 2)`. This function exists, has unit-test coverage at `:4772-4883`, and is wired through the contract's proof-replay and tier-capacity checks.

What's missing is everything **between** the contract and the user: a Rust prover function, an FFI export, Swift and Kotlin bindings, a relayer dispatch case, and the client-side groupType branch.

`swift-mls/Sources/SwiftMLS/ContractClient.swift:80-83` flags this gap explicitly:

> Non-Anarchy types currently reject later `update_commitment` calls until the per-type VK ceremonies land.

This v0.4 design **does not** wire up the existing `update_commitment_democracy` (which exposes `member_count_*` as public inputs and would put Democracy below Anarchy's privacy floor — see §3.4). Instead the design specifies a v2 circuit (occupancy commitment, hidden counts) and a fresh contract deploy that exposes a single polymorphic `update_commitment` entrypoint. The existing testnet contract `CC6N…RWWKE` is abandoned. Rationale and rollout in §6 and §8.

---

## 2. Problem

A Democracy group becomes unusable as soon as its second member joins. The on-chain commitment cannot advance, and the local state stays at epoch 0 with one member. Any chat from the new member is rejected as non-member by the creator's transport (`NostrMessageTransport.handleIncomingEvent`'s BLS check at the receive side), and there is no recovery path short of recreating the group as Anarchy.

This document specifies the work needed to make Democracy groups functional **and** preserve Anarchy's privacy floor (no per-epoch member count, trajectory, or churn visible on chain). The cryptography is not yet ceremony-blessed — all three tiers' VKs are at the dev-key stage — so this design accepts dev-key deployment to testnet, gated by §4.3 to fail closed if the dev path ever runs against a real-ceremony contract.

---

## 3. Constraints

Three hard constraints shape the scope.

### 3.1 The circuit is dev-only

`src/circuit/democracy.rs:50-55`:

> The constraint body below is a first-draft implementation intended for local testing and property verification. It has NOT been reviewed for R1CS-level soundness by a second set of ZK-literate eyes, and the Phase 2 trusted-setup ceremony MUST NOT be run against it until that review concludes.

A proof generated by this circuit, verified against `keyset-democracy-dev/` VKs, asserts only "the prover ran this dev circuit." It does not yet carry a soundness guarantee that the verifier can trust against a malicious prover. This is acceptable for testnet smoke tests by Onym contributors. It is not acceptable for any value-bearing usage.

### 3.2 The Phase 2 ceremony has not run

`keyset-democracy-dev/vk-democracy-DEV-{0,1}-*.json` are derived from a local development setup, not an MPC ceremony. There is no toxic-waste guarantee. `scripts/install-democracy-vks-testnet.sh` exists to deploy these dev VKs to the testnet contract, and a similar script for production must wait until [`democracy-circuit-ceremony.md`](democracy-circuit-ceremony.md) Phase 2 completes.

### 3.3 Single-leaf delta requires a stable slot-index convention

The Democracy circuit (constraint #7 in `src/circuit/democracy.rs`) accepts proofs only when **exactly one Merkle leaf position** changed between `root_old` and `root_new`. The existing client member model (`SEPGroupMemberLeaf`) carries `(public_key_compressed, leaf_hash)` and orders by `compressed_public_key_bytes` (`canonicalize_members`). Inserting a new member into a public-key-sorted tree typically shifts every position after the insertion point — multiple leaves change, single-leaf delta fails.

Without resolving this, the circuit only handles `Replace` operations where the public key is preserved (which is what the test fixture `build_replace_scenario` uses). Real `Insert`/`Remove` operations on real members do not satisfy the constraint.

This is the gating design question. §4.1 proposes a resolution.

### 3.4 No member-metadata leakage is a P0 requirement

Anarchy's `update_commitment` exposes `(c_old, epoch_old, c_new)` as public inputs and reveals nothing about member count, identity, or trajectory. Democracy MUST match this privacy floor — every member-related metadata signal that Anarchy already hides, Democracy hides too. Concretely:

| Metadata | Anarchy hides | Democracy MUST hide |
|---|---|---|
| Exact member count at any epoch | ✓ | ✓ |
| Member-count delta per epoch (joins, removes, replaces distinguishable) | ✓ | ✓ |
| Lifetime cumulative joins | ✓ | ✓ |
| Member identities | ✓ | ✓ (already true via Poseidon-hashed leaves) |

The naive Democracy update path — using `update_commitment_democracy`'s `member_count_old`/`member_count_new` as public inputs — fails the first three rows. An earlier draft of this design (v0.2/v0.3) proposed shipping that path on testnet behind a feature flag and tackling the leak in a follow-up. **That approach is now rejected.** Per project direction (no live users, ceremony for all three tiers still at the Phase 1 / dev-key stage, contract redeploy is acceptable), the cost of shipping count-public first and reworking later — extra ceremony, extra wire-format migration, extra reviewer cycles — exceeds the cost of going straight to a hidden-counts design.

**Decision: this design ships the v2 circuit (occupancy commitment, hidden counts) directly. There is no count-public intermediate stage.** The contract is redeployed at a fresh address with v2-only entrypoints; the existing testnet contract `CC6N…RWWKE` is abandoned (no migration, no users to coordinate). Section §4.7 specifies the occupancy-commitment circuit; §6 lays out the phased rollout. The dev-key ceremony covers all three tiers (Small / Medium / Large) before any client lands.

#### What this design does NOT hide (acknowledged residual leaks)

Strict "no metadata" is impossible — Soroban itself carries some signals. The honest scope of this design's privacy claim is "no member-related metadata leaks." The following are acknowledged residuals, with mitigations called out where reasonable:

| Residual | Why it leaks | Mitigation in this design |
|---|---|---|
| Group exists at all | Chain has a `CommitmentEntry` storage entry per group | None possible — protocol level |
| Epoch counter advances | Chain stores `current.epoch`, observable | None — intrinsic to "newest state wins" |
| Tier (chosen at create time, fixed) | `create_group_v3` argument | Choose tier conservatively at create; tier upgrades are a separate concern (next row) |
| Democracy threshold (chosen at create time, fixed) per §4.7.6 | `create_group_v3` argument; stored in contract; readable via storage inspection | Same publicness class as `tier` and `group_type`. Not exposed in per-update call payload (the contract reads it from storage). Acknowledged residual: a chain observer who reads contract storage learns the threshold value of a group. No per-update leak. |
| Tier upgrades visible | Different VK / different storage layout | **Partial**: tombstone permanence (§4.1.2) means tier capacity is the **lifetime** join cap, so operators size for the 95th-percentile join count and tier upgrades become rare. Eliminating entirely requires a universal-circuit redesign — out of scope. |
| Group type from per-call entrypoint selector | `update_commitment_democracy` selector visible in tx | **Partially solved**: §4.7.4 polymorphic `update_commitment` dispatches all governance types through one entrypoint, eliminating the *selector* leak. **Residual**: the public-inputs payload size still differs by variant — Anarchy is 3 scalars, Democracy is 5 scalars (`c_old`, `epoch_old`, `c_new`, `occupancy_commitment_old`, `occupancy_commitment_new`), Oligarchy will add an `admin_root` scalar. A chain observer that doesn't witness `create_group_v3` can read `group_type` either from contract storage (one storage-load operation per group) OR derive it from the public-inputs scalar count on any update call. Same fundamental signal, two extraction paths. Eliminating both would require padding all variants to a uniform N-scalar shape with circuit-side dummy commitments — non-trivial circuit-side change, deferred. The `group_type` itself is already stored in the contract from `create_group_v3`, so a determined observer always has at least one extraction path; the polymorphic-dispatch fix narrows the convenience of extraction (no longer in the call selector indexed by block explorers by default), not the information itself. |
| `slotIndex` reveals join order to current members | §4.4 wire-format addition | Lateral: current members already see each other. Acknowledged for future pseudonymous-author features. |
| `SEPSaltResponse` size varies with member-list length | §4.4 wire addition | **Solved**: pad to tier-uniform size (see §4.7.5). |
| Update cadence visible via chain timestamps | Inherent to public ledger | None — same as Anarchy |
| Fee-paying account visible | Stellar account model | Mitigated by relayer (existing infrastructure) |

The only material residual after this design lands is "tier upgrades visible," which is a coarse signal (one bit per upgrade) compared to "exact count every epoch" (~`log2(tier_capacity)` bits per epoch). Operators are advised to over-size at create time so upgrades are infrequent or absent.

---

## 4. Design

### 4.1 Slot-index convention (resolves §3.3)

Democracy groups switch from public-key-sorted Merkle ordering to **stable slot-index Merkle ordering**:

- Each member is assigned a slot index `slot ∈ [0, 2^depth)` at the moment they join the group.
- The first member is at slot 0. Subsequent joins take the **lowest never-used slot**. "Never-used" excludes both currently-active slots and tombstones — once a slot has been occupied by any member, it is permanently retired even if that member later leaves.
- Removals leave a tombstone (canonical empty leaf, defined in §4.1.1 below); the slot index is **not** reassigned to a future joiner.
- The Merkle tree at any epoch is over the slot-indexed array `[leaf_0, leaf_1, ..., leaf_{2^depth - 1}]`, where each entry is either a member's domain-tagged leaf hash or the tombstone constant.
- `member_count_old` / `member_count_new` (the new public inputs) are the count of non-tombstone slots, **not** the highest slot index.

#### 4.1.1 Domain separation: members vs. tombstones (normative)

Member leaves and tombstones MUST occupy disjoint subsets of the leaf-hash range. Without domain separation, a Poseidon collision between a real member's secret-derived leaf and the tombstone constant would let a removed member's old leaf masquerade as a tombstone (or a freshly removed slot masquerade as a future member). Either direction is a soundness break under §4.1's "single-leaf delta" property — the circuit cannot distinguish "this slot has been emptied" from "this slot still holds the same member" if the same hash can mean both.

The single source of disjointness is a fixed domain tag prepended to every Poseidon input:

```
member_leaf(sk)  = Poseidon(DOMAIN_MEMBER, Poseidon(sk))
tombstone        = Poseidon(DOMAIN_TOMBSTONE, 0)
```

With `DOMAIN_MEMBER ≠ DOMAIN_TOMBSTONE` (two distinct field elements pinned at the contract level — proposed values: `Fr::from(1)` and `Fr::from(2)`, decided alongside the Phase 2 ceremony spec). Disjointness then follows from Poseidon's collision resistance over the (domain, …) input shape — no probabilistic argument over secret-key range required.

The dev circuit at `src/circuit/democracy.rs` does not currently apply this domain separation. The implementation work in §6 includes a circuit-level change (one extra Poseidon call per leaf, two new constants) before any client wiring is sound. **This is normative**: the spec does not ship without the domain tag, and the test in §7.1 includes a `prove_democracy_rejects_tombstone_collision` case to enforce it.

#### 4.1.2 Why permanent tombstones (not just epoch-bound replay protection)

Old proofs are bound to `(c_old, epoch_old)` and would already fail the contract's `epoch_old == current.epoch` check at any future epoch — cross-epoch replay alone doesn't motivate permanent tombstones. The tombstone-permanence rule has two non-replay justifications:

1. **Same-epoch concurrent updates.** Two acceptors running in parallel against the same epoch can both produce valid proofs against the same `c_old`. Stellar serializes them — only one wins on chain. Without tombstones, the loser's local computation could have placed the new member at a slot that is now a removed-member tombstone in the winning state — slot collision, distinct from epoch collision. Tombstones prevent this by making "empty slot that was previously occupied" structurally distinct from "slot that has never been touched," so the loser's offline rollback (§4.1.3) lands on a deterministic re-assignment.
2. **Stable slot ↔ identity invariant.** Clients persist `(memberPubkey, slotIndex)` pairs. If a removed member's slot were reassigned to a new member, every client's local mapping would have to be invalidated/migrated at exactly the right epoch, requiring synchronous purge-then-reassign coordination. Permanent tombstones let the slot map grow monotonically *within the lifetime of a group*: clients only need to learn new mappings, never forget old ones. **Caveat**: §10 Q1 leaves open whether a `groupSecret`-reuse reset (deactivate + recreate) constitutes a fresh group from the slot-map perspective. If it does (the recommendation in §10 Q1), each reset is a clean slate and the "monotonic, never forget" property holds *per group lifetime* — an explicit reset event is the only legitimate path to forgetting old slot indices. If it doesn't (slots persist across resets), this trade-off needs revisiting.

Trade-offs:

- **Slot exhaustion**: because tombstones are permanent, the `2^depth` ceiling is reached at **cumulative joins** (every member who has ever joined the group, regardless of whether they're still active). For the planned tiers (Small=32, Medium=256, Large=2048), a Democracy group with high churn could exhaust its slot space well before the apparent active-member cap. Mitigation: documentation must explicitly call out that the tier ceiling for Democracy refers to lifetime joins, not peak membership. Operators selecting a tier for a Democracy group must size for total expected churn, not steady-state size. (For Anarchy this stays a peak-membership cap, since Anarchy doesn't use slot indices.)
- **Wire format change**: `SEPGroupMemberLeaf` gains an optional `slot_index: u32?` field. `BootstrapPayload` must carry per-member slot indices for Democracy groups. State updates (`SEPGroupStateUpdate`) likewise. Cross-platform test vectors (`docs/cross-platform-test-vectors.json`) need a new section for Democracy member ordering. Codable parsers tolerate the missing field (so Anarchy/1v1/Oligarchy messages remain unchanged), but a Democracy state update with any member missing `slotIndex` is rejected on receive — there is no implicit "infer from sort order" fallback because that would defeat the single-leaf-delta property. See §4.4 for the exact compatibility rules.
- **Compatibility with Anarchy**: Anarchy keeps public-key-sorted ordering (the existing `UpdateCircuit` doesn't have the single-leaf constraint and is fine with multi-position deltas). The slot convention is Democracy-only, gated on `group.groupType == .democracy`.

Implementation note: slot indices are determined by the **acceptor** of a join (the proposer who lands the chain update). The bootstrap invitation does NOT carry the joining member's future slot index — that's assigned when their `SEPMemberJoined` is processed by an existing member who then runs the chain update.

#### 4.1.3 Slot-index back-channel to the joiner

The acceptor must communicate the assigned slot back to the joiner before the joiner constructs any subsequent `SEPMemberJoined`/state-update broadcast, otherwise the two sides will compute different `root_new` next epoch (the joiner places themselves in a different slot than the acceptor did). The mechanism reuses the existing `SEPGroupStateUpdate` broadcast that already follows the chain publish:

1. Acceptor processes `SEPMemberJoined`, assigns slot `s` to joiner, runs the democracy chain update, broadcasts `SEPGroupStateUpdate` over the group's hidden topic.
2. The state update payload includes the full new member list as `[SEPGroupMemberLeaf]`, each carrying its `slotIndex` (acceptor's view of the canonical assignment).
3. Joiner receives the broadcast, finds its own `publicKeyCompressed` in the member list, extracts and persists `slotIndex`. From this point the joiner uses that slot for any proof it generates.
4. Until the joiner has received and persisted its slot, it MUST NOT initiate a chain update of its own (would race with the acceptor's). The client surfaces this as the existing "syncing with blockchain" UI lock from PR #145.

If the state update broadcast is missed (e.g., joiner is offline when the acceptor publishes), the joiner's `SEPSaltRequest` flow on the next received chat event triggers the acceptor (or any other peer with the latest state) to reply. `SEPSaltResponse` therefore carries the latest member list with slot indices — small extension covered in §4.4.

**Residual case: acceptor crashes after chain publish but before broadcast.** If the acceptor lands the chain update (chain `root_new` recorded) and then crashes before any peer receives the broadcast, AND the joiner is offline at the time, the chain has advanced but no in-network peer holds the slot map. The joiner's salt-request fallback queries peers who also missed the broadcast and cannot reconstruct the assignment from partial state. Recovery options that work in this case: (a) the acceptor restarts and re-broadcasts on next-launch (acceptor's local state is persisted before chain submit, so it can detect "I committed this epoch but did I broadcast?" via a small "last broadcast epoch" marker); (b) any peer with `verify_membership` access can re-derive the slot map by reading the chain's `member_count_new` and the member list from the latest persisted bootstrap, then assigning slots in canonical order — since at this point the chain is definitive and peers have to converge on it anyway. Acknowledged as a testnet-acceptable residual case; (a) is the implemented path, (b) is documented as a manual recovery procedure for support.

#### 4.1.4 Concurrent-acceptor race (two existing members process the same `SEPMemberJoined`)

§4.1.3 covers the joiner-vs-acceptor race. A separate race exists when two existing members both observe the same `SEPMemberJoined` simultaneously and both try to publish the chain update with their own slot assignment. Stellar serializes the publishes — only one transaction lands; the other returns `chainRejected` because `c_old` no longer matches the on-chain commitment.

**The losing acceptor's local state is the issue.** By the time the chain publish fails, the loser has:

- Computed and persisted a candidate `(members, epoch+1, salt+1)` locally.
- Possibly broadcast its own `SEPGroupStateUpdate` (if the implementation broadcasts before chain confirmation, which the design above forbids — see below).
- Signed a slot assignment that the network now disagrees with.

Required client behavior to make this safe:

1. **Broadcast strictly after chain confirmation.** Loser's `SEPGroupStateUpdate` MUST NOT be broadcast until the chain transaction has succeeded. The existing `awaitingChainConfirmation` state in `pendingTransitions` (PR #145) already holds the broadcast back; democracy keeps this contract. (Mirror of §4.1.3's "joiner must not initiate chain update until slot assigned" — same principle, different actor.)
2. **Rollback on `chainRejected`.** Loser receives `chainRejected` from the relayer, restores its local state to the pre-candidate baseline (the existing `chainBaseline` map already tracks this for Anarchy; democracy reuses it), and waits for the winning acceptor's broadcast to learn the new slot map.
3. **Re-converge on the winning slot assignment.** When the winning broadcast arrives, the loser updates its persisted member list to include the winning slot indices. The loser's own outbox of pending operations (none in this iteration; placeholder for future multi-signer flow) is reordered against the new state.

The rollback path is a §7.4 integration test (`DemocracyConcurrentAcceptorRollbackTests` on iOS, parallel on Android): two `OnChainService` mocks driving the same `applyStateUpdate` for the same `SEPMemberJoined`, only one allowed to win. Assert: loser local epoch reverts to baseline, loser persists the winner's slot assignment, no double-counted member.

### 4.2 Single-signer subset (multi-signer quorum is future work)

The Democracy circuit accepts up to `k_max` signer witnesses. Constraint #4 requires `100·K ≥ threshold_numerator · member_count_old` (per §4.7.6's parameterized formulation; recovered to the original `2·K ≥ m_old` when `threshold_numerator = 50`). For a single signer (`K = 1`), this collapses to `m_old ≤ 100 / threshold_numerator`. Coordinating a multi-signer proof requires a quorum-collection UX (vote, gather signatures, aggregate proof) that does not yet exist on the clients.

Scope for this iteration: implement only **single-signer democracy proofs**. The prover errors with `Error::QuorumRequired` (a new error type) when `min_K(threshold_numerator, m_old) > 1`, where `min_K = ⌈threshold_numerator · m_old / 100⌉`.

The single-signer-supported size depends on the group's chosen threshold:

| `threshold_numerator` | Max `m_old` for K=1 | Covers transitions |
|---|---|---|
| 50 (default — simple majority, ties pass per §4.7.6) | 2 | 1→2, 2→3 |
| 51 (strict majority) | 1 | 1→2 only |
| 67 (two-thirds supermajority) | 1 | 1→2 only |
| 75 | 1 | 1→2 only |
| 100 (unanimous) | 1 | 1→2 only |

The bootstrap case (1→2) works for any `threshold_numerator ≤ 100`. The 2→3 case works only when the group is on default 50%. Groups created with stricter thresholds therefore hit `QuorumRequired` at the second-member-add boundary, with a UX message routing the user to the (not-yet-built) multi-signer flow.

Concretely:

- The FFI prover input takes one secret key, not a vector.
- The proof is generated with `K = 1` active signer slot; the remaining `k_max - 1` slots are zero-padded.
- The prover errors with `Error::QuorumRequired` when `⌈threshold_numerator · m_old / 100⌉ > 1`. This computation is included in the prover's preflight; no malformed proof is ever generated.

For the user's motivating case (default 50% threshold, 1→2 bootstrap), this covers:

- 1 → 2 (group bootstrap; the user's broken case)
- 2 → 3 (one peer adding a third) — only at default `threshold_numerator = 50`

It does not cover `≥3` member transitions. Those require quorum collection, tracked as follow-up work in §11.

#### 4.2.1 Circuit invariants vs. contract invariants (what the proof does and does NOT prove)

The Democracy proof, taken in isolation against `update_commitment_democracy`'s VK, asserts a strict subset of the safety properties the system relies on. Several rules are enforced **only by the contract**, not by the circuit. Today this is sound because exactly one consumer (the contract entrypoint at `lib.rs:1440`) verifies these proofs. The moment a second consumer exists — a second contract revision sharing the VK, a different entrypoint that loads the same VK, a relayer-side optimistic check, an off-chain audit tool — those contract-only invariants disappear from the trust boundary.

To make the boundary explicit, this table enumerates each safety-relevant invariant and which side enforces it. Future consumers of the same VK MUST re-enforce every contract-only row before trusting the proof.

**Note on revisions for v2.** This table reflects the v2 circuit (`democracy_v2.rs`, §4.7). Two changes vs. the v1 (count-public) circuit's invariants:

- `m_new ≥ 2` (Democracy floor) and `m_new ≤ tier_capacity` (tier ceiling) move **into** the circuit (count is a popcount of the bitmap private witness), eliminating the v1 case where the circuit accepted proofs the contract rejected.
- `member_count_old == current.member_count` is replaced by `occupancy_commitment_old == current.occupancy_commitment` — same trust property, hidden value.

**Constraint numbering convention.** Every "circuit #N" reference in the table below uses the **v2 circuit's** numbering (`src/circuit/democracy_v2.rs`), which is the same scheme as the v1 (`democracy.rs`) circuit for #0–#10 because v2 inherits v1's constraint ordering and only adds new constraints (bitmap-to-leaf binding, popcount, occupancy-commitment Poseidon, in-circuit floor) at the end of the constraint list. So a reader cross-referencing v1 source code finds the same #0–#10 in the same order; the v2-specific rows in the table cite "circuit, new in v2" rather than a number to make the addition unambiguous. References to `lib.rs` line numbers point at the **current** contract source (the broken-Anarchy-only version on the existing testnet contract); after Phase C those line numbers shift to the v2-redeployed contract's source, but the table is meant to be read against either since the *invariant content* is what's stable.

| Invariant | Circuit-enforced (v2) | Contract-enforced (v2) | Reference |
|---|---|---|---|
| `c_old = Poseidon(Poseidon(root_old, epoch_old), salt_old)` | ✓ | (binding via public input) | `democracy_v2.rs` constraint #9 |
| `c_new = Poseidon(Poseidon(root_new, epoch_old + 1), salt_new)` | ✓ | (binding via public input) | `democracy_v2.rs` constraint #10 |
| `epoch_new = epoch_old + 1` | ✓ (via `c_new` derivation) | also re-checked | circuit #10; contract |
| At least one signer's secret key opens to a leaf in `root_old` | ✓ | — | circuit #1 |
| `K ≥ 1` (no zero-opening Commits) | ✓ (slot-0 active) | — | circuit #0 |
| `2·K ≥ popcount(bitmap_old)` (quorum threshold) | ✓ | — | circuit #4 (was public input, now witness from popcount) |
| Active signers strictly ascending leaf indices | ✓ | — | circuit #2, #3 |
| `\|popcount(bitmap_new) − popcount(bitmap_old)\| ≤ 1` (single-leaf delta) | ✓ | — | circuit #6–#8 |
| `popcount(bitmap_new) ≥ 2` (Democracy floor) | ✓ **(moved from contract)** | — | circuit, new in v2 |
| `popcount(bitmap_new) ≤ tier_capacity` | ✓ **(implicit in bitmap length)** | — | circuit, new in v2 |
| Bitmap-to-leaf binding (`bitmap[i] = 1 ⟺ leaf[i] is domain-tagged member`) | ✓ | — | circuit, new in v2 |
| `occupancy_commitment_*` = `Poseidon(DOMAIN_OCCUPANCY, fold(bitmap_*))` | ✓ | (binding via public input) | circuit, new in v2 |
| Tombstone domain separation (§4.1.1) | ✓ | — | circuit, normative |
| `c_old == current.commitment` (replay against stale state) | — | ✓ | contract |
| `epoch_old == current.epoch` | — | ✓ | contract |
| `occupancy_commitment_old == current.occupancy_commitment` (binds bitmap to chain) | — | ✓ **(replaces v1's count binding)** | contract |
| `c_new` is canonical Fr encoding | — | ✓ | contract |
| `occupancy_commitment_new` is canonical Fr encoding | — | ✓ | contract |
| Proof is fresh (replay-window check) | — | ✓ | contract |
| Claimed `group_type` (in `UpdateCommitmentPublicInputs` discriminant) matches `current.group_type` | — | ✓ | contract, §4.7.4 |

Two consequences worth surfacing in the prover documentation:

1. **The current-state binding is contract-only.** The circuit verifies the *transition* `(c_old, occ_old) → (c_new, occ_new)` but not that those values match the chain's current state. A relayer that pre-validates a proof against an in-memory cached `current` rather than a fresh chain read can be tricked into accepting a proof for a state that's already been advanced past. The contract's binding checks (rows above) are the only authoritative state-binding step.
2. **All count-derived safety checks are in-circuit.** Unlike v1 (where the `m_new ≥ 2` floor was contract-only), the v2 circuit accepts only proofs the contract will also accept. Any future consumer of the same VK can therefore trust the floor without re-imposing it. This is a strict improvement over v1's split enforcement.

The §6 implementation includes a `// CIRCUIT-ONLY:` / `// CONTRACT-ONLY:` comment convention on each invariant assertion in the prover and contract dispatch code so this table stays grep-able as the implementation evolves.

### 4.3 Testnet-gating mechanism

Two layers of defense in depth, so Democracy chain updates cannot accidentally ship to mainnet:

**Layer 1 — Build flag.** `Cargo.toml` adds a feature `democracy-v2-dev-vks` (off by default). The democracy proof FFI export `sep_generate_democracy_update_proof_v2`, the Swift `RustBridge.generateDemocracyUpdateProofV2`, the Kotlin equivalent, and the Democracy variant handling in the polymorphic `ContractClient.updateCommitment` are all gated on this feature. Mainnet release builds (`make release`, `gh workflow run Release`) explicitly do NOT enable the feature; CI assertion test verifies the feature is off in release configs. Testnet/dev builds enable it.

**Layer 2 — VK fingerprint check at runtime.** When a client is about to publish a Democracy update via the polymorphic `ContractClient.updateCommitment`, it first reads the on-chain VK at `(tier, group_type=2)` from the redeployed contract and compares its hash to a hardcoded list of known dev VK fingerprints (§8.3). If the on-chain VK does not match any fingerprint in the list (i.e., a real ceremony VK is now installed), the dev path refuses to run and surfaces a "ceremony complete — release the production client" error. This prevents the (theoretically possible) future scenario where a dev build is run against a mainnet-equivalent contract — the dev client refuses to act because its fingerprints don't match.

**Caching policy.** The fingerprint comparison fetches the on-chain VK at `(tier, group_type=2)` on **every** democracy update publish — no cache. Testnet VK rotations during dev iteration are flagged as `High` likelihood in the §9 risk table, and a stale cached fingerprint would silently bypass the safeguard (a client built against an older fingerprint list would happily publish against a newly-rotated dev VK because its cache says "matches"). The on-chain read adds one Soroban RPC round-trip per democracy update; given the new path is already a chain-publishing operation costing seconds, the additional latency is negligible. If we later add a session-scoped cache for performance, it must be invalidated on any publish failure that returns an error code consistent with VK mismatch.

**Fingerprint list lives in code, with a release coupling.** The hardcoded fingerprint allowlist sits in the client (Swift constants, Kotlin constants — same content, generated from a single source file in `keyset-democracy-dev/`). Each testnet VK rotation therefore requires a client release that ships the new fingerprint. This is intentional friction: it prevents an operator from rotating the VK silently and having every existing field client suddenly trust the new key. A signed remote config could in principle relax this coupling, but adds infrastructure (signing key management, fetch path, fail-closed semantics on fetch failure) that costs more than the rotation cadence justifies for testnet-only use. The release-process doc must list "regenerate `democracy-vk-fingerprints.{swift,kt}` and bump the patch version" as a step on every dev VK rotation. See §8 for the exact ordering.

Layer 2 is the load-bearing one. Layer 1 is an ergonomic bound on which builds even compile the path.

### 4.4 Wire / data-model additions

Two new public-input shapes (the discriminated union from §4.7.4) and one extension to the existing member-leaf:

```swift
// swift-mls/Sources/SwiftMLS/Types.swift
public enum UpdateCommitmentPublicInputs: Codable, Equatable, Sendable {
    case anarchy(cOld: Data, epochOld: UInt64, cNew: Data)
    case democracy(
        cOld: Data,
        epochOld: UInt64,
        cNew: Data,
        occupancyCommitmentOld: Data,   // 32 bytes BE — Poseidon over packed bitmap
        occupancyCommitmentNew: Data    // 32 bytes BE
    )
    // Oligarchy variant added when that path lands.
}

public struct SEPUpdateCommitmentRequest: Codable, Sendable {
    public let groupID: Data
    public let proof: Data                       // 192-byte canonical Groth16
    public let publicInputs: UpdateCommitmentPublicInputs
}
```

The contract reads `current.group_type` from storage to discriminate; the public-input variant carries the proof's actual values. The contract enforces "claimed variant matches stored group_type" as a binding check (per §4.7.4).

`SEPGroupMemberLeaf` gains an optional `slotIndex: UInt32?`. Codable shape adds the field with `nil` allowed; clients that have never seen this field decode messages without error and treat the field as absent. **No version byte is bumped** at the `SEPGroupMemberLeaf` level — the addition is wire-compatible at the parser level.

**Normative: `slotIndex` is NOT part of the per-leaf hash.** The Poseidon leaf hash that feeds the Merkle tree is `Poseidon(DOMAIN_MEMBER, Poseidon(sk))` (per §4.1.1) — it does not include `slotIndex`. Slot index is the tree *position*, not part of the leaf *value*. Two consequences:

1. The same member moving slots (which §4.1's "permanent slot assignment" rule forbids, but a buggy client could attempt) would not change the leaf hash, only the tree position — this is the property that makes the position vs. value distinction load-bearing.
2. A client that re-derives the leaf hash from `(secretKey, publicKeyCompressed)` alone produces the same value any other client would, regardless of whether `slotIndex` is `nil` or set. This keeps the cross-platform test vectors stable across the v0.3 → v0.4 transition (members with no `slotIndex` produce the same leaf hash as members with `slotIndex = 0`).

Compatibility rules:

- Anarchy / 1v1 / Oligarchy state updates: `slotIndex` is always `nil` on send. New clients that observe a non-nil `slotIndex` on a non-Democracy member ignore it.
- Democracy state updates and bootstrap payloads: every member entry MUST carry a non-nil `slotIndex`. A Democracy state update with any member missing `slotIndex` is rejected on receive with a structured `MissingSlotIndex` error — there is no implicit "infer from public-key sort order" fallback because that would defeat the single-leaf-delta property.
- Old client receiving a Democracy state update with `slotIndex` set: the §4.3 Layer 1 build-flag gate prevents the old client from generating a follow-on democracy update at all (the FFI symbol isn't compiled in). The old client can read chat (decryption keys are slot-index-agnostic) but cannot drive epoch transitions.
- Pre-design Democracy groups: with the contract redeploy in Phase C, the question is moot — old groups exist only on the abandoned `CC6N…RWWKE` contract and don't migrate. New Democracy groups created on the redeployed contract are v2-native from epoch 0.

`SEPSaltResponse` gains an optional `members: [SEPGroupMemberLeaf]?` field carrying the latest slot map, plus an optional `padding: Data?` field set to bring total wire size to the tier-uniform maximum (§4.7.5). Same Codable wire-compat rules as `SEPGroupMemberLeaf.slotIndex` — old clients ignore both new fields. The slot-map is what supports the §4.1.3 offline-joiner recovery; the padding is what keeps the wire size from leaking member-list length to a relay observer.

`BootstrapPayload` (`SEPBootstrapPayload`) gains a `thresholdNumerator: UInt8?` field per §4.7.6 — required (non-nil) for Democracy bootstrap payloads, MUST be `nil` for other governance types. A Democracy bootstrap payload with `thresholdNumerator == nil` is rejected on accept with a structured `MissingThreshold` error. Validated to `1..=100` on receive; a value outside that range is treated as a malformed payload. Joining members persist the value in their local group state; it's read by the prover whenever a democracy update proof is generated.

**`SEPSaltResponse` is end-to-end opaque on relays.** The padding scheme requires that the encoded message size remain tier-uniform from sender to receiver. A relay (or any intermediary) that decodes, modifies, and re-encodes a `SEPSaltResponse` could shrink the encoded form (e.g., by stripping the `padding` field per the parser-tolerant Codable rule, or by re-serializing a `SEPGroupMemberLeaf` without optional fields) and break the size-uniform property. Onym's relay path (`relayer/src/handler.rs` and the Nostr relay layer) is normatively forbidden from re-encoding `SEPSaltResponse` payloads — they are forwarded as opaque bytes from the sender's encoding to the receiver. The §7.3 relayer integration test asserts the relayer forwards `SEPSaltResponse` byte-for-byte, never decoding-and-re-encoding. (Chat events are already handled this way today via `event.content` opacity; this is the same pattern extended to protocol messages.) For implementations that can't guarantee opacity, the §4.7.5 padding falls back to a "best-effort hint" rather than a hard size guarantee — but the testnet path requires opacity.

### 4.5 Relayer dispatch

`relayer/src/handler.rs` adds the new contract's `update_commitment` polymorphic entrypoint (§4.7.4) to the function whitelist and dispatches all governance types through it. The stellar CLI invocation forwards `proof` plus the v2 `UpdateCommitmentPublicInputs` (with `occupancy_commitment_old`/`occupancy_commitment_new` instead of count fields) as a single JSON payload to `--public-inputs-file-path`. Because the entrypoint is polymorphic, the relayer doesn't need separate per-type handlers — same dispatch arm covers Anarchy, Democracy, and (future) Oligarchy after their per-type ceremonies land.

### 4.6 Contract redeploy + VK installation

This design ships against a **fresh contract address**. The existing testnet contract (`CC6NUUKG25RSFI6D57HISDQ4HRBLXFAUC3GFVZIACHQX3NLRPYTRWWKE`) is abandoned; no migration. Rationale: no live users, the existing contract holds Anarchy-only `update_commitment` plus the broken Democracy path, and the design's polymorphic-dispatch + occupancy-commitment storage layout doesn't fit the existing `CommitmentEntry` schema cleanly. A redeploy is simpler than an in-place migration and carries no user-facing cost.

`scripts/install-democracy-vks-testnet.sh` (extended to all three tiers and to install the v2 VKs) is invoked once against the new contract, after deploy. It installs:

- `keyset-democracy-dev/tier0-k32/verifying_key.bin` (Small, depth 5, capacity 32 members; circuit `k_max = 32`)
- `keyset-democracy-dev/tier1-k256/verifying_key.bin` (Medium, depth 8, capacity 256 members; circuit `k_max = 256`)
- `keyset-democracy-dev/tier2-k256/verifying_key.bin` (Large, depth 11, **capacity 2048 members but circuit `k_max = 256`**) — **new** in this design; previously out of scope, now in scope per project direction (ceremony for all three tiers at Phase 1 dev-key stage)

**Large tier `k_max` cap (testnet-only-with-cap).** The Large tier's circuit is compiled with `k_max = 256` rather than `k_max = 2048`. The bitmap and slot machinery still cover the full 2048 slots — the cap is on the *signer* count witnessable in a single proof, not on the membership count. Consequence: at the default `threshold_numerator = 50`, a Large group of size > 512 members (i.e., needing K ≥ 257 to clear `100·K ≥ 50·m_old`) cannot produce a proof under this VK. At higher thresholds the boundary tightens further. For testnet — where Large groups in the dev cycle are unlikely to exceed a few hundred members — this is acceptable. A future ceremony cycle that wants to support full-quorum Large updates regenerates the Large VK with `k_max = 1024` (separate, expensive ceremony round); §10.1 Q2's recommendation defers this. The cap is part of the binding contract test in §7.5 (operator UI surfaces "Large group exceeds single-VK signer cap; multi-aggregation proof required" once Phase F lands and the membership grows past the boundary).

The directory name `tier2-k256` (rather than `tier2-k2048` from earlier drafts) reflects the cap.

These VKs are regenerated from the v2 circuit (§4.7), not the v1 dev circuit currently in `src/circuit/democracy.rs`. The §6 phasing ensures the circuit lands before the VK install.

The new contract address is recorded in `RelayerDefaults.contractID` and shipped with the client release that turns on the v2 path. **Updated** clients (post Phase D release) pointing at the new contract get the working v2 flow; if such a client is asked to operate on a group bound to the old contract, the existing `chainRejected` UX surfaces "this group's contract was abandoned, please update."

Important nuance: the *old contract itself* doesn't change behavior — it still has the broken `Error::UnknownGroupType` rejection on Democracy `update_commitment` calls. Pre-Phase-D clients pointing at the old address therefore continue to see the same broken behavior they see today, **not** an "abandoned" message. The "abandoned" UX is purely a client-side check: a Phase-D-or-later client detects "my baked-in `RelayerDefaults.contractID` doesn't match the contract this group was created on" and surfaces the redeploy notice. Old clients have no such check; they just keep failing the way they currently fail. There is no live-user impact (no live users), so this asymmetry is acceptable.

### 4.7 Occupancy-commitment public input (the heart of v2)

The single change that makes "no member-count leakage" work: the public inputs `member_count_old` and `member_count_new` are removed and replaced by `occupancy_commitment_old` and `occupancy_commitment_new` — Poseidon hashes of the per-slot occupancy bitmap.

#### 4.7.1 Bitmap shape

For a group at tier with depth `D` (so `2^D` slots total), the occupancy bitmap is `2^D` bits, one per slot:

- `bitmap[i] = 1` if slot `i` holds an active member (leaf is `Poseidon(DOMAIN_MEMBER, Poseidon(sk_i))`)
- `bitmap[i] = 0` if slot `i` is a tombstone or never used (leaf is `Poseidon(DOMAIN_TOMBSTONE, 0)`)

The bitmap is computed deterministically from the leaf array — there is no separate authoritative source. Clients hold the bitmap derivable on demand from the `[SEPGroupMemberLeaf]` they already persist.

#### 4.7.2 Commitment

The `occupancy_commitment` is a Poseidon-based commitment to the bitmap. The encoding handles up to 2048 bits (Large tier) without exhausting field-element capacity:

```
occupancy_commitment = Poseidon(domain_occupancy, fold(bitmap))
```

where `fold` packs every `BITS_PER_FELT = 252` consecutive bits into one BLS12-381 scalar field element (`Fr` modulus is ~2^254.86, so 252 bits is safely below the canonical-encoding bound). For tier 0 (32 bits) it's one scalar; tier 1 (256 bits) is 2 scalars; tier 2 (2048 bits) is 9 scalars. The Poseidon hash is a single absorbing pass over those scalars plus the `domain_occupancy` tag.

`domain_occupancy` is a third domain tag (alongside `DOMAIN_MEMBER` and `DOMAIN_TOMBSTONE` from §4.1.1), pinned at circuit-compile time. Concrete value: `Fr::from(3)`. (See §10 Q3 for cross-circuit domain-tag coordination.)

#### 4.7.3 Circuit constraints (replaces the count-derived constraints in `src/circuit/democracy.rs`)

The circuit no longer takes counts as public inputs. Instead it:

1. **Witnesses the bitmap** — `2^D` boolean wires per side (old + new). Adds `2 · 2^D` boolean constraints.
2. **Binds the bitmap to the leaf array** — for each slot `i`, prove `bitmap[i] = 1 ⟺ leaf[i] ≠ tombstone_constant`. This is a standard "is-zero" boolean gadget on `d := leaf[i] - tombstone` and requires **two constraints** plus an auxiliary witness `is_active_inv`:

   ```
   (a)  d · is_active_inv = bitmap[i]      // forces bitmap = 1 when d ≠ 0 (witness inv = d⁻¹) and allows bitmap = 0 when d = 0
   (b)  (1 - bitmap[i]) · d = 0             // forces d = 0 when bitmap = 0, blocking the malicious case where leaf ≠ tombstone but bitmap = 0
   ```

   plus the boolean constraint `bitmap[i] · (1 - bitmap[i]) = 0` from witness allocation. **A single constraint is insufficient** — the binding has to be checked in both directions:

   - Without constraint (b), a malicious prover with `leaf ≠ tombstone` can witness `is_active_inv = 0` and bitmap = 0; constraint (a) becomes `d · 0 = 0` (satisfied), and the prover has unwound the binding.
   - Without constraint (a), a tombstone slot with `d = 0` is unconstrained on bitmap.

   With both constraints active, the only satisfying assignments are `(d = 0, bitmap = 0)` (any `is_active_inv`, vacuously) and `(d ≠ 0, bitmap = 1, is_active_inv = d⁻¹)`. The §4.1.1 domain separation is what makes this a soundness-preserving check: the circuit doesn't have to discriminate two random Poseidon outputs, only "domain-tagged member leaf" vs "domain-tagged tombstone constant."

   This gadget is the load-bearing soundness link between the witnessed bitmap and the actual Merkle tree contents — if it's wrong, the entire occupancy-commitment privacy claim collapses (the prover could commit to a bitmap that disagrees with the tree, breaking the popcount-derived count constraints). Any future re-derivation (e.g., during ceremony review) MUST verify both constraints are present and the polarity is correct.
3. **Computes popcount** as a private witness — sum of the bitmap booleans. Used internally for:
   - Quorum: `100·K ≥ threshold_numerator · popcount(bitmap_old)`, where `threshold_numerator ∈ [1, 100]` is a public input set per group at create time (see §4.7.6). Simple majority is `threshold_numerator = 50` (recovering the original `2·K ≥ m_old` constraint when `K, m_old` are scaled by 100/50). Two-thirds supermajority is `threshold_numerator = 67`; unanimous is `threshold_numerator = 100`. The denominator is implicit (always 100); thresholds are expressed as integer percentages.
   - Count-delta: `|popcount(bitmap_new) − popcount(bitmap_old)| ≤ 1` (replaces public-input constraint #6).
   - Floor: `popcount(bitmap_new) ≥ 2` (the previously contract-only `m_new ≥ 2` rule from §4.2.1's table moves into the circuit). Independent of the threshold value — the floor is a structural minimum group size, not a quorum ratio.
   - Ceiling: implicit in the bitmap's fixed length — at most `2^D` slots can be active because the bitmap *is* of length `2^D`. No explicit ceiling constraint needed.
4. **Computes `occupancy_commitment_old`/`occupancy_commitment_new`** as defined in §4.7.2 and exposes them as public inputs (replacing the count fields).
5. **Exposes `threshold_numerator` as a public input** so the verifier can plug in the chain-stored value. Bound to chain via §4.7.6's contract-side check (`claimed_threshold_numerator == current.threshold_numerator`). Range-checked in-circuit to `[1, 100]` via 7-bit decomposition + `numerator ≤ 100` constraint.

R1CS-constraint impact estimate (for tier 1 / depth 8 / 256 slots, the testnet baseline):

| Constraint family | v1 count-public | v2 occupancy commitment |
|---|---|---|
| Bitmap booleans (2 sides × 256 slots × 1 boolean each) | 0 | ~512 |
| Bitmap-to-leaf binding (2 sides × 256 slots × 2 mults each per the §4.7.3-step-2 corrected gadget) | 0 | ~1024 |
| Popcount (additions are free in R1CS — no constraint cost) | 0 | 0 |
| Quorum `100·K ≥ threshold_numerator · popcount(bitmap_old)` (one mult `numerator * popcount`, plus ~17-bit range check on the difference; `threshold_numerator * 2048` ≤ 204 800 ≈ 2^17.6) | ~10 | ~22 |
| Count-delta `\|popcount_new − popcount_old\| ≤ 1` (popcount-witnessed) | ~5 | ~5 |
| Floor `popcount(bitmap_new) ≥ 2` (bit-decomp range check, was contract-only in v1) | 0 | ~25 |
| Threshold range check `1 ≤ threshold_numerator ≤ 100` (7-bit decomp + ≤100 check) | 0 | ~10 |
| Occupancy commitment Poseidon (2 sides, ~2 hashes per side × ~300 R1CS each) | 0 | ~1200 |
| Existing constraints unchanged | ~3500 | ~3500 |
| **Total** | **~3525** | **~6298** (≈1.8× larger) |

A previous draft of this table double-counted popcount (charging 512 constraints as if additions cost something) while undercounting the occupancy-commitment hash. The numbers above are the corrected accounting. The quorum row gained ~12 constraints over the fixed-50% formulation because (a) the multiplication `threshold_numerator · popcount` is now witness-times-witness (one R1CS mult instead of constant scaling), and (b) the difference's range check widens from ~12 bits to ~18 bits. The threshold-range row is new (~10). Net additional cost vs. fixed-50%: ~22 constraints — trivial. Proving time scales roughly linearly with constraint count for Groth16, so v2 proves about 1.8× slower than v1. On a modern device that's the difference between ~150ms and ~270ms — well within budget for an interactive epoch transition.

Phase A exit gate references this estimate ("constraint count within 10% of §4.7.3 estimate"). Updated baseline: **~6298** for tier 1 (depth 8). Tier 0 (depth 5, 32 slots) drops the bitmap-related rows roughly proportional to slot count: ~32 booleans + ~128 mults binding + same Poseidon cost + same fixed rows ≈ ~5082 constraints. Tier 2 (depth 11, 2048 slots) scales the bitmap-related rows by 8× over tier 1: ~4096 booleans + ~8192 mults binding + same Poseidon cost (Poseidon over 9 packed scalars at ~300 each ≈ ~2700) + same fixed rows ≈ ~19022 constraints. Phase A4 must measure and confirm.

#### 4.7.4 Polymorphic `update_commitment` entrypoint (eliminates entrypoint-selector leak)

The new contract exposes a single `update_commitment` entrypoint that accepts any governance type:

```rust
pub fn update_commitment(
    env: Env,
    group_id: BytesN<32>,
    proof: Groth16Proof,
    public_inputs: UpdateCommitmentPublicInputs,
) -> Result<(), Error>
```

Internally, the contract reads `current.group_type` from storage and dispatches to the corresponding VK lookup (`load_update_vk_by_type(tier, group_type)`). No per-type entrypoint name is exposed in the call selector. A chain-trace observer who didn't witness `create_group_v3` cannot derive the group type from the per-epoch update call alone — they'd need to read the contract storage entry, which is also an observable signal but a less convenient one (storage reads aren't part of the canonical event/log stream that block explorers index by default).

This is the §11-deferred polymorphic-dispatch fix from v0.3, pulled forward into the core design now that we're redeploying the contract anyway.

`UpdateCommitmentPublicInputs` is a discriminated enum carried through the proof public inputs; the discriminant is the `group_type` (read from storage, not the public input itself, to prevent type-confusion attacks where a caller claims their proof is for type X while the on-chain group is type Y). The contract enforces `claimed_group_type_in_public_inputs == current.group_type` as a binding check.

#### 4.7.5 `SEPSaltResponse` size padding

Per §3.4 residual #6, the protocol-level `SEPSaltResponse` size varies with member-list length, which is observable to any relay measuring kind-`44114` payload sizes. The fix: pad the encoded `members: [SEPGroupMemberLeaf]?` field to the tier-uniform maximum on send (so a Small-tier salt response is always sized for 32 members, regardless of how many are actually present). The padding is dropped on receive after CBOR/JSON parse — the wire shape is uniform but the decoded array carries only real members. Implementation: an explicit `padding: Data?` field in `SEPSaltResponse` set to a length that brings total payload to the tier ceiling. ~10 LOC on each platform.

#### 4.7.6 Configurable quorum threshold (per group, fixed at create)

The democracy circuit accepts a `threshold_numerator ∈ [1, 100]` public input. The quorum constraint becomes `100·K ≥ threshold_numerator · m_old` (§4.7.3 step 3, quorum row), where `m_old = popcount(bitmap_old)`. Examples:

- `threshold_numerator = 50` — simple majority (`100·K ≥ 50·m_old` ⇔ `K ≥ m_old/2`). Default if the client doesn't specify.
- `threshold_numerator = 67` — two-thirds supermajority.
- `threshold_numerator = 75` — three-quarters supermajority.
- `threshold_numerator = 100` — unanimous (`K = m_old`).

The denominator is implicit (always 100); thresholds are integer percentages. This covers the common cases; thresholds requiring finer granularity (e.g., `60.5%`) are not supported (deliberately — fixing the denominator simplifies the circuit and the UX).

**Tie semantics: ≥ threshold passes (ties favor the proposal).** The constraint `100·K ≥ threshold_numerator · m_old` is non-strict. Concretely, with `threshold_numerator = 50` and `m_old = 4`, `K = 2` satisfies (`200 ≥ 200`) — exactly half the members suffices for "simple majority." This matches the natural reading of "50% threshold = at least half" and Robert's-Rules common-law tie convention. Operators who want strict majority (`> 50%`, ties fail) must pick `threshold_numerator = 51`; the doc's default is 50 for the simpler reading. The §7.1 `prove_democracy_v2_threshold_tie_passes` test pins this semantics with the `threshold=50, m_old=4, K=2` boundary case.

**Public-input ordering (canonical).** The circuit's IC vector for `UpdateByType(2)` has 7 elements in this fixed order: `[base, c_old, epoch_old, c_new, occupancy_commitment_old, occupancy_commitment_new, threshold_numerator]`. Pinned in `docs/soroban-contract-test-vectors.json` (`vk_kind_enum.UpdateByType(2).ic_layout_v2`) — that file is the single source of truth. Any client / contract / cross-reimplementation that disagrees on this ordering produces proofs that fail verify silently. The §7.2 cross-platform test vectors include a fixture covering this layout end-to-end.

**Per-group, fixed at create.** The threshold is set when the group is published via `create_group_v3` and stored in contract state alongside the other group metadata (`group_type`, `tier`, `commitment`, `epoch`, `occupancy_commitment`). It cannot change for the lifetime of the group — there is no `update_threshold` entrypoint and no path through `update_commitment` that mutates it. A group whose members later want a different threshold must recreate as a new group. (Threshold rotation is a §11 follow-up.)

**Wire format unchanged on the update path.** The threshold isn't carried in `UpdateCommitmentPublicInputs::Democracy` — the contract reads it from storage (`current.threshold_numerator`) and supplies it to the Groth16 verifier as a public input alongside the wire-supplied `(c_old, epoch_old, c_new, occupancy_commitment_old, occupancy_commitment_new)`. The prover must construct the proof with the same threshold value (which it learns from the group's local state, populated at create or via the bootstrap payload). Any mismatch surfaces as proof-verification failure on chain.

**Bootstrap payload extended.** `BootstrapPayload` (`SEPBootstrapPayload` Codable) gains a required `thresholdNumerator: UInt8` field for Democracy groups. Joining members read it on accept and persist it in their local group state. (Anarchy / 1v1 / Oligarchy bootstraps don't carry it — `nil` for those types per §4.4's optional-field rule, with the same Democracy-MUST-have-it / non-Democracy-MUST-be-nil semantics.)

**Validation.**
- Contract `create_group_v3`: rejects `threshold_numerator < 1 || threshold_numerator > 100` with `Error::InvalidThreshold` (new error code).
- Circuit: range-checks `threshold_numerator ∈ [1, 100]` via 7-bit decomposition + an explicit `numerator ≤ 100` constraint (~10 R1CS).
- Client UI: defaults to 50; common preset buttons for 50/67/75/100; "custom" entry accepts any 1–100 integer with a tooltip explaining the trade-offs (lower = easier to pass changes, higher = stronger consensus required).

**Privacy class.** `threshold_numerator` is in the same publicness class as `group_type` and `tier` — set at create time, observable to a chain reader via storage inspection, but not exposed in the per-update call payload. A chain observer that reads contract state for a group sees its threshold; one that watches only `update_commitment` calls does not. This matches the v0.4 trust posture for `group_type` (per §3.4's residual table); no new leak is introduced.

**Why per-group instead of per-update?** A per-update threshold would let a group raise the bar for a controversial vote and lower it for routine ones — flexible but trust-eroding (the threshold itself becomes a meta-vote). Per-group is the simpler trust model and matches how real-world bylaws work (the threshold is the governance constitution, fixed at founding, changed only by recreating the polity).

---

## 5. Alternatives Considered

### 5.1 Skip-on-failure ("scoped patch")

Detect `groupType == .democracy` in the iOS/Android publish path and skip the chain update entirely; advance local state only. Trust posture for membership changes degrades to "local-only" — peers must trust each other's broadcasts without on-chain anchor.

- **Pros:** ~30 LOC, one file each on iOS and Android. No cryptography risk. No deployment dependency.
- **Cons:** Permanently degrades the on-chain trust property of Democracy groups. Any malicious or buggy peer can announce a fake member join and other peers have no chain authority to detect it. Unsuitable for any group where on-chain anchoring is the security premise.

Rejected as the primary path. The document separately tracks this as the **runtime fallback** the clients employ when §4.3 Layer 2 detects a fingerprint mismatch (i.e., dev client running against a real-ceremony contract) — in that fallback, the client refuses to chain-update *and* refuses to chat, surfacing a "you need a production build" error to the user. This is stricter than the original scoped-patch suggestion (which silently degraded chat to local-only).

### 5.2 Wait for the ceremony

Don't ship Democracy chain updates at all until the Phase 2 ceremony lands. Democracy groups remain unusable until then, possibly months.

- **Pros:** No dev-cryptography exposure.
- **Cons:** Blocks all testnet UX work on Democracy until the ceremony, which itself is harder to coordinate without a working client to dogfood with. Ceremony review benefits from real-world circuit usage to surface integration issues.

Rejected. The dev-VK-on-testnet path lets us iterate on UX and surface integration issues before the ceremony, gated by §4.3.

### 5.3 Multi-signer support upfront

Implement quorum collection (vote → gather K secret openings → aggregate proof) as part of this design, so groups of any size can advance.

- **Pros:** Solves the entire democracy update problem in one pass.
- **Cons:** Quorum collection is its own substantial UX (proposal lifecycle, vote tallying, secret-opening transport, timeout handling). It also requires careful security analysis — naively transmitting K secret keys to one prover defeats the point of K-of-N signatures, so the actual design is "each of K members produces a partial proof; an aggregator combines them" or "K members each sign a proposal hash; the proposal hash becomes part of the proof input." Either is a separate design doc.

Deferred. §4.2 ships the single-signer subset; multi-signer is a follow-up document.

### 5.4 Count-public on testnet first, occupancy-commitment later

An earlier draft (v0.2 / v0.3 of this doc) proposed shipping `update_commitment_democracy` with `member_count_old` / `member_count_new` as public inputs on testnet first, accepting the privacy regression as a temporary trade-off, and rebuilding the circuit with hidden counts as a separate Phase 3 effort.

- **Pros:** Smaller initial implementation (~990 LOC vs the ~2615 in §6 for the v2 path A–D), no contract redeploy, dev-key ceremony scoped to tiers 0/1 only initially.
- **Cons:** Locks in a wire format that has to be migrated later (separate ceremony, separate VK rotation, separate client release). Two Phase 2 ceremonies instead of one. Two reviewer cycles for cryptography. Degrades Democracy's privacy below Anarchy's even temporarily — which is unacceptable to the project's "no metadata leakage" commitment.

Rejected by project direction (see §3.4). With no live users and ceremony status at Phase 1 dev-key for all three tiers, the cost of redeploy + single ceremony on the v2 shape is lower than the cost of shipping v1, then revisiting. This document is the v2 path; v1 is not implemented.

---

## 6. Implementation Plan — six phases

The work decomposes into six phases (A–F). Each phase ends in a testable, mergeable state — Phase A leaves a working v2 prover with passing Rust tests but no clients calling it; Phase C lands the redeployed contract with no clients pointing at it; etc. Phases are sequenced by dependency, but multiple phases can be in flight in parallel where the dependency graph allows (B can start once A's circuit shape stabilizes, even before A fully merges; D can start once B's prover output format stabilizes).

Within each phase, every numbered step is a discrete PR — small enough to review in isolation, with the design doc as the spec. No step bundles changes across phases.

### Phase A — v2 circuit (`src/circuit/democracy_v2.rs` + prover)

**Goal**: a working Rust-only proof generation + verification round-trip for the v2 (occupancy-commitment) circuit, with all three tier dev VKs generated.

| Step | Files | LOC | Output |
|---|---|---|---|
| A1 | `src/circuit/democracy_v2.rs` (new file). `src/circuit/democracy.rs` is **renamed** to `src/circuit/democracy_v1.rs` in the same commit. The renamed file is gated `#[cfg(test)]` so it stays available for v0.3-vs-v0.4 differential tests but is excluded from the production build and the FFI. Imports in non-test code are migrated to `democracy_v2`; `mod democracy_v1` is declared only inside `#[cfg(test)]` blocks. After the v2 ceremony lands and the differential tests are no longer useful, `democracy_v1.rs` is deleted in a follow-up cleanup. | ~425 | Empty + populated circuit; bitmap witness; popcount; bitmap-to-leaf binding (§4.7.3); occupancy-commitment Poseidon; all count-derived constraints internalized; domain tags from §4.1.1 + new `DOMAIN_OCCUPANCY`; **threshold_numerator public input + parameterized quorum constraint per §4.7.6** (~25 LOC over the fixed-50% formulation) |
| A2 | `src/circuit/democracy_v2.rs` tests | ~225 | Round-trip 1→2, 2→3, 2→4 (Insert), 3→2 (Remove via floor rejection), key rotation, **bitmap-mismatch attack rejection**, **tombstone-collision regression** (§4.1.1), slot-exhaustion at tier ceiling, **threshold sweep** (50/67/75/100 each pass for K satisfying, fail for K below quorum) |
| A3 | `src/prover/mod.rs` `prove_democracy_v2` + `verify_democracy_v2` | ~165 | Bitmap derivation from rosters, occupancy-commitment computation, single-leaf-delta inference, `QuorumRequired` error for `m_old ≥ 3` (single-signer subset per §4.2). Prover reads `threshold_numerator` from group state and supplies it as a public input. Verify-side helper inlines the chain-stored threshold for unit-test convenience. |
| A4 | `scripts/generate-democracy-vk-dev.sh` extension; `keyset-democracy-dev/{tier0-k32,tier1-k256,tier2-k256}/` (Large reuses k_max=256 per the §4.6 cap) | (script + bin output) | All three tier dev VKs regenerated against the v2 circuit; checked-in `verifying_key.bin` + fingerprint hashes |
| A5 | Cargo feature `democracy-v2-dev-vks` (off by default) | `Cargo.toml`, `src/lib.rs` | Feature gate per §4.3 Layer 1 |
| A6 | Cross-platform test vectors | `docs/cross-platform-test-vectors.json` | New `democracy_v2` section: 1→2 and 2→3 reference proofs with expected `occupancy_commitment_old`/`occupancy_commitment_new` |

**Phase A exit criteria**: `cargo test --features democracy-v2-dev-vks circuit::democracy_v2` and `cargo test prover::democracy_v2_round_trip` both green. R1CS constraint count for tier 1 is within 10% of the §4.7.3 estimate (~6298 constraints, see the updated table). Dev VKs check-in includes a fingerprint manifest (`keyset-democracy-dev/fingerprints-v2.json`).

**Phase A is the load-bearing crypto step** — it's where R1CS soundness is established. Subsequent phases depend on the circuit being correct.

### Phase B — FFI export + Swift/Kotlin bridges

**Goal**: the v2 prover is callable from Swift and Kotlin, returning bytes that downstream code can serialize for the contract.

| Step | Files | LOC | Output |
|---|---|---|---|
| B1 | `src/ffi.rs` `sep_generate_democracy_update_proof_v2` | ~80 | Opaque-bytes FFI export, gated on `democracy-v2-dev-vks` |
| B2 | `swift-mls/Sources/SwiftMLS/Types.swift` | ~40 | `SEPDemocracyUpdatePublicInputsV2` (5 fields: c_old, epoch_old, c_new, occupancy_commitment_old, occupancy_commitment_new); `SEPGroupMemberLeaf.slotIndex: UInt32?` per §4.4 |
| B3 | `swift-mls/Sources/SwiftMLS/RustBridge.swift` + `ProofGenerator.swift` | ~80 | `generateDemocracyUpdateProofV2(input:)` |
| B4 | `kotlin-mls/.../Types.kt` | ~40 | Parallel to B2 |
| B5 | `kotlin-mls/.../RustBridge.kt` + `SEPProofGenerator.kt` | ~80 | Parallel to B3; removes `DemocracyProofNotImplementedException` stub |
| B6 | XCFramework + Android NDK rebuild | (script run, no LOC) | Triggered automatically on B1 merge by the existing CI pipeline (`scripts/build-xcframework.sh` and the Android NDK build steps in `.github/workflows/pr.yml`). The committed binary artifacts under `build/` are refreshed in the same PR as B1 if the engineer runs the script locally; otherwise the next CI run produces them. Phase B exit criteria below depend on the committed artifacts carrying the new export, so the merge sequence is "B1 + B6 in one commit" or "B1 then B6 in a follow-up commit before B7." |
| B7 | Bridge round-trip tests | ~100 | Swift and Kotlin tests that match the §4.7's cross-platform test vectors from A6 |

Phase B per-step LOC: `80 + 40 + 80 + 40 + 80 + 0 + 100 = 420`. The cumulative-scope summary at the end of §6 lists Phase B as **~420 LOC** to match (see "Cumulative scope estimate").

**Phase B exit criteria**: from a Swift / Kotlin call site, `generateDemocracyUpdateProofV2` produces bytes whose Rust-side `verify_democracy_v2` accepts them, and whose serialized public inputs match the cross-platform test vectors. No contract or relayer touched.

### Phase C — Contract redeploy with v2 entrypoints

**Goal**: a fresh testnet contract address that accepts v2 proofs, with polymorphic `update_commitment` dispatch (§4.7.4).

| Step | Files | LOC | Output |
|---|---|---|---|
| C1 | `contracts/sep-xxxx/src/lib.rs` v2 entrypoints | ~270 | Polymorphic `update_commitment(group_id, proof, public_inputs)`. Storage layout: `occupancy_commitment: BytesN<32>` per group instead of `member_count: u32`; new `threshold_numerator: u8` per Democracy group. New `create_group_v3` that takes `threshold_numerator` as an argument (validated to `1..=100`, default 50 if not specified at the API level), emits initial occupancy commitment for the creator-only state, and stores the threshold. New `Error::InvalidThreshold` variant. Verifier-side: `update_commitment` reads `current.threshold_numerator` from storage and includes it in the Groth16 public-inputs vector for the Democracy variant (§4.7.6). |
| C2 | Contract tests | ~250 | Mirror existing democracy tests against the v2 entrypoint; add tests for the polymorphic-dispatch type-confusion guard (`claimed_group_type == current.group_type`) and for the floor/ceiling moving in-circuit |
| C3 | `Cargo.toml` (contract crate) — bump SemVer | ~5 | Marks the contract as a breaking change, abandoning the old testnet contract |
| C4 | `scripts/deploy_sep_xxxx_testnet.sh` extension | ~30 | Runs the redeploy, captures the new contract address, writes `RelayerDefaults.contractID` (build-time-injected) |
| C5 | `scripts/install-democracy-vks-testnet.sh` extension to all three tiers, v2 VKs | ~30 | Installs the three v2 VKs from Phase A4 into the new contract |
| C6 | Smoke test on testnet | (manual + scripted) | `create_group_v3` → `update_commitment` round-trip via stellar CLI, against a fresh group; observable via Soroban testnet block explorer |

**Phase C exit criteria**: a real testnet contract at a new address accepts a v2 proof end-to-end, returns success. No client wires to it yet — exercised manually via `stellar` CLI + `cargo`-built proof bytes from Phase B. The old contract `CC6N…RWWKE` is documented as deprecated; clients using it surface "this group's contract was abandoned, please update."

### Phase D — Relayer + clients

**Goal**: iOS and Android clients drive Democracy member adds end-to-end against the new contract via the relayer.

| Step | Files | LOC | Output |
|---|---|---|---|
| D1 | `relayer/src/handler.rs` | ~60 | Whitelist + dispatch for the polymorphic `update_commitment`. Forwards `proof` + serialized `UpdateCommitmentPublicInputs` to stellar CLI |
| D2 | `swift-mls/Sources/SwiftMLS/ContractClient.swift` | ~40 | `updateCommitment(_:)` method calling the new endpoint |
| D3 | `kotlin-mls/.../ContractClient.kt` | ~40 | Parallel to D2 |
| D4 | iOS `OnChainService.publishCommitmentUpdate` dispatch | ~150 | Branches on `group.groupType == .democracy`; assembles bitmap from current member list; calls `generateDemocracyUpdateProofV2`; submits via `ContractClient.updateCommitment`; honors §4.3 Layer 2 fingerprint check |
| D5 | iOS `applyStateUpdate` slot-index assignment, concurrent-acceptor rollback (§4.1.4), acceptor restart re-broadcast (§4.1.3) | ~120 | `lastBroadcastEpoch` marker persisted in PersistenceStore; rollback path on `chainRejected`; on-launch detect-and-rebroadcast |
| D6 | Android parallel for D4 + D5 | ~270 | Same shape, Kotlin |
| D7 | `RelayerDefaults` + client config bumps | ~20 | New contract ID baked into the build |
| D8 | iOS UI: tier-upgrade-on-create UX, "this group's contract was abandoned" banner for old-contract groups | ~80 | Surfaces the §3.4 residual leaks honestly to the user (over-size at create, redeploy notice) |
| D9 | Android UI parallel | ~80 | |
| D10 | `SEPSaltResponse` padding (§4.7.5) on both clients | ~20 | Tier-uniform payload size |

**Phase D exit criteria**: A two-device test (iOS + Android) creates a Medium-tier Democracy group, accepts a join, advances epoch on chain via the new contract, and exchanges chat messages with `slotIndex`-aware encryption. The old contract is not used anywhere in the running clients.

### Phase E — Ceremony coordination (Phase 2 trusted setup, on v2 circuit only)

**Goal**: a real MPC ceremony for the v2 circuit, replacing the dev keys, gating mainnet release.

| Step | Notes |
|---|---|
| E1 | Pre-ceremony R1CS soundness review of `democracy_v2.rs` (external cryptography reviewers per [`democracy-circuit-ceremony.md`](democracy-circuit-ceremony.md)). Output: signed-off circuit hash. |
| E2 | Update `democracy-circuit-ceremony.md` to scope the ceremony to v2 only (drop the v1 plan). |
| E3 | Run the ceremony (per the existing ceremony tooling at `tools/ceremony/`) for all three tiers against the v2 circuit. Output: production VKs + ceremony attestations. |
| E4 | Install production VKs on a **separate** mainnet contract (not the testnet one) via `scripts/install-democracy-vks-mainnet.sh` (new). Mainnet contract address differs from testnet. |
| E5 | Client release: bake mainnet contract ID + production VK fingerprints. Drop the dev fingerprint allowlist in mainnet builds. CI assertion: `democracy-v2-dev-vks` feature is OFF in mainnet release builds. |

**Phase E exit criteria**: a mainnet client can drive a Democracy group through `update_commitment` against the production-VK mainnet contract.

### Phase F — Multi-signer quorum collection (the larger follow-up)

**Goal**: support Democracy groups with `member_count_old ≥ 3`, where a single signer can no longer satisfy the `2K ≥ m_old` constraint.

This phase is itself a multi-step design effort with its own design doc (deferred). High-level scope:

- Proposal lifecycle: any member proposes a member-add or remove; chain proposal anchor.
- Vote-tally protocol: K members each contribute a partial signature / partial proof.
- Aggregation: combine K partial proofs into a single Groth16 proof. Likely requires either MPC at the prover step (heavy) OR a circuit redesign where each signer's contribution is verifiable independently and the proof carries K verified contributions (less heavy but circuit grows in K).
- UX: vote initiation, expiry, cancellation, dual-confirmation for removals.

Phase F is **explicitly out of scope** for this design doc. Without it, Democracy groups are capped at 3 members on the user side. §11 follow-up tracks the design dependency.

### Cumulative scope estimate

| Phase | Engineering LOC (approx) | Calendar (engineer-weeks) |
|---|---|---|
| A — circuit + prover | ~815 (A1 ~425 + A2 ~225 + A3 ~165) | 2–3 (R1CS review is the gate) |
| B — FFI + bindings | ~420 (sums B1–B7; B6 is a CI rebuild, no LOC) | 1 |
| C — contract redeploy | ~585 (C1 ~270 + others unchanged) | 1 |
| D — relayer + clients | ~890 (D adds threshold-aware UX picker; ~10 extra) | 2 |
| **A–D total (testnet-functional)** | **~2710** | **6–7** |
| E — ceremony | (mostly process) | 4–8 (calendar; coordination + MPC sessions) |
| F — multi-signer | (separate design) | not estimated; multi-month |

The "no live users" position lets Phase D land while Phase E is in flight: the testnet path uses dev VKs, mainnet release waits for E.

---

## 7. Test Plan

### 7.1 Rust unit tests

- `prove_democracy_v2_round_trip_one_to_two` — single signer, 1 → 2 member insert, valid proof, verifier passes.
- `prove_democracy_v2_round_trip_two_to_three` — single signer, 2 → 3 member insert, valid proof, verifier passes.
- `prove_democracy_v2_errors_on_quorum_required` — single signer, 3 → 4 member insert, returns `QuorumRequired` without attempting proof generation.
- `prove_democracy_v2_handles_replace` — single signer, key rotation at member's own slot.
- `prove_democracy_v2_rejects_below_floor` — single signer, **2 → 1** transition (the realistic "remove member from a 2-member group" case; 1 → 0 is unreachable from any valid create state since `create_group_v3` already establishes the creator at member-slot 0 with member_count ≥ 1, and the §4.2 single-signer subset cannot remove the sole member without violating the floor in any case). With v2's in-circuit `m_new ≥ 2` floor, the prover **rejects** at proof generation time, returning `BelowMinCount`. The contract-side check is now defense-in-depth, not the only enforcer. Asserts the v2 improvement: prover-side rejection matches `update_commitment` rejection — the circuit accepts only proofs the contract will also accept (§4.2.1 second consequence). The earlier draft cited 1 → 0; that scenario is not reachable from any legitimate group state and was a mis-spec.
- `prove_democracy_v2_rejects_two_to_one` — explicit fixture for the practical floor-violating case: a 2-member Democracy group, one member tries to leave (single-signer demote-self attempt). The prover detects `m_new = 1 < 2` and returns `BelowMinCount`. UX-equivalent of "you can't be the second-to-last member of a Democracy group; either invite a third or recreate as Anarchy/1v1." This is the practical companion to the abstract `_rejects_below_floor` case.
- `prove_democracy_v2_domain_tags_block_tombstone_collision` — paired test exercising the §4.1.1 normative rule. The "domain-tags-on" build (default) is the production path; the "domain-tags-off" build is a `#[cfg(test)]` toggle (`#[cfg(feature = "no-domain-tags-for-test")]` on the leaf-hash function) that strips the outer `Poseidon(DOMAIN_*, …)` wrapping. The test pair:
  - **Domain-tags-off variant**: construct an attack scenario where a member's `Poseidon(sk)` happens to equal the all-zero leaf used as the tombstone constant in the unwrapped scheme (forced via a hand-picked test scalar). Drive a transition that relies on slot disambiguation between this member and a tombstone slot. Assert the prover **succeeds** in producing a malicious proof — i.e., without domain tags, the soundness break is real.
  - **Domain-tags-on variant**: same scenario, default build. Assert the prover either rejects the input (the bitmap-to-leaf binding from §4.7.3 step 2 detects the inconsistency because `domain-tagged member leaf ≠ domain-tagged tombstone constant`) or produces a proof that fails the bitmap-binding constraint at proof-gen time.

  Without the paired structure, a "domain-tags-on" test alone passes trivially regardless of whether domain tags are correctly applied — the threat doesn't materialize because the disjoint domain-tag prefixes already prevent the inner-Poseidon collision from lifting to the outer leaf-hash collision. The paired test is the only way to regress the normative rule. The `no-domain-tags-for-test` Cargo feature is gated to `#[cfg(test)]` so it cannot be enabled in any production build (Phase E mainnet CI assertion already enforces "production features set is fixed"; this feature joins that fixed-out list).
- `prove_democracy_v2_slot_exhaustion` — at tier capacity (Small=32 for the test), drive cumulative joins past `2^depth - 1` (never-used slots run out due to tombstones). Asserts the prover returns `SlotExhausted`, not a malformed proof.
- `prove_democracy_v2_bitmap_mismatch_rejected` — manually feed the prover a bitmap that disagrees with the actual leaf array (e.g., `bitmap[5] = 1` but `leaf[5] = tombstone`). Asserts proof generation fails at the bitmap-to-leaf binding constraint (§4.7.3 step 2). Soundness regression test.
- `prove_democracy_v2_occupancy_commitment_round_trip` — for a known fixture roster, assert that `Poseidon(DOMAIN_OCCUPANCY, fold(bitmap))` matches a hardcoded expected value. Cross-platform vector (§7.2) reuses the same fixture.
- `prove_democracy_v2_threshold_simple_majority` — `threshold_numerator = 50`, group of size 3, K = 2 (50% of 3 rounded up = 2). Asserts the proof verifies. Equivalent to the original fixed-50% behavior; included to confirm the parameterized circuit recovers the v0.4-pre-threshold semantics when the default value is supplied.
- `prove_democracy_v2_threshold_supermajority` — `threshold_numerator = 67`, group of size 3, K = 2 (need `100·2 ≥ 67·3 ⟺ 200 ≥ 201` → false; should fail). Then K = 3 (`300 ≥ 201` → true; should pass). Both asserted.
- `prove_democracy_v2_threshold_unanimous` — `threshold_numerator = 100`, group of size 3, K = 2 (need `200 ≥ 300` → false; should fail). Then K = 3 (`300 ≥ 300` → true; should pass).
- `prove_democracy_v2_threshold_out_of_range_rejected` — `threshold_numerator = 0` and `threshold_numerator = 101` both must be rejected at proof generation by the §4.7.6 range-check constraint. Asserts the in-circuit `1 ≤ numerator ≤ 100` enforcement is active.
- `prove_democracy_v2_threshold_mismatch_with_chain_rejected` — prover constructs a proof with `threshold_numerator = 50` but the (mocked) on-chain group has `current.threshold_numerator = 67`. Asserts the proof verifies as a free-standing Groth16 proof (the circuit doesn't know what the chain stores) but **fails** when the verifier supplies the chain-stored value (67) instead of the prover's claimed value (50). This is the verifier-supplied public-input binding from §4.7.6 — exercised at the prover's `verify_democracy_v2` helper level. **The contract-side companion test lives in §6 step C2** (see `update_commitment_threshold_mismatch_rejected_v2` below) so the same binding is verified end-to-end against a real Soroban harness.
- `prove_democracy_v2_threshold_tie_passes` — `threshold_numerator = 50`, group of size 4, K = 2 (`100·2 = 200`, `50·4 = 200`, exact tie passes per §4.7.6). Asserts the proof verifies. The non-strict-`≥` semantics from §4.7.6 is asserted here — the boundary case that distinguishes "≥ 50%" from "> 50%."
- `prove_democracy_v2_threshold_strict_majority_tie_rejects` — `threshold_numerator = 51`, group of size 4, K = 2 (`100·2 = 200`, `51·4 = 204`, fail). Asserts the proof is rejected — the canonical fixture for "operator wants strict majority" semantics. Pairs with the previous test to nail down the choice between non-strict (`50`) and strict (`51`) thresholds.

### Contract integration tests (live under §6 step C2; cross-referenced here for visibility)

The following contract-side tests live in `contracts/sep-xxxx/src/lib.rs` (§6 step C2) but are listed here so the `update_commitment_v2` test plan stays grep-able by the test name across layers:

- `update_commitment_threshold_mismatch_rejected_v2` — contract integration. Stored `current.threshold_numerator = 67`. Submit a Groth16 proof valid under `threshold_numerator = 50`. Assert the contract's `update_commitment` (Democracy variant dispatch) rejects with `InvalidProof` (the verifier-side binding from §4.7.6 — chain-stored value is the authoritative one supplied to the Groth16 verify, the prover-side claim is moot once it diverges). Companion to the prover-side `prove_democracy_v2_threshold_mismatch_with_chain_rejected`. The two together exercise §4.2.1's "contract-only state binding" row end-to-end.
- `update_commitment_threshold_tie_passes_v2` — contract integration. Stored `current.threshold_numerator = 50`, `m_old = 4`, valid 4 → 5 proof with K = 2. Assert the contract accepts and advances the epoch. Companion to `prove_democracy_v2_threshold_tie_passes`.

### 7.2 Cross-platform vectors

- Add a `democracy_v2` section to `docs/cross-platform-test-vectors.json` covering 1 → 2, 2 → 3, and a removal-then-re-add (testing tombstone permanence and slot-never-reused). Each vector includes: input rosters with slotIndex, expected `occupancy_commitment_old`/`occupancy_commitment_new`, expected proof bytes (or proof-validity check). Both Swift and Kotlin tests reproduce the commitments from the vectors and verify `prove_democracy_v2`'s output.

### 7.3 Relayer integration

- `relayer/tests/democracy_dispatch.rs` — POST to the polymorphic `/update_commitment` endpoint with a known proof + `UpdateCommitmentPublicInputs::Democracy{...}` payload, assert it's forwarded to the stellar CLI with the correct `--public-inputs-file-path` content (the discriminated-union JSON shape per §10 Q4).

### 7.4 iOS / Android integration

- iOS: `StellarChatTests/DemocracyMembershipTests.swift` — boots an in-memory `OnChainService` mock, drives a 1 → 2 member flow, asserts `ContractClient.updateCommitment` is called with the `Democracy` variant of `UpdateCommitmentPublicInputs` (not `Anarchy`). Asserts slot indices are assigned and persisted; bitmap is derived correctly from the canonical member list.
- Android: parallel test in `app/src/test/kotlin/.../DemocracyMembershipTest.kt`.
- **Feature-flag-off behavior.** Because the FFI symbol (`sep_generate_democracy_update_proof_v2`) and Swift/Kotlin bindings are conditionally compiled out per §4.3 Layer 1, a single test target *cannot* reference the symbol when the feature is off. The test source itself wouldn't compile. Solution: a dedicated test target/scheme `StellarChatTests-NoDemocracy` (iOS) and Gradle test variant `flagOffTest` (Android), both built with the feature flag off. These targets contain *only* the negative-path tests that verify (a) the symbol is unresolved at link time (a tiny C shim probes via `dlsym` and asserts `nullptr`), and (b) `OnChainService.publishCommitmentUpdate` for a Democracy group surfaces a strict-refusal error instead of attempting to call the (absent) bridge. The main test target builds with the feature on and exercises the positive path. CI runs both.
- **Mainnet release-config CI assertion (§8.4 automation).** §8.4 lists "confirm `democracy-v2-dev-vks` is OFF in the mainnet build" as a pre-flight; given §4.3 Layer 2 is the load-bearing safeguard, this assertion gets its own automated test rather than relying on manual review. Add `.github/workflows/mainnet-release-assertion.yml` (or extend the existing release pipeline) that: (a) builds the Rust crate with `--profile release` and asserts `cargo metadata --format-version 1 | jq` shows `democracy-v2-dev-vks` is **not** in the resolved feature set; (b) runs a small integration test `tests/release_profile_no_democracy_v2.rs` that, in release-profile builds, asserts `dlsym("sep_generate_democracy_update_proof_v2")` returns null. To verify the assertion *itself* fires when it should, add `tests/ci/mainnet_release_assertion_self_test.rs` (a meta-test): construct a hypothetical release build with the feature force-enabled (via a test-only Cargo cfg), run the assertion script against it, assert it exits non-zero. Without this self-test, the assertion is "load-bearing but unverified" — a config bug could silently disable the safeguard. CI runs the meta-test on every PR touching the release pipeline or the feature flag.
- **§4.3 Layer 2 client-side fingerprint mismatch.** iOS: `StellarChatTests/DemocracyVkFingerprintTests.swift` — stub `OnChainService` to return a VK fingerprint that is *not* in the hardcoded allowlist. Drive the 1 → 2 flow. Assert (a) `RustBridge.generateDemocracyUpdateProofV2` is **never** called, (b) `ContractClient.updateCommitment` is never called, (c) the client surfaces "ceremony complete — release the production client", (d) local state is not advanced. Android parallel. Covers the "VK installed but fingerprint mismatched" leg; the "VK absent entirely" leg is §7.6.1; the "wrong VK installed against real chain" leg is §7.6.2.
- **Slot back-channel test.** iOS: `StellarChatTests/DemocracySlotDistributionTests.swift` — simulate §4.1.3: acceptor publishes a state update with `slotIndex` for the joiner; joiner stores and reuses it. Variants: (a) joiner offline during broadcast, recovers via `SEPSaltResponse` carrying the member list; (b) acceptor crashes after chain publish and before broadcast, restarts, re-broadcasts on next launch (asserts the `lastBroadcastEpoch` marker drives the recovery). Android parallel.
- **Concurrent-acceptor rollback.** iOS: `StellarChatTests/DemocracyConcurrentAcceptorRollbackTests.swift` — two `OnChainService` mocks both processing the same `SEPMemberJoined`, only one chain publish allowed to win. Assert loser reverts to baseline, persists winner's slot assignment via the eventual `SEPGroupStateUpdate` broadcast, no double-counted member, no premature broadcast from loser before chain publish settled. Android parallel. Covers §4.1.4.
- **Bitmap derivation determinism.** iOS: `StellarChatTests/DemocracyBitmapDerivationTests.swift` — given a fixed `[SEPGroupMemberLeaf]` (with mixed active/tombstone slots), assert `canonicalizeBitmap(members)` produces the byte-identical bitmap as the Rust prover and the Kotlin client (cross-checked via the §7.2 vector). Regression test for the bitmap-derivation desync risk in §9.
- **`UpdateCommitmentPublicInputs` discriminator serialization.** iOS: `StellarChatTests/UpdateCommitmentDiscriminatorTests.swift` — for each governance type (Anarchy, Democracy, Oligarchy-when-it-lands), construct a `ChatGroup` instance and call the relayer-payload serializer. Assert: the discriminant in the encoded JSON matches `group.groupType.rawValue` byte-for-byte. Specifically, a Democracy group MUST NOT emit an `Anarchy` variant payload (which would surface as `chainRejected` from §4.7.4's contract-side type-confusion guard, costing a chain round-trip and bad UX). Android parallel test in `app/src/test/kotlin/.../UpdateCommitmentDiscriminatorTest.kt`. The §6 step C2 contract test covers contract-side rejection if a malformed discriminant slips through; this client-side test ensures the discriminant is well-formed at the source.
- **BootstrapPayload threshold validation.** iOS: `StellarChatTests/BootstrapPayloadThresholdTests.swift` — three cases:
  1. *Democracy bootstrap with `thresholdNumerator == nil`*: payload received from a peer (or a malformed local construction) lacking the required field. Assert the parser rejects with `Error::MissingThreshold` (a normative client-side error per §4.4) and the local group is **not** persisted. UX surfaces "incoming Democracy invitation is malformed; cannot accept."
  2. *Democracy bootstrap with `thresholdNumerator` out of range* (e.g., `0` or `101`): assert rejection with the same error. Validates the `1..=100` parser-side check.
  3. *Default-50 path*: a Democracy group created via the client UI without explicitly choosing a threshold. Assert that the constructed `BootstrapPayload.thresholdNumerator == 50` (the default per §4.7.6) — the C1 contract spec accepts default-50 as the API-level fallback, and clients consistently emit 50 rather than `nil` for Democracy bootstraps.

  Android parallel: `BootstrapPayloadThresholdTest.kt`.

### 7.5 Manual testnet end-to-end

Pre-condition: Phase C complete (fresh testnet contract deployed at the new address; `scripts/install-democracy-vks-testnet.sh` run against it; v2 dev VKs for all 3 tiers installed).

Steps:

1. iOS creates a Democracy group (medium tier), publishes via `create_group_v3` against the new contract. Verify on Soroban testnet that `current.group_type == 2` and `current.occupancy_commitment` is set (no `member_count` field — the contract storage layout no longer carries plaintext counts).
2. iOS sends an invitation to Android.
3. Android accepts. Android sends `SEPMemberJoined`.
4. iOS receives the SMJ, runs `applyStateUpdate` → `publishMemberUpdate` → v2 democracy path → polymorphic `update_commitment` with the Democracy variant. Verify the relayer log shows `function=update_commitment`, status 200, and the public-inputs payload carries `occupancy_commitment_old`/`occupancy_commitment_new` (not counts).
5. iOS local state advances to epoch 1, 2 members. Slot indices: iOS at 0, Android at 1.
6. Android receives the broadcast, persists `slotIndex=1` for itself.
7. Android sends a chat message tagged epoch 1. iOS decrypts, BLS check passes, message displays.
8. iOS sends a chat message. Android decrypts and displays.

Privacy verification (the §3.4 "no metadata leaks" claim):

- Read the contract storage entry for this group via Soroban testnet RPC. Assert `member_count` is **not present** anywhere in the entry (would be a layout regression).
- Assert `threshold_numerator` IS present in storage (`current.threshold_numerator == 50` for the default), so the verifier can read it on the next update. This is the §4.7.6 contract-supplied public input — the chain stores it, doesn't expose it on call payloads.
- Inspect the relayer log for the `update_commitment` call: assert no field named `member_count_*` or `threshold_*` appears in the request body or stellar CLI args. Only `c_old`, `epoch_old`, `c_new`, `occupancy_commitment_old`, `occupancy_commitment_new` (5 scalars).
- Run a second 2 → 3 transition. Assert the on-chain trace for the second `update_commitment` call carries the same 5-scalar public-inputs shape as the first (no count field, no threshold field, only occupancy commitments). A chain observer **cannot tell from these calls** whether the group went 1→2 or 5→6 — both produce a 5-scalar payload with hash-shaped occupancy commitments. The observer **can** still distinguish a Democracy update call from an Anarchy update call by the scalar count (5 vs 3); see §3.4 residual on entrypoint-selector partial-solution. The privacy claim is "no count or trajectory leakage *within* a group's update history," not "no group-type leakage" — the latter is acknowledged residual.

**Non-default threshold path (extension of the above):**

9. Create a *second* Democracy group at medium tier with `threshold_numerator = 67` (two-thirds supermajority). Verify on Soroban testnet that `current.threshold_numerator == 67`. The group is created with member_count = 1 (creator only).
10. Add a second member (1 → 2). Single-signer subset works (`100·1 ≥ 67·1` passes). Verify chain advance.
11. Attempt to add a third member (2 → 3). Per §4.2's table, this requires `K ≥ ⌈67·2/100⌉ = 2`, but only K=1 is available in the single-signer subset. Assert the prover returns `QuorumRequired` *before* generating a proof (no chain round-trip wasted). UX surfaces "this group's threshold of 67% requires at least 2 admin signatures for groups of size 2; multi-signer not yet supported."
12. Privacy assertion specific to the threshold path: read the on-chain trace for the 1 → 2 transition above. Assert that the call's public-inputs payload is **byte-equivalent in shape** to the same transition under default 50% from steps 1-8 — the threshold is in storage, not on the wire, so a chain observer cannot tell from these two transactions which group runs at 50% vs 67%. Only direct storage inspection reveals the threshold value.

Failure-mode tests:

- §4.3 Layer 2: hand-edit the dev VK fingerprint list to NOT include the testnet's installed VK; observe that the client refuses to perform the chain update and surfaces the expected error.
- Quorum required: after the group reaches 3 members, attempt a 4th member add; expect `QuorumRequired` surface and a "multi-signer not yet supported" UX error (no silent failure). Phase F is the unblocker.
- Old contract abandonment: an iOS build pointing at the old contract `CC6N…RWWKE` attempts to create a Democracy group; assert the client surfaces "this contract is no longer supported, please update" rather than attempting the call.

### 7.6 VK-mismatch handling, end-to-end (two scenarios)

These exercise VK-mismatch rejection at two different fidelity levels. **7.6.1** is a true contract-side test: the client tries to publish, the contract refuses, the client surfaces the error. **7.6.2** is a client-side test (parallel to §7.4's `DemocracyVkFingerprintTests`) that uses a real Soroban testnet contract as the VK source — the client refuses to publish before the relayer is even called. They cover the same code path with different fidelities; the §7.4 stub-based test exercises the same client logic faster (no chain round-trip) and the §7.6.2 contract-anchored test verifies that the real on-chain `read_vk` plumbing works as the §7.4 stub claims it does.

The §7.4 / §7.6.2 fidelity overlap is intentional: both tests must pass, both must continue to test "the client's runtime fingerprint check refuses to publish on mismatch." If they drift (e.g., §7.4's stub returns a payload shape that the real contract doesn't), §7.6.2 catches the drift. CI runs §7.4 on every PR and §7.6.2 on a slower cadence (nightly or pre-release) since it requires a testnet contract.

**7.6.1 No VK installed (true contract-side fail-closed).** Pre-condition: a fresh testnet contract is set up *without* running the dev VK install. The democracy update entrypoint exists but no VK is registered for `(tier, group_type=2)`.

Run the same flow as 7.5. The contract should reject with `VkNotInitialized` (or analogous), the client should NOT silently fall back to non-chain-anchored chat, and the user should see a "this group requires updates that the network doesn't support yet" error.

**7.6.2 Wrong VK installed (client-side Layer 2, contract-anchored fidelity).** Pre-condition: a fresh testnet contract has a VK installed at `(tier, group_type=2)` that is **not** in the dev fingerprint allowlist — e.g., a one-off ceremony VK from a parallel chain, or a VK fingerprint deliberately rotated to simulate a future production deployment.

Run the same flow as 7.5. Assert: the client's runtime fingerprint check (§4.3 Layer 2) reads the on-chain VK, computes the fingerprint, finds no match in the allowlist, surfaces the "ceremony complete — release the production client" error, and never reaches the relayer. The relayer logs show no `update_commitment` call for this group during the test. (Note: this test verifies client refusal happens, but the *path* is client-side — not contract-side — so the §7.6 header now reads "VK-mismatch handling, end-to-end" rather than "contract-side fail-closed" to be accurate.)

---

## 8. Rollout (cross-phase coordination)

§6 lays out *what* lands. This section says *when* each phase can start and what gates the transition between phases.

### 8.1 Phase entry/exit gates

| Phase | Entry trigger | Exit gate (must hold before next phase fully merges) |
|---|---|---|
| A — Circuit | This design doc lands (PR #146 merged) | All A-phase tests green; R1CS constraint count within 10% of §4.7.3 estimate; cross-platform vectors checked in |
| B — FFI/bindings | A1–A5 merged (circuit shape stable; vectors not strictly required, can land in parallel with A6) | XCFramework + Android NDK rebuilt; bridge round-trip tests pass against A's vectors |
| C — Contract | A1–A4 merged (need v2 VKs to install) — can run **in parallel with B** | Fresh testnet contract deployed; smoke test via stellar CLI + Phase B proof bytes passes; old contract `CC6N…RWWKE` documented as deprecated |
| D — Relayer + clients | B and C exit gates met | Two-device manual smoke test (§7.5) passes against the new contract; CI green; old contract not used anywhere in client builds |
| E — Ceremony | A's circuit hash signed off by external R1CS reviewers; D shipped to testnet for ≥2 weeks (dogfood window) | Ceremony attestations published; production VKs installed on a separate mainnet contract; mainnet build CI assertion (`democracy-v2-dev-vks` OFF) passes |
| F — Multi-signer | Phase F design doc lands (separate PR) | (Out of scope for this rollout — gates a *separate* mainnet release with multi-signer support) |

A→B/C parallelism is the calendar saver: B and C can both start as soon as A's circuit shape stabilizes (around A2 / A3), even before A6 (cross-platform vectors) is final.

### 8.2 Testnet contract address management

The new contract address is the single coordination point between Phase C and Phase D. Process:

1. **Phase C operator deploys** via `scripts/deploy_sep_xxxx_testnet.sh`, captures `C…` address from the output.
2. **Update `RelayerDefaults.contractID`** at build time via the existing `relayer/.env` → `RelayerDefaults.generated.swift` pipeline (and the Android equivalent). The `relayer/.env.example` is updated in a single commit; CI sync ships it to the build env.
3. **Phase D clients ship** with the new contract baked in. Old clients pointing at `CC6N…RWWKE` continue to fail; the §3.4 "old contract abandoned" UX surfaces.

The testnet redeploy is one-shot (no live users to coordinate). If a future redeploy is needed (e.g., contract bug discovered), the same triplet runs again — a Phase C-style redeploy + Phase D client release. This is the same friction as the existing dev-VK rotation runbook in v0.3, generalized.

### 8.3 Dev-VK fingerprint allowlist (still load-bearing)

Phase D bakes the dev-VK fingerprints into the client per §4.3 Layer 2. The fingerprint coordination process from v0.3 §8.3 carries forward unchanged:

1. After Phase A4 regenerates the v2 dev VKs, compute SHA-256 fingerprints of each tier's `verifying_key.bin`.
2. Write to `keyset-democracy-dev/fingerprints-v2.json` (single source of truth).
3. Generate `swift-mls/Sources/SwiftMLS/DemocracyVkFingerprints.swift` and `kotlin-mls/.../DemocracyVkFingerprints.kt` from that JSON via a small build-time script.
4. Phase D ships the generated constants in the client release.

Future testnet VK rotations (e.g., circuit bug fix during Phase A iteration) require regenerating the fingerprint file and a client patch release — same as v0.3.

### 8.4 Mainnet release pre-flight

Phase E ends with a mainnet release. The existing `breaking-changes-release-process.md` runbook is updated with:

- Confirm `democracy-v2-dev-vks` feature is **OFF** in the mainnet build (CI assertion in PR builds)
- Confirm mainnet build's `RelayerDefaults.contractID` points at the **mainnet** contract address (not the testnet one)
- Confirm production VK fingerprints (from the ceremony) are baked in, **replacing** the dev allowlist for mainnet builds — not appending. A mainnet client that accepts a dev VK fingerprint is the failure mode the §4.3 Layer 2 safeguard is designed to prevent; the allowlist swap is what makes the safeguard tight.

---

## 9. Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Dev client run against mainnet contract | Low | High (user actions performed against unverified circuit) | §4.3 Layer 2 fingerprint check at runtime; clients refuse to act on fingerprint mismatch. Mainnet build also drops the dev allowlist entirely (§8.4). |
| Phase 2 ceremony review surfaces a soundness issue in `democracy_v2.rs` | Medium | High (re-circuit + re-ceremony) | §6 Phase A's R1CS soundness review precedes the ceremony. The dev-key dogfood window in Phase D before Phase E is the natural place to surface integration issues that imply circuit changes. The constraint to "no count-public intermediate" means a circuit issue blocks mainnet but doesn't break testnet — the dev path keeps working with the dev-key VKs. |
| Quorum-required cases (member_count_old ≥ 3) hit production usage | Medium | Medium (groups can't grow past 3 without Phase F) | UX surfaces "this needs a vote, not yet implemented" message. Phase F is its own design doc + multi-month effort; gates the second mainnet release, not the first. |
| Testnet contract loses VK across redeploys | High (per existing memory of testnet rotations) | Medium (feature breaks until script re-run + client release) | §8.3 spells out the coordinated triplet (install → regenerate fingerprints → release client). Acceptor restart re-broadcast (§4.1.3) covers the in-flight case. Rotations are now expected to be rare since the v2 circuit is the design target. |
| Old clients in the field after Phase D ships | Low (no live users today; only contributor builds) | Low (old clients pointing at the abandoned contract see "this group's contract was abandoned" UX) | §3.4 residual: tier upgrades visible. Acknowledged. The contract-redeploy banner is the user-facing recovery path. |
| Tier upgrades remain visible on chain | Medium | Low (one-bit-per-upgrade signal; weaker than count-public but still observable) | Operators size tiers conservatively at create time so upgrades are infrequent. Universal-circuit redesign that hides tier is a research project deferred indefinitely. |
| Acceptor crashes after chain publish but before broadcast (single-leg) | Medium (a crash can happen between any two operations; the window is small but not vanishing) | Low (auto-recoverable on acceptor restart via the `lastBroadcastEpoch` marker, no peer involvement needed) | Acceptor persists `lastBroadcastEpoch` marker before chain submit; on next launch detects "I committed but didn't broadcast" and re-publishes. Tested in §7.4. |
| Acceptor crashes AND joiner is offline at the same time (compound, "permanent loss" case) | Low (requires both legs to occur within the broadcast window) | Medium (recoverable only via manual peer reconstruction from the on-chain `occupancy_commitment` + persisted bootstrap state) | Manual support recovery procedure documented; acceptor restart re-broadcast (§4.1.3) is the primary defense, the manual procedure is the safety net. |
| Two acceptors race the same `SEPMemberJoined` | Medium | Low (rollback path in §4.1.4 handles this; loser reverts local state to baseline before broadcast) | §4.1.4 rollback path; tested in §7.4 `DemocracyConcurrentAcceptorRollbackTests`. Loser MUST hold broadcast until chain confirms. |
| Domain-tag collision across circuits | Low | High (cross-circuit proof reuse if `DOMAIN_MEMBER`/`DOMAIN_TOMBSTONE`/`DOMAIN_OCCUPANCY` collide with values used in `MembershipCircuit` or `UpdateCircuit`) | §10 Q3 — explicit cross-circuit audit pass before circuit lock-in. Domain tag values pinned in a single header file (`src/circuit/domain_tags.rs`) with collision-detection assertions in each circuit's setup. |
| Bitmap-derivation desync (client computes bitmap from member list, but two clients disagree on whether a slot is "active" or "tombstoned") | Low | High (desync produces incompatible occupancy commitments) | Bitmap derivation rule is normative (§4.7.1): `bitmap[i] = 1 iff leaf[i] is `Poseidon(DOMAIN_MEMBER, ...)` after domain-tag check`. Single-source helper (`canonicalizeBitmap` in `swift-mls`/`kotlin-mls`/Rust). Cross-platform test vectors (A6) cover the boundary cases. |
| Public-input ordering drift between client prover and contract verifier (the IC-vector position of `threshold_numerator`, occupancy commitments, etc.) | Low | High (silent verify failure on every Democracy update; impossible to debug without diffing IC vectors) | Single source of truth: `docs/soroban-contract-test-vectors.json` `vk_kind_enum.UpdateByType(2).ic_layout_v2` pins the canonical 7-element IC layout `[base, c_old, epoch_old, c_new, occupancy_commitment_old, occupancy_commitment_new, threshold_numerator]`. Phase A6 cross-platform test vectors include a fixture that exercises the full IC layout end-to-end. CI assertion compares the Rust prover's emitted public-input vector against the contract's expected layout per the test-vector fixture. |
| `threshold_numerator` storage layout differs between contract redeploys (e.g., different operators independently deploy v2 with subtly different `CommitmentEntryV3` shapes) | Low | High (cross-deployment incompatibility; clients pointed at the wrong contract see opaque verify failures) | Storage layout pinned in `docs/soroban-contract-test-vectors.json` `storage_layout_v2.commitment_entry_v3`. Future contract revisions that reorder or rename fields require a doc update + a new test vector entry; CI's `cargo expand` on the contract crate is grep-asserted against the pinned layout to catch silent drift. |

---

## 10. Open Questions

The questions below are split by their position in the dependency graph. **Phase-A blockers** must be ratified before Phase A1 (circuit) PR opens — the values they pin are inputs to the circuit shape itself and changing them after VK generation invalidates the dev VKs. **Other open questions** are tracked but don't block any specific phase entry.

### 10.1 Phase-A blockers (must ratify before A1 opens)

1. **Domain tag values across circuits (Phase A1 input).** §4.1.1 / §4.7.2 propose:
   - `DOMAIN_MEMBER = Fr::from(1)`
   - `DOMAIN_TOMBSTONE = Fr::from(2)`
   - `DOMAIN_OCCUPANCY = Fr::from(3)`

   These are the recommended values pending one-line confirmation in A1. The audit work for ratification: search `src/circuit/` for any other `Fr::from(N)` in a hash-input position (`MembershipCircuit`, `UpdateCircuit`, future `OligarchyCircuit`). At the time of writing, no other circuit uses domain tags, so values 1/2/3 are safe to claim. The ratification step (1 hour of work, mostly grep): confirm the claim against current `main`, codify the constants in a new `src/circuit/domain_tags.rs` with `const_assert!` style uniqueness checks, and reference that file from each circuit. **If a future circuit (e.g. Oligarchy) needs more domain tags, it picks values 4+, never reusing 1/2/3.** The §6 Phase A1 step opens with this domain-tags PR; the rest of Phase A depends on it.

2. **Tier-2 (Large) `k_max` value (Phase A4 input).** Phase A4 generates the Large-tier dev VK. The democracy circuit's `2K ≥ m_old` constraint at `m_old = 2048` (Large tier capacity) requires `k_max ≥ 1024` for the maximum-size case to be provable. **Recommendation: cap `k_max` at 256 for the Large dev VK**, the same as Medium. The democracy single-signer subset (§4.2) only uses K=1 for groups of size ≤ 2; the Phase F multi-signer follow-up needs to scale K with `m_old` but is out of scope here. Capping `k_max = 256` means a Phase F effort that wants to support a 2048-member Large-tier group with full quorum will need to either redesign for partial-aggregation proofs (reasonable; the natural Phase F shape) or regenerate the Large VK with `k_max = 1024` (a separate ceremony, expensive). The cap is an explicit limitation and is documented in §4.6 alongside the "Large is testnet-only-with-cap" note. **Phase A4 ratifies `k_max = 256` for Large unless a different rationale surfaces during A1's circuit construction.**

### 10.2 Other open questions (do not block phase entry)

3. **Slot reuse policy after group reset.** If a group is deactivated and a new one created with the same `groupSecret`, do slot indices reset? (Probably yes — new group is a new contract entry.) Need to confirm with the deactivate-group flow before Phase D. §4.1.2 already foreshadows this as an exception to "monotonic, never forget" — the recommendation here is "yes, reset," confirmed during D5 client-state work.

4. **Backporting slot-index + occupancy commitment to Anarchy?** The v2 design could in principle replace Anarchy's public-key-sorted multi-leaf-delta `UpdateCircuit` too, unifying the codebase on a single update circuit shape. Trade-off: Anarchy doesn't currently leak count, but its circuit is simpler and ceremony has run for it. Revisit at the start of Phase E, since the ceremony plan there decides whether Anarchy and Democracy share a circuit family.

5. **`UpdateCommitmentPublicInputs` enum encoding on the wire.** The §4.4 Codable shape uses Swift's natural enum-with-associated-values encoding. The Soroban contract receives the *flat* fields (5 scalars for Democracy variant — threshold is contract-supplied per §4.7.6, not on the wire). The relayer's job is to translate. Recommendation: discriminated union with the `group_type` discriminant explicit on the wire (matches the §4.7.4 type-confusion guard). Resolved in D1 (relayer dispatch).

6. **Threshold UX picker — accept any integer 1–100, or whitelist common values?** Phase D's create-group UI surfaces the threshold control. Options: (a) free-form integer entry, validated `1..=100`; (b) preset buttons for common ratios (50, 67, 75, 100) plus a "custom" affordance; (c) presets-only, no custom path. Recommendation: (b) — friendlier UX, no functional restriction. Doesn't affect the protocol; cosmetic-tier decision but worth committing to a default. The contract / circuit accepts the full range regardless of which UX path is chosen.

(Items resolved in v0.4 vs prior versions: count-leak privacy regression (§3.4 → no longer accepted; circuit redesign now in §4.7); pre-design migration (no longer needed — contract redeploy in Phase C makes pre-design state moot); polymorphic dispatch (was §11 follow-up, now §4.7.4 in core design); tier-uniform salt-response padding (was acknowledged residual, now §4.7.5 in core design).)

---

## 11. Follow-Up Work

- **Phase F — Multi-signer democracy proofs.** Quorum-collection design doc (proposal lifecycle, vote tally, K-of-N partial-proof aggregation). Implements the `K ≥ ⌈m_old / 2⌉` general case. Required before Democracy groups can grow past 3 members. Multi-month effort with its own design doc; out of scope for v0.4. Phase F design must arrive **before** the second mainnet Democracy release.
- **Phase E — Phase 2 trusted-setup ceremony for v2 circuit.** Tracked in [`democracy-circuit-ceremony.md`](democracy-circuit-ceremony.md), updated to v2-only scope. Three-tier ceremony, MPC, attestations. Mainnet blocker. ~4–8 weeks calendar.
- **Oligarchy update path.** Per-type circuit + VK; admin-only quorum. Slots into the polymorphic `update_commitment` (§4.7.4) once its circuit and ceremony land. Documented in [`group-governance-types-design.md`](group-governance-types-design.md). Independently schedulable; not blocked by anything in this design after Phase D.
- **iOS sync-lock UI integration with the v2 path.** The UI lock from [`fix/epoch-sync-and-ui-lock`](https://github.com/rinat-enikeev/stellar-mls/pull/145) generalizes to "any pending chain transition." Phase D5/D6 wires `awaitingChainConfirmation` into the new democracy publish path; the banner surface is unchanged.
- **Backport unified update circuit to Anarchy.** Per §10 Q2, evaluate at the start of Phase E whether Anarchy should also adopt the slot-index + occupancy-commitment shape. If yes, would unify ceremony work but requires a coordinated migration of existing Anarchy groups (which DO have users in the broader Onym roadmap context). If no, keep Anarchy on its current `UpdateCircuit`. Decision deferred; not gating Phase A–D.
- **Eliminating tier-upgrade visibility.** §3.4 acknowledges this as the only material residual leak after this design lands. Eliminating it requires either fixed-size groups (massive overhead) or a universal-circuit redesign (research project). Not on any near-term roadmap; tracked as a long-tail privacy-improvement item.
- **Threshold rotation.** §4.7.6 fixes `threshold_numerator` at group creation; changing it requires recreating the group. A future feature would let an existing group vote to change its own threshold (a meta-governance operation). Design challenges: the meta-vote itself uses *some* threshold to pass — the existing one, the new one, or a higher fixed bar like unanimous? Each choice has a different trust story. Documented as a Phase G candidate; not on the immediate roadmap.
