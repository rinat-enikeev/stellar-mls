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
// Matches the threshold value baked into democracy-update fixtures
// across all three tiers. The quorum circuit at d=5/d=8 is exercised
// non-trivially with `slack = K_MAX - threshold = 1`; the simplified
// circuit at d=11 doesn't constrain threshold but uses the same value
// for cross-tier consistency.
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

/// 32 bytes of `0xFF` exceeds the BLS12-381 Fr modulus, so
/// `Fr::from_bytes ∘ Fr::to_bytes` reduces it and the round-trip
/// fails — the contract surfaces this as `InvalidCommitmentEncoding`.
fn non_canonical_fr(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0xFFu8; 32])
}

/// Big-endian u64 padded to 32 bytes — matches `be32_from_u64` in lib.rs
/// and is what `update_commitment` / `verify_membership` compare against
/// for `epoch_old` and `threshold_numerator` PIs.
fn be32(env: &Env, value: u64) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[24..32].copy_from_slice(&value.to_be_bytes());
    BytesN::from_array(env, &bytes)
}

/// PI vector `[commitment, be32(0)]` matching `create_group`'s
/// membership-PI shape so wire-validation passes and the test exercises
/// later branches (group_exists, tier limit, etc.).
fn pi_create_for(env: &Env, commitment: &BytesN<32>) -> Vec<BytesN<32>> {
    let mut pi = Vec::new(env);
    pi.push_back(commitment.clone());
    pi.push_back(be32(env, 0));
    pi
}

fn inject_inactive_group(
    env: &Env,
    contract_id: &Address,
    group_id: &BytesN<32>,
    tier: u32,
) {
    let z = canonical_zero(env);
    env.as_contract(contract_id, || {
        let entry = CommitmentEntry {
            commitment: z.clone(),
            epoch: 0,
            timestamp: env.ledger().timestamp(),
            tier,
            active: false,
            occupancy_commitment: z,
            threshold_numerator: 50,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Group(group_id.clone()), &entry);
    });
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
    // Inject with threshold=99 but the canonical PI uses threshold=CANONICAL_THRESHOLD (1)
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

// ---- Regression: create_group validation rejections ----

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_group_rejects_non_canonical_commitment() {
    let (env, client, _admin) = setup_env();
    let bad = non_canonical_fr(&env);
    let z = canonical_zero(&env);
    let pi = pi_create_for(&env, &bad);
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[1u8; 32]),
        &bad,
        &0u32,
        &50u32,
        &z,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_group_rejects_non_canonical_occupancy_commitment() {
    let (env, client, _admin) = setup_env();
    let z = canonical_zero(&env);
    let bad_occ = non_canonical_fr(&env);
    let pi = pi_create_for(&env, &z);
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[2u8; 32]),
        &z,
        &0u32,
        &50u32,
        &bad_occ,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_create_group_rejects_duplicate_group_id() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[3u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 50, 0, 0);
    let pi = pi_create_for(&env, &z);
    client.create_group(
        &caller(&env),
        &group_id,
        &z,
        &0u32,
        &50u32,
        &z,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_create_group_enforces_tier_group_limit() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .set(&DataKey::GroupCount(0u32), &10_000u32);
    });
    let z = canonical_zero(&env);
    let pi = pi_create_for(&env, &z);
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[42u8; 32]),
        &z,
        &0u32,
        &50u32,
        &z,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #14)")]
fn test_create_group_restricted_mode_rejects_non_admin() {
    // Admin enables restricted mode (mock_all_auths covers caller.
    // require_auth), then a non-admin caller hits the contract-level
    // `caller != admin` value check and returns AdminOnly.
    let (env, client, _admin) = setup_env();
    client.set_restricted_mode(&true);
    let z = canonical_zero(&env);
    let pi = pi_create_for(&env, &z);
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[55u8; 32]),
        &z,
        &0u32,
        &50u32,
        &z,
        &malformed_proof(&env),
        &pi,
    );
}

