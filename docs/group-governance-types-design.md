## Preamble

```
Title: Configurable Group Governance Types for SEP-XXXX
Author: @rinat-enikeev
Status: Draft
Created: 2026-04-18
Updated: 2026-04-18
Version: 0.0.1
Supersedes: none
Extends:   docs/design-doc.md, docs/sep.md, docs/update-circuit-binding-design.md
References: docs/secure-member-removal-design.md, docs/implementation_plan.md,
            docs/vuln-unbound-new-commitment.md
Discussion: TBD
```

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** in this document
are to be interpreted as described in RFC 2119.

---

## 1. Introduction

SEP-XXXX currently enforces a single implicit governance rule: *any* current member of a
group **MAY** unilaterally rewrite the group's membership commitment by submitting a valid
Groth16 membership proof to `update_commitment`. There are no admins, no quorums, no
co-signatures. This design doc proposes an additive mechanism — selected once at group
creation — that extends the contract with four configurable governance models:

1. **Anarchy** — the existing permissive policy; any member may Commit any change.
2. **1v1** — exactly two participants, no subsequent membership changes.
3. **Democracy** — a Commit requires a proof carrying ≥ ⌈n/2⌉ member co-openings.
4. **Oligarchy** — a Commit requires a proof that the submitter belongs to a per-group
   admin set, disjointly tracked on-chain as a second commitment.

The design preserves every existing privacy invariant of SEP-XXXX: the on-chain record
reveals neither member identities, nor voters, nor admins. All governance rules are
enforced **inside the ZK circuit**, not by caller-identity checks on Soroban.

---

## 2. Background

The current contract at `contracts/sep-xxxx/src/lib.rs` stores, per group, only a
`CommitmentEntry { commitment, epoch, timestamp, tier, active }` (lines 131–144). Three
state transitions exist:

- `create_group` (lines 421–514) — founder authorises with `require_auth()` and supplies
  a membership proof at epoch 0.
- `update_commitment` (lines 532–609) — accepts an `UpdateCircuit` proof that binds
  `(c_old, epoch_old, c_new)`. Any current member may submit; the transaction signer
  is decoupled from the proof prover by a relayer.
- `deactivate_group` (lines 654–718) — any current member may freeze the group.

Prior design docs acknowledge the permissive nature explicitly:

> "The current scheme already allows any member to author an update that removes every
> other member (self-take-over by a legitimate member is a pre-existing governance
> property)." — `docs/update-circuit-binding-design.md`, §4.3

Real-world groups — DAO councils, private family chats, 1:1 direct messages — need
finer-grained rules. Existing SEP-XXXX cannot serve these cases without social
out-of-band coordination that is brittle and unauditable.

This document specifies a first-class, circuit-enforced governance layer that extends
SEP-XXXX in a backward-compatible way.

---

## 3. Problem and Goals

### 3.1 Problem

Anarchy leaks policy to the social layer. Two users in a "1:1" conversation have no
protocol-level guarantee that their counterpart cannot silently replace them. A 100-member
community relies on vigilance against any single insider rewriting the group. There is no
machine-verifiable record of *who agreed* to a Commit.

### 3.2 Goals

**G1 — Member-blindness preservation.** The contract **MUST NOT** learn any member
identity, vote, or admin status from a state-changing transaction.

**G2 — Proof-only authorisation preservation.** The Stellar account submitting a
transaction **MUST** remain decoupled from the prover (fee-decoupling via relayer is
unchanged).

**G3 — Compositional reuse.** New governance types **SHOULD** reuse the existing
`MembershipCircuit`, `UpdateCircuit`, and Poseidon Merkle infrastructure wherever the
on-chain transition shape is compatible.

**G4 — Backward compatibility.** Legacy groups created before this SEP revision
**MUST** continue to operate under Anarchy semantics without migration.

**G5 — Per-group configurability.** The governance type **MUST** be immutable for the
lifetime of a group; it is chosen once at `create_group_v2`.

### 3.3 Non-goals

Sybil resistance, identity binding, key custody, key rotation under compromise, and
cross-group membership correlation remain out of scope — see `docs/sep.md §Out of scope`
and §8 of this document.

---

## 4. Scope

### 4.1 In scope

- A new `group_type` tag persisted per group at creation.
- Four governance types (Anarchy, 1v1, Democracy, Oligarchy) with per-type circuit
  variants and authorisation rules.
