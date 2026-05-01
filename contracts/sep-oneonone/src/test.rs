//! Inline test suite for the SEP 1v1 contract — PLONK-migration era.

extern crate std;

use super::*;
use soroban_sdk::testutils::Address as _;

// ================================================================
// Canonical fixtures
// ================================================================

const MEMBERSHIP_PROOF_BYTES: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/proof-d5.bin");
const MEMBERSHIP_PI_BYTES: &[u8; 64] =
    include_bytes!("../../plonk-verifier/tests/fixtures/pi-d5.bin");
const ONEONONE_CREATE_PROOF_BYTES: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/oneonone-create-proof.bin");
const ONEONONE_CREATE_PI_BYTES: &[u8; 64] =
    include_bytes!("../../plonk-verifier/tests/fixtures/oneonone-create-pi.bin");

const CANONICAL_MEMBERSHIP_EPOCH: u64 = 1234;

fn membership_proof(env: &Env) -> BytesN<1601> {
    BytesN::from_array(env, MEMBERSHIP_PROOF_BYTES)
}

fn membership_commitment(env: &Env) -> BytesN<32> {
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&MEMBERSHIP_PI_BYTES[..32]);
    BytesN::from_array(env, &arr)
}

fn create_proof(env: &Env) -> BytesN<1601> {
    BytesN::from_array(env, ONEONONE_CREATE_PROOF_BYTES)
}

fn create_commitment(env: &Env) -> BytesN<32> {
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&ONEONONE_CREATE_PI_BYTES[..32]);
    BytesN::from_array(env, &arr)
}

fn create_pi(env: &Env) -> Vec<BytesN<32>> {
    let mut pi = Vec::new(env);
    pi.push_back(create_commitment(env));
    pi.push_back(be32_from_u64(env, 0));
    pi
}

fn membership_pi(env: &Env, commitment: BytesN<32>, epoch: u64) -> Vec<BytesN<32>> {
    let mut pi = Vec::new(env);
    pi.push_back(commitment);
    pi.push_back(be32_from_u64(env, epoch));
    pi
}

fn canonical_zero(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

fn non_canonical_fr(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0xff; 32])
}

fn malformed_proof(env: &Env) -> BytesN<1601> {
    BytesN::from_array(env, &[0xAAu8; 1601])
}

// ================================================================
// Setup
// ================================================================

fn setup_env() -> (Env, SepOneOnOneContractClient<'static>, Address) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(SepOneOnOneContract, (admin.clone(),));
    let client = SepOneOnOneContractClient::new(&env, &contract_id);
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
    epoch: u64,
) {
    env.as_contract(contract_id, || {
        let entry = CommitmentEntry {
            commitment: commitment.clone(),
            epoch,
            timestamp: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Group(group_id.clone()), &entry);
        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::GroupCount)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::GroupCount, &(count + 1));
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
// 2. create_group
// ================================================================

/// **Load-bearing.** Canonical 1v1 create proof verifies on-chain.
#[test]
fn test_create_group_happy_path() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let commitment = create_commitment(&env);
    let pi = create_pi(&env);
    let group_id = BytesN::from_array(&env, &[1u8; 32]);
    client.create_group(&c, &group_id, &commitment, &create_proof(&env), &pi);

    let entry = client.get_commitment(&group_id);
    assert_eq!(entry.commitment, commitment);
    assert_eq!(entry.epoch, 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_create_group_rejects_duplicate_group_id() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[1u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, 0);

    let c = caller(&env);
    let pi = membership_pi(&env, z.clone(), 0);
    client.create_group(&c, &group_id, &z, &malformed_proof(&env), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_group_rejects_non_canonical_commitment() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let bad = non_canonical_fr(&env);
    let pi = membership_pi(&env, bad.clone(), 0);
    client.create_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &bad,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
fn test_create_group_rejects_invalid_proof() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = membership_pi(&env, z.clone(), 0);
    let r = client.try_create_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &malformed_proof(&env),
        &pi,
    );
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof, got {:?}", other),
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_create_group_rejects_pi_count_mismatch() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let mut pi = Vec::new(&env);
    pi.push_back(z.clone());
    client.create_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_create_group_rejects_pi_commitment_mismatch() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = membership_pi(&env, BytesN::from_array(&env, &[7u8; 32]), 0);
    client.create_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_create_group_rejects_pi_epoch_nonzero() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = membership_pi(&env, z.clone(), 1);
    client.create_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
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
    let z = canonical_zero(&env);
    let pi = membership_pi(&env, z.clone(), 0);
    client.create_group(
        &c,
        &BytesN::from_array(&env, &[55u8; 32]),
        &z,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_create_group_rejects_replayed_proof() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let commitment = create_commitment(&env);
    let pi = create_pi(&env);
    let group_id_a = BytesN::from_array(&env, &[1u8; 32]);
    client.create_group(&c, &group_id_a, &commitment, &create_proof(&env), &pi);

    // Same proof, distinct group_id → ProofReplay (#12).
    let group_id_b = BytesN::from_array(&env, &[2u8; 32]);
    client.create_group(&c, &group_id_b, &commitment, &create_proof(&env), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_create_group_enforces_count_limit() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .set(&DataKey::GroupCount, &10_000u32);
    });
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = membership_pi(&env, z.clone(), 0);
    client.create_group(
        &c,
        &BytesN::from_array(&env, &[42u8; 32]),
        &z,
        &malformed_proof(&env),
        &pi,
    );
}

// ================================================================
// 3. verify_membership
// ================================================================

/// **Load-bearing.** Canonical membership proof verifies on-chain.
#[test]
fn test_verify_membership_happy_path() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[30u8; 32]);
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &membership_commitment(&env),
        CANONICAL_MEMBERSHIP_EPOCH,
    );
    let pi = membership_pi(
        &env,
        membership_commitment(&env),
        CANONICAL_MEMBERSHIP_EPOCH,
    );
    let result = client.verify_membership(&group_id, &membership_proof(&env), &pi);
    assert!(result, "canonical membership proof should verify");
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_verify_membership_rejects_wrong_commitment() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[31u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, 0);
    let pi = membership_pi(&env, BytesN::from_array(&env, &[7u8; 32]), 0);
    client.verify_membership(&group_id, &malformed_proof(&env), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_verify_membership_rejects_wrong_epoch() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[32u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, 5);
    let pi = membership_pi(&env, z, 4);
    client.verify_membership(&group_id, &malformed_proof(&env), &pi);
}

