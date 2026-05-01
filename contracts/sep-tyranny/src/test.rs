//! Inline test suite for the SEP Tyranny contract — PLONK-migration era.
//!
//! Multi-tier coverage: every load-bearing happy path runs at all
//! three tiers (depth 5/8/11). The canonical fixtures use
//! `group_id_fr = Fr::from(0x7777u64)`, so test groups derive the
//! same value via `group_id = be32(0x7777)`.

extern crate std;

use super::*;
use soroban_sdk::testutils::Address as _;

// ================================================================
// Canonical fixtures
// ================================================================

const PROOF_D5: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/proof-d5.bin");
const PROOF_D8: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/proof-d8.bin");
const PROOF_D11: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/proof-d11.bin");
const PI_D5: &[u8; 64] =
    include_bytes!("../../plonk-verifier/tests/fixtures/pi-d5.bin");
const PI_D8: &[u8; 64] =
    include_bytes!("../../plonk-verifier/tests/fixtures/pi-d8.bin");
const PI_D11: &[u8; 64] =
    include_bytes!("../../plonk-verifier/tests/fixtures/pi-d11.bin");

const TYR_CREATE_PROOF_D5: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/tyranny-create-proof-d5.bin");
const TYR_CREATE_PROOF_D8: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/tyranny-create-proof-d8.bin");
const TYR_CREATE_PROOF_D11: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/tyranny-create-proof-d11.bin");
const TYR_CREATE_PI_D5: &[u8; 128] =
    include_bytes!("../../plonk-verifier/tests/fixtures/tyranny-create-pi-d5.bin");
const TYR_CREATE_PI_D8: &[u8; 128] =
    include_bytes!("../../plonk-verifier/tests/fixtures/tyranny-create-pi-d8.bin");
const TYR_CREATE_PI_D11: &[u8; 128] =
    include_bytes!("../../plonk-verifier/tests/fixtures/tyranny-create-pi-d11.bin");

const TYR_UPDATE_PROOF_D5: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/tyranny-update-proof-d5.bin");
const TYR_UPDATE_PROOF_D8: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/tyranny-update-proof-d8.bin");
const TYR_UPDATE_PROOF_D11: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/tyranny-update-proof-d11.bin");
const TYR_UPDATE_PI_D5: &[u8; 160] =
    include_bytes!("../../plonk-verifier/tests/fixtures/tyranny-update-pi-d5.bin");
const TYR_UPDATE_PI_D8: &[u8; 160] =
    include_bytes!("../../plonk-verifier/tests/fixtures/tyranny-update-pi-d8.bin");
const TYR_UPDATE_PI_D11: &[u8; 160] =
    include_bytes!("../../plonk-verifier/tests/fixtures/tyranny-update-pi-d11.bin");

const CANONICAL_EPOCH: u64 = 1234;

/// Group ID matching the canonical witness's group_id_fr =
/// Fr::from(0x7777u64). 32-byte BE.
fn canonical_group_id(env: &Env) -> BytesN<32> {
    let mut arr = [0u8; 32];
    arr[24..32].copy_from_slice(&0x7777u64.to_be_bytes());
    BytesN::from_array(env, &arr)
}

fn proof_for_tier(env: &Env, tier: u32, kind: &str) -> BytesN<1601> {
    let bytes = match (kind, tier) {
        ("membership", 0) => PROOF_D5,
        ("membership", 1) => PROOF_D8,
        ("membership", 2) => PROOF_D11,
        ("create", 0) => TYR_CREATE_PROOF_D5,
        ("create", 1) => TYR_CREATE_PROOF_D8,
        ("create", 2) => TYR_CREATE_PROOF_D11,
        ("update", 0) => TYR_UPDATE_PROOF_D5,
        ("update", 1) => TYR_UPDATE_PROOF_D8,
        ("update", 2) => TYR_UPDATE_PROOF_D11,
        _ => panic!("unknown tier/kind"),
    };
    BytesN::from_array(env, bytes)
}

fn pi_membership(env: &Env, tier: u32) -> Vec<BytesN<32>> {
    let bytes: &[u8] = match tier {
        0 => PI_D5,
        1 => PI_D8,
        2 => PI_D11,
        _ => panic!(),
    };
    let mut pi = Vec::new(env);
    for i in 0..2 {
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes[i * 32..(i + 1) * 32]);
        pi.push_back(BytesN::from_array(env, &arr));
    }
    pi
}

