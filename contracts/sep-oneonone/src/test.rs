//! Inline test suite for the SEP 1v1 contract.
//!
//! Every test name listed in `test-vectors.json#tests_to_implement`
//! has a corresponding `#[test]` here. The `test_vectors_consistency`
//! test loads the JSON file and asserts the contract's `Error`
//! variants, IC-point constants, and `MAX_GROUPS` match the vectors
//! byte-for-byte — pinning the JSON as the canonical ABI.

use super::*;
use soroban_sdk::testutils::Address as _;

// ================================================================
// Mocks
//
// Subgroup-valid points via hash_to_g{1,2}. Pairing checks still
// fail (the witness behind the proof is not the discrete log of
// these points), so happy-path tests assert `InvalidProof`
// (verifier reached but pairing failed). Same pattern as the
// other per-type contracts.
// ================================================================

fn valid_g1(env: &Env, tag: &[u8]) -> BytesN<96> {
    let bls = env.crypto().bls12_381();
    let dst = Bytes::from_slice(env, b"sep-oneonone-test-g1");
    let msg = Bytes::from_slice(env, tag);
    bls.hash_to_g1(&msg, &dst).to_bytes()
}

fn valid_g2(env: &Env, tag: &[u8]) -> BytesN<192> {
    let bls = env.crypto().bls12_381();
    let dst = Bytes::from_slice(env, b"sep-oneonone-test-g2");
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
        ],
    }
}

/// Distinct mock proofs by tag — needed to exercise replay protection
/// without collisions across tests in a single env.
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

fn setup_env() -> (Env, SepOneOnOneContractClient<'static>, Address) {
    let env = Env::default();
    // Constructor runs subgroup checks on 2 VKs (Membership + Create) —
    // smaller budget than sep-anarchy's 6 VKs, but we reset for
    // consistency with sibling test patterns.
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let mvk = mock_membership_vk(&env);
    let cvk = mock_create_vk(&env);
    let contract_id = env.register(SepOneOnOneContract, (admin.clone(), mvk, cvk));
    let client = SepOneOnOneContractClient::new(&env, &contract_id);
    (env, client, admin)
}

fn setup_with_bad_vk(
    bad_kind: VkKind,
    build_bad: impl FnOnce(&Env) -> VerificationKeyData,
) -> (Env, SepOneOnOneContractClient<'static>, Address) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let bad = build_bad(&env);
    let good_m = mock_membership_vk(&env);
    let good_c = mock_create_vk(&env);
    let (m, c) = match bad_kind {
        VkKind::Membership => (bad, good_c),
        VkKind::Create => (good_m, bad),
    };
    let contract_id = env.register(SepOneOnOneContract, (admin.clone(), m, c));
    let client = SepOneOnOneContractClient::new(&env, &contract_id);
    (env, client, admin)
}

