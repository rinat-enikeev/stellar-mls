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
3. **Democracy** — a Commit requires a proof carrying co-openings from at least half
   of the current members (the exact threshold is `2·K ≥ n`, i.e. ≥50%; see §6.1 for
   why this is *≥50%* and not *strict majority*).
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
  keys, disjoint from the member tree in structure. An admin **SHOULD** also be a
  member; this is a client-enforced convention (the UI only surfaces "Promote" on
  current members), not a circuit-enforced invariant. See §6.4.4 and §8 T10 for the
  rationale and residual risk.
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
exactly 2 leaves in the founding member tree. `create_group_v2` **MUST** reject any
1v1 creation with `tier != 0` and return `Invalid1v1Tier` (see §6.3.1). After
creation, `update_commitment` **MUST** return `OneOnOneImmutable (17)`. The only state
transitions permitted are `deactivate_group` (by either member) and `bump_group_ttl`
(permissionless). Either participant leaving the group is equivalent to
`deactivate_group`.

**Democracy (`group_type = 2`).** A Democracy group persists an additional scalar on
chain: `member_count: u32`, stored as a field of `CommitmentEntryV2` (§6.2.1). This
value is **authoritative** — the contract validates any caller-supplied
`member_count_old` against the stored value before invoking the Groth16 verifier, and
stores an updated `member_count_new` on success.

A Commit **MUST** be accompanied by a `DemocracyUpdateCircuit` proof that binds
`(c_old, epoch_old, c_new, member_count_old, member_count_new)` and proves:

1. the prover knows `K` distinct secret keys of leaves in the old member tree;
2. `K ≥ 1`;
3. `2·K ≥ member_count_old`;
4. the new tree differs from the old tree by at most one leaf (i.e.
   `|member_count_new − member_count_old| ≤ 1`), enforced in-circuit by counting
   differing Merkle-path siblings.

Before verifying the proof the contract **MUST** check:

- `extra.member_count_old == stored.member_count` (else `MemberCountMismatch`);
- `extra.member_count_new ∈ {m − 1, m, m + 1}` where `m = stored.member_count`
  (redundant-but-defensive; the circuit also enforces this).

On success the contract stores `member_count = member_count_new` in the new
`CommitmentEntryV2` record. This construction closes the quorum-bypass gap described
in §8 T6: a malicious coordinator cannot lie about `n` because the contract overrides
any caller-supplied value with its authoritative on-chain record.

**Oligarchy (`group_type = 3`).** At `create_group_v2`, the founder **MUST** supply an
`admin_root: BytesN<32>` — a salted Poseidon commitment over a tree containing at
least the founder's BLS pubkey (exact encoding in §6.2.2). The contract persists this
under `DataKey::AdminSet(group_id)`. Two state transitions gate on admin status:

- **Member ops.** `update_commitment` **MUST** carry an `OligarchyUpdateCircuit` proof
  that binds `(c_old, epoch_old, c_new, admin_root_old)` and proves the prover knows a
  secret key whose leaf opens against **both** the member tree and the admin tree.
- **Admin rotation.** A new contract function `update_admin_set(group_id, proof,
  admin_public_inputs)` accepts an `AdminUpdateCircuit` proof that binds
  `(admin_root_old, admin_epoch_old, admin_root_new)` and proves the prover is a current
  admin. The admin tree has its own monotonic epoch (`admin_epoch`), salt, and history
  window, disjoint from the member tree's.