fn pi_create(env: &Env, tier: u32) -> Vec<BytesN<32>> {
    let bytes: &[u8] = match tier {
        0 => TYR_CREATE_PI_D5,
        1 => TYR_CREATE_PI_D8,
        2 => TYR_CREATE_PI_D11,
        _ => panic!(),
    };
    let mut pi = Vec::new(env);
    for i in 0..4 {
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes[i * 32..(i + 1) * 32]);
        pi.push_back(BytesN::from_array(env, &arr));
    }
    pi
}

fn pi_update(env: &Env, tier: u32) -> Vec<BytesN<32>> {
    let bytes: &[u8] = match tier {
        0 => TYR_UPDATE_PI_D5,
        1 => TYR_UPDATE_PI_D8,
        2 => TYR_UPDATE_PI_D11,
        _ => panic!(),
    };
    let mut pi = Vec::new(env);
    for i in 0..5 {
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes[i * 32..(i + 1) * 32]);
        pi.push_back(BytesN::from_array(env, &arr));
    }
    pi
}

fn malformed_proof(env: &Env) -> BytesN<1601> {
    BytesN::from_array(env, &[0xAAu8; 1601])
}

fn canonical_zero(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

// ================================================================
// Setup
// ================================================================

fn setup_env() -> (Env, SepTyrannyContractClient<'static>, Address) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(SepTyrannyContract, (admin.clone(),));
    let client = SepTyrannyContractClient::new(&env, &contract_id);
    (env, client, admin)
}

fn caller(env: &Env) -> Address {
    Address::generate(env)
}

fn inject_group(
    env: &Env,
    contract_id: &Address,
    group_id: &BytesN<32>,
    commitment: &BytesN<32>,
    admin_pubkey_commitment: &BytesN<32>,
    tier: u32,
    epoch: u64,
) {
    env.as_contract(contract_id, || {
        let entry = CommitmentEntry {
            commitment: commitment.clone(),
            epoch,
            timestamp: env.ledger().timestamp(),
            tier,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Group(group_id.clone()), &entry);
        env.storage().persistent().set(
            &DataKey::AdminCommitment(group_id.clone()),
            admin_pubkey_commitment,
        );
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

// ================================================================
// 1. Initialization
// ================================================================

#[test]
fn test_initialize() {
    let (_env, _client, _admin) = setup_env();
}

// ================================================================
// 2. Multi-tier happy paths
// ================================================================

/// Drive create_group on the canonical fixture for the given tier.
fn run_create_happy_path(tier: u32) {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let group_id = canonical_group_id(&env);
    let pi = pi_create(&env, tier);
    let commitment = pi.get(0).unwrap();
    let admin_comm = pi.get(2).unwrap();

    client.create_group(
        &c,
        &group_id,
        &commitment,
        &tier,
        &admin_comm,
        &proof_for_tier(&env, tier, "create"),
        &pi,
    );

    let entry = client.get_commitment(&group_id);
    assert_eq!(entry.commitment, commitment);
    assert_eq!(entry.tier, tier);
    assert_eq!(entry.epoch, 0);
}

#[test]
fn test_create_group_happy_path_d5() {
    run_create_happy_path(0);
}

#[test]
fn test_create_group_happy_path_d8() {
    run_create_happy_path(1);
}

#[test]
fn test_create_group_happy_path_d11() {
    run_create_happy_path(2);
}

/// Drive update_commitment on the canonical fixture for `tier`.
fn run_update_happy_path(tier: u32) {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = canonical_group_id(&env);
    let upi = pi_update(&env, tier);
    let c_old = upi.get(0).unwrap();
    let c_new = upi.get(2).unwrap();
    let admin_comm = upi.get(3).unwrap();
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &c_old,
        &admin_comm,
        tier,
        CANONICAL_EPOCH,
    );

    client.update_commitment(&group_id, &proof_for_tier(&env, tier, "update"), &upi);

    let post = client.get_commitment(&group_id);
    assert_eq!(post.commitment, c_new);
    assert_eq!(post.epoch, CANONICAL_EPOCH + 1);
    assert_eq!(post.tier, tier);
}

#[test]
fn test_update_commitment_happy_path_d5() {
    run_update_happy_path(0);
}

#[test]
fn test_update_commitment_happy_path_d8() {
    run_update_happy_path(1);
}

#[test]
fn test_update_commitment_happy_path_d11() {
    run_update_happy_path(2);
}

fn run_verify_membership_happy_path(tier: u32) {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = canonical_group_id(&env);
    let pi = pi_membership(&env, tier);
    let commitment = pi.get(0).unwrap();
    let admin_comm = canonical_zero(&env); // not relevant for verify_membership
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &commitment,
        &admin_comm,
        tier,
        CANONICAL_EPOCH,
    );

    let result =
        client.verify_membership(&group_id, &proof_for_tier(&env, tier, "membership"), &pi);
    assert!(result, "tier {tier} membership proof should verify");
}

#[test]
fn test_verify_membership_happy_path_d5() {
    run_verify_membership_happy_path(0);
}

#[test]
fn test_verify_membership_happy_path_d8() {
    run_verify_membership_happy_path(1);
}

#[test]
fn test_verify_membership_happy_path_d11() {
    run_verify_membership_happy_path(2);
}

// ================================================================
// 3. Reject paths (depth=5)
// ================================================================

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_create_group_rejects_invalid_tier() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let group_id = canonical_group_id(&env);
    let pi = pi_create(&env, 0);
    client.create_group(
        &c,
        &group_id,
        &pi.get(0).unwrap(),
        &3u32,
        &pi.get(2).unwrap(),
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_create_group_rejects_pi_count_mismatch() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let mut pi = Vec::new(&env);
    pi.push_back(z.clone());
    pi.push_back(z.clone());
    pi.push_back(z.clone());
    client.create_group(
        &c,
        &canonical_group_id(&env),
        &z,
        &0u32,
        &z,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
fn test_create_group_rejects_invalid_proof() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let group_id = canonical_group_id(&env);
    let pi = pi_create(&env, 0);
    let r = client.try_create_group(
        &c,
        &group_id,
        &pi.get(0).unwrap(),
        &0u32,
        &pi.get(2).unwrap(),
        &malformed_proof(&env),
        &pi,
    );
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof, got {:?}", other),
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_update_commitment_rejects_unknown_group() {
    let (env, client, _admin) = setup_env();
    let group_id = BytesN::from_array(&env, &[99u8; 32]);
    let pi = pi_update(&env, 0);
    client.update_commitment(&group_id, &malformed_proof(&env), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_group_rejects_non_canonical_commitment() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let pi = pi_create(&env, 0);
    let bad = BytesN::from_array(&env, &[0xffu8; 32]);
    client.create_group(
        &c,
        &canonical_group_id(&env),
        &bad,
        &0u32,
        &pi.get(2).unwrap(),
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_group_rejects_non_canonical_admin_pubkey_commitment() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let pi = pi_create(&env, 0);
    let bad = BytesN::from_array(&env, &[0xffu8; 32]);
    client.create_group(
        &c,
        &canonical_group_id(&env),
        &pi.get(0).unwrap(),
        &0u32,
        &bad,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_create_group_rejects_duplicate_group_id() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let c = caller(&env);
    let group_id = canonical_group_id(&env);
    let pi = pi_create(&env, 0);
    let commitment = pi.get(0).unwrap();
    let admin_comm = pi.get(2).unwrap();
    inject_group(&env, &contract_id, &group_id, &commitment, &admin_comm, 0, 0);
    client.create_group(
        &c,
        &group_id,
        &commitment,
        &0u32,
        &admin_comm,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_create_group_enforces_tier_group_limit() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let c = caller(&env);
    let pi = pi_create(&env, 0);
    env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .set(&DataKey::GroupCount(0), &MAX_GROUPS_PER_TIER);
    });
    client.create_group(
        &c,
        &canonical_group_id(&env),
        &pi.get(0).unwrap(),
        &0u32,
        &pi.get(2).unwrap(),
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #14)")]
fn test_create_group_restricted_mode_rejects_non_admin() {
    let (env, client, _admin) = setup_env();
    client.set_restricted_mode(&true);
    let c = caller(&env);
    let pi = pi_create(&env, 0);
    client.create_group(
        &c,
        &canonical_group_id(&env),
        &pi.get(0).unwrap(),
        &0u32,
        &pi.get(2).unwrap(),
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
fn test_create_group_restricted_mode_admin_can_create() {
    let (env, client, admin) = setup_env();
    client.set_restricted_mode(&true);
    let group_id = canonical_group_id(&env);
    let pi = pi_create(&env, 0);
    let commitment = pi.get(0).unwrap();
    let admin_comm = pi.get(2).unwrap();
    client.create_group(
        &admin,
        &group_id,
        &commitment,
        &0u32,
        &admin_comm,
        &proof_for_tier(&env, 0, "create"),
        &pi,
    );
    let entry = client.get_commitment(&group_id);
    assert_eq!(entry.commitment, commitment);
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_create_group_rejects_replayed_proof() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let c = caller(&env);
    let group_id = canonical_group_id(&env);
    let pi = pi_create(&env, 0);
    let proof = proof_for_tier(&env, 0, "create");
    let preimage = Bytes::from_slice(&env, proof.to_array().as_slice());
    let hash: BytesN<32> = env.crypto().sha256(&preimage).into();
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsedProof(hash), &true);
    });
    client.create_group(
        &c,
        &group_id,
        &pi.get(0).unwrap(),
        &0u32,
        &pi.get(2).unwrap(),
        &proof,
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_commitment_rejects_stale_c_old() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = canonical_group_id(&env);
    let upi = pi_update(&env, 0);
    let admin_comm = upi.get(3).unwrap();
    let other = BytesN::from_array(&env, &[7u8; 32]);
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &other,
        &admin_comm,
        0,
        CANONICAL_EPOCH,
    );
    client.update_commitment(&group_id, &malformed_proof(&env), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_commitment_rejects_wrong_epoch_old() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = canonical_group_id(&env);
    let upi = pi_update(&env, 0);
    let c_old = upi.get(0).unwrap();
    let admin_comm = upi.get(3).unwrap();
    inject_group(&env, &contract_id, &group_id, &c_old, &admin_comm, 0, 999);
    client.update_commitment(&group_id, &malformed_proof(&env), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_update_commitment_rejects_non_canonical_c_new() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = canonical_group_id(&env);
    let upi = pi_update(&env, 0);
    let c_old = upi.get(0).unwrap();
    let admin_comm = upi.get(3).unwrap();
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &c_old,
        &admin_comm,
        0,
        CANONICAL_EPOCH,
    );
    let bad = BytesN::from_array(&env, &[0xffu8; 32]);
    let mut bad_pi = Vec::new(&env);
    bad_pi.push_back(c_old);
    bad_pi.push_back(upi.get(1).unwrap());
    bad_pi.push_back(bad);
    bad_pi.push_back(admin_comm);
    bad_pi.push_back(upi.get(4).unwrap());
    client.update_commitment(&group_id, &malformed_proof(&env), &bad_pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn test_update_commitment_rejects_epoch_overflow() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = canonical_group_id(&env);
    let upi = pi_update(&env, 0);
    let c_old = upi.get(0).unwrap();
    let admin_comm = upi.get(3).unwrap();
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &c_old,
        &admin_comm,
        0,
        u64::MAX,
    );
    client.update_commitment(&group_id, &malformed_proof(&env), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_update_commitment_rejects_replayed_proof() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = canonical_group_id(&env);
    let upi = pi_update(&env, 0);
    let c_old = upi.get(0).unwrap();
    let admin_comm = upi.get(3).unwrap();
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &c_old,
        &admin_comm,
        0,
        CANONICAL_EPOCH,
    );
    let proof = proof_for_tier(&env, 0, "update");
    let preimage = Bytes::from_slice(&env, proof.to_array().as_slice());
    let hash: BytesN<32> = env.crypto().sha256(&preimage).into();
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsedProof(hash), &true);
    });
    client.update_commitment(&group_id, &proof, &upi);
}

#[test]
fn test_update_commitment_does_not_mutate_admin_pubkey_commitment() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = canonical_group_id(&env);
    let upi = pi_update(&env, 0);
    let c_old = upi.get(0).unwrap();
    let admin_comm = upi.get(3).unwrap();
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &c_old,
        &admin_comm,
        0,
        CANONICAL_EPOCH,
    );
    client.update_commitment(&group_id, &proof_for_tier(&env, 0, "update"), &upi);
    let post_admin = client.get_admin_commitment(&group_id);
    assert_eq!(post_admin, admin_comm);
}

// ================================================================
// 4. Queries
// ================================================================

#[test]
fn test_get_commitment_returns_state() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[50u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 1, 3);
    let entry = client.get_commitment(&group_id);
    assert_eq!(entry.tier, 1);
    assert_eq!(entry.epoch, 3);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_get_commitment_rejects_unknown_group() {
    let (_env, client, _admin) = setup_env();
    let env = &client.env;
    let group_id = BytesN::from_array(env, &[98u8; 32]);
    client.get_commitment(&group_id);
}

#[test]
fn test_bump_group_ttl_extends() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[52u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 0);
    client.bump_group_ttl(&group_id);
}

// ================================================================
// 5. test-vectors.json consistency
// ================================================================

#[test]
fn test_vectors_consistency() {
    use serde_json::Value;
    let raw = include_str!("../test-vectors.json");
    let v: Value = serde_json::from_str(raw).expect("test-vectors.json is valid JSON");
    let errors = v["error_codes"]["vectors"].as_array().unwrap();
    let expected: &[(&str, u32)] = &[
        ("NotInitialized", Error::NotInitialized as u32),
        ("AlreadyInitialized", Error::AlreadyInitialized as u32),
        ("GroupAlreadyExists", Error::GroupAlreadyExists as u32),
        ("GroupNotFound", Error::GroupNotFound as u32),
        ("InvalidProof", Error::InvalidProof as u32),
        ("InvalidTier", Error::InvalidTier as u32),
        ("PublicInputsMismatch", Error::PublicInputsMismatch as u32),
        ("InvalidEpoch", Error::InvalidEpoch as u32),
        ("ProofReplay", Error::ProofReplay as u32),
        ("TierGroupLimitReached", Error::TierGroupLimitReached as u32),
        ("AdminOnly", Error::AdminOnly as u32),
        ("InvalidCommitmentEncoding", Error::InvalidCommitmentEncoding as u32),
    ];
    for (name, code) in expected {
        let entry = errors
            .iter()
            .find(|e| e["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("test-vectors.json missing error code: {}", name));
        let json_code = entry["code"].as_u64().unwrap() as u32;
        assert_eq!(json_code, *code, "error code drift for {}", name);
    }

    let tiers = v["tier"]["vectors"].as_array().unwrap();
    assert_eq!(tiers.len(), 3, "tier vectors must enumerate 3 tiers");
    for entry in tiers {
        let tier = entry["tier"].as_u64().unwrap() as u32;
        let expected_cap = entry["capacity"].as_u64().unwrap() as u32;
        assert_eq!(tier_capacity(tier), expected_cap, "tier_capacity({})", tier);
    }

    // Pin PI counts against vk_kind_enum (ic_count = base + pi_count).
    let vk_kinds = v["vk_kind_enum"]["vectors"].as_array().unwrap();
    let pi_count_for = |name: &str| -> u32 {
        let entry = vk_kinds
            .iter()
            .find(|e| e["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("test-vectors.json missing vk_kind: {}", name));
        (entry["ic_count"].as_u64().unwrap() as u32) - 1
    };
    assert_eq!(pi_count_for("Membership"), MEMBERSHIP_PI_COUNT);
    assert_eq!(pi_count_for("Create"), CREATE_PI_COUNT);
    assert_eq!(pi_count_for("Update"), UPDATE_PI_COUNT);
}

// ================================================================
// Gas benchmarks (Phase C.5)
// ================================================================

fn bench_verify_membership_at_tier(tier: u32) {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = canonical_group_id(&env);
    let pi = pi_membership(&env, tier);
    let commitment = pi.get(0).unwrap();
    let admin_comm = canonical_zero(&env);
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &commitment,
        &admin_comm,
        tier,
        CANONICAL_EPOCH,
    );

    env.cost_estimate().budget().reset_tracker();
    let result =
        client.verify_membership(&group_id, &proof_for_tier(&env, tier, "membership"), &pi);
    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();

    assert!(result, "tier {tier} membership proof should verify");
    std::eprintln!(
        "[gas-bench] sep-tyranny verify_membership(tier={}): cpu={} mem={}",
        tier, cpu, mem
    );
}

#[test]
fn bench_verify_membership_d5() {
    bench_verify_membership_at_tier(0);
}

#[test]
fn bench_verify_membership_d8() {
    bench_verify_membership_at_tier(1);
}

#[test]
fn bench_verify_membership_d11() {
    bench_verify_membership_at_tier(2);
}

fn bench_update_commitment_at_tier(tier: u32) {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = canonical_group_id(&env);
    let upi = pi_update(&env, tier);
    let c_old = upi.get(0).unwrap();
    let admin_comm = upi.get(3).unwrap();
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &c_old,
        &admin_comm,
        tier,
        CANONICAL_EPOCH,
    );

    env.cost_estimate().budget().reset_tracker();
    client.update_commitment(&group_id, &proof_for_tier(&env, tier, "update"), &upi);
    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();

    std::eprintln!(
        "[gas-bench] sep-tyranny update_commitment(tier={}): cpu={} mem={}",
        tier, cpu, mem
    );
}

#[test]
fn bench_update_commitment_d5() {
    bench_update_commitment_at_tier(0);
}

#[test]
fn bench_update_commitment_d8() {
    bench_update_commitment_at_tier(1);
}

#[test]
fn bench_update_commitment_d11() {
    bench_update_commitment_at_tier(2);
}