// ---- Regression: update_commitment validation rejections ----

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_update_commitment_rejects_inactive_group() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[60u8; 32]);
    inject_inactive_group(&env, &contract_id, &group_id, 0);
    let upi = pi_update(&env, 0);
    client.update_commitment(&group_id, &demo_update_proof(&env, 0), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_commitment_rejects_stale_c_old() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[61u8; 32]);
    let stored = BytesN::from_array(&env, &[0xAAu8; 32]); // diverges from upi[0]
    let upi = pi_update(&env, 0);
    let occ_old = upi.get(3).unwrap();
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &stored,
        &occ_old,
        CANONICAL_THRESHOLD,
        0,
        CANONICAL_EPOCH,
    );
    client.update_commitment(&group_id, &demo_update_proof(&env, 0), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_commitment_rejects_wrong_epoch_old() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[62u8; 32]);
    let upi = pi_update(&env, 0);
    let c_old = upi.get(0).unwrap();
    let occ_old = upi.get(3).unwrap();
    // Inject with a stored epoch that diverges from upi[1].
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
    client.update_commitment(&group_id, &demo_update_proof(&env, 0), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_commitment_rejects_stale_occupancy_commitment_old() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[63u8; 32]);
    let upi = pi_update(&env, 0);
    let c_old = upi.get(0).unwrap();
    let stored_occ = BytesN::from_array(&env, &[0xBBu8; 32]); // diverges from upi[3]
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &c_old,
        &stored_occ,
        CANONICAL_THRESHOLD,
        0,
        CANONICAL_EPOCH,
    );
    client.update_commitment(&group_id, &demo_update_proof(&env, 0), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_update_commitment_rejects_non_canonical_c_new() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[64u8; 32]);
    let upi = pi_update(&env, 0);
    let c_old = upi.get(0).unwrap();
    let epoch_old_be = upi.get(1).unwrap();
    let occ_old = upi.get(3).unwrap();
    let occ_new = upi.get(4).unwrap();
    let threshold_pi = upi.get(5).unwrap();
    // Decode stored epoch from PI[1] so the stored.epoch check passes.
    let mut be = [0u8; 8];
    be.copy_from_slice(&epoch_old_be.to_array()[24..32]);
    let stored_epoch = u64::from_be_bytes(be);
    // Decode stored threshold from PI[5].
    be.copy_from_slice(&threshold_pi.to_array()[24..32]);
    let stored_threshold = u64::from_be_bytes(be) as u32;
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &c_old,
        &occ_old,
        stored_threshold,
        0,
        stored_epoch,
    );
    // Replace c_new (PI[2]) with a non-canonical Fr.
    let mut tampered = Vec::new(&env);
    tampered.push_back(c_old.clone());
    tampered.push_back(epoch_old_be);
    tampered.push_back(non_canonical_fr(&env));
    tampered.push_back(occ_old);
    tampered.push_back(occ_new);
    tampered.push_back(threshold_pi);
    client.update_commitment(&group_id, &demo_update_proof(&env, 0), &tampered);
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_update_commitment_rejects_non_canonical_occupancy_commitment_new() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[65u8; 32]);
    let upi = pi_update(&env, 0);
    let c_old = upi.get(0).unwrap();
    let epoch_old_be = upi.get(1).unwrap();
    let c_new = upi.get(2).unwrap();
    let occ_old = upi.get(3).unwrap();
    let threshold_pi = upi.get(5).unwrap();
    let mut be = [0u8; 8];
    be.copy_from_slice(&epoch_old_be.to_array()[24..32]);
    let stored_epoch = u64::from_be_bytes(be);
    be.copy_from_slice(&threshold_pi.to_array()[24..32]);
    let stored_threshold = u64::from_be_bytes(be) as u32;
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &c_old,
        &occ_old,
        stored_threshold,
        0,
        stored_epoch,
    );
    let mut tampered = Vec::new(&env);
    tampered.push_back(c_old.clone());
    tampered.push_back(epoch_old_be);
    tampered.push_back(c_new);
    tampered.push_back(occ_old);
    tampered.push_back(non_canonical_fr(&env));
    tampered.push_back(threshold_pi);
    client.update_commitment(&group_id, &demo_update_proof(&env, 0), &tampered);
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_update_commitment_rejects_replayed_proof() {
    // Pre-record the SHA256 of the happy-path proof so the replay check
    // fires before the verifier gets a chance to accept it.
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[66u8; 32]);
    let upi = pi_update(&env, 0);
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
    let proof = demo_update_proof(&env, 0);
    env.as_contract(&contract_id, || {
        let preimage = Bytes::from_slice(&env, proof.to_array().as_slice());
        let hash: BytesN<32> = env.crypto().sha256(&preimage).into();
        env.storage()
            .persistent()
            .set(&DataKey::UsedProof(hash), &true);
    });
    client.update_commitment(&group_id, &proof, &upi);
}

// ---- Regression: verify_membership rejections ----

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_verify_membership_rejects_wrong_commitment() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[70u8; 32]);
    let stored = BytesN::from_array(&env, &[0xCCu8; 32]);
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
    let pi = pi_membership(&env, 0); // PI[0] is the fixture's commitment, not `stored`
    client.verify_membership(&group_id, &membership_proof(&env, 0), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_verify_membership_rejects_wrong_epoch() {
    // Stored epoch diverges from PI[1] (the fixture encodes
    // `be32(CANONICAL_EPOCH)` — pinned by run_verify_membership_happy_
    // path), so the entrypoint's `epoch != be32_from_u64(stored.epoch)`
    // check fires before the verifier sees the proof. Parallels
    // test_verify_membership_rejects_wrong_commitment for the second
    // membership PI slot.
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[72u8; 32]);
    let pi = pi_membership(&env, 0);
    let commitment = pi.get(0).unwrap();
    let z = canonical_zero(&env);
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &commitment,
        &z,
        CANONICAL_THRESHOLD,
        0,
        CANONICAL_EPOCH + 1, // diverges from PI[1] = be32(CANONICAL_EPOCH)
    );
    client.verify_membership(&group_id, &membership_proof(&env, 0), &pi);
}

