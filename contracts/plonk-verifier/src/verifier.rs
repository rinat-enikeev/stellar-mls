//! Top-level Soroban PLONK verifier — closes the Phase C.2 prereq loop.
//!
//! Wires together every module in this crate built across PRs
//! #185–#191 and runs the final pairing equation via
//! `env.crypto().bls12_381().pairing_check`. Mirrors the off-chain
//! reference at `sep-xxxx-circuits::circuit::plonk::verifier`
//! (PR #184).
//!
//! For our no-Plookup, single-instance case the verification
//! equation reduces to:
//!
//! ```text
//!   A = opening_proof + u · shifted_opening_proof
//!   B = [D]_1
//!     + ζ · opening_proof
//!     + u · ζ·g · shifted_opening_proof
//!     − [E]_1 · g_1                       (g_1 = vk.open_key.g)
//!
//!   accept iff e(A, [τ]_2) = e(B, [1]_2)
//! ```
//!
//! Implemented as the multi-pairing check
//! `e(A, [τ]_2) · e(−B, [1]_2) = 1`, fed through
//! `env.crypto().bls12_381().pairing_check(&[(A, βh), (−B, h)])` —
//! one host call.
//!
//! `[D]_1` comes from PR #190's `aggregate_poly_commitments`
//! (MSM-folded here); `[E]_1` from PR #191's `aggregate_evaluations`.
//!
//! ## `srs_g2_compressed` contract
//!
//! Soroban's BLS12-381 host primitives don't expose a "compress this
//! G2 point" operation, so we can't derive the transcript's
//! `srs_g2_compressed` (96 B BE) on-chain from the parsed VK's
//! `open_key_powers_of_h[1]` (192 B uncompressed). The contract
//! embeds **both** forms — the uncompressed VK via
//! `include_bytes!("…vk.bin")` and the pre-computed compressed G2
//! via `include_bytes!("…srs-g2.bin")` — both produced together by
//! the bake-vk pipeline.

use soroban_sdk::crypto::bls12_381::{Fr, G1Affine, G2Affine};
use soroban_sdk::{BytesN, Env, Vec};

use crate::byte_helpers::{decode_fr_array, fr_from_le_bytes, g1_from_bytes, g2_from_bytes};
use crate::proof_format::{ParsedProof, FR_LEN, NUM_WIRE_SIGMA_EVALS, NUM_WIRE_TYPES};
use crate::verifier_aggregate::{aggregate_poly_commitments, ChallengesFr};
use crate::verifier_aggregate_evals::aggregate_evaluations;
use crate::verifier_challenges::compute_challenges;
use crate::verifier_lin_poly::compute_lin_poly_constant_term;
use crate::verifier_polys::{
    evaluate_pi_poly, evaluate_vanishing_poly, first_and_last_lagrange_coeffs, DomainParams,
};
use crate::vk_format::{ParsedVerifyingKey, G2_COMPRESSED_LEN};

/// Errors `verify` can raise. `PairingMismatch` is the verifier's
/// "rejected the proof" outcome; the others reflect malformed-input
/// conditions that should not happen if the contract entry point
/// has run the upstream byte parsers.
///
/// **Note on adversarial inputs**: this enum does *not* cover off-
/// curve / non-canonical G1 / G2 / Fr bytes. The byte parsers
/// (`parse_proof_bytes`, `parse_vk_bytes`) only validate structural
/// shape; cryptographic validation happens deep inside Soroban's
/// `g1_msm` / `pairing_check` host primitives, which **trap** on
/// invalid inputs rather than returning errors. A trap inside a
/// contract entry point is consensus-safe — it rejects the proof
/// the same way `Err(PairingMismatch)` does — but the caller sees
/// a contract failure, not a `Result::Err`. The contract surface
/// should treat both as "verification rejected" and not depend on
/// distinguishing them.
#[derive(Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// Public-input count doesn't match what the VK expects.
    BadPublicInputCount { expected: u64, actual: u32 },
    /// Pairing equation failed — proof rejected.
    PairingMismatch,
}

