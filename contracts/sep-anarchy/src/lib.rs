//! SEP Anarchy Soroban Contract — per-type single-signer membership group.
//!
//! This is the **per-type Anarchy** contract, one of an eventual four
//! (Anarchy, OneOnOne, Democracy, Oligarchy) in separate crates. The
//! `group_type` discriminator is implicit in the contract address;
//! there is no polymorphic dispatch and no group_type field in storage.
//!
//! Anarchy is the protocol's null case: no quorum threshold, no admin
//! set, no occupancy commitment. `member_count` on storage is
//! informational only — supplied at create_group, never updated by
//! the contract. Operators who don't want to publish a count pass `0`
//! (the v1 sep-xxxx "not tracked" sentinel).
//!
//! Every value pinned in `contracts/sep-anarchy/test-vectors.json`
//! MUST match the constants and behaviors defined here. The
//! `test_vectors_consistency` inline test asserts the match at build
//! time.
//!
//! # Verification
//!
//! Membership proofs use a 3-IC-point Groth16 verifying key with
//! public inputs `(commitment, epoch)`. Update proofs use a 4-IC-point
//! Groth16 verifying key with public inputs `(c_old, epoch_old,
//! c_new)` per the existing v2 keyset Anarchy circuit. Smaller than
//! Democracy's / Oligarchy's 7-IC update VK (no contract-supplied
//! threshold input).
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
const UPDATE_IC_POINTS: u32 = 4;

/// Number of slot positions a Merkle tree of the given tier can hold.
/// * tier 0 (Small)  — depth 5  → 32
/// * tier 1 (Medium) — depth 8  → 256
/// * tier 2 (Large)  — depth 11 → 2048
///
/// Anarchy is value-agnostic to the bitmap occupancy (member_count is
/// informational, not enforced). This function is preserved as the
/// canonical source of truth for the `tier_capacity` test-vector pin
/// (`test_vectors_consistency`).
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
/// Future contracts in this family (OneOnOne) MUST keep this numbering
/// disjoint where overlap would confuse cross-contract clients.
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
    // 16 (UnknownVkKind) elided — VkKind exhaustively matched.
    InvalidPoint = 26,
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
    pub member_count: u32,
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

/// Emitted when admin toggles restricted mode. Lets chain observers
/// detect mode flips without inspecting instance storage. Same shape
/// as sep-oligarchy's `RestrictedModeChanged`.
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

/// On-chain state of an Anarchy group at a particular epoch.
///
/// Differences from `sep-xxxx`'s `CommitmentEntryV2`:
///   * No `group_type` field — single-type contract.
///   * No `occupancy_commitment` field — Anarchy doesn't hide counts.
///   * `member_count` preserved as informational (per design §3.3).
///     The contract NEVER updates this field after create.
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
    /// Informational member count (per v1 sep-xxxx; sentinel `0`
    /// means "not tracked"). The contract is value-agnostic to this
    /// field; it is set at `create_group` and never mutated.
    pub member_count: u32,
}

/// Public inputs supplied to `create_group`, `verify_membership`, and
/// `deactivate_group`. Two scalars wired to `Membership` IC points 1
/// and 2.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicInputs {
    pub commitment: BytesN<32>,
    pub epoch: u64,
}

/// Wire payload for `update_commitment`. Three wire-supplied scalars
/// — Anarchy's natural shape, smaller than Democracy/Oligarchy's 5.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateCommitmentPublicInputs {
    pub c_old: BytesN<32>,
    pub epoch_old: u64,
    pub c_new: BytesN<32>,
}

/// Selector for which VK family is being installed or rotated by
/// `update_vk`. No `UpdateByType(u32)` because the contract is
/// Anarchy-only — group_type is implicit in the contract address.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VkKind {
    /// Membership-circuit VK (3 IC points).
    Membership,
    /// Anarchy update-circuit VK (4 IC points).
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
    /// Anarchy update-circuit VK per tier (persistent). No
    /// `(tier, group_type)` second key — single-type contract.
    UpdateVK(u32),
    /// Current group state (persistent).
    Group(BytesN<32>),
    /// Group history — rolling window (persistent).
    History(BytesN<32>),
    /// Used proof hashes — global nullifier set (persistent, TTL bounded).
    UsedProof(BytesN<32>),
    /// Active group count per tier (instance, MAX_GROUPS_PER_TIER limit).
    GroupCount(u32),
}

// ================================================================
// Contract
// ================================================================

#[contract]
pub struct SepAnarchyContract;

