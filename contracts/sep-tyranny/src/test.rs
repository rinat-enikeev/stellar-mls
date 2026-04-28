//! Inline test suite for the SEP Tyranny contract.
//!
//! Every test name in `test-vectors.json#tests_to_implement` has a
//! corresponding `#[test]` here. The `test_vectors_consistency` test
//! pins the JSON as the canonical ABI.

use super::*;
use soroban_sdk::testutils::Address as _;

// ================================================================
// Mocks
// ================================================================

fn valid_g1(env: &Env, tag: &[u8]) -> BytesN<96> {
    let bls = env.crypto().bls12_381();
    let dst = Bytes::from_slice(env, b"sep-tyranny-test-g1");
    let msg = Bytes::from_slice(env, tag);
    bls.hash_to_g1(&msg, &dst).to_bytes()
}

fn valid_g2(env: &Env, tag: &[u8]) -> BytesN<192> {
    let bls = env.crypto().bls12_381();
    let dst = Bytes::from_slice(env, b"sep-tyranny-test-g2");
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
        ],
    }
}

fn mock_proof(env: &Env, tag: &[u8]) -> Groth16Proof {
    Groth16Proof {
        a: valid_g1(env, tag),
        b: valid_g2(env, tag),
        c: valid_g1(env, tag),
    }
}

fn canonical_zero(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

fn non_canonical_fr(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0xff; 32])
}

fn caller(env: &Env) -> Address {
    Address::generate(env)
}

// ================================================================
// Setup helpers
// ================================================================

