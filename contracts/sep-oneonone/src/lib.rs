//! SEP 1v1 Soroban Contract — per-type immutable two-party group.
//!
//! Fourth in the per-type contract family (Anarchy, OneOnOne, Democracy,
//! Oligarchy, Tyranny). The smallest of the family:
//!
//!   * No `update_commitment` entrypoint — 1v1 groups are immutable.
//!   * No `deactivate_group` entrypoint — postmortem #153.
//!   * No tier parameter — tier hardcoded to 0 (Small) → depth=5.
//!   * Single Membership VK + single Create VK.
//!
//! Every value pinned in `contracts/sep-oneonone/test-vectors.json`
//! MUST match the constants and behaviors defined here. The
//! `test_vectors_consistency` inline test asserts the match.
//!
//! # Verification
//!
//! Both proof families verify via the shared `plonk-verifier` crate
//! against baked VKs (no admin rotation, no on-chain VK storage):
//!
//!   * **Membership** (`verify_membership`): the depth=5 anarchy
//!     membership VK with public inputs `(commitment, epoch)`. A 1v1
//!     group's commitment is byte-identical to what
//!     `MembershipCircuit` would produce against the same `(root,
//!     epoch=0, salt)` triple, so the same VK accepts both flows.
//!   * **Create** (`create_group`): a 1v1-specific VK that enforces
//!     "exactly 2 non-zero leaves at founding" inside the witness.
//!     Same 2-PI shape as Membership; different in-circuit constraints.
//!     See `circuit::plonk::oneonone_create` in the prover crate.
//!
//! VKs and the compressed `[τ]_2` are embedded via `include_bytes!`
//! from the `plonk-verifier` test fixtures. Rotating any VK requires
//! a contract redeploy.

#![no_std]
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype,
    crypto::bls12_381::Fr,
    Address, Bytes, BytesN, Env, Vec,
};

use plonk_verifier::proof_format::{parse_proof_bytes, FR_LEN, PROOF_LEN};
use plonk_verifier::verifier::verify as plonk_verify;
use plonk_verifier::vk_format::{parse_vk_bytes, G2_COMPRESSED_LEN};

// ================================================================
// Constants
// ================================================================

const LEDGER_THRESHOLD: u32 = 17_280;
const LEDGER_BUMP: u32 = 518_400;

/// Maximum number of groups this contract instance will ever create.
/// Monotonic increment-only since 1v1 has no deactivate path.
const MAX_GROUPS: u32 = 10_000;

/// Public-input count for both Membership and Create circuits.
const PI_COUNT: u32 = 2;

// ================================================================
// Embedded baked VKs + SRS-G2
// ================================================================

/// Membership VK shared with sep-anarchy at depth=5.
const MEMBERSHIP_VK: &[u8] =
    include_bytes!("../../plonk-verifier/tests/fixtures/vk-d5.bin");

/// 1v1-specific create VK. Enforces "exactly 2 non-zero leaves at
/// founding" inside the witness; same 2-PI shape as Membership.
const CREATE_VK: &[u8] =
    include_bytes!("../../plonk-verifier/tests/fixtures/oneonone-create-vk.bin");

const SRS_G2: &[u8; G2_COMPRESSED_LEN] =
    include_bytes!("../../plonk-verifier/tests/fixtures/srs-g2-compressed.bin");

// ================================================================
// Errors
// ================================================================

/// Numeric values pinned in `test-vectors.json` `error_codes.vectors`.
/// Codes 9 (`InvalidVkLength`) and 26 (`InvalidPoint`) are no longer
/// reachable post-PLONK migration but the numeric slots remain
/// reserved for cross-contract numbering alignment.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    GroupAlreadyExists = 4,
    GroupNotFound = 5,
    InvalidProof = 7,
    PublicInputsMismatch = 10,
    ProofReplay = 12,
    GroupCountLimitReached = 13,
    AdminOnly = 14,
    InvalidCommitmentEncoding = 15,
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

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentEntry {
    pub commitment: BytesN<32>,
    pub epoch: u64,
    pub timestamp: u64,
}

