//! SEP Oligarchy Soroban Contract — per-type private group with hidden
//! member + admin counts via salted combined occupancy commitment, and
//! configurable admin quorum threshold.
//!
//! This is the **per-type Oligarchy** contract, one of an eventual four
//! (Anarchy, OneOnOne, Democracy, Oligarchy) in separate crates. The
//! `group_type` discriminator is implicit in the contract address;
//! there is no polymorphic dispatch and no group_type field in storage.
//!
//! Every value pinned in `contracts/sep-oligarchy/test-vectors.json`
//! MUST match the constants and behaviors defined here. The
//! `test_vectors_consistency` inline test asserts the match at build
//! time.
//!
//! # Verification
//!
//! Membership proofs use a 3-IC-point Groth16 verifying key with
//! public inputs `(commitment, epoch)` and open against the **member
//! tree only** — chat-capacity proof, parallel to sep-democracy. Admin
//! authorization for state changes is done exclusively via the update
//! VK's K-signer subset.
//!
//! Create proofs use a 7-IC-point Groth16 verifying key with public
//! inputs `(commitment, 0, occupancy_commitment, member_root,
//! admin_root, salt_initial)`. The verbose binding closes the
//! create-time self-DoS sep-democracy still carries (a malicious
//! creator cannot supply a bogus `occupancy_commitment` because the
//! proof binds it). Per design v0.1.4 §4.8.
//!
//! Update proofs use a 7-IC-point Groth16 verifying key with public
//! inputs `(c_old, epoch_old, c_new, occupancy_commitment_old,
//! occupancy_commitment_new, admin_threshold_numerator)` per the
//! v0.1.4 oligarchy circuit (`docs/oligarchy-update-testnet-design.md`
//! §4.7.3, §4.7.6). The `admin_threshold_numerator` is supplied by the
//! contract from `current.admin_threshold_numerator` storage at verify
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

/// Maximum number of active groups allowed per member tier.
const MAX_GROUPS_PER_TIER: u32 = 10_000;

/// IC point counts for the three VK families this contract verifies.
///
/// MUST match `test-vectors.json` `vk_kind_enum.*.ic_count`.
const MEMBERSHIP_IC_POINTS: u32 = 3;
const CREATE_IC_POINTS: u32 = 7;
const UPDATE_IC_POINTS: u32 = 7;

/// Number of slot positions a member-tree of the given tier can hold.
/// * tier 0 (Small)  — depth 5  → 32
/// * tier 1 (Medium) — depth 8  → 256
/// * tier 2 (Large)  — depth 11 → 2048
///
/// Per design §4.7.2 / §4.7.3 the v0.1.4 contract no longer enforces a
/// per-tier `member_count` ceiling at runtime — the bitmap occupancy
/// commitment internalizes the bound. This function is preserved as
/// the canonical source of truth for the `tier_capacity` test-vector
/// pin (`test_vectors_consistency`).
///
/// NOTE: this is the MEMBER tier capacity. The admin tier is fixed at
/// Small (depth=5, 32 slots) per design §4.6 across all member tiers
/// and is NOT a contract argument.
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
/// Future contracts in this family (Anarchy / OneOnOne) MUST keep this
/// numbering disjoint where overlap would confuse cross-contract
/// clients.
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
    // Error code 16 (UnknownVkKind) is unreachable: VkKind is
    // exhaustively matched in update_vk. Same elision as
    // sep-democracy.
    InvalidPoint = 26,
    GroupStillActive = 27,
    /// `create_oligarchy_group` rejects `admin_threshold_numerator < 1
    /// || > 100` (design v0.1.4 §4.7.6).
    InvalidThreshold = 28,
    /// Reserved for the in-circuit floor `popcount(member_bitmap_initial)
    /// >= 1 && popcount(admin_bitmap_initial) >= 1` per §4.8.
    /// NOT contract-enforced — the create proof's circuit binds bitmaps
    /// to roots and rejects bogus initial states; this code is reserved
    /// for ABI discoverability so cross-platform clients know it exists.
    #[allow(dead_code)]
    InvalidInitialMembership = 30,
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


/// Emitted when admin toggles restricted mode. Lets chain observers
/// detect mode flips without inspecting instance storage. Per PR #149
/// review (releaseng chunk 1 note #2) — audit transparency.
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

