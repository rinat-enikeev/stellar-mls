//! SEP Oligarchy Soroban Contract — PLONK migration.
//!
//! Per-type private group with hidden member + admin counts (combined
//! occupancy commitment) and configurable admin quorum threshold.
//!
//! ## Verification
//!
//!   * Membership VK (per tier): 2 PIs `(commitment, epoch)` —
//!     reuses anarchy's per-tier baked VKs.
//!   * Create VK: 6 PIs `(commitment, epoch=0,
//!     occupancy_commitment, member_root, admin_root, salt_initial)`.
//!   * Update VK: 6 PIs `(c_old, epoch_old, c_new,
//!     occupancy_commitment_old, occupancy_commitment_new,
//!     admin_threshold_numerator)`.
//!
//! `admin_threshold_numerator` is contract-supplied at update time
//! and never on the wire.
//!
//! ## Status — simplified initial port
//!
//! The PLONK ports at `circuit::plonk::oligarchy` preserve the
//! public-input shapes of the originals but reduce in-circuit
//! semantics to commitment binding. The full Groth16 reference
//! enforced K-of-N admin quorum + single-leaf delta + admin-tree
//! membership; those constraints are flagged for a follow-up PR.

#![no_std]
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype,
    crypto::bls12_381::Fr,
    Address, Bytes, BytesN, Env, Vec,
};

use plonk_verifier::proof_format::{parse_proof_bytes, FR_LEN, PROOF_LEN};
use plonk_verifier::verifier::verify as plonk_verify;
use plonk_verifier::vk_format::{parse_vk_bytes, G2_COMPRESSED_LEN};

const HISTORY_WINDOW: u32 = 64;
const LEDGER_THRESHOLD: u32 = 17_280;
const LEDGER_BUMP: u32 = 518_400;
const MAX_GROUPS_PER_TIER: u32 = 10_000;

const MEMBERSHIP_PI_COUNT: u32 = 2;
const CREATE_PI_COUNT: u32 = 6;
const UPDATE_PI_COUNT: u32 = 6;

#[cfg(test)]
fn tier_capacity(tier: u32) -> u32 {
    match tier {
        0 => 32,
        1 => 256,
        2 => 2048,
        _ => 0,
    }
}

// Per-tier oligarchy-specific membership VKs (issue #208). The
// standard `vk-d{N}.bin` files used by the other 4 contracts encode
// a 2-level commitment relation `c = H(H(root, epoch), salt)` that
// doesn't match what `synthesize_oligarchy_create` /
// `synthesize_oligarchy_update_quorum` store on-chain (3-level chain
// with the `H(occ, admin_root)` leg). The
// `oligarchy-membership-vk-d{N}.bin` files anchor the matching
// 3-level chain so honestly-created oligarchy groups can produce
// valid `verify_membership` proofs against their stored commitment.
const VK_MEMBERSHIP_D5: &[u8] =
    include_bytes!("../../plonk-verifier/tests/fixtures/oligarchy-membership-vk-d5.bin");
const VK_MEMBERSHIP_D8: &[u8] =
    include_bytes!("../../plonk-verifier/tests/fixtures/oligarchy-membership-vk-d8.bin");
const VK_MEMBERSHIP_D11: &[u8] =
    include_bytes!("../../plonk-verifier/tests/fixtures/oligarchy-membership-vk-d11.bin");

const VK_CREATE: &[u8] =
    include_bytes!("../../plonk-verifier/tests/fixtures/oligarchy-create-vk.bin");
const VK_UPDATE: &[u8] =
    include_bytes!("../../plonk-verifier/tests/fixtures/oligarchy-update-vk.bin");

const SRS_G2: &[u8; G2_COMPRESSED_LEN] =
    include_bytes!("../../plonk-verifier/tests/fixtures/srs-g2-compressed.bin");