/// Pins the trap-vs-`Ok(false)` boundary documented at
/// `verify_membership`'s docstring: a byte-valid PLONK proof for a
/// *different* circuit (the canonical create proof, valid against
/// `ONEONONE_CREATE_VK`) parses cleanly through `parse_proof_bytes`,
/// fails verification against `MEMBERSHIP_VK`, and surfaces as
/// `Err(InvalidProof) → Ok(false)` — i.e. it does NOT trap in BLS
/// host primitives. Catches a regression where the verifier glue
/// drifts toward propagating verify-failure as a host trap.
#[test]
fn test_verify_membership_well_formed_wrong_vk_returns_false() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[33u8; 32]);
    let commitment = create_commitment(&env);
    inject_group(&env, &contract_id, &group_id, &commitment, 0);
    let pi = membership_pi(&env, commitment, 0);
    let result = client.verify_membership(&group_id, &create_proof(&env), &pi);
    assert!(
        !result,
        "well-formed PLONK proof for the wrong VK must return Ok(false), not trap"
    );
}

// ================================================================
// 4. Admin entrypoints
// ================================================================

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_set_restricted_mode_requires_auth() {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    let admin = Address::generate(&env);
    let contract_id = env.register(SepOneOnOneContract, (admin,));
    let client = SepOneOnOneContractClient::new(&env, &contract_id);
    client.set_restricted_mode(&true);
}

// ================================================================
// 5. Queries
// ================================================================

#[test]
fn test_get_commitment_returns_state() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[50u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, 3);
    let entry = client.get_commitment(&group_id);
    assert_eq!(entry.commitment, z);
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
    inject_group(&env, &contract_id, &group_id, &z, 0);
    client.bump_group_ttl(&group_id);
    let post = client.get_commitment(&group_id);
    assert_eq!(post.commitment, z);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_bump_group_ttl_rejects_unknown() {
    let (_env, client, _admin) = setup_env();
    let env = &client.env;
    let group_id = BytesN::from_array(env, &[99u8; 32]);
    client.bump_group_ttl(&group_id);
}

// ================================================================
// 6. test-vectors.json consistency
// ================================================================

#[test]
fn test_vectors_consistency() {
    use serde_json::Value;
    let raw = include_str!("../test-vectors.json");
    let v: Value = serde_json::from_str(raw).expect("test-vectors.json is valid JSON");

    let errors = v["error_codes"]["vectors"]
        .as_array()
        .expect("error_codes.vectors is an array");
    let expected: &[(&str, u32)] = &[
        ("NotInitialized", Error::NotInitialized as u32),
        ("AlreadyInitialized", Error::AlreadyInitialized as u32),
        ("GroupAlreadyExists", Error::GroupAlreadyExists as u32),
        ("GroupNotFound", Error::GroupNotFound as u32),
        ("InvalidProof", Error::InvalidProof as u32),
        ("PublicInputsMismatch", Error::PublicInputsMismatch as u32),
        ("ProofReplay", Error::ProofReplay as u32),
        ("GroupCountLimitReached", Error::GroupCountLimitReached as u32),
        ("AdminOnly", Error::AdminOnly as u32),
        ("InvalidCommitmentEncoding", Error::InvalidCommitmentEncoding as u32),
    ];
    for (name, code) in expected {
        let entry = errors
            .iter()
            .find(|e| e["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("test-vectors.json missing error {}", name));
        let json_code = entry["code"].as_u64().unwrap() as u32;
        assert_eq!(json_code, *code, "error code drift for {}", name);
    }

    let max = v["max_groups"]["value"].as_u64().unwrap() as u32;
    assert_eq!(max, MAX_GROUPS, "MAX_GROUPS drift");

    let test_count = v["tests_to_implement"]["categories"]["total"]
        .as_u64()
        .unwrap();
    assert_eq!(test_count, 21, "test count pin drift");
}

// ================================================================
// Gas benchmarks (Phase C.5)
// ================================================================

#[test]
fn bench_verify_membership() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[40u8; 32]);
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &membership_commitment(&env),
        CANONICAL_MEMBERSHIP_EPOCH,
    );
    let pi = membership_pi(
        &env,
        membership_commitment(&env),
        CANONICAL_MEMBERSHIP_EPOCH,
    );

    env.cost_estimate().budget().reset_tracker();
    let result = client.verify_membership(&group_id, &membership_proof(&env), &pi);
    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();

    assert!(result, "canonical 1v1 membership proof should verify");
    std::eprintln!(
        "[gas-bench] sep-oneonone verify_membership: cpu={} mem={}",
        cpu, mem
    );
}

#[test]
fn bench_create_group() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let commitment = create_commitment(&env);
    let pi = create_pi(&env);
    let group_id = BytesN::from_array(&env, &[42u8; 32]);

    env.cost_estimate().budget().reset_tracker();
    client.create_group(&c, &group_id, &commitment, &create_proof(&env), &pi);
    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();

    std::eprintln!(
        "[gas-bench] sep-oneonone create_group: cpu={} mem={}",
        cpu, mem
    );
}