#[test]
fn test_verify_membership_inactive_group_returns_false() {
    // verify_membership is read-only and intentionally works on groups
    // whose `active` flag is false. With an aligned-but-malformed proof
    // the verifier rejects → Ok(false) (no error surfaced).
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[71u8; 32]);
    inject_inactive_group(&env, &contract_id, &group_id, 0);
    let z = canonical_zero(&env);
    let mut pi = Vec::new(&env);
    pi.push_back(z.clone());
    pi.push_back(be32(&env, 0));
    let result = client.verify_membership(&group_id, &malformed_proof(&env), &pi);
    assert!(!result, "malformed proof against inactive group should be Ok(false)");
}

// ---- Regression: archive + history ----

#[test]
fn test_get_history_returns_chronological_entries() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[80u8; 32]);
    let z = canonical_zero(&env);
    inject_group(
        &env, &contract_id, &group_id, &z, &z, CANONICAL_THRESHOLD, 0, 0,
    );
    env.as_contract(&contract_id, || {
        let mut history: Vec<CommitmentEntry> = Vec::new(&env);
        for i in 0u64..3 {
            history.push_back(CommitmentEntry {
                commitment: BytesN::from_array(&env, &[i as u8; 32]),
                epoch: i,
                timestamp: 100 + i,
                tier: 0,
                active: true,
                occupancy_commitment: z.clone(),
                threshold_numerator: CANONICAL_THRESHOLD,
            });
        }
        env.storage()
            .persistent()
            .set(&DataKey::History(group_id.clone()), &history);
    });
    let result = client.get_history(&group_id, &10u32);
    assert_eq!(result.len(), 3);
    assert_eq!(result.get(0).unwrap().epoch, 0);
    assert_eq!(result.get(1).unwrap().epoch, 1);
    assert_eq!(result.get(2).unwrap().epoch, 2);
}

#[test]
fn test_archive_entry_appends_and_prunes_at_window() {
    // Drive HISTORY_WINDOW + 6 appends through the actual archive_entry
    // helper (the same path update_commitment funnels through on every
    // accept) so the rolling-window prune behaviour is pinned end-to-end
    // — not just the post-prune `History` shape.
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[81u8; 32]);
    let z = canonical_zero(&env);
    inject_group(
        &env, &contract_id, &group_id, &z, &z, CANONICAL_THRESHOLD, 0, 0,
    );
    let total: u64 = (HISTORY_WINDOW as u64) + 6; // exceeds cap by 6
    env.as_contract(&contract_id, || {
        for i in 0u64..total {
            let entry = CommitmentEntry {
                commitment: BytesN::from_array(&env, &[(i & 0xff) as u8; 32]),
                epoch: i,
                timestamp: 1000 + i,
                tier: 0,
                active: true,
                occupancy_commitment: z.clone(),
                threshold_numerator: CANONICAL_THRESHOLD,
            };
            SepDemocracyContract::archive_entry(&env, &group_id, &entry);
        }
    });
    let history = client.get_history(&group_id, &(2 * HISTORY_WINDOW));
    assert_eq!(history.len(), HISTORY_WINDOW);
    assert_eq!(
        history.get(0).unwrap().epoch,
        total - HISTORY_WINDOW as u64,
    );
    assert_eq!(history.get(history.len() - 1).unwrap().epoch, total - 1);
}

#[test]
fn test_vectors_consistency() {
    use serde_json::Value;
    let raw = include_str!("../test-vectors.json");
    let v: Value = serde_json::from_str(raw).expect("test-vectors.json is valid JSON");
    let errors = v["error_codes"]["vectors"].as_array().unwrap();
    // Cover every live Error variant. Missing-from-JSON now hard-fails
    // instead of silently skipping (`unwrap_or_else(panic!)` rather than
    // `if let Some`), so contract↔vectors drift breaks the build.
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
        ("InvalidCommitmentEncoding", Error::InvalidCommitmentEncoding as u32),
        ("InvalidThreshold", Error::InvalidThreshold as u32),
    ];
    for (name, code) in expected {
        let entry = errors
            .iter()
            .find(|e| e["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("test-vectors.json missing error {}", name));
        let json_code = entry["code"].as_u64().unwrap() as u32;
        assert_eq!(
            json_code, *code,
            "error code drift for {}: vectors say {}, contract says {}",
            name, json_code, code,
        );
    }

    let tiers = v["tier"]["vectors"].as_array().unwrap();
    for entry in tiers {
        let tier = entry["tier"].as_u64().unwrap() as u32;
        let cap = entry["capacity"].as_u64().unwrap() as u32;
        assert_eq!(tier_capacity(tier), cap);
    }
}