fn membership_vk_for_tier(tier: u32) -> Option<&'static [u8]> {
    match tier {
        0 => Some(VK_MEMBERSHIP_D5),
        1 => Some(VK_MEMBERSHIP_D8),
        2 => Some(VK_MEMBERSHIP_D11),
        _ => None,
    }
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    GroupAlreadyExists = 4,
    GroupNotFound = 5,
    GroupInactive = 6,
    InvalidProof = 7,
    InvalidTier = 8,
    PublicInputsMismatch = 10,
    InvalidEpoch = 11,
    ProofReplay = 12,
    TierGroupLimitReached = 13,
    AdminOnly = 14,
    InvalidCommitmentEncoding = 15,
    InvalidThreshold = 28,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupCreated {
    #[topic]
    pub group_id: BytesN<32>,
    pub commitment: BytesN<32>,
    pub epoch: u64,
    pub tier: u32,
    pub occupancy_commitment: BytesN<32>,
    pub admin_threshold_numerator: u32,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentUpdated {
    #[topic]
    pub group_id: BytesN<32>,
    pub commitment: BytesN<32>,
    pub epoch: u64,
    pub occupancy_commitment: BytesN<32>,
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

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentEntry {
    pub commitment: BytesN<32>,
    pub epoch: u64,
    pub timestamp: u64,
    pub tier: u32,
    pub active: bool,
    pub occupancy_commitment: BytesN<32>,
    pub admin_threshold_numerator: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    RestrictedMode,
    Group(BytesN<32>),
    History(BytesN<32>),
    UsedProof(BytesN<32>),
    GroupCount(u32),
}

#[contract]
pub struct SepOligarchyContract;

#[contractimpl]
impl SepOligarchyContract {
    pub fn __constructor(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
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

    pub fn create_oligarchy_group(
        env: Env,
        caller: Address,
        group_id: BytesN<32>,
        commitment: BytesN<32>,
        member_tier: u32,
        admin_threshold_numerator: u32,
        occupancy_commitment_initial: BytesN<32>,
        proof: BytesN<1601>,
        public_inputs: Vec<BytesN<32>>,
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

        if member_tier > 2 {
            return Err(Error::InvalidTier);
        }
        if admin_threshold_numerator < 1 || admin_threshold_numerator > 100 {
            return Err(Error::InvalidThreshold);
        }
        if !is_canonical_fr(&commitment) {
            return Err(Error::InvalidCommitmentEncoding);
        }
        if !is_canonical_fr(&occupancy_commitment_initial) {
            return Err(Error::InvalidCommitmentEncoding);
        }
        if public_inputs.len() != CREATE_PI_COUNT {
            return Err(Error::PublicInputsMismatch);
        }
        if public_inputs.get(0).unwrap() != commitment {
            return Err(Error::PublicInputsMismatch);
        }
        if public_inputs.get(1).unwrap() != be32_from_u64(&env, 0) {
            return Err(Error::PublicInputsMismatch);
        }
        if public_inputs.get(2).unwrap() != occupancy_commitment_initial {
            return Err(Error::PublicInputsMismatch);
        }
        if Self::group_exists(&env, &group_id) {
            return Err(Error::GroupAlreadyExists);
        }

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::GroupCount(member_tier))
            .unwrap_or(0);
        if count >= MAX_GROUPS_PER_TIER {
            return Err(Error::TierGroupLimitReached);
        }

        Self::check_proof_replay(&env, &proof)?;
        verify_plonk_proof(&env, VK_CREATE, &proof, &public_inputs)?;
        Self::record_proof(&env, &proof);

        let timestamp = env.ledger().timestamp();
        let entry = CommitmentEntry {
            commitment: commitment.clone(),
            epoch: 0,
            timestamp,
            tier: member_tier,
            active: true,
            occupancy_commitment: occupancy_commitment_initial.clone(),
            admin_threshold_numerator,
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
            .set(&DataKey::GroupCount(member_tier), &(count + 1));
        Self::bump_group(&env, &group_id);

        GroupCreated {
            group_id,
            commitment,
            epoch: 0,
            tier: member_tier,
            occupancy_commitment: occupancy_commitment_initial,
            admin_threshold_numerator,
            timestamp,
        }
        .publish(&env);
        Ok(())
    }

    pub fn update_commitment(
        env: Env,
        group_id: BytesN<32>,
        proof: BytesN<1601>,
        public_inputs: Vec<BytesN<32>>,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        let current = Self::load_group(&env, &group_id)?;
        if !current.active {
            return Err(Error::GroupInactive);
        }
        let new_epoch = current.epoch.checked_add(1).ok_or(Error::InvalidEpoch)?;

        if public_inputs.len() != UPDATE_PI_COUNT {
            return Err(Error::PublicInputsMismatch);
        }
        let c_old = public_inputs.get(0).unwrap();
        let epoch_old_be = public_inputs.get(1).unwrap();
        let c_new = public_inputs.get(2).unwrap();
        let occ_old_pi = public_inputs.get(3).unwrap();
        let occ_new_pi = public_inputs.get(4).unwrap();
        let threshold_pi = public_inputs.get(5).unwrap();

        if c_old != current.commitment {
            return Err(Error::PublicInputsMismatch);
        }
        if epoch_old_be != be32_from_u64(&env, current.epoch) {
            return Err(Error::PublicInputsMismatch);
        }
        if !is_canonical_fr(&c_new) {
            return Err(Error::InvalidCommitmentEncoding);
        }
        if !is_canonical_fr(&occ_new_pi) {
            return Err(Error::InvalidCommitmentEncoding);
        }
        if occ_old_pi != current.occupancy_commitment {
            return Err(Error::PublicInputsMismatch);
        }
        if threshold_pi != be32_from_u64(&env, current.admin_threshold_numerator as u64) {
            return Err(Error::PublicInputsMismatch);
        }

        Self::check_proof_replay(&env, &proof)?;
        verify_plonk_proof(&env, VK_UPDATE, &proof, &public_inputs)?;
        Self::record_proof(&env, &proof);

        let timestamp = env.ledger().timestamp();
        Self::archive_entry(&env, &group_id, &current);

        let new_entry = CommitmentEntry {
            commitment: c_new.clone(),
            epoch: new_epoch,
            timestamp,
            tier: current.tier,
            active: true,
            occupancy_commitment: occ_new_pi.clone(),
            admin_threshold_numerator: current.admin_threshold_numerator,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Group(group_id.clone()), &new_entry);
        Self::bump_group(&env, &group_id);

        CommitmentUpdated {
            group_id,
            commitment: c_new,
            epoch: new_epoch,
            occupancy_commitment: occ_new_pi,
            timestamp,
        }
        .publish(&env);
        Ok(())
    }

    pub fn verify_membership(
        env: Env,
        group_id: BytesN<32>,
        proof: BytesN<1601>,
        public_inputs: Vec<BytesN<32>>,
    ) -> Result<bool, Error> {
        Self::require_initialized(&env)?;
        let state = Self::load_group(&env, &group_id)?;
        if public_inputs.len() != MEMBERSHIP_PI_COUNT {
            return Err(Error::PublicInputsMismatch);
        }
        if public_inputs.get(0).unwrap() != state.commitment {
            return Err(Error::PublicInputsMismatch);
        }
        if public_inputs.get(1).unwrap() != be32_from_u64(&env, state.epoch) {
            return Err(Error::PublicInputsMismatch);
        }
        let vk = membership_vk_for_tier(state.tier).ok_or(Error::InvalidTier)?;
        match verify_plonk_proof(&env, vk, &proof, &public_inputs) {
            Ok(()) => Ok(true),
            Err(Error::InvalidProof) => Ok(false),
            Err(other) => Err(other),
        }
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

    fn proof_hash(env: &Env, proof: &BytesN<1601>) -> BytesN<32> {
        let preimage = Bytes::from_slice(env, proof.to_array().as_slice());
        env.crypto().sha256(&preimage).into()
    }

    fn check_proof_replay(env: &Env, proof: &BytesN<1601>) -> Result<(), Error> {
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

    fn record_proof(env: &Env, proof: &BytesN<1601>) {
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

const _: () = {
    assert!(PROOF_LEN == 1601);
    assert!(FR_LEN == 32);
    assert!(G2_COMPRESSED_LEN == 96);
};

// Stack buffer width for `verify_plonk_proof`. Must dominate every
// per-circuit PI count this contract dispatches against.
const MAX_PI_COUNT: usize = 6;
const _: () = {
    assert!(MEMBERSHIP_PI_COUNT as usize <= MAX_PI_COUNT);
    assert!(CREATE_PI_COUNT as usize <= MAX_PI_COUNT);
    assert!(UPDATE_PI_COUNT as usize <= MAX_PI_COUNT);
};

fn verify_plonk_proof(
    env: &Env,
    vk_bytes: &[u8],
    proof: &BytesN<1601>,
    public_inputs: &Vec<BytesN<32>>,
) -> Result<(), Error> {
    let parsed_vk = parse_vk_bytes(vk_bytes).map_err(|_| Error::InvalidProof)?;
    let proof_array: [u8; PROOF_LEN] = proof.to_array();
    let parsed_proof = parse_proof_bytes(&proof_array).map_err(|_| Error::InvalidProof)?;
    let n = public_inputs.len() as usize;
    if n > MAX_PI_COUNT {
        return Err(Error::PublicInputsMismatch);
    }
    let mut pi_buf: [[u8; FR_LEN]; MAX_PI_COUNT] = [[0u8; FR_LEN]; MAX_PI_COUNT];
    for i in 0..n {
        pi_buf[i] = public_inputs.get(i as u32).unwrap().to_array();
    }
    plonk_verify(env, &parsed_vk, SRS_G2, &parsed_proof, &pi_buf[..n])
        .map_err(|_| Error::InvalidProof)
}

fn be32_from_u64(env: &Env, value: u64) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[24..32].copy_from_slice(&value.to_be_bytes());
    BytesN::from_array(env, &bytes)
}

fn is_canonical_fr(value: &BytesN<32>) -> bool {
    let fr = Fr::from_bytes(value.clone());
    let canonical: BytesN<32> = fr.to_bytes();
    canonical == *value
}

#[cfg(test)]
mod test;
