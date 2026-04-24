//! SEP-XXXX Soroban Contract — Private Group Membership Registry
//!
//! Stores group commitments on-chain and verifies Groth16 membership
//! proofs using BLS12-381 host functions. No member identity ever
//! appears on-chain.
//!
//! # Verification
//!
//! The Groth16 verification equation:
//!   e(π_A, π_B) = e(α, β) · e(vk_x, γ) · e(π_C, δ)
//!
//! is checked as a multi-pairing:
//!   e(-π_A, π_B) · e(α, β) · e(vk_x, γ) · e(π_C, δ) = 1_GT
//!
//! where vk_x = IC[0] + commitment·IC[1] + epoch·IC[2].
//!
//! All curve operations use Soroban's BLS12-381 host functions.

#![no_std]
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype,
    crypto::bls12_381::{Fr, G1Affine, G2Affine},
    vec, Address, Bytes, BytesN, Env, Vec, U256,
};

// ================================================================
// Constants
// ================================================================

/// Maximum number of history entries retained per group.
///
/// N-21: Older entries are pruned from contract state but remain permanently
/// available via contract events (GroupCreated, CommitmentUpdated, GroupDeactivated).
/// Off-chain event indexing is required for full audit trail beyond this window.
const HISTORY_WINDOW: u32 = 64;

/// Minimum TTL threshold for persistent storage (ledgers, ~1 day).
const LEDGER_THRESHOLD: u32 = 17_280;

/// TTL bump amount for persistent storage (ledgers, ~30 days).
const LEDGER_BUMP: u32 = 518_400;

/// TTL bump for `GroupCounted` receipts (~60 days).
///
/// Audit re-review Finding #3: the per-group receipt that enables
/// `reconcile_tier_count` MUST outlive the group's `GroupV2` entry so
/// that after the group goes cold (no calls, `GroupV2` expires) there
/// is still a window during which anyone can observe the dangling
/// tier-count slot and clean it up. Using a bump twice the group TTL
/// gives operators a ~30-day grace window after cold-group expiry to
/// run reconciliation. Stored in persistent storage (not instance)
/// so each receipt carries its own expiry and does not consume the
/// ~64KB shared instance budget.
const GROUP_COUNTED_LEDGER_BUMP: u32 = LEDGER_BUMP * 2;

/// Maximum number of active groups allowed per tier (M-4: storage abuse prevention).
/// The admin can increase this limit by re-deploying with a higher value.
const MAX_GROUPS_PER_TIER: u32 = 10_000;

/// Number of filled leaves a Merkle tree of the given tier can hold.
/// * tier 0 (Small)  — depth 5  → 32
/// * tier 1 (Medium) — depth 8  → 256
/// * tier 2 (Large)  — depth 11 → 2048
///
/// Callers MUST validate `tier <= 2` before invoking; behaviour for
/// out-of-range tiers is undefined.
fn tier_capacity(tier: u32) -> u32 {
    match tier {
        0 => 32,
        1 => 256,
        2 => 2048,
        _ => 0,
    }
}

// Replay-nullifier scoping — see `proof_hash`.
//
// The nullifier is a global `sha256(proof.a || proof.b || proof.c)`:
// once ANY proof bytes have been submitted to ANY state-changing
// entrypoint, those exact bytes cannot be resubmitted to ANY entrypoint
// on ANY group. This is intentionally the strongest available contract-
// level scope because the MembershipCircuit does not bind `group_id` or
// an operation tag, so two groups sharing `(commitment, epoch)` — e.g.
// a deliberate clone of a target group's initial membership tree —
// would otherwise accept the same proof bytes on both groups.
//
// An earlier revision of this contract tried `sha256(group_id || proof)`
// to bound per-group storage growth, but that weakened cross-group
// protection: an attacker who observed a proof against group A could
// replay it against a cloned group B. The storage-growth concern is
// subsumed by `MAX_GROUPS_PER_TIER` and by the natural TTL expiry of
// `UsedProof` entries.
//
// ── What contract-level nullifiers CANNOT close ────────────────────
//
// 1. Pre-inclusion mempool / front-run replay. The nullifier is only
//    recorded after the honest caller's transaction lands. A watcher
//    that sees the pending transaction can resubmit the same proof
//    bytes to `deactivate_group` and win ordering. `verify_membership`
//    itself is non-state-changing and leaks the proof to any observer
//    of the RPC simulation. Contract-level nullifiers protect only
//    post-inclusion replay; pre-inclusion protection requires the
//    circuit to bind an operation tag (so a VERIFY proof is not a
//    valid DEACTIVATE proof) plus `group_id` and the calling address.
//
// 2. Long-lived groups whose `UsedProof` entries expire. TTL is
//    `LEDGER_BUMP` (~30 days). A group that never advances
//    `(commitment, epoch)` allows a create-time proof to be replayed
//    once its nullifier has expired. Extending per-proof TTL forever
//    is not a contract-only fix (rent is unbounded). Enumerating
//    per-group nullifiers grows without bound and still enables
//    pre-inclusion replay.
//
// ── Real closure: MembershipCircuit v2 ─────────────────────────────
//
// A circuit rotation that binds — at minimum — an operation tag
// (CREATE / VERIFY / DEACTIVATE / UPDATE / UPDATE_ADMIN), `group_id`,
// and a per-group monotonic `proof_nonce` as public inputs would make
// a leaked proof useless outside the exact op+group+nonce it was
// generated for. Optional additions: `caller` (with
// `require_auth()`), `expires_at_ledger`, and an `intent_hash` for
// challenge-bound attestations. Tracked as the v2 circuit follow-up;
// until then the current global nullifier is defence-in-depth, not
// primary replay protection.

// ================================================================
// Errors
// ================================================================

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// Contract has not been initialized yet.
    NotInitialized = 1,
    /// Contract is already initialized.
    AlreadyInitialized = 2,
    /// Reserved (was: Unauthorized). Admin checks use require_auth() which panics.
    Reserved3 = 3,
    /// A group with this ID already exists.
    GroupAlreadyExists = 4,
    /// No group exists with this ID.
    GroupNotFound = 5,
    /// Group has been deactivated.
    GroupInactive = 6,
    /// Groth16 proof verification failed.
    InvalidProof = 7,
    /// Tier must be 0 (Small), 1 (Medium), or 2 (Large).
    InvalidTier = 8,
    /// Verification key IC vector must have exactly 3 elements.
    InvalidVkLength = 9,
    /// Caller-supplied public inputs do not match on-chain state.
    PublicInputsMismatch = 10,
    /// Caller-supplied epoch is not exactly stored_epoch + 1.
    InvalidEpoch = 11,
    /// This proof has already been used (replay detected).
    ProofReplay = 12,
    /// Maximum number of groups for this tier has been reached.
    TierGroupLimitReached = 13,
    /// Restricted mode: only admin can create groups (N-26).
    AdminOnly = 14,
    /// A supplied commitment is not a canonical BLS12-381 Fr encoding
    /// (>= modulus, or otherwise fails the roundtrip canonical check).
    /// Introduced with the UpdateCircuit binding fix (#59).
    InvalidCommitmentEncoding = 15,
    /// An unsupported `VkKind` value was passed to `update_vk`.
    /// Introduced with the UpdateCircuit binding fix (#59).
    UnknownVkKind = 16,
    /// `update_commitment` was called on a 1v1 group. 1v1 groups are
    /// immutable after creation — the only valid state transition is
    /// `deactivate_group`, which either participant can call via their
    /// membership proof.
    OneOnOneImmutable = 17,
    /// `group_type` is not a known value (0 = Anarchy, 1 = 1v1, 2 = Democracy,
    /// 3 = Oligarchy) or is not yet enabled in this contract version.
    /// Introduced with the group-governance-types design.
    UnknownGroupType = 18,
    /// Oligarchy operation requested on a group that has no admin set
    /// stored (either the group isn't Oligarchy, or the admin seed is
    /// missing — should never happen for a well-formed Oligarchy group).
    MissingAdminRoot = 19,
    /// The supplied `admin_root` public input does not match the current
    /// stored admin-tree root for the group.
    AdminRootMismatch = 20,
    /// A Democracy update omitted the member-count public input, or the
    /// caller failed to supply the quorum witness bundle.
    /// Reserved for the ceremony-gated update dispatch.
    DemocracyInputMissing = 21,
    // Code 22 reserved for `NotAdmin` (Oligarchy update dispatch).
    /// 1v1 groups MUST use tier 0 (Small). Any other tier is rejected.
    Invalid1v1Tier = 23,
    /// The `member_count` public input for a Democracy update does not
    /// match the authoritative value stored on-chain.
    /// Reserved for the ceremony-gated update dispatch.
    MemberCountMismatch = 24,
    /// `member_count` is out of range for the requested tier / group type
    /// (e.g. < 2 for Democracy, or > 2^depth for the selected tier).
    MemberCountOutOfRange = 25,
    /// A BLS12-381 curve point (from a proof or a VK) is not in the
    /// prime-order subgroup. Surfaced by the audit-2026-04 hardening that
    /// validates VK points at install time and proof points at verify time,
    /// so invalid-subgroup inputs can never reach `pairing_check`.
    InvalidPoint = 26,
    /// `reconcile_tier_count` was called on a group whose state storage is
    /// still live. The correct decrement path is `deactivate_group`.
    /// Audit-followup Finding #6.
    GroupStillActive = 27,
}

// ================================================================
// Events
// ================================================================

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupCreated {
    #[topic]
    pub group_id: BytesN<32>,
    pub commitment: BytesN<32>,
    pub epoch: u64,
    pub tier: u32,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentUpdated {
    #[topic]
    pub group_id: BytesN<32>,
    pub commitment: BytesN<32>,
    pub epoch: u64,
    pub timestamp: u64,
}

/// Oligarchy admin-set rotation event. Emitted by
/// `update_admin_commitment` on success; the `admin_commitment` is the
/// salted Poseidon commitment over the new admin tree (§6.2.2).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminCommitmentUpdated {
    #[topic]
    pub group_id: BytesN<32>,
    pub admin_commitment: BytesN<32>,
    pub admin_epoch: u64,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupDeactivated {
    #[topic]
    pub group_id: BytesN<32>,
    pub final_epoch: u64,
    pub timestamp: u64,
}

/// Audit-followup Finding #6: emitted by `reconcile_tier_count` so
/// operators can audit tier-counter reconciliations.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TierCountReconciled {
    #[topic]
    pub group_id: BytesN<32>,
    pub tier: u32,
    pub new_count: u32,
}

// ================================================================
// Types
// ================================================================

/// On-chain state of a group at a particular epoch.
///
/// This is the legacy (pre-governance-types) record format. The contract
/// continues to read this format for groups created before the V2 migration,
/// but every state-changing write produces a `CommitmentEntryV2`. See
/// `load_group_v2` for the read-path fallback semantics.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentEntry {
    /// Poseidon commitment (BLS12-381 field element, 32 bytes big-endian).
    pub commitment: BytesN<32>,
    /// Epoch counter (starts at 0, increments by 1).
    pub epoch: u64,
    /// Ledger timestamp when this state was recorded.
    pub timestamp: u64,
    /// Circuit tier: 0 = Small (32 members), 1 = Medium (256), 2 = Large (2048).
    pub tier: u32,
    /// Whether the group accepts further updates.
    pub active: bool,
}

/// On-chain state of a group, extended with governance-type metadata.
///
/// Introduced by the group-governance-types design. Every write produced by
/// Phase-0 (and later) code paths is a `CommitmentEntryV2`. Legacy
/// `CommitmentEntry` records are treated as `group_type = 0` (Anarchy) with
/// `member_count = 0` (unknown) when read via `load_group_v2`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentEntryV2 {
    /// Poseidon commitment (BLS12-381 field element, 32 bytes big-endian).
    pub commitment: BytesN<32>,
    /// Epoch counter (starts at 0, increments by 1).
    pub epoch: u64,
    /// Ledger timestamp when this state was recorded.
    pub timestamp: u64,
    /// Circuit tier: 0 = Small, 1 = Medium, 2 = Large.
    pub tier: u32,
    /// Whether the group accepts further updates.
    pub active: bool,
    /// Governance type: 0 = Anarchy, 1 = 1v1, 2 = Democracy, 3 = Oligarchy.
    pub group_type: u32,
    /// Current member count. Populated for Democracy groups so the quorum
    /// check is bound to authoritative state instead of client-supplied input.
    /// For other types this is informational; `0` means "not tracked".
    pub member_count: u32,
}

/// Public inputs for Groth16 proof verification.
///
/// Callers MUST supply these explicitly; the contract verifies they
/// match the on-chain state before using them in the pairing check.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicInputs {
    /// The commitment being proved against.
    pub commitment: BytesN<32>,
    /// The epoch being proved against.
    pub epoch: u64,
}

/// Public inputs for the update-transition Groth16 proof (`UpdateCircuit`).
///
/// The circuit proves that a valid member of the group at `(c_old, epoch_old)`
/// authorises a transition to `c_new = Poseidon(Poseidon(root_new, epoch_old + 1), salt_new)`.
/// All three fields are cryptographically bound by the proof — none are free
/// for the caller to mutate post-proof (#59 fix).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdatePublicInputs {
    /// Pre-transition commitment. MUST equal the current on-chain commitment.
    pub c_old: BytesN<32>,
    /// Pre-transition epoch. MUST equal the current on-chain epoch.
    pub epoch_old: u64,
    /// Post-transition commitment. Persisted on success; cryptographically
    /// bound by the proof (this is the whole point of `UpdateCircuit`).
    pub c_new: BytesN<32>,
}

/// Public inputs for the Democracy update circuit — see
/// `src/circuit/democracy.rs` and `docs/group-governance-types-design.md
/// §6.4.2`. Five fields in a fixed order that MUST match the circuit's
/// allocation order: c_old, epoch_old, c_new, member_count_old,
/// member_count_new.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DemocracyUpdatePublicInputs {
    /// Pre-transition commitment. MUST equal the current on-chain commitment.
    pub c_old: BytesN<32>,
    /// Pre-transition epoch. MUST equal the current on-chain epoch.
    pub epoch_old: u64,
    /// Post-transition commitment. Persisted on success.
    pub c_new: BytesN<32>,
    /// Pre-transition member count. MUST equal the stored `member_count` —
    /// the authoritative quorum-denominator binding (§6.4.2).
    pub member_count_old: u32,
    /// Post-transition member count. Must be in `{m-1, m, m+1}` relative to
    /// `member_count_old`; persisted on success.
    pub member_count_new: u32,
}

/// Public inputs for the Oligarchy admin-rotation circuit (§6.4.4). Shape
/// is identical to `UpdatePublicInputs` — the circuit is `UpdateCircuit`
/// rebound to the admin tree, so the three salted-commitment fields fix
/// the transition over the admin commitment, not the member commitment.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminUpdatePublicInputs {
    /// Pre-transition admin commitment. MUST equal the stored `AdminSet`.
    pub admin_c_old: BytesN<32>,
    /// Pre-transition admin epoch. MUST equal the stored `AdminEpoch`.
    pub admin_epoch_old: u64,
    /// Post-transition admin commitment; persisted on success.
    pub admin_c_new: BytesN<32>,
}

/// Selector for which verification-key family is being set or rotated.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VkKind {
    /// Membership circuit VK (3 IC points: IC[0], IC[1]=commitment, IC[2]=epoch).
    Membership,
    /// Update-transition circuit VK (4 IC points: IC[0], IC[1]=c_old, IC[2]=epoch_old, IC[3]=c_new).
    Update,
    /// Per-group-type update VK (6 IC points for Democracy:
    /// IC[0], IC[1]=c_old, IC[2]=epoch_old, IC[3]=c_new,
    /// IC[4]=member_count_old, IC[5]=member_count_new).
    ///
    /// Installed by the admin after the per-type Phase 2 ceremony concludes
    /// (see `docs/democracy-circuit-ceremony.md §3`). `group_type = 2`
    /// routes `update_commitment_democracy` through this VK family.
    UpdateByType(u32),
    /// Admin-rotation circuit VK for Oligarchy groups (4 IC points, same
    /// shape as `Update`). Routed by `update_admin_commitment` via
    /// `DataKey::AdminUpdateVK`. The `tier` argument to `update_vk` is
    /// ignored for this variant — admin trees are single-tier per §6.4.4.
    AdminUpdate,
}

/// Groth16 verification key stored as raw bytes (contract-storage friendly).
///
/// **Design note (M-9):** Verification keys are stored per tier, not per group.
/// All groups of the same tier share a VK. This is intentional: the circuit is
/// parameterized only by tree depth (tier), so all groups of a given tier use
/// the same circuit and therefore the same VK. If the VK needs rotation (e.g.,
/// circuit bug), all groups of that tier are affected. A future per-group VK
/// override can be added via `DataKey::GroupVK(BytesN<32>)` with fallback to
/// the tier-level VK if not set.
///
/// Points use the BLS12-381 uncompressed serialization:
///   G1 = x(48 bytes) || y(48 bytes) = 96 bytes
///   G2 = x0(48) || x1(48) || y0(48) || y1(48) = 192 bytes
#[contracttype]
#[derive(Clone, Debug)]
pub struct VerificationKeyData {
    /// α in G1 (96 bytes, uncompressed).
    pub alpha_g1: BytesN<96>,
    /// β in G2 (192 bytes, uncompressed).
    pub beta_g2: BytesN<192>,
    /// γ in G2 (192 bytes, uncompressed).
    pub gamma_g2: BytesN<192>,
    /// δ in G2 (192 bytes, uncompressed).
    pub delta_g2: BytesN<192>,
    /// IC[0..n] in G1 (96 bytes each).
    /// For this circuit: IC[0] (base), IC[1] (commitment), IC[2] (epoch).
    pub ic: Vec<BytesN<96>>,
}

/// Groth16 proof stored as raw bytes (contract-parameter friendly).
///
/// 96 + 192 + 96 = 384 bytes uncompressed.
/// (The SEP specifies 192-byte compressed proofs; clients decompress
/// before submitting to the contract.)
#[contracttype]
#[derive(Clone, Debug)]
pub struct Groth16Proof {
    /// π_A in G1 (96 bytes, uncompressed).
    pub a: BytesN<96>,
    /// π_B in G2 (192 bytes, uncompressed).
    pub b: BytesN<192>,
    /// π_C in G1 (96 bytes, uncompressed).
    pub c: BytesN<96>,
}

// ================================================================
// Storage Keys
// ================================================================

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Contract admin address (instance storage).
    Admin,
    /// Membership-circuit verification key for tier 0/1/2 (persistent storage).
    VK(u32),
    /// Update-circuit verification key for tier 0/1/2 (persistent storage).
    /// Introduced with the #59 UpdateCircuit binding fix. Separate from `VK`
    /// so the two key families can rotate independently.
    UpdateVK(u32),
    /// Per-group-type update VK (persistent storage). Installed by the admin
    /// after the governance-specific Phase 2 ceremony concludes. `group_type =
    /// 2` (Democracy) uses 6 IC points; future types may use different
    /// shapes.
    UpdateVKByType(u32, u32),
    /// Current group state (persistent storage).
    ///
    /// Legacy key. Reads fall back to this when `GroupV2` is absent; writes
    /// always target `GroupV2` and delete this key during lazy migration.
    Group(BytesN<32>),
    /// Group history — rolling window of past entries (persistent storage).
    /// Legacy key; superseded by `HistoryV2` via the same migration rules.
    History(BytesN<32>),
    /// Current group state in V2 format (persistent storage).
    /// Produced by all post-migration writes.
    GroupV2(BytesN<32>),
    /// Group history in V2 format (persistent storage).
    ///
    /// **IMPORTANT (audit-2026-04 LOW-4):** despite the V2 name, the stored
    /// value is `Vec<CommitmentEntry>` — the V1 struct — NOT
    /// `Vec<CommitmentEntryV2>`. History entries intentionally drop
    /// `group_type` (invariant per group, recoverable from the current V2
    /// record) and `member_count` (not needed for historical queries, which
    /// serve the rolling audit window). Any future refactor that needs
    /// governance metadata in history MUST introduce a new key
    /// (`HistoryV3` or similar) with explicit conversion logic — do NOT
    /// silently change the value type stored under this key, as that
    /// would break decode on every existing group.
    HistoryV2(BytesN<32>),
    /// Oligarchy admin-set salted Poseidon commitment (persistent storage).
    /// Stores `admin_commitment = Poseidon(Poseidon(admin_root, admin_epoch),
    /// admin_salt)` per §6.2.2. Set at create time and rotated by
    /// `update_admin_commitment` once the `AdminUpdateVK` is installed.
    AdminSet(BytesN<32>),
    /// Monotonic admin-tree epoch for an Oligarchy group (persistent storage).
    /// Bumped by `update_admin_commitment`. Missing == 0 for groups created
    /// before this field existed (migration no-op: the first admin update
    /// after deployment consumes epoch 0).
    AdminEpoch(BytesN<32>),
    /// Admin-rotation circuit verification key (persistent storage). Single
    /// key, no tier parameter — admin trees are capped at 32 entries per
    /// §6.2.2 admin-tier note, and the circuit is shape-identical to
    /// `UpdateCircuit` (4 IC points). Installed via
    /// `update_vk(VkKind::AdminUpdate, 0, …)`.
    AdminUpdateVK,
    /// Used proof hash — prevents cross-function and cross-group proof replay.
    UsedProof(BytesN<32>),
    /// Active group count per tier (instance storage, for M-4 limit enforcement).
    GroupCount(u32),
    /// When true, only the admin can create new groups (N-26 access control).
    RestrictedMode,
    /// Per-group "this id contributed +1 to GroupCount(tier)" receipt.
    ///
    /// Stored in PERSISTENT storage with its own TTL
    /// (`GROUP_COUNTED_LEDGER_BUMP`, ~60 days — twice `LEDGER_BUMP`).
    /// The invariant we depend on is:
    ///
    ///   GroupCounted(id) expiry > GroupV2(id) expiry
    ///
    /// An intermediate audit revision put this receipt in INSTANCE
    /// storage to guarantee the above without a custom TTL, but
    /// instance storage is bounded (~64KB shared across all groups),
    /// which would have capped live groups per tier well below
    /// `MAX_GROUPS_PER_TIER`. Persistent storage with an explicit
    /// longer TTL gives the same reconciliation guarantee without the
    /// capacity regression: `bump_group` extends `GroupV2` by
    /// `LEDGER_BUMP` and the receipt by `GROUP_COUNTED_LEDGER_BUMP`,
    /// so after a group goes cold the receipt survives for
    /// ~`LEDGER_BUMP` longer than the group itself — the window
    /// during which `reconcile_tier_count` can decrement the slot.
    ///
    /// Written at every group-creation path; deleted by
    /// `deactivate_group` or `reconcile_tier_count`. The stored value
    /// is the tier the group was counted in, so reconciliation knows
    /// which `GroupCount(tier)` slot to decrement without trusting
    /// caller input.
    GroupCounted(BytesN<32>),
}