/// Verify a TurboPlonk proof for a circuit using the no-Plookup,
/// single-instance flow. Returns `Ok(())` to accept,
/// `Err(_)` to reject.
///
/// Inputs are byte-form via the parsed-VK / parsed-proof structures
/// and a pre-compressed SRS G2 element. The contract entry point
/// embeds the VK + compressed G2 via `include_bytes!` and parses on
/// the proof bytes the user submitted.
pub fn verify(
    env: &Env,
    vk: &ParsedVerifyingKey,
    srs_g2_compressed: &[u8; G2_COMPRESSED_LEN],
    proof: &ParsedProof,
    public_inputs_be: &[[u8; FR_LEN]],
) -> Result<(), VerifyError> {
    // --- 0. Public-input count must match the VK header. ----------
    if public_inputs_be.len() as u64 != vk.num_inputs {
        return Err(VerifyError::BadPublicInputCount {
            expected: vk.num_inputs,
            actual: public_inputs_be.len() as u32,
        });
    }

    // --- 1. Drive the transcript and reduce the 6 challenges. -----
    let raw = compute_challenges(env, vk, srs_g2_compressed, public_inputs_be, proof);
    let challenges = ChallengesFr {
        beta: Fr::from_bytes(BytesN::from_array(env, &raw.beta)),
        gamma: Fr::from_bytes(BytesN::from_array(env, &raw.gamma)),
        alpha: Fr::from_bytes(BytesN::from_array(env, &raw.alpha)),
        zeta: Fr::from_bytes(BytesN::from_array(env, &raw.zeta)),
        v: Fr::from_bytes(BytesN::from_array(env, &raw.v)),
        u: Fr::from_bytes(BytesN::from_array(env, &raw.u)),
    };

    // --- 2. Domain-derived polynomial evaluations at ζ. -----------
    let params = DomainParams::for_size(env, vk.domain_size);
    let vanish_eval = evaluate_vanishing_poly(&challenges.zeta, &params);
    let (lagrange_1_eval, _lagrange_n_eval) =
        first_and_last_lagrange_coeffs(&challenges.zeta, &vanish_eval, &params);

    // --- 3. Public-input polynomial evaluation. -------------------
    // Reduce public inputs BE→Fr.
    let mut public_inputs_fr: alloc::vec::Vec<Fr> =
        alloc::vec::Vec::with_capacity(public_inputs_be.len());
    for be in public_inputs_be.iter() {
        public_inputs_fr.push(Fr::from_bytes(BytesN::from_array(env, be)));
    }
    let pi_eval = evaluate_pi_poly(
        &public_inputs_fr,
        &challenges.zeta,
        &vanish_eval,
        &params,
    );

    // --- 4. Linearisation-polynomial constant term r_0. -----------
    let w_evals: [Fr; NUM_WIRE_TYPES] =
        decode_fr_array(env, &proof.wires_evals);
    let sigma_evals: [Fr; NUM_WIRE_SIGMA_EVALS] =
        decode_fr_array(env, &proof.wire_sigma_evals);
    let perm_next_eval = fr_from_le_bytes(env, &proof.perm_next_eval);
    let lin_poly_constant = compute_lin_poly_constant_term(
        challenges.alpha.clone(),
        challenges.beta.clone(),
        challenges.gamma.clone(),
        pi_eval,
        lagrange_1_eval.clone(),
        &w_evals,
        &sigma_evals,
        perm_next_eval,
    );

    // --- 5. Aggregate poly commitments → MSM-able [D]_1. ----------
    let agg = aggregate_poly_commitments(
        env,
        &challenges,
        vanish_eval,
        lagrange_1_eval,
        vk,
        proof,
    );
    let d_1 = agg.multi_scalar_multiply(env);

    // --- 6. Aggregate evaluations → scalar [E]_1. -----------------
    let aggregate_eval = aggregate_evaluations(env, lin_poly_constant, proof, &agg.v_uv_buffer);

    // --- 7. Final pairing check. ----------------------------------
    final_pairing_check(env, vk, proof, &challenges, &params, d_1, aggregate_eval)
}

