//! Inline test suite for the SEP Oligarchy contract — PLONK-migration era.

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

const OLI_CREATE_PROOF: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/oligarchy-create-proof.bin");
const OLI_CREATE_PI: &[u8; 192] =
    include_bytes!("../../plonk-verifier/tests/fixtures/oligarchy-create-pi.bin");
const OLI_UPDATE_PROOF: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/oligarchy-update-proof.bin");
const OLI_UPDATE_PI: &[u8; 192] =
    include_bytes!("../../plonk-verifier/tests/fixtures/oligarchy-update-pi.bin");

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

fn pi_from_concat(env: &Env, bytes: &[u8], n: usize) -> Vec<BytesN<32>> {
    let mut pi = Vec::new(env);
    for i in 0..n {
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

fn setup_env() -> (Env, SepOligarchyContractClient<'static>, Address) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(SepOligarchyContract, (admin.clone(),));
    let client = SepOligarchyContractClient::new(&env, &contract_id);
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
    threshold: u32,
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
            admin_threshold_numerator: threshold,
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

/// **Load-bearing.** Canonical oligarchy create proof verifies on-chain.
#[test]
fn test_create_oligarchy_group_happy_path() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let pi = pi_from_concat(&env, OLI_CREATE_PI, 6);
    let commitment = pi.get(0).unwrap();
    let occ = pi.get(2).unwrap();
    client.create_oligarchy_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &commitment,
        &0u32,
        &CANONICAL_THRESHOLD,
        &occ,
        &BytesN::from_array(&env, OLI_CREATE_PROOF),
        &pi,
    );
}

/// **Load-bearing.** Canonical oligarchy update proof verifies.
#[test]
fn test_update_commitment_happy_path() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[7u8; 32]);
    let upi = pi_from_concat(&env, OLI_UPDATE_PI, 6);
    let c_old = upi.get(0).unwrap();
    let occ_old = upi.get(3).unwrap();
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &c_old,
        &occ_old,
        CANONICAL_THRESHOLD,
        0,
        CANONICAL_EPOCH,
    );
    client.update_commitment(&group_id, &BytesN::from_array(&env, OLI_UPDATE_PROOF), &upi);

    let post = client.get_commitment(&group_id);
    assert_eq!(post.commitment, upi.get(2).unwrap());
    assert_eq!(post.epoch, CANONICAL_EPOCH + 1);
    assert_eq!(post.admin_threshold_numerator, CANONICAL_THRESHOLD);
}

/// **Load-bearing.** Multi-tier verify_membership using anarchy VKs.
fn run_verify_membership(tier: u32) {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[(tier as u8 + 30); 32]);
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
    run_verify_membership(0);
}

#[test]
fn test_verify_membership_happy_path_d8() {
    run_verify_membership(1);
}

#[test]
fn test_verify_membership_happy_path_d11() {
    run_verify_membership(2);
}

#[test]
#[should_panic(expected = "Error(Contract, #28)")]
fn test_create_rejects_invalid_threshold() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = pi_membership(&env, 0);
    client.create_oligarchy_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &0u32,
        &101u32,
        &z,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_update_rejects_unknown_group() {
    let (env, client, _admin) = setup_env();
    let group_id = BytesN::from_array(&env, &[99u8; 32]);
    let pi = pi_from_concat(&env, OLI_UPDATE_PI, 6);
    client.update_commitment(&group_id, &malformed_proof(&env), &pi);
}

#[test]
fn test_get_commitment_returns_state() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[50u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 51, 1, 3);
    let entry = client.get_commitment(&group_id);
    assert_eq!(entry.tier, 1);
    assert_eq!(entry.admin_threshold_numerator, 51);
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
    ];
    for (name, code) in expected {
        if let Some(entry) = errors.iter().find(|e| e["name"].as_str() == Some(name)) {
            assert_eq!(entry["code"].as_u64().unwrap() as u32, *code);
        }
    }
    let _ = tier_capacity(0);
}