// ================================================================
// Contract
// ================================================================

#[contract]
pub struct SepXxxxContract;

#[contractimpl]
impl SepXxxxContract {
    // ---- Admin ----

    /// Soroban constructor — runs atomically as part of contract deployment.
    ///
    /// Audit-followup Finding #5: deploy+init-in-one-txn closes the window
    /// where a front-runner could call `initialize` with their own keys
    /// between deploy and the legitimate init call. Requires `admin` auth
    /// the same way `initialize` does; modern Soroban deploys sign both the
    /// deploy and the constructor invocation together.
    ///
    /// Arguments, checks, and side-effects are identical to `initialize`.
    /// `initialize` is retained as a migration path for contracts that were
    /// deployed before this constructor existed (pre-v1.10.18 testnet /
    /// mainnet instances) and for test environments that deploy raw WASM.
    pub fn __constructor(
        env: Env,
        admin: Address,
        vk_small: VerificationKeyData,
        vk_medium: VerificationKeyData,
        vk_large: VerificationKeyData,
        update_vk_small: VerificationKeyData,
        update_vk_medium: VerificationKeyData,
        update_vk_large: VerificationKeyData,
    ) -> Result<(), Error> {
        Self::do_initialize(
            &env,
            admin,
            vk_small,
            vk_medium,
            vk_large,
            update_vk_small,
            update_vk_medium,
            update_vk_large,
        )
    }

    /// Initialize the contract with verification keys for all three tiers.
    ///
    /// Must be called exactly once. The `admin` address is recorded and
    /// required for future admin operations. Each membership VK must have
    /// exactly 3 IC points (commitment + epoch); each update VK must have
    /// exactly 4 IC points (c_old + epoch_old + c_new). The update VKs are
    /// introduced by the #59 fix to close the `update_commitment` binding gap.
    ///
    /// For new deployments prefer `__constructor`, which runs atomically
    /// with deploy. This entrypoint is retained so pre-constructor contract
    /// instances can still be initialized post-hoc.
    pub fn initialize(
        env: Env,
        admin: Address,
        vk_small: VerificationKeyData,
        vk_medium: VerificationKeyData,
        vk_large: VerificationKeyData,
        update_vk_small: VerificationKeyData,
        update_vk_medium: VerificationKeyData,
        update_vk_large: VerificationKeyData,
    ) -> Result<(), Error> {
        Self::do_initialize(
            &env,
            admin,
            vk_small,
            vk_medium,
            vk_large,
            update_vk_small,
            update_vk_medium,
            update_vk_large,
        )
    }

    fn do_initialize(
        env: &Env,
        admin: Address,
        vk_small: VerificationKeyData,
        vk_medium: VerificationKeyData,
        vk_large: VerificationKeyData,
        update_vk_small: VerificationKeyData,
        update_vk_medium: VerificationKeyData,
        update_vk_large: VerificationKeyData,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();

        if vk_small.ic.len() != 3 || vk_medium.ic.len() != 3 || vk_large.ic.len() != 3 {
            return Err(Error::InvalidVkLength);
        }
        if update_vk_small.ic.len() != 4
            || update_vk_medium.ic.len() != 4
            || update_vk_large.ic.len() != 4
        {
            return Err(Error::InvalidVkLength);
        }

        // Audit-2026-04 MEDIUM-1 + LOW-3: reject structurally-invalid VKs at
        // install time so no verification path can ever hand an invalid-
        // subgroup point to `pairing_check`.
        validate_vk_points(&vk_small)?;
        validate_vk_points(&vk_medium)?;
        validate_vk_points(&vk_large)?;
        validate_vk_points(&update_vk_small)?;
        validate_vk_points(&update_vk_medium)?;
        validate_vk_points(&update_vk_large)?;

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&DataKey::VK(0), &vk_small);
        env.storage()
            .persistent()
            .set(&DataKey::VK(1), &vk_medium);
        env.storage()
            .persistent()
            .set(&DataKey::VK(2), &vk_large);
        env.storage()
            .persistent()
            .set(&DataKey::UpdateVK(0), &update_vk_small);
        env.storage()
            .persistent()
            .set(&DataKey::UpdateVK(1), &update_vk_medium);
        env.storage()
            .persistent()
            .set(&DataKey::UpdateVK(2), &update_vk_large);