/// Run the final pairing equation
/// `e(A, [τ]_2) ?= e(B, [1]_2)` in the form
/// `e(A, [τ]_2) · e(−B, [1]_2) = 1`.
///
/// `A = opening_proof + u·shifted_opening_proof`
/// `B = [D]_1 + ζ·opening_proof + u·ζ·g·shifted_opening_proof − [E]_1·g_1`
fn final_pairing_check(
    env: &Env,
    vk: &ParsedVerifyingKey,
    proof: &ParsedProof,
    challenges: &ChallengesFr,
    params: &DomainParams,
    d_1: G1Affine,
    aggregate_eval: Fr,
) -> Result<(), VerifyError> {
    let opening = g1_from_bytes(env, &proof.opening_proof);
    let shifted_opening = g1_from_bytes(env, &proof.shifted_opening_proof);
    let g_1 = g1_from_bytes(env, &vk.open_key_g);
    let h = g2_from_bytes(env, &vk.open_key_h);
    // `beta_h` here is the KZG SRS element `[τ]_2` — the second G2
    // generator, classically denoted `[β]_2` in KZG papers (jf-plonk's
    // VerifyingKey.open_key.beta_h follows that convention). It is
    // **not** related to `challenges.beta`, the PLONK Fiat-Shamir β
    // squeezed earlier in the transcript. Two distinct β's; only the
    // KZG one shows up in the pairing.
    let beta_h = g2_from_bytes(env, &vk.open_key_beta_h);

    let zeta = challenges.zeta.clone();
    let u = challenges.u.clone();
    let zeta_g = zeta.clone() * params.group_gen.clone();

    // A = opening + u · shifted_opening
    let a = opening.clone() + (shifted_opening.clone() * u.clone());
    // B = [D]_1 + ζ·opening + u·ζ·g·shifted_opening − [E]_1·g_1
    let zeta_opening = opening * zeta;
    let uzg_shifted = shifted_opening * (u * zeta_g);
    let e_g_1 = g_1 * aggregate_eval;
    // Subtract via add-with-negate (Soroban G1 has Neg but no Sub).
    let b = d_1 + zeta_opening + uzg_shifted + (-e_g_1);

    // pairing_check([(A, βh), (−B, h)]) returns true iff product is 1.
    let mut g1_vec: Vec<G1Affine> = Vec::new(env);
    g1_vec.push_back(a);
    g1_vec.push_back(-b);
    let mut g2_vec: Vec<G2Affine> = Vec::new(env);
    g2_vec.push_back(beta_h);
    g2_vec.push_back(h);

    let ok = env.crypto().bls12_381().pairing_check(g1_vec, g2_vec);
    if ok {
        Ok(())
    } else {
        Err(VerifyError::PairingMismatch)
    }
}

