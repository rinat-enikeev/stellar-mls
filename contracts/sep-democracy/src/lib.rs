//! SEP Democracy Soroban Contract — per-type private group with hidden
//! member counts via occupancy commitment.
//!
//! This is the **per-type Democracy** contract, one of an eventual four
//! (Anarchy, OneOnOne, Democracy, Oligarchy) in separate crates. The
//! `group_type` discriminator is implicit in the contract address;
//! there is no polymorphic dispatch and no group_type field in storage.
//!
//! Every value pinned in `contracts/sep-democracy/test-vectors.json`
//! MUST match the constants and behaviors defined here. The
//! `test_vectors_consistency` inline test asserts the match at build
//! time.
//!
//! # Verification
//!
//! Membership proofs use a 3-IC-point Groth16 verifying key with
//! public inputs `(commitment, epoch)`. Update proofs use a 7-IC-point
//! Groth16 verifying key with public inputs
//! `(c_old, epoch_old, c_new, occupancy_commitment_old,
//!   occupancy_commitment_new, threshold_numerator)` per the v2
//! democracy circuit (`docs/democracy-update-testnet-design.md`
//! §4.7.3, §4.7.6). The `threshold_numerator` is supplied by the
//! contract from `current.threshold_numerator` storage at verify
//! time — NOT carried on the wire — so a chain observer cannot
//! distinguish two groups with different thresholds by call-payload
//! analysis. Only direct storage inspection reveals the threshold
//! value.
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
/// Older entries are pruned from contract state but remain available
/// via contract events.
const HISTORY_WINDOW: u32 = 64;

/// Minimum TTL threshold for persistent storage (~1 day in ledgers).
const LEDGER_THRESHOLD: u32 = 17_280;

/// TTL bump amount for persistent storage (~30 days in ledgers).
const LEDGER_BUMP: u32 = 518_400;

/// Maximum number of active groups allowed per tier.
const MAX_GROUPS_PER_TIER: u32 = 10_000;

/// IC point counts for the two VK families this contract verifies.
///
/// MUST match `test-vectors.json` `vk_kind_enum.*.ic_count`.
const MEMBERSHIP_IC_POINTS: u32 = 3;
const UPDATE_IC_POINTS: u32 = 7;

/// Number of slot positions a Merkle tree of the given tier can hold.
/// * tier 0 (Small)  — depth 5  → 32
/// * tier 1 (Medium) — depth 8  → 256
/// * tier 2 (Large)  — depth 11 → 2048
///
/// Per design §4.7.2 / §4.7.3 the v2 contract no longer enforces a
/// per-tier `member_count` ceiling at runtime — the bitmap occupancy
/// commitment internalizes the bound. This function is preserved as
/// the canonical source of truth for the `tier_capacity` test-vector
/// pin (`test_vectors_consistency`).
#[allow(dead_code)]
fn tier_capacity(tier: u32) -> u32 {
    match tier {
        0 => 32,
        1 => 256,
        2 => 2048,
        _ => 0,
    }
}

// ================================================================
// Errors
// ================================================================

/// Numeric values pinned in `test-vectors.json` `error_codes.vectors`.
/// Future contracts in this family (Anarchy / OneOnOne / Oligarchy)
/// MUST keep this numbering disjoint where overlap would confuse
/// cross-contract clients.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    /// Reserved (was: Unauthorized). Admin checks use `require_auth()`
    /// which panics directly.
    Reserved3 = 3,
    GroupAlreadyExists = 4,
    GroupNotFound = 5,
    GroupInactive = 6,
    InvalidProof = 7,
    InvalidTier = 8,
    InvalidVkLength = 9,
    PublicInputsMismatch = 10,
    InvalidEpoch = 11,
    ProofReplay = 12,
    TierGroupLimitReached = 13,
    AdminOnly = 14,
    InvalidCommitmentEncoding = 15,
    UnknownVkKind = 16,
    InvalidPoint = 26,
    GroupStillActive = 27,
    /// `create_group` rejects `threshold_numerator < 1 || > 100`
    /// (design v0.5.1 §4.7.6).
    InvalidThreshold = 28,
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