/// On-chain state of an Oligarchy group at a particular epoch.
///
/// Differences from `sep-xxxx`'s `CommitmentEntryV2`:
///   * No `group_type` field — single-type contract.
///   * No `member_count` / `admin_count` fields — replaced by combined
///     `occupancy_commitment` (design §3.5 hidden-counts requirement).
///   * No `admin_root` field — v0.1.3 §3.5 fix; admin_root is a
///     circuit-internal private witness reconstructed off-chain.
///   * Adds `admin_threshold_numerator: u32` (design §4.7.6).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentEntry {
    /// Poseidon bundled-roots commitment (BLS12-381 field element,
    /// 32 bytes BE, canonical Fr encoding) per §4.7.2.
    pub commitment: BytesN<32>,
    /// Epoch counter (starts at 0, increments by 1 per successful
    /// `update_commitment`).
    pub epoch: u64,
    /// Ledger timestamp when this state was recorded.
    pub timestamp: u64,
    /// Member tier: 0 = Small, 1 = Medium, 2 = Large. Fixed at
    /// `create_oligarchy_group`. Admin tier is fixed at Small per
    /// design §4.6 and NOT stored.
    pub tier: u32,
    /// Whether the group accepts further updates.
    pub active: bool,
    /// Salted combined Poseidon commitment over (member_bitmap,
    /// admin_bitmap, salt_occ) per §4.7.2 v0.1.4. Replaces v1's
    /// member_count + admin_count + admin_root and hides the counts
    /// + which-tree-changed from chain observers.
    pub occupancy_commitment: BytesN<32>,
    /// Admin quorum threshold as integer percentage in [1, 100]
    /// (design §4.7.6). Fixed at `create_oligarchy_group`; never
    /// mutated by `update_commitment`.
    pub admin_threshold_numerator: u32,
}

/// Public inputs supplied to `verify_membership`.
///
/// Two scalars wired to `Membership` IC points 1 and 2.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicInputs {
    pub commitment: BytesN<32>,
    pub epoch: u64,
}

/// Public inputs supplied to `create_oligarchy_group`. Verbose v0.1.4
/// §4.8 binding: contract receives all six inputs explicitly so the
/// create proof binds member_root, admin_root, salt_initial,
/// occupancy_commitment, commitment, and epoch=0 — closing the
/// create-time self-DoS sep-democracy still carries.
///
/// Six scalars wired to `Create` IC points 1..=6.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatePublicInputs {
    pub commitment: BytesN<32>,
    pub epoch: u64,
    pub occupancy_commitment: BytesN<32>,
    pub member_root: BytesN<32>,
    pub admin_root: BytesN<32>,
    pub salt_initial: BytesN<32>,
}

/// Wire payload for `update_commitment` per design §4.4 / §4.7.3.
///
/// Five wire-supplied scalars. The 6th Groth16 public input
/// (`admin_threshold_numerator`) is **NOT** on the wire — the contract
/// reads it from `current.admin_threshold_numerator` at verify time
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
/// Oligarchy-only — group_type is implicit in the contract address.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VkKind {
    /// Membership-circuit VK (3 IC points). Member-tree opening only.
    Membership,
    /// v0.1.4 oligarchy create-circuit VK (7 IC points).
    Create,
    /// v0.1.4 oligarchy update-circuit VK (7 IC points).
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
    /// Whether only admin can `create_oligarchy_group` (instance storage).
    RestrictedMode,
    /// Membership-circuit VK per member tier (persistent).
    VK(u32),
    /// v0.1.4 oligarchy create-circuit VK per member tier (persistent).
    CreateVK(u32),
    /// v0.1.4 oligarchy update-circuit VK per member tier (persistent).
    UpdateVK(u32),
    /// Current group state (persistent).
    Group(BytesN<32>),
    /// Group history — rolling window (persistent).
    History(BytesN<32>),
    /// Used proof hashes — global nullifier set (persistent, TTL bounded).
    UsedProof(BytesN<32>),
    /// Active group count per member tier (instance, MAX_GROUPS_PER_TIER limit).
    GroupCount(u32),
}

// ================================================================
// Contract
// ================================================================

#[contract]
pub struct SepOligarchyContract;

#[contractimpl]
impl SepOligarchyContract {
    // ---- Initialization ----

