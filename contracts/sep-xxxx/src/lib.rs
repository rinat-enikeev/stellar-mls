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

/// Maximum number of active groups allowed per tier (M-4: storage abuse prevention).
/// The admin can increase this limit by re-deploying with a higher value.
const MAX_GROUPS_PER_TIER: u32 = 10_000;

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
    // Codes 19–22 reserved for Phase 2 (Oligarchy).
    /// 1v1 groups MUST use tier 0 (Small). Any other tier is rejected.
    Invalid1v1Tier = 23,
    // Codes 24–25 reserved for Phase 3 (Democracy).
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

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupDeactivated {
    #[topic]
    pub group_id: BytesN<32>,
    pub final_epoch: u64,
    pub timestamp: u64,
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

/// Selector for which verification-key family is being set or rotated.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VkKind {
    /// Membership circuit VK (3 IC points: IC[0], IC[1]=commitment, IC[2]=epoch).
    Membership,
    /// Update-transition circuit VK (4 IC points: IC[0], IC[1]=c_old, IC[2]=epoch_old, IC[3]=c_new).
    Update,
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
    HistoryV2(BytesN<32>),
    /// Used proof hash — prevents cross-function and cross-group proof replay.
    UsedProof(BytesN<32>),
    /// Active group count per tier (instance storage, for M-4 limit enforcement).
    GroupCount(u32),
    /// When true, only the admin can create new groups (N-26 access control).
    RestrictedMode,
}

// ================================================================
// Contract
// ================================================================

#[contract]
pub struct SepXxxxContract;

#[contractimpl]
impl SepXxxxContract {
    // ---- Admin ----

    /// Initialize the contract with verification keys for all three tiers.
    ///
    /// Must be called exactly once. The `admin` address is recorded and
    /// required for future admin operations. Each membership VK must have
    /// exactly 3 IC points (commitment + epoch); each update VK must have
    /// exactly 4 IC points (c_old + epoch_old + c_new). The update VKs are
    /// introduced by the #59 fix to close the `update_commitment` binding gap.
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

        if tier > 2 {
            return Err(Error::InvalidTier);
        }

        let (key, expected_ic_len) = match kind {
            VkKind::Membership => (DataKey::VK(tier), 3u32),
            VkKind::Update => (DataKey::UpdateVK(tier), 4u32),
        };