/// On-chain state of a Democracy group at a particular epoch.
///
/// Differences from `sep-xxxx`'s `CommitmentEntryV2`:
///   * No `group_type` field — single-type contract.
///   * No `member_count` field — replaced by `occupancy_commitment`
///     (design §3.4 hidden-counts requirement).
///   * Adds `threshold_numerator: u8` (design §4.7.6).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentEntry {
    /// Poseidon commitment (BLS12-381 field element, 32 bytes BE,
    /// canonical Fr encoding).
    pub commitment: BytesN<32>,
    /// Epoch counter (starts at 0, increments by 1 per successful
    /// `update_commitment`).
    pub epoch: u64,
    /// Ledger timestamp when this state was recorded.
    pub timestamp: u64,
    /// Circuit tier: 0 = Small, 1 = Medium, 2 = Large. Fixed at
    /// `create_group`.
    pub tier: u32,
    /// Whether the group accepts further updates.
    pub active: bool,
    /// Poseidon hash of the slot bitmap (design §4.7.2). Replaces
    /// v1's `member_count` and hides the count from chain observers.
    pub occupancy_commitment: BytesN<32>,
    /// Quorum threshold as integer percentage in [1, 100]
    /// (design §4.7.6). Fixed at `create_group`; never mutated by
    /// `update_commitment`.
    pub threshold_numerator: u32,
}

/// Public inputs supplied to `create_group` and `verify_membership`.
///
/// Two scalars wired to `Membership` IC points 1 and 2.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicInputs {
    pub commitment: BytesN<32>,
    pub epoch: u64,
}

/// Wire payload for `update_commitment` per design §4.4 / §4.7.3.
///
/// Five wire-supplied scalars. The 6th Groth16 public input
/// (`threshold_numerator`) is **NOT** on the wire — the contract
/// reads it from `current.threshold_numerator` at verify time
/// (§4.7.6 contract-supplied input).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateCommitmentPublicInputs {
    pub c_old: BytesN<32>,
    pub epoch_old: u64,
    pub c_new: BytesN<32>,
    pub occupancy_commitment_old: BytesN<32>,
    pub occupancy_commitment_new: BytesN<32>,
}

/// Selector for which VK family is being installed or rotated by
/// `update_vk`. No `UpdateByType(u32)` because the contract is
/// Democracy-only — group_type is implicit in the contract address.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VkKind {
    /// Membership-circuit VK (3 IC points).
    Membership,
    /// v2 democracy update-circuit VK (7 IC points).
    Update,
}

/// Groth16 verification key stored as raw bytes (uncompressed BLS12-381).
///
///   G1 = 96 bytes uncompressed
///   G2 = 192 bytes uncompressed
#[contracttype]
#[derive(Clone, Debug)]
pub struct VerificationKeyData {
    pub alpha_g1: BytesN<96>,
    pub beta_g2: BytesN<192>,
    pub gamma_g2: BytesN<192>,
    pub delta_g2: BytesN<192>,
    pub ic: Vec<BytesN<96>>,
}

/// Groth16 proof in uncompressed BLS12-381 form. Total 384 bytes.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Groth16Proof {
    pub a: BytesN<96>,
    pub b: BytesN<192>,
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
    /// Whether only admin can `create_group` (instance storage).
    RestrictedMode,
    /// Membership-circuit VK per tier (persistent).
    VK(u32),
    /// v2 democracy update-circuit VK per tier (persistent). No
    /// `(tier, group_type)` second key — single-type contract.
    UpdateVK(u32),
    /// Current group state (persistent).
    Group(BytesN<32>),
    /// Group history — rolling window (persistent).
    History(BytesN<32>),
    /// Used proof hashes — global nullifier set (persistent, TTL bounded).
    UsedProof(BytesN<32>),
    /// Active group count per tier (instance, M-4 limit enforcement).
    GroupCount(u32),
}

// ================================================================
// Contract
// ================================================================

#[contract]
pub struct SepDemocracyContract;

#[contractimpl]
impl SepDemocracyContract {
    // ---- Initialization ----

    /// Atomic constructor: takes admin and the per-tier VKs for both
    /// the membership and update circuits. Runs at deploy time.
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

    /// Post-deploy initializer for instances created without
    /// `__constructor`. Idempotency-rejected after first call.
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

        let m = MEMBERSHIP_IC_POINTS;
        if vk_small.ic.len() != m
            || vk_medium.ic.len() != m
            || vk_large.ic.len() != m
        {
            return Err(Error::InvalidVkLength);
        }
        let u = UPDATE_IC_POINTS;
        if update_vk_small.ic.len() != u
            || update_vk_medium.ic.len() != u
            || update_vk_large.ic.len() != u
        {
            return Err(Error::InvalidVkLength);
        }

