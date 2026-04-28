//! SEP 1v1 Soroban Contract — per-type immutable two-party group.
//!
//! Fourth in the per-type contract family (Anarchy, OneOnOne, Democracy,
//! Oligarchy). The smallest of the four:
//!
//!   * No `update_commitment` entrypoint — 1v1 groups are immutable.
//!   * No `deactivate_group` entrypoint — postmortem #153.
//!   * No tier parameter — tier hardcoded to 0 (Small).
//!   * Single Membership VK + single Create VK (not per-tier arrays).
//!
//! Every value pinned in `contracts/sep-oneonone/test-vectors.json`
//! MUST match the constants and behaviors defined here. The
//! `test_vectors_consistency` inline test asserts the match.
//!
//! # Verification
//!
//! Both Membership and Create VKs are 3-IC-point Groth16 verifying
//! keys with public inputs `(commitment, epoch)`. The Create circuit
//! enforces "exactly 2 non-zero leaves at founding" inside its
//! witness; the contract's verifier is shape-identical for both VKs.
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

/// Minimum TTL threshold for persistent storage (~1 day in ledgers).
const LEDGER_THRESHOLD: u32 = 17_280;

/// TTL bump amount for persistent storage (~30 days in ledgers).
const LEDGER_BUMP: u32 = 518_400;

/// Maximum number of groups this contract instance will ever create.
/// Monotonic increment-only since 1v1 has no deactivate path; the cap
/// is a one-way ratchet. See test-vectors.json `max_groups.operational_note`.
const MAX_GROUPS: u32 = 10_000;

/// IC point counts. Both VK families share the same shape: 3 IC points
/// (base + commitment + epoch). The 2-leaf invariant on Create is
/// enforced inside the witness, not in the public-input layout.
const MEMBERSHIP_IC_POINTS: u32 = 3;
const CREATE_IC_POINTS: u32 = 3;

// ================================================================
// Errors
// ================================================================

/// Numeric values pinned in `test-vectors.json` `error_codes.vectors`.
/// Number alignment with sibling per-type contracts is intentional:
/// `7 = InvalidProof` everywhere, `26 = InvalidPoint` everywhere, etc.
/// Gaps in the numbering (3, 6, 8, 11, 16-25, 27-30) are reserved by
/// sibling contracts for variants 1v1 doesn't need.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    GroupAlreadyExists = 4,
    GroupNotFound = 5,
    InvalidProof = 7,
    InvalidVkLength = 9,
    PublicInputsMismatch = 10,
    ProofReplay = 12,
    GroupCountLimitReached = 13,
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
    pub timestamp: u64,
}

/// Emitted when admin toggles restricted mode. Lets chain observers
/// detect mode flips without inspecting instance storage.
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

/// On-chain state of a 1v1 group. Smaller than sibling
/// contracts' CommitmentEntry: no `tier` (always 0), no `active`
/// (no deactivate path), no `member_count` (always 2). 1v1 groups
/// also never advance epoch, so `epoch` is always 0 — kept on the
/// struct only because off-chain consumers may want a uniform
/// "(commitment, epoch)" projection across contract types.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentEntry {
    pub commitment: BytesN<32>,
    pub epoch: u64,
    pub timestamp: u64,
}

/// Public inputs supplied to `create_group` and `verify_membership`.
/// Two scalars wired to IC points 1 and 2 (commitment, epoch).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicInputs {
    pub commitment: BytesN<32>,
    pub epoch: u64,
}

/// Selector for which VK family is being installed or rotated by
/// `update_vk`. No `Update` variant — 1v1 has no update circuit.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VkKind {
    /// Membership-circuit VK (3 IC points), used by `verify_membership`.
    Membership,
    /// OneOnOneCreate-circuit VK (3 IC points), used by `create_group`.
    /// Distinct from Membership because the Create circuit enforces
    /// the 2-leaf invariant in its witness; the public-input shape
    /// is the same.
    Create,
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
    /// Membership-circuit VK — single VK, no per-tier array (persistent).
    MembershipVK,
    /// Create-circuit VK — single VK, no per-tier array (persistent).
    CreateVK,
    /// Current group state — immutable after create (persistent).
    Group(BytesN<32>),
    /// Used proof hashes — global nullifier set (persistent, TTL bounded).
    UsedProof(BytesN<32>),
    /// Active group count — single counter, no per-tier array (instance).
    GroupCount,
}

// ================================================================
// Contract
// ================================================================

#[contract]
pub struct SepOneOnOneContract;