        for tier in 0..3u32 {
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::VK(tier), LEDGER_THRESHOLD, LEDGER_BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::UpdateVK(tier), LEDGER_THRESHOLD, LEDGER_BUMP);
        }

        Ok(())
    }

    // ---- Admin Operations ----

    /// N-12: Update a verification key for a specific tier and kind.
    ///
    /// Requires admin authorization. Membership VKs must have 3 IC points;
    /// update VKs must have 4 IC points. Enables independent key rotation
    /// per kind without contract redeployment.
    pub fn update_vk(
        env: Env,
        kind: VkKind,
        tier: u32,
        new_vk: VerificationKeyData,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        // AdminUpdate ignores tier (single-tier admin trees per §6.4.4).
        // All other kinds require a valid membership/update tier.
        if !matches!(kind, VkKind::AdminUpdate) && tier > 2 {
            return Err(Error::InvalidTier);
        }

        let (key, expected_ic_len) = match &kind {
            VkKind::Membership => (DataKey::VK(tier), 3u32),
            VkKind::Update => (DataKey::UpdateVK(tier), 4u32),
            VkKind::UpdateByType(group_type) => match *group_type {
                // Democracy: 5 public inputs → 6 IC points.
                2 => (DataKey::UpdateVKByType(tier, 2), 6u32),
                _ => return Err(Error::UnknownVkKind),
            },
            // AdminUpdate: 3 public inputs (admin_c_old, admin_epoch_old,
            // admin_c_new) → 4 IC points. Same shape as Update, different key.
            VkKind::AdminUpdate => (DataKey::AdminUpdateVK, 4u32),
        };

        if new_vk.ic.len() != expected_ic_len {
            return Err(Error::InvalidVkLength);
        }
        // Audit-2026-04 MEDIUM-1 + LOW-3: subgroup-validate every VK point
        // before persisting, so a later rotation cannot install a VK that
        // makes every subsequent verification trap.
        validate_vk_points(&new_vk)?;

        env.storage().persistent().set(&key, &new_vk);
        env.storage()
            .persistent()
            .extend_ttl(&key, LEDGER_THRESHOLD, LEDGER_BUMP);

        Ok(())
    }

    /// N-26: Toggle restricted mode. When enabled, only the admin can create groups.
    pub fn set_restricted_mode(
        env: Env,
        restricted: bool,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.storage().instance().set(&DataKey::RestrictedMode, &restricted);
        Ok(())
    }

    /// N-16: Extend the TTL of a group's persistent storage.
    ///
    /// Callable by anyone — prevents inactive groups from silently expiring.
    /// Groups that receive no state-changing operations for ~60 days would
    /// otherwise lose their on-chain data.
    pub fn bump_group_ttl(
        env: Env,
        group_id: BytesN<32>,
    ) -> Result<(), Error> {
        if !Self::group_exists(&env, &group_id) {
            return Err(Error::GroupNotFound);
        }
        Self::bump_group(&env, &group_id);
        Ok(())
    }

    /// Post-audit Finding #4: extend the TTL of all shared verification-key
    /// storage entries (per-tier Membership VKs, per-tier Update VKs,
    /// per-group-type Update VKs, and the AdminUpdate VK).
    ///
    /// Permissionless — anyone who cares about contract liveness (admin
    /// automation, watchdog scripts, dependent clients) can keep the
    /// verification machinery alive. `bump_group_ttl` does NOT bump these
    /// keys because per-group bumps would pay unnecessary cost on every
    /// group touch; a single `bump_vks` call amortises the cost globally.
    ///
    /// Silently no-ops for keys that have never been installed.
    pub fn bump_vks(env: Env) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        let storage = env.storage().persistent();
        for tier in 0u32..=2 {
            if storage.has(&DataKey::VK(tier)) {
                storage.extend_ttl(&DataKey::VK(tier), LEDGER_THRESHOLD, LEDGER_BUMP);
            }
            if storage.has(&DataKey::UpdateVK(tier)) {
                storage.extend_ttl(&DataKey::UpdateVK(tier), LEDGER_THRESHOLD, LEDGER_BUMP);
            }
            // Per-group-type update VKs. Democracy (2) is the only shape
            // currently defined; loop over known `group_type` values.
            for group_type in [2u32] {
                if storage.has(&DataKey::UpdateVKByType(tier, group_type)) {
                    storage.extend_ttl(
                        &DataKey::UpdateVKByType(tier, group_type),
                        LEDGER_THRESHOLD,
                        LEDGER_BUMP,
                    );
                }
            }
        }
        if storage.has(&DataKey::AdminUpdateVK) {
            storage.extend_ttl(&DataKey::AdminUpdateVK, LEDGER_THRESHOLD, LEDGER_BUMP);
        }
        // Instance storage carries the contract admin, restricted-mode flag,
        // and per-tier GroupCount — bump it here too so an automation that
        // only calls `bump_vks` also keeps those alive.
        env.storage()
            .instance()
            .extend_ttl(LEDGER_THRESHOLD, LEDGER_BUMP);
        Ok(())
    }

    /// Audit-followup Finding #6: permissionlessly reconcile
    /// `GroupCount(tier)` after a group has gone cold — its `GroupV2` /
    /// legacy `Group` entry has expired without `deactivate_group` ever
    /// being called — so a tier cannot be permanently pinned at the M-4
    /// limit by abandoned groups.
    ///
    /// Authorisation is cryptographic-at-creation, not sender-based:
    ///   1. `GroupCounted(group_id)` must still be present. This receipt
    ///      is written at every creation path and deleted by
    ///      `deactivate_group` after decrement, so its presence proves the
    ///      group was once counted and has not yet been reconciled.
    ///   2. Neither `GroupV2(group_id)` nor legacy `Group(group_id)` may
    ///      be present. If either is, the group still exists and the
    ///      correct decrement path is `deactivate_group`.
    ///
    /// Decrements `GroupCount(tier)` (tier read from the receipt) by one
    /// and removes the receipt so a second call cannot double-decrement.
    ///
    /// Returns `GroupStillActive` if the group's storage is still live and
    /// `GroupNotFound` if no reconciliation receipt exists (either never
    /// created at this contract, already reconciled, or both the group and
    /// receipt TTLs lapsed).
    pub fn reconcile_tier_count(env: Env, group_id: BytesN<32>) -> Result<(), Error> {
        Self::require_initialized(&env)?;

        if Self::group_exists(&env, &group_id) {
            return Err(Error::GroupStillActive);
        }

        let receipt_key = DataKey::GroupCounted(group_id.clone());
        let tier: u32 = env
            .storage()
            .persistent()
            .get(&receipt_key)
            .ok_or(Error::GroupNotFound)?;

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::GroupCount(tier))
            .unwrap_or(0);
        if count > 0 {
            env.storage()
                .instance()
                .set(&DataKey::GroupCount(tier), &(count - 1));
        }
        env.storage().persistent().remove(&receipt_key);

        TierCountReconciled {
            group_id,
            tier,
            new_count: count.saturating_sub(1),
        }
        .publish(&env);

        Ok(())
    }

    // ---- Group Operations ----

    /// Create a new private membership group (Anarchy governance).
    ///
    /// Kept for backward compatibility with pre-governance-types clients.
    /// Internally this delegates to `create_group_v2` with `group_type = 0`
    /// (Anarchy) and `member_count = 0` (untracked). The stored record
    /// uses the V2 format — the legacy `Group` key is no longer produced
    /// by post-migration contract builds.
    pub fn create_group(
        env: Env,
        caller: Address,
        group_id: BytesN<32>,
        commitment: BytesN<32>,
        tier: u32,
        proof: Groth16Proof,
        public_inputs: PublicInputs,
    ) -> Result<(), Error> {
        Self::create_group_v2(
            env,
            caller,
            group_id,
            commitment,
            tier,
            0, // group_type: Anarchy
            0, // member_count: untracked
            proof,
            public_inputs,
        )
    }

    /// Create a new private membership group with an explicit governance type.
    ///
    /// `group_type` selects the on-chain policy for who may modify the group:
    /// * `0` — Anarchy: any current member (proof-only authorisation)
    /// * `1` — 1v1: fixed two-member group (reserved for Phase 1)
    /// * `2` — Democracy: ≥50% quorum on kick/invite (reserved for Phase 3)
    /// * `3` — Oligarchy: admin set gates membership changes (reserved for Phase 2)
    ///
    /// In this contract build only `group_type = 0` is accepted; any other
    /// value is rejected with `UnknownGroupType`. Later phases enable the
    /// remaining types without altering this function's ABI.
    ///
    /// `member_count` is persisted as informational state for Anarchy (not
    /// enforced) and will be used as an authoritative quorum basis by the
    /// Democracy circuit when Phase 3 lands.
    ///
    /// **Audit-followup Findings #1-full and #2 — deferred to circuit work.**
    /// The MembershipCircuit as currently compiled binds only `commitment`
    /// and `epoch` as public inputs. It does NOT bind `group_id` or
    /// `group_type`. Consequences the contract cannot close alone:
    ///   * A proof created for one (commitment, epoch=0) pair verifies for
    ///     another group that happens to land on the same commitment at
    ///     epoch 0. `group_id` is checked as a public input here but is
    ///     NOT part of the SNARK statement; nullifier scoping by
    ///     `group_id` (Finding #1-partial, landed) only blocks same-group
    ///     replay.
    ///   * `group_type` is trusted from the caller. If two clients agree
    ///     off-chain on the governance policy but one submits
    ///     `group_type = 0` (Anarchy) on-chain, the on-chain record is
    ///     Anarchy and `update_commitment` bypasses governance. The ZK
    ///     proof cannot attest to the governance type.
    /// Full closure requires re-generating the MembershipCircuit to bind
    /// both fields as public inputs, then a coordinated circuit + VK
    /// rotation. Tracked as a follow-up PR.
    pub fn create_group_v2(
        env: Env,
        caller: Address,
        group_id: BytesN<32>,
        commitment: BytesN<32>,
        tier: u32,
        group_type: u32,
        member_count: u32,
        proof: Groth16Proof,
        public_inputs: PublicInputs,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        caller.require_auth();

        // N-26: In restricted mode, only the admin can create groups.
        let restricted: bool = env
            .storage()
            .instance()
            .get(&DataKey::RestrictedMode)
            .unwrap_or(false);
        if restricted {
            let admin: Address = env
                .storage()
                .instance()
                .get(&DataKey::Admin)
                .ok_or(Error::NotInitialized)?;
            if caller != admin {
                return Err(Error::AdminOnly);
            }
        }

        if tier > 2 {
            return Err(Error::InvalidTier);
        }
        match group_type {
            0 => {
                // Anarchy — any `member_count`, any tier.
            }
            1 => {
                // 1v1: fixed two-member group, Small tier only. The member
                // set is frozen at creation; `update_commitment` is rejected
                // for group_type = 1. The participant count is persisted as
                // an on-chain invariant so future clients can surface it
                // without trusting the caller.
                if tier != 0 {
                    return Err(Error::Invalid1v1Tier);
                }
                if member_count != 2 {
                    return Err(Error::PublicInputsMismatch);
                }
            }
            2 => {
                // Democracy: ≥50% quorum on kick/invite. The quorum check
                // is enforced inside the `DemocracyUpdateCircuit`
                // (ceremony-gated — not active in this build).
                //
                // `member_count` is advisory at creation — the contract
                // does not cross-check it against the Merkle commitment.
                // Clients routinely create groups solo and grow via
                // off-chain MLS joins, so we accept any `member_count` in
                // [0, tier_capacity]. Quorum semantics only bind once a
                // DemocracyUpdate proof lands, at which point member_count
                // is bound by the update circuit.
                if member_count > tier_capacity(tier) {
                    return Err(Error::MemberCountOutOfRange);
                }
            }
            _ => {
                // Oligarchy uses `create_oligarchy_group` which takes the
                // admin_root seed. Types > 3 are not defined.
                return Err(Error::UnknownGroupType);
            }
        }
        if public_inputs.commitment != commitment || public_inputs.epoch != 0 {
            return Err(Error::PublicInputsMismatch);
        }
        if Self::group_exists(&env, &group_id) {
            return Err(Error::GroupAlreadyExists);
        }

        // Audit-2026-04 MEDIUM-2: enforce canonical Fr encoding of the
        // commitment up front, matching the style used for `c_new`
        // (update_commitment) and oligarchy `admin_root`. The verifier
        // already performs this check internally, but surfacing it here
        // keeps the error model consistent and does not rely on verifier
        // internals for input validation.
        let commitment_fr = Fr::from_bytes(commitment.clone());
        let commitment_canonical: BytesN<32> = commitment_fr.to_bytes();
        if commitment_canonical != commitment {
            return Err(Error::InvalidCommitmentEncoding);
        }

        // M-4: Enforce per-tier group count limit to prevent storage abuse.
        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::GroupCount(tier))
            .unwrap_or(0);
        if count >= MAX_GROUPS_PER_TIER {
            return Err(Error::TierGroupLimitReached);
        }

        Self::check_proof_replay(&env, &proof)?;

        let vk = Self::load_vk(&env, tier)?;
        if !verify_groth16_proof(&env, &vk, &proof, &commitment, 0) {
            return Err(Error::InvalidProof);
        }

        Self::record_proof(&env, &proof);

        let timestamp = env.ledger().timestamp();
        let entry = CommitmentEntryV2 {
            commitment: commitment.clone(),
            epoch: 0,
            timestamp,
            tier,
            active: true,
            group_type,
            member_count,
        };

        env.storage()
            .persistent()
            .set(&DataKey::GroupV2(group_id.clone()), &entry);
        env.storage().persistent().set(
            &DataKey::HistoryV2(group_id.clone()),
            &Vec::<CommitmentEntry>::new(&env),
        );
        env.storage()
            .instance()
            .set(&DataKey::GroupCount(tier), &(count + 1));
        // Audit-followup Finding #6: receipt enabling permissionless
        // `reconcile_tier_count` after cold-group expiry. Stored in
        // persistent storage with its own longer TTL so the receipt
        // outlives the group entry (re-audit Finding #3 — instance
        // storage would cap active groups well below MAX_GROUPS_PER_TIER).
        let receipt_key = DataKey::GroupCounted(group_id.clone());
        env.storage().persistent().set(&receipt_key, &tier);
        env.storage().persistent().extend_ttl(
            &receipt_key,
            LEDGER_THRESHOLD,
            GROUP_COUNTED_LEDGER_BUMP,
        );
        Self::bump_group(&env, &group_id);

        GroupCreated {
            group_id,
            commitment,
            epoch: 0,
            tier,
            timestamp,
        }
        .publish(&env);

        Ok(())
    }

    /// Create a new Oligarchy group (`group_type = 3`).
    ///
    /// Seeds the admin set (Poseidon root of a separate admin tree) at
    /// creation time alongside the standard membership commitment. The
    /// creator proves membership via the usual small/medium/large
    /// membership VK; the admin-set contents are not verified by the
    /// contract — they are the creator's attestation, pinned by the
    /// `AdminSet` storage slot so later admin-only operations (ceremony-
    /// gated in this build) can check them.
    ///
    /// Any current admin can promote or demote other admins via
    /// `update_admin_set` (reserved for a follow-up PR once the
    /// `AdminUpdateCircuit` VK lands). The creator's only unique role is
    /// seeding the initial admin set: after creation the creator holds
    /// no special privilege beyond that of any other admin.
    ///
    /// **Audit-followup Finding #2 — deferred to circuit work.** The
    /// MembershipCircuit does not bind `group_type` as a public input, so
    /// the contract cannot cryptographically verify that the caller is
    /// creating an Oligarchy vs. an Anarchy vs. a Democracy. Governance-
    /// type integrity today rests on off-chain client discipline. See the
    /// `create_group_v2` doc comment for the full threat model and the
    /// planned circuit-rotation follow-up.
    pub fn create_oligarchy_group(
        env: Env,
        caller: Address,
        group_id: BytesN<32>,
        commitment: BytesN<32>,
        tier: u32,
        member_count: u32,
        admin_root: BytesN<32>,
        proof: Groth16Proof,
        public_inputs: PublicInputs,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        caller.require_auth();

        let restricted: bool = env
            .storage()
            .instance()
            .get(&DataKey::RestrictedMode)
            .unwrap_or(false);
        if restricted {
            let admin: Address = env
                .storage()
                .instance()
                .get(&DataKey::Admin)
                .ok_or(Error::NotInitialized)?;
            if caller != admin {
                return Err(Error::AdminOnly);
            }
        }

        if tier > 2 {
            return Err(Error::InvalidTier);
        }
        // `member_count` is advisory at creation (see create_group_v2
        // Democracy arm). Clients typically start Oligarchy groups solo
        // and promote later admins via ceremony-gated rotation, so
        // member_count < 2 is permitted here.
        if member_count > tier_capacity(tier) {
            return Err(Error::MemberCountOutOfRange);
        }
        if public_inputs.commitment != commitment || public_inputs.epoch != 0 {
            return Err(Error::PublicInputsMismatch);
        }
        if Self::group_exists(&env, &group_id) {
            return Err(Error::GroupAlreadyExists);
        }

        // `admin_root` is a Poseidon-hashed commitment to the admin tree.
        // We validate canonical Fr encoding so a malformed seed cannot be
        // accepted into storage.
        let admin_root_fr = Fr::from_bytes(admin_root.clone());
        let admin_root_canonical: BytesN<32> = admin_root_fr.to_bytes();
        if admin_root_canonical != admin_root {
            return Err(Error::InvalidCommitmentEncoding);
        }

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::GroupCount(tier))
            .unwrap_or(0);
        if count >= MAX_GROUPS_PER_TIER {
            return Err(Error::TierGroupLimitReached);
        }

        Self::check_proof_replay(&env, &proof)?;

        let vk = Self::load_vk(&env, tier)?;
        if !verify_groth16_proof(&env, &vk, &proof, &commitment, 0) {
            return Err(Error::InvalidProof);
        }

        Self::record_proof(&env, &proof);

        let timestamp = env.ledger().timestamp();
        let entry = CommitmentEntryV2 {
            commitment: commitment.clone(),
            epoch: 0,
            timestamp,
            tier,
            active: true,
            group_type: 3,
            member_count,
        };

        env.storage()
            .persistent()
            .set(&DataKey::GroupV2(group_id.clone()), &entry);
        env.storage().persistent().set(
            &DataKey::HistoryV2(group_id.clone()),
            &Vec::<CommitmentEntry>::new(&env),
        );
        env.storage()
            .persistent()
            .set(&DataKey::AdminSet(group_id.clone()), &admin_root);
        env.storage().persistent().extend_ttl(
            &DataKey::AdminSet(group_id.clone()),
            LEDGER_THRESHOLD,
            LEDGER_BUMP,
        );
        // Seed admin epoch at 0. Bumped by `update_admin_commitment`.
        env.storage()
            .persistent()
            .set(&DataKey::AdminEpoch(group_id.clone()), &0u64);
        env.storage().persistent().extend_ttl(
            &DataKey::AdminEpoch(group_id.clone()),
            LEDGER_THRESHOLD,
            LEDGER_BUMP,
        );
        env.storage()
            .instance()
            .set(&DataKey::GroupCount(tier), &(count + 1));
        // Audit-followup Finding #6: reconciliation receipt for oligarchy.
        // Persistent storage with longer TTL than the group itself, so
        // `reconcile_tier_count` still works after cold-group expiry
        // (re-audit Finding #3).
        let receipt_key = DataKey::GroupCounted(group_id.clone());
        env.storage().persistent().set(&receipt_key, &tier);
        env.storage().persistent().extend_ttl(
            &receipt_key,
            LEDGER_THRESHOLD,
            GROUP_COUNTED_LEDGER_BUMP,
        );
        Self::bump_group(&env, &group_id);

        GroupCreated {
            group_id,
            commitment,
            epoch: 0,
            tier,
            timestamp,
        }
        .publish(&env);

        Ok(())
    }

    /// Read the Poseidon admin-tree root for an Oligarchy group.
    ///
    /// Returns `MissingAdminRoot` if the group is not Oligarchy-typed
    /// (no `AdminSet` slot was ever written for it).
    pub fn get_admin_root(env: Env, group_id: BytesN<32>) -> Result<BytesN<32>, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::AdminSet(group_id))
            .ok_or(Error::MissingAdminRoot)
    }

    /// Update a group's commitment (epoch transition).
    ///
    /// **#59 fix — binding the new commitment.** This function no longer
    /// accepts a free-form `new_commitment` / `new_epoch` pair. The new
    /// commitment is the cryptographically-bound public input `c_new`
    /// of an `UpdateCircuit` Groth16 proof, which the contract verifies
    /// against the tier's update VK. The new epoch is derived on-chain
    /// as `stored_epoch + 1` and is never a client-supplied value.
    ///
    /// Callers supply `public_inputs = (c_old, epoch_old, c_new)`; the
    /// contract rejects the call unless `c_old` and `epoch_old` match the
    /// current stored state, and `c_new` is a canonical Fr encoding.
    ///
    /// N-14: Uses proof-based authorisation only (no `caller.require_auth()`).
    /// The fix above ensures N-14 no longer amplifies any binding gap — the
    /// only surface left is the proof itself.
    pub fn update_commitment(
        env: Env,
        group_id: BytesN<32>,
        proof: Groth16Proof,
        public_inputs: UpdatePublicInputs,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;

        let current = Self::load_group_v2(&env, &group_id)?;

        if !current.active {
            return Err(Error::GroupInactive);
        }
        // Only Anarchy is routable through this function. Future phases
        // will dispatch on `group_type` to the appropriate update circuit
        // (Democracy/Oligarchy use their own update VKs).
        match current.group_type {
            0 => {}
            1 => return Err(Error::OneOnOneImmutable),
            _ => return Err(Error::UnknownGroupType),
        }

        // N-22: Use checked_add to guard against u64 overflow (theoretical).
        let new_epoch = current.epoch.checked_add(1).ok_or(Error::InvalidEpoch)?;

        if public_inputs.c_old != current.commitment
            || public_inputs.epoch_old != current.epoch
        {
            return Err(Error::PublicInputsMismatch);
        }

        // #59: Reject non-canonical c_new encodings. Fr::from_bytes silently
        // reduces mod r, which would let a caller craft two byte-distinct
        // but field-equivalent c_new values with the same proof. The
        // roundtrip check forces exactly one canonical representation.
        let c_new_fr = Fr::from_bytes(public_inputs.c_new.clone());
        let c_new_canonical: BytesN<32> = c_new_fr.to_bytes();
        if c_new_canonical != public_inputs.c_new {
            return Err(Error::InvalidCommitmentEncoding);
        }

        Self::check_proof_replay(&env, &proof)?;

        let vk = Self::load_update_vk(&env, current.tier)?;
        if !verify_groth16_proof_update(
            &env,
            &vk,
            &proof,
            &current.commitment,
            current.epoch,
            &public_inputs.c_new,
        ) {
            return Err(Error::InvalidProof);
        }

        Self::record_proof(&env, &proof);

        let timestamp = env.ledger().timestamp();

        Self::archive_entry(&env, &group_id, &current);

        let new_entry = CommitmentEntryV2 {
            commitment: public_inputs.c_new.clone(),
            epoch: new_epoch,
            timestamp,
            tier: current.tier,
            active: true,
            group_type: current.group_type,
            member_count: current.member_count,
        };
        Self::write_group_v2(&env, &group_id, &new_entry);
        Self::bump_group(&env, &group_id);

        CommitmentUpdated {
            group_id,
            commitment: public_inputs.c_new,
            epoch: new_epoch,
            timestamp,
        }
        .publish(&env);

        Ok(())
    }

    /// Update the commitment of a Democracy (`group_type = 2`) group.
    ///
    /// Separate entrypoint from `update_commitment` because the Democracy
    /// circuit proves a richer relation (§6.4.2): the prover must open the
    /// current commitment to a quorum of ≥ ⌈n/2⌉ signers and witness a
    /// single-leaf delta between the old and new member trees. The public
    /// input vector therefore has five elements, not three, and feeds a
    /// separately-ceremony-gated VK stored at
    /// `DataKey::UpdateVKByType(tier, 2)`.
    ///
    /// Contract-side checks (pre-verification):
    ///   * group exists, is active, and has `group_type == 2`
    ///   * `public_inputs.c_old == stored.commitment`
    ///   * `public_inputs.epoch_old == stored.epoch`
    ///   * `public_inputs.member_count_old == stored.member_count` —
    ///     authoritative quorum denominator binding
    ///   * `|public_inputs.member_count_new − member_count_old| ≤ 1`
    ///   * `public_inputs.member_count_new` is within tier capacity and ≥ 2
    ///     (democracy-of-one is forbidden at create time and preserved here)
    ///   * `c_new` is canonical
    ///
    /// On success, `member_count_new` is persisted into the V2 entry. The
    /// rest follows the same proof-replay/archive/event emission pattern as
    /// `update_commitment`.
    ///
    /// **Audit-followup Finding #3 — deferred to circuit work.** The
    /// on-chain `member_count` is the authoritative quorum denominator
    /// *once it is trusted*, but the value stored at create time is the
    /// creator's unchecked declaration: the MembershipCircuit does not
    /// bind `member_count` to the Merkle tree's actual size, and the
    /// DemocracyUpdateCircuit binds `member_count_old` as a public input
    /// without proving it equals the tree's size either. Consequence: a
    /// lying creator can declare `member_count = 1` on a 1000-member
    /// group and the first democracy update goes through with quorum = 1.
    /// Subsequent updates are self-consistent but rooted in the lie. Full
    /// closure requires the MembershipCircuit to prove `member_count ==
    /// tree_size` at creation (and the DemocracyUpdateCircuit to prove
    /// it continues to hold). Tracked alongside Findings #1-full and #2.
    pub fn update_commitment_democracy(
        env: Env,
        group_id: BytesN<32>,
        proof: Groth16Proof,
        public_inputs: DemocracyUpdatePublicInputs,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;

        let current = Self::load_group_v2(&env, &group_id)?;

        if !current.active {
            return Err(Error::GroupInactive);
        }
        if current.group_type != 2 {
            return Err(Error::UnknownGroupType);
        }

        let new_epoch = current.epoch.checked_add(1).ok_or(Error::InvalidEpoch)?;

        if public_inputs.c_old != current.commitment
            || public_inputs.epoch_old != current.epoch
        {
            return Err(Error::PublicInputsMismatch);
        }

        // Source-of-truth binding (§6.4.2): the on-chain `member_count` is
        // the authoritative quorum denominator; the circuit uses it as a
        // public input, so the caller cannot vary it independently.
        if public_inputs.member_count_old != current.member_count {
            return Err(Error::MemberCountMismatch);
        }

        // `member_count_new ∈ {m-1, m, m+1}`. Uses saturating arithmetic so
        // the check is sound even at u32 boundary values (V-C4).
        let m_old = current.member_count;
        let m_new = public_inputs.member_count_new;
        let valid_delta = m_new == m_old
            || (m_new == m_old.saturating_add(1))
            || (m_old > 0 && m_new.saturating_add(1) == m_old);
        if !valid_delta {
            return Err(Error::MemberCountOutOfRange);
        }

        // Post-transition count must stay within the tier's capacity and
        // keep the democracy-of-≥2 invariant.
        if m_new < 2 {
            return Err(Error::MemberCountOutOfRange);
        }
        if m_new > tier_capacity(current.tier) {
            return Err(Error::MemberCountOutOfRange);
        }

        let c_new_fr = Fr::from_bytes(public_inputs.c_new.clone());
        let c_new_canonical: BytesN<32> = c_new_fr.to_bytes();
        if c_new_canonical != public_inputs.c_new {
            return Err(Error::InvalidCommitmentEncoding);
        }

        Self::check_proof_replay(&env, &proof)?;

        let vk = Self::load_update_vk_by_type(&env, current.tier, 2)?;
        if !verify_groth16_proof_democracy_update(
            &env,
            &vk,
            &proof,
            &current.commitment,
            current.epoch,
            &public_inputs.c_new,
            m_old,
            m_new,
        ) {
            return Err(Error::InvalidProof);
        }

        Self::record_proof(&env, &proof);

        let timestamp = env.ledger().timestamp();
        Self::archive_entry(&env, &group_id, &current);

        let new_entry = CommitmentEntryV2 {
            commitment: public_inputs.c_new.clone(),
            epoch: new_epoch,
            timestamp,
            tier: current.tier,
            active: true,
            group_type: current.group_type,
            member_count: m_new,
        };
        Self::write_group_v2(&env, &group_id, &new_entry);
        Self::bump_group(&env, &group_id);

        CommitmentUpdated {
            group_id,
            commitment: public_inputs.c_new,
            epoch: new_epoch,
            timestamp,
        }
        .publish(&env);

        Ok(())
    }

    /// Rotate the admin set of an Oligarchy (`group_type = 3`) group.
    ///
    /// Proof is `AdminUpdateCircuit` — shape-identical to the member
    /// `UpdateCircuit` but rebound to the admin tree (§6.4.4). The three
    /// public inputs are the salted admin commitments plus the admin epoch:
    /// `(admin_c_old, admin_epoch_old, admin_c_new)`.
    ///
    /// Contract-side checks (pre-verification):
    ///   * group exists, is active, and has `group_type == 3`
    ///   * `public_inputs.admin_c_old == stored AdminSet(group_id)` — the
    ///     source-of-truth binding; the caller cannot attest to a stale root
    ///   * `public_inputs.admin_epoch_old == stored AdminEpoch(group_id)`
    ///   * `admin_c_new` is a canonical Fr encoding
    ///   * proof is not a replay
    ///
    /// On success, the new commitment is written to `AdminSet` and the
    /// admin epoch is bumped. The *member* commitment is untouched — admin
    /// rotation and member rotation are independent state transitions.
    ///
    /// Design note on the admin-∈-member invariant: this circuit does NOT
    /// cross-check that the rotating admin is also a member of the group.
    /// Per §6.4.4 and §8 T10, that invariant is enforced client-side (UIs
    /// only surface "Promote" for current members) and not in-circuit —
    /// tightening it would ~2× constraint count. A non-member admin can
    /// rotate the admin set further but cannot produce an
    /// `OligarchyUpdateCircuit` proof, so the residual risk is confined to
    /// admin-set churn.
    pub fn update_admin_commitment(
        env: Env,
        group_id: BytesN<32>,
        proof: Groth16Proof,
        public_inputs: AdminUpdatePublicInputs,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;

        let current = Self::load_group_v2(&env, &group_id)?;

        if !current.active {
            return Err(Error::GroupInactive);
        }
        if current.group_type != 3 {
            return Err(Error::UnknownGroupType);
        }

        let stored_admin_commitment: BytesN<32> = env
            .storage()
            .persistent()
            .get(&DataKey::AdminSet(group_id.clone()))
            .ok_or(Error::MissingAdminRoot)?;
        if public_inputs.admin_c_old != stored_admin_commitment {
            return Err(Error::AdminRootMismatch);
        }

        // Missing AdminEpoch -> 0 (migration fallback for groups created
        // before this field existed; the first admin rotation consumes
        // epoch 0, after which the slot is always populated).
        let stored_admin_epoch: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::AdminEpoch(group_id.clone()))
            .unwrap_or(0u64);
        if public_inputs.admin_epoch_old != stored_admin_epoch {
            return Err(Error::PublicInputsMismatch);
        }

        let new_admin_epoch = stored_admin_epoch
            .checked_add(1)
            .ok_or(Error::InvalidEpoch)?;

        // Canonical encoding check on the new admin commitment.
        let c_new_fr = Fr::from_bytes(public_inputs.admin_c_new.clone());
        let c_new_canonical: BytesN<32> = c_new_fr.to_bytes();
        if c_new_canonical != public_inputs.admin_c_new {
            return Err(Error::InvalidCommitmentEncoding);
        }

        Self::check_proof_replay(&env, &proof)?;

        let vk = Self::load_admin_update_vk(&env)?;
        // Reuse the UpdateCircuit verifier — same 3 public inputs, 4 IC
        // points. The circuit is rebound to the admin tree but the
        // verification equation is identical.
        if !verify_groth16_proof_update(
            &env,
            &vk,
            &proof,
            &public_inputs.admin_c_old,
            public_inputs.admin_epoch_old,
            &public_inputs.admin_c_new,
        ) {
            return Err(Error::InvalidProof);
        }

        Self::record_proof(&env, &proof);

        env.storage()
            .persistent()
            .set(&DataKey::AdminSet(group_id.clone()), &public_inputs.admin_c_new);
        env.storage().persistent().extend_ttl(
            &DataKey::AdminSet(group_id.clone()),
            LEDGER_THRESHOLD,
            LEDGER_BUMP,
        );
        env.storage()
            .persistent()
            .set(&DataKey::AdminEpoch(group_id.clone()), &new_admin_epoch);
        env.storage().persistent().extend_ttl(
            &DataKey::AdminEpoch(group_id.clone()),
            LEDGER_THRESHOLD,
            LEDGER_BUMP,
        );

        let timestamp = env.ledger().timestamp();
        AdminCommitmentUpdated {
            group_id,
            admin_commitment: public_inputs.admin_c_new,
            admin_epoch: new_admin_epoch,
            timestamp,
        }
        .publish(&env);

        Ok(())
    }

    /// Read the current admin epoch for an Oligarchy group. Returns 0 if
    /// the epoch slot was never written (pre-migration groups).
    pub fn get_admin_epoch(env: Env, group_id: BytesN<32>) -> Result<u64, Error> {
        Self::require_initialized(&env)?;
        let current = Self::load_group_v2(&env, &group_id)?;
        if current.group_type != 3 {
            return Err(Error::MissingAdminRoot);
        }
        Ok(env
            .storage()
            .persistent()
            .get(&DataKey::AdminEpoch(group_id))
            .unwrap_or(0u64))
    }

    /// Verify a membership proof against the current group state
    /// (read-only, non-consuming).
    ///
    /// **Semantic: this is read-only.** The proof is checked against
    /// the current `(commitment, epoch)` stored for the group, but
    /// nothing is written — no nullifier is burnt, no TTL is bumped.
    /// Callers can safely invoke this with `--send no` / simulation-
    /// only RPC calls; the relayer and smoke scripts rely on that.
    ///
    /// **What this does NOT do.** Calling `verify_membership`
    /// successfully offers NO protection against the same proof bytes
    /// later being replayed into a state-changing entrypoint
    /// (`deactivate_group`, `update_commitment`, etc.) — by this
    /// caller or by any observer of the simulation / mempool. If a
    /// caller needs that guarantee they MUST call
    /// `consume_membership_proof` instead, which records the
    /// nullifier. See the module-level "Replay-nullifier scoping"
    /// comment for the full story on what contract-level nullifiers
    /// can and cannot close.
    ///
    /// **Trust model.** The bool returned is a statement about the
    /// proof and the group state at the observed ledger. It is not a
    /// replay-protective credential. UIs that show "you are a member"
    /// should use this; security-sensitive attestations should use
    /// `consume_membership_proof`.
    pub fn verify_membership(
        env: Env,
        group_id: BytesN<32>,
        proof: Groth16Proof,
        public_inputs: PublicInputs,
    ) -> Result<bool, Error> {
        Self::require_initialized(&env)?;

        let state = Self::load_group_v2(&env, &group_id)?;

        if public_inputs.commitment != state.commitment
            || public_inputs.epoch != state.epoch
        {
            return Err(Error::PublicInputsMismatch);
        }

        let vk = Self::load_vk(&env, state.tier)?;
        let valid = verify_groth16_proof(
            &env,
            &vk,
            &proof,
            &state.commitment,
            state.epoch,
        );

        Ok(valid)
    }

    /// Verify a membership proof and burn its nullifier.
    ///
    /// **Semantic: this IS state-changing.** On success the proof is
    /// recorded in the `UsedProof` set, so the same proof bytes
    /// cannot be re-submitted to any state-changing entrypoint on any
    /// group. Use this when the caller needs the on-chain guarantee
    /// "this proof is mine, nobody else can reuse it" — e.g. a signed
    /// attestation that a given address was a member at a given
    /// ledger without exposing the proof to frontrun-style reuse
    /// into `deactivate_group`.
    ///
    /// Callers MUST submit this as a real transaction (not
    /// `--send no` / simulation); simulated calls do not persist the
    /// nullifier and therefore confer no replay protection.
    ///
    /// **What this still does NOT close.**
    ///   * Pre-inclusion mempool frontrun: an observer of the pending
    ///     transaction can race the honest caller with the same proof
    ///     bytes. Circuit-level fix required (bind an operation tag
    ///     and `caller` as public inputs).
    ///   * TTL-aged replay: once the `UsedProof` entry for a proof
    ///     expires (~`LEDGER_BUMP` ledgers), the bytes become
    ///     replayable again. Bounded only by ledger-level
    ///     economics; circuit-level fix required for unconditional
    ///     expiry protection.
    ///
    /// See the module-level "Replay-nullifier scoping" comment and
    /// the deactivate_group doc for the full residual-exposure model.
    pub fn consume_membership_proof(
        env: Env,
        group_id: BytesN<32>,
        proof: Groth16Proof,
        public_inputs: PublicInputs,
    ) -> Result<bool, Error> {
        Self::require_initialized(&env)?;

        let state = Self::load_group_v2(&env, &group_id)?;

        if public_inputs.commitment != state.commitment
            || public_inputs.epoch != state.epoch
        {
            return Err(Error::PublicInputsMismatch);
        }

        Self::check_proof_replay(&env, &proof)?;

        let vk = Self::load_vk(&env, state.tier)?;
        let valid = verify_groth16_proof(
            &env,
            &vk,
            &proof,
            &state.commitment,
            state.epoch,
        );

        if valid {
            Self::record_proof(&env, &proof);
        }

        Ok(valid)
    }

    /// Deactivate a group (requires membership proof).
    ///
    /// After deactivation `verify_membership` and `get_state` still work,
    /// but `update_commitment` is rejected. This is irreversible.
    ///
    /// **Design decision (V-C1 safety valve):** deactivation is intentionally
    /// governance-type-agnostic.  Any single member who can produce a valid
    /// membership proof may deactivate *any* group type — including Democracy
    /// (which otherwise requires ≥50 % quorum) and Oligarchy (which otherwise
    /// requires admin authorization).  This asymmetry is deliberate: it
    /// ensures no group can become un-deactivatable even when the governance
    /// quorum is unreachable (e.g. enough members have left).
    ///
    /// **Audit-2026-04 LOW-2 (acknowledged, no change).** Reviewers MUST
    /// assume a single dissatisfied member can irreversibly retire any
    /// group they belong to. UIs and integrators cannot rely on Democracy
    /// quorum or Oligarchy admin authorisation to gate deactivation. If a
    /// future product requirement needs governance-gated deactivation, add
    /// a per-group policy flag at creation time rather than removing this
    /// safety valve globally (it would strand any group whose governance
    /// quorum becomes unreachable).
    ///
    /// N-14: Uses proof-based authorization only (same rationale as `update_commitment`).
    ///
    /// **Re-audit Finding #4 — known residual exposure (circuit-level).**
    /// Two distinct replay paths remain open that contract-level code
    /// cannot close:
    ///
    ///   1. **Pre-inclusion mempool frontrun.** The deactivate proof
    ///      is visible in the pending transaction before it is mined.
    ///      An observer can submit `deactivate_group` with the same
    ///      proof and may win ordering. Nullifier burning here is
    ///      post-inclusion only. (Note: `epoch` is ALREADY bound as a
    ///      MembershipCircuit public input, so "bind epoch" is not
    ///      the missing piece — what is missing is an OPERATION TAG
    ///      and `caller` binding, so a proof produced for VERIFY
    ///      cannot be reused for DEACTIVATE and a proof produced by
    ///      one address cannot be stolen by another.)
    ///   2. **TTL-aged proof replay.** `UsedProof` entries expire
    ///      after `LEDGER_BUMP` (~30 days). A group that stays at
    ///      `(commitment, epoch)` for longer than that can be
    ///      deactivated by replaying ANY earlier membership proof
    ///      whose nullifier has since expired — including the
    ///      create-time proof, if the group has never been updated.
    ///      Contract-level mitigations (per-group nullifier lists,
    ///      keeping the nullifier set forever) all have unbounded-
    ///      growth or rent problems; the v2-circuit fix is a per-
    ///      group monotonic `proof_nonce` bound as a public input so
    ///      stale proofs stop verifying regardless of nullifier TTL.
    ///
    /// Operator mitigation today: periodically call
    /// `update_commitment` even for no-op membership changes so the
    /// epoch advances and stale proofs stop verifying via public-
    /// input mismatch before their nullifier expires.
    pub fn deactivate_group(
        env: Env,
        group_id: BytesN<32>,
        proof: Groth16Proof,
        public_inputs: PublicInputs,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;

        let current = Self::load_group_v2(&env, &group_id)?;

        if !current.active {
            return Err(Error::GroupInactive);
        }
        if public_inputs.commitment != current.commitment
            || public_inputs.epoch != current.epoch
        {
            return Err(Error::PublicInputsMismatch);
        }

        Self::check_proof_replay(&env, &proof)?;

        let vk = Self::load_vk(&env, current.tier)?;
        if !verify_groth16_proof(&env, &vk, &proof, &current.commitment, current.epoch) {
            return Err(Error::InvalidProof);
        }

        Self::record_proof(&env, &proof);

        let timestamp = env.ledger().timestamp();
        let deactivated = CommitmentEntryV2 {
            active: false,
            timestamp,
            ..current.clone()
        };
        Self::write_group_v2(&env, &group_id, &deactivated);
        Self::bump_group(&env, &group_id);

        // N-23: Decrement per-tier group count so deactivated groups
        // don't permanently consume the M-4 tier limit.
        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::GroupCount(current.tier))
            .unwrap_or(0);
        if count > 0 {
            env.storage()
                .instance()
                .set(&DataKey::GroupCount(current.tier), &(count - 1));
        }
        // Audit-followup Finding #6: clear the reconciliation receipt so
        // `reconcile_tier_count` cannot double-decrement. Persistent
        // storage per re-audit Finding #3.
        if env
            .storage()
            .persistent()
            .has(&DataKey::GroupCounted(group_id.clone()))
        {
            env.storage()
                .persistent()
                .remove(&DataKey::GroupCounted(group_id.clone()));
        }

        GroupDeactivated {
            group_id,
            final_epoch: current.epoch,
            timestamp,
        }
        .publish(&env);

        Ok(())
    }

    // ---- Queries ----

    /// Get the current state of a group in legacy (V1) shape.
    ///
    /// Returns a `CommitmentEntry` projection of the V2 record. Clients that
    /// need the governance type or member count MUST call `get_state_v2`.
    pub fn get_state(env: Env, group_id: BytesN<32>) -> Result<CommitmentEntry, Error> {
        let v2 = Self::load_group_v2(&env, &group_id)?;
        Ok(CommitmentEntry {
            commitment: v2.commitment,
            epoch: v2.epoch,
            timestamp: v2.timestamp,
            tier: v2.tier,
            active: v2.active,
        })
    }

    /// Get the current V2 state of a group, including governance metadata.
    pub fn get_state_v2(
        env: Env,
        group_id: BytesN<32>,
    ) -> Result<CommitmentEntryV2, Error> {
        Self::load_group_v2(&env, &group_id)
    }

    /// Get the history of a group (most recent entries, up to `max_entries`
    /// capped by the contract's history window of 64).
    ///
    /// Full history is always available via contract events.
    pub fn get_history(
        env: Env,
        group_id: BytesN<32>,
        max_entries: u32,
    ) -> Result<Vec<CommitmentEntry>, Error> {
        if !Self::group_exists(&env, &group_id) {
            return Err(Error::GroupNotFound);
        }
        let history: Vec<CommitmentEntry> = env
            .storage()
            .persistent()
            .get(&DataKey::HistoryV2(group_id.clone()))
            .or_else(|| env.storage().persistent().get(&DataKey::History(group_id)))
            .unwrap_or(Vec::new(&env));

        let cap = if max_entries < history.len() {
            max_entries
        } else {
            history.len()
        };
        if cap == history.len() {
            return Ok(history);
        }
        let start = history.len() - cap;
        let mut result = Vec::new(&env);
        for i in start..history.len() {
            result.push_back(history.get(i).unwrap());
        }
        Ok(result)
    }

    // ---- Internal helpers ----

    fn require_initialized(env: &Env) -> Result<(), Error> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::NotInitialized);
        }
        Ok(())
    }

    /// Load the V2 record for a group, synthesising one from the legacy
    /// `Group` key if only the pre-migration format is present.
    ///
    /// The legacy entry is treated as Anarchy (`group_type = 0`) with
    /// `member_count = 0`. This lets old groups continue to operate without
    /// a forced migration step; the first state-changing write produced
    /// by any governance-aware code path will persist a `GroupV2` record.
    fn load_group_v2(
        env: &Env,
        group_id: &BytesN<32>,
    ) -> Result<CommitmentEntryV2, Error> {
        if let Some(v2) = env
            .storage()
            .persistent()
            .get::<DataKey, CommitmentEntryV2>(&DataKey::GroupV2(group_id.clone()))
        {
            return Ok(v2);
        }
        let legacy: CommitmentEntry = env
            .storage()
            .persistent()
            .get(&DataKey::Group(group_id.clone()))
            .ok_or(Error::GroupNotFound)?;
        Ok(CommitmentEntryV2 {
            commitment: legacy.commitment,
            epoch: legacy.epoch,
            timestamp: legacy.timestamp,
            tier: legacy.tier,
            active: legacy.active,
            group_type: 0,
            member_count: 0,
        })
    }

    /// Whether a group exists (under either the V2 or legacy key).
    fn group_exists(env: &Env, group_id: &BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::GroupV2(group_id.clone()))
            || env
                .storage()
                .persistent()
                .has(&DataKey::Group(group_id.clone()))
    }

    /// Persist a V2 group record and lazily drop the legacy entry if present.
    /// History is migrated on first write: the legacy `History` list is
    /// copied into `HistoryV2` (same `CommitmentEntry` shape) and the legacy
    /// key is deleted.
    fn write_group_v2(
        env: &Env,
        group_id: &BytesN<32>,
        entry: &CommitmentEntryV2,
    ) {
        env.storage()
            .persistent()
            .set(&DataKey::GroupV2(group_id.clone()), entry);

        if env
            .storage()
            .persistent()
            .has(&DataKey::Group(group_id.clone()))
        {
            env.storage()
                .persistent()
                .remove(&DataKey::Group(group_id.clone()));
        }

        if !env
            .storage()
            .persistent()
            .has(&DataKey::HistoryV2(group_id.clone()))
        {
            let legacy_history: Vec<CommitmentEntry> = env
                .storage()
                .persistent()
                .get(&DataKey::History(group_id.clone()))
                .unwrap_or(Vec::new(env));
            env.storage()
                .persistent()
                .set(&DataKey::HistoryV2(group_id.clone()), &legacy_history);
            if env
                .storage()
                .persistent()
                .has(&DataKey::History(group_id.clone()))
            {
                env.storage()
                    .persistent()
                    .remove(&DataKey::History(group_id.clone()));
            }
        }
    }

    fn load_vk(env: &Env, tier: u32) -> Result<VerificationKeyData, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::VK(tier))
            .ok_or(Error::NotInitialized)
    }

    fn load_update_vk(env: &Env, tier: u32) -> Result<VerificationKeyData, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::UpdateVK(tier))
            .ok_or(Error::NotInitialized)
    }

    /// Load the per-group-type update VK (e.g. Democracy for
    /// `group_type = 2`). Returns `NotInitialized` when the admin has not yet
    /// installed the ceremony-gated VK for this (tier, type) pair — the
    /// Democracy dispatcher treats that as a feature-not-yet-enabled signal.
    fn load_update_vk_by_type(
        env: &Env,
        tier: u32,
        group_type: u32,
    ) -> Result<VerificationKeyData, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::UpdateVKByType(tier, group_type))
            .ok_or(Error::NotInitialized)
    }

    /// Load the AdminUpdate VK (single key, no tier param per §6.4.4).
    /// Returns `NotInitialized` until the admin installs it via
    /// `update_vk(VkKind::AdminUpdate, 0, …)`.
    fn load_admin_update_vk(env: &Env) -> Result<VerificationKeyData, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::AdminUpdateVK)
            .ok_or(Error::NotInitialized)
    }

    fn bump_group(env: &Env, group_id: &BytesN<32>) {
        // Bump whichever key variant is present. After the first V2 write
        // the legacy keys are removed, so post-migration calls no-op on
        // the legacy branches.
        //
        // Post-audit Finding #4: also bump the per-group oligarchy
        // metadata keys (`AdminSet`, `AdminEpoch`). These are persistent
        // per-group entries and are required for `update_admin_commitment`
        // to function; leaving them unbumped while `GroupV2` is renewed
        // would let oligarchy admin rotation silently break long-lived
        // groups. Shared VK entries are bumped via a separate public
        // `bump_vks` entrypoint so anyone concerned with contract liveness
        // (not just callers touching one group) can refresh them.
        if env
            .storage()
            .persistent()
            .has(&DataKey::GroupV2(group_id.clone()))
        {
            env.storage().persistent().extend_ttl(
                &DataKey::GroupV2(group_id.clone()),
                LEDGER_THRESHOLD,
                LEDGER_BUMP,
            );
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::HistoryV2(group_id.clone()))
        {
            env.storage().persistent().extend_ttl(
                &DataKey::HistoryV2(group_id.clone()),
                LEDGER_THRESHOLD,
                LEDGER_BUMP,
            );
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::Group(group_id.clone()))
        {
            env.storage().persistent().extend_ttl(
                &DataKey::Group(group_id.clone()),
                LEDGER_THRESHOLD,
                LEDGER_BUMP,
            );
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::History(group_id.clone()))
        {
            env.storage().persistent().extend_ttl(
                &DataKey::History(group_id.clone()),
                LEDGER_THRESHOLD,
                LEDGER_BUMP,
            );
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::AdminSet(group_id.clone()))
        {
            env.storage().persistent().extend_ttl(
                &DataKey::AdminSet(group_id.clone()),
                LEDGER_THRESHOLD,
                LEDGER_BUMP,
            );
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::AdminEpoch(group_id.clone()))
        {
            env.storage().persistent().extend_ttl(
                &DataKey::AdminEpoch(group_id.clone()),
                LEDGER_THRESHOLD,
                LEDGER_BUMP,
            );
        }
        // Re-audit Finding #3: the `GroupCounted` reconciliation
        // receipt lives in persistent storage with a longer TTL than
        // the group itself, so extend it separately. The invariant
        // `receipt_expiry > group_expiry` is what lets
        // `reconcile_tier_count` observe the dangling tier-count slot
        // after a cold group's `GroupV2` has expired.
        if env
            .storage()
            .persistent()
            .has(&DataKey::GroupCounted(group_id.clone()))
        {
            env.storage().persistent().extend_ttl(
                &DataKey::GroupCounted(group_id.clone()),
                LEDGER_THRESHOLD,
                GROUP_COUNTED_LEDGER_BUMP,
            );
        }
        // Bump instance storage TTL (admin key, GroupCount tier
        // counters). Does not cover per-group receipts anymore.
        env.storage()
            .instance()
            .extend_ttl(LEDGER_THRESHOLD, LEDGER_BUMP);
    }

    /// Compute the global replay nullifier for a proof.
    ///
    /// Preimage: `proof.a || proof.b || proof.c`. See the module-level
    /// "Replay-nullifier scoping" comment for why this is deliberately
    /// unscoped: the MembershipCircuit does not bind `group_id` or an
    /// operation tag, so a proof valid for one `(commitment, epoch)`
    /// pair is accepted by ANY entrypoint on ANY group that happens to
    /// share those public inputs. Scoping by `group_id` would re-open
    /// cross-group replay against deliberately-cloned groups.
    fn proof_hash(env: &Env, proof: &Groth16Proof) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.append(&Bytes::from_slice(env, proof.a.to_array().as_slice()));
        preimage.append(&Bytes::from_slice(env, proof.b.to_array().as_slice()));
        preimage.append(&Bytes::from_slice(env, proof.c.to_array().as_slice()));
        env.crypto().sha256(&preimage).into()
    }

    /// Reject if these proof bytes have already been submitted anywhere.
    /// See [`proof_hash`] for the design rationale.
    fn check_proof_replay(env: &Env, proof: &Groth16Proof) -> Result<(), Error> {
        let hash = Self::proof_hash(env, proof);
        if env
            .storage()
            .persistent()
            .has(&DataKey::UsedProof(hash))
        {
            return Err(Error::ProofReplay);
        }
        Ok(())
    }

    /// Record a proof nullifier so the same bytes cannot be replayed
    /// anywhere in the contract.
    fn record_proof(env: &Env, proof: &Groth16Proof) {
        let hash = Self::proof_hash(env, proof);
        env.storage()
            .persistent()
            .set(&DataKey::UsedProof(hash.clone()), &true);
        env.storage().persistent().extend_ttl(
            &DataKey::UsedProof(hash),
            LEDGER_THRESHOLD,
            LEDGER_BUMP,
        );
    }

    /// Append the current state to the rolling history window, then drop
    /// any legacy `History` entry so readers see one source of truth.
    ///
    /// See the `DataKey::HistoryV2` doc for the V1-shape storage choice
    /// (audit-2026-04 LOW-4): history is persisted as `CommitmentEntry`,
    /// intentionally dropping `group_type` and `member_count`.
    fn archive_entry(env: &Env, group_id: &BytesN<32>, entry: &CommitmentEntryV2) {
        let mut history: Vec<CommitmentEntry> = env
            .storage()
            .persistent()
            .get(&DataKey::HistoryV2(group_id.clone()))
            .or_else(|| {
                env.storage()
                    .persistent()
                    .get(&DataKey::History(group_id.clone()))
            })
            .unwrap_or(Vec::new(env));

        // History is persisted in the V1 shape — group_type/member_count
        // are either invariant (group_type) or recoverable from the current
        // record, and history entries never need to round-trip through a V2
        // governance check.
        history.push_back(CommitmentEntry {
            commitment: entry.commitment.clone(),
            epoch: entry.epoch,
            timestamp: entry.timestamp,
            tier: entry.tier,
            active: entry.active,
        });

        if history.len() > HISTORY_WINDOW {
            let mut pruned = Vec::new(env);
            let start = history.len() - HISTORY_WINDOW;
            for i in start..history.len() {
                pruned.push_back(history.get(i).unwrap());
            }
            history = pruned;
        }

        env.storage()
            .persistent()
            .set(&DataKey::HistoryV2(group_id.clone()), &history);
        // Drop legacy history — any future read will see the V2 copy.
        if env
            .storage()
            .persistent()
            .has(&DataKey::History(group_id.clone()))
        {
            env.storage()
                .persistent()
                .remove(&DataKey::History(group_id.clone()));
        }
    }
}