Non-admin members **MAY NOT** call `update_commitment` or `update_admin_set`.
`deactivate_group`, by contrast, remains callable by **any current member** (not only
admins) — this is a deliberate safety valve to guarantee forward progress if the
admin set becomes empty or captured. See §8 T8. If all admins leave via the admin
rotation circuit, the group enters a **frozen** state for membership/admin changes;
messaging still works (off-chain) and deactivation remains available to any member.

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
    pub group_type: u32,    // 0 Anarchy, 1 OneOnOne, 2 Democracy, 3 Oligarchy
    pub member_count: u32,  // authoritative n (used by Democracy; informational otherwise)
}
```

`member_count` is the count of non-zero leaves in the member tree at this epoch. It
is written by `create_group_v2` and updated by every successful `update_commitment`.
For Anarchy and Oligarchy the field is informational; for Democracy it is
load-bearing (see §6.1). For 1v1 it is always `2`. Clients **SHOULD** cross-check it
against their local view as a sanity guard.

The legacy `CommitmentEntry` stays defined for migration reads; writes always produce
`CommitmentEntryV2`.

#### 6.2.2 New `DataKey` variants (append-only)

```rust
pub enum DataKey {
    // ... existing variants unchanged ...
    GroupV2(BytesN<32>),                    // -> CommitmentEntryV2
    HistoryV2(BytesN<32>),                  // -> Vec<CommitmentEntryV2>
    AdminSet(BytesN<32>),                   // -> BytesN<32>   (salted admin commitment)
    AdminEpoch(BytesN<32>),                 // -> u64
    AdminHistory(BytesN<32>),               // -> Vec<AdminEntry>
    UpdateVKByType(u32 /*tier*/, u32 /*group_type*/), // -> VerificationKeyData
    AdminUpdateVK,                          // -> VerificationKeyData (fixed tier 0)
    OneOnOneCreateVK,                       // -> VerificationKeyData (fixed tier 0)
}
```

**AdminSet encoding.** The 32-byte value stored under `AdminSet(group_id)` is a
*salted commitment* that mirrors the member commitment formula, not a bare Merkle
root:

```
admin_commitment = Poseidon( Poseidon(admin_root, admin_epoch), admin_salt )
```

This matches the member commitment shape (`c = Poseidon(Poseidon(root, epoch), salt)`)
and keeps the `AdminUpdateCircuit` shape-identical to `UpdateCircuit`. `admin_salt`
is distributed by the creator to admins through the existing per-member X25519 inbox
(see `docs/secure-member-removal-design.md`); it is never on-chain.

**Admin tree tier.** The admin tree uses a **fixed tier-0 depth (5, capacity 32
admins)** regardless of the member tier. Admin sets are expected to be much smaller
than member sets; locking tier 0 avoids wasted constraints and collapses the four
admin-update ceremonies (per member tier) into one. If larger admin sets are needed
in future, a follow-up revision can introduce `AdminUpdateVK(tier)` and keyed
circuits — deliberately deferred.

**Lazy upgrade race.** Reads try `GroupV2(id)` first and fall back to legacy
`Group(id)`, synthesising `group_type = 0` and `member_count` from the local member
list. Writes **MUST** check `GroupV2(id)` first and fall back to legacy `Group(id)`
only if V2 is absent. On successful write the entry **MUST** be stored under
`GroupV2(id)` and the legacy `Group(id)` entry removed atomically in the same
transaction. Two concurrent state-changing transactions on the same legacy group
still cannot race because Soroban serialises contract invocations on a given
contract/key pair; the second transaction sees the V2 entry written by the first.
History entries under `History(id)` are **not** rewritten (they are read-only
archives).

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
    member_count: u32,                    // founding member count
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
    Democracy {
        member_count_old: u32, // MUST equal stored.member_count
        member_count_new: u32, // stored on success; circuit-bound to c_new
    },
    Oligarchy {
        admin_root_old: BytesN<32>, // MUST equal AdminSet(group_id)
    },
}
```

For Anarchy, `extra = None` (wire-compatible with today's clients that call the
legacy `update_commitment`). For 1v1, the function returns `OneOnOneImmutable`
unconditionally.

**Why `admin_root_old` is passed by the caller even though the contract could read it
from storage.** The `admin_root_old` is a **Groth16 public input** bound by the
Oligarchy circuit to the rest of the proof. It **must** be presented to the
verifier, and that value is what the caller packages into `GovernancePublicInputs`.
The contract additionally validates `extra.admin_root_old == AdminSet(group_id)` as
defence-in-depth — this guarantees the verified proof was bound to the *current*
admin set, not some historical one. Without the mismatch check, a caller could
replay an old-epoch proof against a still-accepted old admin root. The apparent
redundancy is intentional belt-and-braces.

Democracy's `member_count_old` is handled differently: the contract **overrides** any
caller-supplied value with the stored authoritative count before passing it to the
verifier (and rejects mismatches with `MemberCountMismatch`). This is what closes
the quorum-bypass gap — the caller cannot influence the value the circuit sees.

