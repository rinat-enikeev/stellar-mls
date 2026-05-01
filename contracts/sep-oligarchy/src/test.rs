//! Inline test suite for the SEP Oligarchy contract — PLONK-migration era.

extern crate std;

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
// Matches the threshold value baked into the oligarchy-update VK
// fixture by `build_canonical_oligarchy_update_quorum_witness` —
// `K = K_MAX = 2`, threshold = 1, slack = 1 (non-boundary so the
// production VK exercises the threshold + slack range gates
// non-trivially through the canonical witness).
const CANONICAL_THRESHOLD: u32 = 1;

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

fn non_canonical_fr(env: &Env) -> BytesN<32> {
    // 0xFF…FF is well above the BLS12-381 Fr modulus, so it fails the
    // round-trip canonicality check without colliding with any real wire value.
    BytesN::from_array(env, &[0xFFu8; 32])
}

fn be32(env: &Env, value: u64) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[24..32].copy_from_slice(&value.to_be_bytes());
    BytesN::from_array(env, &bytes)
}

fn make_update_pi(
    env: &Env,
    c_old: BytesN<32>,
    epoch_old: u64,
    c_new: BytesN<32>,
    occ_old: BytesN<32>,
    occ_new: BytesN<32>,
    threshold: u32,
) -> Vec<BytesN<32>> {
    let mut pi = Vec::new(env);
    pi.push_back(c_old);
    pi.push_back(be32(env, epoch_old));
    pi.push_back(c_new);
    pi.push_back(occ_old);
    pi.push_back(occ_new);
    pi.push_back(be32(env, threshold as u64));
    pi
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
    inject_group_with_active(
        env,
        contract_id,
        group_id,
        commitment,
        occupancy_commitment,
        threshold,
        tier,
        epoch,
        true,
    );
}