// ================================================================
// Groth16 Verification
// ================================================================

/// Validate that every G1/G2 point in a VK lies in the prime-order subgroup.
///
/// Soroban's `G1Affine::from_bytes` / `G2Affine::from_bytes` are non-validating
/// byte wrappers (they check neither on-curve nor subgroup membership), so an
/// unvalidated VK could otherwise push an invalid-subgroup point into
/// `pairing_check` and trap at verify time. Called once at VK install
/// (`initialize` / `update_vk`) so each proof verification does not repay the
/// subgroup-check cost. Audit-2026-04 MEDIUM-1 + LOW-3.
///
/// `is_in_subgroup()` runs the host's on-curve check first, which means this
/// function surfaces two rejection modes:
///
/// * On-curve but not in the prime-order subgroup → `Err(InvalidPoint)` (the
///   actual subgroup-attack defense).
/// * Off-curve / malformed encoding → host traps with
///   `Error(Crypto, InvalidInput)` before the subgroup bit is inspected.
///
/// Both paths reject; only the first yields our typed error variant.
fn validate_vk_points(vk: &VerificationKeyData) -> Result<(), Error> {
    if !G1Affine::from_bytes(vk.alpha_g1.clone()).is_in_subgroup() {
        return Err(Error::InvalidPoint);
    }
    if !G2Affine::from_bytes(vk.beta_g2.clone()).is_in_subgroup() {
        return Err(Error::InvalidPoint);
    }
    if !G2Affine::from_bytes(vk.gamma_g2.clone()).is_in_subgroup() {
        return Err(Error::InvalidPoint);
    }
    if !G2Affine::from_bytes(vk.delta_g2.clone()).is_in_subgroup() {
        return Err(Error::InvalidPoint);
    }
    for i in 0..vk.ic.len() {
        if !G1Affine::from_bytes(vk.ic.get(i).unwrap()).is_in_subgroup() {
            return Err(Error::InvalidPoint);
        }
    }
    Ok(())
}

/// Validate that a proof's three points lie in the prime-order subgroup.
///
/// Returns `false` so the calling verifier can treat this as a normal
/// "invalid proof" without panicking — the three verifiers already collapse
/// every cryptographic rejection path into `-> bool`. Callers map `false`
/// into `Error::InvalidProof`. Audit-2026-04 MEDIUM-1.
///
/// Same caveat as `validate_vk_points`: on-curve-but-not-in-subgroup points
/// return `false` cleanly; off-curve / malformed encodings instead trap at
/// the host level with `Error(Crypto, InvalidInput)`. Both reject the proof.
fn validate_proof_points(proof: &Groth16Proof) -> bool {
    G1Affine::from_bytes(proof.a.clone()).is_in_subgroup()
        && G2Affine::from_bytes(proof.b.clone()).is_in_subgroup()
        && G1Affine::from_bytes(proof.c.clone()).is_in_subgroup()
}

