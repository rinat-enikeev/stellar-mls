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
const HISTORY_WINDOW: u32 = 64;

/// Minimum TTL threshold for persistent storage (ledgers, ~1 day).
const LEDGER_THRESHOLD: u32 = 17_280;

/// TTL bump amount for persistent storage (ledgers, ~30 days).
const LEDGER_BUMP: u32 = 518_400;

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
    /// Caller is not the contract admin.
    Unauthorized = 3,
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

/// Groth16 verification key stored as raw bytes (contract-storage friendly).
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
    /// Verification key for tier 0/1/2 (persistent storage).
    VK(u32),
    /// Current group state (persistent storage).
    Group(BytesN<32>),
    /// Group history — rolling window of past entries (persistent storage).
    History(BytesN<32>),
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
    /// required for future admin operations. Each VK must have exactly
    /// 3 IC points (for the 2 public inputs: commitment and epoch).
    pub fn initialize(
        env: Env,
        admin: Address,
        vk_small: VerificationKeyData,
        vk_medium: VerificationKeyData,
        vk_large: VerificationKeyData,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();

        if vk_small.ic.len() != 3 || vk_medium.ic.len() != 3 || vk_large.ic.len() != 3 {
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

        for tier in 0..3u32 {
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::VK(tier), LEDGER_THRESHOLD, LEDGER_BUMP);
        }

        Ok(())
    }

    // ---- Group Operations ----

    /// Create a new private membership group.
    ///
    /// The proof must verify against `(commitment, epoch=0)` using the
    /// verification key for `tier`. This proves the caller is a member
    /// of the set being committed.
    pub fn create_group(
        env: Env,
        group_id: BytesN<32>,
        commitment: BytesN<32>,
        tier: u32,
        proof: Groth16Proof,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;

        if tier > 2 {
            return Err(Error::InvalidTier);
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::Group(group_id.clone()))
        {
            return Err(Error::GroupAlreadyExists);
        }

        let vk = Self::load_vk(&env, tier)?;
        if !verify_groth16_proof(&env, &vk, &proof, &commitment, 0) {
            return Err(Error::InvalidProof);
        }

        let timestamp = env.ledger().timestamp();
        let entry = CommitmentEntry {
            commitment: commitment.clone(),
            epoch: 0,
            timestamp,
            tier,
            active: true,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Group(group_id.clone()), &entry);
        env.storage().persistent().set(
            &DataKey::History(group_id.clone()),
            &Vec::<CommitmentEntry>::new(&env),
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

    /// Update a group's commitment (epoch transition).
    ///
    /// The proof is verified against the **current** commitment and epoch,
    /// proving the updater is a member *before* the transition. On success
    /// the group advances to `epoch + 1` with the new commitment.
    pub fn update_commitment(
        env: Env,
        group_id: BytesN<32>,
        new_commitment: BytesN<32>,
        proof: Groth16Proof,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;

        let current: CommitmentEntry = env
            .storage()
            .persistent()
            .get(&DataKey::Group(group_id.clone()))
            .ok_or(Error::GroupNotFound)?;

        if !current.active {
            return Err(Error::GroupInactive);
        }

        let vk = Self::load_vk(&env, current.tier)?;
        if !verify_groth16_proof(&env, &vk, &proof, &current.commitment, current.epoch) {
            return Err(Error::InvalidProof);
        }

        let new_epoch = current.epoch + 1;
        let timestamp = env.ledger().timestamp();

        Self::archive_entry(&env, &group_id, &current);

        let new_entry = CommitmentEntry {
            commitment: new_commitment.clone(),
            epoch: new_epoch,
            timestamp,
            tier: current.tier,
            active: true,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Group(group_id.clone()), &new_entry);
        Self::bump_group(&env, &group_id);

        CommitmentUpdated {
            group_id,
            commitment: new_commitment,
            epoch: new_epoch,
            timestamp,
        }
        .publish(&env);

        Ok(())
    }

    /// Verify a membership proof against the current group state.
    ///
    /// Read-only — does not modify contract state.
    pub fn verify_membership(
        env: Env,
        group_id: BytesN<32>,
        proof: Groth16Proof,
    ) -> Result<bool, Error> {
        Self::require_initialized(&env)?;

        let state: CommitmentEntry = env
            .storage()
            .persistent()
            .get(&DataKey::Group(group_id.clone()))
            .ok_or(Error::GroupNotFound)?;

        let vk = Self::load_vk(&env, state.tier)?;
        Ok(verify_groth16_proof(
            &env,
            &vk,
            &proof,
            &state.commitment,
            state.epoch,
        ))
    }

    /// Deactivate a group (requires membership proof).
    ///
    /// After deactivation `verify_membership` and `get_state` still work,
    /// but `update_commitment` is rejected. This is irreversible.
    pub fn deactivate_group(
        env: Env,
        group_id: BytesN<32>,
        proof: Groth16Proof,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;

        let current: CommitmentEntry = env
            .storage()
            .persistent()
            .get(&DataKey::Group(group_id.clone()))
            .ok_or(Error::GroupNotFound)?;

        if !current.active {
            return Err(Error::GroupInactive);
        }

        let vk = Self::load_vk(&env, current.tier)?;
        if !verify_groth16_proof(&env, &vk, &proof, &current.commitment, current.epoch) {
            return Err(Error::InvalidProof);
        }

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

        GroupDeactivated {
            group_id,
            final_epoch: current.epoch,
            timestamp,
        }
        .publish(&env);

        Ok(())
    }

    // ---- Queries ----

    /// Get the current state of a group.
    pub fn get_state(env: Env, group_id: BytesN<32>) -> Result<CommitmentEntry, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Group(group_id))
            .ok_or(Error::GroupNotFound)
    }

    /// Get the history of a group (most recent entries, up to 64).
    ///
    /// Full history is always available via contract events.
    pub fn get_history(env: Env, group_id: BytesN<32>) -> Result<Vec<CommitmentEntry>, Error> {
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Group(group_id.clone()))
        {
            return Err(Error::GroupNotFound);
        }
        Ok(env
            .storage()
            .persistent()
            .get(&DataKey::History(group_id))
            .unwrap_or(Vec::new(&env)))
    }

    // ---- Internal helpers ----

    fn require_initialized(env: &Env) -> Result<(), Error> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::NotInitialized);
        }
        Ok(())
    }

    fn load_vk(env: &Env, tier: u32) -> Result<VerificationKeyData, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::VK(tier))
            .ok_or(Error::NotInitialized)
    }

    fn bump_group(env: &Env, group_id: &BytesN<32>) {
        env.storage().persistent().extend_ttl(
            &DataKey::Group(group_id.clone()),
            LEDGER_THRESHOLD,
            LEDGER_BUMP,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::History(group_id.clone()),
            LEDGER_THRESHOLD,
            LEDGER_BUMP,
        );
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
// Groth16 Verification
// ================================================================

/// Verify a Groth16 proof using BLS12-381 host functions.
///
/// The standard Groth16 equation:
///   e(π_A, π_B) = e(α, β) · e(vk_x, γ) · e(π_C, δ)
///
/// is rewritten as a pairing check (product == 1_GT):
///   e(-π_A, π_B) · e(α, β) · e(vk_x, γ) · e(π_C, δ) = 1_GT
///
/// Steps:
///   1. Convert BytesN storage values to BLS12-381 SDK types
///   2. Compute vk_x = IC[0] + commitment·IC[1] + epoch·IC[2]
///   3. Negate π_A using the Neg trait (flips y-coordinate)
///   4. Run pairing_check with 4 pairs
fn verify_groth16_proof(
    env: &Env,
    vk: &VerificationKeyData,
    proof: &Groth16Proof,
    commitment: &BytesN<32>,
    epoch: u64,
) -> bool {
    let bls = env.crypto().bls12_381();

    // --- Convert storage bytes to BLS types ---
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

    // --- Public inputs as Fr scalars ---
    // Commitment: 32-byte big-endian field element
    let commitment_fr = Fr::from_bytes(commitment.clone());
    // Epoch: u64 → U256 → Fr
    let epoch_bytes = Bytes::from_array(env, &u64_to_u256_be(epoch));
    let epoch_fr = Fr::from_u256(U256::from_be_bytes(env, &epoch_bytes));

    // --- Compute vk_x = IC[0] + commitment·IC[1] + epoch·IC[2] ---
    let msm_points: Vec<G1Affine> = vec![env, ic1, ic2];
    let msm_scalars: Vec<Fr> = vec![env, commitment_fr, epoch_fr];
    let msm_result = bls.g1_msm(msm_points, msm_scalars);
    let vk_x = bls.g1_add(&ic0, &msm_result);

    // --- Negate π_A (flips y-coordinate via Neg trait) ---
    let neg_a = -proof_a;

    // --- Pairing check: e(-A, B) · e(α, β) · e(vk_x, γ) · e(C, δ) == 1_GT ---
    let g1s: Vec<G1Affine> = vec![env, neg_a, alpha, vk_x, proof_c];
    let g2s: Vec<G2Affine> = vec![env, proof_b, beta, gamma, delta];

    bls.pairing_check(g1s, g2s)
}

/// Convert a u64 to a 32-byte big-endian array (zero-padded for U256).
fn u64_to_u256_be(val: u64) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[24..32].copy_from_slice(&val.to_be_bytes());
    bytes
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

    /// Create a mock VK with zero-byte arrays. Not valid for BLS ops —
    /// only for testing contract logic paths that don't call the verifier.
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

    #[test]
    fn test_initialize() {
        let (env, client, admin) = setup_env();
        let vk = mock_vk(&env);
        client.initialize(&admin, &vk, &vk, &vk);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #2)")]
    fn test_double_initialize_rejected() {
        let (env, client, admin) = setup_env();
        let vk = mock_vk(&env);
        client.initialize(&admin, &vk, &vk, &vk);
        client.initialize(&admin, &vk, &vk, &vk);
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
            ic: vec![&env, g1.clone(), g1], // only 2 IC points
        };
        let good_vk = mock_vk(&env);

        client.initialize(&admin, &bad_vk, &good_vk, &good_vk);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #5)")]
    fn test_group_not_found() {
        let (env, client, admin) = setup_env();
        let vk = mock_vk(&env);
        client.initialize(&admin, &vk, &vk, &vk);

        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        client.get_state(&group_id);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #8)")]
    fn test_invalid_tier_rejected() {
        let (env, client, admin) = setup_env();
        let vk = mock_vk(&env);
        client.initialize(&admin, &vk, &vk, &vk);

        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        let proof = Groth16Proof {
            a: BytesN::from_array(&env, &[0u8; 96]),
            b: BytesN::from_array(&env, &[0u8; 192]),
            c: BytesN::from_array(&env, &[0u8; 96]),
        };

        client.create_group(&group_id, &commitment, &3u32, &proof);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1)")]
    fn test_not_initialized_rejected() {
        let (env, client, _admin) = setup_env();
        let group_id = BytesN::from_array(&env, &[1u8; 32]);
        let commitment = BytesN::from_array(&env, &[2u8; 32]);
        let proof = Groth16Proof {
            a: BytesN::from_array(&env, &[0u8; 96]),
            b: BytesN::from_array(&env, &[0u8; 192]),
            c: BytesN::from_array(&env, &[0u8; 96]),
        };

        client.create_group(&group_id, &commitment, &0u32, &proof);
    }

    // NOTE: Tests exercising actual Groth16 verification require valid
    // test vectors (VK, proof, public inputs) generated by the circuits
    // crate. Those belong in integration tests.
}
