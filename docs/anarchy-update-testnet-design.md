# Anarchy Per-Type Contract — Testnet Implementation Design

**Date:** 2026-04-27
**Status:** Draft (Proposal — pre-implementation)
**Author:** Onym contributors
**Version:** 0.1 — initial design for the per-type Anarchy contract (`sep-anarchy`), parallel to `sep-democracy` (PR #148, design v0.5.1) and `sep-oligarchy` (PR #149, design v0.1.4).
**Supersedes:** (none — first iteration)
**Related:**
- [`democracy-update-testnet-design.md`](democracy-update-testnet-design.md) — sibling design; per-type architecture decisions, slot-index convention, ceremony framing, and testnet gating are inherited verbatim and cross-referenced rather than duplicated below.
- [`oligarchy-update-testnet-design.md`](oligarchy-update-testnet-design.md) — sibling design; verbose-create binding pattern + deploy-script hardening rules cross-referenced.
- [`group-governance-types-design.md`](group-governance-types-design.md) — parent design that introduces `groupType ∈ {Anarchy=0, OneOnOne=1, Democracy=2, Oligarchy=3}`.
- `contracts/sep-xxxx/src/lib.rs:917-937` (`create_group`), `:973-1129` (`create_group_v2`), `:1318-1400` (`update_commitment`). Anarchy is the only governance type that already has full create + update + deactivate support in v1 sep-xxxx; this design extracts that surface into a focused per-type crate.

---

## 1. Background

Anarchy groups (`groupType = 0`) are the simplest governance type in the SEP family. Any current member may submit an `update_commitment` proof and advance the group state — no quorum, no admin set, no member-count threshold. Authorization is purely cryptographic: the prover must demonstrate knowledge of a secret key behind a member leaf in the current commitment's tree. There is no co-signer mechanism, no voting, and no on-chain enforcement of governance policy beyond the Groth16 proof itself.

`contracts/sep-xxxx/src/lib.rs` already implements the full Anarchy lifecycle:
- `create_group` / `create_group_v2` accept `(commitment, tier, member_count, group_type=0, …)` and verify a 3-IC membership proof.
- `update_commitment` accepts `(c_old, epoch_old, c_new)` and verifies a 4-IC update proof against `current.commitment` / `current.epoch`.
- ~~`deactivate_group` accepts a membership proof against the current state and flips `active = false`.~~ **Removed in postmortem #153** — the entrypoint had a front-running vulnerability that could not be closed at the contract level (any observer of an honest `verify_membership` call could replay the leaked proof to permanently freeze the group). See `docs/postmortem-deactivate-group-frontrun.md`.
- `verify_membership` is read-only.

This works correctly. There is no Anarchy "missing entrypoint" problem analogous to what Democracy and Oligarchy faced before their respective design docs.

## 2. Problem

The monolithic `contracts/sep-xxxx` carries dispatch overhead, storage fields, and error variants for **every** governance type on **every** Anarchy operation. An Anarchy update pays for `match group_type` branching, an unused `admin_root` storage slot in the `CommitmentEntry` shape's reservation, and ABI surface for `MissingAdminRoot` / `Invalid1v1Tier` / `MemberCountMismatch` errors that Anarchy will never produce. The unused `group_type` storage discriminator costs 4 bytes per `CommitmentEntry` even though the value is always `0`.

Beyond WASM bloat, the bigger cost is auditability: Anarchy's privacy floor is the simplest of any governance type (no count hiding, no quorum hiding, no admin-set hiding), but reading the sep-xxxx code requires mentally subtracting Democracy / Oligarchy / 1v1 logic to identify what Anarchy actually does. A focused `sep-anarchy` crate makes the type's privacy + soundness arguments standalone-auditable.

This design specifies the per-type extraction. **No new functionality.** **No v2 improvements over v1.** Pure refactor: drop the multi-type dispatch surface, mirror v1 Anarchy logic verbatim, and ship at a fresh contract address with the existing v2 keyset.

## 3. Constraints

The five constraints. The first two are inherited from Democracy + Oligarchy; the remaining three are Anarchy-specific (and lighter — Anarchy is the protocol's minimum case).

### 3.1 The circuit is the v1 baseline (no Phase A circuit work)

Unlike Democracy v0.5 + Oligarchy v0.1.4 — which both required **new** v2 circuits with occupancy-commitment binding and (for Oligarchy) two-tree dispatcher logic — Anarchy reuses the existing `keyset-v2/vk-{small,medium,large}.json` + `keyset-v2/vk-update-{small,medium,large}.json` circuits. These are the same ones the monolithic `sep-xxxx` Anarchy paths verify against today.

Phase A work is **zero LOC** for Anarchy. No circuit, no prover, no fixture generation. The deploy script consumes the `keyset-v2/` directory directly.

### 3.2 The Phase 2 ceremony has not run

Same as Democracy + Oligarchy. `keyset-v2/` is the dev keyset. Production VKs land via the same MPC ceremony that produces Democracy + Oligarchy production keys (per [Democracy §6 Phase E](democracy-update-testnet-design.md#phase-e--ceremony-coordination-extends-democracys-phase-e)) — Anarchy's existing dev VKs are simply rotated at ceremony time.

### 3.3 No metadata leakage beyond v1 (informational `member_count` preserved)

Anarchy's privacy floor is the simplest of the four types. The `member_count` field on `CommitmentEntryV2` was introduced in v1 sep-xxxx as informational (`0` means "not tracked" per the parent governance-types design §6.2.1). For Anarchy specifically, `member_count` is **not authoritative** — the contract does not enforce member-count thresholds, range checks, or quorum denominators against this field.

This design preserves `member_count` on storage as an informational field for parity with v1. It is documented as a publicness-class equal to `tier` (visible to chain observers; not zero-knowledge-hidden). Operators who don't want to publish a count can pass `0` (the design's documented "not tracked" sentinel), matching v1 behavior. **No upgrade path that hides the count is in scope** for this design — Anarchy's contract simply doesn't claim to hide it. (A future design that introduces v2-style occupancy commitment for Anarchy is §11 follow-up.)

#### 3.3.1 What this design does NOT hide (acknowledged residuals)

| Residual | Why it leaks | Mitigation in this design |
|---|---|---|
| The group is Anarchy-typed | `group_type=0` is implicit in the contract address (per-type architecture); same publicness class as `tier` | Per-type contract makes the group_type observable as the contract address itself. Wire payload (3 scalars) differs from Democracy's / Oligarchy's 5 scalars, but this is downstream of the address-as-discriminator and adds no incremental information. |
| Member count | `member_count` field on every `CommitmentEntry` is publicly readable from chain storage | Acknowledged. Operators set `0` to opt out. Phase 2-style occupancy-commitment hiding is §11 follow-up if a future design wants to close this. |
| Update cadence | Timestamps + epoch counters on every successful `update_commitment` | Same residual as Democracy + Oligarchy; intrinsic to public ledger. |
| Group existence + tier | Visible from `Group(group_id)` and per-tier `GroupCount` storage entries | Same residual as Democracy + Oligarchy. |

**No new privacy axes vs v1.** The §3.4 Democracy / §3.5 Oligarchy "hidden member counts" / "hidden which tree changed" claims do not apply to Anarchy — the contract makes weaker privacy claims, and the v1 implementation already meets them.

### 3.4 Single signer (no co-signer mechanism)

Anarchy's UpdateCircuit takes a single witnessed secret key; the proof asserts the prover knows one member's key. No K-of-N quorum, no multi-signer aggregation, no admin-quorum subset. This matches v1 Anarchy. Multi-signer Anarchy is not on the roadmap (it would functionally be Democracy with `threshold_numerator = 1/N`); operators who want quorum semantics use the Democracy contract instead.

### 3.5 Permanent tombstones (inherited)

Same constraint as [Democracy §3.3](democracy-update-testnet-design.md#33-single-leaf-delta-requires-a-stable-slot-index-convention). Tombstoned slots are permanent within a group's lifetime. Slot-index convention from Democracy applies — and the `member_count` informational field tracks the *active* count, not the *lifetime* count.

---

## 4. Design

### 4.1 Slot-index convention (inherited)

Inherited from [Democracy §4.1](democracy-update-testnet-design.md#41-slot-index-convention-resolves-§33). Anarchy's member tree carries the same slot-index assignment, the same `SEPGroupMemberLeaf.slotIndex: UInt32?` Codable field, and the same tombstone-permanence rule.

#### 4.1.1 Domain separation (subset of Democracy's)

Anarchy uses only three domain tags from the [Democracy §4.1.1 set](democracy-update-testnet-design.md#411-domain-separation-extends-democracy-§411):

| Tag | Value | Purpose |
|---|---|---|
| `DOMAIN_MEMBER` | `Fr::from(1)` | Member-tree leaf prefix: `Poseidon(DOMAIN_MEMBER, Poseidon(sk))`. |
| `DOMAIN_TOMBSTONE` | `Fr::from(2)` | Tombstone leaf constant. |
| (no `DOMAIN_OCCUPANCY`) | — | Anarchy doesn't carry an occupancy commitment. The `DOMAIN_OCCUPANCY=3` tag exists in the cross-circuit registry but Anarchy's circuit doesn't reference it. |

§10 cross-circuit domain-tag audit covers values 1–6 across all governance types; Anarchy's subset does not introduce any new tags.

### 4.2 Single-signer authorization (no quorum)

Anarchy has no `threshold_numerator`. Constraint #4 in Anarchy's circuit is `K = 1` — exactly one signer. There is no parameterized formula like `100·K ≥ threshold_numerator · popcount(...)` because Anarchy has no notion of "what fraction of members must approve."

The prover-side preflight is simply: "do I know a member-leaf secret key?" If yes, generate the proof. If no, reject. There is no `QuorumRequired` failure mode equivalent to Democracy / Oligarchy.

#### 4.2.1 Circuit invariants vs. contract invariants (subset)

| Invariant | Circuit-enforced | Contract-enforced | Notes |
|---|---|---|---|
| `c_old = Poseidon(Poseidon(root_member, epoch_old), salt_old)` | ✓ | (binding via public input) | Single-tree commitment. |
| `c_new = Poseidon(Poseidon(root_member_new, epoch_old + 1), salt_new)` | ✓ | (binding via public input) | Same shape, advanced epoch. |
| The signer's secret key opens to a leaf in `root_member_old` | ✓ | — | Standard membership witness. |
| `K = 1` (single signer; no co-signer aggregation) | ✓ | — | Anarchy specifically. |
| Single-leaf delta (≤1 leaf change per update) | ✓ | — | Inherited from Democracy. |
| Tombstone domain separation | ✓ | — | Inherited. |
| `c_old == current.commitment`, `epoch_old == current.epoch` | — | ✓ | State chaining. |
| `c_new` canonical Fr encoding | — | ✓ | InvalidCommitmentEncoding gate. |
| Proof is fresh (replay-window check) | — | ✓ | Same SHA256(a‖b‖c) global nullifier as Democracy. |

No `member_count` constraints — the field is informational.

### 4.3 Testnet-gating mechanism (inherited from [Democracy §4.3](democracy-update-testnet-design.md#43-testnet-gating-mechanism))

Same two-layer scheme:
- **Layer 1**: Cargo feature `anarchy-v2-dev-vks` (off by default), gates the FFI export, Swift/Kotlin bindings, and Anarchy variant in `ContractClient.updateCommitment`.
- **Layer 2**: VK fingerprint check at runtime against a hardcoded allowlist baked into client builds. Mainnet builds drop the dev allowlist entirely.

(Note: in v1 sep-xxxx Anarchy is the *default* governance type and not feature-gated, since the existing contract shipped before the per-type architecture was decided. For the per-type `sep-anarchy` contract, the new contract address replaces the existing testnet sep-xxxx Anarchy address; clients route to the new address via the same fingerprint-allowlist mechanism Democracy + Oligarchy use.)

### 4.4 Wire / data-model

A new variant on the polymorphic `UpdateCommitmentPublicInputs` enum (Swift / Kotlin clients):

```swift
public enum UpdateCommitmentPublicInputs: Codable, Equatable, Sendable {
    case anarchy(cOld: Data, epochOld: UInt64, cNew: Data)                     // 3 scalars  ←  this design
    case democracy(                                                              // 5 scalars
        cOld: Data, epochOld: UInt64, cNew: Data,
        occupancyCommitmentOld: Data, occupancyCommitmentNew: Data
    )
    case oligarchy(                                                              // 5 scalars (per oligarchy v0.1.4)
        cOld: Data, epochOld: UInt64, cNew: Data,
        occupancyCommitmentOld: Data, occupancyCommitmentNew: Data
    )
}
```

**3-scalar wire payload** is byte-distinguishable from Democracy / Oligarchy's 5-scalar shape — but the per-type architecture means each contract has its own deployed address. Cross-contract observers see the address first; payload-shape differences are downstream of the address-as-discriminator. The §3.4-style "uniform-shape padding" structural fix (deferred for the entrypoint-selector residual in the monolithic design) collapses for per-type: each contract's payload-shape is constant within the contract, and cross-contract distinguishability is already maximal via the address.

`SEPGroupMemberLeaf.slotIndex: UInt32?` carries forward unchanged from Democracy. Same Codable wire-compat rules; same normative-leaf-hash rule (slotIndex NOT part of the leaf hash).

`BootstrapPayload` (`SEPBootstrapPayload`) for Anarchy carries:
- `members: [SEPGroupMemberLeaf]` — required.
- (No `admins` / `adminThresholdNumerator` / `memberThresholdNumerator` / `occupancySalt` — Anarchy has no admin set, no quorum threshold, no occupancy salt.)

`SEPSaltResponse.members: [SEPGroupMemberLeaf]?` carries forward; an Anarchy salt response carries only the standard `salt: Data` field (the `c_new`-salt that v1 Anarchy already uses). No occupancySalt (no occupancy commitment).

`SEPGroupStateUpdate` for Anarchy carries `members` only. Same v1 shape.

### 4.5 Relayer dispatch (inherited)

The polymorphic `update_commitment` entrypoint already covers Anarchy + Democracy + Oligarchy in the relayer's translation layer. The relayer doesn't need a per-type dispatch arm — the Anarchy variant carries 3 scalars, the contract's address is an Anarchy contract, the message is forwarded byte-for-byte.

### 4.6 Contract redeploy + VK installation

Per-type contract deploys at a fresh address. The atomic constructor takes the admin address plus the existing v2 keyset Anarchy VKs:

`__constructor(admin, vk_small, vk_medium, vk_large, update_vk_small, update_vk_medium, update_vk_large)` — 7 args total (admin + 6 VKs). Smaller than Democracy's 7 args (admin + 6 VKs same shape) and Oligarchy's 10 args (admin + 9 VKs).

`create_group(group_id, commitment, tier, member_count, …)`. Storage gains, per Anarchy group:

- `commitment: BytesN<32>` — the bundled Poseidon commitment.
- `epoch: u64`, `timestamp: u64`, `tier: u32`, `active: bool` — same as Democracy.
- `member_count: u32` — informational only. Set at create; **never updated by `update_commitment`** (the contract has no Poseidon host and cannot recompute member count from the bitmap; clients track this off-chain). Operators who don't want to publish a count pass `0`.

**No** `group_type` (per-type contract), **no** `occupancy_commitment` (Anarchy doesn't claim to hide the count), **no** `admin_root` (no admins), **no** `threshold_numerator` (no quorum), **no** `salt_occ` (no occupancy commitment).

The existing testnet contract `CC6N…RWWKE` (the monolithic `sep-xxxx`) is abandoned for Anarchy purposes. All v2 Anarchy state lives at the new sep-anarchy address.

### 4.7 No occupancy commitment (Anarchy doesn't hide counts)

Where Democracy v0.5 introduced an occupancy-commitment via `Poseidon(DOMAIN_OCCUPANCY, fold(bitmap))` to hide member counts on the wire, and Oligarchy v0.1.4 extended this with a salted combined commitment over both trees, **Anarchy does neither**. The v1 design accepted that `member_count` is publicly readable; this per-type extraction preserves that posture.

Consequently:
- No occupancy-commitment fields on the wire or in storage.
- No per-epoch salt for occupancy.
- No two-tree dispatcher (Anarchy has only one tree).
- No `target_tree` private witness.

The Anarchy circuit's R1CS count is the smallest of the four governance types — roughly half of Democracy's tier-1 ~6298 (per [Democracy §4.7.3](democracy-update-testnet-design.md#473-circuit-constraints)). Proving time at tier-1 is roughly **~150ms** (matching v1).

### 4.8 No `create_anarchy_group_v2` verbose binding

Where Oligarchy v0.1.4 §4.8 introduced a verbose `create_oligarchy_group_v2` signature that binds `member_root`, `admin_root`, `salt_initial`, `occupancy_commitment`, `commitment_initial`, and `epoch=0` as public inputs to a 7-IC Create VK (closing the create-time self-DoS sep-democracy still carries), **Anarchy does not need this**. The only create-time data is `commitment` itself, which the standard 3-IC membership VK binds via public input. There is no separate `occupancy_commitment_initial` to validate, no `admin_root` or `salt_initial` to bind.

Anarchy's `create_group` reuses the standard membership VK at the same 3 IC points — same as v1. No separate Create VK family.

### 4.9 Initial member set, `create_group`

```
create_group(
    caller,
    group_id,
    commitment,              // = Poseidon(Poseidon(member_root, 0), salt_initial)
    tier,
    member_count,            // informational; 0 = not tracked
    proof,                   // standard membership proof against (commitment, epoch=0)
    public_inputs,           // PublicInputs { commitment, epoch }
)
```

The creator constructs `member_root` with themselves at member-slot 0 (tombstone elsewhere). The `proof` proves the creator knows the secret key behind the leaf at slot 0. After successful create, only `commitment` (plus `tier`, `member_count`, `epoch=0`, `timestamp`, `active=true`) is persisted; `member_root` and `salt_initial` are consumed in proof verification and discarded. Same shape v1 Anarchy uses.

**The contract does NOT enforce a member-count floor at create.** A creator who passes `member_count = 0` (per the v1 "not tracked" sentinel) gets a group whose informational count is `0`; subsequent updates leave this unchanged. The circuit's "you must witness a member-leaf secret to update" requirement is the only natural floor — if all members tombstone themselves, no further update can succeed. This is identical to v1 Anarchy's posture.

### 4.10 Cross-tree atomic updates (NOT applicable)

Oligarchy §4.10's cross-tree atomic-update concern doesn't apply to Anarchy (one tree). Multi-leaf updates within a single transaction would require a multi-signer Anarchy variant, which is out of scope per §3.4.

---

## 5. Alternatives Considered

### 5.1 Keep Anarchy in the monolithic `sep-xxxx` contract

Skip the per-type extraction; rely on `sep-xxxx`'s existing v1 Anarchy paths.

- **Pros**: Zero-LOC change. No new contract address, no client routing update.
- **Cons**: Carries dispatch overhead + multi-type ABI surface for every Anarchy operation. Auditability loss. Inconsistent with the per-type architecture established for Democracy + Oligarchy.

Rejected. Per-type extraction is the established pattern; sticking with the monolith leaves Anarchy as the odd-one-out.

### 5.2 Strip `member_count` entirely from the per-type contract

Drop the informational `member_count` field from `CommitmentEntry`. Operators who want to track count do so off-chain.

- **Pros**: Smaller storage footprint per group. Closes the (acknowledged-already) leak that the field publishes a count on-chain. Anarchy v2-style privacy improvement.
- **Cons**: Breaks compatibility with v1 sep-xxxx ABI; clients reading `current.member_count` would need to handle the field's absence. Per the user's option-A choice for this design, "pure per-type extraction" preserves v1 surface verbatim.

Rejected per user direction. Documented as §11 follow-up — a future v2 Anarchy design could close this if a use case requires.

### 5.3 Pad Anarchy wire payload to 5 scalars (match Democracy / Oligarchy)

Add two zero scalars to Anarchy's `update_commitment` payload so the wire shape matches Democracy / Oligarchy.

- **Pros**: Wire-shape distinguishability between Anarchy and Democracy / Oligarchy contracts is closed.
- **Cons**: The per-type architecture means each contract is at a separate address; cross-contract observers already see the address first, making wire-shape matching redundant. The padding adds 64 bytes per update without buying any privacy.

Rejected. Per-type architecture renders the §3.4-style padding fix moot.

### 5.4 Add v2 occupancy commitment to Anarchy

Hide `member_count` via the same `Poseidon(DOMAIN_OCCUPANCY, fold(bitmap))` mechanism Democracy v0.5 introduced.

- **Pros**: Closes the count-leak residual.
- **Cons**: Requires a new circuit (Anarchy v2 with occupancy binding), new Phase A work, ceremony rotation. v1 Anarchy explicitly accepted member_count visibility; closing it is a v2 redesign, not an extraction.

Rejected per user option-A choice. §11 follow-up.

### 5.5 Bind `group_id` in the MembershipCircuit (Audit Finding #1 from sep-xxxx)

`contracts/sep-xxxx`'s audit identified that `MembershipCircuit` doesn't bind `group_id` as a public input — a proof from group A could (in theory, with cooperative attacker) be replayed against group B if both share the same root + epoch. The audit deferred this to a future circuit rotation.

- **Pros**: Closes the cross-group replay surface for Anarchy.
- **Cons**: Requires regenerating the Anarchy MembershipCircuit + UpdateCircuit, which means new VKs + ceremony work. Out of scope for "pure per-type extraction."

Rejected per user option-A choice. §11 follow-up; would require Phase A circuit work.

---

## 6. Implementation Plan — five phases

Same phase shape as [Democracy §6](democracy-update-testnet-design.md#6-implementation-plan--six-phases) and [Oligarchy §6](oligarchy-update-testnet-design.md#6-implementation-plan--six-phases). **Phase A is zero LOC for Anarchy** (existing v2 keyset reused); other phases are smaller than Democracy / Oligarchy due to no occupancy / threshold / admin-set logic.

### Phase A — circuit + prover (existing; no new work)

Anarchy's UpdateCircuit + MembershipCircuit already ship in `keyset-v2/` (per `docs/update-circuit-binding-design.md`). Per-type extraction reuses these directly. Phase A LOC: **0**.

If a future v2 Anarchy redesign (per §11) lands the audit-finding fixes (`group_id` binding, operation-tag binding, member-count occupancy hiding), Phase A scope grows accordingly. For pure extraction, Phase A is satisfied.

### Phase B — FFI + Swift/Kotlin bridges (existing; no new work)

The existing v1 Anarchy FFI exports (`sep_generate_update_proof_v2` taking the 3-scalar payload) work as-is. The per-type contract's `update_commitment` accepts byte-identical payloads. Phase B LOC: **0** for pure extraction; ~30 LOC if clients need an explicit `case anarchy(...)` enum variant for per-type contract routing (add later if helpful).

### Phase C — Contract redeploy with Anarchy entrypoints

| Step | Files | LOC | Output |
|---|---|---|---|
| C1 | `contracts/sep-anarchy/Cargo.toml` + `Cargo.lock` (copied from sep-democracy) | ~40 | Crate scaffold; `=25.3.0` soroban-sdk pin. |
| C2 | `contracts/sep-anarchy/test-vectors.json` | ~400 | Canonical contract-ABI pin. Created FIRST per the established workflow. |
| C3 | `contracts/sep-anarchy/src/lib.rs` | ~600 | The contract — see §B in the impl plan companion doc. |
| C4 | `contracts/sep-anarchy/src/test.rs` | ~500 | ~35 inline tests covering every entry in `tests_to_implement`. |
| C5 | `scripts/deploy_sep_anarchy_testnet.sh` | (script) | Sibling of Oligarchy / Democracy deploy scripts. Reads VKs from `keyset-v2/`. |
| C6 | `docs/contract/anarchy/{impl_plan,proof_of_soundness,proof_of_correctness}.md` | ~1500 (docs) | Standard per-contract documentation triple. |

Total: ~600 LOC contract + ~500 LOC tests + ~1500 LOC docs.

**Phase C exit criteria**: `cargo test --manifest-path contracts/sep-anarchy/Cargo.toml` reports ~35/35 pass. `stellar contract build` produces a working wasm. `test_vectors_consistency` enforces error-code / IC-count / tier-capacity / max-groups-per-tier consistency.

### Phase D — Relayer + clients

| Step | Files | LOC | Output |
|---|---|---|---|
| D1 | `relayer/src/handler.rs` Anarchy variant in public-inputs translation | ~30 | The relayer's translation layer already accepts `UpdateCommitmentPublicInputs.anarchy(...)`; adding routing to the new contract address is a config update. |
| D2 | iOS `OnChainService.publishCommitmentUpdate` Anarchy dispatch | ~50 | Branches on `group.groupType == .anarchy`; routes to the new contract address. Uses existing `generateUpdateProofV2` from v1 Anarchy paths. |
| D3 | Android parallel | ~50 | |
| D4 | `RelayerDefaults` + client config | ~10 | New contract address registered. |

Total: ~140 LOC. Smaller than Democracy's / Oligarchy's Phase D because no UI work (Anarchy has no admin-management surface, no quorum picker).

### Phase E — Ceremony coordination (shared with Democracy + Oligarchy)

Same MPC ceremony. Anarchy's existing dev VKs are rotated to production VKs at ceremony time. **No new circuits** vs Democracy + Oligarchy ceremony scope (Anarchy reuses circuits that are already in the ceremony plan via `keyset-v2/`).

### Cumulative scope estimate

| Phase | Engineering LOC (approx) | Calendar (engineer-weeks) |
|---|---|---|
| A — circuit + prover | 0 (existing) | 0 (no work) |
| B — FFI + bindings | 0 (existing) — or ~30 LOC for per-type variant routing | < 1 |
| C — contract redeploy | ~1100 (contract + tests + docs) | 1 |
| D — relayer + clients | ~140 | < 1 |
| **A–D total** | **~1240** | **2–3** |
| E — ceremony (shared) | (process; no new scope) | (free-rider on Democracy / Oligarchy) |

By far the smallest of the three governance-type per-type contracts shipped to date. No Phase F (multi-admin quorum is Democracy / Oligarchy concern; Anarchy is single-signer by definition).

---

## 7. Test Plan

Inherits all of [Democracy §7](democracy-update-testnet-design.md#7-test-plan)'s shape, with Anarchy-specific simplifications.

### 7.1 Rust unit tests (Anarchy-specific)

Inline tests in `contracts/sep-anarchy/src/test.rs` parallel to Democracy's `~43-test` suite, scoped down for Anarchy:

- `test_initialize` — constructor accepts admin + 6 VKs; persistent storage written.
- `test_invalid_membership_vk_length_rejected` — IC ≠ 3 → `InvalidVkLength`.
- `test_invalid_update_vk_length_rejected` — IC ≠ 4 → `InvalidVkLength`.
- `test_create_group_happy_path` — valid create reaches verifier (mock proof fails pairing → `InvalidProof`).
- `test_create_group_rejects_invalid_tier` — tier > 2 → `InvalidTier`.
- `test_create_group_rejects_duplicate_group_id` — second create on same id → `GroupAlreadyExists`.
- `test_create_group_rejects_non_canonical_commitment` — non-canonical Fr → `InvalidCommitmentEncoding`.
- `test_create_group_rejects_invalid_proof` — bad proof → `InvalidProof`.
- `test_create_group_restricted_mode_rejects_non_admin` — `caller != admin` in restricted mode → `AdminOnly`.
- `test_create_group_enforces_tier_group_limit` — `GroupCount(tier) >= MAX_GROUPS_PER_TIER` → `TierGroupLimitReached`.
- `test_update_commitment_happy_path` — valid update reaches verifier.
- `test_update_commitment_rejects_stale_c_old` — `c_old != current.commitment` → `PublicInputsMismatch`.
- `test_update_commitment_rejects_wrong_epoch_old` — `epoch_old != current.epoch` → `PublicInputsMismatch`.
- `test_update_commitment_rejects_non_canonical_c_new` — `InvalidCommitmentEncoding`.
- `test_update_commitment_rejects_replayed_proof` — `ProofReplay`.
- `test_update_commitment_rejects_inactive_group` — `GroupInactive`.
- `test_update_commitment_rejects_unknown_group` — `GroupNotFound`.
- `test_verify_membership_*` (4 tests) — read-only verifier branches.
- `test_update_vk_requires_auth` — admin-only.
- `test_update_vk_rotates_membership_vk`, `test_update_vk_rotates_update_vk` — Membership / Update rotation paths.
- `test_update_vk_rejects_invalid_tier` — tier > 2 → `InvalidTier`.
- `test_get_commitment_returns_current_state`, `test_get_commitment_rejects_unknown_group`.
- `test_get_history_returns_chronological_entries`, `test_get_history_rejects_unknown_group`.
- `test_archive_entry_appends_and_prunes` — direct helper test.
- `test_bump_group_ttl_extends_group_storage`, `test_bump_group_ttl_rejects_unknown_group`.
- `test_vectors_consistency` — ABI pin.

Total: ~35 tests. Smaller than Democracy's 43 (no threshold tests) and Oligarchy's 51 (no admin-threshold + no Create VK tests).

**Mock-proof strategy parallels sep-democracy / sep-oligarchy**: `valid_g1` / `valid_g2` via `hash_to_g{1,2}` for subgroup-valid points; mock proofs cannot pass `pairing_check`, so happy-path tests assert `InvalidProof` (verifier reached) rather than success. The full positive round-trip is gated on real Groth16 proofs, which already exist in v1 sep-xxxx (no Phase A blocker for Anarchy).

### 7.2 Cross-platform vectors

`docs/cross-platform-test-vectors.json` already covers Anarchy v2 (the existing 3-scalar payload). The per-type contract reuses the same circuit + same payload, so existing vectors apply.

### 7.3 Relayer integration

`update_commitment` already accepts the `UpdateCommitmentPublicInputs.anarchy(...)` variant in v1 sep-xxxx; the relayer-side translation is unchanged. One test asserts the relayer forwards a 3-scalar Anarchy payload to stellar CLI without re-shaping.

### 7.4 iOS / Android integration

- `AnarchyMembershipTests.swift` / `.kt` — boots in-memory `OnChainService` mocks; drives a 1 → 2 member-add Anarchy flow against the new contract address; asserts the correct 3-scalar `UpdateCommitmentPublicInputs.anarchy(...)` is emitted and slot-index assignment lands.
- Layer 2 fingerprint mismatch + bitmap-derivation determinism (single-tree only).
- `UpdateCommitmentPublicInputs` discriminator serialization extended to Anarchy variant.

### 7.5 Manual testnet end-to-end

Pre-condition: Phase C complete (fresh testnet contract deployed; existing `keyset-v2/` Anarchy VKs installed).

Steps:
1. iOS creates an Anarchy group at medium tier with `member_count = 0` (the "not tracked" sentinel). Verify on Soroban testnet that `current.tier == 1`, `current.member_count == 0`, `current.commitment` is set. Verify NO `group_type` field, NO `occupancy_commitment` field, NO `admin_root` / `admin_threshold_numerator` field exist in the contract storage entry.
2. iOS sends an invitation to Android.
3. Android accepts. Android sends `SEPMemberJoined`.
4. iOS processes the SMJ, runs Anarchy v2 path, calls `update_commitment` with 3-scalar Anarchy payload. Verify relayer log shows `function=update_commitment` with 3-scalar payload, status 200.
5. iOS local state advances: epoch 1, 2 active members.
6. Android receives broadcast; persists slot index = 1.
7. iOS chat to Android works; Android chat to iOS works (both members; BLS auth passes).
8. **Privacy verification**: read the contract storage entry. Assert `member_count` is still `0` (the contract does not auto-update it; clients track off-chain). Assert payload is 3 scalars `(c_old, epoch_old, c_new)` matching the `update_commitment_public_inputs_wire_format` pin in the test-vectors.

Failure-mode tests:
- §4.3 Layer 2 fingerprint mismatch.
- Member-removal of last member: the group becomes effectively unupdateable (no member can witness the next proof). UX warning at the deactivation surface.
- `tier > 2` at create: rejected with `InvalidTier`.
- Replayed proof bytes: rejected with `ProofReplay`.

### 7.6 VK-mismatch handling, end-to-end (inherited from Democracy)

Same shape as Democracy's tests — no Anarchy VK installed → `NotInitialized`; wrong Anarchy VK installed → `InvalidProof`.

---

## 8. Rollout (cross-phase coordination)

Same gating mechanism as [Democracy §8](democracy-update-testnet-design.md#8-rollout-cross-phase-coordination). Phase C + D for Anarchy can run **in parallel with** Democracy + Oligarchy — no shared dependencies, since each per-type contract is at its own address with its own VKs.

The natural sequencing:
1. Land this design doc (PR-X off `main`).
2. Land the per-type Anarchy contract (PR-Y off `main`, parallel to PR #148 for Democracy and PR #149 for Oligarchy).
3. Phase D (clients) — single client release covering Anarchy + Democracy + Oligarchy at their respective per-type contract addresses.
4. Phase E (ceremony) — single MPC session covers all three governance types' production VKs.
5. Mainnet release covers all three per-type contracts simultaneously.

If priorities diverge, Anarchy can ship Phase C alone (testnet-only) without blocking Democracy / Oligarchy — the per-type architecture means there's no shared contract dependency.

---

## 9. Risks

Inherits all of [Democracy §9](democracy-update-testnet-design.md#9-risks)'s risks. Additions specific to Anarchy:

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| `member_count` drift between contract storage and off-chain reality | High (intrinsic to "informational only") | Low (the field is documented as advisory) | §11 follow-up: introduce v2-style occupancy commitment to authoritatively track. |
| Single-signer model means a malicious last-member can update arbitrarily before being removed | Low (per-group) | Medium (group state is whatever the last member committed) | Inherent to Anarchy. Operators choose Anarchy knowing this; switch to Democracy / Oligarchy for stronger authorization. |
| Cross-group replay (sep-xxxx Audit Finding #1) — Anarchy MembershipCircuit doesn't bind `group_id` | Low (requires cooperative attacker) | Low (the proof binds `commitment`, which differs across groups, so replay would require commitment collision) | §11 follow-up: bind `group_id` in the MembershipCircuit at next ceremony rotation. |
| Pre-inclusion mempool replay | High (intrinsic to public ledger) | Low (matches Democracy + Oligarchy's posture) | Same residual; circuit-level operation-tag binding is §11 follow-up. |
| Self-bricking via last-member removal | Low | Medium (group becomes unupdatable but chat works for existing members) | Inherent to Anarchy. UX warning. (Unlike Democracy / Oligarchy which have in-circuit floors, Anarchy's only floor is "no witness, no proof" which is game-theoretic rather than enforced.) |

---

## 10. Open Questions

### 10.1 Phase-C blockers (must ratify before C1 opens)

**None.** Anarchy's per-type extraction has no Phase-A circuit blockers (existing v2 circuits work) and no novel Phase-C contract decisions beyond what Democracy + Oligarchy already established. The contract surface is the smallest of the three per-type contracts.

### 10.2 Other open questions

1. **Should the per-type Anarchy contract emit an event when `member_count` is supplied at create time?** Currently planned: no event field for `member_count` (the value is informational and clients track it off-chain anyway). Operators can read it via `get_commitment`. Recommendation: keep as is; revisit if a real use case demands a `MemberCountSet` event.

2. **Should `update_commitment` reset `member_count` to 0 (the sentinel) automatically?** If a client passes a non-zero `member_count` at create then the group's actual count drifts, the stored field becomes stale. Currently planned: no auto-reset (the contract has no Poseidon host and cannot recompute counts). Clients are responsible for either keeping it accurate (via a hypothetical future `update_member_count` admin entrypoint) or accepting the drift. Recommendation: leave drift to clients; don't add an admin entrypoint for this.

3. **Should the design propose an `update_member_count` admin entrypoint for operators who want to refresh the informational field?** Out of scope for this design. §11 follow-up if a use case demands.

### 10.3 Resolved (folded into core design)

- v2 occupancy commitment for Anarchy: out of scope per user direction (option A — pure extraction).
- `group_id` binding in MembershipCircuit: §11 follow-up; sep-xxxx audit finding deferred.
- Operation-tag binding for mempool-replay fix: §11 follow-up; out of scope for pure extraction.
- Multi-signer Anarchy: rejected (functionally equivalent to Democracy at threshold = 1/N).
- Wire-payload padding to 5 scalars: rejected (per-type address discrimination renders padding moot).

---

## 11. Follow-Up Work

- **v2 Anarchy with occupancy commitment**: hide `member_count` via the same mechanism Democracy v0.5 introduced. Closes the count-leak residual. Requires new Anarchy circuit + ceremony rotation.
- **`group_id` binding in MembershipCircuit + UpdateCircuit**: closes the cross-group replay surface flagged in sep-xxxx Audit Finding #1. Requires circuit regeneration; ceremony rotation.
- **Operation-tag binding**: closes pre-inclusion mempool replay (parallel to the equivalent residual in Democracy + Oligarchy).
- **`update_member_count` admin entrypoint**: lets operators refresh the informational `member_count` field if drift is observed. Probably never needed.
- **Multi-signer Anarchy**: explicitly out of scope; if a use case appears, recommend Democracy at threshold `1/N` instead.
- **Drop `member_count` entirely**: a future v2 design could remove the informational field once all clients migrate to off-chain tracking. Storage shrink + privacy improvement.