/// Verify a Groth16 proof using BLS12-381 host functions.
fn verify_groth16_proof(
    env: &Env,
    vk: &VerificationKeyData,
    proof: &Groth16Proof,
    commitment: &BytesN<32>,
    epoch: u64,
) -> bool {
    let bls = env.crypto().bls12_381();

    if !validate_proof_points(proof) {
        return false;
    }

    let proof_a = G1Affine::from_bytes(proof.a.clone());
    let proof_b = G2Affine::from_bytes(proof.b.clone());
    let proof_c = G1Affine::from_bytes(proof.c.clone());

    let alpha = G1Affine::from_bytes(vk.alpha_g1.clone());
    let beta = G2Affine::from_bytes(vk.beta_g2.clone());
    let gamma = G2Affine::from_bytes(vk.gamma_g2.clone());
    let delta = G2Affine::from_bytes(vk.delta_g2.clone());

    let ic0 = G1Affine::from_bytes(vk.ic.get(0).unwrap());
    let ic1 = G1Affine::from_bytes(vk.ic.get(1).unwrap());
    let ic2 = G1Affine::from_bytes(vk.ic.get(2).unwrap());

    let commitment_fr = Fr::from_bytes(commitment.clone());
    // Canonical check: reject non-canonical field elements (>= modulus).
    // Fr::from_bytes silently reduces mod r; the roundtrip detects this.
    let canonical_bytes: BytesN<32> = commitment_fr.to_bytes();
    if canonical_bytes != *commitment {
        return false;
    }
    let epoch_bytes = Bytes::from_array(env, &u64_to_u256_be(epoch));
    let epoch_fr = Fr::from_u256(U256::from_be_bytes(env, &epoch_bytes));

    let msm_points: Vec<G1Affine> = vec![env, ic1, ic2];
    let msm_scalars: Vec<Fr> = vec![env, commitment_fr, epoch_fr];
    let msm_result = bls.g1_msm(msm_points, msm_scalars);
    let vk_x = bls.g1_add(&ic0, &msm_result);

    let neg_a = -proof_a;

    let g1s: Vec<G1Affine> = vec![env, neg_a, alpha, vk_x, proof_c];
    let g2s: Vec<G2Affine> = vec![env, proof_b, beta, gamma, delta];

    bls.pairing_check(g1s, g2s)
}

fn u64_to_u256_be(val: u64) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[24..32].copy_from_slice(&val.to_be_bytes());
    bytes
}

/// Verify an UpdateCircuit Groth16 proof with 3 public inputs: `(c_old, epoch_old, c_new)`.
///
/// The IC vector has exactly 4 elements (validated at `initialize` / `update_vk`
/// time). `vk_x = IC[0] + c_old·IC[1] + epoch_old·IC[2] + c_new·IC[3]`.
///
/// Pairing check: `e(-π_A, π_B) · e(α, β) · e(vk_x, γ) · e(π_C, δ) = 1_GT`.
fn verify_groth16_proof_update(
    env: &Env,
    vk: &VerificationKeyData,
    proof: &Groth16Proof,
    c_old: &BytesN<32>,
    epoch_old: u64,
    c_new: &BytesN<32>,
) -> bool {
    let bls = env.crypto().bls12_381();

    if !validate_proof_points(proof) {
        return false;
    }

    let proof_a = G1Affine::from_bytes(proof.a.clone());
    let proof_b = G2Affine::from_bytes(proof.b.clone());
    let proof_c = G1Affine::from_bytes(proof.c.clone());

    let alpha = G1Affine::from_bytes(vk.alpha_g1.clone());
    let beta = G2Affine::from_bytes(vk.beta_g2.clone());
    let gamma = G2Affine::from_bytes(vk.gamma_g2.clone());
    let delta = G2Affine::from_bytes(vk.delta_g2.clone());

    // ic.len() was validated == 4 at set time, but guard against storage tampering.
    if vk.ic.len() != 4 {
        return false;
    }
    let ic0 = G1Affine::from_bytes(vk.ic.get(0).unwrap());
    let ic1 = G1Affine::from_bytes(vk.ic.get(1).unwrap());
    let ic2 = G1Affine::from_bytes(vk.ic.get(2).unwrap());
    let ic3 = G1Affine::from_bytes(vk.ic.get(3).unwrap());

    // Canonical check on c_old (same roundtrip rule as the membership verifier).
    let c_old_fr = Fr::from_bytes(c_old.clone());
    let c_old_canonical: BytesN<32> = c_old_fr.to_bytes();
    if c_old_canonical != *c_old {
        return false;
    }
    // Canonical check on c_new (defence-in-depth; `update_commitment` already enforces this).
    let c_new_fr = Fr::from_bytes(c_new.clone());
    let c_new_canonical: BytesN<32> = c_new_fr.to_bytes();
    if c_new_canonical != *c_new {
        return false;
    }

    let epoch_bytes = Bytes::from_array(env, &u64_to_u256_be(epoch_old));
    let epoch_fr = Fr::from_u256(U256::from_be_bytes(env, &epoch_bytes));

    let msm_points: Vec<G1Affine> = vec![env, ic1, ic2, ic3];
    let msm_scalars: Vec<Fr> = vec![env, c_old_fr, epoch_fr, c_new_fr];
    let msm_result = bls.g1_msm(msm_points, msm_scalars);
    let vk_x = bls.g1_add(&ic0, &msm_result);

    let neg_a = -proof_a;

    let g1s: Vec<G1Affine> = vec![env, neg_a, alpha, vk_x, proof_c];
    let g2s: Vec<G2Affine> = vec![env, proof_b, beta, gamma, delta];

    bls.pairing_check(g1s, g2s)
}

fn u32_to_fr(env: &Env, value: u32) -> Fr {
    let bytes = Bytes::from_array(env, &u64_to_u256_be(value as u64));
    Fr::from_u256(U256::from_be_bytes(env, &bytes))
}

/// Verify a DemocracyUpdateCircuit Groth16 proof with 5 public inputs:
/// `(c_old, epoch_old, c_new, member_count_old, member_count_new)`.
///
/// IC length is 6: `IC[0]` base plus one per public input.
/// `vk_x = IC[0] + c_old·IC[1] + epoch_old·IC[2] + c_new·IC[3] +
///        member_count_old·IC[4] + member_count_new·IC[5]`.
#[allow(clippy::too_many_arguments)]
fn verify_groth16_proof_democracy_update(
    env: &Env,
    vk: &VerificationKeyData,
    proof: &Groth16Proof,
    c_old: &BytesN<32>,
    epoch_old: u64,
    c_new: &BytesN<32>,
    member_count_old: u32,
    member_count_new: u32,
) -> bool {
    let bls = env.crypto().bls12_381();

    if !validate_proof_points(proof) {
        return false;
    }

    let proof_a = G1Affine::from_bytes(proof.a.clone());
    let proof_b = G2Affine::from_bytes(proof.b.clone());
    let proof_c = G1Affine::from_bytes(proof.c.clone());

    let alpha = G1Affine::from_bytes(vk.alpha_g1.clone());
    let beta = G2Affine::from_bytes(vk.beta_g2.clone());
    let gamma = G2Affine::from_bytes(vk.gamma_g2.clone());
    let delta = G2Affine::from_bytes(vk.delta_g2.clone());

    // Validated == 6 at set time; defence-in-depth against storage tampering.
    if vk.ic.len() != 6 {
        return false;
    }
    let ic0 = G1Affine::from_bytes(vk.ic.get(0).unwrap());
    let ic1 = G1Affine::from_bytes(vk.ic.get(1).unwrap());
    let ic2 = G1Affine::from_bytes(vk.ic.get(2).unwrap());
    let ic3 = G1Affine::from_bytes(vk.ic.get(3).unwrap());
    let ic4 = G1Affine::from_bytes(vk.ic.get(4).unwrap());
    let ic5 = G1Affine::from_bytes(vk.ic.get(5).unwrap());

    let c_old_fr = Fr::from_bytes(c_old.clone());
    let c_old_canonical: BytesN<32> = c_old_fr.to_bytes();
    if c_old_canonical != *c_old {
        return false;
    }
    let c_new_fr = Fr::from_bytes(c_new.clone());
    let c_new_canonical: BytesN<32> = c_new_fr.to_bytes();
    if c_new_canonical != *c_new {
        return false;
    }

    let epoch_bytes = Bytes::from_array(env, &u64_to_u256_be(epoch_old));
    let epoch_fr = Fr::from_u256(U256::from_be_bytes(env, &epoch_bytes));
    let m_old_fr = u32_to_fr(env, member_count_old);
    let m_new_fr = u32_to_fr(env, member_count_new);

    let msm_points: Vec<G1Affine> = vec![env, ic1, ic2, ic3, ic4, ic5];
    let msm_scalars: Vec<Fr> = vec![env, c_old_fr, epoch_fr, c_new_fr, m_old_fr, m_new_fr];
    let msm_result = bls.g1_msm(msm_points, msm_scalars);
    let vk_x = bls.g1_add(&ic0, &msm_result);

    let neg_a = -proof_a;
    let g1s: Vec<G1Affine> = vec![env, neg_a, alpha, vk_x, proof_c];
    let g2s: Vec<G2Affine> = vec![env, proof_b, beta, gamma, delta];
    bls.pairing_check(g1s, g2s)
}