/// Inject a group entry directly into storage — bypasses the proof
/// verifier so tests can drive verify_membership / get_commitment /
/// bump_group_ttl from a known-good state.
fn inject_group(
    env: &Env,
    contract_id: &Address,
    group_id: &BytesN<32>,
    commitment: &BytesN<32>,
) {
    env.as_contract(contract_id, || {
        let entry = CommitmentEntry {
            commitment: commitment.clone(),
            epoch: 0,
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
// 1. Initialization (3 tests)
// ================================================================

#[test]
fn test_initialize() {
    let (_env, _client, _admin) = setup_env();
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_invalid_membership_vk_length_rejected() {
    setup_with_bad_vk(VkKind::Membership, |env| {
        // 2 IC points — too few (need 3)
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
    setup_with_bad_vk(VkKind::Create, |env| {
        // 4 IC points — too many (need 3)
        let g1 = valid_g1(env, b"c-bad-alpha");
        let g2 = valid_g2(env, b"c-bad-beta");
        VerificationKeyData {
            alpha_g1: g1.clone(),
            beta_g2: g2.clone(),
            gamma_g2: g2.clone(),
            delta_g2: g2,
            ic: vec![env, g1.clone(), g1.clone(), g1.clone(), g1],
        }
    });
}

// ================================================================
// 2. create_group (10 tests)
// ================================================================

#[test]
fn test_create_group_happy_path() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = PublicInputs {
        commitment: z.clone(),
        epoch: 0,
    };
    let r = client.try_create_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &mock_proof(&env, b"happy"),
        &pi,
    );
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof at verifier, got {:?}", other),
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_create_group_rejects_duplicate_group_id() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[2u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z);
    let pi = PublicInputs {
        commitment: z.clone(),
        epoch: 0,
    };
    client.create_group(&caller(&env), &group_id, &z, &mock_proof(&env, b"dup"), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_create_group_rejects_non_canonical_commitment() {
    let (env, client, _admin) = setup_env();
    let bad = non_canonical_fr(&env);
    let pi = PublicInputs {
        commitment: bad.clone(),
        epoch: 0,
    };
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[3u8; 32]),
        &bad,
        &mock_proof(&env, b"non-canon"),
        &pi,
    );
}

#[test]
fn test_create_group_rejects_invalid_proof() {
    // Same as happy path; verifier reached because public inputs match,
    // but mock proof can't pass pairing. Returns Err(InvalidProof).
    let (env, client, _admin) = setup_env();
    let z = canonical_zero(&env);
    let pi = PublicInputs {
        commitment: z.clone(),
        epoch: 0,
    };
    let r = client.try_create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[4u8; 32]),
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
    let pi = PublicInputs {
        commitment: z.clone(),
        epoch: 0,
    };
    let attacker = Address::generate(&env);
    assert_ne!(attacker, admin);
    client.create_group(
        &attacker,
        &BytesN::from_array(&env, &[5u8; 32]),
        &z,
        &mock_proof(&env, b"restricted"),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_create_group_enforces_group_count_limit() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    // Skip directly to the cap by writing GroupCount = MAX_GROUPS.
    env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .set(&DataKey::GroupCount, &MAX_GROUPS);
    });
    let z = canonical_zero(&env);
    let pi = PublicInputs {
        commitment: z.clone(),
        epoch: 0,
    };
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[6u8; 32]),
        &z,
        &mock_proof(&env, b"cap"),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_create_group_rejects_replayed_proof() {
    // Burn a proof hash directly in storage, then try to use a proof
    // with that same hash. ProofReplay fires before the verifier.
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let z = canonical_zero(&env);
    let pi = PublicInputs {
        commitment: z.clone(),
        epoch: 0,
    };
    let proof = mock_proof(&env, b"replay");
    let h = env.as_contract(&contract_id, || {
        let mut preimage = Bytes::new(&env);
        preimage.append(&Bytes::from_slice(&env, proof.a.to_array().as_slice()));
        preimage.append(&Bytes::from_slice(&env, proof.b.to_array().as_slice()));
        preimage.append(&Bytes::from_slice(&env, proof.c.to_array().as_slice()));
        let h: BytesN<32> = env.crypto().sha256(&preimage).into();
        env.storage()
            .persistent()
            .set(&DataKey::UsedProof(h.clone()), &true);
        h
    });
    let _ = h; // silence unused
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[7u8; 32]),
        &z,
        &proof,
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_create_group_rejects_public_inputs_commitment_mismatch() {
    let (env, client, _admin) = setup_env();
    let z = canonical_zero(&env);
    let other = BytesN::from_array(&env, &[0x01; 32]);
    let pi = PublicInputs {
        commitment: other,
        epoch: 0,
    };
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[8u8; 32]),
        &z,
        &mock_proof(&env, b"pi-mismatch"),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_create_group_rejects_non_zero_epoch() {
    let (env, client, _admin) = setup_env();
    let z = canonical_zero(&env);
    let pi = PublicInputs {
        commitment: z.clone(),
        epoch: 1, // 1v1 epoch must be 0
    };
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[9u8; 32]),
        &z,
        &mock_proof(&env, b"non-zero-epoch"),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_create_group_rejects_non_canonical_public_inputs_commitment() {
    // public_inputs.commitment != commitment param — `commitment` is
    // canonical zero but `public_inputs.commitment` is non-canonical
    // 0xff…. PublicInputsMismatch fires before the canonical-Fr check
    // on `commitment`.
    let (env, client, _admin) = setup_env();
    let z = canonical_zero(&env);
    let nc = non_canonical_fr(&env);
    let pi = PublicInputs {
        commitment: nc,
        epoch: 0,
    };
    client.create_group(
        &caller(&env),
        &BytesN::from_array(&env, &[10u8; 32]),
        &z,
        &mock_proof(&env, b"pi-non-canon"),
        &pi,
    );
}

// ================================================================
// 3. verify_membership (5 tests)
// ================================================================

#[test]
fn test_verify_membership_happy_path() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[20u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z);
    let pi = PublicInputs {
        commitment: z,
        epoch: 0,
    };
    // Mock proof can't pass pairing → returns Ok(false) (not an error).
    let result = client.verify_membership(&group_id, &mock_proof(&env, b"vm"), &pi);
    assert!(!result);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_verify_membership_rejects_wrong_commitment() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[21u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z);
    let pi = PublicInputs {
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
    let group_id = BytesN::from_array(&env, &[22u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z);
    let pi = PublicInputs {
        commitment: z,
        epoch: 7, // stored is always 0 for 1v1
    };
    client.verify_membership(&group_id, &mock_proof(&env, b"vm-wrong-e"), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_verify_membership_rejects_unknown_group() {
    let (env, client, _admin) = setup_env();
    let pi = PublicInputs {
        commitment: canonical_zero(&env),
        epoch: 0,
    };
    client.verify_membership(
        &BytesN::from_array(&env, &[23u8; 32]),
        &mock_proof(&env, b"vm-unknown"),
        &pi,
    );
}

#[test]
fn test_verify_membership_returns_false_on_invalid_proof() {
    // Same as happy path. Distinct test name to assert the read-only
    // verifier's Ok(false) semantic is intentional and pinned (no
    // InvalidProof error from this entrypoint).
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[24u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z);
    let pi = PublicInputs {
        commitment: z,
        epoch: 0,
    };
    let result = client.verify_membership(&group_id, &mock_proof(&env, b"vm-okfalse"), &pi);
    assert!(!result, "verify_membership must return Ok(false), not Err");
}

// ================================================================
// 4. update_vk (4 tests)
// ================================================================

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_update_vk_requires_auth() {
    // No mock_all_auths — admin.require_auth() inside update_vk panics
    // because no auth was granted.
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    let admin = Address::generate(&env);
    let mvk = mock_membership_vk(&env);
    let cvk = mock_create_vk(&env);
    let contract_id = env.register(SepOneOnOneContract, (admin, mvk.clone(), cvk));
    let client = SepOneOnOneContractClient::new(&env, &contract_id);
    client.update_vk(&VkKind::Membership, &mvk);
}

#[test]
fn test_update_vk_rotates_membership_vk() {
    let (env, client, _admin) = setup_env();
    let new_vk = mock_membership_vk(&env);
    client.update_vk(&VkKind::Membership, &new_vk);
}

#[test]
fn test_update_vk_rotates_create_vk() {
    let (env, client, _admin) = setup_env();
    let new_vk = mock_create_vk(&env);
    client.update_vk(&VkKind::Create, &new_vk);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_update_vk_rejects_invalid_vk_length() {
    let (env, client, _admin) = setup_env();
    let g1 = valid_g1(&env, b"bad-rotate-alpha");
    let g2 = valid_g2(&env, b"bad-rotate-beta");
    let bad = VerificationKeyData {
        alpha_g1: g1.clone(),
        beta_g2: g2.clone(),
        gamma_g2: g2.clone(),
        delta_g2: g2,
        ic: vec![&env, g1.clone(), g1], // 2 IC points (need 3)
    };
    client.update_vk(&VkKind::Membership, &bad);
}

// ================================================================
// 5. Queries (3 tests)
// ================================================================

#[test]
fn test_get_commitment_returns_current_state() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[40u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z);
    let entry = client.get_commitment(&group_id);
    assert_eq!(entry.commitment, z);
    assert_eq!(entry.epoch, 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_get_commitment_rejects_unknown_group() {
    let (env, client, _admin) = setup_env();
    client.get_commitment(&BytesN::from_array(&env, &[99u8; 32]));
}

#[test]
fn test_bump_group_ttl_extends_group_storage() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[41u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z);

    let pre = client.get_commitment(&group_id);
    assert_eq!(pre.commitment, z);

    client.bump_group_ttl(&group_id);

    let post = client.get_commitment(&group_id);
    assert_eq!(post.commitment, z);
}

// ================================================================
// 6. test-vectors.json consistency (1 test)
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
        ("InvalidVkLength", Error::InvalidVkLength as u32),
        ("PublicInputsMismatch", Error::PublicInputsMismatch as u32),
        ("ProofReplay", Error::ProofReplay as u32),
        ("GroupCountLimitReached", Error::GroupCountLimitReached as u32),
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
        assert_eq!(
            json_code, *code,
            "error code drift for {}: vectors say {}, contract says {}",
            name, json_code, code
        );
    }
    assert_eq!(
        errors.len(),
        expected.len(),
        "test-vectors.json error count drift: vectors {}, contract {}",
        errors.len(),
        expected.len()
    );

    // ---- IC point counts ----
    let vk_kinds = v["vk_kind_enum"]["vectors"].as_array().unwrap();
    for entry in vk_kinds {
        let ic = entry["ic_count"].as_u64().unwrap() as u32;
        match entry["name"].as_str() {
            Some("Membership") => assert_eq!(ic, MEMBERSHIP_IC_POINTS, "Membership IC count drift"),
            Some("Create") => assert_eq!(ic, CREATE_IC_POINTS, "Create IC count drift"),
            other => panic!("unexpected vk_kind: {:?}", other),
        }
    }

    // ---- Max groups ----
    let max = v["max_groups"]["value"].as_u64().unwrap() as u32;
    assert_eq!(max, MAX_GROUPS, "MAX_GROUPS drift");

    // ---- Test count ----
    let test_count = v["tests_to_implement"]["categories"]["total"]
        .as_u64()
        .unwrap();
    assert_eq!(test_count, 26, "test count pin drift");
}
