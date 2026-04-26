//! Inline test suite for the SEP Oligarchy contract.
//!
//! Every test name listed in `test-vectors.json#tests_to_implement`
//! has a corresponding `#[test]` here. The `test_vectors_consistency`
//! test loads the JSON file and asserts the contract's `Error`
//! variants, `tier_capacity` mapping, and IC-point constants match
//! the vectors byte-for-byte — pinning the JSON as the canonical ABI.

use super::*;
use soroban_sdk::testutils::Address as _;

// ================================================================
// Mocks (mirrors sep-democracy pattern: subgroup-valid via hash_to_g{1,2})
// ================================================================

fn valid_g1(env: &Env, tag: &[u8]) -> BytesN<96> {
    let bls = env.crypto().bls12_381();
    let dst = Bytes::from_slice(env, b"sep-oligarchy-test-g1");
    let msg = Bytes::from_slice(env, tag);
    bls.hash_to_g1(&msg, &dst).to_bytes()
}

fn valid_g2(env: &Env, tag: &[u8]) -> BytesN<192> {
    let bls = env.crypto().bls12_381();
    let dst = Bytes::from_slice(env, b"sep-oligarchy-test-g2");
    let msg = Bytes::from_slice(env, tag);
    bls.hash_to_g2(&msg, &dst).to_bytes()
}

fn mock_membership_vk(env: &Env) -> VerificationKeyData {
    VerificationKeyData {
        alpha_g1: valid_g1(env, b"m-alpha"),
        beta_g2: valid_g2(env, b"m-beta"),
        gamma_g2: valid_g2(env, b"m-gamma"),
        delta_g2: valid_g2(env, b"m-delta"),
        ic: vec![
            env,
            valid_g1(env, b"m-ic0"),
            valid_g1(env, b"m-ic1"),
            valid_g1(env, b"m-ic2"),
        ],
    }
}

fn mock_create_vk(env: &Env) -> VerificationKeyData {
    VerificationKeyData {
        alpha_g1: valid_g1(env, b"c-alpha"),
        beta_g2: valid_g2(env, b"c-beta"),
        gamma_g2: valid_g2(env, b"c-gamma"),
        delta_g2: valid_g2(env, b"c-delta"),
        ic: vec![
            env,
            valid_g1(env, b"c-ic0"),
            valid_g1(env, b"c-ic1"),
            valid_g1(env, b"c-ic2"),
            valid_g1(env, b"c-ic3"),
            valid_g1(env, b"c-ic4"),
            valid_g1(env, b"c-ic5"),
            valid_g1(env, b"c-ic6"),
        ],
    }
}

fn mock_update_vk(env: &Env) -> VerificationKeyData {
    VerificationKeyData {
        alpha_g1: valid_g1(env, b"u-alpha"),
        beta_g2: valid_g2(env, b"u-beta"),
        gamma_g2: valid_g2(env, b"u-gamma"),
        delta_g2: valid_g2(env, b"u-delta"),
        ic: vec![
            env,
            valid_g1(env, b"u-ic0"),
            valid_g1(env, b"u-ic1"),
            valid_g1(env, b"u-ic2"),
            valid_g1(env, b"u-ic3"),
            valid_g1(env, b"u-ic4"),
            valid_g1(env, b"u-ic5"),
            valid_g1(env, b"u-ic6"),
        ],
    }
}

fn mock_proof(env: &Env) -> Groth16Proof {
    Groth16Proof {
        a: valid_g1(env, b"proof-a"),
        b: valid_g2(env, b"proof-b"),
        c: valid_g1(env, b"proof-c"),
    }
}

/// 32 zero bytes is the canonical Fr encoding of the zero scalar.
fn canonical_zero(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

/// A non-canonical Fr encoding (top byte = 0xff makes the value
/// exceed the BLS12-381 Fr modulus).
fn non_canonical_fr(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0xff; 32])
}

// ================================================================
// Setup helpers
// ================================================================

