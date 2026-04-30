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
//! Membership and update proofs are TurboPlonk over BLS12-381 + the
//! EF KZG SRS, verified through the shared `plonk-verifier` crate
//! (`plonk_verifier::verifier::verify`). Per-tier baked VKs and the
//! 96-byte compressed `[τ]_2` are embedded via `include_bytes!` from
//! the `plonk-verifier` test fixtures (which act as the canonical
//! pin for the prover/verifier pipeline; see PR #194 for the
//! drift-detector). Public-input layout:
//!
//! - **Membership** (`create_group`, `verify_membership`):
//!   `(commitment, epoch)` — 2 BE-encoded scalars.
//! - **Update** (`update_commitment`):
//!   `(c_old, epoch_old, c_new)` — 3 BE-encoded scalars.
//!
//! The Groth16 path was dropped wholesale in this PR; there is no
//! `update_vk` admin entrypoint, no `DataKey::VK` / `DataKey::UpdateVK`
//! storage, and no per-tier VK constructor args. Rotating a VK now
//! means redeploying the contract.

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

/// Maximum number of history entries retained per group.
const HISTORY_WINDOW: u32 = 64;

/// Minimum TTL threshold for persistent storage (~1 day in ledgers).
const LEDGER_THRESHOLD: u32 = 17_280;

/// TTL bump amount for persistent storage (~30 days in ledgers).
const LEDGER_BUMP: u32 = 518_400;

/// Maximum number of active groups allowed per tier.
const MAX_GROUPS_PER_TIER: u32 = 10_000;

/// Number of public inputs the membership circuit consumes.
const MEMBERSHIP_PI_COUNT: u32 = 2;

/// Number of public inputs the update circuit consumes.
const UPDATE_PI_COUNT: u32 = 3;

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
// Embedded baked VKs + SRS-G2
// ================================================================
//
// All bytes come from `contracts/plonk-verifier/tests/fixtures/`,
// produced by the off-chain prover-side
// `circuit::plonk::verifier::tests::plonk_verifier_fixtures_match_or_regenerate`
// test (which double-acts as a drift detector when run without
// `STELLAR_REGEN_FIXTURES=1`). Rotating any of these requires a
// contract redeploy; there is no admin-rotation entrypoint.

const VK_D5: &[u8] =
    include_bytes!("../../plonk-verifier/tests/fixtures/vk-d5.bin");
const VK_D8: &[u8] =
    include_bytes!("../../plonk-verifier/tests/fixtures/vk-d8.bin");
const VK_D11: &[u8] =
    include_bytes!("../../plonk-verifier/tests/fixtures/vk-d11.bin");

const UPDATE_VK_D5: &[u8] =
    include_bytes!("../../plonk-verifier/tests/fixtures/update-vk-d5.bin");
const UPDATE_VK_D8: &[u8] =
    include_bytes!("../../plonk-verifier/tests/fixtures/update-vk-d8.bin");
const UPDATE_VK_D11: &[u8] =
    include_bytes!("../../plonk-verifier/tests/fixtures/update-vk-d11.bin");

/// Compressed `[τ]_2` shared across all baked VKs (single EF KZG SRS).
/// The off-chain regen test asserts byte-equality across both circuits
/// at all three tiers — if they ever diverge, that test fails first.
const SRS_G2: &[u8; G2_COMPRESSED_LEN] =
    include_bytes!("../../plonk-verifier/tests/fixtures/srs-g2-compressed.bin");

/// Look up the membership VK for `tier` ∈ {0,1,2}.
fn membership_vk_for_tier(tier: u32) -> Option<&'static [u8]> {
    match tier {
        0 => Some(VK_D5),
        1 => Some(VK_D8),
        2 => Some(VK_D11),
        _ => None,
    }
}

/// Look up the update VK for `tier` ∈ {0,1,2}.
fn update_vk_for_tier(tier: u32) -> Option<&'static [u8]> {
    match tier {
        0 => Some(UPDATE_VK_D5),
        1 => Some(UPDATE_VK_D8),
        2 => Some(UPDATE_VK_D11),
        _ => None,
    }
}

// ================================================================
// Errors
// ================================================================