// Internal Fr/G1/G2 byte conversion helpers live in
// `crate::byte_helpers`. `extern crate alloc;` is at the crate root
// (`lib.rs`).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proof_format::parse_proof_bytes;
    use crate::test_fixtures::{build_synthetic_proof_bytes, build_synthetic_vk_bytes};
    use crate::vk_format::parse_vk_bytes;

    fn synthetic_srs_g2_compressed() -> [u8; G2_COMPRESSED_LEN] {
        let mut a = [0u8; G2_COMPRESSED_LEN];
        a[0] = 0xDE;
        a[1] = 0xAD;
        a
    }

    fn synthetic_public_inputs() -> [[u8; FR_LEN]; 2] {
        let mut p = [[0u8; FR_LEN]; 2];
        p[0][0] = 0x70;
        p[1][0] = 0x71;
        p
    }

    /// Wrong public-input count short-circuits with
    /// `BadPublicInputCount` before any crypto work. Pure
    /// shape-validation; doesn't need on-curve G1 bytes.
    #[test]
    fn rejects_wrong_public_input_count() {
        let env = Env::default();
        let vk_bytes = build_synthetic_vk_bytes(8192, 2);
        let proof_bytes = build_synthetic_proof_bytes();
        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let parsed_proof = parse_proof_bytes(&proof_bytes).expect("parse proof");
        let srs_g2 = synthetic_srs_g2_compressed();

        // VK declares num_inputs=2; pass only 1.
        let too_few: [[u8; FR_LEN]; 1] = [[0x70; FR_LEN]];
        let result = verify(&env, &parsed_vk, &srs_g2, &parsed_proof, &too_few);
        assert_eq!(
            result,
            Err(VerifyError::BadPublicInputCount {
                expected: 2,
                actual: 1,
            }),
        );

        // Three is also wrong.
        let too_many: [[u8; FR_LEN]; 3] = [[0u8; FR_LEN]; 3];
        let result =
            verify(&env, &parsed_vk, &srs_g2, &parsed_proof, &too_many);
        assert_eq!(
            result,
            Err(VerifyError::BadPublicInputCount {
                expected: 2,
                actual: 3,
            }),
        );

        // The correct count (2) doesn't short-circuit here — it
        // proceeds into the crypto path which fails on synthetic
        // (off-curve) G1 bytes. We don't assert success; the
        // `accepts_canonical_proof_for_all_tiers` test in the
        // fixture-bundling follow-up exercises the accept path.
    }

    /// Bytes regenerated from the prover-side
    /// `plonk_verifier_fixtures_match_or_regenerate` test (the
    /// off-chain `sep-xxxx-circuits` crate, all three membership
    /// tiers). Re-emit by running:
    ///
    /// ```text
    /// STELLAR_REGEN_FIXTURES=1 cargo test --features plonk \
    ///   -p sep-xxxx-circuits \
    ///   circuit::plonk::verifier::tests::plonk_verifier_fixtures_match_or_regenerate
    /// ```
    ///
    /// from the workspace root. Without the env var that same test
    /// is a drift detector — it asserts these bytes still match what
    /// the canonical-witness pipeline produces today.
    ///
    /// `[τ]_2` is shared across tiers (single SRS), so only one
    /// `srs-g2-compressed.bin` ships.
    const FIXTURE_VK_D5: &[u8; crate::vk_format::VK_LEN] =
        include_bytes!("../tests/fixtures/vk-d5.bin");
    const FIXTURE_PROOF_D5: &[u8; crate::proof_format::PROOF_LEN] =
        include_bytes!("../tests/fixtures/proof-d5.bin");
    const FIXTURE_PI_D5: &[u8; 2 * FR_LEN] =
        include_bytes!("../tests/fixtures/pi-d5.bin");

    const FIXTURE_VK_D8: &[u8; crate::vk_format::VK_LEN] =
        include_bytes!("../tests/fixtures/vk-d8.bin");
    const FIXTURE_PROOF_D8: &[u8; crate::proof_format::PROOF_LEN] =
        include_bytes!("../tests/fixtures/proof-d8.bin");
    const FIXTURE_PI_D8: &[u8; 2 * FR_LEN] =
        include_bytes!("../tests/fixtures/pi-d8.bin");

    const FIXTURE_VK_D11: &[u8; crate::vk_format::VK_LEN] =
        include_bytes!("../tests/fixtures/vk-d11.bin");
    const FIXTURE_PROOF_D11: &[u8; crate::proof_format::PROOF_LEN] =
        include_bytes!("../tests/fixtures/proof-d11.bin");
    const FIXTURE_PI_D11: &[u8; 2 * FR_LEN] =
        include_bytes!("../tests/fixtures/pi-d11.bin");

    const FIXTURE_SRS_G2: &[u8; G2_COMPRESSED_LEN] =
        include_bytes!("../tests/fixtures/srs-g2-compressed.bin");

    // Update-circuit fixtures — produced by the same prover-side regen
    // test. Public-input layout: `(c_old, epoch_old, c_new)` BE-encoded.
    const FIXTURE_UPDATE_VK_D5: &[u8; crate::vk_format::VK_LEN] =
        include_bytes!("../tests/fixtures/update-vk-d5.bin");
    const FIXTURE_UPDATE_PROOF_D5: &[u8; crate::proof_format::PROOF_LEN] =
        include_bytes!("../tests/fixtures/update-proof-d5.bin");
    const FIXTURE_UPDATE_PI_D5: &[u8; 3 * FR_LEN] =
        include_bytes!("../tests/fixtures/update-pi-d5.bin");

    const FIXTURE_UPDATE_VK_D8: &[u8; crate::vk_format::VK_LEN] =
        include_bytes!("../tests/fixtures/update-vk-d8.bin");
    const FIXTURE_UPDATE_PROOF_D8: &[u8; crate::proof_format::PROOF_LEN] =
        include_bytes!("../tests/fixtures/update-proof-d8.bin");
    const FIXTURE_UPDATE_PI_D8: &[u8; 3 * FR_LEN] =
        include_bytes!("../tests/fixtures/update-pi-d8.bin");

    const FIXTURE_UPDATE_VK_D11: &[u8; crate::vk_format::VK_LEN] =
        include_bytes!("../tests/fixtures/update-vk-d11.bin");
    const FIXTURE_UPDATE_PROOF_D11: &[u8; crate::proof_format::PROOF_LEN] =
        include_bytes!("../tests/fixtures/update-proof-d11.bin");
    const FIXTURE_UPDATE_PI_D11: &[u8; 3 * FR_LEN] =
        include_bytes!("../tests/fixtures/update-pi-d11.bin");

    /// Split a flat 64-byte PI fixture into the `[[u8; 32]; 2]` shape
    /// `verify` expects (membership: 2 PIs).
    fn split_pi(pi: &[u8; 2 * FR_LEN]) -> [[u8; FR_LEN]; 2] {
        let mut out = [[0u8; FR_LEN]; 2];
        out[0].copy_from_slice(&pi[..FR_LEN]);
        out[1].copy_from_slice(&pi[FR_LEN..]);
        out
    }

    /// Split a flat 96-byte PI fixture into the `[[u8; 32]; 3]` shape
    /// `verify` expects (update: 3 PIs).
    fn split_update_pi(pi: &[u8; 3 * FR_LEN]) -> [[u8; FR_LEN]; 3] {
        let mut out = [[0u8; FR_LEN]; 3];
        out[0].copy_from_slice(&pi[..FR_LEN]);
        out[1].copy_from_slice(&pi[FR_LEN..2 * FR_LEN]);
        out[2].copy_from_slice(&pi[2 * FR_LEN..]);
        out
    }

    /// Drive the accept path on a given tier's fixtures. Used by the
    /// per-tier accept tests to keep test bodies thin while still
    /// surfacing tier-specific failures distinctly.
    fn assert_accepts(
        vk_bytes: &[u8; crate::vk_format::VK_LEN],
        proof_bytes: &[u8; crate::proof_format::PROOF_LEN],
        pi: &[u8; 2 * FR_LEN],
        depth: usize,
    ) {
        let env = Env::default();
        let parsed_vk = parse_vk_bytes(vk_bytes).expect("parse vk");
        let parsed_proof = parse_proof_bytes(proof_bytes).expect("parse proof");
        let public_inputs = split_pi(pi);
        let result = verify(&env, &parsed_vk, FIXTURE_SRS_G2, &parsed_proof, &public_inputs);
        assert_eq!(
            result,
            Ok(()),
            "canonical depth-{depth} proof was rejected: {result:?}",
        );
    }

    /// **Load-bearing.** Canonical proof at depth=5 verifies.
    #[test]
    fn accepts_canonical_proof_d5() {
        assert_accepts(FIXTURE_VK_D5, FIXTURE_PROOF_D5, FIXTURE_PI_D5, 5);
    }

    /// **Load-bearing.** Canonical proof at depth=8 verifies. Pins
    /// the on-chain `DomainParams` / FFT-precompute path for the
    /// middle tier — a size-dependent regression here would slip
    /// past the off-chain `accepts_canonical_proof_for_all_tiers`.
    #[test]
    fn accepts_canonical_proof_d8() {
        assert_accepts(FIXTURE_VK_D8, FIXTURE_PROOF_D8, FIXTURE_PI_D8, 8);
    }

    /// **Load-bearing.** Canonical proof at depth=11 verifies. Pins
    /// the largest-domain path on-chain (n=32768).
    #[test]
    fn accepts_canonical_proof_d11() {
        assert_accepts(FIXTURE_VK_D11, FIXTURE_PROOF_D11, FIXTURE_PI_D11, 11);
    }

    /// Drive the accept path on a given tier's **update** fixtures.
    fn assert_accepts_update(
        vk_bytes: &[u8; crate::vk_format::VK_LEN],
        proof_bytes: &[u8; crate::proof_format::PROOF_LEN],
        pi: &[u8; 3 * FR_LEN],
        depth: usize,
    ) {
        let env = Env::default();
        let parsed_vk = parse_vk_bytes(vk_bytes).expect("parse update vk");
        let parsed_proof = parse_proof_bytes(proof_bytes).expect("parse update proof");
        let public_inputs = split_update_pi(pi);
        let result = verify(&env, &parsed_vk, FIXTURE_SRS_G2, &parsed_proof, &public_inputs);
        assert_eq!(
            result,
            Ok(()),
            "canonical depth-{depth} update proof was rejected: {result:?}",
        );
    }

    /// **Load-bearing.** Canonical update proof at depth=5 verifies.
    /// Closes the contract-side prereq for `update_commitment` —
    /// without this, sep-anarchy's update entrypoint would have no
    /// PLONK VK to call into.
    #[test]
    fn accepts_canonical_update_proof_d5() {
        assert_accepts_update(
            FIXTURE_UPDATE_VK_D5,
            FIXTURE_UPDATE_PROOF_D5,
            FIXTURE_UPDATE_PI_D5,
            5,
        );
    }

    /// **Load-bearing.** Canonical update proof at depth=8 verifies.
    #[test]
    fn accepts_canonical_update_proof_d8() {
        assert_accepts_update(
            FIXTURE_UPDATE_VK_D8,
            FIXTURE_UPDATE_PROOF_D8,
            FIXTURE_UPDATE_PI_D8,
            8,
        );
    }

    /// **Load-bearing.** Canonical update proof at depth=11 verifies.
    #[test]
    fn accepts_canonical_update_proof_d11() {
        assert_accepts_update(
            FIXTURE_UPDATE_VK_D11,
            FIXTURE_UPDATE_PROOF_D11,
            FIXTURE_UPDATE_PI_D11,
            11,
        );
    }

    // 1v1 create-circuit fixtures — single tier (depth=5 hardcoded;
    // sep-oneonone has no per-tier dimension). 2 PIs: `(commitment,
    // epoch=0)`. The circuit enforces "exactly 2 non-zero leaves at
    // founding" via in-circuit constants for positions 2..32.
    const FIXTURE_ONEONONE_CREATE_VK: &[u8; crate::vk_format::VK_LEN] =
        include_bytes!("../tests/fixtures/oneonone-create-vk.bin");
    const FIXTURE_ONEONONE_CREATE_PROOF: &[u8; crate::proof_format::PROOF_LEN] =
        include_bytes!("../tests/fixtures/oneonone-create-proof.bin");
    const FIXTURE_ONEONONE_CREATE_PI: &[u8; 2 * FR_LEN] =
        include_bytes!("../tests/fixtures/oneonone-create-pi.bin");

    /// **Load-bearing.** Canonical 1v1 create proof verifies.
    #[test]
    fn accepts_canonical_oneonone_create_proof() {
        let env = Env::default();
        let parsed_vk = parse_vk_bytes(FIXTURE_ONEONONE_CREATE_VK).expect("parse vk");
        let parsed_proof =
            parse_proof_bytes(FIXTURE_ONEONONE_CREATE_PROOF).expect("parse proof");
        let public_inputs = split_pi(FIXTURE_ONEONONE_CREATE_PI);
        let result = verify(&env, &parsed_vk, FIXTURE_SRS_G2, &parsed_proof, &public_inputs);
        assert_eq!(result, Ok(()), "1v1 create proof was rejected: {result:?}");
    }

    /// Tampered commitment in 1v1 create-PI rejects.
    #[test]
    fn rejects_tampered_oneonone_create_commitment() {
        let env = Env::default();
        let parsed_vk = parse_vk_bytes(FIXTURE_ONEONONE_CREATE_VK).expect("parse vk");
        let parsed_proof =
            parse_proof_bytes(FIXTURE_ONEONONE_CREATE_PROOF).expect("parse proof");
        let mut public_inputs = split_pi(FIXTURE_ONEONONE_CREATE_PI);
        public_inputs[0][FR_LEN - 1] ^= 0x01;
        let result = verify(&env, &parsed_vk, FIXTURE_SRS_G2, &parsed_proof, &public_inputs);
        assert_eq!(result, Err(VerifyError::PairingMismatch));
    }

    /// Drive the reject path on update fixtures: tamper one PI byte and
    /// expect `Err(PairingMismatch)`. Catches verifier-side regressions
    /// in 3-PI public-input encoding that the 2-PI tamper tests
    /// (`rejects_tampered_commitment` / `_epoch`) don't cover.
    fn assert_rejects_tampered_update(pi_index: usize, label: &str) {
        let env = Env::default();
        let parsed_vk = parse_vk_bytes(FIXTURE_UPDATE_VK_D5).expect("parse update vk");
        let parsed_proof =
            parse_proof_bytes(FIXTURE_UPDATE_PROOF_D5).expect("parse update proof");
        let mut public_inputs = split_update_pi(FIXTURE_UPDATE_PI_D5);
        // Flip the LSB (BE) — a different valid Fr.
        public_inputs[pi_index][FR_LEN - 1] ^= 0x01;
        let result = verify(&env, &parsed_vk, FIXTURE_SRS_G2, &parsed_proof, &public_inputs);
        assert_eq!(
            result,
            Err(VerifyError::PairingMismatch),
            "tampered {label} should reject with PairingMismatch",
        );
    }

    /// Tampering `c_old` (PI[0]) rejects. Mirrors the off-chain
    /// `synthesize_rejects_tampered_c_old`.
    #[test]
    fn rejects_tampered_update_c_old() {
        assert_rejects_tampered_update(0, "c_old");
    }

    /// Tampering `epoch_old` (PI[1]) rejects. Catches PI ordering bugs
    /// specific to the 3-PI update layout.
    #[test]
    fn rejects_tampered_update_epoch_old() {
        assert_rejects_tampered_update(1, "epoch_old");
    }

    /// Tampering `c_new` (PI[2]) rejects. Mirrors the off-chain
    /// `synthesize_rejects_tampered_c_new`.
    #[test]
    fn rejects_tampered_update_c_new() {
        assert_rejects_tampered_update(2, "c_new");
    }

    /// Tampering with the public commitment (input[0]) rejects with
    /// `PairingMismatch`. Catches a bug where public inputs aren't
    /// fed into the transcript correctly. Mirrors the off-chain
    /// `rejects_tampered_commitment` test, which pins the same mode.
    ///
    /// If a future refactor intentionally adds an earlier sanity gate
    /// (e.g. PI canonicality pre-check), update both this test and
    /// the off-chain reference together.
    #[test]
    fn rejects_tampered_commitment() {
        let env = Env::default();
        let parsed_vk = parse_vk_bytes(FIXTURE_VK_D5).expect("parse vk");
        let parsed_proof = parse_proof_bytes(FIXTURE_PROOF_D5).expect("parse proof");
        let mut public_inputs = split_pi(FIXTURE_PI_D5);
        // Flip the LSB (BE) of the commitment — a different valid Fr.
        public_inputs[0][FR_LEN - 1] ^= 0x01;
        let result = verify(&env, &parsed_vk, FIXTURE_SRS_G2, &parsed_proof, &public_inputs);
        assert_eq!(
            result,
            Err(VerifyError::PairingMismatch),
            "tampered commitment should reject with PairingMismatch",
        );
    }

    /// Tampering with the epoch (input[1]) also rejects with
    /// `PairingMismatch`. Catches public-input ordering bugs.
    #[test]
    fn rejects_tampered_epoch() {
        let env = Env::default();
        let parsed_vk = parse_vk_bytes(FIXTURE_VK_D5).expect("parse vk");
        let parsed_proof = parse_proof_bytes(FIXTURE_PROOF_D5).expect("parse proof");
        let mut public_inputs = split_pi(FIXTURE_PI_D5);
        public_inputs[1][FR_LEN - 1] ^= 0x01;
        let result = verify(&env, &parsed_vk, FIXTURE_SRS_G2, &parsed_proof, &public_inputs);
        assert_eq!(result, Err(VerifyError::PairingMismatch));
    }
}
