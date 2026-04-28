//! SEP Tyranny Soroban Contract — per-type single-admin governance.
//!
//! Fifth in the per-type contract family (Anarchy, OneOnOne, Democracy,
//! Oligarchy, Tyranny). Single-admin governance: the admin's BLS
//! pubkey is committed at creation (Poseidon-hashed), pinned for the
//! group's lifetime, and only proofs demonstrating knowledge of the
//! admin's secret key advance the membership commitment.
//!
//! Tyranny is NOT Oligarchy K=1 — no admin tree, no threshold; admin
//! is a single pinned pubkey hash. Tyranny is NOT Anarchy with
//! caller.require_auth() — the cryptographic binding hides the
//! admin's Stellar address from chain observers.
//!
//! Every value pinned in `contracts/sep-tyranny/test-vectors.json`
//! MUST match the constants and behaviors defined here. The
//! `test_vectors_consistency` inline test asserts the match.
//!
//! # Verification
//!
//!   * Membership VK: 3 IC points, public inputs `(commitment, epoch)`
//!   * Create VK:     4 IC points, public inputs `(commitment, epoch=0,
//!                    admin_pubkey_commitment)`
//!   * Update VK:     5 IC points, public inputs `(c_old, epoch_old,
//!                    c_new, admin_pubkey_commitment)` — the last is
//!                    contract-supplied from `current.admin_pubkey_commitment`
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

const HISTORY_WINDOW: u32 = 64;
const LEDGER_THRESHOLD: u32 = 17_280;
const LEDGER_BUMP: u32 = 518_400;
const MAX_GROUPS_PER_TIER: u32 = 10_000;

const MEMBERSHIP_IC_POINTS: u32 = 3;
const CREATE_IC_POINTS: u32 = 4;
const UPDATE_IC_POINTS: u32 = 5;

#[cfg(test)]
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
/// Number alignment with sibling per-type contracts is intentional;
/// gaps in the numbering (3, 6, 16-25, 27-30) are reserved by sibling
/// contracts for variants Tyranny doesn't need.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    GroupAlreadyExists = 4,
    GroupNotFound = 5,
    InvalidProof = 7,
    InvalidTier = 8,
    InvalidVkLength = 9,
    PublicInputsMismatch = 10,
    InvalidEpoch = 11,
    ProofReplay = 12,
    TierGroupLimitReached = 13,
    AdminOnly = 14,
    InvalidCommitmentEncoding = 15,
    InvalidPoint = 26,
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
    pub admin_pubkey_commitment: BytesN<32>,
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
pub struct RestrictedModeChanged {
    #[topic]
    pub admin: Address,
    pub restricted: bool,
    pub timestamp: u64,
}

// ================================================================
// Types
// ================================================================

/// On-chain state of a Tyranny group at a particular epoch.
///
/// Differences from per-type-template:
///   * No `group_type` (single-type contract)
///   * No `active` (no deactivate path; postmortem #153)
///   * No `member_count`, no `occupancy_commitment` (Tyranny is
///     value-agnostic to count)
///   * No `threshold_numerator` (single admin, no quorum)
///
/// `admin_pubkey_commitment` lives in its own per-group storage slot
/// (`DataKey::AdminCommitment(group_id)`) rather than as a field
/// here. It is invariant for a group's lifetime; carrying it on
/// every history snapshot would duplicate ~2KB per group at
/// HISTORY_WINDOW=64 with no informational gain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentEntry {
    pub commitment: BytesN<32>,
    pub epoch: u64,
    pub timestamp: u64,
    pub tier: u32,
}

/// Public inputs supplied to `verify_membership`. 2 wire fields → 3 IC.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MembershipPublicInputs {
    pub commitment: BytesN<32>,
    pub epoch: u64,
}

/// Public inputs supplied to `create_group`. 3 wire fields → 4 IC.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatePublicInputs {
    pub commitment: BytesN<32>,
    pub epoch: u64,
    pub admin_pubkey_commitment: BytesN<32>,
}

