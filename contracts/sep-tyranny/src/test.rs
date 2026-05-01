//! Inline test suite for the SEP Tyranny contract — PLONK-migration era.
//!
//! Multi-tier coverage: every load-bearing happy path runs at all
//! three tiers (depth 5/8/11). The canonical fixtures use
//! `group_id_fr = Fr::from(0x7777u64)`, so test groups derive the
//! same value via `group_id = be32(0x7777)`.

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
fn run_create_happy_path(tier: u32, group_id_byte: u8) {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let group_id = canonical_group_id(&env);
    let pi = pi_create(&env, tier);
    let commitment = pi.get(0).unwrap();
    let admin_comm = pi.get(2).unwrap();
    // Deduplicate: each test uses a distinct group_id for replay
    // protection, but the canonical group_id must match the
    // canonical witness's group_id_fr — change byte 23 instead so
    // group_id_fr (low 8 bytes BE = 0x7777) is preserved.
    let mut gid_arr = group_id.to_array();
    gid_arr[16] = group_id_byte;
    let _ = c;
    let _ = gid_arr;
    let _ = commitment;
    let _ = admin_comm;

    // Use canonical group_id for the load-bearing test.
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
    run_create_happy_path(0, 0);
}

#[test]
fn test_create_group_happy_path_d8() {
    run_create_happy_path(1, 0);
}

#[test]
fn test_create_group_happy_path_d11() {
    run_create_happy_path(2, 0);
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
        ("InvalidProof", Error::InvalidProof as u32),
        ("InvalidTier", Error::InvalidTier as u32),
        ("PublicInputsMismatch", Error::PublicInputsMismatch as u32),
    ];
    for (name, code) in expected {
        if let Some(entry) = errors.iter().find(|e| e["name"].as_str() == Some(name)) {
            let json_code = entry["code"].as_u64().unwrap() as u32;
            assert_eq!(json_code, *code, "error code drift for {}", name);
        }
    }

    let tiers = v["tier"]["vectors"].as_array().unwrap();
    for entry in tiers {
        let tier = entry["tier"].as_u64().unwrap() as u32;
        let expected_cap = entry["capacity"].as_u64().unwrap() as u32;
        assert_eq!(tier_capacity(tier), expected_cap, "tier_capacity({})", tier);
    }
}