- A second per-group commitment (admin set) for Oligarchy.
- Client SDK changes in `swift-mls/` and `kotlin-mls/` to expose governance APIs.
- Mobile UX changes in `clients/ios/` and `clients/android/` to select a governance
  type at create time and surface governance-specific affordances (vote, promote,
  admin badge).

### 4.2 Out of scope

- Dynamic governance-type changes (a Democracy that upgrades to Oligarchy mid-life).
- Quorum thresholds other than ⌈n/2⌉ for Democracy. Configurable thresholds are a
  follow-up SEP revision.
- Sybil-resistant identity provisioning.
- Voice/video call permissions — see `docs/sep-voice-video-calls.md` for separate
  treatment.

---

## 5. Terminology

- **Commit** (MLS sense) — a membership transition that advances the epoch by one and
  produces a new commitment.
- **member tree** — the Poseidon Merkle tree whose root feeds the group commitment.
- **admin tree** (Oligarchy only) — a second Poseidon Merkle tree of admin BLS public
  keys, disjoint from the member tree in structure but overlapping in membership. An
  admin is always also a member.
- **ballot** — in Democracy, the off-chain object collecting co-openings over a
  proposed `c_new`.
- **coordinator** — in Democracy, any group member who aggregates a ballot and
  generates the final proof.

---

## 6. Specification

### 6.1 Governance rules (normative)

Let `n` denote the current member count of a group at the pre-transition epoch.

**Anarchy (`group_type = 0`).** Matches the current SEP-XXXX behaviour. A Commit
**MUST** be accompanied by a valid `UpdateCircuit` Groth16 proof that binds
`(c_old, epoch_old, c_new)`. The prover **MUST** know the secret key of a leaf in the
old member tree. No other authorisation is required.

**1v1 (`group_type = 1`).** A 1v1 group **MUST** be created with `tier = 0` and with
exactly 2 leaves in the founding member tree. After creation, `update_commitment`
**MUST** return a new error `OneOnOneImmutable (17)`. The only state transitions
permitted are `deactivate_group` (by either member) and `bump_group_ttl` (permissionless).
Either participant leaving the group is equivalent to `deactivate_group`.

**Democracy (`group_type = 2`).** A Commit **MUST** be accompanied by a
`DemocracyUpdateCircuit` proof that, in addition to the standard binding of
`(c_old, epoch_old, c_new)`, proves:

1. the prover knows `K` distinct secret keys of leaves in the old member tree;
2. `2·K ≥ n`;
3. `n` equals the public input `member_count_old`, itself bound to `c_old`.

The value of `n` is not stored on-chain today; it is made available to the circuit as
a public input and bound to `c_old` by a commitment extension defined in §6.4.3.

**Oligarchy (`group_type = 3`).** At `create_group_v2`, the founder **MUST** supply an
`admin_root: BytesN<32>` — a Poseidon commitment over a tree containing at least the
founder's BLS pubkey. The contract persists this under `DataKey::AdminSet(group_id)`.
Two state transitions gate on admin status:

- **Member ops.** `update_commitment` **MUST** carry an `OligarchyUpdateCircuit` proof
  that binds `(c_old, epoch_old, c_new, admin_root_old)` and proves the prover knows a
  secret key whose leaf opens against **both** the member tree and the admin tree.
- **Admin rotation.** A new contract function `update_admin_set(group_id, proof,
  admin_public_inputs)` accepts an `AdminUpdateCircuit` proof that binds
  `(admin_root_old, admin_epoch_old, admin_root_new)` and proves the prover is a current
  admin. The admin tree has its own monotonic epoch (`admin_epoch`), salt, and history
  window, disjoint from the member tree's.

Non-admin members **MAY NOT** call `update_commitment` or `update_admin_set`. If all
admins leave (via the admin rotation circuit), the group enters a **frozen** state:
messaging still works (off-chain), but no further member or admin changes are possible.
Deactivation remains available to any current admin.

### 6.2 On-chain data model

#### 6.2.1 `CommitmentEntryV2`

```rust
#[contracttype]
pub struct CommitmentEntryV2 {
    pub commitment: BytesN<32>,
    pub epoch: u64,
    pub timestamp: u64,
    pub tier: u32,
    pub active: bool,
    pub group_type: u32, // 0 Anarchy, 1 OneOnOne, 2 Democracy, 3 Oligarchy
}
```