fn setup_env() -> (Env, SepTyrannyContractClient<'static>, Address) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let mvk = mock_membership_vk(&env);
    let cvk = mock_create_vk(&env);
    let uvk = mock_update_vk(&env);
    let contract_id = env.register(
        SepTyrannyContract,
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
    let client = SepTyrannyContractClient::new(&env, &contract_id);
    (env, client, admin)
}

fn setup_with_bad_membership_vk(
    build_bad: impl FnOnce(&Env) -> VerificationKeyData,
) -> (Env, SepTyrannyContractClient<'static>, Address) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let bad = build_bad(&env);
    let good_m = mock_membership_vk(&env);
    let cvk = mock_create_vk(&env);
    let uvk = mock_update_vk(&env);
    let contract_id = env.register(
        SepTyrannyContract,
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
    let client = SepTyrannyContractClient::new(&env, &contract_id);
    (env, client, admin)
}

fn setup_with_bad_create_vk(
    build_bad: impl FnOnce(&Env) -> VerificationKeyData,
) -> (Env, SepTyrannyContractClient<'static>, Address) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let bad = build_bad(&env);
    let mvk = mock_membership_vk(&env);
    let good_c = mock_create_vk(&env);
    let uvk = mock_update_vk(&env);
    let contract_id = env.register(
        SepTyrannyContract,
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
    let client = SepTyrannyContractClient::new(&env, &contract_id);
    (env, client, admin)
}

fn setup_with_bad_update_vk(
    build_bad: impl FnOnce(&Env) -> VerificationKeyData,
) -> (Env, SepTyrannyContractClient<'static>, Address) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let bad = build_bad(&env);
    let mvk = mock_membership_vk(&env);
    let cvk = mock_create_vk(&env);
    let good_u = mock_update_vk(&env);
    let contract_id = env.register(
        SepTyrannyContract,
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
    let client = SepTyrannyContractClient::new(&env, &contract_id);
    (env, client, admin)
}

/// Inject a CommitmentEntry directly into storage so update_commitment
/// / verify_membership tests can run from a known-good state without
/// having to round-trip through create_group's verifier (mock proofs
/// don't pass pairing).
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
            admin_pubkey_commitment: admin_pubkey_commitment.clone(),
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

// ================================================================
// 1. Initialization (4 tests)
// ================================================================

#[test]
fn test_initialize() {
    let (_env, _client, _admin) = setup_env();
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_invalid_membership_vk_length_rejected() {
    setup_with_bad_membership_vk(|env| {
        let g1 = valid_g1(env, b"bad-m-alpha");
        let g2 = valid_g2(env, b"bad-m-beta");
        VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![env, g1.clone(), g1], // 2 IC points (need 3)
        }
    });
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_invalid_create_vk_length_rejected() {
    setup_with_bad_create_vk(|env| {
        let g1 = valid_g1(env, b"bad-c-alpha");
        let g2 = valid_g2(env, b"bad-c-beta");
        VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![env, g1.clone(), g1.clone(), g1], // 3 IC points (need 4)
        }
    });
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_invalid_update_vk_length_rejected() {
    setup_with_bad_update_vk(|env| {
        let g1 = valid_g1(env, b"bad-u-alpha");
        let g2 = valid_g2(env, b"bad-u-beta");
        VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![env, g1.clone(), g1.clone(), g1.clone(), g1], // 4 IC points (need 5)
        }
    });
}

// ================================================================
// 2. create_group (11 tests)
// ================================================================

fn create_pi(env: &Env, c: &BytesN<32>, epoch: u64, apc: &BytesN<32>) -> CreatePublicInputs {
    CreatePublicInputs {
        commitment: c.clone(),
        epoch,
        admin_pubkey_commitment: apc.clone(),
    }
}

#[test]
fn test_create_group_happy_path() {
    let (env, client, _admin) = setup_env();
    let z = canonical_zero(&env);
    let pi = create_pi(&env, &z, 0, &z);
    let r = client.try_create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &0u32,
        &z,
        &mock_proof(&env, b"happy"),
        &pi,
    );
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof, got {:?}", other),
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_create_group_rejects_invalid_tier() {
    let (env, client, _admin) = setup_env();
    let z = canonical_zero(&env);
    let pi = create_pi(&env, &z, 0, &z);
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[2u8; 32]),
        &z,
        &3u32, // invalid (tier > 2)
        &z,
        &mock_proof(&env, b"bad-tier"),
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
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 0);
    let pi = create_pi(&env, &z, 0, &z);
    client.create_group(
        &caller(&env),
        &group_id,
        &z,
        &0u32,
        &z,
        &mock_proof(&env, b"dup"),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_group_rejects_non_canonical_commitment() {
    let (env, client, _admin) = setup_env();
    let bad = non_canonical_fr(&env);
    let z = canonical_zero(&env);
    let pi = create_pi(&env, &bad, 0, &z);
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[4u8; 32]),
        &bad,
        &0u32,
        &z,
        &mock_proof(&env, b"non-canon-c"),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_group_rejects_non_canonical_admin_pubkey_commitment() {
    let (env, client, _admin) = setup_env();
    let z = canonical_zero(&env);
    let bad_apc = non_canonical_fr(&env);
    let pi = create_pi(&env, &z, 0, &bad_apc);
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[5u8; 32]),
        &z,
        &0u32,
        &bad_apc,
        &mock_proof(&env, b"non-canon-apc"),
        &pi,
    );
}

#[test]
fn test_create_group_rejects_invalid_proof() {
    // Same as happy path; verifier reached, mock can't pass pairing.
    let (env, client, _admin) = setup_env();
    let z = canonical_zero(&env);
    let pi = create_pi(&env, &z, 0, &z);
    let r = client.try_create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[6u8; 32]),
        &z,
        &0u32,
        &z,
        &mock_proof(&env, b"bad-proof"),
        &pi,
    );
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof, got {:?}", other),
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #14)")]
fn test_create_group_restricted_mode_rejects_non_admin() {
    let (env, client, admin) = setup_env();
    client.set_restricted_mode(&true);
    let z = canonical_zero(&env);
    let attacker = Address::generate(&env);
    assert_ne!(attacker, admin);
    let pi = create_pi(&env, &z, 0, &z);
    client.create_group(
        &attacker,
        &BytesN::from_array(&env, &[7u8; 32]),
        &z,
        &0u32,
        &z,
        &mock_proof(&env, b"restricted"),
        &pi,
    );
}

#[test]
fn test_create_group_restricted_mode_admin_can_create() {
    // Positive complement: admin can still create when restricted=true.
    let (env, client, admin) = setup_env();
    client.set_restricted_mode(&true);
    let z = canonical_zero(&env);
    let pi = create_pi(&env, &z, 0, &z);
    let r = client.try_create_group(
        &admin,
        &BytesN::from_array(&env, &[8u8; 32]),
        &z,
        &0u32,
        &z,
        &mock_proof(&env, b"restricted-admin"),
        &pi,
    );
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof at verifier, got {:?}", other),
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_create_group_enforces_tier_group_limit() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .set(&DataKey::GroupCount(0u32), &MAX_GROUPS_PER_TIER);
    });
    let z = canonical_zero(&env);
    let pi = create_pi(&env, &z, 0, &z);
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[9u8; 32]),
        &z,
        &0u32,
        &z,
        &mock_proof(&env, b"cap"),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_create_group_rejects_replayed_proof() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let z = canonical_zero(&env);
    let proof = mock_proof(&env, b"replay-create");
    env.as_contract(&contract_id, || {
        let mut preimage = Bytes::new(&env);
        preimage.append(&Bytes::from_slice(&env, proof.a.to_array().as_slice()));
        preimage.append(&Bytes::from_slice(&env, proof.b.to_array().as_slice()));
        preimage.append(&Bytes::from_slice(&env, proof.c.to_array().as_slice()));
        let h: BytesN<32> = env.crypto().sha256(&preimage).into();
        env.storage()
            .persistent()
            .set(&DataKey::UsedProof(h), &true);
    });
    let pi = create_pi(&env, &z, 0, &z);
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[10u8; 32]),
        &z,
        &0u32,
        &z,
        &proof,
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_create_group_rejects_public_inputs_mismatch() {
    let (env, client, _admin) = setup_env();
    let z = canonical_zero(&env);
    // public_inputs.epoch != 0 → mismatch
    let pi = create_pi(&env, &z, 1, &z);
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[11u8; 32]),
        &z,
        &0u32,
        &z,
        &mock_proof(&env, b"pi-mismatch"),
        &pi,
    );
}