/// Wire payload for `update_commitment`. 3 wire scalars; the verifier
/// consumes a 4-public-input vector — the 4th input
/// (`admin_pubkey_commitment`) is contract-supplied from
/// `current.admin_pubkey_commitment`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdatePublicInputs {
    pub c_old: BytesN<32>,
    pub epoch_old: u64,
    pub c_new: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VkKind {
    Membership,
    Create,
    Update,
}

/// Groth16 verification key stored as raw bytes (uncompressed BLS12-381).
#[contracttype]
#[derive(Clone, Debug)]
pub struct VerificationKeyData {
    pub alpha_g1: BytesN<96>,
    pub beta_g2: BytesN<192>,
    pub gamma_g2: BytesN<192>,
    pub delta_g2: BytesN<192>,
    pub ic: Vec<BytesN<96>>,
}

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
    Admin,
    RestrictedMode,
    /// Membership-circuit VK per tier (persistent).
    VK(u32),
    /// Tyranny Create-circuit VK per tier (persistent).
    CreateVK(u32),
    /// Tyranny Update-circuit VK per tier (persistent).
    UpdateVK(u32),
    Group(BytesN<32>),
    /// Per-group admin pubkey commitment (Poseidon hash of admin's BLS
    /// pubkey). Pinned at create_group, never mutated. Stored in its
    /// own slot rather than as a CommitmentEntry field to avoid
    /// duplicating an immutable value across `History` snapshots.
    AdminCommitment(BytesN<32>),
    History(BytesN<32>),
    UsedProof(BytesN<32>),
    GroupCount(u32),
}

// ================================================================
// Contract
// ================================================================

#[contract]
pub struct SepTyrannyContract;

#[contractimpl]
impl SepTyrannyContract {
    /// One-time init. 9 VKs total: Membership × 3 tiers + Create × 3
    /// + Update × 3.
    #[allow(clippy::too_many_arguments)]
    pub fn __constructor(
        env: Env,
        admin: Address,
        vk_membership_small: VerificationKeyData,
        vk_membership_medium: VerificationKeyData,
        vk_membership_large: VerificationKeyData,
        vk_create_small: VerificationKeyData,
        vk_create_medium: VerificationKeyData,
        vk_create_large: VerificationKeyData,
        vk_update_small: VerificationKeyData,
        vk_update_medium: VerificationKeyData,
        vk_update_large: VerificationKeyData,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();

        for vk in [&vk_membership_small, &vk_membership_medium, &vk_membership_large] {
            if vk.ic.len() != MEMBERSHIP_IC_POINTS {
                return Err(Error::InvalidVkLength);
            }
            validate_vk_points(vk)?;
        }
        for vk in [&vk_create_small, &vk_create_medium, &vk_create_large] {
            if vk.ic.len() != CREATE_IC_POINTS {
                return Err(Error::InvalidVkLength);
            }
            validate_vk_points(vk)?;
        }
        for vk in [&vk_update_small, &vk_update_medium, &vk_update_large] {
            if vk.ic.len() != UPDATE_IC_POINTS {
                return Err(Error::InvalidVkLength);
            }
            validate_vk_points(vk)?;
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().persistent().set(&DataKey::VK(0), &vk_membership_small);
        env.storage().persistent().set(&DataKey::VK(1), &vk_membership_medium);
        env.storage().persistent().set(&DataKey::VK(2), &vk_membership_large);
        env.storage().persistent().set(&DataKey::CreateVK(0), &vk_create_small);
        env.storage().persistent().set(&DataKey::CreateVK(1), &vk_create_medium);
        env.storage().persistent().set(&DataKey::CreateVK(2), &vk_create_large);
        env.storage().persistent().set(&DataKey::UpdateVK(0), &vk_update_small);
        env.storage().persistent().set(&DataKey::UpdateVK(1), &vk_update_medium);
        env.storage().persistent().set(&DataKey::UpdateVK(2), &vk_update_large);

        for tier in 0u32..=2 {
            env.storage().persistent().extend_ttl(
                &DataKey::VK(tier),
                LEDGER_THRESHOLD,
                LEDGER_BUMP,
            );
            env.storage().persistent().extend_ttl(
                &DataKey::CreateVK(tier),
                LEDGER_THRESHOLD,
                LEDGER_BUMP,
            );
            env.storage().persistent().extend_ttl(
                &DataKey::UpdateVK(tier),
                LEDGER_THRESHOLD,
                LEDGER_BUMP,
            );
        }

        Ok(())
    }

    /// Rotate one of the three VK families at a tier. Admin-gated.
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
        let (key, expected_ic) = match kind {
            VkKind::Membership => (DataKey::VK(tier), MEMBERSHIP_IC_POINTS),
            VkKind::Create => (DataKey::CreateVK(tier), CREATE_IC_POINTS),
            VkKind::Update => (DataKey::UpdateVK(tier), UPDATE_IC_POINTS),
        };
        if new_vk.ic.len() != expected_ic {
            return Err(Error::InvalidVkLength);
        }
        validate_vk_points(&new_vk)?;

        env.storage().persistent().set(&key, &new_vk);
        env.storage()
            .persistent()
            .extend_ttl(&key, LEDGER_THRESHOLD, LEDGER_BUMP);
        Ok(())
    }

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