// ================================================================
// Tests
// ================================================================

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    /// Audit-followup Finding #5: contracts register with `__constructor`
    /// args now — the constructor initialises admin + all VKs atomically at
    /// deploy time, so tests start with a fully-initialised contract.
    /// Tests that exercised the old `initialize` entrypoint still cover
    /// `AlreadyInitialized` via `try_initialize` on an already-constructed
    /// client; tests that wanted input-validation panics now get them at
    /// `env.register` time (the constructor propagates the typed contract
    /// error verbatim).
    fn setup_env() -> (Env, SepXxxxContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let vk = mock_vk(&env);
        let uvk = mock_update_vk(&env);
        let contract_id = env.register(
            SepXxxxContract,
            (
                admin.clone(),
                vk.clone(),
                vk.clone(),
                vk.clone(),
                uvk.clone(),
                uvk.clone(),
                uvk,
            ),
        );
        let client = SepXxxxContractClient::new(&env, &contract_id);
        (env, client, admin)
    }

    /// Register the contract with an arbitrary membership VK (for the
    /// slot that would otherwise use `mock_vk`) so we can drive the
    /// constructor's VK-length / subgroup validation paths. All other
    /// slots use valid mocks.
    ///
    /// The bad VK is built via a closure so its `BytesN` objects are
    /// bound to the same `Env` used for `env.register`. Building the VK
    /// on a different `Env` and passing it in would trigger a
    /// "mis-tagged object reference" host error.
    fn setup_env_bad_membership_vk(
        build_bad_vk: impl FnOnce(&Env) -> VerificationKeyData,
    ) -> (Env, SepXxxxContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let bad_vk = build_bad_vk(&env);
        let good_vk = mock_vk(&env);
        let uvk = mock_update_vk(&env);
        // `env.register` panics if the constructor errors; tests that
        // drive this helper use `#[should_panic]` to catch the panic.
        let contract_id = env.register(
            SepXxxxContract,
            (
                admin.clone(),
                bad_vk,
                good_vk.clone(),
                good_vk,
                uvk.clone(),
                uvk.clone(),
                uvk,
            ),
        );
        let client = SepXxxxContractClient::new(&env, &contract_id);
        (env, client, admin)
    }

    fn setup_env_bad_update_vk(
        build_bad_update_vk: impl FnOnce(&Env) -> VerificationKeyData,
    ) -> (Env, SepXxxxContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let bad_update_vk = build_bad_update_vk(&env);
        let vk = mock_vk(&env);
        let good_update = mock_update_vk(&env);
        let contract_id = env.register(
            SepXxxxContract,
            (
                admin.clone(),
                vk.clone(),
                vk.clone(),
                vk,
                bad_update_vk,
                good_update.clone(),
                good_update,
            ),
        );
        let client = SepXxxxContractClient::new(&env, &contract_id);
        (env, client, admin)
    }

    // All mock VK/proof helpers produce subgroup-valid G1/G2 points via
    // `hash_to_g1` / `hash_to_g2`. Plain zero-byte encodings no longer pass
    // `validate_vk_points` (audit-2026-04 MEDIUM-1 + LOW-3): `initialize`
    // and `update_vk` now reject any VK with off-subgroup points, and the
    // three verifiers reject proofs whose `a/b/c` are off-subgroup.
    //
    // These hashes are *not* cryptographically linked to any particular
    // circuit — they just yield curve points the contract accepts
    // structurally. Tests that reach the pairing check still fail at
    // `pairing_check`; tests that only exercise contract-side error
    // paths (tier/epoch/replay/canonicality) are unaffected.

    fn valid_g1(env: &Env, tag: &[u8]) -> BytesN<96> {
        let bls = env.crypto().bls12_381();
        let dst = Bytes::from_slice(env, b"sep-xxxx-test-g1");
        let msg = Bytes::from_slice(env, tag);
        bls.hash_to_g1(&msg, &dst).to_bytes()
    }

    fn valid_g2(env: &Env, tag: &[u8]) -> BytesN<192> {
        let bls = env.crypto().bls12_381();
        let dst = Bytes::from_slice(env, b"sep-xxxx-test-g2");
        let msg = Bytes::from_slice(env, tag);
        bls.hash_to_g2(&msg, &dst).to_bytes()
    }

    fn mock_vk(env: &Env) -> VerificationKeyData {
        VerificationKeyData {
            alpha_g1: valid_g1(env, b"alpha"),
            beta_g2: valid_g2(env, b"beta"),
            gamma_g2: valid_g2(env, b"gamma"),
            delta_g2: valid_g2(env, b"delta"),
            ic: vec![
                env,
                valid_g1(env, b"ic0"),
                valid_g1(env, b"ic1"),
                valid_g1(env, b"ic2"),
            ],
        }
    }

    fn mock_update_vk(env: &Env) -> VerificationKeyData {
        VerificationKeyData {
            alpha_g1: valid_g1(env, b"u-alpha"),
            beta_g2: valid_g2(env, b"u-beta"),
            gamma_g2: valid_g2(env, b"u-gamma"),
            delta_g2: valid_g2(env, b"u-delta"),
            ic: vec![
                env,
                valid_g1(env, b"u-ic0"),
                valid_g1(env, b"u-ic1"),
                valid_g1(env, b"u-ic2"),
                valid_g1(env, b"u-ic3"),
            ],
        }
    }

    fn mock_proof(env: &Env) -> Groth16Proof {
        Groth16Proof {
            a: valid_g1(env, b"proof-a"),
            b: valid_g2(env, b"proof-b"),
            c: valid_g1(env, b"proof-c"),
        }
    }

    /// Smoke test: `setup_env` registers the contract via `__constructor`,
    /// which means a successful return here is itself proof that the
    /// constructor wrote the admin + VK slots without erroring.
    #[test]
    fn test_initialize() {
        let (_env, _client, _admin) = setup_env();
    }

    // Double-initialize rejection is covered by
    // `test_constructor_initializes_atomically`, which uses
    // `try_initialize` to assert `AlreadyInitialized` without exhausting
    // the test budget on a second full VK validation pass.

    /// The constructor rejects a membership VK with the wrong IC length.
    /// `env.register` unwraps the host error, so the `#[should_panic]`
    /// matcher sees the typed contract error verbatim.
    #[test]
    #[should_panic(expected = "Error(Contract, #9)")]
    fn test_invalid_vk_length_rejected() {
        let _ = setup_env_bad_membership_vk(|env| {
            let g1 = valid_g1(env, b"bad-alpha");
            let g2 = valid_g2(env, b"bad-beta");
            VerificationKeyData {
                alpha_g1: g1.clone(),
                beta_g2: g2.clone(),
                gamma_g2: g2.clone(),
                delta_g2: g2,
                ic: vec![env, g1.clone(), g1],
            }
        });
    }

    /// The constructor rejects an update VK with 3 IC points (needs 4).
    #[test]
    #[should_panic(expected = "Error(Contract, #9)")]
    fn test_invalid_update_vk_length_rejected() {
        let _ = setup_env_bad_update_vk(|env| {
            let g1 = valid_g1(env, b"u-bad-alpha");
            let g2 = valid_g2(env, b"u-bad-beta");
            VerificationKeyData {
                alpha_g1: g1.clone(),
                beta_g2: g2.clone(),
                gamma_g2: g2.clone(),
                delta_g2: g2,
                ic: vec![env, g1.clone(), g1.clone(), g1],
            }
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #5)")]
    fn test_group_not_found() {
        let (env, client, _admin) = setup_env();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        client.get_state(&group_id);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #8)")]
    fn test_invalid_tier_rejected() {
        let (env, client, _admin) = setup_env();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };

        client.create_group(&caller, &group_id, &commitment, &3u32, &mock_proof(&env), &pi);
    }

    // NOTE: post-Finding #5 there is no test-reachable uninitialised-
    // contract state — `setup_env` registers the contract via
    // `__constructor`, which writes the admin + VK slots atomically. The
    // `NotInitialized` error path still exists in production for pre-
    // constructor contracts deployed before v1.10.19 and not yet migrated
    // via the legacy `initialize` entrypoint.

    #[test]
    #[should_panic(expected = "Error(Contract, #10)")]
    fn test_public_inputs_mismatch_on_create() {
        let (env, client, _admin) = setup_env();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        let wrong_pi = PublicInputs {
            commitment: BytesN::from_array(&env, &[9u8; 32]),
            epoch: 0,
        };

        client.create_group(&caller, &group_id, &commitment, &0u32, &mock_proof(&env), &wrong_pi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #10)")]
    fn test_public_inputs_wrong_epoch_on_create() {
        let (env, client, _admin) = setup_env();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        let wrong_pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 5,
        };

        client.create_group(&caller, &group_id, &commitment, &0u32, &mock_proof(&env), &wrong_pi);
    }

    // NOTE: Tests exercising actual Groth16 verification require valid
    // test vectors (VK, proof, public inputs) generated by the circuits
    // crate. End-to-end verification is covered by the testnet deployment
    // script (scripts/deploy_sep_xxxx_testnet.sh).

    // ================================================================
    // Additional Helpers
    // ================================================================

    /// `setup_env` already initialises the contract via `__constructor`
    /// (audit-followup Finding #5); this wrapper just also returns the
    /// contract address for tests that want to reach into storage via
    /// `env.as_contract`.
    fn setup_initialized() -> (Env, SepXxxxContractClient<'static>, Address, Address) {
        let (env, client, admin) = setup_env();
        let contract_id = client.address.clone();
        (env, client, admin, contract_id)
    }

    /// Inject an active group directly into contract storage.
    /// Bypasses create_group (which requires BLS12-381 host functions).
    fn inject_group(
        env: &Env,
        contract_id: &Address,
        group_id: &BytesN<32>,
        commitment: &BytesN<32>,
        epoch: u64,
        tier: u32,
    ) {
        env.as_contract(contract_id, || {
            let entry = CommitmentEntry {
                commitment: commitment.clone(),
                epoch,
                timestamp: env.ledger().timestamp(),
                tier,
                active: true,
            };
            env.storage()
                .persistent()
                .set(&DataKey::Group(group_id.clone()), &entry);
            env.storage().persistent().set(
                &DataKey::History(group_id.clone()),
                &Vec::<CommitmentEntry>::new(env),
            );
            let count: u32 = env
                .storage()
                .instance()
                .get(&DataKey::GroupCount(tier))
                .unwrap_or(0);
            env.storage()
                .instance()
                .set(&DataKey::GroupCount(tier), &(count + 1));
        });
    }

    /// Inject a deactivated group directly into contract storage.
    fn inject_inactive_group(
        env: &Env,
        contract_id: &Address,
        group_id: &BytesN<32>,
        commitment: &BytesN<32>,
        epoch: u64,
        tier: u32,
    ) {
        env.as_contract(contract_id, || {
            let entry = CommitmentEntry {
                commitment: commitment.clone(),
                epoch,
                timestamp: env.ledger().timestamp(),
                tier,
                active: false,
            };
            env.storage()
                .persistent()
                .set(&DataKey::Group(group_id.clone()), &entry);
            env.storage().persistent().set(
                &DataKey::History(group_id.clone()),
                &Vec::<CommitmentEntry>::new(env),
            );
        });
    }

    /// Record a proof nullifier as used in contract storage. Must match
    /// the hashing scheme in `proof_hash` (global, no group_id scope).
    /// `_group_id` is retained in the signature for call-site legibility.
    fn inject_used_proof(
        env: &Env,
        contract_id: &Address,
        _group_id: &BytesN<32>,
        proof: &Groth16Proof,
    ) {
        env.as_contract(contract_id, || {
            let mut preimage = Bytes::new(env);
            preimage.append(&Bytes::from_slice(env, proof.a.to_array().as_slice()));
            preimage.append(&Bytes::from_slice(env, proof.b.to_array().as_slice()));
            preimage.append(&Bytes::from_slice(env, proof.c.to_array().as_slice()));
            let hash: BytesN<32> = env.crypto().sha256(&preimage).into();
            env.storage()
                .persistent()
                .set(&DataKey::UsedProof(hash), &true);
        });
    }

    /// Set a tier's group count directly in contract storage.
    fn inject_tier_count(env: &Env, contract_id: &Address, tier: u32, count: u32) {
        env.as_contract(contract_id, || {
            env.storage()
                .instance()
                .set(&DataKey::GroupCount(tier), &count);
        });
    }

    /// Inject a group with pre-populated history entries.
    fn inject_group_with_history(
        env: &Env,
        contract_id: &Address,
        group_id: &BytesN<32>,
        commitment: &BytesN<32>,
        epoch: u64,
        tier: u32,
        history: Vec<CommitmentEntry>,
    ) {
        env.as_contract(contract_id, || {
            let entry = CommitmentEntry {
                commitment: commitment.clone(),
                epoch,
                timestamp: env.ledger().timestamp(),
                tier,
                active: true,
            };
            env.storage()
                .persistent()
                .set(&DataKey::Group(group_id.clone()), &entry);
            env.storage()
                .persistent()
                .set(&DataKey::History(group_id.clone()), &history);
        });
    }

    // ================================================================
    // State & Lifecycle Tests
    // ================================================================

    #[test]
    fn test_get_state_returns_injected_entry() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 0, 1);

        let state = client.get_state(&group_id);
        assert_eq!(state.commitment, commitment);
        assert_eq!(state.epoch, 0);
        assert_eq!(state.tier, 1);
        assert!(state.active);
    }

    #[test]
    fn test_get_state_at_nonzero_epoch() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 42, 2);

        let state = client.get_state(&group_id);
        assert_eq!(state.epoch, 42);
        assert_eq!(state.tier, 2);
    }

    #[test]
    fn test_deactivated_group_get_state_still_works() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_inactive_group(&env, &contract_id, &group_id, &commitment, 5, 0);

        let state = client.get_state(&group_id);
        assert_eq!(state.commitment, commitment);
        assert_eq!(state.epoch, 5);
        assert!(!state.active);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #4)")]
    fn test_create_group_rejects_duplicate_group_id() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 0, 0);

        let caller = Address::generate(&env);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_group(&caller, &group_id, &commitment, &0u32, &mock_proof(&env), &pi);
    }

    // ================================================================
    // update_commitment Error Paths
    // ================================================================
    //
    // #59: `new_commitment` / `new_epoch` are no longer client-supplied.
    // The new commitment is the proof's public input `c_new`; the new
    // epoch is derived on-chain as `stored_epoch + 1`. Tests that used to
    // provide a mismatched `new_epoch` and expect #11 InvalidEpoch have
    // been removed — that code path no longer exists.

    fn c_new_ok(env: &Env) -> BytesN<32> {
        // 32-byte all-zeroes is canonical Fr (zero element).
        BytesN::from_array(env, &[0u8; 32])
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #5)")]
    fn test_update_commitment_group_not_found() {
        let (env, client, _admin, _cid) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        let upi = UpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 0,
            c_new: c_new_ok(&env),
        };
        client.update_commitment(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #6)")]
    fn test_update_commitment_rejects_inactive_group() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_inactive_group(&env, &contract_id, &group_id, &commitment, 5, 0);

        let upi = UpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 5,
            c_new: c_new_ok(&env),
        };
        client.update_commitment(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #10)")]
    fn test_update_commitment_wrong_c_old() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 5, 0);

        let upi = UpdatePublicInputs {
            c_old: BytesN::from_array(&env, &[9u8; 32]),
            epoch_old: 5,
            c_new: c_new_ok(&env),
        };
        client.update_commitment(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #10)")]
    fn test_update_commitment_wrong_epoch_old() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 5, 0);

        let upi = UpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 3, // stored is 5
            c_new: c_new_ok(&env),
        };
        client.update_commitment(&group_id, &mock_proof(&env), &upi);
    }

    // ================================================================
    // Epoch Enforcement — #59 fix derives new_epoch on-chain.
    // Only the u64::MAX overflow path survives as a dedicated test.
    // ================================================================

    #[test]
    #[should_panic(expected = "Error(Contract, #11)")]
    fn test_epoch_overflow_handled() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        // Inject group at u64::MAX — checked_add(1) overflows.
        inject_group(&env, &contract_id, &group_id, &commitment, u64::MAX, 0);

        let upi = UpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: u64::MAX,
            c_new: c_new_ok(&env),
        };
        client.update_commitment(&group_id, &mock_proof(&env), &upi);
    }

    // ================================================================
    // #59: Non-canonical c_new is rejected up front.
    // ================================================================

    #[test]
    #[should_panic(expected = "Error(Contract, #15)")]
    fn test_update_commitment_rejects_non_canonical_c_new() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 5, 0);

        // All-0xFF is >= BLS12-381 Fr modulus — not a canonical encoding.
        let non_canonical = BytesN::from_array(&env, &[0xFFu8; 32]);
        let upi = UpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 5,
            c_new: non_canonical,
        };
        client.update_commitment(&group_id, &mock_proof(&env), &upi);
    }

    // ================================================================
    // Proof Replay Prevention — Theorem 6
    // ================================================================

    #[test]
    #[should_panic(expected = "Error(Contract, #12)")]
    fn test_proof_replay_rejected_on_create() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let proof = mock_proof(&env);
        inject_used_proof(&env, &contract_id, &group_id, &proof);

        let caller = Address::generate(&env);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_group(&caller, &group_id, &commitment, &0u32, &proof, &pi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #12)")]
    fn test_proof_replay_rejected_on_update() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 5, 0);

        let proof = mock_proof(&env);
        inject_used_proof(&env, &contract_id, &group_id, &proof);

        let upi = UpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 5,
            c_new: c_new_ok(&env),
        };
        client.update_commitment(&group_id, &proof, &upi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #12)")]
    fn test_proof_replay_rejected_on_deactivate() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 5, 0);

        let proof = mock_proof(&env);
        inject_used_proof(&env, &contract_id, &group_id, &proof);

        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 5,
        };
        client.deactivate_group(&group_id, &proof, &pi);
    }

    // ================================================================
    // deactivate_group Error Paths
    // ================================================================

    #[test]
    #[should_panic(expected = "Error(Contract, #5)")]
    fn test_deactivate_group_not_found() {
        let (env, client, _admin, _cid) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.deactivate_group(&group_id, &mock_proof(&env), &pi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #6)")]
    fn test_deactivate_rejects_already_inactive() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_inactive_group(&env, &contract_id, &group_id, &commitment, 5, 0);

        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 5,
        };
        client.deactivate_group(&group_id, &mock_proof(&env), &pi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #10)")]
    fn test_deactivate_public_inputs_mismatch() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 5, 0);

        let pi = PublicInputs {
            commitment: BytesN::from_array(&env, &[9u8; 32]),
            epoch: 5,
        };
        client.deactivate_group(&group_id, &mock_proof(&env), &pi);
    }

    // ================================================================
    // verify_membership Error Paths
    // ================================================================

    #[test]
    #[should_panic(expected = "Error(Contract, #5)")]
    fn test_verify_membership_group_not_found() {
        let (env, client, _admin, _cid) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.verify_membership(&group_id, &mock_proof(&env), &pi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #10)")]
    fn test_verify_membership_public_inputs_mismatch() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 3, 0);

        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 99, // doesn't match stored epoch 3
        };
        client.verify_membership(&group_id, &mock_proof(&env), &pi);
    }

    /// Re-audit Finding #1: `verify_membership` is now read-only
    /// again (the split-API fix). A previously-burnt nullifier MUST
    /// NOT block it — replay protection is the job of
    /// `consume_membership_proof` instead. The call still returns
    /// `false` because the mock proof doesn't verify cryptographically,
    /// but importantly it does not panic with `ProofReplay (#12)`.
    #[test]
    fn test_verify_membership_is_read_only_ignores_nullifier() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 3, 0);

        let proof = mock_proof(&env);
        inject_used_proof(&env, &contract_id, &group_id, &proof);

        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 3,
        };
        // Returns false (proof does not verify), but does not panic
        // with ProofReplay — read-only path bypasses the nullifier check.
        assert!(!client.verify_membership(&group_id, &proof, &pi));
    }

    /// Re-audit Finding #1: `consume_membership_proof` is the
    /// state-changing variant that DOES enforce the nullifier. A
    /// pre-recorded nullifier must block it before the verifier runs.
    #[test]
    #[should_panic(expected = "Error(Contract, #12)")]
    fn test_consume_membership_proof_rejects_replayed_proof() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 3, 0);

        let proof = mock_proof(&env);
        inject_used_proof(&env, &contract_id, &group_id, &proof);

        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 3,
        };
        client.consume_membership_proof(&group_id, &proof, &pi);
    }

    /// Re-audit Finding #1: `consume_membership_proof` must surface
    /// the same `PublicInputsMismatch` + `GroupNotFound` error paths
    /// as `verify_membership`, and must be callable.
    #[test]
    #[should_panic(expected = "Error(Contract, #10)")]
    fn test_consume_membership_proof_public_inputs_mismatch() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 3, 0);

        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 99,
        };
        client.consume_membership_proof(&group_id, &mock_proof(&env), &pi);
    }

    /// Regression: `verify_membership` must resolve V2-native groups (those
    /// created by `create_group_v2` with no legacy `DataKey::Group` entry).
    /// Previously it read `DataKey::Group` directly and returned
    /// `GroupNotFound` for every V2-native group. After the fix it reads via
    /// `load_group_v2`, so mismatched inputs surface as `PublicInputsMismatch`
    /// (#10) rather than `GroupNotFound` (#5).
    #[test]
    #[should_panic(expected = "Error(Contract, #10)")]
    fn test_verify_membership_resolves_v2_native_group() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        // Write ONLY the V2 record — no legacy DataKey::Group entry.
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 3, 0, 0, 0);

        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 99, // doesn't match stored epoch 3
        };
        client.verify_membership(&group_id, &mock_proof(&env), &pi);
    }

    // ================================================================
    // VK Management Tests
    // ================================================================

    #[test]
    fn test_update_vk_succeeds() {
        let (env, client, _admin, _cid) = setup_initialized();
        let new_vk = mock_vk(&env);
        client.update_vk(&VkKind::Membership, &0u32, &new_vk);
    }

    #[test]
    fn test_update_vk_all_tiers() {
        let (env, client, _admin, _cid) = setup_initialized();
        let new_vk = mock_vk(&env);
        client.update_vk(&VkKind::Membership, &0u32, &new_vk);
        client.update_vk(&VkKind::Membership, &1u32, &new_vk);
        client.update_vk(&VkKind::Membership, &2u32, &new_vk);
    }

    #[test]
    fn test_update_update_vk_succeeds() {
        let (env, client, _admin, _cid) = setup_initialized();
        let new_update_vk = mock_update_vk(&env);
        client.update_vk(&VkKind::Update, &0u32, &new_update_vk);
        client.update_vk(&VkKind::Update, &1u32, &new_update_vk);
        client.update_vk(&VkKind::Update, &2u32, &new_update_vk);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #8)")]
    fn test_update_vk_invalid_tier() {
        let (env, client, _admin, _cid) = setup_initialized();
        let vk = mock_vk(&env);
        client.update_vk(&VkKind::Membership, &3u32, &vk);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #9)")]
    fn test_update_vk_invalid_ic_length() {
        let (env, client, _admin, _cid) = setup_initialized();
        let g1 = BytesN::from_array(&env, &[0u8; 96]);
        let g2 = BytesN::from_array(&env, &[0u8; 192]);
        let bad_vk = VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![&env, g1.clone(), g1], // only 2 IC points, need 3
        };
        client.update_vk(&VkKind::Membership, &0u32, &bad_vk);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #9)")]
    fn test_update_update_vk_wrong_ic_length() {
        // Passing a 3-IC VK for VkKind::Update must fail (Update wants 4 IC).
        let (env, client, _admin, _cid) = setup_initialized();
        let wrong_vk = mock_vk(&env); // has 3 IC, not 4
        client.update_vk(&VkKind::Update, &0u32, &wrong_vk);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #9)")]
    fn test_update_membership_vk_wrong_ic_length() {
        // Passing a 4-IC update VK for VkKind::Membership must fail.
        let (env, client, _admin, _cid) = setup_initialized();
        let wrong_vk = mock_update_vk(&env); // has 4 IC, not 3
        client.update_vk(&VkKind::Membership, &0u32, &wrong_vk);
    }

    // ================================================================
    // Tier Limits — M-4
    // ================================================================

    #[test]
    #[should_panic(expected = "Error(Contract, #13)")]
    fn test_tier_limit_enforced_on_create() {
        let (env, client, _admin, contract_id) = setup_initialized();
        // Set tier 0 count to the maximum
        inject_tier_count(&env, &contract_id, 0, MAX_GROUPS_PER_TIER);

        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_group(&caller, &group_id, &commitment, &0u32, &mock_proof(&env), &pi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #13)")]
    fn test_tier_limit_exact_boundary() {
        let (env, client, _admin, contract_id) = setup_initialized();
        // Exactly at the limit — should be rejected
        inject_tier_count(&env, &contract_id, 1, MAX_GROUPS_PER_TIER);

        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_group(&caller, &group_id, &commitment, &1u32, &mock_proof(&env), &pi);
    }

    #[test]
    fn test_tier_counts_stored_correctly() {
        let (env, _client, _admin, contract_id) = setup_initialized();
        // Inject groups in different tiers and verify counts via get_state
        let g1 = BytesN::from_array(&env, &[1u8; 32]);
        let g2 = BytesN::from_array(&env, &[2u8; 32]);
        let commitment = BytesN::from_array(&env, &[3u8; 32]);
        inject_group(&env, &contract_id, &g1, &commitment, 0, 0); // tier 0
        inject_group(&env, &contract_id, &g2, &commitment, 0, 1); // tier 1

        let s1 = _client.get_state(&g1);
        let s2 = _client.get_state(&g2);
        assert_eq!(s1.tier, 0);
        assert_eq!(s2.tier, 1);
    }

    // ================================================================
    // Restricted Mode — N-26
    // ================================================================

    #[test]
    #[should_panic(expected = "Error(Contract, #14)")]
    fn test_restricted_mode_blocks_non_admin() {
        let (env, client, _admin, _cid) = setup_initialized();
        client.set_restricted_mode(&true);

        let non_admin = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_group(
            &non_admin,
            &group_id,
            &commitment,
            &0u32,
            &mock_proof(&env),
            &pi,
        );
    }

    #[test]
    fn test_set_restricted_mode_succeeds() {
        let (_env, client, _admin, _cid) = setup_initialized();
        client.set_restricted_mode(&true);
        client.set_restricted_mode(&false);
    }

    // ================================================================
    // History Tests
    // ================================================================

    #[test]
    fn test_history_initially_empty() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 0, 0);

        let history = client.get_history(&group_id, &100);
        assert_eq!(history.len(), 0);
    }

    #[test]
    fn test_history_populated_via_injection() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);

        let mut history = Vec::new(&env);
        for i in 0..5u64 {
            history.push_back(CommitmentEntry {
                commitment: commitment.clone(),
                epoch: i,
                timestamp: 1000 + i,
                tier: 0,
                active: true,
            });
        }
        inject_group_with_history(
            &env,
            &contract_id,
            &group_id,
            &commitment,
            5,
            0,
            history,
        );

        let result = client.get_history(&group_id, &100);
        assert_eq!(result.len(), 5);
        assert_eq!(result.get(0).unwrap().epoch, 0);
        assert_eq!(result.get(4).unwrap().epoch, 4);
    }

    #[test]
    fn test_get_history_respects_max_entries() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);

        let mut history = Vec::new(&env);
        for i in 0..10u64 {
            history.push_back(CommitmentEntry {
                commitment: commitment.clone(),
                epoch: i,
                timestamp: 1000 + i,
                tier: 0,
                active: true,
            });
        }
        inject_group_with_history(
            &env,
            &contract_id,
            &group_id,
            &commitment,
            10,
            0,
            history,
        );

        // Request only 3 most recent
        let result = client.get_history(&group_id, &3);
        assert_eq!(result.len(), 3);
        // Should return epochs 7, 8, 9 (most recent)
        assert_eq!(result.get(0).unwrap().epoch, 7);
        assert_eq!(result.get(1).unwrap().epoch, 8);
        assert_eq!(result.get(2).unwrap().epoch, 9);
    }

    #[test]
    fn test_get_history_max_exceeds_length() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);

        let mut history = Vec::new(&env);
        for i in 0..3u64 {
            history.push_back(CommitmentEntry {
                commitment: commitment.clone(),
                epoch: i,
                timestamp: 1000 + i,
                tier: 0,
                active: true,
            });
        }
        inject_group_with_history(
            &env,
            &contract_id,
            &group_id,
            &commitment,
            3,
            0,
            history,
        );

        // Request 100 but only 3 exist
        let result = client.get_history(&group_id, &100);
        assert_eq!(result.len(), 3);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #5)")]
    fn test_get_history_group_not_found() {
        let (_env, client, _admin, _cid) = setup_initialized();
        let group_id = BytesN::from_array(&_env, &[1u8; 32]);
        client.get_history(&group_id, &10);
    }

    // ================================================================
    // TTL Bumping Tests
    // ================================================================

    #[test]
    fn test_bump_group_ttl_succeeds() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 0, 0);

        // Should not panic — callable by anyone
        client.bump_group_ttl(&group_id);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #5)")]
    fn test_bump_group_ttl_not_found() {
        let (env, client, _admin, _cid) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        client.bump_group_ttl(&group_id);
    }

    #[test]
    fn test_bump_callable_by_anyone() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 0, 0);

        // bump_group_ttl doesn't require admin or any auth
        // (env.mock_all_auths is set, but the function itself doesn't call require_auth)
        client.bump_group_ttl(&group_id);
        // Calling twice should also work
        client.bump_group_ttl(&group_id);
    }

    // ================================================================
    // Groth16 Helper Tests
    // ================================================================

    #[test]
    fn test_u64_to_u256_be_zero() {
        let result = u64_to_u256_be(0);
        assert_eq!(result, [0u8; 32]);
    }

    #[test]
    fn test_u64_to_u256_be_one() {
        let result = u64_to_u256_be(1);
        let mut expected = [0u8; 32];
        expected[31] = 1;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_u64_to_u256_be_max() {
        let result = u64_to_u256_be(u64::MAX);
        let mut expected = [0u8; 32];
        expected[24..32].copy_from_slice(&[0xFF; 8]);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_u64_to_u256_be_specific_value() {
        // 256 = 0x0100
        let result = u64_to_u256_be(256);
        let mut expected = [0u8; 32];
        expected[30] = 1;
        expected[31] = 0;
        assert_eq!(result, expected);
    }

    // ================================================================
    // Multiple Groups Tests
    // ================================================================

    #[test]
    fn test_multiple_groups_independent() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let g1 = BytesN::from_array(&env, &[1u8; 32]);
        let g2 = BytesN::from_array(&env, &[2u8; 32]);
        let c1 = BytesN::from_array(&env, &[0xA0; 32]);
        let c2 = BytesN::from_array(&env, &[0xB0; 32]);

        inject_group(&env, &contract_id, &g1, &c1, 0, 0);
        inject_group(&env, &contract_id, &g2, &c2, 10, 1);

        let s1 = client.get_state(&g1);
        let s2 = client.get_state(&g2);

        assert_eq!(s1.commitment, c1);
        assert_eq!(s1.epoch, 0);
        assert_eq!(s1.tier, 0);

        assert_eq!(s2.commitment, c2);
        assert_eq!(s2.epoch, 10);
        assert_eq!(s2.tier, 1);
    }

    #[test]
    fn test_mixed_active_inactive_groups() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let g1 = BytesN::from_array(&env, &[1u8; 32]);
        let g2 = BytesN::from_array(&env, &[2u8; 32]);
        let commitment = BytesN::from_array(&env, &[3u8; 32]);

        inject_group(&env, &contract_id, &g1, &commitment, 0, 0);
        inject_inactive_group(&env, &contract_id, &g2, &commitment, 5, 0);

        assert!(client.get_state(&g1).active);
        assert!(!client.get_state(&g2).active);
    }

    // ================================================================
    // Archive Window Tests
    // ================================================================

    #[test]
    fn test_archive_entry_via_injection_at_window_boundary() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);

        // Inject exactly HISTORY_WINDOW entries
        let mut history = Vec::new(&env);
        for i in 0..HISTORY_WINDOW as u64 {
            history.push_back(CommitmentEntry {
                commitment: commitment.clone(),
                epoch: i,
                timestamp: 1000 + i,
                tier: 0,
                active: true,
            });
        }
        inject_group_with_history(
            &env,
            &contract_id,
            &group_id,
            &commitment,
            HISTORY_WINDOW as u64,
            0,
            history,
        );

        let result = client.get_history(&group_id, &(HISTORY_WINDOW + 10));
        assert_eq!(result.len(), HISTORY_WINDOW);
    }

    // ================================================================
    // V2 Schema & Lazy Migration Tests (governance-types Phase 0)
    // ================================================================

    /// Inject a V2 group record directly (bypassing create_group_v2 which
    /// needs a real Groth16 proof).
    fn inject_group_v2(
        env: &Env,
        contract_id: &Address,
        group_id: &BytesN<32>,
        commitment: &BytesN<32>,
        epoch: u64,
        tier: u32,
        group_type: u32,
        member_count: u32,
    ) {
        env.as_contract(contract_id, || {
            let entry = CommitmentEntryV2 {
                commitment: commitment.clone(),
                epoch,
                timestamp: env.ledger().timestamp(),
                tier,
                active: true,
                group_type,
                member_count,
            };
            env.storage()
                .persistent()
                .set(&DataKey::GroupV2(group_id.clone()), &entry);
            env.storage().persistent().set(
                &DataKey::HistoryV2(group_id.clone()),
                &Vec::<CommitmentEntry>::new(env),
            );
            let count: u32 = env
                .storage()
                .instance()
                .get(&DataKey::GroupCount(tier))
                .unwrap_or(0);
            env.storage()
                .instance()
                .set(&DataKey::GroupCount(tier), &(count + 1));
        });
    }

    #[test]
    fn test_legacy_group_read_via_get_state_v2_defaults_to_anarchy() {
        // A pre-migration `Group` record is synthesised into a V2 view
        // with group_type = 0 and member_count = 0.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 7, 1);

        let state = client.get_state_v2(&group_id);
        assert_eq!(state.commitment, commitment);
        assert_eq!(state.epoch, 7);
        assert_eq!(state.tier, 1);
        assert!(state.active);
        assert_eq!(state.group_type, 0);
        assert_eq!(state.member_count, 0);
    }

    #[test]
    fn test_get_state_projects_v2_to_v1_shape() {
        // `get_state` must still return the legacy shape for a V2-native
        // record so pre-governance clients keep working.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[3u8; 32]);
        let commitment = BytesN::from_array(&env, &[4u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 2, 0, 42);

        let state_v1 = client.get_state(&group_id);
        assert_eq!(state_v1.commitment, commitment);
        assert_eq!(state_v1.epoch, 0);
        assert_eq!(state_v1.tier, 2);
        assert!(state_v1.active);

        let state_v2 = client.get_state_v2(&group_id);
        assert_eq!(state_v2.group_type, 0);
        assert_eq!(state_v2.member_count, 42);
    }

    #[test]
    fn test_v2_takes_precedence_when_both_entries_present() {
        // Concurrent keys should not leak a read: V2 wins unconditionally.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[5u8; 32]);
        let c_legacy = BytesN::from_array(&env, &[6u8; 32]);
        let c_v2 = BytesN::from_array(&env, &[7u8; 32]);

        // Inject a stale legacy entry first, then a fresh V2 entry for the
        // same group_id — simulates a mid-migration state.
        inject_group(&env, &contract_id, &group_id, &c_legacy, 3, 0);
        env.as_contract(&contract_id, || {
            let entry = CommitmentEntryV2 {
                commitment: c_v2.clone(),
                epoch: 9,
                timestamp: env.ledger().timestamp(),
                tier: 0,
                active: true,
                group_type: 0,
                member_count: 5,
            };
            env.storage()
                .persistent()
                .set(&DataKey::GroupV2(group_id.clone()), &entry);
        });

        let state = client.get_state_v2(&group_id);
        assert_eq!(state.commitment, c_v2);
        assert_eq!(state.epoch, 9);
        assert_eq!(state.member_count, 5);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #18)")]
    fn test_create_group_v2_rejects_unknown_group_type() {
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[8u8; 32]);
        let commitment = BytesN::from_array(&env, &[9u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        // group_type = 4 is beyond any defined value — must fail with
        // UnknownGroupType before any proof check runs.
        client.create_group_v2(
            &caller,
            &group_id,
            &commitment,
            &0u32,
            &4u32,
            &0u32,
            &mock_proof(&env),
            &pi,
        );
    }

    #[test]
    fn test_create_group_v2_accepts_democracy_type_at_validation() {
        // With Phase 3 enabled, group_type = 2 (Democracy) with a valid
        // member_count reaches the Groth16 verifier. The mock proof fails
        // there (host rejects zero G1 points), which proves dispatch
        // passed the governance check.
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[10u8; 32]);
        let commitment = BytesN::from_array(&env, &[11u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        let result = client.try_create_group_v2(
            &caller,
            &group_id,
            &commitment,
            &0u32,
            &2u32, // Democracy
            &10u32,
            &mock_proof(&env),
            &pi,
        );
        match result {
            Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {
                // reached Groth16 verifier — dispatch OK
            }
            other => panic!("expected verifier rejection, got {:?}", other),
        }
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #18)")]
    fn test_create_group_v2_rejects_oligarchy_type() {
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[12u8; 32]);
        let commitment = BytesN::from_array(&env, &[13u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_group_v2(
            &caller,
            &group_id,
            &commitment,
            &0u32,
            &3u32, // Oligarchy
            &0u32,
            &mock_proof(&env),
            &pi,
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #4)")]
    fn test_create_group_v2_rejects_duplicate_via_legacy() {
        // A group_id already occupied by a legacy V1 record must also
        // collide when create_group_v2 is invoked.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[14u8; 32]);
        let commitment = BytesN::from_array(&env, &[15u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 0, 0);

        let caller = Address::generate(&env);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_group_v2(
            &caller,
            &group_id,
            &commitment,
            &0u32,
            &0u32,
            &0u32,
            &mock_proof(&env),
            &pi,
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #4)")]
    fn test_create_group_v2_rejects_duplicate_via_v2() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[16u8; 32]);
        let commitment = BytesN::from_array(&env, &[17u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 0, 0);

        let caller = Address::generate(&env);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_group_v2(
            &caller,
            &group_id,
            &commitment,
            &0u32,
            &0u32,
            &0u32,
            &mock_proof(&env),
            &pi,
        );
    }

    #[test]
    fn test_get_history_for_legacy_only_group_uses_legacy_key() {
        // History must come through even when only the legacy keys exist.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[18u8; 32]);
        let commitment = BytesN::from_array(&env, &[19u8; 32]);

        let mut history = Vec::new(&env);
        history.push_back(CommitmentEntry {
            commitment: commitment.clone(),
            epoch: 0,
            timestamp: 1000,
            tier: 0,
            active: true,
        });
        inject_group_with_history(
            &env,
            &contract_id,
            &group_id,
            &commitment,
            1,
            0,
            history,
        );

        let result = client.get_history(&group_id, &10);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_bump_group_ttl_works_on_legacy_group() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[20u8; 32]);
        let commitment = BytesN::from_array(&env, &[21u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 0, 0);

        // Must not error even though only the legacy key is present.
        client.bump_group_ttl(&group_id);
    }

    #[test]
    fn test_bump_group_ttl_works_on_v2_group() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[22u8; 32]);
        let commitment = BytesN::from_array(&env, &[23u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 0, 0);

        client.bump_group_ttl(&group_id);
    }

    // ================================================================
    // 1v1 Group Type Tests (governance-types Phase 1)
    // ================================================================

    #[test]
    #[should_panic(expected = "Error(Contract, #23)")]
    fn test_create_1v1_rejects_non_small_tier() {
        // 1v1 groups MUST be Small (tier 0) — 32-leaf depth is the
        // smallest supported and matches the two-member invariant.
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[24u8; 32]);
        let commitment = BytesN::from_array(&env, &[25u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_group_v2(
            &caller,
            &group_id,
            &commitment,
            &1u32, // Medium — not allowed for 1v1
            &1u32, // 1v1
            &2u32,
            &mock_proof(&env),
            &pi,
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #23)")]
    fn test_create_1v1_rejects_large_tier() {
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[26u8; 32]);
        let commitment = BytesN::from_array(&env, &[27u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_group_v2(
            &caller,
            &group_id,
            &commitment,
            &2u32, // Large — not allowed for 1v1
            &1u32,
            &2u32,
            &mock_proof(&env),
            &pi,
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #10)")]
    fn test_create_1v1_rejects_wrong_member_count() {
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[28u8; 32]);
        let commitment = BytesN::from_array(&env, &[29u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        // 1v1 requires member_count == 2 — 3 is rejected pre-proof.
        client.create_group_v2(
            &caller,
            &group_id,
            &commitment,
            &0u32,
            &1u32,
            &3u32,
            &mock_proof(&env),
            &pi,
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #17)")]
    fn test_update_commitment_rejects_1v1_group() {
        // A 1v1 group is immutable after creation — update_commitment must
        // fail with OneOnOneImmutable regardless of proof validity.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[30u8; 32]);
        let commitment = BytesN::from_array(&env, &[31u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 1, 2);

        let upi = UpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 0,
            c_new: BytesN::from_array(&env, &[0u8; 32]),
        };
        client.update_commitment(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    fn test_get_state_v2_for_1v1_reports_type_and_count() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[32u8; 32]);
        let commitment = BytesN::from_array(&env, &[33u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 1, 2);

        let state = client.get_state_v2(&group_id);
        assert_eq!(state.group_type, 1);
        assert_eq!(state.member_count, 2);
        assert_eq!(state.tier, 0);
        assert!(state.active);
    }

    // ================================================================
    // Oligarchy Group Type Tests (governance-types Phase 2)
    // ================================================================

    /// Inject an Oligarchy V2 group plus its admin-tree root directly
    /// (bypasses the Groth16 check required by create_oligarchy_group).
    fn inject_oligarchy_group(
        env: &Env,
        contract_id: &Address,
        group_id: &BytesN<32>,
        commitment: &BytesN<32>,
        admin_root: &BytesN<32>,
        epoch: u64,
        tier: u32,
        member_count: u32,
    ) {
        inject_group_v2(
            env,
            contract_id,
            group_id,
            commitment,
            epoch,
            tier,
            3, // Oligarchy
            member_count,
        );
        env.as_contract(contract_id, || {
            env.storage()
                .persistent()
                .set(&DataKey::AdminSet(group_id.clone()), admin_root);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #8)")]
    fn test_create_oligarchy_rejects_invalid_tier() {
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[34u8; 32]);
        let commitment = BytesN::from_array(&env, &[35u8; 32]);
        let admin_root = BytesN::from_array(&env, &[0u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_oligarchy_group(
            &caller,
            &group_id,
            &commitment,
            &3u32, // invalid tier
            &5u32,
            &admin_root,
            &mock_proof(&env),
            &pi,
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #15)")]
    fn test_create_oligarchy_rejects_non_canonical_admin_root() {
        // All-ones (0xff * 32) is >= BLS12-381 Fr modulus, so it must
        // be rejected with InvalidCommitmentEncoding before proof check.
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[36u8; 32]);
        let commitment = BytesN::from_array(&env, &[37u8; 32]);
        let bad_admin_root = BytesN::from_array(&env, &[0xffu8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_oligarchy_group(
            &caller,
            &group_id,
            &commitment,
            &0u32,
            &5u32,
            &bad_admin_root,
            &mock_proof(&env),
            &pi,
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #4)")]
    fn test_create_oligarchy_rejects_duplicate_group_id() {
        // A group_id already present as a V2 record must collide.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[38u8; 32]);
        let commitment = BytesN::from_array(&env, &[39u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 0, 0);

        let caller = Address::generate(&env);
        let admin_root = BytesN::from_array(&env, &[0u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_oligarchy_group(
            &caller,
            &group_id,
            &commitment,
            &0u32,
            &5u32,
            &admin_root,
            &mock_proof(&env),
            &pi,
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #19)")]
    fn test_get_admin_root_for_non_oligarchy_group_missing() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[40u8; 32]);
        let commitment = BytesN::from_array(&env, &[41u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 0, 0);

        client.get_admin_root(&group_id);
    }

    #[test]
    fn test_get_admin_root_returns_stored_value() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[42u8; 32]);
        let commitment = BytesN::from_array(&env, &[43u8; 32]);
        let admin_root = BytesN::from_array(&env, &[44u8; 32]);
        inject_oligarchy_group(
            &env,
            &contract_id,
            &group_id,
            &commitment,
            &admin_root,
            0,
            1,
            10,
        );

        let stored = client.get_admin_root(&group_id);
        assert_eq!(stored, admin_root);

        // And the V2 record reports group_type = 3.
        let state = client.get_state_v2(&group_id);
        assert_eq!(state.group_type, 3);
        assert_eq!(state.member_count, 10);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #18)")]
    fn test_update_commitment_rejects_oligarchy_pending_ceremony() {
        // Oligarchy update routing needs the AdminUpdate / OligarchyUpdate
        // VKs, which ship with the trusted setup ceremony. Until then,
        // update_commitment must reject group_type = 3 with UnknownGroupType.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[45u8; 32]);
        let commitment = BytesN::from_array(&env, &[46u8; 32]);
        let admin_root = BytesN::from_array(&env, &[47u8; 32]);
        inject_oligarchy_group(
            &env,
            &contract_id,
            &group_id,
            &commitment,
            &admin_root,
            0,
            0,
            4,
        );

        let upi = UpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 0,
            c_new: BytesN::from_array(&env, &[0u8; 32]),
        };
        client.update_commitment(&group_id, &mock_proof(&env), &upi);
    }

    // ================================================================
    // Democracy Group Type Tests (governance-types Phase 3)
    // ================================================================

    // Note: `member_count ∈ {0, 1}` is accepted at creation for Democracy
    // (and Oligarchy) so clients can start solo and grow via off-chain MLS
    // joins. The `m_new < 2` invariant is enforced later by
    // `democracy_update_commitment` once quorum actually matters.
    //
    // The two tests below replace the deleted
    // `test_create_democracy_rejects_member_count_{zero,one}` cases
    // (PR #84 relaxed the floor — see review on pullrequestreview-4136498592).
    // They lock in the relaxation by asserting the validation path now
    // *accepts* 0/1 and defers to the Groth16 verifier; the verifier
    // abort (`Err(Err(_))`) is the signal that every validation check
    // passed.

    #[test]
    fn test_create_democracy_accepts_member_count_zero() {
        // Bootstrap case: a group creator publishing on-chain before any
        // member has joined off-chain. Must no longer be rejected at
        // create time — quorum binding is a later-epoch concern.
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[60u8; 32]);
        let commitment = BytesN::from_array(&env, &[61u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        let result = client.try_create_group_v2(
            &caller,
            &group_id,
            &commitment,
            &0u32, // Small tier
            &2u32, // Democracy
            &0u32, // member_count = 0 — previously rejected, now accepted
            &mock_proof(&env),
            &pi,
        );
        match result {
            Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {
                // reached Groth16 verifier — validation passed
            }
            other => panic!("expected verifier rejection, got {:?}", other),
        }
    }

    #[test]
    fn test_create_democracy_accepts_member_count_one() {
        // Solo-creator case: the caller is the sole initial member and
        // will grow the group via invites. Must reach the verifier.
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[62u8; 32]);
        let commitment = BytesN::from_array(&env, &[63u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        let result = client.try_create_group_v2(
            &caller,
            &group_id,
            &commitment,
            &0u32,
            &2u32, // Democracy
            &1u32, // member_count = 1 — previously rejected, now accepted
            &mock_proof(&env),
            &pi,
        );
        match result {
            Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {
                // reached Groth16 verifier — validation passed
            }
            other => panic!("expected verifier rejection, got {:?}", other),
        }
    }

    #[test]
    fn test_create_oligarchy_accepts_member_count_zero() {
        // Oligarchy mirrors Democracy: `member_count ∈ {0, 1}` is accepted
        // at creation (bootstrap / solo-admin case). The admin tree is
        // seeded independently, so zero group members is still meaningful.
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[72u8; 32]);
        let commitment = BytesN::from_array(&env, &[73u8; 32]);
        // Any canonical Fr encoding works as an admin_root for this test.
        let admin_root = BytesN::from_array(&env, &[0u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        let result = client.try_create_oligarchy_group(
            &caller,
            &group_id,
            &commitment,
            &0u32,
            &0u32, // member_count = 0
            &admin_root,
            &mock_proof(&env),
            &pi,
        );
        match result {
            Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {
                // reached Groth16 verifier — validation passed
            }
            other => panic!("expected verifier rejection, got {:?}", other),
        }
    }

    #[test]
    fn test_create_oligarchy_accepts_member_count_one() {
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[74u8; 32]);
        let commitment = BytesN::from_array(&env, &[75u8; 32]);
        let admin_root = BytesN::from_array(&env, &[0u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        let result = client.try_create_oligarchy_group(
            &caller,
            &group_id,
            &commitment,
            &0u32,
            &1u32, // member_count = 1
            &admin_root,
            &mock_proof(&env),
            &pi,
        );
        match result {
            Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {
                // reached Groth16 verifier — validation passed
            }
            other => panic!("expected verifier rejection, got {:?}", other),
        }
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #25)")]
    fn test_create_democracy_rejects_member_count_exceeds_small_capacity() {
        // Small tier holds 32 leaves; 33 members overflows the Merkle tree.
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[64u8; 32]);
        let commitment = BytesN::from_array(&env, &[65u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_group_v2(
            &caller,
            &group_id,
            &commitment,
            &0u32,   // Small tier (cap = 32)
            &2u32,
            &33u32, // exceeds capacity
            &mock_proof(&env),
            &pi,
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #25)")]
    fn test_create_democracy_rejects_member_count_exceeds_large_capacity() {
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[66u8; 32]);
        let commitment = BytesN::from_array(&env, &[67u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_group_v2(
            &caller,
            &group_id,
            &commitment,
            &2u32,      // Large tier (cap = 2048)
            &2u32,
            &2049u32, // exceeds capacity
            &mock_proof(&env),
            &pi,
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #18)")]
    fn test_update_commitment_rejects_democracy_pending_ceremony() {
        // Democracy quorum enforcement lives in `DemocracyUpdateCircuit`,
        // which is ceremony-gated. Until the VK ships, update_commitment
        // must reject group_type = 2 with UnknownGroupType.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[68u8; 32]);
        let commitment = BytesN::from_array(&env, &[69u8; 32]);
        inject_group_v2(
            &env,
            &contract_id,
            &group_id,
            &commitment,
            0,
            0,
            2, // Democracy
            5,
        );

        let upi = UpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 0,
            c_new: BytesN::from_array(&env, &[0u8; 32]),
        };
        client.update_commitment(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    fn test_get_state_v2_for_democracy_reports_member_count() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[70u8; 32]);
        let commitment = BytesN::from_array(&env, &[71u8; 32]);
        inject_group_v2(
            &env,
            &contract_id,
            &group_id,
            &commitment,
            3,
            1,
            2,   // Democracy
            17,
        );

        let state = client.get_state_v2(&group_id);
        assert_eq!(state.group_type, 2);
        assert_eq!(state.member_count, 17);
        assert_eq!(state.epoch, 3);
    }

    #[test]
    fn test_tier_capacity_constants() {
        // The Merkle-tree leaf bounds are a consensus-critical invariant
        // of the governance scheme — lock them in.
        assert_eq!(tier_capacity(0), 32);
        assert_eq!(tier_capacity(1), 256);
        assert_eq!(tier_capacity(2), 2048);
    }

    // ================================================================
    // Democracy `update_commitment_democracy` dispatcher tests
    //
    // These exercise the pre-verification rejection paths. The positive
    // path (a real Groth16 proof verifying against a real Democracy VK)
    // is covered by integration tests once the dev ceremony lands
    // (see `docs/democracy-circuit-ceremony.md §3`).
    // ================================================================

    fn mock_democracy_vk(env: &Env) -> VerificationKeyData {
        // 6 IC points (1 base + 5 public inputs) to satisfy the set-time
        // length check. Will never verify a real proof — used for
        // reject-path tests only.
        let g1 = BytesN::from_array(env, &[0u8; 96]);
        let g2 = BytesN::from_array(env, &[0u8; 192]);
        VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![env, g1.clone(), g1.clone(), g1.clone(), g1.clone(), g1.clone(), g1],
        }
    }

    fn install_democracy_vk(
        env: &Env,
        contract_id: &Address,
        tier: u32,
    ) {
        env.as_contract(contract_id, || {
            env.storage()
                .persistent()
                .set(&DataKey::UpdateVKByType(tier, 2), &mock_democracy_vk(env));
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #9)")]
    fn test_update_vk_democracy_rejects_wrong_ic_length() {
        let (env, client, _admin, _cid) = setup_initialized();
        let g1 = BytesN::from_array(&env, &[0u8; 96]);
        let g2 = BytesN::from_array(&env, &[0u8; 192]);
        // Democracy VKs require 6 IC points; 5 must be rejected.
        let bad_vk = VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![&env, g1.clone(), g1.clone(), g1.clone(), g1.clone(), g1],
        };
        client.update_vk(&VkKind::UpdateByType(2u32), &0u32, &bad_vk);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #16)")]
    fn test_update_vk_rejects_unknown_group_type_for_update_by_type() {
        // group_type = 4 has no defined update VK shape.
        let (env, client, _admin, _cid) = setup_initialized();
        let vk = mock_democracy_vk(&env);
        client.update_vk(&VkKind::UpdateByType(4u32), &0u32, &vk);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #18)")]
    fn test_update_commitment_democracy_rejects_non_democracy_group() {
        // Anarchy group sent through the Democracy dispatcher must reject.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[80u8; 32]);
        let commitment = BytesN::from_array(&env, &[81u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 0, 0);

        let upi = DemocracyUpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 0,
            c_new: BytesN::from_array(&env, &[0u8; 32]),
            member_count_old: 0,
            member_count_new: 0,
        };
        client.update_commitment_democracy(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #24)")]
    fn test_update_commitment_democracy_rejects_member_count_mismatch() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[82u8; 32]);
        let commitment = BytesN::from_array(&env, &[83u8; 32]);
        // Stored member_count = 5; caller claims 4.
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 2, 5);

        let upi = DemocracyUpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 0,
            c_new: BytesN::from_array(&env, &[0u8; 32]),
            member_count_old: 4, // wrong
            member_count_new: 5,
        };
        client.update_commitment_democracy(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #25)")]
    fn test_update_commitment_democracy_rejects_delta_too_large() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[84u8; 32]);
        let commitment = BytesN::from_array(&env, &[85u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 2, 5);

        let upi = DemocracyUpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 0,
            c_new: BytesN::from_array(&env, &[0u8; 32]),
            member_count_old: 5,
            member_count_new: 7, // |Δ| = 2 — out of {-1, 0, +1}
        };
        client.update_commitment_democracy(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #25)")]
    fn test_update_commitment_democracy_rejects_drop_below_quorum_floor() {
        // A Democracy with 2 members removing one collapses to "democracy
        // of one" — rejected.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[86u8; 32]);
        let commitment = BytesN::from_array(&env, &[87u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 2, 2);

        let upi = DemocracyUpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 0,
            c_new: BytesN::from_array(&env, &[0u8; 32]),
            member_count_old: 2,
            member_count_new: 1, // below democracy-of-≥2 floor
        };
        client.update_commitment_democracy(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1)")]
    fn test_update_commitment_democracy_rejects_when_vk_missing() {
        // Valid-looking Democracy call, but no Democracy VK has been
        // installed for this tier yet. Must fail with NotInitialized.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[88u8; 32]);
        let commitment = BytesN::from_array(&env, &[89u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 2, 4);

        let c_new = Fr::from_bytes(BytesN::from_array(&env, &[0x01; 32])).to_bytes();
        let upi = DemocracyUpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 0,
            c_new,
            member_count_old: 4,
            member_count_new: 4,
        };
        client.update_commitment_democracy(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #10)")]
    fn test_update_commitment_democracy_rejects_wrong_c_old() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[90u8; 32]);
        let commitment = BytesN::from_array(&env, &[91u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 2, 4);
        install_democracy_vk(&env, &contract_id, 0);

        let upi = DemocracyUpdatePublicInputs {
            c_old: BytesN::from_array(&env, &[0xFF; 32]), // not the stored one
            epoch_old: 0,
            c_new: BytesN::from_array(&env, &[0u8; 32]),
            member_count_old: 4,
            member_count_new: 4,
        };
        client.update_commitment_democracy(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    fn test_deactivate_group_dispatches_past_oligarchy_check() {
        // Deactivate is the safety valve — works for every governance
        // type including Oligarchy (a member proof is sufficient).
        // Proves this by checking that the call gets *past* the
        // group-type dispatch: the mock proof fails in the Groth16
        // verifier (which host-panics on zero curve points in our test
        // harness). The fact we reach the verifier at all means the
        // governance check let Oligarchy through.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[48u8; 32]);
        let commitment = BytesN::from_array(&env, &[49u8; 32]);
        let admin_root = BytesN::from_array(&env, &[50u8; 32]);
        inject_oligarchy_group(
            &env,
            &contract_id,
            &group_id,
            &commitment,
            &admin_root,
            0,
            0,
            3,
        );

        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        let result = client.try_deactivate_group(&group_id, &mock_proof(&env), &pi);
        // We expect some form of verifier rejection (host trap OR a clean
        // `InvalidProof` after point-validation/pairing_check), not a
        // governance-level rejection (which would surface a different code).
        match result {
            Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {
                // reached Groth16 verifier — governance dispatch OK
            }
            other => panic!("expected verifier rejection, got {:?}", other),
        }
    }

    // ================================================================
    // Oligarchy AdminUpdate dispatcher tests (§6.4.4)
    //
    // Admin rotation reuses the UpdateCircuit shape against the admin
    // tree. Positive-path proof verification requires a real VK from
    // the dev ceremony; these are reject-path tests exercising the
    // contract-side checks only.
    // ================================================================

    fn mock_admin_update_vk(env: &Env) -> VerificationKeyData {
        // 4 IC points (1 base + 3 public inputs) to satisfy the set-time
        // length check. Uses valid subgroup points so it passes the
        // install-time structural check added in the 2026-04 audit
        // (MEDIUM-1 / LOW-3). Will never verify a real proof.
        VerificationKeyData {
            alpha_g1: valid_g1(env, b"a-alpha"),
            beta_g2: valid_g2(env, b"a-beta"),
            gamma_g2: valid_g2(env, b"a-gamma"),
            delta_g2: valid_g2(env, b"a-delta"),
            ic: vec![
                env,
                valid_g1(env, b"a-ic0"),
                valid_g1(env, b"a-ic1"),
                valid_g1(env, b"a-ic2"),
                valid_g1(env, b"a-ic3"),
            ],
        }
    }

    fn install_admin_update_vk(env: &Env, contract_id: &Address) {
        env.as_contract(contract_id, || {
            env.storage()
                .persistent()
                .set(&DataKey::AdminUpdateVK, &mock_admin_update_vk(env));
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #9)")]
    fn test_update_vk_admin_update_rejects_wrong_ic_length() {
        let (env, client, _admin, _cid) = setup_initialized();
        let g1 = BytesN::from_array(&env, &[0u8; 96]);
        let g2 = BytesN::from_array(&env, &[0u8; 192]);
        // AdminUpdate VKs require 4 IC points; 3 must be rejected.
        let bad_vk = VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![&env, g1.clone(), g1.clone(), g1],
        };
        client.update_vk(&VkKind::AdminUpdate, &0u32, &bad_vk);
    }

    #[test]
    fn test_update_vk_admin_update_ignores_tier() {
        // AdminUpdate is single-tier per §6.4.4 — the `tier` arg is a
        // no-op, and values outside 0..=2 must NOT trigger InvalidTier.
        let (env, client, _admin, _cid) = setup_initialized();
        let vk = mock_admin_update_vk(&env);
        // tier = 7 would fail InvalidTier for Membership/Update; must
        // succeed here and land under the single DataKey::AdminUpdateVK.
        client.update_vk(&VkKind::AdminUpdate, &7u32, &vk);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #18)")]
    fn test_update_admin_commitment_rejects_non_oligarchy_group() {
        // Anarchy group cannot go through the admin dispatcher.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[92u8; 32]);
        let commitment = BytesN::from_array(&env, &[93u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 0, 0);

        let upi = AdminUpdatePublicInputs {
            admin_c_old: BytesN::from_array(&env, &[0u8; 32]),
            admin_epoch_old: 0,
            admin_c_new: BytesN::from_array(&env, &[0u8; 32]),
        };
        client.update_admin_commitment(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #19)")]
    fn test_update_admin_commitment_rejects_missing_admin_root() {
        // Oligarchy metadata set, but the AdminSet slot is missing (can
        // only happen from a directly-poked group entry — real create
        // flow always populates it). Must fail with MissingAdminRoot.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[94u8; 32]);
        let commitment = BytesN::from_array(&env, &[95u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 3, 5);
        // Intentionally do NOT seed AdminSet.

        let upi = AdminUpdatePublicInputs {
            admin_c_old: BytesN::from_array(&env, &[0u8; 32]),
            admin_epoch_old: 0,
            admin_c_new: BytesN::from_array(&env, &[0u8; 32]),
        };
        client.update_admin_commitment(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #20)")]
    fn test_update_admin_commitment_rejects_wrong_admin_c_old() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[96u8; 32]);
        let commitment = BytesN::from_array(&env, &[97u8; 32]);
        let stored_admin = BytesN::from_array(&env, &[11u8; 32]);
        inject_oligarchy_group(
            &env, &contract_id, &group_id, &commitment, &stored_admin, 0, 0, 5,
        );

        let upi = AdminUpdatePublicInputs {
            admin_c_old: BytesN::from_array(&env, &[0xAB; 32]), // not stored
            admin_epoch_old: 0,
            admin_c_new: BytesN::from_array(&env, &[0u8; 32]),
        };
        client.update_admin_commitment(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #10)")]
    fn test_update_admin_commitment_rejects_wrong_admin_epoch_old() {
        // Admin epoch defaults to 0 (no slot written by inject). Caller
        // claiming epoch_old=1 must be rejected as PublicInputsMismatch.
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[98u8; 32]);
        let commitment = BytesN::from_array(&env, &[99u8; 32]);
        let stored_admin = BytesN::from_array(&env, &[12u8; 32]);
        inject_oligarchy_group(
            &env, &contract_id, &group_id, &commitment, &stored_admin, 0, 0, 5,
        );

        let upi = AdminUpdatePublicInputs {
            admin_c_old: stored_admin,
            admin_epoch_old: 1, // wrong — stored is 0 (default)
            admin_c_new: BytesN::from_array(&env, &[0u8; 32]),
        };
        client.update_admin_commitment(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1)")]
    fn test_update_admin_commitment_rejects_when_vk_missing() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[100u8; 32]);
        let commitment = BytesN::from_array(&env, &[101u8; 32]);
        let stored_admin = BytesN::from_array(&env, &[13u8; 32]);
        inject_oligarchy_group(
            &env, &contract_id, &group_id, &commitment, &stored_admin, 0, 0, 5,
        );

        let c_new = Fr::from_bytes(BytesN::from_array(&env, &[0x01; 32])).to_bytes();
        let upi = AdminUpdatePublicInputs {
            admin_c_old: stored_admin,
            admin_epoch_old: 0,
            admin_c_new: c_new,
        };
        client.update_admin_commitment(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #15)")]
    fn test_update_admin_commitment_rejects_non_canonical_c_new() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[102u8; 32]);
        let commitment = BytesN::from_array(&env, &[103u8; 32]);
        let stored_admin = BytesN::from_array(&env, &[14u8; 32]);
        inject_oligarchy_group(
            &env, &contract_id, &group_id, &commitment, &stored_admin, 0, 0, 5,
        );
        install_admin_update_vk(&env, &contract_id);

        // 0xFF..FF is non-canonical (> Fr modulus).
        let upi = AdminUpdatePublicInputs {
            admin_c_old: stored_admin,
            admin_epoch_old: 0,
            admin_c_new: BytesN::from_array(&env, &[0xFF; 32]),
        };
        client.update_admin_commitment(&group_id, &mock_proof(&env), &upi);
    }

    #[test]
    fn test_get_admin_epoch_for_fresh_oligarchy_is_zero() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[104u8; 32]);
        let commitment = BytesN::from_array(&env, &[105u8; 32]);
        let stored_admin = BytesN::from_array(&env, &[15u8; 32]);
        inject_oligarchy_group(
            &env, &contract_id, &group_id, &commitment, &stored_admin, 0, 0, 5,
        );
        let epoch = client.get_admin_epoch(&group_id);
        assert_eq!(epoch, 0);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #19)")]
    fn test_get_admin_epoch_rejects_non_oligarchy() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[106u8; 32]);
        let commitment = BytesN::from_array(&env, &[107u8; 32]);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 0, 0);
        client.get_admin_epoch(&group_id);
    }

    // ================================================================
    // Audit-2026-04 — new test coverage
    // ================================================================

    // MEDIUM-1 + LOW-3 test coverage notes:
    //
    // `validate_vk_points` / `validate_proof_points` invoke
    // `G1Affine::is_in_subgroup()` / `G2Affine::is_in_subgroup()`. Those
    // helpers run the host's on-curve check FIRST — so they raise two
    // distinct failure modes depending on the attacker's input:
    //
    //   * Off-curve bytes (including all-zero encodings): the host traps
    //     with `Error(Crypto, InvalidInput)` ("point not on curve") before
    //     the subgroup check runs. This surfaces as a host-level panic in
    //     the try_* envelope — still a rejection, but not via our typed
    //     `Error::InvalidPoint`.
    //   * On-curve-but-not-in-subgroup points: `is_in_subgroup()` returns
    //     false, and our helpers convert that into `Error::InvalidPoint`
    //     (for VKs) or a `false` return / `Error::InvalidProof` (for
    //     proofs). This is the actual subgroup-attack defense.
    //
    // Exercising the second path cleanly requires hardcoded witness points
    // (on the full curve but outside the prime-order subgroup), which
    // we do not carry in-tree. The tests below therefore use off-curve
    // zero-byte encodings and assert that SOME rejection path fires —
    // which at minimum proves the validator is reachable and not silently
    // skipped. The typed `Error::InvalidPoint` variant remains defined
    // and wired for the subgroup-attack path even when untested.

    /// MEDIUM-1 + LOW-3: `initialize` rejects a VK with malformed / off-
    /// curve points. See module-level comment above on the two rejection
    /// paths.
    #[test]
    #[should_panic(expected = "Error(")]
    fn test_initialize_rejects_invalid_vk_point() {
        let _ = setup_env_bad_membership_vk(|env| {
            let bad_g1 = BytesN::from_array(env, &[0u8; 96]);
            VerificationKeyData {
                alpha_g1: bad_g1.clone(),
                beta_g2: valid_g2(env, b"beta"),
                gamma_g2: valid_g2(env, b"gamma"),
                delta_g2: valid_g2(env, b"delta"),
                ic: vec![
                    env,
                    valid_g1(env, b"ic0"),
                    valid_g1(env, b"ic1"),
                    valid_g1(env, b"ic2"),
                ],
            }
        });
    }

    /// MEDIUM-1 + LOW-3: `update_vk` rejects a replacement VK with
    /// malformed / off-curve points.
    #[test]
    #[should_panic(expected = "Error(")]
    fn test_update_vk_rejects_invalid_point() {
        let (env, client, _admin, _cid) = setup_initialized();
        let bad_g2 = BytesN::from_array(&env, &[0u8; 192]);
        let bad_vk = VerificationKeyData {
            alpha_g1: valid_g1(&env, b"alpha"),
            beta_g2: bad_g2,
            gamma_g2: valid_g2(&env, b"gamma"),
            delta_g2: valid_g2(&env, b"delta"),
            ic: vec![
                &env,
                valid_g1(&env, b"ic0"),
                valid_g1(&env, b"ic1"),
                valid_g1(&env, b"ic2"),
            ],
        };
        client.update_vk(&VkKind::Membership, &0u32, &bad_vk);
    }

    /// MEDIUM-1: a proof with a malformed / off-curve G1 point is rejected
    /// before reaching `pairing_check`.
    #[test]
    #[should_panic(expected = "Error(")]
    fn test_verify_rejects_invalid_proof_point() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = c_new_ok(&env);
        inject_group_v2(&env, &contract_id, &group_id, &commitment, 0, 0, 0, 0);

        let bad_proof = Groth16Proof {
            a: BytesN::from_array(&env, &[0u8; 96]),
            b: valid_g2(&env, b"b"),
            c: valid_g1(&env, b"c"),
        };
        let upi = UpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 0,
            c_new: c_new_ok(&env),
        };
        client.update_commitment(&group_id, &bad_proof, &upi);
    }

    /// MEDIUM-2: `create_group_v2` rejects a non-canonical commitment up
    /// front, with `InvalidCommitmentEncoding` — not a downstream
    /// `InvalidProof`.
    #[test]
    #[should_panic(expected = "Error(Contract, #15)")]
    fn test_create_group_rejects_non_canonical_commitment() {
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        // All-0xFF is >= BLS12-381 Fr modulus — not canonical.
        let non_canonical = BytesN::from_array(&env, &[0xFFu8; 32]);
        let pi = PublicInputs {
            commitment: non_canonical.clone(),
            epoch: 0,
        };
        client.create_group(&caller, &group_id, &non_canonical, &0u32, &mock_proof(&env), &pi);
    }

    /// Post-audit Finding #1 (partial contract-level fix): a nullifier
    /// burned on any operation against a group blocks any subsequent
    /// operation on that group with the same proof. No per-op scoping.
    #[test]
    #[should_panic(expected = "Error(Contract, #12)")]
    fn test_replay_within_same_group_blocked() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 5, 0);

        let proof = mock_proof(&env);
        inject_used_proof(&env, &contract_id, &group_id, &proof);

        let upi = UpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 5,
            c_new: c_new_ok(&env),
        };
        client.update_commitment(&group_id, &proof, &upi);
    }

    /// Finding #1 (post-fix): the nullifier is GLOBAL. A proof burned on
    /// any group blocks re-submission of the same proof bytes on any
    /// other group. This defeats the "clone a target group at the same
    /// (commitment, epoch) and replay the observed proof" attack that
    /// the earlier group-scoped preimage allowed.
    #[test]
    #[should_panic(expected = "Error(Contract, #12)")]
    fn test_replay_nullifier_blocks_cross_group() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_a = BytesN::from_array(&env, &[1u8; 32]);
        let group_b = BytesN::from_array(&env, &[2u8; 32]);
        let commitment = BytesN::from_array(&env, &[3u8; 32]);
        inject_group(&env, &contract_id, &group_b, &commitment, 5, 0);

        let proof = mock_proof(&env);
        // Burn the nullifier as if the proof had already been spent on
        // group A. With a global nullifier it should also block group B.
        inject_used_proof(&env, &contract_id, &group_a, &proof);

        let upi = UpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 5,
            c_new: c_new_ok(&env),
        };
        client.update_commitment(&group_b, &proof, &upi);
    }

    /// Post-audit Finding #1: a nullifier burned on one operation on a
    /// group (e.g. a create-time membership proof) MUST block a
    /// different operation on the same group (e.g. deactivate) when the
    /// same proof bytes are submitted. This is the cross-entrypoint
    /// replay that the 2026-04 op-tag-scoped scheme regressed.
    #[test]
    #[should_panic(expected = "Error(Contract, #12)")]
    fn test_replay_across_ops_same_group_blocked() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 5, 0);

        let proof = mock_proof(&env);
        // Burn the nullifier once (as if deactivate had already run).
        inject_used_proof(&env, &contract_id, &group_id, &proof);

        // An update attempt on the same group with the same proof must
        // be rejected as a replay.
        let upi = UpdatePublicInputs {
            c_old: commitment.clone(),
            epoch_old: 5,
            c_new: c_new_ok(&env),
        };
        client.update_commitment(&group_id, &proof, &upi);
    }

    // ───────────────────────────────────────────────────────────────────
    // Audit-followup Finding #5 — `__constructor` atomic deploy+init
    // ───────────────────────────────────────────────────────────────────

    /// Deploying with constructor args initialises the contract atomically;
    /// a subsequent `initialize` call is rejected with `AlreadyInitialized`.
    #[test]
    fn test_constructor_initializes_atomically() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let vk = mock_vk(&env);
        let uvk = mock_update_vk(&env);

        let contract_id = env.register(
            SepXxxxContract,
            (
                admin.clone(),
                vk.clone(),
                vk.clone(),
                vk.clone(),
                uvk.clone(),
                uvk.clone(),
                uvk.clone(),
            ),
        );
        let client = SepXxxxContractClient::new(&env, &contract_id);

        // Constructor should have written admin + VKs. A post-deploy
        // initialize call MUST be rejected.
        let res = client.try_initialize(&admin, &vk, &vk, &vk, &uvk, &uvk, &uvk);
        assert!(matches!(res, Err(Ok(Error::AlreadyInitialized))));

        // And the happy-path post-init operations still work — prove the
        // admin slot is populated by exercising an admin-gated entrypoint.
        client.set_restricted_mode(&true);
    }

    // ───────────────────────────────────────────────────────────────────
    // Audit-followup Finding #6 — permissionless `reconcile_tier_count`
    // ───────────────────────────────────────────────────────────────────

    /// With the group's persistent state absent and a `GroupCounted`
    /// receipt present, `reconcile_tier_count` decrements the per-tier
    /// counter by one and removes the receipt so a second call fails.
    #[test]
    fn test_reconcile_tier_count_decrements_when_group_gone() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let tier = 1u32;
        let group_id = BytesN::from_array(&env, &[42u8; 32]);

        // Simulate a create-then-expire history: no `Group` / `GroupV2`
        // entry survives, but the tier counter is still +1 and the
        // reconciliation receipt is still in persistent storage
        // (re-audit Finding #3: receipt has its own longer TTL so it
        // outlives the group entry).
        env.as_contract(&contract_id, || {
            env.storage()
                .instance()
                .set(&DataKey::GroupCount(tier), &3u32);
            env.storage()
                .persistent()
                .set(&DataKey::GroupCounted(group_id.clone()), &tier);
        });

        client.reconcile_tier_count(&group_id);

        env.as_contract(&contract_id, || {
            let count: u32 = env
                .storage()
                .instance()
                .get(&DataKey::GroupCount(tier))
                .unwrap();
            assert_eq!(count, 2);
            assert!(
                !env.storage()
                    .persistent()
                    .has(&DataKey::GroupCounted(group_id.clone())),
                "receipt must be removed so we cannot double-decrement"
            );
        });
    }

    /// Calling `reconcile_tier_count` on an id that still has live
    /// persistent state MUST fail with `GroupStillActive`.
    #[test]
    #[should_panic(expected = "Error(Contract, #27)")]
    fn test_reconcile_tier_count_rejects_live_group() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[9u8; 32]);
        let commitment = BytesN::from_array(&env, &[8u8; 32]);
        inject_group(&env, &contract_id, &group_id, &commitment, 0, 0);

        // Write the receipt — does not matter that `inject_group` didn't —
        // so `reconcile_tier_count` reaches the liveness check.
        env.as_contract(&contract_id, || {
            env.storage()
                .persistent()
                .set(&DataKey::GroupCounted(group_id.clone()), &0u32);
        });

        client.reconcile_tier_count(&group_id);
    }

    /// Calling `reconcile_tier_count` without any reconciliation receipt
    /// fails with `GroupNotFound`. This prevents a caller from forcing a
    /// decrement against an arbitrary id that was never counted.
    #[test]
    #[should_panic(expected = "Error(Contract, #5)")]
    fn test_reconcile_tier_count_rejects_missing_receipt() {
        let (env, client, _admin, _contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[7u8; 32]);
        client.reconcile_tier_count(&group_id);
    }

    /// A normal `deactivate_group` flow MUST clear the receipt, so a
    /// subsequent `reconcile_tier_count` finds nothing to reconcile (and
    /// therefore cannot double-decrement).
    #[test]
    #[should_panic(expected = "Error(Contract, #5)")]
    fn test_reconcile_no_op_after_clean_deactivation() {
        let (env, client, _admin, contract_id) = setup_initialized();
        let group_id = BytesN::from_array(&env, &[11u8; 32]);
        let commitment = BytesN::from_array(&env, &[12u8; 32]);

        // Simulate a freshly-deactivated group: persistent state cleared,
        // counter holds zero, receipt was removed by `deactivate_group`.
        env.as_contract(&contract_id, || {
            env.storage()
                .instance()
                .set(&DataKey::GroupCount(0u32), &0u32);
        });
        // (no `GroupCounted` entry, no `GroupV2`/`Group` entry)
        let _ = commitment;

        client.reconcile_tier_count(&group_id);
    }

    // ───────────────────────────────────────────────────────────────────
    // Audit-followup Finding #4 — `bump_group` covers admin keys;
    // `bump_vks` covers shared keys.
    // ───────────────────────────────────────────────────────────────────

    /// `bump_vks` is callable by any account — liveness is not admin-gated.
    #[test]
    fn test_bump_vks_callable_by_anyone() {
        let (_env, client, _admin, _contract_id) = setup_initialized();
        client.bump_vks();
    }
}