/// Numeric values pinned in `test-vectors.json` `error_codes.vectors`.
/// Codes 9 (`InvalidVkLength`) and 26 (`InvalidPoint`) are no longer
/// reachable post-PLONK migration but the numeric slots are reserved
/// (test-vectors.json `dropped_from_anarchy_plonk` documents the
/// rationale).
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
    PublicInputsMismatch = 10,
    InvalidEpoch = 11,
    ProofReplay = 12,
    TierGroupLimitReached = 13,
    AdminOnly = 14,
    InvalidCommitmentEncoding = 15,
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

/// Emitted when admin toggles restricted mode.
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

    /// Atomic constructor: takes only the admin address. The per-tier
    /// VKs (membership + update) are baked into the contract via
    /// `include_bytes!` and need no constructor input.
    pub fn __constructor(env: Env, admin: Address) -> Result<(), Error> {
        Self::do_initialize(&env, admin)
    }

    fn do_initialize(env: &Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    // ---- Admin ----

    /// Admin toggles restricted mode. When `restricted == true`, only
    /// the admin may call `create_group`.
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
    /// Bumps `Group(group_id)` and `History(group_id)` only — does
    /// NOT touch `UsedProof(...)` entries (global-nullifier scope,
    /// only refreshed by `record_proof` at successful state-changing
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
    /// Validates: tier ≤ 2, group_id unused, `commitment` is
    /// canonical Fr, `public_inputs` matches the
    /// `(commitment, epoch=0)` pair the prover signed, proof verifies
    /// under the membership VK at `tier`, proof not replayed, tier
    /// capacity not exceeded. `member_count` is informational and
    /// accepted without validation (any u32; sentinel `0` means
    /// "not tracked").
    pub fn create_group(
        env: Env,
        caller: Address,
        group_id: BytesN<32>,
        commitment: BytesN<32>,
        tier: u32,
        member_count: u32,
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

        if tier > 2 {
            return Err(Error::InvalidTier);
        }
        if !is_canonical_fr(&commitment) {
            return Err(Error::InvalidCommitmentEncoding);
        }
        // Public inputs (commitment, epoch=0) must match the wire args.
        if public_inputs.len() != MEMBERSHIP_PI_COUNT {
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
            .get(&DataKey::GroupCount(tier))
            .unwrap_or(0);
        if count >= MAX_GROUPS_PER_TIER {
            return Err(Error::TierGroupLimitReached);
        }

        Self::check_proof_replay(&env, &proof)?;

        let vk_bytes = membership_vk_for_tier(tier).ok_or(Error::InvalidTier)?;
        verify_plonk_proof(&env, vk_bytes, &proof, &public_inputs)?;

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
    /// `public_inputs` MUST be `(c_old, epoch_old, c_new)` — the
    /// contract validates `c_old == state.commitment` and
    /// `epoch_old == state.epoch` against storage, and uses
    /// `public_inputs[2]` (`c_new`) as the next commitment. No
    /// separate `c_new` wire arg.
    ///
    /// No `caller.require_auth()` — the membership PLONK proof IS
    /// the authorization (the prover demonstrated knowledge of a
    /// secret key behind a member leaf at `c_old`). Any address can
    /// submit on behalf of the group; the proof carries the auth.
    /// Same convention as `sep-democracy` / `sep-oligarchy`.
    ///
    /// `member_count` is NOT updated by this entrypoint. The contract
    /// has no way to recompute it (no Poseidon host) and clients
    /// track it off-chain anyway.
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

        if c_old != current.commitment {
            return Err(Error::PublicInputsMismatch);
        }
        if epoch_old_be != be32_from_u64(&env, current.epoch) {
            return Err(Error::PublicInputsMismatch);
        }
        if !is_canonical_fr(&c_new) {
            return Err(Error::InvalidCommitmentEncoding);
        }

        Self::check_proof_replay(&env, &proof)?;

        let vk_bytes = update_vk_for_tier(current.tier).ok_or(Error::InvalidTier)?;
        verify_plonk_proof(&env, vk_bytes, &proof, &public_inputs)?;

        Self::record_proof(&env, &proof);

        let timestamp = env.ledger().timestamp();
        Self::archive_entry(&env, &group_id, &current);

        let new_entry = CommitmentEntry {
            commitment: c_new.clone(),
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
            commitment: c_new,
            epoch: new_epoch,
            timestamp,
        }
        .publish(&env);
        Ok(())
    }

    /// Read-only membership verification.
    ///
    /// `public_inputs` MUST be `(commitment, epoch)` — both validated
    /// against the group's stored state.
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

        let vk_bytes = membership_vk_for_tier(state.tier).ok_or(Error::InvalidTier)?;
        match verify_plonk_proof(&env, vk_bytes, &proof, &public_inputs) {
            Ok(()) => Ok(true),
            Err(Error::InvalidProof) => Ok(false),
            Err(other) => Err(other),
        }
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

    /// SHA-256 of the proof bytes — global-nullifier identifier.
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

// ================================================================
// PLONK verification glue
// ================================================================

/// Cross-check at compile time that the verifier-crate constants
/// haven't drifted out from under the contract.
const _: () = {
    assert!(PROOF_LEN == 1601, "plonk_verifier::PROOF_LEN drifted");
    assert!(FR_LEN == 32, "plonk_verifier::FR_LEN drifted");
    assert!(G2_COMPRESSED_LEN == 96, "plonk_verifier::G2_COMPRESSED_LEN drifted");
    // Pin MAX_PI_COUNT against the per-circuit PI counts so a future
    // increase to either circuit can't silently exceed the buffer.
    assert!(
        MAX_PI_COUNT >= MEMBERSHIP_PI_COUNT as usize,
        "MAX_PI_COUNT < MEMBERSHIP_PI_COUNT"
    );
    assert!(
        MAX_PI_COUNT >= UPDATE_PI_COUNT as usize,
        "MAX_PI_COUNT < UPDATE_PI_COUNT"
    );
};

/// Parse the embedded VK bytes + the wire proof, copy public inputs
/// into the `[[u8; 32]]` shape `verify` expects, run the verifier.
///
/// Maps every failure mode to `Error::InvalidProof`:
/// - VK bytes fail to parse (should never happen for embedded bytes,
///   but defended for completeness).
/// - Proof bytes fail to parse (caller submitted a malformed blob).
/// - `verifier::verify` returns `Err(_)` (any kind: pairing mismatch,
///   PI count mismatch, off-curve trap surfaces as a contract panic
///   from the host's `g1_msm` / `pairing_check` and never reaches
///   here as a `Result::Err`).
fn verify_plonk_proof(
    env: &Env,
    vk_bytes: &[u8],
    proof: &BytesN<1601>,
    public_inputs: &Vec<BytesN<32>>,
) -> Result<(), Error> {
    let parsed_vk = parse_vk_bytes(vk_bytes).map_err(|_| Error::InvalidProof)?;

    let proof_array: [u8; PROOF_LEN] = proof.to_array();
    let parsed_proof = parse_proof_bytes(&proof_array).map_err(|_| Error::InvalidProof)?;

    // verify takes &[[u8; 32]]; build that into a fixed buffer sized
    // for this contract's circuits (membership=2, update=3 — see
    // MAX_PI_COUNT). The const-assert above pins this against the
    // per-circuit PI counts.
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

/// Upper bound on public-input count for this contract's circuits.
/// Keep this in sync with `MEMBERSHIP_PI_COUNT` / `UPDATE_PI_COUNT`.
const MAX_PI_COUNT: usize = 3;

// ================================================================
// Encoding helpers
// ================================================================

/// `u64` → 32-byte big-endian Fr scalar (high 24 bytes zero, low 8
/// bytes carrying the value). Matches the in-circuit `Fr::from(u64)`
/// encoding used by both `MembershipCircuit` and `UpdateCircuit`.
fn be32_from_u64(env: &Env, value: u64) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[24..32].copy_from_slice(&value.to_be_bytes());
    BytesN::from_array(env, &bytes)
}

/// Round-trip canonicality check: reduce `value` mod r and compare.
/// Catches non-canonical bit patterns that `Fr::from_bytes` would
/// silently mod-reduce on the verifier side.
fn is_canonical_fr(value: &BytesN<32>) -> bool {
    let fr = Fr::from_bytes(value.clone());
    let canonical: BytesN<32> = fr.to_bytes();
    canonical == *value
}

#[cfg(test)]
mod test;