        validate_vk_points(&vk_small)?;
        validate_vk_points(&vk_medium)?;
        validate_vk_points(&vk_large)?;
        validate_vk_points(&update_vk_small)?;
        validate_vk_points(&update_vk_medium)?;
        validate_vk_points(&update_vk_large)?;

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().persistent().set(&DataKey::VK(0), &vk_small);
        env.storage().persistent().set(&DataKey::VK(1), &vk_medium);
        env.storage().persistent().set(&DataKey::VK(2), &vk_large);
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
            env.storage().persistent().extend_ttl(
                &DataKey::UpdateVK(tier),
                LEDGER_THRESHOLD,
                LEDGER_BUMP,
            );
        }
        Ok(())
    }

    // ---- Admin ----

    /// Admin-only VK rotation, per design §4.6 fingerprint-coordination
    /// triplet. Membership VK at `tier` ∈ {0,1,2} OR Update VK at
    /// `tier` ∈ {0,1,2}.
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
            VkKind::Membership => (DataKey::VK(tier), MEMBERSHIP_IC_POINTS),
            VkKind::Update => (DataKey::UpdateVK(tier), UPDATE_IC_POINTS),
        };
        if new_vk.ic.len() != expected_ic_len {
            return Err(Error::InvalidVkLength);
        }
        validate_vk_points(&new_vk)?;
        env.storage().persistent().set(&key, &new_vk);
        env.storage()
            .persistent()
            .extend_ttl(&key, LEDGER_THRESHOLD, LEDGER_BUMP);
        Ok(())
    }

    /// Admin toggles restricted mode. When `restricted == true`,
    /// only the admin may call `create_group`.
    pub fn set_restricted_mode(env: Env, restricted: bool) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::RestrictedMode, &restricted);
        Ok(())
    }

    /// Permissionless TTL bump for a group's persistent storage.
    pub fn bump_group_ttl(env: Env, group_id: BytesN<32>) -> Result<(), Error> {
        if !Self::group_exists(&env, &group_id) {
            return Err(Error::GroupNotFound);
        }
        Self::bump_group(&env, &group_id);
        Ok(())
    }

    // ---- Group lifecycle ----

    /// Create a new Democracy group at epoch 0.
    ///
    /// Validates: tier ≤ 2, threshold_numerator ∈ [1, 100], group_id
    /// unused, `commitment` and `occupancy_commitment_initial` are
    /// canonical Fr, public inputs match the proof, proof verifies
    /// under the membership VK at `tier`, proof not replayed, tier
    /// capacity not exceeded.
    pub fn create_group(
        env: Env,
        caller: Address,
        group_id: BytesN<32>,
        commitment: BytesN<32>,
        tier: u32,
        threshold_numerator: u32,
        occupancy_commitment_initial: BytesN<32>,
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
        if threshold_numerator < 1 || threshold_numerator > 100 {
            return Err(Error::InvalidThreshold);
        }
        if public_inputs.commitment != commitment || public_inputs.epoch != 0 {
            return Err(Error::PublicInputsMismatch);
        }
        if Self::group_exists(&env, &group_id) {
            return Err(Error::GroupAlreadyExists);
        }
        if !is_canonical_fr(&commitment) {
            return Err(Error::InvalidCommitmentEncoding);
        }
        if !is_canonical_fr(&occupancy_commitment_initial) {
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
        if !verify_membership_proof(&env, &vk, &proof, &commitment, 0) {
            return Err(Error::InvalidProof);
        }

        Self::record_proof(&env, &proof);

        let timestamp = env.ledger().timestamp();
        let entry = CommitmentEntry {
            commitment: commitment.clone(),
            epoch: 0,
            timestamp,
            tier,
            active: true,
            occupancy_commitment: occupancy_commitment_initial,
            threshold_numerator,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Group(group_id.clone()), &entry);
        env.storage().persistent().set(
            &DataKey::History(group_id.clone()),
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

    /// Advance a Democracy group to the next epoch.
    ///
    /// The verifier consumes 6 Groth16 public inputs in canonical
    /// order: 5 from the wire payload, 1 (`threshold_numerator`) from
    /// `current.threshold_numerator` per design §4.7.6.
    pub fn update_commitment(
        env: Env,
        group_id: BytesN<32>,
        proof: Groth16Proof,
        public_inputs: UpdateCommitmentPublicInputs,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;

        let current = Self::load_group(&env, &group_id)?;
        if !current.active {
            return Err(Error::GroupInactive);
        }

        let new_epoch = current.epoch.checked_add(1).ok_or(Error::InvalidEpoch)?;

        if public_inputs.c_old != current.commitment
            || public_inputs.epoch_old != current.epoch
            || public_inputs.occupancy_commitment_old != current.occupancy_commitment
        {
            return Err(Error::PublicInputsMismatch);
        }

        if !is_canonical_fr(&public_inputs.c_new) {
            return Err(Error::InvalidCommitmentEncoding);
        }
        if !is_canonical_fr(&public_inputs.occupancy_commitment_new) {
            return Err(Error::InvalidCommitmentEncoding);
        }

        Self::check_proof_replay(&env, &proof)?;

        let vk = Self::load_update_vk(&env, current.tier)?;
        if !verify_update_proof(
            &env,
            &vk,
            &proof,
            &public_inputs.c_old,
            public_inputs.epoch_old,
            &public_inputs.c_new,
            &public_inputs.occupancy_commitment_old,
            &public_inputs.occupancy_commitment_new,
            current.threshold_numerator,
        ) {
            return Err(Error::InvalidProof);
        }

        Self::record_proof(&env, &proof);

        let timestamp = env.ledger().timestamp();
        Self::archive_entry(&env, &group_id, &current);

        let new_entry = CommitmentEntry {
            commitment: public_inputs.c_new.clone(),
            epoch: new_epoch,
            timestamp,
            tier: current.tier,
            active: true,
            occupancy_commitment: public_inputs.occupancy_commitment_new,
            // Threshold is fixed at create; never mutated by update_commitment
            // per design §4.7.6.
            threshold_numerator: current.threshold_numerator,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Group(group_id.clone()), &new_entry);
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

    /// Read-only membership verification.
    pub fn verify_membership(
        env: Env,
        group_id: BytesN<32>,
        proof: Groth16Proof,
        public_inputs: PublicInputs,
    ) -> Result<bool, Error> {
        Self::require_initialized(&env)?;
        let state = Self::load_group(&env, &group_id)?;

        if public_inputs.commitment != state.commitment
            || public_inputs.epoch != state.epoch
        {
            return Err(Error::PublicInputsMismatch);
        }
        let vk = Self::load_vk(&env, state.tier)?;
        Ok(verify_membership_proof(
            &env,
            &vk,
            &proof,
            &state.commitment,
            state.epoch,
        ))
    }

    /// Permanently freeze the group. Any current member may deactivate
    /// (V-C1 safety valve from sep-xxxx). Irreversible.
    pub fn deactivate_group(
        env: Env,
        group_id: BytesN<32>,
        proof: Groth16Proof,
        public_inputs: PublicInputs,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        let current = Self::load_group(&env, &group_id)?;
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
        if !verify_membership_proof(&env, &vk, &proof, &current.commitment, current.epoch) {
            return Err(Error::InvalidProof);
        }
        Self::record_proof(&env, &proof);

        let timestamp = env.ledger().timestamp();
        let deactivated = CommitmentEntry {
            active: false,
            timestamp,
            ..current.clone()
        };
        env.storage()
            .persistent()
            .set(&DataKey::Group(group_id.clone()), &deactivated);
        Self::bump_group(&env, &group_id);

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

    pub fn get_commitment(
        env: Env,
        group_id: BytesN<32>,
    ) -> Result<CommitmentEntry, Error> {
        Self::load_group(&env, &group_id)
    }

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
            .get(&DataKey::History(group_id))
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

    fn load_group(env: &Env, group_id: &BytesN<32>) -> Result<CommitmentEntry, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Group(group_id.clone()))
            .ok_or(Error::GroupNotFound)
    }

    fn group_exists(env: &Env, group_id: &BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::Group(group_id.clone()))
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
    }

    fn proof_hash(env: &Env, proof: &Groth16Proof) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.append(&Bytes::from_slice(env, proof.a.to_array().as_slice()));
        preimage.append(&Bytes::from_slice(env, proof.b.to_array().as_slice()));
        preimage.append(&Bytes::from_slice(env, proof.c.to_array().as_slice()));
        env.crypto().sha256(&preimage).into()
    }

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

    fn record_proof(env: &Env, proof: &Groth16Proof) {
        let hash = Self::proof_hash(env, proof);
        env.storage()
            .persistent()
            .set(&DataKey::UsedProof(hash.clone()), &true);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::UsedProof(hash), LEDGER_THRESHOLD, LEDGER_BUMP);
    }

    fn archive_entry(env: &Env, group_id: &BytesN<32>, entry: &CommitmentEntry) {
        let mut history: Vec<CommitmentEntry> = env
            .storage()
            .persistent()
            .get(&DataKey::History(group_id.clone()))
            .unwrap_or(Vec::new(env));

        history.push_back(entry.clone());
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
            .set(&DataKey::History(group_id.clone()), &history);
    }
}