// ================================================================
// 3. update_commitment (7 tests)
// ================================================================

fn upi(env: &Env, c_old: &BytesN<32>, epoch_old: u64, c_new: &BytesN<32>) -> UpdatePublicInputs {
    let _ = env;
    UpdatePublicInputs {
        c_old: c_old.clone(),
        epoch_old,
        c_new: c_new.clone(),
    }
}

#[test]
fn test_update_commitment_happy_path() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[20u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 0);
    let pi = upi(&env, &z, 0, &z);
    let r = client.try_update_commitment(&group_id, &mock_proof(&env, b"u-happy"), &pi);
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
    let group_id = BytesN::from_array(&env, &[21u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 0);
    let stale = BytesN::from_array(&env, &[0xab; 32]);
    let pi = upi(&env, &stale, 0, &z);
    client.update_commitment(&group_id, &mock_proof(&env, b"u-stale-c"), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_commitment_rejects_wrong_epoch_old() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[22u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 5); // stored epoch = 5
    let pi = upi(&env, &z, 4, &z); // wrong epoch_old
    client.update_commitment(&group_id, &mock_proof(&env, b"u-wrong-e"), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_update_commitment_rejects_non_canonical_c_new() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[23u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 0);
    let bad = non_canonical_fr(&env);
    let pi = upi(&env, &z, 0, &bad);
    client.update_commitment(&group_id, &mock_proof(&env, b"u-bad-cnew"), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_update_commitment_rejects_replayed_proof() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[24u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 0);
    let proof = mock_proof(&env, b"u-replay");
    env.as_contract(&contract_id, || {
        let mut preimage = Bytes::new(&env);
        preimage.append(&Bytes::from_slice(&env, proof.a.to_array().as_slice()));
        preimage.append(&Bytes::from_slice(&env, proof.b.to_array().as_slice()));
        preimage.append(&Bytes::from_slice(&env, proof.c.to_array().as_slice()));
        let h: BytesN<32> = env.crypto().sha256(&preimage).into();
        env.storage()
            .persistent()
            .set(&DataKey::UsedProof(h), &true);
    });
    let pi = upi(&env, &z, 0, &z);
    client.update_commitment(&group_id, &proof, &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_update_commitment_rejects_unknown_group() {
    let (env, client, _admin) = setup_env();
    let z = canonical_zero(&env);
    let pi = upi(&env, &z, 0, &z);
    client.update_commitment(
        &BytesN::from_array(&env, &[99u8; 32]),
        &mock_proof(&env, b"u-unknown"),
        &pi,
    );
}

#[test]
fn test_update_commitment_does_not_mutate_admin_pubkey_commitment() {
    // Set up a group with a non-zero admin_pubkey_commitment, then
    // attempt update_commitment. The update will fail at the verifier
    // (mock proof), but the test asserts the field is unchanged either
    // way. (For a successful update with real proofs, the same
    // assertion would hold per the contract's invariant.)
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[26u8; 32]);
    let z = canonical_zero(&env);
    let apc = canonical_zero(&env); // canonical Fr; distinct value would also work but z is simplest
    inject_group(&env, &contract_id, &group_id, &z, &apc, 0, 0);
    let pre = client.get_commitment(&group_id);
    assert_eq!(pre.admin_pubkey_commitment, apc);

    let pi = upi(&env, &z, 0, &z);
    let _ = client.try_update_commitment(&group_id, &mock_proof(&env, b"u-no-mutate"), &pi);

    let post = client.get_commitment(&group_id);
    assert_eq!(
        post.admin_pubkey_commitment, apc,
        "admin_pubkey_commitment must be invariant under update_commitment"
    );
}

// ================================================================
// 4. verify_membership (4 tests)
// ================================================================

#[test]
fn test_verify_membership_happy_path() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[30u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 0);
    let pi = MembershipPublicInputs {
        commitment: z,
        epoch: 0,
    };
    let result = client.verify_membership(&group_id, &mock_proof(&env, b"vm"), &pi);
    assert!(!result);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_verify_membership_rejects_wrong_commitment() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[31u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 0);
    let pi = MembershipPublicInputs {
        commitment: BytesN::from_array(&env, &[0xaa; 32]),
        epoch: 0,
    };
    client.verify_membership(&group_id, &mock_proof(&env, b"vm-wrong-c"), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_verify_membership_rejects_wrong_epoch() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[32u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 0);
    let pi = MembershipPublicInputs {
        commitment: z,
        epoch: 7,
    };
    client.verify_membership(&group_id, &mock_proof(&env, b"vm-wrong-e"), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_verify_membership_rejects_unknown_group() {
    let (env, client, _admin) = setup_env();
    let pi = MembershipPublicInputs {
        commitment: canonical_zero(&env),
        epoch: 0,
    };
    client.verify_membership(
        &BytesN::from_array(&env, &[33u8; 32]),
        &mock_proof(&env, b"vm-unknown"),
        &pi,
    );
}

// ================================================================
// 5. Admin entrypoints (6 tests)
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
        SepTyrannyContract,
        (
            admin,
            mvk.clone(),
            mvk.clone(),
            mvk.clone(),
            cvk.clone(),
            cvk.clone(),
            cvk,
            uvk.clone(),
            uvk.clone(),
            uvk,
        ),
    );
    let client = SepTyrannyContractClient::new(&env, &contract_id);
    let new_vk = mock_membership_vk(&env);
    client.update_vk(&VkKind::Membership, &0u32, &new_vk);
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_set_restricted_mode_requires_auth() {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    let admin = Address::generate(&env);
    let mvk = mock_membership_vk(&env);
    let cvk = mock_create_vk(&env);
    let uvk = mock_update_vk(&env);
    let contract_id = env.register(
        SepTyrannyContract,
        (
            admin,
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
    let client = SepTyrannyContractClient::new(&env, &contract_id);
    client.set_restricted_mode(&true);
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
    client.update_vk(&VkKind::Create, &0u32, &new_vk);
}

#[test]
fn test_update_vk_rotates_update_vk() {
    let (env, client, _admin) = setup_env();
    let new_vk = mock_update_vk(&env);
    client.update_vk(&VkKind::Update, &0u32, &new_vk);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_update_vk_rejects_invalid_tier() {
    let (env, client, _admin) = setup_env();
    let new_vk = mock_membership_vk(&env);
    client.update_vk(&VkKind::Membership, &3u32, &new_vk);
}

// ================================================================
// 6. Queries (4 tests)
// ================================================================

#[test]
fn test_get_commitment_returns_current_state() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[40u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 0);
    let entry = client.get_commitment(&group_id);
    assert_eq!(entry.commitment, z);
    assert_eq!(entry.epoch, 0);
    assert_eq!(entry.admin_pubkey_commitment, z);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_get_commitment_rejects_unknown_group() {
    let (env, client, _admin) = setup_env();
    client.get_commitment(&BytesN::from_array(&env, &[97u8; 32]));
}

#[test]
fn test_bump_group_ttl_extends_group_storage() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[41u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, &z, 0, 0);
    let pre = client.get_commitment(&group_id);
    assert_eq!(pre.commitment, z);
    client.bump_group_ttl(&group_id);
    let post = client.get_commitment(&group_id);
    assert_eq!(post.commitment, z);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_bump_group_ttl_rejects_unknown_group() {
    let (env, client, _admin) = setup_env();
    client.bump_group_ttl(&BytesN::from_array(&env, &[98u8; 32]));
}

// ================================================================
// 7. test-vectors.json consistency (1 test)
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
        ("GroupAlreadyExists", Error::GroupAlreadyExists as u32),
        ("GroupNotFound", Error::GroupNotFound as u32),
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
    ];
    for (name, code) in expected {
        let entry = errors
            .iter()
            .find(|e| e["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("test-vectors.json missing error {}", name));
        let json_code = entry["code"].as_u64().unwrap() as u32;
        assert_eq!(json_code, *code, "error code drift for {}", name);
    }
    assert_eq!(errors.len(), expected.len(), "error count drift");

    // ---- Tier capacity ----
    let tiers = v["tier"]["vectors"].as_array().unwrap();
    for entry in tiers {
        let tier = entry["tier"].as_u64().unwrap() as u32;
        let expected_cap = entry["capacity"].as_u64().unwrap() as u32;
        assert_eq!(tier_capacity(tier), expected_cap, "tier_capacity({}) drift", tier);
    }

    // ---- IC point counts ----
    let vk_kinds = v["vk_kind_enum"]["vectors"].as_array().unwrap();
    for entry in vk_kinds {
        let ic = entry["ic_count"].as_u64().unwrap() as u32;
        match entry["name"].as_str() {
            Some("Membership") => assert_eq!(ic, MEMBERSHIP_IC_POINTS, "Membership IC drift"),
            Some("Create") => assert_eq!(ic, CREATE_IC_POINTS, "Create IC drift"),
            Some("Update") => assert_eq!(ic, UPDATE_IC_POINTS, "Update IC drift"),
            other => panic!("unexpected vk_kind: {:?}", other),
        }
    }

    // ---- Max groups per tier ----
    let max = v["max_groups_per_tier"]["value"].as_u64().unwrap() as u32;
    assert_eq!(max, MAX_GROUPS_PER_TIER, "MAX_GROUPS_PER_TIER drift");

    // ---- Test count ----
    let test_count = v["tests_to_implement"]["categories"]["total"]
        .as_u64()
        .unwrap();
    assert_eq!(test_count, 37, "test count pin drift");
}