fn inject_group_with_active(
    env: &Env,
    contract_id: &Address,
    group_id: &BytesN<32>,
    commitment: &BytesN<32>,
    occupancy_commitment: &BytesN<32>,
    threshold: u32,
    tier: u32,
    epoch: u64,
    active: bool,
) {
    env.as_contract(contract_id, || {
        let entry = CommitmentEntry {
            commitment: commitment.clone(),
            epoch,
            timestamp: env.ledger().timestamp(),
            tier,
            active,
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

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_create_rejects_invalid_tier() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = pi_membership(&env, 0);
    client.create_oligarchy_group(
        &c,
        &BytesN::from_array(&env, &[2u8; 32]),
        &z,
        &3u32,
        &CANONICAL_THRESHOLD,
        &z,
        &malformed_proof(&env),
        &pi,
    );
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
#[should_panic(expected = "Error(Contract, #28)")]
fn test_create_rejects_threshold_zero() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = pi_membership(&env, 0);
    client.create_oligarchy_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &0u32,
        &0u32,
        &z,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_rejects_non_canonical_commitment() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let pi = pi_from_concat(&env, OLI_CREATE_PI, 6);
    let occ = pi.get(2).unwrap();
    client.create_oligarchy_group(
        &c,
        &BytesN::from_array(&env, &[4u8; 32]),
        &non_canonical_fr(&env),
        &0u32,
        &CANONICAL_THRESHOLD,
        &occ,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_rejects_non_canonical_occupancy() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let pi = pi_from_concat(&env, OLI_CREATE_PI, 6);
    let commitment = pi.get(0).unwrap();
    client.create_oligarchy_group(
        &c,
        &BytesN::from_array(&env, &[5u8; 32]),
        &commitment,
        &0u32,
        &CANONICAL_THRESHOLD,
        &non_canonical_fr(&env),
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_create_rejects_duplicate_group_id() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[3u8; 32]);
    let z = canonical_zero(&env);
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &z,
        &z,
        CANONICAL_THRESHOLD,
        0,
        CANONICAL_EPOCH,
    );
    let c = caller(&env);
    let pi = pi_from_concat(&env, OLI_CREATE_PI, 6);
    let commitment = pi.get(0).unwrap();
    let occ = pi.get(2).unwrap();
    client.create_oligarchy_group(
        &c,
        &group_id,
        &commitment,
        &0u32,
        &CANONICAL_THRESHOLD,
        &occ,
        &BytesN::from_array(&env, OLI_CREATE_PROOF),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #14)")]
fn test_create_restricted_mode_rejects_non_admin() {
    let (env, client, _admin) = setup_env();
    client.set_restricted_mode(&true);
    let c = caller(&env);
    let pi = pi_from_concat(&env, OLI_CREATE_PI, 6);
    let commitment = pi.get(0).unwrap();
    let occ = pi.get(2).unwrap();
    client.create_oligarchy_group(
        &c,
        &BytesN::from_array(&env, &[6u8; 32]),
        &commitment,
        &0u32,
        &CANONICAL_THRESHOLD,
        &occ,
        &BytesN::from_array(&env, OLI_CREATE_PROOF),
        &pi,
    );
}

#[test]
fn test_create_admin_can_call_in_restricted_mode() {
    let (env, client, admin) = setup_env();
    client.set_restricted_mode(&true);
    let pi = pi_from_concat(&env, OLI_CREATE_PI, 6);
    let commitment = pi.get(0).unwrap();
    let occ = pi.get(2).unwrap();
    client.create_oligarchy_group(
        &admin,
        &BytesN::from_array(&env, &[6u8; 32]),
        &commitment,
        &0u32,
        &CANONICAL_THRESHOLD,
        &occ,
        &BytesN::from_array(&env, OLI_CREATE_PROOF),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_create_enforces_tier_group_limit() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .set(&DataKey::GroupCount(0u32), &MAX_GROUPS_PER_TIER);
    });
    let c = caller(&env);
    let pi = pi_from_concat(&env, OLI_CREATE_PI, 6);
    let commitment = pi.get(0).unwrap();
    let occ = pi.get(2).unwrap();
    client.create_oligarchy_group(
        &c,
        &BytesN::from_array(&env, &[7u8; 32]),
        &commitment,
        &0u32,
        &CANONICAL_THRESHOLD,
        &occ,
        &malformed_proof(&env),
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

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_update_rejects_unknown_group() {
    let (env, client, _admin) = setup_env();
    let group_id = BytesN::from_array(&env, &[99u8; 32]);
    let pi = pi_from_concat(&env, OLI_UPDATE_PI, 6);
    client.update_commitment(&group_id, &malformed_proof(&env), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_update_rejects_inactive_group() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[60u8; 32]);
    let z = canonical_zero(&env);
    inject_group_with_active(
        &env,
        &contract_id,
        &group_id,
        &z,
        &z,
        CANONICAL_THRESHOLD,
        0,
        CANONICAL_EPOCH,
        false,
    );
    let pi = pi_from_concat(&env, OLI_UPDATE_PI, 6);
    client.update_commitment(&group_id, &malformed_proof(&env), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_rejects_stale_c_old() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[61u8; 32]);
    let upi = pi_from_concat(&env, OLI_UPDATE_PI, 6);
    let occ_old = upi.get(3).unwrap();
    let stale = BytesN::from_array(&env, &[0xCDu8; 32]);
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &stale,
        &occ_old,
        CANONICAL_THRESHOLD,
        0,
        CANONICAL_EPOCH,
    );
    client.update_commitment(&group_id, &malformed_proof(&env), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_rejects_wrong_epoch_old() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[62u8; 32]);
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
        CANONICAL_EPOCH + 1,
    );
    client.update_commitment(&group_id, &malformed_proof(&env), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_rejects_stale_occupancy_old() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[63u8; 32]);
    let upi = pi_from_concat(&env, OLI_UPDATE_PI, 6);
    let c_old = upi.get(0).unwrap();
    let stale_occ = BytesN::from_array(&env, &[0x77u8; 32]);
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &c_old,
        &stale_occ,
        CANONICAL_THRESHOLD,
        0,
        CANONICAL_EPOCH,
    );
    client.update_commitment(&group_id, &malformed_proof(&env), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_rejects_wrong_threshold() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[64u8; 32]);
    let upi = pi_from_concat(&env, OLI_UPDATE_PI, 6);
    let c_old = upi.get(0).unwrap();
    let occ_old = upi.get(3).unwrap();
    // Storage threshold ≠ wire threshold (PI[5] is CANONICAL_THRESHOLD).
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &c_old,
        &occ_old,
        CANONICAL_THRESHOLD + 1,
        0,
        CANONICAL_EPOCH,
    );
    client.update_commitment(&group_id, &malformed_proof(&env), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_update_rejects_non_canonical_c_new() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[65u8; 32]);
    let upi_canonical = pi_from_concat(&env, OLI_UPDATE_PI, 6);
    let c_old = upi_canonical.get(0).unwrap();
    let occ_old = upi_canonical.get(3).unwrap();
    let occ_new = upi_canonical.get(4).unwrap();
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
    let pi = make_update_pi(
        &env,
        c_old,
        CANONICAL_EPOCH,
        non_canonical_fr(&env),
        occ_old,
        occ_new,
        CANONICAL_THRESHOLD,
    );
    client.update_commitment(&group_id, &malformed_proof(&env), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_update_rejects_non_canonical_occupancy_new() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[66u8; 32]);
    let upi_canonical = pi_from_concat(&env, OLI_UPDATE_PI, 6);
    let c_old = upi_canonical.get(0).unwrap();
    let c_new = upi_canonical.get(2).unwrap();
    let occ_old = upi_canonical.get(3).unwrap();
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
    let pi = make_update_pi(
        &env,
        c_old,
        CANONICAL_EPOCH,
        c_new,
        occ_old,
        non_canonical_fr(&env),
        CANONICAL_THRESHOLD,
    );
    client.update_commitment(&group_id, &malformed_proof(&env), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_update_rejects_replayed_proof() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[67u8; 32]);
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
    // Pre-record the canonical update proof's hash to simulate replay.
    let hash: BytesN<32> = env.as_contract(&contract_id, || {
        let preimage = Bytes::from_slice(&env, OLI_UPDATE_PROOF);
        env.crypto().sha256(&preimage).into()
    });
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsedProof(hash), &true);
    });
    client.update_commitment(&group_id, &BytesN::from_array(&env, OLI_UPDATE_PROOF), &upi);
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
#[should_panic(expected = "Error(Contract, #10)")]
fn test_verify_membership_rejects_wrong_commitment() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[40u8; 32]);
    let pi = pi_membership(&env, 0);
    let stored = BytesN::from_array(&env, &[0x33u8; 32]);
    let z = canonical_zero(&env);
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &stored,
        &z,
        CANONICAL_THRESHOLD,
        0,
        CANONICAL_EPOCH,
    );
    client.verify_membership(&group_id, &membership_proof(&env, 0), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_verify_membership_rejects_wrong_epoch() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[41u8; 32]);
    let pi = pi_membership(&env, 0);
    let commitment = pi.get(0).unwrap();
    let z = canonical_zero(&env);
    // PI[1] in the fixture encodes epoch=0; injecting epoch=42 forces a
    // mismatch on the contract's epoch-binding check before the proof verifier
    // is reached.
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &commitment,
        &z,
        CANONICAL_THRESHOLD,
        0,
        42,
    );
    client.verify_membership(&group_id, &membership_proof(&env, 0), &pi);
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
#[should_panic(expected = "Error(Contract, #5)")]
fn test_get_commitment_rejects_unknown_group() {
    let (_env, client, _admin) = setup_env();
    let env = client.env.clone();
    let group_id = BytesN::from_array(&env, &[0xEFu8; 32]);
    client.get_commitment(&group_id);
}