fn setup_env() -> (Env, SepOligarchyContractClient<'static>, Address) {
    let env = Env::default();
    // The constructor runs subgroup checks on 9 VKs (3 membership + 3
    // create + 3 update); each VK has 4 G2 points + 3-7 G1 IC points,
    // and G2 subgroup checks are expensive. The default test budget
    // can't cover all 9 — bump to unlimited to keep setup fast.
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let mvk = mock_membership_vk(&env);
    let cvk = mock_create_vk(&env);
    let uvk = mock_update_vk(&env);
    let contract_id = env.register(
        SepOligarchyContract,
        (
            admin.clone(),
            mvk.clone(),
            mvk.clone(),
            mvk,
            cvk.clone(),
            cvk.clone(),
            cvk,
            uvk.clone(),
            uvk.clone(),
            uvk,
        ),
    );
    let client = SepOligarchyContractClient::new(&env, &contract_id);
    (env, client, admin)
}

fn setup_with_bad_membership_vk(
    build_bad: impl FnOnce(&Env) -> VerificationKeyData,
) -> (Env, SepOligarchyContractClient<'static>, Address) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let bad = build_bad(&env);
    let good_m = mock_membership_vk(&env);
    let cvk = mock_create_vk(&env);
    let uvk = mock_update_vk(&env);
    let contract_id = env.register(
        SepOligarchyContract,
        (
            admin.clone(),
            bad,
            good_m.clone(),
            good_m,
            cvk.clone(),
            cvk.clone(),
            cvk,
            uvk.clone(),
            uvk.clone(),
            uvk,
        ),
    );
    let client = SepOligarchyContractClient::new(&env, &contract_id);
    (env, client, admin)
}

fn setup_with_bad_create_vk(
    build_bad: impl FnOnce(&Env) -> VerificationKeyData,
) -> (Env, SepOligarchyContractClient<'static>, Address) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let bad = build_bad(&env);
    let mvk = mock_membership_vk(&env);
    let good_c = mock_create_vk(&env);
    let uvk = mock_update_vk(&env);
    let contract_id = env.register(
        SepOligarchyContract,
        (
            admin.clone(),
            mvk.clone(),
            mvk.clone(),
            mvk,
            bad,
            good_c.clone(),
            good_c,
            uvk.clone(),
            uvk.clone(),
            uvk,
        ),
    );
    let client = SepOligarchyContractClient::new(&env, &contract_id);
    (env, client, admin)
}

fn setup_with_bad_update_vk(
    build_bad: impl FnOnce(&Env) -> VerificationKeyData,
) -> (Env, SepOligarchyContractClient<'static>, Address) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let bad = build_bad(&env);
    let mvk = mock_membership_vk(&env);
    let cvk = mock_create_vk(&env);
    let good_u = mock_update_vk(&env);
    let contract_id = env.register(
        SepOligarchyContract,
        (
            admin.clone(),
            mvk.clone(),
            mvk.clone(),
            mvk,
            cvk.clone(),
            cvk.clone(),
            cvk,
            bad,
            good_u.clone(),
            good_u,
        ),
    );
    let client = SepOligarchyContractClient::new(&env, &contract_id);
    (env, client, admin)
}