        RestrictedModeChanged {
            admin,
            restricted,
            timestamp: env.ledger().timestamp(),
        }
        .publish(&env);
        Ok(())
    }

    pub fn bump_group_ttl(env: Env, group_id: BytesN<32>) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        if !Self::group_exists(&env, &group_id) {
            return Err(Error::GroupNotFound);
        }
        Self::bump_group(&env, &group_id);
        Ok(())
    }

    /// Create a Tyranny group. Verifies a Create-circuit proof against
    /// `(commitment, epoch=0, admin_pubkey_commitment)`. The Create
    /// circuit's witness includes the admin's BLS secret key; the
    /// circuit constrains `Poseidon(admin_pubkey) ==
    /// admin_pubkey_commitment`.
    pub fn create_group(
        env: Env,
        caller: Address,
        group_id: BytesN<32>,
        commitment: BytesN<32>,
        tier: u32,
        admin_pubkey_commitment: BytesN<32>,
        proof: Groth16Proof,
        public_inputs: CreatePublicInputs,
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
        if public_inputs.commitment != commitment
            || public_inputs.epoch != 0
            || public_inputs.admin_pubkey_commitment != admin_pubkey_commitment
        {
            return Err(Error::PublicInputsMismatch);
        }
        if Self::group_exists(&env, &group_id) {
            return Err(Error::GroupAlreadyExists);
        }
        if !is_canonical_fr(&commitment) {
            return Err(Error::InvalidCommitmentEncoding);
        }
        if !is_canonical_fr(&admin_pubkey_commitment) {
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

        let vk = Self::load_create_vk(&env, tier)?;
        if !verify_create_proof(&env, &vk, &proof, &commitment, 0, &admin_pubkey_commitment) {
            return Err(Error::InvalidProof);
        }
        Self::record_proof(&env, &proof);

        let timestamp = env.ledger().timestamp();
        let entry = CommitmentEntry {
            commitment: commitment.clone(),
            epoch: 0,
            timestamp,
            tier,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Group(group_id.clone()), &entry);
        env.storage().persistent().set(
            &DataKey::AdminCommitment(group_id.clone()),
            &admin_pubkey_commitment,
        );
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
            admin_pubkey_commitment,
            tier,
            timestamp,
        }
        .publish(&env);
        Ok(())
    }

    /// Advance the group's commitment. The Update circuit's witness
    /// includes the admin's BLS secret key; the circuit verifies
    /// `Poseidon(admin_pubkey) == current.admin_pubkey_commitment`.
    /// `admin_pubkey_commitment` is contract-supplied from storage as
    /// the 4th public input — not on the wire.
    ///
    /// No `caller.require_auth()`: the Update proof IS the authorization.
    pub fn update_commitment(
        env: Env,
        group_id: BytesN<32>,
        proof: Groth16Proof,
        public_inputs: UpdatePublicInputs,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;

        let current = Self::load_group(&env, &group_id)?;

        let new_epoch = current.epoch.checked_add(1).ok_or(Error::InvalidEpoch)?;

        if public_inputs.c_old != current.commitment
            || public_inputs.epoch_old != current.epoch
        {
            return Err(Error::PublicInputsMismatch);
        }
        if !is_canonical_fr(&public_inputs.c_new) {
            return Err(Error::InvalidCommitmentEncoding);
        }

        Self::check_proof_replay(&env, &proof)?;

        // admin_pubkey_commitment is INVARIANT under update_commitment;
        // read from its dedicated storage slot, NOT from
        // CommitmentEntry (which doesn't carry it). update_commitment
        // never touches DataKey::AdminCommitment(group_id).
        let admin_commitment = Self::load_admin_commitment(&env, &group_id)?;

        let vk = Self::load_update_vk(&env, current.tier)?;
        if !verify_update_proof(
            &env,
            &vk,
            &proof,
            &public_inputs.c_old,
            public_inputs.epoch_old,
            &public_inputs.c_new,
            &admin_commitment,
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

    /// Read-only membership verification. Returns Ok(false) on bad
    /// proof rather than InvalidProof — read-only verifier semantics.
    pub fn verify_membership(
        env: Env,
        group_id: BytesN<32>,
        proof: Groth16Proof,
        public_inputs: MembershipPublicInputs,
    ) -> Result<bool, Error> {
        Self::require_initialized(&env)?;
        let state = Self::load_group(&env, &group_id)?;

        if public_inputs.commitment != state.commitment
            || public_inputs.epoch != state.epoch
        {
            return Err(Error::PublicInputsMismatch);
        }
        let vk = Self::load_membership_vk(&env, state.tier)?;
        Ok(verify_membership_proof(
            &env,
            &vk,
            &proof,
            &state.commitment,
            state.epoch,
        ))
    }

    pub fn get_commitment(
        env: Env,
        group_id: BytesN<32>,
    ) -> Result<CommitmentEntry, Error> {
        Self::require_initialized(&env)?;
        Self::load_group(&env, &group_id)
    }

    pub fn get_history(
        env: Env,
        group_id: BytesN<32>,
        max_entries: u32,
    ) -> Result<Vec<CommitmentEntry>, Error> {
        Self::require_initialized(&env)?;
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

    fn load_admin_commitment(env: &Env, group_id: &BytesN<32>) -> Result<BytesN<32>, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::AdminCommitment(group_id.clone()))
            .ok_or(Error::GroupNotFound)
    }

    fn group_exists(env: &Env, group_id: &BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::Group(group_id.clone()))
    }

    fn load_membership_vk(env: &Env, tier: u32) -> Result<VerificationKeyData, Error> {
        if tier > 2 {
            return Err(Error::InvalidTier);
        }
        env.storage()
            .persistent()
            .get(&DataKey::VK(tier))
            .ok_or(Error::NotInitialized)
    }

    fn load_create_vk(env: &Env, tier: u32) -> Result<VerificationKeyData, Error> {
        if tier > 2 {
            return Err(Error::InvalidTier);
        }
        env.storage()
            .persistent()
            .get(&DataKey::CreateVK(tier))
            .ok_or(Error::NotInitialized)
    }

    fn load_update_vk(env: &Env, tier: u32) -> Result<VerificationKeyData, Error> {
        if tier > 2 {
            return Err(Error::InvalidTier);
        }
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
            .has(&DataKey::AdminCommitment(group_id.clone()))
        {
            env.storage().persistent().extend_ttl(
                &DataKey::AdminCommitment(group_id.clone()),
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
        if env.storage().persistent().has(&DataKey::UsedProof(hash)) {
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

/// Pairing check for a Groth16 proof under a 5-IC layout:
///
///   IC[1] · in1 + IC[2] · in2 + IC[3] · in3 + IC[4] · in4   (+ IC[0] base)
///
/// Used by both the 4-IC Create-VK call (with `in4 = 0` packed into a
/// dummy slot — actually Create has 4 IC, so we need a separate
/// helper) and 5-IC Update-VK call. To keep verifiers shape-specific
/// and auditable, this file has three: `verify_membership_proof`
/// (3-IC, public inputs `(commitment, epoch)`), `verify_create_proof`
/// (4-IC, public inputs `(commitment, epoch, admin_pubkey_commitment)`),
/// and `verify_update_proof` (5-IC, public inputs `(c_old, epoch_old,
/// c_new, admin_pubkey_commitment)`).

/// Verify a 3-IC-point Membership proof. Public inputs:
///   IC[1] · commitment + IC[2] · epoch (+ IC[0] base).
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

/// Verify a 4-IC-point Create proof. Public inputs:
///   IC[1] · commitment + IC[2] · epoch + IC[3] · admin_pubkey_commitment (+ IC[0] base).
fn verify_create_proof(
    env: &Env,
    vk: &VerificationKeyData,
    proof: &Groth16Proof,
    commitment: &BytesN<32>,
    epoch: u64,
    admin_pubkey_commitment: &BytesN<32>,
) -> bool {
    if vk.ic.len() != CREATE_IC_POINTS {
        return false;
    }
    if !validate_proof_points(proof) {
        return false;
    }
    if !is_canonical_fr(commitment) {
        return false;
    }
    if !is_canonical_fr(admin_pubkey_commitment) {
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

    let commitment_fr = Fr::from_bytes(commitment.clone());
    let epoch_bytes = Bytes::from_array(env, &u64_to_u256_be(epoch));
    let epoch_fr = Fr::from_u256(U256::from_be_bytes(env, &epoch_bytes));
    let admin_fr = Fr::from_bytes(admin_pubkey_commitment.clone());

    let msm_points: Vec<G1Affine> = vec![env, ic1, ic2, ic3];
    let msm_scalars: Vec<Fr> = vec![env, commitment_fr, epoch_fr, admin_fr];
    let msm_result = bls.g1_msm(msm_points, msm_scalars);
    let vk_x = bls.g1_add(&ic0, &msm_result);

    let neg_a = -proof_a;
    let g1s: Vec<G1Affine> = vec![env, neg_a, alpha, vk_x, proof_c];
    let g2s: Vec<G2Affine> = vec![env, proof_b, beta, gamma, delta];

    bls.pairing_check(g1s, g2s)
}

/// Verify a 5-IC-point Update proof. Public inputs in canonical order:
///
///   IC[1] · c_old + IC[2] · epoch_old + IC[3] · c_new
///   + IC[4] · admin_pubkey_commitment   (+ IC[0] base)
///
/// `admin_pubkey_commitment` is contract-supplied (read from
/// `current.admin_pubkey_commitment`); the other three come from the
/// wire.
#[allow(clippy::too_many_arguments)]
fn verify_update_proof(
    env: &Env,
    vk: &VerificationKeyData,
    proof: &Groth16Proof,
    c_old: &BytesN<32>,
    epoch_old: u64,
    c_new: &BytesN<32>,
    admin_pubkey_commitment: &BytesN<32>,
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
    if !is_canonical_fr(admin_pubkey_commitment) {
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

    let c_old_fr = Fr::from_bytes(c_old.clone());
    let c_new_fr = Fr::from_bytes(c_new.clone());
    let epoch_bytes = Bytes::from_array(env, &u64_to_u256_be(epoch_old));
    let epoch_fr = Fr::from_u256(U256::from_be_bytes(env, &epoch_bytes));
    let admin_fr = Fr::from_bytes(admin_pubkey_commitment.clone());

    let msm_points: Vec<G1Affine> = vec![env, ic1, ic2, ic3, ic4];
    let msm_scalars: Vec<Fr> = vec![env, c_old_fr, epoch_fr, c_new_fr, admin_fr];
    let msm_result = bls.g1_msm(msm_points, msm_scalars);
    let vk_x = bls.g1_add(&ic0, &msm_result);

    let neg_a = -proof_a;
    let g1s: Vec<G1Affine> = vec![env, neg_a, alpha, vk_x, proof_c];
    let g2s: Vec<G2Affine> = vec![env, proof_b, beta, gamma, delta];

    bls.pairing_check(g1s, g2s)
}

#[cfg(test)]
mod test;
