//! Inline test suite for the SEP Democracy contract — PLONK-migration era.

use super::*;
use soroban_sdk::testutils::Address as _;

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

const DEMO_UPDATE_PROOF_D5: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/democracy-update-proof-d5.bin");
const DEMO_UPDATE_PROOF_D8: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/democracy-update-proof-d8.bin");
const DEMO_UPDATE_PROOF_D11: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/democracy-update-proof-d11.bin");
const DEMO_UPDATE_PI_D5: &[u8; 192] =
    include_bytes!("../../plonk-verifier/tests/fixtures/democracy-update-pi-d5.bin");
const DEMO_UPDATE_PI_D8: &[u8; 192] =
    include_bytes!("../../plonk-verifier/tests/fixtures/democracy-update-pi-d8.bin");
const DEMO_UPDATE_PI_D11: &[u8; 192] =
    include_bytes!("../../plonk-verifier/tests/fixtures/democracy-update-pi-d11.bin");

const CANONICAL_EPOCH: u64 = 1234;
const CANONICAL_THRESHOLD: u32 = 5;

fn membership_proof(env: &Env, tier: u32) -> BytesN<1601> {
    BytesN::from_array(
        env,
        match tier {
            0 => PROOF_D5,
            1 => PROOF_D8,
            2 => PROOF_D11,
            _ => panic!(),
        },
    )
}

fn demo_update_proof(env: &Env, tier: u32) -> BytesN<1601> {
    BytesN::from_array(
        env,
        match tier {
            0 => DEMO_UPDATE_PROOF_D5,
            1 => DEMO_UPDATE_PROOF_D8,
            2 => DEMO_UPDATE_PROOF_D11,
            _ => panic!(),
        },
    )
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

fn pi_update(env: &Env, tier: u32) -> Vec<BytesN<32>> {
    let bytes: &[u8] = match tier {
        0 => DEMO_UPDATE_PI_D5,
        1 => DEMO_UPDATE_PI_D8,
        2 => DEMO_UPDATE_PI_D11,
        _ => panic!(),
    };
    let mut pi = Vec::new(env);
    for i in 0..6 {
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

fn setup_env() -> (Env, SepDemocracyContractClient<'static>, Address) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(SepDemocracyContract, (admin.clone(),));
    let client = SepDemocracyContractClient::new(&env, &contract_id);
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
    occupancy_commitment: &BytesN<32>,
    threshold_numerator: u32,
    tier: u32,
    epoch: u64,
) {
    env.as_contract(contract_id, || {
        let entry = CommitmentEntry {
            commitment: commitment.clone(),
            epoch,
            timestamp: env.ledger().timestamp(),
            tier,
            active: true,
            occupancy_commitment: occupancy_commitment.clone(),
            threshold_numerator,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Group(group_id.clone()), &entry);
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

#[test]
fn test_initialize() {
    let (_env, _client, _admin) = setup_env();
}

// ---- Multi-tier happy paths ----

fn run_update_happy_path(tier: u32) {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[(tier as u8 + 1); 32]);
    let upi = pi_update(&env, tier);
    let c_old = upi.get(0).unwrap();
    let c_new = upi.get(2).unwrap();
    let occ_old = upi.get(3).unwrap();
    let occ_new = upi.get(4).unwrap();
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &c_old,
        &occ_old,
        CANONICAL_THRESHOLD,
        tier,
        CANONICAL_EPOCH,
    );

    client.update_commitment(&group_id, &demo_update_proof(&env, tier), &upi);

    let post = client.get_commitment(&group_id);
    assert_eq!(post.commitment, c_new);
    assert_eq!(post.epoch, CANONICAL_EPOCH + 1);
    assert_eq!(post.occupancy_commitment, occ_new);
    assert_eq!(post.threshold_numerator, CANONICAL_THRESHOLD);
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
    let group_id = BytesN::from_array(&env, &[(tier as u8 + 10); 32]);
    let pi = pi_membership(&env, tier);
    let commitment = pi.get(0).unwrap();
    let z = canonical_zero(&env);
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &commitment,
        &z,
        CANONICAL_THRESHOLD,
        tier,
        CANONICAL_EPOCH,
    );
    let result = client.verify_membership(&group_id, &membership_proof(&env, tier), &pi);
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

// ---- Reject paths ----

#[test]
#[should_panic(expected = "Error(Contract, #28)")]
fn test_create_group_rejects_invalid_threshold() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = pi_membership(&env, 0);
    client.create_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &0u32,
        &101u32, // out of [1, 100]
        &z,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_create_group_rejects_invalid_tier() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = pi_membership(&env, 0);
    client.create_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &3u32,
        &50u32,
        &z,
        &malformed_proof(&env),
        &pi,
    );
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
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_commitment_rejects_wrong_threshold_pi() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[7u8; 32]);
    let upi = pi_update(&env, 0);
    let c_old = upi.get(0).unwrap();
    let occ_old = upi.get(3).unwrap();
    // Inject with threshold=99 but the canonical PI uses threshold=5
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &c_old,
        &occ_old,
        99,
        0,
        CANONICAL_EPOCH,
    );
    client.update_commitment(&group_id, &demo_update_proof(&env, 0), &upi);
}

// ---- Queries ----

#[test]
fn test_get_commitment_returns_state() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[50u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 51, 1, 3);
    let entry = client.get_commitment(&group_id);
    assert_eq!(entry.tier, 1);
    assert_eq!(entry.epoch, 3);
    assert_eq!(entry.threshold_numerator, 51);
}

#[test]
fn test_bump_group_ttl_extends() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[52u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 50, 0, 0);
    client.bump_group_ttl(&group_id);
}

#[test]
fn test_vectors_consistency() {
    use serde_json::Value;
    let raw = include_str!("../test-vectors.json");
    let v: Value = serde_json::from_str(raw).expect("test-vectors.json is valid JSON");
    let errors = v["error_codes"]["vectors"].as_array().unwrap();
    let expected: &[(&str, u32)] = &[
        ("NotInitialized", Error::NotInitialized as u32),
        ("InvalidProof", Error::InvalidProof as u32),
        ("PublicInputsMismatch", Error::PublicInputsMismatch as u32),
        ("InvalidThreshold", Error::InvalidThreshold as u32),
    ];
    for (name, code) in expected {
        if let Some(entry) = errors.iter().find(|e| e["name"].as_str() == Some(name)) {
            assert_eq!(entry["code"].as_u64().unwrap() as u32, *code);
        }
    }

    let tiers = v["tier"]["vectors"].as_array().unwrap();
    for entry in tiers {
        let tier = entry["tier"].as_u64().unwrap() as u32;
        let cap = entry["capacity"].as_u64().unwrap() as u32;
        assert_eq!(tier_capacity(tier), cap);
    }
}