The legacy `CommitmentEntry` stays defined for migration reads; writes always produce
`CommitmentEntryV2`.

#### 6.2.2 New `DataKey` variants (append-only)

```rust
pub enum DataKey {
    // ... existing variants unchanged ...
    GroupV2(BytesN<32>),                    // -> CommitmentEntryV2
    HistoryV2(BytesN<32>),                  // -> Vec<CommitmentEntryV2>
    AdminSet(BytesN<32>),                   // -> BytesN<32>   (admin Poseidon root)
    AdminEpoch(BytesN<32>),                 // -> u64
    AdminHistory(BytesN<32>),               // -> Vec<AdminEntry>
    UpdateVKByType(u32 /*tier*/, u32 /*group_type*/), // -> VerificationKeyData
    AdminUpdateVK(u32 /*tier*/),            // -> VerificationKeyData
    OneOnOneCreateVK,                        // -> VerificationKeyData (tier 0 only)
}
```

Legacy `Group(id)` and `History(id)` remain. Reads try V2 first, fall back to legacy and
synthesise `group_type = 0`. The first state-changing operation on a legacy group
lazily upgrades the record to V2; history entries are **not** rewritten.

#### 6.2.3 `AdminEntry`

```rust
#[contracttype]
pub struct AdminEntry {
    pub root: BytesN<32>,
    pub epoch: u64,
    pub timestamp: u64,
}
```

Admin history mirrors member history with a 64-entry rolling window.

### 6.3 Contract surface

Entries added to `contracts/sep-xxxx/src/lib.rs`:

```rust
pub fn create_group_v2(
    env: Env,
    caller: Address,
    group_id: BytesN<32>,
    commitment: BytesN<32>,
    tier: u32,
    group_type: u32,
    admin_root: Option<BytesN<32>>,       // Some(_) iff group_type == 3
    proof: Groth16Proof,
    public_inputs: PublicInputs,          // membership proof at epoch 0
) -> Result<(), Error>;

pub fn update_admin_set(
    env: Env,
    group_id: BytesN<32>,
    proof: Groth16Proof,
    admin_public_inputs: AdminUpdatePublicInputs,
) -> Result<(), Error>;
```

`update_commitment` is refactored to a typed dispatch:

```rust
pub fn update_commitment(
    env: Env,
    group_id: BytesN<32>,
    proof: Groth16Proof,
    public_inputs: UpdatePublicInputs,
    // NEW: optional extension for Democracy/Oligarchy
    extra: Option<GovernancePublicInputs>,
) -> Result<(), Error>;
```

where `GovernancePublicInputs` is:

```rust
#[contracttype]
pub enum GovernancePublicInputs {
    Democracy { member_count_old: u32 },
    Oligarchy { admin_root_old: BytesN<32> }, // MUST equal AdminSet(group_id)
}
```

For Anarchy, `extra = None` (wire-compatible with today's clients that call the legacy
`update_commitment`). For 1v1, the function returns `OneOnOneImmutable` unconditionally.

#### 6.3.1 New error codes

```rust
OneOnOneImmutable     = 17, // update_commitment on group_type == 1
UnknownGroupType      = 18,
MissingAdminRoot      = 19, // create_group_v2 with group_type == 3 and admin_root = None
AdminRootMismatch     = 20, // Oligarchy extra.admin_root_old != storage
DemocracyInputMissing = 21, // group_type == 2 and extra != Some(Democracy{..})
NotAdmin              = 22, // update_admin_set proof fails admin membership
```

#### 6.3.2 Events

No new event types are introduced. `CommitmentUpdated` is re-emitted for Democracy and
Oligarchy Commits identically. A new `AdminSetUpdated` event parallels `CommitmentUpdated`:

```rust
#[contractevent]
pub struct AdminSetUpdated {
    #[topic] pub group_id: BytesN<32>,
    pub admin_root: BytesN<32>,
    pub admin_epoch: u64,
    pub timestamp: u64,
}
```

### 6.4 Circuits

All new circuits live under `src/circuit/` alongside `update.rs` (the existing
`UpdateCircuit`). All circuits compile per tier (tree depths 5, 8, 11) except where
noted. Each has a Groth16 setup producing a `VerificationKeyData` persisted under the
storage key listed in §6.2.2. Proofs remain 192 bytes compressed / 384 bytes
uncompressed. Verification cost remains 3 BLS12-381 pairings per proof.