    /// Atomic constructor: takes admin and the per-tier VKs for the
    /// membership, create, and update circuits. Runs at deploy time.
    #[allow(clippy::too_many_arguments)]
    pub fn __constructor(
        env: Env,
        admin: Address,
        vk_small: VerificationKeyData,
        vk_medium: VerificationKeyData,
        vk_large: VerificationKeyData,
        create_vk_small: VerificationKeyData,
        create_vk_medium: VerificationKeyData,
        create_vk_large: VerificationKeyData,
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
            create_vk_small,
            create_vk_medium,
            create_vk_large,
            update_vk_small,
            update_vk_medium,
            update_vk_large,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn do_initialize(
        env: &Env,
        admin: Address,
        vk_small: VerificationKeyData,
        vk_medium: VerificationKeyData,
        vk_large: VerificationKeyData,
        create_vk_small: VerificationKeyData,
        create_vk_medium: VerificationKeyData,
        create_vk_large: VerificationKeyData,
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
        let c = CREATE_IC_POINTS;
        if create_vk_small.ic.len() != c
            || create_vk_medium.ic.len() != c
            || create_vk_large.ic.len() != c
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
        validate_vk_points(&create_vk_small)?;
        validate_vk_points(&create_vk_medium)?;
        validate_vk_points(&create_vk_large)?;
        validate_vk_points(&update_vk_small)?;
        validate_vk_points(&update_vk_medium)?;
        validate_vk_points(&update_vk_large)?;

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().persistent().set(&DataKey::VK(0), &vk_small);
        env.storage().persistent().set(&DataKey::VK(1), &vk_medium);
        env.storage().persistent().set(&DataKey::VK(2), &vk_large);
        env.storage()
            .persistent()
            .set(&DataKey::CreateVK(0), &create_vk_small);
        env.storage()
            .persistent()
            .set(&DataKey::CreateVK(1), &create_vk_medium);
        env.storage()
            .persistent()
            .set(&DataKey::CreateVK(2), &create_vk_large);
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

    // ---- Admin ----

    /// Admin-only VK rotation, per design §4.6 fingerprint-coordination
    /// triplet. Membership / Create / Update VK at `tier` ∈ {0,1,2}.
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
            VkKind::Create => (DataKey::CreateVK(tier), CREATE_IC_POINTS),
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
    /// only the admin may call `create_oligarchy_group`. Emits
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
    /// touch `UsedProof(...)` entries. The global-nullifier set is
    /// keyed by proof-bytes hash with contract-global scope; bumping
    /// it requires recording a fresh proof via a state-changing
    /// entrypoint.
    pub fn bump_group_ttl(env: Env, group_id: BytesN<32>) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        if !Self::group_exists(&env, &group_id) {
            return Err(Error::GroupNotFound);
        }
        Self::bump_group(&env, &group_id);
        Ok(())
    }

    // ---- Group lifecycle ----

    /// Create a new Oligarchy group at epoch 0.
    ///
    /// Validates: member_tier ≤ 2, admin_threshold_numerator ∈ [1, 100],
    /// group_id unused, all five wire-supplied scalars are canonical Fr,
    /// public inputs match the proof, proof verifies under the create
    /// VK at `member_tier`, proof not replayed, tier capacity not
    /// exceeded.
    ///
    /// Per design v0.1.4 §4.8 verbose binding: the create proof binds
    /// commitment, occupancy_commitment, member_root, admin_root, and
    /// salt_initial as public inputs. salt_occ_initial is bundled into
    /// occupancy_commitment per §4.7.2 v0.1.4 and consumed inside the
    /// circuit (not a separate IC point).
    #[allow(clippy::too_many_arguments)]
    pub fn create_oligarchy_group(
        env: Env,
        caller: Address,
        group_id: BytesN<32>,
        commitment: BytesN<32>,
        member_tier: u32,
        admin_threshold_numerator: u32,
        occupancy_commitment_initial: BytesN<32>,
        member_root: BytesN<32>,
        admin_root: BytesN<32>,
        salt_initial: BytesN<32>,
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

        if member_tier > 2 {
            return Err(Error::InvalidTier);
        }
        if admin_threshold_numerator < 1 || admin_threshold_numerator > 100 {
            return Err(Error::InvalidThreshold);
        }
        if public_inputs.commitment != commitment
            || public_inputs.epoch != 0
            || public_inputs.occupancy_commitment != occupancy_commitment_initial
            || public_inputs.member_root != member_root
            || public_inputs.admin_root != admin_root
            || public_inputs.salt_initial != salt_initial
        {
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
        if !is_canonical_fr(&member_root) {
            return Err(Error::InvalidCommitmentEncoding);
        }
        if !is_canonical_fr(&admin_root) {
            return Err(Error::InvalidCommitmentEncoding);
        }
        if !is_canonical_fr(&salt_initial) {
            return Err(Error::InvalidCommitmentEncoding);
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

        let vk = Self::load_create_vk(&env, member_tier)?;
        if !verify_create_proof(
            &env,
            &vk,
            &proof,
            &commitment,
            0,
            &occupancy_commitment_initial,
            &member_root,
            &admin_root,
            &salt_initial,
        ) {
            return Err(Error::InvalidProof);
        }

        Self::record_proof(&env, &proof);

        let timestamp = env.ledger().timestamp();
        let entry = CommitmentEntry {
            commitment: commitment.clone(),
            epoch: 0,
            timestamp,
            tier: member_tier,
            active: true,
            occupancy_commitment: occupancy_commitment_initial,
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
            timestamp,
        }
        .publish(&env);
        Ok(())
    }

    /// Advance an Oligarchy group to the next epoch.
    ///
    /// The verifier consumes 6 Groth16 public inputs in canonical
    /// order: 5 from the wire payload, 1 (`admin_threshold_numerator`)
    /// from `current.admin_threshold_numerator` per design §4.7.6.
    ///
    /// No `caller.require_auth()` — the admin-quorum Groth16 proof IS
    /// the authorization (the prover demonstrated knowledge of K admin
    /// secret keys behind admin leaves at `admin_root` bundled into
    /// `c_old`). Any address can submit on behalf of the group; the
    /// proof carries the auth. Same convention as sep-democracy's
    /// update entrypoints.
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
            current.admin_threshold_numerator,
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
            // admin_threshold_numerator is fixed at create; never mutated
            // by update_commitment per design §4.7.6.
            admin_threshold_numerator: current.admin_threshold_numerator,
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
    /// Opens against the **member tree** only (chat-capacity proof,
    /// parallel to sep-democracy). Admin status not verified by this
    /// call — admin authorization for state changes is exclusively via
    /// `update_commitment`'s K-signer subset.
    ///
    /// No `check_proof_replay` — verify is read-only and does not
    /// consume the global nullifier; the same proof bytes can be
    /// re-submitted to this entrypoint indefinitely without burning
    /// `UsedProof` storage. Same convention as sep-democracy. Note
    /// that high-frequency repeat verifies on the same proof are
    /// observable to a chain observer (the call is not free); that's
    /// a metering concern, not a soundness one.
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

/// Verify a 7-IC-point Create proof. Public inputs in canonical order
/// (design v0.1.4 §4.8):
///
/// ```
/// IC[1]·commitment + IC[2]·epoch + IC[3]·occupancy_commitment
///   + IC[4]·member_root + IC[5]·admin_root + IC[6]·salt_initial
/// ```
///
/// All six inputs come from the wire. salt_occ_initial is bundled into
/// occupancy_commitment per §4.7.2 v0.1.4 and consumed inside the
/// circuit (not a separate IC point).
#[allow(clippy::too_many_arguments)]
fn verify_create_proof(
    env: &Env,
    vk: &VerificationKeyData,
    proof: &Groth16Proof,
    commitment: &BytesN<32>,
    epoch: u64,
    occupancy_commitment: &BytesN<32>,
    member_root: &BytesN<32>,
    admin_root: &BytesN<32>,
    salt_initial: &BytesN<32>,
) -> bool {
    if vk.ic.len() != CREATE_IC_POINTS {
        return false;
    }
    if !validate_proof_points(proof) {
        return false;
    }
    if !is_canonical_fr(commitment)
        || !is_canonical_fr(occupancy_commitment)
        || !is_canonical_fr(member_root)
        || !is_canonical_fr(admin_root)
        || !is_canonical_fr(salt_initial)
    {
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

    let commitment_fr = Fr::from_bytes(commitment.clone());
    let occ_fr = Fr::from_bytes(occupancy_commitment.clone());
    let member_root_fr = Fr::from_bytes(member_root.clone());
    let admin_root_fr = Fr::from_bytes(admin_root.clone());
    let salt_fr = Fr::from_bytes(salt_initial.clone());
    let epoch_bytes = Bytes::from_array(env, &u64_to_u256_be(epoch));
    let epoch_fr = Fr::from_u256(U256::from_be_bytes(env, &epoch_bytes));

    let msm_points: Vec<G1Affine> = vec![env, ic1, ic2, ic3, ic4, ic5, ic6];
    let msm_scalars: Vec<Fr> = vec![
        env,
        commitment_fr,
        epoch_fr,
        occ_fr,
        member_root_fr,
        admin_root_fr,
        salt_fr,
    ];
    let msm_result = bls.g1_msm(msm_points, msm_scalars);
    let vk_x = bls.g1_add(&ic0, &msm_result);

    let neg_a = -proof_a;
    let g1s: Vec<G1Affine> = vec![env, neg_a, alpha, vk_x, proof_c];
    let g2s: Vec<G2Affine> = vec![env, proof_b, beta, gamma, delta];

    bls.pairing_check(g1s, g2s)
}

/// Verify a 7-IC-point v0.1.4 oligarchy update proof. Public inputs in
/// canonical order (design §4.7.6):
///
/// ```
/// IC[1]·c_old + IC[2]·epoch_old + IC[3]·c_new
///   + IC[4]·occupancy_commitment_old + IC[5]·occupancy_commitment_new
///   + IC[6]·admin_threshold_numerator
/// ```
///
/// The 6th input (`admin_threshold_numerator`) is contract-supplied
/// from storage; the other 5 come from the wire
/// `UpdateCommitmentPublicInputs`.
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
    admin_threshold_numerator: u32,
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
    let threshold_fr = u32_to_fr(env, admin_threshold_numerator);

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