// ================================================================
// Storage Keys
// ================================================================

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    RestrictedMode,
    Group(BytesN<32>),
    UsedProof(BytesN<32>),
    GroupCount,
}

// ================================================================
// Contract
// ================================================================

#[contract]
pub struct SepOneOnOneContract;

#[contractimpl]
impl SepOneOnOneContract {
    /// One-time initialization. Constructor takes only the admin
    /// `Address` — VKs are baked.
    pub fn __constructor(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    /// Toggle restricted mode.
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

    /// Permissionless TTL bump.
    pub fn bump_group_ttl(env: Env, group_id: BytesN<32>) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        if !Self::group_exists(&env, &group_id) {
            return Err(Error::GroupNotFound);
        }
        env.storage().persistent().extend_ttl(
            &DataKey::Group(group_id),
            LEDGER_THRESHOLD,
            LEDGER_BUMP,
        );
        Ok(())
    }

    /// Create a 1v1 group at epoch 0.
    pub fn create_group(
        env: Env,
        caller: Address,
        group_id: BytesN<32>,
        commitment: BytesN<32>,
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

        if !is_canonical_fr(&commitment) {
            return Err(Error::InvalidCommitmentEncoding);
        }
        if public_inputs.len() != PI_COUNT {
            return Err(Error::PublicInputsMismatch);
        }
        if public_inputs.get(0).unwrap() != commitment {
            return Err(Error::PublicInputsMismatch);
        }
        if public_inputs.get(1).unwrap() != be32_from_u64(&env, 0) {
            return Err(Error::PublicInputsMismatch);
        }
        if Self::group_exists(&env, &group_id) {
            return Err(Error::GroupAlreadyExists);
        }

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::GroupCount)
            .unwrap_or(0);
        if count >= MAX_GROUPS {
            return Err(Error::GroupCountLimitReached);
        }

        Self::check_proof_replay(&env, &proof)?;
        verify_plonk_proof(&env, CREATE_VK, &proof, &public_inputs)?;
        Self::record_proof(&env, &proof);

        let timestamp = env.ledger().timestamp();
        let entry = CommitmentEntry {
            commitment: commitment.clone(),
            epoch: 0,
            timestamp,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Group(group_id.clone()), &entry);
        env.storage().persistent().extend_ttl(
            &DataKey::Group(group_id.clone()),
            LEDGER_THRESHOLD,
            LEDGER_BUMP,
        );
        env.storage()
            .instance()
            .set(&DataKey::GroupCount, &(count + 1));

        GroupCreated {
            group_id,
            commitment,
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
        proof: BytesN<1601>,
        public_inputs: Vec<BytesN<32>>,
    ) -> Result<bool, Error> {
        Self::require_initialized(&env)?;
        let state = Self::load_group(&env, &group_id)?;

        if public_inputs.len() != PI_COUNT {
            return Err(Error::PublicInputsMismatch);
        }
        if public_inputs.get(0).unwrap() != state.commitment {
            return Err(Error::PublicInputsMismatch);
        }
        if public_inputs.get(1).unwrap() != be32_from_u64(&env, state.epoch) {
            return Err(Error::PublicInputsMismatch);
        }
        match verify_plonk_proof(&env, MEMBERSHIP_VK, &proof, &public_inputs) {
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
}

// ================================================================
// PLONK verification glue
// ================================================================

const _: () = {
    assert!(PROOF_LEN == 1601, "plonk_verifier::PROOF_LEN drifted");
    assert!(FR_LEN == 32, "plonk_verifier::FR_LEN drifted");
    assert!(G2_COMPRESSED_LEN == 96, "plonk_verifier::G2_COMPRESSED_LEN drifted");
};

const MAX_PI_COUNT: usize = 2;

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