#### 6.4.1 `OneOnOneCreateCircuit`

- **Purpose:** enforce exactly 2 non-zero leaves at founding for 1v1 groups.
- **Tier:** 0 only (`MembershipCircuit` tier 0 has depth 5; we use a dedicated
  2-leaf fixed-depth-1 variant).
- **Public inputs:** `commitment` (bound to `c = Poseidon(Poseidon(root, 0), salt)`
  as in the existing membership circuit).
- **Witnesses:** two leaves `leaf_A = Poseidon(pk_A)`, `leaf_B = Poseidon(pk_B)`;
  `salt`.
- **Key constraints:**
  1. `root = Poseidon(leaf_A, leaf_B)` (2-leaf tree).
  2. `leaf_A ≠ leaf_B` (distinct-members gadget).
  3. `commitment = Poseidon(Poseidon(root, 0), salt)`.
- **Storage:** `DataKey::OneOnOneCreateVK`.

#### 6.4.2 `DemocracyUpdateCircuit`

- **Purpose:** prove that a Commit is authorised by ≥ ⌈n/2⌉ member openings.
- **Tier × K_max matrix:** compiled per tier and per maximum signer count `K_max`.
  v1 ships `(tier=0, K_max=32)` and `(tier=1, K_max=256)`; tier 2 is deferred.
- **Public inputs:** `c_old`, `epoch_old`, `c_new`, `member_count_old`.
- **Witnesses:** ordered list of `K` tuples `(sk_i, merkle_path_i, leaf_idx_i)` for
  `K ≤ K_max`; `root_old`, `root_new`, `salt_old`, `salt_new`.
- **Key constraints:**
  1. For each `i ∈ [0, K)`, `Poseidon(sk_i)` opens to `root_old` along `merkle_path_i`
     at `leaf_idx_i`.
  2. Indices `leaf_idx_0 < leaf_idx_1 < … < leaf_idx_{K-1}` (strict ascending, gives
     distinctness for free).
  3. `2 · K ≥ member_count_old`.
  4. `member_count_old ≤ 2^tier_depth` (sanity).
  5. `c_old = Poseidon(Poseidon(root_old, epoch_old), salt_old)` — reuses the
     existing commitment gadget.
  6. `c_new = Poseidon(Poseidon(root_new, epoch_old + 1), salt_new)`.
  7. `member_count_old` is not itself bound to `c_old` in v1 (acceptable leakage; see
     §8). A follow-up revision may include `n` inside the commitment.
- **Storage:** `DataKey::UpdateVKByType(tier, 2)`. Multiple `K_max` variants are
  discriminated by the leading byte of the proof envelope, not by separate DataKey
  variants — v1 ships one `K_max` per tier to avoid complexity.
- **Cost:** O(K · tier_depth) Poseidon constraints. For tier 1, `K_max=256`, depth 8:
  ≈ 256·8·300 ≈ 614k Poseidon constraints. Proving time rises linearly; verification
  is still 3 pairings.

#### 6.4.3 `OligarchyUpdateCircuit`

- **Purpose:** prove member Commit is authored by an admin.
- **Public inputs:** `c_old`, `epoch_old`, `c_new`, `admin_root_old`.
- **Witnesses:** `sk` (secret key), `member_path`, `member_leaf_idx`, `admin_path`,
  `admin_leaf_idx`, `root_old`, `root_new`, `salt_old`, `salt_new`.
- **Key constraints:**
  1. `leaf = Poseidon(sk)`.
  2. `leaf` opens to `root_old` along `member_path` at `member_leaf_idx`.
  3. `leaf` opens to `admin_root_old` along `admin_path` at `admin_leaf_idx`.
  4. `c_old = Poseidon(Poseidon(root_old, epoch_old), salt_old)`.
  5. `c_new = Poseidon(Poseidon(root_new, epoch_old + 1), salt_new)`.
- **Storage:** `DataKey::UpdateVKByType(tier, 3)`. Compiled per tier.

#### 6.4.4 `AdminUpdateCircuit`

Shape-identical to the existing `UpdateCircuit`, but over the admin tree:

- **Public inputs:** `admin_root_old`, `admin_epoch_old`, `admin_root_new`.
- **Constraints:** identical to `UpdateCircuit` with tree variables rebound to the
  admin tree.