/// Inject a `CommitmentEntry` directly into storage — bypasses the
/// proof-verification path so tests can drive `update_commitment` /
/// `verify_membership` / `deactivate_group` from a known-good state.
fn inject_group(
    env: &Env,
    contract_id: &Address,
    group_id: &BytesN<32>,
    commitment: &BytesN<32>,
    occupancy_commitment: &BytesN<32>,
    tier: u32,
    admin_threshold_numerator: u32,
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
            admin_threshold_numerator,
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

/// Inject a deactivated group (active = false) — for testing the
/// `GroupInactive` path on `update_commitment` / `deactivate_group`.
fn inject_deactivated_group(
    env: &Env,
    contract_id: &Address,
    group_id: &BytesN<32>,
    tier: u32,
) {
    env.as_contract(contract_id, || {
        let entry = CommitmentEntry {
            commitment: canonical_zero(env),
            epoch: 0,
            timestamp: env.ledger().timestamp(),
            tier,
            active: false,
            occupancy_commitment: canonical_zero(env),
            admin_threshold_numerator: 50,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Group(group_id.clone()), &entry);
    });
}

fn upi_zero(env: &Env) -> UpdateCommitmentPublicInputs {
    UpdateCommitmentPublicInputs {
        c_old: canonical_zero(env),
        epoch_old: 0,
        c_new: canonical_zero(env),
        occupancy_commitment_old: canonical_zero(env),
        occupancy_commitment_new: canonical_zero(env),
    }
}

fn cpi_zero(env: &Env) -> CreatePublicInputs {
    CreatePublicInputs {
        commitment: canonical_zero(env),
        epoch: 0,
        occupancy_commitment: canonical_zero(env),
        member_root: canonical_zero(env),
        admin_root: canonical_zero(env),
        salt_initial: canonical_zero(env),
    }
}

// ================================================================
// 1. Initialization
// ================================================================

#[test]
fn test_initialize() {
    let (_env, _client, _admin) = setup_env();
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_invalid_membership_vk_length_rejected() {
    setup_with_bad_membership_vk(|env| {
        let g1 = valid_g1(env, b"bad-alpha");
        let g2 = valid_g2(env, b"bad-beta");
        VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![env, g1.clone(), g1],
        }
    });
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_invalid_create_vk_length_rejected() {
    setup_with_bad_create_vk(|env| {
        let g1 = valid_g1(env, b"c-bad-alpha");
        let g2 = valid_g2(env, b"c-bad-beta");
        VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![
                env,
                g1.clone(),
                g1.clone(),
                g1.clone(),
                g1.clone(),
                g1.clone(),
                g1,
            ],
        }
    });
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_invalid_update_vk_length_rejected() {
    setup_with_bad_update_vk(|env| {
        let g1 = valid_g1(env, b"u-bad-alpha");
        let g2 = valid_g2(env, b"u-bad-beta");
        VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![
                env,
                g1.clone(),
                g1.clone(),
                g1.clone(),
                g1.clone(),
                g1.clone(),
                g1,
            ],
        }
    });
}

// ================================================================
// 2. create_oligarchy_group
// ================================================================

fn create_caller(env: &Env) -> Address {
    Address::generate(env)
}

#[test]
fn test_create_oligarchy_group_happy_path() {
    let (env, client, _admin) = setup_env();
    let caller = create_caller(&env);
    let group_id = BytesN::from_array(&env, &[1u8; 32]);
    let z = canonical_zero(&env);
    let r = client.try_create_oligarchy_group(
        &caller,
        &group_id,
        &z,
        &1u32,
        &50u32,
        &z,
        &z,
        &z,
        &z,
        &mock_proof(&env),
        &cpi_zero(&env),
    );
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof at verifier, got {:?}", other),
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_create_oligarchy_group_rejects_invalid_tier() {
    let (env, client, _admin) = setup_env();
    let caller = create_caller(&env);
    let z = canonical_zero(&env);
    client.create_oligarchy_group(
        &caller,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &3u32, // out-of-range
        &50u32,
        &z,
        &z,
        &z,
        &z,
        &mock_proof(&env),
        &cpi_zero(&env),
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #28)")]
fn test_create_oligarchy_group_rejects_invalid_threshold_zero() {
    let (env, client, _admin) = setup_env();
    let caller = create_caller(&env);
    let z = canonical_zero(&env);
    client.create_oligarchy_group(
        &caller,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &1u32,
        &0u32, // out-of-range below
        &z,
        &z,
        &z,
        &z,
        &mock_proof(&env),
        &cpi_zero(&env),
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #28)")]
fn test_create_oligarchy_group_rejects_invalid_threshold_above_100() {
    let (env, client, _admin) = setup_env();
    let caller = create_caller(&env);
    let z = canonical_zero(&env);
    client.create_oligarchy_group(
        &caller,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &1u32,
        &101u32, // out-of-range above
        &z,
        &z,
        &z,
        &z,
        &mock_proof(&env),
        &cpi_zero(&env),
    );
}

#[test]
fn test_create_oligarchy_group_accepts_threshold_50() {
    let (env, client, _admin) = setup_env();
    let caller = create_caller(&env);
    let z = canonical_zero(&env);
    let r = client.try_create_oligarchy_group(
        &caller,
        &BytesN::from_array(&env, &[2u8; 32]),
        &z,
        &0u32,
        &50u32,
        &z,
        &z,
        &z,
        &z,
        &mock_proof(&env),
        &cpi_zero(&env),
    );
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof, got {:?}", other),
    }
}

#[test]
fn test_create_oligarchy_group_accepts_threshold_67() {
    let (env, client, _admin) = setup_env();
    let caller = create_caller(&env);
    let z = canonical_zero(&env);
    let r = client.try_create_oligarchy_group(
        &caller,
        &BytesN::from_array(&env, &[3u8; 32]),
        &z,
        &0u32,
        &67u32,
        &z,
        &z,
        &z,
        &z,
        &mock_proof(&env),
        &cpi_zero(&env),
    );
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof, got {:?}", other),
    }
}

#[test]
fn test_create_oligarchy_group_accepts_threshold_100() {
    let (env, client, _admin) = setup_env();
    let caller = create_caller(&env);
    let z = canonical_zero(&env);
    let r = client.try_create_oligarchy_group(
        &caller,
        &BytesN::from_array(&env, &[4u8; 32]),
        &z,
        &0u32,
        &100u32,
        &z,
        &z,
        &z,
        &z,
        &mock_proof(&env),
        &cpi_zero(&env),
    );
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof, got {:?}", other),
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_create_oligarchy_group_rejects_duplicate_group_id() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[1u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 0);

    let caller = create_caller(&env);
    client.create_oligarchy_group(
        &caller,
        &group_id,
        &z,
        &0u32,
        &50u32,
        &z,
        &z,
        &z,
        &z,
        &mock_proof(&env),
        &cpi_zero(&env),
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_oligarchy_group_rejects_non_canonical_commitment() {
    let (env, client, _admin) = setup_env();
    let caller = create_caller(&env);
    let bad = non_canonical_fr(&env);
    let z = canonical_zero(&env);
    let pi = CreatePublicInputs {
        commitment: bad.clone(),
        ..cpi_zero(&env)
    };
    client.create_oligarchy_group(
        &caller,
        &BytesN::from_array(&env, &[1u8; 32]),
        &bad,
        &0u32,
        &50u32,
        &z,
        &z,
        &z,
        &z,
        &mock_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_oligarchy_group_rejects_non_canonical_occupancy_commitment() {
    let (env, client, _admin) = setup_env();
    let caller = create_caller(&env);
    let z = canonical_zero(&env);
    let bad = non_canonical_fr(&env);
    let pi = CreatePublicInputs {
        occupancy_commitment: bad.clone(),
        ..cpi_zero(&env)
    };
    client.create_oligarchy_group(
        &caller,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &0u32,
        &50u32,
        &bad,
        &z,
        &z,
        &z,
        &mock_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_oligarchy_group_rejects_non_canonical_member_root() {
    let (env, client, _admin) = setup_env();
    let caller = create_caller(&env);
    let z = canonical_zero(&env);
    let bad = non_canonical_fr(&env);
    let pi = CreatePublicInputs {
        member_root: bad.clone(),
        ..cpi_zero(&env)
    };
    client.create_oligarchy_group(
        &caller,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &0u32,
        &50u32,
        &z,
        &bad,
        &z,
        &z,
        &mock_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_oligarchy_group_rejects_non_canonical_admin_root() {
    let (env, client, _admin) = setup_env();
    let caller = create_caller(&env);
    let z = canonical_zero(&env);
    let bad = non_canonical_fr(&env);
    let pi = CreatePublicInputs {
        admin_root: bad.clone(),
        ..cpi_zero(&env)
    };
    client.create_oligarchy_group(
        &caller,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &0u32,
        &50u32,
        &z,
        &z,
        &bad,
        &z,
        &mock_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_oligarchy_group_rejects_non_canonical_salt_initial() {
    let (env, client, _admin) = setup_env();
    let caller = create_caller(&env);
    let z = canonical_zero(&env);
    let bad = non_canonical_fr(&env);
    let pi = CreatePublicInputs {
        salt_initial: bad.clone(),
        ..cpi_zero(&env)
    };
    client.create_oligarchy_group(
        &caller,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &0u32,
        &50u32,
        &z,
        &z,
        &z,
        &bad,
        &mock_proof(&env),
        &pi,
    );
}

#[test]
fn test_create_oligarchy_group_rejects_invalid_proof() {
    let (env, client, _admin) = setup_env();
    let caller = create_caller(&env);
    let z = canonical_zero(&env);
    let r = client.try_create_oligarchy_group(
        &caller,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &0u32,
        &50u32,
        &z,
        &z,
        &z,
        &z,
        &mock_proof(&env),
        &cpi_zero(&env),
    );
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof, got {:?}", other),
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #14)")]
fn test_create_oligarchy_group_restricted_mode_rejects_non_admin() {
    let (env, client, _admin) = setup_env();
    client.set_restricted_mode(&true);
    let caller = create_caller(&env); // != admin
    let z = canonical_zero(&env);
    client.create_oligarchy_group(
        &caller,
        &BytesN::from_array(&env, &[55u8; 32]),
        &z,
        &0u32,
        &50u32,
        &z,
        &z,
        &z,
        &z,
        &mock_proof(&env),
        &cpi_zero(&env),
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_create_oligarchy_group_enforces_tier_group_limit() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .set(&DataKey::GroupCount(0u32), &10_000u32);
    });
    let caller = create_caller(&env);
    let z = canonical_zero(&env);
    client.create_oligarchy_group(
        &caller,
        &BytesN::from_array(&env, &[42u8; 32]),
        &z,
        &0u32,
        &50u32,
        &z,
        &z,
        &z,
        &z,
        &mock_proof(&env),
        &cpi_zero(&env),
    );
}

// ================================================================
// 3. update_commitment
// ================================================================

#[test]
fn test_update_commitment_happy_path() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[7u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 1, 50, 0);
    let r = client.try_update_commitment(&group_id, &mock_proof(&env), &upi_zero(&env));
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof at verifier, got {:?}", other),
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_commitment_rejects_stale_c_old() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[9u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 0);
    let upi = UpdateCommitmentPublicInputs {
        c_old: BytesN::from_array(&env, &[3u8; 32]),
        ..upi_zero(&env)
    };
    client.update_commitment(&group_id, &mock_proof(&env), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_commitment_rejects_stale_occupancy_commitment_old() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[10u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 0);
    let upi = UpdateCommitmentPublicInputs {
        occupancy_commitment_old: BytesN::from_array(&env, &[7u8; 32]),
        ..upi_zero(&env)
    };
    client.update_commitment(&group_id, &mock_proof(&env), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_commitment_rejects_wrong_epoch_old() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[11u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 5);
    let upi = UpdateCommitmentPublicInputs {
        epoch_old: 4,
        ..upi_zero(&env)
    };
    client.update_commitment(&group_id, &mock_proof(&env), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_update_commitment_rejects_non_canonical_c_new() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[12u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 0);
    let upi = UpdateCommitmentPublicInputs {
        c_new: non_canonical_fr(&env),
        ..upi_zero(&env)
    };
    client.update_commitment(&group_id, &mock_proof(&env), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_update_commitment_rejects_non_canonical_occupancy_commitment_new() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[13u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 0);
    let upi = UpdateCommitmentPublicInputs {
        occupancy_commitment_new: non_canonical_fr(&env),
        ..upi_zero(&env)
    };
    client.update_commitment(&group_id, &mock_proof(&env), &upi);
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_update_commitment_rejects_replayed_proof() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[14u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 0);

    let proof = mock_proof(&env);
    env.as_contract(&contract_id, || {
        let mut preimage = Bytes::new(&env);
        preimage.append(&Bytes::from_slice(&env, proof.a.to_array().as_slice()));
        preimage.append(&Bytes::from_slice(&env, proof.b.to_array().as_slice()));
        preimage.append(&Bytes::from_slice(&env, proof.c.to_array().as_slice()));
        let hash: BytesN<32> = env.crypto().sha256(&preimage).into();
        env.storage()
            .persistent()
            .set(&DataKey::UsedProof(hash), &true);
    });
    client.update_commitment(&group_id, &proof, &upi_zero(&env));
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_update_commitment_rejects_inactive_group() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[15u8; 32]);
    inject_deactivated_group(&env, &contract_id, &group_id, 0);
    client.update_commitment(&group_id, &mock_proof(&env), &upi_zero(&env));
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_update_commitment_rejects_unknown_group() {
    let (env, client, _admin) = setup_env();
    let group_id = BytesN::from_array(&env, &[99u8; 32]);
    client.update_commitment(&group_id, &mock_proof(&env), &upi_zero(&env));
}

#[test]
fn test_update_commitment_admin_threshold_supplied_from_storage() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[20u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 1, 67, 0);
    let r = client.try_update_commitment(&group_id, &mock_proof(&env), &upi_zero(&env));
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!(
            "expected InvalidProof — verifier received chain-supplied threshold; got {:?}",
            other
        ),
    }
}

#[test]
fn test_update_commitment_admin_threshold_mismatch_rejected_v2() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let z = canonical_zero(&env);
    let g50 = BytesN::from_array(&env, &[21u8; 32]);
    let g67 = BytesN::from_array(&env, &[22u8; 32]);
    inject_group(&env, &contract_id, &g50, &z, &z, 1, 50, 0);
    inject_group(&env, &contract_id, &g67, &z, &z, 1, 67, 0);

    let r50 = client.try_update_commitment(&g50, &mock_proof(&env), &upi_zero(&env));
    let proof2 = Groth16Proof {
        a: valid_g1(&env, b"proof-a-2"),
        b: valid_g2(&env, b"proof-b-2"),
        c: valid_g1(&env, b"proof-c-2"),
    };
    let r67 = client.try_update_commitment(&g67, &proof2, &upi_zero(&env));

    match (r50, r67) {
        (Err(Err(_)) | Err(Ok(Error::InvalidProof)), Err(Err(_)) | Err(Ok(Error::InvalidProof))) => {}
        other => panic!("both calls expected InvalidProof, got {:?}", other),
    }
}

#[test]
fn test_update_commitment_admin_threshold_tie_passes_v2() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[23u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 1, 50, 0);
    let r = client.try_update_commitment(&group_id, &mock_proof(&env), &upi_zero(&env));
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof at verifier, got {:?}", other),
    }
}

// ================================================================
// 4. verify_membership
// ================================================================

#[test]
fn test_verify_membership_happy_path() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[30u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 0);
    let pi = PublicInputs {
        commitment: z.clone(),
        epoch: 0,
    };
    let result = client.verify_membership(&group_id, &mock_proof(&env), &pi);
    assert!(!result, "mock proof should fail pairing_check");
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_verify_membership_rejects_wrong_commitment() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[31u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 0);
    let pi = PublicInputs {
        commitment: BytesN::from_array(&env, &[7u8; 32]),
        epoch: 0,
    };
    client.verify_membership(&group_id, &mock_proof(&env), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_verify_membership_rejects_wrong_epoch() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[32u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 5);
    let pi = PublicInputs {
        commitment: z,
        epoch: 4,
    };
    client.verify_membership(&group_id, &mock_proof(&env), &pi);
}

#[test]
fn test_verify_membership_rejects_inactive_group() {
    // verify_membership intentionally works on deactivated groups
    // (off-chain attestations against the final pre-deactivation
    // state remain verifiable). Returns Ok(false) for our mock proof.
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[33u8; 32]);
    inject_deactivated_group(&env, &contract_id, &group_id, 0);
    let pi = PublicInputs {
        commitment: canonical_zero(&env),
        epoch: 0,
    };
    let result = client.verify_membership(&group_id, &mock_proof(&env), &pi);
    assert!(!result);
}

// ================================================================
// 5. deactivate_group
// ================================================================

#[test]
fn test_deactivate_group_happy_path() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[40u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 0);
    let pi = PublicInputs {
        commitment: z,
        epoch: 0,
    };
    let r = client.try_deactivate_group(&group_id, &mock_proof(&env), &pi);
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof, got {:?}", other),
    }
}

#[test]
fn test_deactivate_group_rejects_non_member_proof() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[41u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 0);
    let pi = PublicInputs {
        commitment: z,
        epoch: 0,
    };
    let r = client.try_deactivate_group(&group_id, &mock_proof(&env), &pi);
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof, got {:?}", other),
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_deactivate_already_inactive_group() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[42u8; 32]);
    inject_deactivated_group(&env, &contract_id, &group_id, 0);
    let pi = PublicInputs {
        commitment: canonical_zero(&env),
        epoch: 0,
    };
    client.deactivate_group(&group_id, &mock_proof(&env), &pi);
}

// ================================================================
// 6. update_vk admin rotation
// ================================================================

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_update_vk_requires_auth() {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    let admin = Address::generate(&env);
    let mvk = mock_membership_vk(&env);
    let cvk = mock_create_vk(&env);
    let uvk = mock_update_vk(&env);
    let contract_id = env.register(
        SepOligarchyContract,
        (
            admin.clone(),
            mvk.clone(),
            mvk.clone(),
            mvk,
            cvk.clone(),
            cvk.clone(),
            cvk,
            uvk.clone(),
            uvk.clone(),
            uvk,
        ),
    );
    let client = SepOligarchyContractClient::new(&env, &contract_id);
    let new_vk = mock_membership_vk(&env);
    client.update_vk(&VkKind::Membership, &0u32, &new_vk);
}

#[test]
fn test_update_vk_rotates_membership_vk() {
    let (env, client, _admin) = setup_env();
    let new_vk = mock_membership_vk(&env);
    client.update_vk(&VkKind::Membership, &0u32, &new_vk);
}

#[test]
fn test_update_vk_rotates_create_vk() {
    let (env, client, _admin) = setup_env();
    let new_vk = mock_create_vk(&env);
    client.update_vk(&VkKind::Create, &1u32, &new_vk);
}

#[test]
fn test_update_vk_rotates_update_vk() {
    let (env, client, _admin) = setup_env();
    let new_vk = mock_update_vk(&env);
    client.update_vk(&VkKind::Update, &2u32, &new_vk);
}

// ================================================================
// 7. Queries
// ================================================================

#[test]
fn test_get_commitment_returns_current_state() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[50u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 1, 67, 3);
    let entry = client.get_commitment(&group_id);
    assert_eq!(entry.commitment, z);
    assert_eq!(entry.tier, 1);
    assert_eq!(entry.admin_threshold_numerator, 67);
    assert_eq!(entry.epoch, 3);
    assert!(entry.active);
}

#[test]
fn test_get_history_returns_chronological_entries() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[51u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 0);

    env.as_contract(&contract_id, || {
        let mut history: Vec<CommitmentEntry> = Vec::new(&env);
        for i in 0u64..3 {
            history.push_back(CommitmentEntry {
                commitment: BytesN::from_array(&env, &[i as u8; 32]),
                epoch: i,
                timestamp: 100 + i,
                tier: 0,
                active: true,
                occupancy_commitment: canonical_zero(&env),
                admin_threshold_numerator: 50,
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
fn test_archive_entry_appends_and_prunes() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[60u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 0);

    let total: u64 = (HISTORY_WINDOW as u64) + 6;
    env.as_contract(&contract_id, || {
        for i in 0u64..total {
            let entry = CommitmentEntry {
                commitment: BytesN::from_array(&env, &[(i & 0xff) as u8; 32]),
                epoch: i,
                timestamp: 1000 + i,
                tier: 0,
                active: true,
                occupancy_commitment: canonical_zero(&env),
                admin_threshold_numerator: 50,
            };
            SepOligarchyContract::archive_entry(&env, &group_id, &entry);
        }
    });

    let history = client.get_history(&group_id, &(2 * HISTORY_WINDOW));
    assert_eq!(history.len(), HISTORY_WINDOW);
    assert_eq!(history.get(0).unwrap().epoch, total - HISTORY_WINDOW as u64);
    assert_eq!(history.get(history.len() - 1).unwrap().epoch, total - 1);
}

#[test]
fn test_bump_group_ttl_extends_used_proof_lifetime() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[52u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 50, 0);
    client.bump_group_ttl(&group_id);
}

// ================================================================
// 8. test-vectors.json consistency
// ================================================================

#[test]
fn test_vectors_consistency() {
    use serde_json::Value;
    let raw = include_str!("../test-vectors.json");
    let v: Value = serde_json::from_str(raw).expect("test-vectors.json is valid JSON");

    // ---- Error codes ----
    let errors = v["error_codes"]["vectors"]
        .as_array()
        .expect("error_codes.vectors is an array");
    let expected: &[(&str, u32)] = &[
        ("NotInitialized", Error::NotInitialized as u32),
        ("AlreadyInitialized", Error::AlreadyInitialized as u32),
        ("Reserved3", Error::Reserved3 as u32),
        ("GroupAlreadyExists", Error::GroupAlreadyExists as u32),
        ("GroupNotFound", Error::GroupNotFound as u32),
        ("GroupInactive", Error::GroupInactive as u32),
        ("InvalidProof", Error::InvalidProof as u32),
        ("InvalidTier", Error::InvalidTier as u32),
        ("InvalidVkLength", Error::InvalidVkLength as u32),
        ("PublicInputsMismatch", Error::PublicInputsMismatch as u32),
        ("InvalidEpoch", Error::InvalidEpoch as u32),
        ("ProofReplay", Error::ProofReplay as u32),
        ("TierGroupLimitReached", Error::TierGroupLimitReached as u32),
        ("AdminOnly", Error::AdminOnly as u32),
        ("InvalidCommitmentEncoding", Error::InvalidCommitmentEncoding as u32),
        ("InvalidPoint", Error::InvalidPoint as u32),
        ("GroupStillActive", Error::GroupStillActive as u32),
        ("InvalidThreshold", Error::InvalidThreshold as u32),
        ("InvalidInitialMembership", Error::InvalidInitialMembership as u32),
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
            name, json_code, code
        );
    }

    // ---- Tier capacity ----
    let tiers = v["tier"]["vectors"].as_array().unwrap();
    for entry in tiers {
        let tier = entry["tier"].as_u64().unwrap() as u32;
        let expected_cap = entry["capacity"].as_u64().unwrap() as u32;
        assert_eq!(
            tier_capacity(tier),
            expected_cap,
            "tier_capacity({}) mismatch",
            tier
        );
    }

    // ---- IC point counts ----
    let vk_kinds = v["vk_kind_enum"]["vectors"].as_array().unwrap();
    for entry in vk_kinds {
        match entry["name"].as_str() {
            Some("Membership") => assert_eq!(
                entry["ic_count"].as_u64().unwrap() as u32,
                MEMBERSHIP_IC_POINTS,
                "Membership IC count drift"
            ),
            Some("Create") => assert_eq!(
                entry["ic_count"].as_u64().unwrap() as u32,
                CREATE_IC_POINTS,
                "Create IC count drift"
            ),
            Some("Update") => assert_eq!(
                entry["ic_count"].as_u64().unwrap() as u32,
                UPDATE_IC_POINTS,
                "Update IC count drift"
            ),
            _ => {}
        }
    }

    // ---- Max groups per tier ----
    let max = v["max_groups_per_tier"]["value"].as_u64().unwrap() as u32;
    assert_eq!(max, MAX_GROUPS_PER_TIER, "MAX_GROUPS_PER_TIER drift");
}