#### 6.3.1 New error codes

```rust
OneOnOneImmutable     = 17, // update_commitment on group_type == 1
UnknownGroupType      = 18,
MissingAdminRoot      = 19, // create_group_v2 with group_type == 3 and admin_root = None
AdminRootMismatch     = 20, // Oligarchy extra.admin_root_old != storage
DemocracyInputMissing = 21, // group_type == 2 and extra != Some(Democracy{..})
NotAdmin              = 22, // update_admin_set proof fails admin membership
Invalid1v1Tier        = 23, // create_group_v2 with group_type == 1 and tier != 0
MemberCountMismatch   = 24, // Democracy extra.member_count_old != stored.member_count
MemberCountOutOfRange = 25, // Democracy extra.member_count_new outside {m-1, m, m+1}
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

- **Purpose:** prove that a Commit is authorised by at least half of the current
  members, for an authoritative `n` pinned on-chain.
- **Tier × K_max matrix:** compiled per tier and per maximum signer count `K_max`.
  v1 ships `(tier=0, K_max=32)` and `(tier=1, K_max=256)`; tier 2 is deferred.
- **Public inputs:** `c_old`, `epoch_old`, `c_new`, `member_count_old`,
  `member_count_new`.
- **Witnesses:** ordered list of `K` tuples `(sk_i, merkle_path_i, leaf_idx_i)` for
  `1 ≤ K ≤ K_max`; `root_old`, `root_new`, `salt_old`, `salt_new`; delta witness
  identifying the single leaf position changed between `root_old` and `root_new`.
- **Key constraints:**
  1. `K ≥ 1` (no zero-opening Commits even if `member_count_old = 0`).
  2. For each `i ∈ [0, K)`, `Poseidon(sk_i)` opens to `root_old` along
     `merkle_path_i` at `leaf_idx_i`.
  3. Indices `leaf_idx_0 < leaf_idx_1 < … < leaf_idx_{K-1}` (strict ascending, gives
     distinctness for free).
  4. `2 · K ≥ member_count_old` (the ≥50% threshold).
  5. `member_count_old ≤ 2^tier_depth` and `member_count_new ≤ 2^tier_depth`.
  6. `|member_count_new − member_count_old| ≤ 1` — exactly one leaf position differs
     between `root_old` and `root_new`, and the leaf-change pattern (zero→nonzero,
     nonzero→zero, or nonzero→nonzero) matches the count delta.
  7. `c_old = Poseidon(Poseidon(root_old, epoch_old), salt_old)` — reuses the
     existing commitment gadget.
  8. `c_new = Poseidon(Poseidon(root_new, epoch_old + 1), salt_new)`.
- **Authoritative `n` binding.** `member_count_old` is **not** bound cryptographically
  to `c_old`. Instead, soundness is provided by the contract pre-check
  `extra.member_count_old == stored.member_count` (§6.1, §6.3). The contract is the
  source of truth; the caller cannot influence the verifier input. `member_count_new`
  is circuit-bound to `c_new` via constraint 6 and is stored on success.
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

**v1 rollout status (PR #77).** `OligarchyUpdateCircuit` is **not shipped in
PR #77**. In v1 the member-rekey path is intentionally frozen for Oligarchy
groups: `update_commitment` rejects `group_type == 3` with `UnknownGroupType`,
and no `update_commitment_oligarchy` dispatcher is registered. This is the
fail-closed posture — better than routing Oligarchy through the shared
`UpdateCircuit` VK (which would let any member rekey, silently degrading
Oligarchy to Anarchy for member ops). What v1 *does* support for Oligarchy:

| Op | Status in v1 | Path |
|---|---|---|
| Create group with admin set | ✓ ships | `create_oligarchy_group` |
| Rotate admin set | ✓ ships | `update_admin_commitment` (uses new `AdminUpdate` VK) |
| Rekey member set (add/remove/replace a member) | ✗ frozen | Returns `UnknownGroupType` until §6.4.3 ships |

The practical consequence: Oligarchy groups created on v1 have a **fixed
member roster** but a **rotatable admin set**. Admin churn works; member
churn requires a follow-up release carrying `OligarchyUpdateCircuit` + the
contract dispatcher that routes `group_type == 3` through it. Clients
that expose Oligarchy must gate the "remove member"/"invite" UI behind a
feature flag tied to the circuit's availability.

#### 6.4.4 `AdminUpdateCircuit`

Shape-identical to the existing `UpdateCircuit`, but over the admin tree:

- **Public inputs:** `admin_c_old`, `admin_epoch_old`, `admin_c_new` (the *salted*
  admin commitments, not bare Merkle roots — see §6.2.2).
- **Constraints:** identical to `UpdateCircuit` with tree variables rebound to the
  admin tree; binds `admin_c_old = Poseidon(Poseidon(admin_root_old, admin_epoch_old),
  admin_salt_old)` and similarly for the new admin commitment.
- **Admin ∈ Member invariant (intentionally *not* circuit-enforced).** The circuit
  proves only that the prover is in the admin tree; it does **not** cross-check the
  member tree. A client could therefore rotate the admin set to include a pubkey
  that is not in the member tree. The resulting non-member admin can further rotate
  the admin set but *cannot* produce an `OligarchyUpdateCircuit` proof (constraint 2
  of §6.4.3 demands member-tree membership). The worst-case damage is confined to
  admin-set churn; no member-level authority leaks. Enforcing the invariant in-
  circuit would require threading the member root into `AdminUpdateCircuit` public
  inputs, roughly doubling its constraint count. v1 treats this as a client-enforced
  convention (iOS/Android UIs only surface "Promote" for current members); §8 T10
  documents the residual risk.
- **Storage:** `DataKey::AdminUpdateVK` (a single VK, tier 0 fixed; see §6.2.2 admin
  tree tier note).

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
  `ballot_id`. This exposes voter identity *within* the group and makes votes on
  successive ballots linkable for as long as a member's leaf is stable. It is **not**
  on-chain leakage. See §8 T11 for the full treatment; clients **SHOULD** surface a
  one-time notice to users that other members can see how they voted.
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
- 1v1: no kick affordance. A "Close Conversation" button replaces it (semantically
  calls `deactivate_group` — "leave" is a misnomer since deactivation freezes the
  group for both participants).
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
- `test_1v1_create_reject_tier_nonzero` — tier 1 or 2 with `group_type = 1` returns
  `Invalid1v1Tier`.
- `test_1v1_update_rejected` — `update_commitment` on 1v1 group returns
  `OneOnOneImmutable`.
- `test_1v1_deactivate_success` — either member can deactivate.

**Democracy.**
- `test_democracy_majority_accepts` — n=5, K=3, success.
- `test_democracy_sub_majority_rejects` — n=5, K=2, `InvalidProof`.
- `test_democracy_single_opening_rejects_when_n_large` — n=100, K=1, `InvalidProof`
  (regression against the member-count-spoofing vulnerability).
- `test_democracy_duplicate_vote_rejects` — K=3 but two paths to same leaf,
  `InvalidProof` (strict-ascending gadget catches this).
- `test_democracy_even_boundary` — n=4, K=2 accepts (2·2 ≥ 4).
- `test_democracy_member_count_spoofing_rejected` — caller passes
  `extra.member_count_old = 2` when stored is `100`; contract rejects with
  `MemberCountMismatch` before proof verification.
- `test_democracy_member_count_new_out_of_range` — caller passes
  `member_count_new = stored + 5`; contract rejects with `MemberCountOutOfRange`.
- `test_democracy_member_count_one` — n=1, K=1 accepts (edge case; degenerate but
  valid since `2·1 ≥ 1` and `K ≥ 1`).
- `test_democracy_zero_opening_rejected` — K=0 with spoofed `member_count_old = 0`
  rejected by the `K ≥ 1` constraint even if the contract pre-check passes.
- `test_democracy_stored_count_tracks` — successive adds and removes: stored
  `member_count` reflects each ±1 change across epochs.

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
- `test_oligarchy_deactivate_by_any_member` — non-admin member calls
  `deactivate_group`; succeeds (§6.1 safety valve).

**Dispatch & migration.**
- `test_unknown_group_type_rejected` — `create_group_v2(group_type = 99)` returns
  `UnknownGroupType`.
- `test_legacy_lazy_upgrade_on_write` — write path correctly produces a `GroupV2`
  record and removes the legacy entry.
- `test_v2_precedence_on_concurrent_upgrade` — sequential V2-write then legacy-read
  returns V2 data (no fallback shadowing).

**Replay.** All new proof types participate in the `UsedProof` replay tracking at
`lib.rs:824,836-838`.

### 7.2 Cross-platform test vectors

Extend `docs/cross-platform-test-vectors.json`
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

*Storage-cost side effect.* Each successful `update_commitment*` or
`update_admin_commitment` writes a `UsedProof(proof_digest)` ledger entry
with a 30-day TTL (`LEDGER_BUMP = 518_400` ledgers, ≈5 s/ledger). At
sustained steady state with `G` active groups averaging `r` Commits/day,
the concurrent `UsedProof` footprint is `30·G·r` entries; with Soroban's
per-entry overhead of ~100 bytes (key + metadata) that's `3·G·r` KB of
persistent state. Sample points: 1 000 groups × 1 Commit/day → ~3 MB;
10 000 groups × 10 Commits/day → ~90 MB. The 30-day TTL amortises cost
because nobody has any incentive to bump `UsedProof` entries (they are
useful only as anti-replay and become inert once the caller could no
longer replay anyway — the contract already re-derives `c_old` from
storage, so a 30+ day old proof binds to a state the caller cannot
produce), so entries expire naturally. Soroban rent is charged per
ledger-entry per ledger, so the contract's steady-state rent cost scales
linearly with `G·r`; the contract admin pays this unless a future
revision passes the cost through to the submitter. No protocol-soundness
bearing — documented for operational planning.

**T6 — Member-count integrity (Democracy, quorum-bypass class).** If the value of
`n` used by the Democracy circuit were caller-supplied and unverified, a single
member could spoof `member_count_old = 2` in an `n = 100` group and satisfy
`2·K ≥ member_count_old` with a lone co-opening — trivially collapsing Democracy to
Anarchy. **Mitigation (specified, not deferred):** `member_count` is stored on-chain
in `CommitmentEntryV2`, written by `create_group_v2`, updated on every successful
`update_commitment`, and **overridden** by the contract before proof verification
(§6.1, §6.3). Any caller-supplied mismatch is rejected with `MemberCountMismatch`
before any pairing check runs. Combined with the in-circuit `K ≥ 1` constraint
(§6.4.2), a quorum-bypass by count spoofing is impossible.

**Residual privacy leak.** The stored `member_count` is plaintext on chain; an
observer reads the exact count at every Democracy transition. The tier already
leaks an upper bound, so exact-count disclosure is a bounded additional concession.
Binding `n` inside the commitment formula (as a privacy hardening — not a
correctness fix) is tracked in §10 as a follow-up.

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

**T10 — Non-member admin in Oligarchy.** Because `AdminUpdateCircuit` does not
cross-check the member tree (§6.4.4), a malicious admin could rotate the admin set
to include a BLS pubkey that is not a current member. The resulting "ghost admin"
can further rotate the admin set (admin-tree membership alone is sufficient for
`update_admin_set`) but **cannot** produce a valid `OligarchyUpdateCircuit` proof
(which demands member-tree membership too) and therefore cannot modify the member
set. Damage is confined to admin-set churn. **Mitigation:** iOS and Android
`GroupInfoView.swift` surface the "Promote" affordance only on current members; a
server-side linter could also validate admin-tree transitions against the latest
member tree post-Commit. A future revision **MAY** enforce admin ∈ member inside
the circuit at the cost of ~2× constraint count.

**T11 — Democracy voter linkability within group.** The co-opening protocol (§6.6)
reveals each voter's Poseidon leaf to the coordinator and to any member who ingests
the ballot payload. Because a member's leaf is stable across epochs (until they
rotate keys), a member's vote on ballot *X* is cryptographically linkable to their
vote on ballot *Y*. This is leakage *within* the encrypted group channel — no
external observer learns anything — and is consistent with how physical group votes
work. Clients **SHOULD** document this to users ("others in the group can see how
you voted"). No on-chain leak.

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