- **Storage:** `DataKey::AdminUpdateVK(tier)`.

### 6.5 Invite flow (off-chain, unchanged transport)

Invites ride Nostr transport exactly as today (see `docs/sep.md §Transport` and
`swift-mls/Sources/SwiftMLS/InvitationSender.swift`). What changes is the **client-side
acceptance rule** on the invitee:

| Type | Invitee acceptance rule |
|------|-------------------------|
| Anarchy | Accept bootstrap if the invite bundle is signed by any member. (Today's behaviour.) |
| 1v1 | Invites are disabled; the UI does not expose an Invite affordance. |
| Democracy | Invitee waits for the `CommitmentUpdated` event on Stellar that reflects their addition to `c_new` before finalising local state. If no event lands within a configurable TTL (default 30 minutes), the bootstrap bundle is discarded. |
| Oligarchy | Same as Democracy: the invitee trusts the on-chain `CommitmentUpdated` event. Alternatively, an admin-signed invite envelope (signed with the admin's BLS key + `admin_path` proof attached) is accepted pre-event as a UX optimisation. |

This approach avoids introducing any new on-chain approval primitive. The existing
`CommitmentUpdated` event is already the authoritative signal that a Commit landed;
it is the strongest possible approval artifact because its creation proves the
governance rule was satisfied in-circuit.

Non-admin "invite requests" in Oligarchy travel as a new Nostr event kind
(proposed: `34115 — sep-invite-request`) directed at admins. Admins locally approve or
decline; on approval they send the standard invite. This is a pure client-layer
protocol with no on-chain footprint.

### 6.6 Voting coordinator protocol (Democracy)

Democracy Commits rely on off-chain co-opening collection. The protocol:

```
 ┌──────────────┐      1. propose c_new       ┌──────────────┐
 │  coordinator │ ─────────────────────────► │   members    │
 │  (any member)│                             │  (Nostr chan)│
 └──────────────┘      2. sign(c_new) each   └──────────────┘
        │     ◄─────────────────────────────────────   │
        │                                              │
        │      3. aggregate K openings, K·2 ≥ n        │
        │                                              │
        │      4. generate DemocracyUpdateProof        │
        │                                              │
        ▼      5. submit via relayer                   │
  Stellar contract ─── CommitmentUpdated ──► all members
```

- Any member **MAY** act as coordinator for any ballot. There is no election; multiple
  coordinators running in parallel is safe because the contract rejects
  proof-replay via `UsedProof` tracking (lines 819–842).
- Ballots are identified off-chain by `ballot_id = SHA256(group_id ‖ c_old ‖ c_new)`.
  This prevents double-voting on the same proposal.
- Members co-open by revealing their Poseidon leaf and a fresh signature over
  `ballot_id`. This is a minor identity leak *within* the group (other members now know
  who voted) but **not** on-chain. Leakage inside an encrypted group channel is
  acceptable and matches user expectations for democratic voting.
- Ballots **SHOULD** have a client-configured TTL (default 24 hours); expired ballots
  are discarded locally.

### 6.7 Client UX

#### 6.7.1 Create group

`clients/ios/StellarChat/.../Views/CreateGroupView.swift` and the Android equivalent
gain a governance-type picker after the name field:

```
┌─ Create Group ────────────────────────────┐
│ Name: [____________________]             │
│                                          │
│ Type:                                    │
│   ( ) Open        (anyone can invite/kick)│
│   ( ) 1v1         (frozen at 2)          │
│   ( ) Democracy   (majority vote)        │
│   ( ) Oligarchy   (admins)               │
│                                          │
│ [ Cancel ]                   [ Create ]  │
└──────────────────────────────────────────┘
```

User-facing labels prefer English over jargon ("Open" for Anarchy). Internally, the
`SEPGroupType` enum maps these to `u32` values.

1v1 selection coerces `tier = 0` and limits the initial invite UI to exactly one
recipient. Oligarchy selection marks the creator as the sole initial admin.

#### 6.7.2 Group info screen

`clients/ios/StellarChat/StellarChat/Views/GroupInfoView.swift` (the member list row
and `removeMember(_:)` at ≈line 403) gains a per-type conditional:

- Anarchy: unchanged. Tap-to-kick on any row.
- 1v1: no kick affordance. A "Leave & Close" button replaces it.
- Democracy: tap-to-propose-kick opens a ballot composer. Pending ballots appear in a
  new `PendingBallotsView.swift`.
- Oligarchy: admin badge next to admin rows; "Promote" / "Demote" buttons visible only
  to admins; "Kick" visible only to admins; non-admin rows see a "Request Invite"
  affordance at the bottom.

State machine for the Democracy ballot view:

```
   [ New Ballot ]
        │
        ▼
   ┌────────┐  submit vote   ┌────────┐  K·2 ≥ n   ┌────────┐
   │Pending │ ─────────────► │Collect │ ─────────► │ Ready  │
   └────────┘                └────────┘            └────────┘
        │                                               │
        │  TTL expires                                  │ coordinator
        ▼                                               ▼ generates proof
   ┌────────┐                                        ┌────────┐
   │Expired │                                        │Posted  │
   └────────┘                                        └────────┘
                                                        │
                                        CommitmentUpdated
                                                        ▼
                                                   ┌────────┐
                                                   │Finalised│
                                                   └────────┘
```

#### 6.7.3 File-level change inventory

Contract (`contracts/sep-xxxx/src/`):
- `lib.rs` — new types (§6.2), new errors (§6.3.1), new entry points (§6.3), `update_commitment` dispatch.

Circuits (`src/circuit/`):
- new modules: `onevone.rs`, `democracy.rs`, `oligarchy.rs`, `admin_update.rs`.

Rust core (`src/`):
- `commitment/` — extend commitment builder for 2-leaf case (1v1).
- `merkle/` — expose admin-tree helpers.
- `prover/` — new proof generators for each circuit.
- `ffi.rs`, `jni_ffi.rs` — expose FFI for the four new provers.

Swift SDK (`swift-mls/Sources/SwiftMLS/`):
- `Types.swift` — add `SEPGroupType`, extend `SEPCommitmentEntry`, add `SEPAdminEntry`.
- `ProofGenerator.swift` — `generateOneOnOneCreateProof`, `generateDemocracyUpdateProof`, `generateOligarchyUpdateProof`, `generateAdminUpdateProof`.
- `ContractClient.swift` — `createGroupV2`, `updateAdminSet`, typed dispatch.
- `CommitmentBuilder.swift` — helpers for admin-tree commitment.
- `GroupStateUpdate.swift` — ballot payload types.
- `InvitationSender.swift` — admin-signed invite envelopes for Oligarchy.
- `RustBridge.swift` — new FFI entry points.

Kotlin SDK (`kotlin-mls/src/main/java/com/stellarmls/mls/`): the parallel set of files
mirroring the Swift surface.

iOS app (`clients/ios/StellarChat/StellarChat/`):
- `Models/ChatGroup.swift` — `groupType`, client-side `admins: [BLSPubkey]`, `pendingBallots`.
- `Views/CreateGroupView.swift` — type picker.
- `Views/GroupInfoView.swift` — conditional affordances.
- `Views/PendingBallotsView.swift` — new.
- `StellarChatApp.swift` — `removeMember`, `inviteMember`, `promoteMember`, `demoteMember` branching on `groupType`.

Android app (`clients/android/`): parallel set under `com.stellarmls.stellarchat` packages.

---

## 7. Testing and verification

### 7.1 Contract tests

Extend the test module in `contracts/sep-xxxx/src/lib.rs` and the test snapshots under
`contracts/sep-xxxx/test_snapshots/`. Per-type scenarios:

**Anarchy (regression).**
- Existing tests MUST pass unchanged.
- `test_legacy_group_read_as_v2_default` — create group via legacy `create_group`, then
  verify V2 read returns `group_type = 0`.
- `test_anarchy_upgrade_to_v2_on_first_update` — legacy group, call
  `update_commitment`, assert record is now under `GroupV2`.

**1v1.**
- `test_1v1_create_success` — tier 0, exactly 2 leaves, proof via `OneOnOneCreateCircuit`.
- `test_1v1_create_reject_3_leaves` — expect `InvalidProof`.
- `test_1v1_update_rejected` — `update_commitment` on 1v1 group returns
  `OneOnOneImmutable`.
- `test_1v1_deactivate_success` — either member can deactivate.

**Democracy.**
- `test_democracy_majority_accepts` — n=5, K=3, success.
- `test_democracy_sub_majority_rejects` — n=5, K=2, `InvalidProof`.
- `test_democracy_duplicate_vote_rejects` — K=3 but two paths to same leaf,
  `InvalidProof` (strict-ascending gadget catches this).
- `test_democracy_even_boundary` — n=4, K=2 accepts (2·2 ≥ 4).
- `test_democracy_member_count_mismatch` — `member_count_old` passed is wrong, circuit
  rejects.

**Oligarchy.**
- `test_oligarchy_create_requires_admin_root` — `group_type=3` with `admin_root=None`
  returns `MissingAdminRoot`.
- `test_oligarchy_admin_member_op_success`.
- `test_oligarchy_non_admin_member_op_rejects` — `InvalidProof`.
- `test_oligarchy_admin_update_success`.
- `test_oligarchy_admin_root_mismatch` — `extra.admin_root_old ≠ AdminSet(id)`
  returns `AdminRootMismatch`.
- `test_oligarchy_ex_admin_cannot_rotate` — after demotion the former admin fails
  `update_admin_set`.
- `test_oligarchy_frozen_after_all_admins_leave` — admin set rotated to empty-like
  state, subsequent `update_commitment` and `update_admin_set` fail.

**Migration & replay.** All new proof types participate in the `UsedProof` replay
tracking at `lib.rs:819-842`.

### 7.2 Cross-platform test vectors

Extend `/Users/programyzer/Developer/stellar-mls/docs/cross-platform-test-vectors.json`
with per-type vectors so iOS (Swift) and Android (Kotlin) reach bit-identical commitments
and proofs. Vectors MUST cover:

- a canonical 1v1 group at creation (2 leaves, salt, commitment);
- a canonical Democracy ballot at n=4, K=2 (each signer's opening witness);
- a canonical Oligarchy admin-only member op.

### 7.3 End-to-end verification

End-to-end verification on testnet (`docs/testnet-deployment.md`):

1. Deploy updated contract to testnet.
2. Using `scripts/`, exercise one flow per governance type: create → update → deactivate.
3. Confirm `CommitmentUpdated` / `AdminSetUpdated` events land as expected.
4. Confirm legacy groups created pre-deployment continue to function.
5. Re-run the mobile smoke test suites (iOS fastlane, Android fastlane) against the
   new SDKs; UX flows per §6.7.

### 7.4 Ceremony artefacts

Each new circuit requires a trusted setup. Reuse the Phase 1 powers-of-tau from
`docs/trusted-setup-ceremony-phase1-coordinator-playbook.md`; run new Phase 2 ceremonies
for `OneOnOneCreateCircuit`, `DemocracyUpdateCircuit` (per tier × K_max),
`OligarchyUpdateCircuit` (per tier), and `AdminUpdateCircuit` (per tier). Procedure
follows the existing `trusted-setup-ceremony-phase2-participant-playbook.md`.

---

## 8. Threat model

**T1 — Sybil in Democracy.** An adversary who controls ⌈n/2⌉ member slots trivially
passes the threshold. Mitigation is out of scope: identity provisioning in SEP-XXXX is
a social-layer concern (an inviter must trust who they admit). This SEP is not a
defence against insider Sybil; it **is** a defence against *unilateral* action by any
single legitimate member.

**T2 — Coordinator liveness.** In Democracy, no ballot lands if no member volunteers
to coordinate. Mitigation: any member may coordinate any ballot; there is no leader
election. Clients surface pending ballots with "I will coordinate" affordance.

**T3 — Coordinator censorship.** A malicious coordinator may discard collected
openings and post nothing. Detection is immediate (the ballot TTL expires without a
`CommitmentUpdated`). Any other member **MAY** coordinate the same ballot using the
openings they collected in parallel. The coordinator role carries no special power.

**T4 — Admin coup in Oligarchy.** Because any admin may demote any other admin
(per resolved design decision), a rogue admin can unilaterally remove peers. This is
equivalent to Anarchy applied to the admin set. Mitigation is social-layer
(choose admins carefully) and is acknowledged as the price of a simple, symmetric
admin circuit. A follow-up revision **MAY** introduce threshold-admin rules inside
`AdminUpdateCircuit` (same shape as `DemocracyUpdateCircuit`) if needed.

**T5 — Proof replay.** All new proofs participate in existing `UsedProof` tracking
(`contracts/sep-xxxx/src/lib.rs:824,836-838`); replay is prevented identically to
today's `UpdateCircuit`.

**T6 — Member-count leakage (Democracy).** `member_count_old` is a public input on
`DemocracyUpdateCircuit`. An observer who watches Democracy Commits learns the exact
member count at each transition. The tier already leaks an upper bound; exact-count
leakage is an additional but bounded concession. Out-of-scope for v1; a follow-up
revision may bind `n` into the commitment.

**T7 — 1v1 count leak.** A 1v1 group is detectable as such from its `group_type`
tag. This is semantic (the fact it's a 1v1 is the feature); no privacy loss beyond
what the user deliberately selected.

**T8 — Wedge group: all-admins-left.** An Oligarchy group whose admin set becomes
effectively empty (last admin demotes themselves) is frozen. Messaging continues;
membership and admin updates do not. `deactivate_group` remains callable by any
current *member* (not admin) per the unchanged `deactivate_group` rules — that's a
deliberate safety valve to guarantee forward progress.

**T9 — Downgrade attack on legacy groups.** A relayer or malicious client could
attempt to route a pre-V2 anarchy group through the new dispatcher with spoofed
`group_type = 3`. Defence: the contract reads the authoritative `group_type` from
on-chain storage (`CommitmentEntryV2.group_type` or synthesised `0` for legacy); the
caller's `extra` input is only used for extension public inputs, not for type
selection.

---

## 9. Rollout

Phased to minimise circuit-ceremony surface per release.

**Phase 0 — schema plumbing.** Ship `CommitmentEntryV2`, `DataKey::GroupV2`, lazy-read
fallback, and a passthrough `create_group_v2` that only accepts `group_type = 0`. No
new circuits. Legacy clients unaffected; new clients exercise V2 write path on Anarchy
groups. Mobile apps ship with feature flag off.

**Phase 1 — 1v1.** Introduce `OneOnOneCreateCircuit`, its VK, and the
`OneOnOneImmutable` error. UI exposes the 1v1 option. Smallest circuit surface —
validates the type-dispatch infrastructure end-to-end before larger circuits land.

**Phase 2 — Oligarchy.** Introduce `OligarchyUpdateCircuit`, `AdminUpdateCircuit`,
admin-set storage, and `update_admin_set`. The dual-tree pattern built here is the
foundation Democracy will reuse for its distinct-indices gadget.

**Phase 3 — Democracy.** Introduce `DemocracyUpdateCircuit` for tier 0 and tier 1.
Tier 2 deferred pending benchmark data on prover cost at `K_max = 2048`.

**Phase 4 — Polish.** Full `PendingBallotsView` UX; admin-signed invite envelope
optimisation; cross-platform vectors; operator documentation updates under
`docs/mainnet-deployment.md`.

Each phase ships its own `update_vk` admin call to install the new VK(s) on mainnet.
Rotation is covered by the existing VK-rotation path (`lib.rs:341`, `update_vk`).

---

## 10. Open questions

1. **Democracy threshold configurability.** v1 hard-codes `⌈n/2⌉`. Should the founder
   be able to pick `⌈2n/3⌉` or `n-1`? Requires additional public input and circuit
   parameterisation. Deferred to a follow-up SEP revision.
2. **Threshold-admin Oligarchy.** See T4. A future revision MAY replace
   `AdminUpdateCircuit` with a Democracy-shaped threshold circuit over the admin tree.
3. **1v1 post-creation mutability.** Decided for v1: none. Future revisions MAY add a
   "swap partner with dual-signature" transition if user demand warrants it.
4. **Cross-type migration.** Not supported in v1. Would require a dedicated
   type-transition circuit and careful state surgery.

---

## 11. References

- `docs/design-doc.md` — SEP-XXXX core design.
- `docs/sep.md` — full SEP-XXXX specification.
- `docs/update-circuit-binding-design.md` — `UpdateCircuit` binding (#59 fix).
- `docs/vuln-unbound-new-commitment.md` — the vulnerability that motivated §6.4
  commitment binding throughout.
- `docs/secure-member-removal-design.md` — removal cryptography; interacts with
  Democracy (rekey delivery post-Commit).
- `docs/relay-design-doc.md` — Nostr transport; unchanged by this SEP.
- `docs/push-notification-design.md` — subscription model; may need per-type
  notification routing in a follow-up.
- `contracts/sep-xxxx/src/lib.rs` — authoritative contract source.
- RFC 9420 — Messaging Layer Security.
- RFC 2119 — key words for use in requirements-level documents.
