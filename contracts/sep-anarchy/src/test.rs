//! Inline test suite for the SEP Anarchy contract — PLONK-migration era.
//!
//! Most tests exercise pre-verifier gates (canonicality, tier, replay,
//! PI shape, restricted mode) that don't depend on a passing
//! cryptographic verification, and assert the appropriate error
//! return. Where a real proof IS needed (the `update_commitment`
//! happy path), we use the canonical depth-5 update fixture from
//! `contracts/plonk-verifier/tests/fixtures/` and inject a group at
//! the matching `(c_old, epoch_old)` pair so the proof verifies.
//!
//! `create_group` has no verify-passing happy path here: the canonical
//! membership fixture binds to `epoch=1234`, but the contract enforces
//! `epoch=0` at creation. That gap is covered upstream by the
//! `accepts_canonical_proof_d{5,8,11}` tests in the `plonk-verifier`
//! crate (which exercise the verify call end-to-end on the same
//! bytes).

use super::*;
use soroban_sdk::testutils::Address as _;

// ================================================================
// Canonical fixtures (membership + update at depth=5)
// ================================================================

/// Canonical membership proof bytes (depth=5). Binds to
/// `MEMBERSHIP_PI[0]` (commitment) at epoch 1234.
const MEMBERSHIP_PROOF_BYTES: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/proof-d5.bin");

/// Canonical membership public inputs: `[commitment, epoch=1234]`
/// concatenated, BE-encoded, 64 bytes total.
const MEMBERSHIP_PI: &[u8; 64] =
    include_bytes!("../../plonk-verifier/tests/fixtures/pi-d5.bin");

/// Canonical update proof bytes (depth=5). Binds to
/// `(UPDATE_PI[0], UPDATE_PI[1], UPDATE_PI[2])` =
/// `(c_old, epoch_old=1234, c_new)`.
const UPDATE_PROOF_BYTES: &[u8; 1601] =
    include_bytes!("../../plonk-verifier/tests/fixtures/update-proof-d5.bin");

/// Canonical update public inputs concatenated (3 × 32 = 96 bytes):
/// `[c_old, epoch_old=1234, c_new]`.
const UPDATE_PI: &[u8; 96] =
    include_bytes!("../../plonk-verifier/tests/fixtures/update-pi-d5.bin");

/// Canonical update epoch_old (canonical witness uses 1234).
const CANONICAL_EPOCH: u64 = 1234;

fn membership_proof(env: &Env) -> BytesN<1601> {
    BytesN::from_array(env, MEMBERSHIP_PROOF_BYTES)
}

fn membership_commitment(env: &Env) -> BytesN<32> {
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&MEMBERSHIP_PI[..32]);
    BytesN::from_array(env, &arr)
}

fn membership_pi(env: &Env, commitment: BytesN<32>, epoch: u64) -> Vec<BytesN<32>> {
    let mut pi = Vec::new(env);
    pi.push_back(commitment);
    pi.push_back(be32_from_u64(env, epoch));
    pi
}

fn update_proof(env: &Env) -> BytesN<1601> {
    BytesN::from_array(env, UPDATE_PROOF_BYTES)
}

fn update_c_old(env: &Env) -> BytesN<32> {
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&UPDATE_PI[..32]);
    BytesN::from_array(env, &arr)
}

fn update_c_new(env: &Env) -> BytesN<32> {
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&UPDATE_PI[64..96]);
    BytesN::from_array(env, &arr)
}

fn update_pi(env: &Env) -> Vec<BytesN<32>> {
    let mut pi = Vec::new(env);
    pi.push_back(update_c_old(env));
    pi.push_back(be32_from_u64(env, CANONICAL_EPOCH));
    pi.push_back(update_c_new(env));
    pi
}

fn canonical_zero(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

fn non_canonical_fr(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0xff; 32])
}

/// 1601 bytes of `0xAA` — fails `parse_proof_bytes` at the first
/// length-prefix word (valid prefix is `5u64 LE` = `0x05_00_00_00…`,
/// not `0xAAAA…`). Useful for "InvalidProof at verify" tests where
/// we don't care whether the failure is at parse or pairing.
fn malformed_proof(env: &Env) -> BytesN<1601> {
    BytesN::from_array(env, &[0xAAu8; 1601])
}