#[contractimpl]
impl SepOneOnOneContract {
    /// One-time initialization. Pins admin + the two VKs (Membership +
    /// Create). Both VKs MUST have exactly 3 IC points; small-subgroup
    /// validation runs on every G1/G2 point.
    pub fn __constructor(
        env: Env,
        admin: Address,
        vk_membership: VerificationKeyData,
        vk_create: VerificationKeyData,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();

        if vk_membership.ic.len() != MEMBERSHIP_IC_POINTS {
            return Err(Error::InvalidVkLength);
        }
        validate_vk_points(&vk_membership)?;
        if vk_create.ic.len() != CREATE_IC_POINTS {
            return Err(Error::InvalidVkLength);
        }
        validate_vk_points(&vk_create)?;

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&DataKey::MembershipVK, &vk_membership);
        env.storage()
            .persistent()
            .set(&DataKey::CreateVK, &vk_create);
        env.storage().persistent().extend_ttl(
            &DataKey::MembershipVK,
            LEDGER_THRESHOLD,
            LEDGER_BUMP,
        );
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::CreateVK, LEDGER_THRESHOLD, LEDGER_BUMP);

        Ok(())
    }

    /// Rotate one of the two VKs. Admin-gated. No `tier` parameter —
    /// tier is hardcoded to 0.
    pub fn update_vk(
        env: Env,
        kind: VkKind,
        new_vk: VerificationKeyData,
    ) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let (key, expected_ic) = match kind {
            VkKind::Membership => (DataKey::MembershipVK, MEMBERSHIP_IC_POINTS),
            VkKind::Create => (DataKey::CreateVK, CREATE_IC_POINTS),
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

    /// Toggle restricted mode. When `true`, `create_group` rejects
    /// non-admin callers with `AdminOnly`. Default `false`.
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

    /// Permissionless TTL bump. The only ongoing lifecycle event for
    /// a 1v1 group: stops the entry from aging out of persistent
    /// storage. Bumps Group TTL only — UsedProof entries are not
    /// per-group-keyed and are not bumped here.
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

    /// Create a 1v1 group. Verifies a Create-circuit proof against
    /// `(commitment, epoch=0)`. The Create circuit enforces "exactly
    /// 2 non-zero leaves" inside its witness; the contract sees only
    /// the standard 3-IC public-input shape.
    pub fn create_group(
        env: Env,
        caller: Address,
        group_id: BytesN<32>,
        commitment: BytesN<32>,
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

        if public_inputs.commitment != commitment || public_inputs.epoch != 0 {
            return Err(Error::PublicInputsMismatch);
        }
        if Self::group_exists(&env, &group_id) {
            return Err(Error::GroupAlreadyExists);
        }
        if !is_canonical_fr(&commitment) {
            return Err(Error::InvalidCommitmentEncoding);
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

        let vk: VerificationKeyData = env
            .storage()
            .persistent()
            .get(&DataKey::CreateVK)
            .ok_or(Error::NotInitialized)?;
        if !verify_proof(&env, &vk, CREATE_IC_POINTS, &proof, &commitment, 0) {
            return Err(Error::InvalidProof);
        }

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

    /// Verify a membership proof. Read-only; returns `Ok(false)` on
    /// invalid proof rather than `InvalidProof` (read-only verifier
    /// semantics, parallel to sibling contracts).
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

        let vk: VerificationKeyData = env
            .storage()
            .persistent()
            .get(&DataKey::MembershipVK)
            .ok_or(Error::NotInitialized)?;
        Ok(verify_proof(
            &env,
            &vk,
            MEMBERSHIP_IC_POINTS,
            &proof,
            &state.commitment,
            state.epoch,
        ))
    }

    /// Read-only state lookup.
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

/// Verify a 3-IC-point proof against `(commitment, epoch)` public
/// inputs. Used for both Membership (at `verify_membership`) and
/// Create (at `create_group`) — the public-input shape is identical;
/// the caller passes the appropriate VK and asserts its expected IC
/// count via `expected_ic_count` so a future divergence between the
/// two VK families surfaces here as a verification failure rather
/// than a silent mismatch.
///
/// Pairing check: `e(-π_A, π_B) · e(α, β) · e(vk_x, γ) · e(π_C, δ) = 1_GT`.
fn verify_proof(
    env: &Env,
    vk: &VerificationKeyData,
    expected_ic_count: u32,
    proof: &Groth16Proof,
    commitment: &BytesN<32>,
    epoch: u64,
) -> bool {
    if vk.ic.len() != expected_ic_count {
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

#[cfg(test)]
mod test;