// ================================================================
// Groth16 verification helpers
// ================================================================

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

fn validate_proof_points(proof: &Groth16Proof) -> bool {
    G1Affine::from_bytes(proof.a.clone()).is_in_subgroup()
        && G2Affine::from_bytes(proof.b.clone()).is_in_subgroup()
        && G1Affine::from_bytes(proof.c.clone()).is_in_subgroup()
}

fn is_canonical_fr(value: &BytesN<32>) -> bool {
    let fr = Fr::from_bytes(value.clone());
    let canonical: BytesN<32> = fr.to_bytes();
    canonical == *value
}

fn u64_to_u256_be(val: u64) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[24..32].copy_from_slice(&val.to_be_bytes());
    bytes
}

fn u32_to_fr(env: &Env, value: u32) -> Fr {
    let bytes = Bytes::from_array(env, &u64_to_u256_be(value as u64));
    Fr::from_u256(U256::from_be_bytes(env, &bytes))
}

/// Verify a 3-IC-point Membership proof. Public inputs:
///   IC[1] · commitment + IC[2] · epoch (plus IC[0] base).
///
/// Pairing check: `e(-π_A, π_B) · e(α, β) · e(vk_x, γ) · e(π_C, δ) = 1_GT`.
fn verify_membership_proof(
    env: &Env,
    vk: &VerificationKeyData,
    proof: &Groth16Proof,
    commitment: &BytesN<32>,
    epoch: u64,
) -> bool {
    if vk.ic.len() != MEMBERSHIP_IC_POINTS {
        return false;
    }
    if !validate_proof_points(proof) {
        return false;
    }
    if !is_canonical_fr(commitment) {
        return false;
    }
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

/// Verify a 7-IC-point v2 democracy update proof. Public inputs in
/// canonical order (design §4.7.6):
///
/// ```
/// IC[1]·c_old + IC[2]·epoch_old + IC[3]·c_new
///   + IC[4]·occupancy_commitment_old + IC[5]·occupancy_commitment_new
///   + IC[6]·threshold_numerator
/// ```
///
/// The 6th input (`threshold_numerator`) is contract-supplied from
/// storage; the other 5 come from the wire `UpdateCommitmentPublicInputs`.
#[allow(clippy::too_many_arguments)]
fn verify_update_proof(
    env: &Env,
    vk: &VerificationKeyData,
    proof: &Groth16Proof,
    c_old: &BytesN<32>,
    epoch_old: u64,
    c_new: &BytesN<32>,
    occupancy_commitment_old: &BytesN<32>,
    occupancy_commitment_new: &BytesN<32>,
    threshold_numerator: u32,
) -> bool {
    if vk.ic.len() != UPDATE_IC_POINTS {
        return false;
    }
    if !validate_proof_points(proof) {
        return false;
    }
    if !is_canonical_fr(c_old) {
        return false;
    }
    if !is_canonical_fr(c_new) {
        return false;
    }
    if !is_canonical_fr(occupancy_commitment_old) {
        return false;
    }
    if !is_canonical_fr(occupancy_commitment_new) {
        return false;
    }
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
    let ic3 = G1Affine::from_bytes(vk.ic.get(3).unwrap());
    let ic4 = G1Affine::from_bytes(vk.ic.get(4).unwrap());
    let ic5 = G1Affine::from_bytes(vk.ic.get(5).unwrap());
    let ic6 = G1Affine::from_bytes(vk.ic.get(6).unwrap());

    let c_old_fr = Fr::from_bytes(c_old.clone());
    let c_new_fr = Fr::from_bytes(c_new.clone());
    let occ_old_fr = Fr::from_bytes(occupancy_commitment_old.clone());
    let occ_new_fr = Fr::from_bytes(occupancy_commitment_new.clone());
    let epoch_bytes = Bytes::from_array(env, &u64_to_u256_be(epoch_old));
    let epoch_fr = Fr::from_u256(U256::from_be_bytes(env, &epoch_bytes));
    let threshold_fr = u32_to_fr(env, threshold_numerator);

    let msm_points: Vec<G1Affine> = vec![env, ic1, ic2, ic3, ic4, ic5, ic6];
    let msm_scalars: Vec<Fr> = vec![
        env,
        c_old_fr,
        epoch_fr,
        c_new_fr,
        occ_old_fr,
        occ_new_fr,
        threshold_fr,
    ];
    let msm_result = bls.g1_msm(msm_points, msm_scalars);
    let vk_x = bls.g1_add(&ic0, &msm_result);

    let neg_a = -proof_a;
    let g1s: Vec<G1Affine> = vec![env, neg_a, alpha, vk_x, proof_c];
    let g2s: Vec<G2Affine> = vec![env, proof_b, beta, gamma, delta];

    bls.pairing_check(g1s, g2s)
}

#[cfg(test)]
mod test;