// ================================================================
// Setup
// ================================================================

fn setup_env() -> (Env, SepAnarchyContractClient<'static>, Address) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(SepAnarchyContract, (admin.clone(),));
    let client = SepAnarchyContractClient::new(&env, &contract_id);
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
    tier: u32,
    member_count: u32,
    epoch: u64,
) {
    env.as_contract(contract_id, || {
        let entry = CommitmentEntry {
            commitment: commitment.clone(),
            epoch,
            timestamp: env.ledger().timestamp(),
            tier,
            active: true,
            member_count,
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
            member_count: 0,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Group(group_id.clone()), &entry);
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

#[test]
fn test_create_group_rejects_invalid_proof() {
    // Canonical witness binds to epoch=1234; create_group enforces
    // epoch=0 PI. A canonical proof submitted with epoch-0 PI will
    // fail PI consistency before reaching verify. To exercise the
    // verify call itself, use a malformed-proof byte stream.
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let commitment = membership_commitment(&env);
    let pi = membership_pi(&env, commitment.clone(), 0);
    let r = client.try_create_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &commitment,
        &0u32,
        &0u32,
        &malformed_proof(&env),
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
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = membership_pi(&env, z.clone(), 0);
    client.create_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &3u32, // out-of-range
        &0u32,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_create_group_rejects_duplicate_group_id() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[1u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, 0, 0, 0);

    let c = caller(&env);
    let pi = membership_pi(&env, z.clone(), 0);
    client.create_group(
        &c,
        &group_id,
        &z,
        &0u32,
        &0u32,
        &malformed_proof(&env),
        &pi,
    );
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
        &0u32,
        &0u32,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_create_group_rejects_pi_count_mismatch() {
    // Pass 1 PI when 2 are required.
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let mut pi = Vec::new(&env);
    pi.push_back(z.clone());
    client.create_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &0u32,
        &0u32,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_create_group_rejects_pi_commitment_mismatch() {
    // PI[0] must equal the wire `commitment` arg.
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = membership_pi(&env, BytesN::from_array(&env, &[7u8; 32]), 0);
    client.create_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &0u32,
        &0u32,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_create_group_rejects_pi_epoch_nonzero() {
    // PI[1] (epoch) must be zero at create_group.
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = membership_pi(&env, z.clone(), 1);
    client.create_group(
        &c,
        &BytesN::from_array(&env, &[1u8; 32]),
        &z,
        &0u32,
        &0u32,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #14)")]
fn test_create_group_restricted_mode_rejects_non_admin() {
    let (env, client, _admin) = setup_env();
    client.set_restricted_mode(&true);
    let c = caller(&env); // != admin
    let z = canonical_zero(&env);
    let pi = membership_pi(&env, z.clone(), 0);
    client.create_group(
        &c,
        &BytesN::from_array(&env, &[55u8; 32]),
        &z,
        &0u32,
        &0u32,
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
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = membership_pi(&env, z.clone(), 0);
    client.create_group(
        &c,
        &BytesN::from_array(&env, &[42u8; 32]),
        &z,
        &0u32,
        &0u32,
        &malformed_proof(&env),
        &pi,
    );
}

#[test]
fn test_create_group_accepts_member_count_zero() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = membership_pi(&env, z.clone(), 0);
    let r = client.try_create_group(
        &c,
        &BytesN::from_array(&env, &[2u8; 32]),
        &z,
        &0u32,
        &0u32, // explicit "not tracked"
        &malformed_proof(&env),
        &pi,
    );
    // Reaches verify (mock proof fails with InvalidProof), confirming
    // member_count=0 passes all pre-verify gates.
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof, got {:?}", other),
    }
}

#[test]
fn test_create_group_accepts_member_count_arbitrary() {
    let (env, client, _admin) = setup_env();
    let c = caller(&env);
    let z = canonical_zero(&env);
    let pi = membership_pi(&env, z.clone(), 0);
    let r = client.try_create_group(
        &c,
        &BytesN::from_array(&env, &[3u8; 32]),
        &z,
        &0u32,
        &4_294_967_295u32, // u32::MAX accepted
        &malformed_proof(&env),
        &pi,
    );
    match r {
        Err(Err(_)) | Err(Ok(Error::InvalidProof)) => {}
        other => panic!("expected InvalidProof, got {:?}", other),
    }
}

// ================================================================
// 3. update_commitment
// ================================================================

/// **Load-bearing.** A group at the canonical `(c_old, epoch_old)`
/// state accepts the canonical update proof: epoch advances to 1235,
/// commitment becomes `c_new`, history archives the old entry.
#[test]
fn test_update_commitment_happy_path() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[7u8; 32]);
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &update_c_old(&env),
        0, // tier=0 → depth=5 → matches canonical fixture
        5,
        CANONICAL_EPOCH,
    );

    client.update_commitment(&group_id, &update_proof(&env), &update_pi(&env));

    let post = client.get_commitment(&group_id);
    assert_eq!(post.commitment, update_c_new(&env), "commitment advanced");
    assert_eq!(post.epoch, CANONICAL_EPOCH + 1, "epoch incremented");
    assert_eq!(post.tier, 0, "tier preserved");
    assert!(post.active, "still active");
    assert_eq!(post.member_count, 5, "member_count preserved");
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_commitment_rejects_pi_count_mismatch() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[8u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, 0, 0, 0);
    let mut pi = Vec::new(&env); // empty
    pi.push_back(z);
    client.update_commitment(&group_id, &malformed_proof(&env), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_commitment_rejects_stale_c_old() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[9u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, 0, 0, 0);
    let mut pi = Vec::new(&env);
    pi.push_back(BytesN::from_array(&env, &[3u8; 32])); // wrong c_old
    pi.push_back(be32_from_u64(&env, 0));
    pi.push_back(z);
    client.update_commitment(&group_id, &malformed_proof(&env), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_commitment_rejects_wrong_epoch_old() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[11u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, 0, 0, 5);
    let mut pi = Vec::new(&env);
    pi.push_back(z.clone());
    pi.push_back(be32_from_u64(&env, 4)); // state.epoch=5; this says 4
    pi.push_back(z);
    client.update_commitment(&group_id, &malformed_proof(&env), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_update_commitment_rejects_non_canonical_c_new() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[12u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, 0, 0, 0);
    let mut pi = Vec::new(&env);
    pi.push_back(z.clone());
    pi.push_back(be32_from_u64(&env, 0));
    pi.push_back(non_canonical_fr(&env));
    client.update_commitment(&group_id, &malformed_proof(&env), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_update_commitment_rejects_replayed_proof() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[14u8; 32]);
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &update_c_old(&env),
        0,
        0,
        CANONICAL_EPOCH,
    );

    // Pre-mark the proof's hash as used — same logic as
    // SepAnarchyContract::record_proof.
    let proof = update_proof(&env);
    env.as_contract(&contract_id, || {
        let preimage = Bytes::from_slice(&env, &proof.to_array());
        let hash: BytesN<32> = env.crypto().sha256(&preimage).into();
        env.storage()
            .persistent()
            .set(&DataKey::UsedProof(hash), &true);
    });
    client.update_commitment(&group_id, &proof, &update_pi(&env));
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_update_commitment_rejects_inactive_group() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[15u8; 32]);
    inject_deactivated_group(&env, &contract_id, &group_id, 0);
    let z = canonical_zero(&env);
    let mut pi = Vec::new(&env);
    pi.push_back(z.clone());
    pi.push_back(be32_from_u64(&env, 0));
    pi.push_back(z);
    client.update_commitment(&group_id, &malformed_proof(&env), &pi);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_update_commitment_rejects_unknown_group() {
    let (env, client, _admin) = setup_env();
    let group_id = BytesN::from_array(&env, &[99u8; 32]);
    let z = canonical_zero(&env);
    let mut pi = Vec::new(&env);
    pi.push_back(z.clone());
    pi.push_back(be32_from_u64(&env, 0));
    pi.push_back(z);
    client.update_commitment(&group_id, &malformed_proof(&env), &pi);
}

#[test]
fn test_update_commitment_does_not_mutate_member_count() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[20u8; 32]);
    inject_group(
        &env,
        &contract_id,
        &group_id,
        &update_c_old(&env),
        0,
        42,
        CANONICAL_EPOCH,
    );
    let pre = client.get_commitment(&group_id);
    assert_eq!(pre.member_count, 42);

    client.update_commitment(&group_id, &update_proof(&env), &update_pi(&env));
    let post = client.get_commitment(&group_id);
    assert_eq!(
        post.member_count, 42,
        "member_count must remain whatever was set at create"
    );
}

// ================================================================
// 4. verify_membership
// ================================================================

/// **Load-bearing.** A group at the canonical `(commitment, epoch)`
/// returns `true` for the canonical membership proof.
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
        0,
        0,
        CANONICAL_EPOCH,
    );
    let pi = membership_pi(&env, membership_commitment(&env), CANONICAL_EPOCH);
    let result = client.verify_membership(&group_id, &membership_proof(&env), &pi);
    assert!(result, "canonical proof should verify");
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_verify_membership_rejects_wrong_commitment() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[31u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, 0, 0, 0);
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
    inject_group(&env, &contract_id, &group_id, &z, 0, 0, 5);
    let pi = membership_pi(&env, z, 4); // state.epoch=5; pass 4
    client.verify_membership(&group_id, &malformed_proof(&env), &pi);
}

#[test]
fn test_verify_membership_returns_false_on_inactive_group() {
    // verify_membership intentionally does NOT check state.active —
    // it stays read-only against any historical state. The contract
    // returns Ok(false) for an arbitrary proof against a deactivated
    // group's frozen state. This pins the defense-in-depth path.
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[33u8; 32]);
    inject_deactivated_group(&env, &contract_id, &group_id, 0);
    let pi = membership_pi(&env, canonical_zero(&env), 0);
    let result = client.verify_membership(&group_id, &malformed_proof(&env), &pi);
    assert!(!result);
}

// ================================================================
// 5. Queries
// ================================================================

#[test]
fn test_get_commitment_returns_current_state() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[50u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, 1, 7, 3);
    let entry = client.get_commitment(&group_id);
    assert_eq!(entry.commitment, z);
    assert_eq!(entry.tier, 1);
    assert_eq!(entry.member_count, 7);
    assert_eq!(entry.epoch, 3);
    assert!(entry.active);
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
fn test_get_history_returns_chronological_entries() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[51u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, 0, 0, 0);

    env.as_contract(&contract_id, || {
        let mut history: Vec<CommitmentEntry> = Vec::new(&env);
        for i in 0u64..3 {
            history.push_back(CommitmentEntry {
                commitment: BytesN::from_array(&env, &[i as u8; 32]),
                epoch: i,
                timestamp: 100 + i,
                tier: 0,
                active: true,
                member_count: 0,
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
#[should_panic(expected = "Error(Contract, #5)")]
fn test_get_history_rejects_unknown_group() {
    let (_env, client, _admin) = setup_env();
    let env = &client.env;
    let group_id = BytesN::from_array(env, &[97u8; 32]);
    client.get_history(&group_id, &10u32);
}

#[test]
fn test_bump_group_ttl_extends_group_storage() {
    let (env, client, _admin) = setup_env();
    let contract_id = client.address.clone();
    let group_id = BytesN::from_array(&env, &[52u8; 32]);
    let z = canonical_zero(&env);
    inject_group(&env, &contract_id, &group_id, &z, 0, 0, 0);

    let pre = client.get_commitment(&group_id);
    assert_eq!(pre.commitment, z);

    client.bump_group_ttl(&group_id);

    let post = client.get_commitment(&group_id);
    assert_eq!(post.commitment, z);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_bump_group_ttl_rejects_unknown_group() {
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

    // ---- Error codes (PLONK-era set; InvalidVkLength=9 and
    // InvalidPoint=26 dropped along with the Groth16 path) ----
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
        ("PublicInputsMismatch", Error::PublicInputsMismatch as u32),
        ("InvalidEpoch", Error::InvalidEpoch as u32),
        ("ProofReplay", Error::ProofReplay as u32),
        ("TierGroupLimitReached", Error::TierGroupLimitReached as u32),
        ("AdminOnly", Error::AdminOnly as u32),
        ("InvalidCommitmentEncoding", Error::InvalidCommitmentEncoding as u32),
        ("GroupStillActive", Error::GroupStillActive as u32),
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

    // ---- Max groups per tier ----
    let max = v["max_groups_per_tier"]["value"].as_u64().unwrap() as u32;
    assert_eq!(max, MAX_GROUPS_PER_TIER, "MAX_GROUPS_PER_TIER drift");
}