        if new_vk.ic.len() != expected_ic_len {
            return Err(Error::InvalidVkLength);
        }

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
            _ => {
                // Types 2/3 land in later phases.
                return Err(Error::UnknownGroupType);
            }
        }
        if public_inputs.commitment != commitment || public_inputs.epoch != 0 {
            return Err(Error::PublicInputsMismatch);
        }
        if Self::group_exists(&env, &group_id) {
            return Err(Error::GroupAlreadyExists);
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

    /// Verify a membership proof against the current group state.
    ///
    /// Read-only — does not modify contract state. Proofs submitted here
    /// are NOT recorded, so the same proof can be re-verified and can
    /// still be used for state-changing operations.
    pub fn verify_membership(
        env: Env,
        group_id: BytesN<32>,
        proof: Groth16Proof,
        public_inputs: PublicInputs,
    ) -> Result<bool, Error> {
        Self::require_initialized(&env)?;

        let state: CommitmentEntry = env
            .storage()
            .persistent()
            .get(&DataKey::Group(group_id.clone()))
            .ok_or(Error::GroupNotFound)?;

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

    /// Deactivate a group (requires membership proof).
    ///
    /// After deactivation `verify_membership` and `get_state` still work,
    /// but `update_commitment` is rejected. This is irreversible.
    ///
    /// N-14: Uses proof-based authorization only (same rationale as `update_commitment`).
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

    fn bump_group(env: &Env, group_id: &BytesN<32>) {
        // Bump whichever key variant is present. After the first V2 write
        // the legacy keys are removed, so post-migration calls no-op on
        // the legacy branches.
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
        // Bump instance storage TTL to prevent admin key loss.
        env.storage()
            .instance()
            .extend_ttl(LEDGER_THRESHOLD, LEDGER_BUMP);
    }

    /// Compute a SHA-256 hash of the proof components for replay tracking.
    fn proof_hash(env: &Env, proof: &Groth16Proof) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.append(&Bytes::from_slice(env, proof.a.to_array().as_slice()));
        preimage.append(&Bytes::from_slice(env, proof.b.to_array().as_slice()));
        preimage.append(&Bytes::from_slice(env, proof.c.to_array().as_slice()));
        env.crypto().sha256(&preimage).into()
    }

    /// Reject if this exact proof has been submitted before (cross-function
    /// and cross-group replay prevention).
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

    /// Record a proof hash so it cannot be replayed.
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

/// Verify a Groth16 proof using BLS12-381 host functions.
fn verify_groth16_proof(
    env: &Env,
    vk: &VerificationKeyData,
    proof: &Groth16Proof,
    commitment: &BytesN<32>,
    epoch: u64,
) -> bool {
    let bls = env.crypto().bls12_381();

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

// ================================================================
// Tests
// ================================================================

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    fn setup_env() -> (Env, SepXxxxContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(SepXxxxContract, ());
        let client = SepXxxxContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        (env, client, admin)
    }

    fn mock_vk(env: &Env) -> VerificationKeyData {
        let g1 = BytesN::from_array(env, &[0u8; 96]);
        let g2 = BytesN::from_array(env, &[0u8; 192]);
        VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![env, g1.clone(), g1.clone(), g1],
        }
    }

    fn mock_update_vk(env: &Env) -> VerificationKeyData {
        let g1 = BytesN::from_array(env, &[0u8; 96]);
        let g2 = BytesN::from_array(env, &[0u8; 192]);
        VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![env, g1.clone(), g1.clone(), g1.clone(), g1],
        }
    }

    fn mock_proof(env: &Env) -> Groth16Proof {
        Groth16Proof {
            a: BytesN::from_array(env, &[0u8; 96]),
            b: BytesN::from_array(env, &[0u8; 192]),
            c: BytesN::from_array(env, &[0u8; 96]),
        }
    }

    #[test]
    fn test_initialize() {
        let (env, client, admin) = setup_env();
        let vk = mock_vk(&env);
        let uvk = mock_update_vk(&env);
        client.initialize(&admin, &vk, &vk, &vk, &uvk, &uvk, &uvk);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #2)")]
    fn test_double_initialize_rejected() {
        let (env, client, admin) = setup_env();
        let vk = mock_vk(&env);
        let uvk = mock_update_vk(&env);
        client.initialize(&admin, &vk, &vk, &vk, &uvk, &uvk, &uvk);
        client.initialize(&admin, &vk, &vk, &vk, &uvk, &uvk, &uvk);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #9)")]
    fn test_invalid_vk_length_rejected() {
        let (env, client, admin) = setup_env();
        let g1 = BytesN::from_array(&env, &[0u8; 96]);
        let g2 = BytesN::from_array(&env, &[0u8; 192]);

        let bad_vk = VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![&env, g1.clone(), g1],
        };
        let good_vk = mock_vk(&env);
        let uvk = mock_update_vk(&env);

        client.initialize(&admin, &bad_vk, &good_vk, &good_vk, &uvk, &uvk, &uvk);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #9)")]
    fn test_invalid_update_vk_length_rejected() {
        // Update VK with 3 IC points (should be 4).
        let (env, client, admin) = setup_env();
        let g1 = BytesN::from_array(&env, &[0u8; 96]);
        let g2 = BytesN::from_array(&env, &[0u8; 192]);
        let bad_update_vk = VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![&env, g1.clone(), g1.clone(), g1],
        };
        let good_vk = mock_vk(&env);
        let good_update_vk = mock_update_vk(&env);

        client.initialize(
            &admin,
            &good_vk,
            &good_vk,
            &good_vk,
            &bad_update_vk,
            &good_update_vk,
            &good_update_vk,
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #5)")]
    fn test_group_not_found() {
        let (env, client, admin) = setup_env();
        let vk = mock_vk(&env);
        let uvk = mock_update_vk(&env);
        client.initialize(&admin, &vk, &vk, &vk, &uvk, &uvk, &uvk);

        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        client.get_state(&group_id);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #8)")]
    fn test_invalid_tier_rejected() {
        let (env, client, admin) = setup_env();
        let vk = mock_vk(&env);
        let uvk = mock_update_vk(&env);
        client.initialize(&admin, &vk, &vk, &vk, &uvk, &uvk, &uvk);

        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };

        client.create_group(&caller, &group_id, &commitment, &3u32, &mock_proof(&env), &pi);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1)")]
    fn test_not_initialized_rejected() {
        let (env, client, _admin) = setup_env();
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
    #[should_panic(expected = "Error(Contract, #10)")]
    fn test_public_inputs_mismatch_on_create() {
        let (env, client, admin) = setup_env();
        let vk = mock_vk(&env);
        let uvk = mock_update_vk(&env);
        client.initialize(&admin, &vk, &vk, &vk, &uvk, &uvk, &uvk);

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
        let (env, client, admin) = setup_env();
        let vk = mock_vk(&env);
        let uvk = mock_update_vk(&env);
        client.initialize(&admin, &vk, &vk, &vk, &uvk, &uvk, &uvk);

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

    /// Initialize the contract and return the contract address too.
    fn setup_initialized() -> (Env, SepXxxxContractClient<'static>, Address, Address) {
        let (env, client, admin) = setup_env();
        let vk = mock_vk(&env);
        let uvk = mock_update_vk(&env);
        client.initialize(&admin, &vk, &vk, &vk, &uvk, &uvk, &uvk);
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

    /// Record a proof hash as used in contract storage.
    fn inject_used_proof(env: &Env, contract_id: &Address, proof: &Groth16Proof) {
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
    #[should_panic(expected = "Error(Contract, #1)")]
    fn test_update_commitment_not_initialized() {
        let (env, client, _admin) = setup_env();
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
        let proof = mock_proof(&env);
        inject_used_proof(&env, &contract_id, &proof);

        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
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
        inject_used_proof(&env, &contract_id, &proof);

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
        inject_used_proof(&env, &contract_id, &proof);

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

    #[test]
    #[should_panic(expected = "Error(Contract, #1)")]
    fn test_update_vk_not_initialized() {
        let (env, client, _admin) = setup_env();
        let vk = mock_vk(&env);
        client.update_vk(&VkKind::Membership, &0u32, &vk);
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

    #[test]
    #[should_panic(expected = "Error(Contract, #1)")]
    fn test_set_restricted_mode_requires_init() {
        let (_env, client, _admin) = setup_env();
        client.set_restricted_mode(&true);
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
    #[should_panic(expected = "Error(Contract, #18)")]
    fn test_create_group_v2_rejects_democracy_type() {
        let (env, client, _admin, _cid) = setup_initialized();
        let caller = Address::generate(&env);
        let group_id = BytesN::from_array(&env, &[10u8; 32]);
        let commitment = BytesN::from_array(&env, &[11u8; 32]);
        let pi = PublicInputs {
            commitment: commitment.clone(),
            epoch: 0,
        };
        client.create_group_v2(
            &caller,
            &group_id,
            &commitment,
            &0u32,
            &2u32, // Democracy
            &10u32,
            &mock_proof(&env),
            &pi,
        );
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
}