#[test]
fn test_bump_group_ttl_extends() {
    // TTL extension is not directly observable from contract scope; this is a
    // smoke test that the call succeeds and the entry remains readable. Pair
    // with `test_bump_group_ttl_rejects_unknown_group` for negative coverage.
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[52u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 50, 0, 0);
    client.bump_group_ttl(&group_id);
    let _ = client.get_commitment(&group_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_bump_group_ttl_rejects_unknown_group() {
    let (env, client, _admin) = setup_env();
    let group_id = BytesN::from_array(&env, &[0xFEu8; 32]);
    client.bump_group_ttl(&group_id);
}

#[test]
fn test_vectors_consistency() {
    use serde_json::Value;
    let raw = include_str!("../test-vectors.json");
    let v: Value = serde_json::from_str(raw).expect("test-vectors.json is valid JSON");

    // Every error variant the contract surfaces — pinned to the JSON's vectors
    // list so renames/drops in either side fail the build. Reserved3 is in the
    // JSON but not the contract enum (3 is intentionally unused).
    let expected: &[(&str, u32)] = &[
        ("NotInitialized", Error::NotInitialized as u32),
        ("AlreadyInitialized", Error::AlreadyInitialized as u32),
        ("GroupAlreadyExists", Error::GroupAlreadyExists as u32),
        ("GroupNotFound", Error::GroupNotFound as u32),
        ("GroupInactive", Error::GroupInactive as u32),
        ("InvalidProof", Error::InvalidProof as u32),
        ("InvalidTier", Error::InvalidTier as u32),
        ("PublicInputsMismatch", Error::PublicInputsMismatch as u32),
        ("InvalidEpoch", Error::InvalidEpoch as u32),
        ("ProofReplay", Error::ProofReplay as u32),
        ("TierGroupLimitReached", Error::TierGroupLimitReached as u32),
        ("AdminOnly", Error::AdminOnly as u32),
        (
            "InvalidCommitmentEncoding",
            Error::InvalidCommitmentEncoding as u32,
        ),
        ("InvalidThreshold", Error::InvalidThreshold as u32),
    ];
    let errors = v["error_codes"]["vectors"].as_array().unwrap();
    for (name, code) in expected {
        let entry = errors
            .iter()
            .find(|e| e["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("test-vectors.json missing error variant {name}"));
        assert_eq!(
            entry["code"].as_u64().unwrap() as u32,
            *code,
            "code drift for {name}",
        );
    }

    // Tier capacities — pinned to the contract's tier_capacity table.
    let tiers = v["tier"]["vectors"].as_array().unwrap();
    for entry in tiers {
        let tier = entry["tier"].as_u64().unwrap() as u32;
        let capacity = entry["capacity"].as_u64().unwrap() as u32;
        assert_eq!(
            tier_capacity(tier),
            capacity,
            "tier_capacity drift for tier {tier}",
        );
    }
    // Tier 3 is rejected by the contract; tier_capacity returns 0 as a sentinel.
    assert_eq!(tier_capacity(3), 0);

    // Cross-group cap pinned to the JSON.
    let max_groups = v["max_groups_per_tier"]["value"].as_u64().unwrap() as u32;
    assert_eq!(MAX_GROUPS_PER_TIER, max_groups);
}

// ================================================================
// Gas benchmarks (Phase C.5)
// ================================================================

fn bench_verify_membership_at_tier(tier: u32) {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[(40 + tier as u8); 32]);
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

    env.cost_estimate().budget().reset_tracker();
    let result = client.verify_membership(&group_id, &membership_proof(&env, tier), &pi);
    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();

    assert!(result, "tier {tier} membership proof should verify");
    std::eprintln!(
        "[gas-bench] sep-oligarchy verify_membership(tier={}): cpu={} mem={}",
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

/// Single-tier (admin tree depth fixed at 5 across all member tiers).
#[test]
fn bench_create_oligarchy_group() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let pi = pi_from_concat(&env, OLI_CREATE_PI, 6);
    let commitment = pi.get(0).unwrap();
    let occ = pi.get(2).unwrap();

    env.cost_estimate().budget().reset_tracker();
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
    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();

    std::eprintln!(
        "[gas-bench] sep-oligarchy create_oligarchy_group: cpu={} mem={}",
        cpu, mem
    );
}

#[test]
fn bench_update_commitment() {
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

    env.cost_estimate().budget().reset_tracker();
    client.update_commitment(&group_id, &BytesN::from_array(&env, OLI_UPDATE_PROOF), &upi);
    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();

    std::eprintln!(
        "[gas-bench] sep-oligarchy update_commitment: cpu={} mem={}",
        cpu, mem
    );
}