#[contractimpl]
impl SepAnarchyContract {
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

    /// Admin-only VK rotation. Membership VK at `tier` ∈ {0,1,2} OR
    /// Update VK at `tier` ∈ {0,1,2}.
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
    /// only the admin may call `create_group`. Emits
    /// `RestrictedModeChanged` for audit transparency.
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

        let timestamp = env.ledger().timestamp();
        RestrictedModeChanged {
            admin,
            restricted,
            timestamp,
        }
        .publish(&env);
        Ok(())
    }

    /// Permissionless TTL bump for a group's persistent storage.
    ///
    /// Bumps `Group(group_id)` and `History(group_id)` only — does NOT
    /// touch `UsedProof(...)` entries (global-nullifier scope, only
    /// refreshed by `record_proof` at successful state-changing
    /// entrypoints).
    pub fn bump_group_ttl(env: Env, group_id: BytesN<32>) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        if !Self::group_exists(&env, &group_id) {
            return Err(Error::GroupNotFound);
        }
        Self::bump_group(&env, &group_id);
        Ok(())
    }

    // ---- Group lifecycle ----

    /// Create a new Anarchy group at epoch 0.
    ///
    /// Validates: tier ≤ 2, group_id unused, `commitment` is canonical
    /// Fr, public inputs match the proof, proof verifies under the
    /// membership VK at `tier`, proof not replayed, tier capacity not
    /// exceeded. `member_count` is informational and accepted without
    /// validation (any u32; sentinel `0` means "not tracked").
    pub fn create_group(
        env: Env,
        caller: Address,
        group_id: BytesN<32>,
        commitment: BytesN<32>,
        tier: u32,
        member_count: u32,
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
            member_count,
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
            member_count,
            timestamp,
        }
        .publish(&env);
        Ok(())
    }

    /// Advance an Anarchy group to the next epoch.
    ///
    /// The verifier consumes 3 Groth16 public inputs in canonical
    /// order from the wire payload. No contract-supplied input
    /// (Anarchy has no quorum threshold).
    ///
    /// No `caller.require_auth()` — the membership Groth16 proof IS
    /// the authorization (the prover demonstrated knowledge of a
    /// secret key behind a member leaf at `c_old`). Any address can
    /// submit on behalf of the group; the proof carries the auth.
    /// Same convention as `sep-democracy` / `sep-oligarchy`.
    ///
    /// `member_count` is NOT updated by this entrypoint. The contract
    /// has no way to recompute it (no Poseidon host) and clients track
    /// it off-chain anyway.
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
        {
            return Err(Error::PublicInputsMismatch);
        }

        if !is_canonical_fr(&public_inputs.c_new) {
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
            // member_count is informational; the contract preserves
            // whatever was set at create. Clients track the actual
            // count off-chain.
            member_count: current.member_count,
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
    ///
    /// No `check_proof_replay` — verify is read-only and does not
    /// consume the global nullifier; the same proof bytes can be
    /// re-submitted indefinitely without burning `UsedProof` storage.
    /// Same convention as sep-democracy / sep-oligarchy.
    ///
    /// Does NOT check `state.active` — post-deactivation attestations
    /// against the frozen final state remain verifiable forever (a
    /// chain observer who saved the final pre-deactivation state can
    /// always re-prove membership).
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
        // Archive the final active state to history before flipping
        // the live entry inactive — mirrors the update_commitment
        // pattern (history holds prior states; current state is in
        // Group). Without this the freeze snapshot is lost.
        Self::archive_entry(&env, &group_id, &current);
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
        if tier > 2 {
            return Err(Error::InvalidTier);
        }
        env.storage()
            .persistent()
            .get(&DataKey::VK(tier))
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

/// Verify a 4-IC-point Anarchy update proof. Public inputs in
/// canonical order:
///
/// ```
/// IC[1] · c_old + IC[2] · epoch_old + IC[3] · c_new   (plus IC[0] base)
/// ```
///
/// All three inputs come from the wire. No contract-supplied scalar
/// (unlike Democracy/Oligarchy which supply admin_threshold from
/// storage).
fn verify_update_proof(
    env: &Env,
    vk: &VerificationKeyData,
    proof: &Groth16Proof,
    c_old: &BytesN<32>,
    epoch_old: u64,
    c_new: &BytesN<32>,
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

    let c_old_fr = Fr::from_bytes(c_old.clone());
    let c_new_fr = Fr::from_bytes(c_new.clone());
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

#[cfg(test)]
mod test;
