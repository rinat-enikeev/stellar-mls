//! `compute_lin_poly_constant_term` port — the constant term `r_0`
//! of the linearisation polynomial used by the TurboPlonk verifier.
//!
//! Mirrors jf-plonk's `Verifier::compute_lin_poly_constant_term`
//! (`plonk/src/proof_system/verifier.rs:358-432`) for the no-Plookup,
//! single-instance case our membership circuits use. With those
//! simplifications the formula collapses to:
//!
//! ```text
//!   r_0 = PI(ζ)
//!       − α²·L_0(ζ)
//!       − α·z(ζ·g)·(γ + w_{n-1})·Π_{i=0}^{n-2}(γ + w_i + β·σ_i)
//! ```
//!
//! where:
//! - `n = NUM_WIRE_TYPES = GATE_WIDTH + 1 = 5`,
//! - `w_i = wires_evals[i]` (5 entries from the proof),
//! - `σ_i = wire_sigma_evals[i]` (4 entries — the last sigma is
//!   omitted as a verifier optimisation in jf-plonk; the product
//!   runs over `i = 0, 1, …, n − 2` — exactly 4 iterations for
//!   our `n = 5`),
//! - `z(ζ·g) = perm_next_eval`,
//! - `α, β, γ` are challenges, `L_0(ζ)` and `PI(ζ)` come from
//!   [`super::verifier_polys`].
//!
//! The product expression matches jf-plonk's fold:
//!
//! ```text
//!   acc = α · perm_next · (γ + w_{n-1});
//!   for i in 0..(n-1):    // Rust half-open range, 4 iterations for n=5
//!       acc *= (γ + w_i + β · σ_i);
//! ```
//!
//! Soroban portability: takes/returns arkworks `Fr`. The contract
//! port re-implements the same formula using whatever Fr ops the
//! host provides; the test below pins the formula against jf-plonk's
//! source by replicating it inline.

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use ark_ff_v05::Field;

use crate::circuit::plonk::proof_format::{NUM_WIRE_SIGMA_EVALS, NUM_WIRE_TYPES};
use crate::circuit::plonk::vk_format::NUM_SIGMA_COMMS;

// Compile-time guard: the proof's wire count, the VK's sigma-commitment
// count, and the number of sigma evaluations all line up the way the
// formula expects (5 wires, 5 sigma comms in VK, 4 sigma evals in proof).
const _: () = assert!(NUM_WIRE_TYPES == 5);
const _: () = assert!(NUM_SIGMA_COMMS == 5);
const _: () = assert!(NUM_WIRE_SIGMA_EVALS == 4);

/// Compute the constant term `r_0` of the linearisation polynomial.
///
/// Caller is responsible for converting byte-form inputs (challenges
/// from [`super::verifier_challenges`], proof evaluations from
/// `ParsedProof`, public inputs in BE) into the `Fr` arguments and
/// for precomputing `pi_eval` and `lagrange_1_eval` via
/// [`super::verifier_polys`].
///
/// Single-instance, no-Plookup. Asserts on slice lengths so a
/// mis-shaped proof eval set fails loudly rather than silently
/// producing wrong arithmetic.
pub fn compute_lin_poly_constant_term(
    alpha: Fr,
    beta: Fr,
    gamma: Fr,
    pi_eval: Fr,
    lagrange_1_eval: Fr,
    wires_evals: &[Fr],
    wire_sigma_evals: &[Fr],
    perm_next_eval: Fr,
) -> Fr {
    assert_eq!(
        wires_evals.len(),
        NUM_WIRE_TYPES,
        "wires_evals length must match NUM_WIRE_TYPES = {NUM_WIRE_TYPES}"
    );
    assert_eq!(
        wire_sigma_evals.len(),
        NUM_WIRE_SIGMA_EVALS,
        "wire_sigma_evals length must match NUM_WIRE_SIGMA_EVALS = {NUM_WIRE_SIGMA_EVALS}"
    );

    let alpha_squared = alpha.square();
    let n_minus_1 = NUM_WIRE_TYPES - 1;
    let last_w_eval = wires_evals[n_minus_1];
    let first_w_evals = &wires_evals[..n_minus_1];

    // acc = α · perm_next_eval · (γ + w_{n-1})
    // for (w_i, σ_i): acc *= (γ + w_i + β · σ_i)
    let permutation_term = first_w_evals
        .iter()
        .zip(wire_sigma_evals.iter())
        .fold(
            alpha * perm_next_eval * (gamma + last_w_eval),
            |acc, (w_eval, sigma_eval)| {
                acc * (gamma + w_eval + beta * sigma_eval)
            },
        );

    pi_eval - alpha_squared * lagrange_1_eval - permutation_term
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bls12_381_v05::{Bls12_381, Fr};
    use ark_ff_v05::{BigInteger, One, PrimeField, Zero};
    use ark_serialize_v05::{CanonicalDeserialize, CanonicalSerialize};
    use ark_std_v05::UniformRand;
    use jf_plonk::proof_system::structs::VerifyingKey;
    use jf_relation::PlonkCircuit;
    use rand_chacha::rand_core::SeedableRng;

    use crate::circuit::plonk::baker::{bake_membership_vk, build_canonical_membership_witness};
    use crate::circuit::plonk::membership::synthesize_membership;
    use crate::circuit::plonk::proof_format::parse_proof_bytes;
    use crate::circuit::plonk::verifier_challenges::compute_challenges;
    use crate::circuit::plonk::verifier_polys::{
        evaluate_pi_poly, evaluate_vanishing_poly, first_and_last_lagrange_coeffs, DomainParams,
    };
    use crate::circuit::plonk::vk_format::{parse_vk_bytes, FR_LEN, G2_COMPRESSED_LEN};
    use crate::prover::plonk;

    /// Reference implementation of the formula, expanded inline from
    /// jf-plonk's `compute_lin_poly_constant_term` source — the
    /// no-Plookup, single-instance specialisation. Used as the
    /// in-test oracle for the formula port. The TRUE oracle for this
    /// quantity is the end-to-end verifier-accepts test that lands
    /// once the full verifier is wired up; until then, transcribing
    /// the formula here keeps the port honest against typos in either
    /// direction (port + test wouldn't both go wrong the same way
    /// without re-reading jf-plonk).
    fn reference_lin_poly_constant(
        alpha: Fr,
        beta: Fr,
        gamma: Fr,
        pi_eval: Fr,
        l_0: Fr,
        w_evals: &[Fr; 5],
        sigma_evals: &[Fr; 4],
        perm_next: Fr,
    ) -> Fr {
        let mut acc = alpha * perm_next * (gamma + w_evals[4]);
        for i in 0..4 {
            acc = acc * (gamma + w_evals[i] + beta * sigma_evals[i]);
        }
        pi_eval - alpha.square() * l_0 - acc
    }

    /// Generate a deterministic Fr.
    fn fr(seed: u64) -> Fr {
        Fr::from(seed)
    }

    /// Random inputs (no constraints from a real proof) — checks the
    /// formula transcription pure-symbolically against the in-test
    /// reference. Catches typos in the loop, the sign of the terms,
    /// or the alpha-squared multiplier.
    #[test]
    fn matches_inline_reference_for_random_inputs() {
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([3u8; 32]);
        for _ in 0..20 {
            let alpha = Fr::rand(&mut rng);
            let beta = Fr::rand(&mut rng);
            let gamma = Fr::rand(&mut rng);
            let pi_eval = Fr::rand(&mut rng);
            let l_0 = Fr::rand(&mut rng);
            let w_evals = [
                Fr::rand(&mut rng),
                Fr::rand(&mut rng),
                Fr::rand(&mut rng),
                Fr::rand(&mut rng),
                Fr::rand(&mut rng),
            ];
            let sigma_evals = [
                Fr::rand(&mut rng),
                Fr::rand(&mut rng),
                Fr::rand(&mut rng),
                Fr::rand(&mut rng),
            ];
            let perm_next = Fr::rand(&mut rng);

            let ours = compute_lin_poly_constant_term(
                alpha,
                beta,
                gamma,
                pi_eval,
                l_0,
                &w_evals,
                &sigma_evals,
                perm_next,
            );
            let theirs = reference_lin_poly_constant(
                alpha, beta, gamma, pi_eval, l_0, &w_evals, &sigma_evals, perm_next,
            );
            assert_eq!(ours, theirs, "formula mismatch on random inputs");
        }
    }

    /// Symbolic spot-check: with α = β = γ = 0 the entire L_0 and
    /// permutation terms vanish (α = 0 kills both: 0² = 0 = 0³ = 0
    /// and the permutation-term seed is α·perm_next·(γ + w_4) = 0).
    /// So `r_0 = pi_eval`. This only catches a bug where the L_0
    /// term was multiplied by `α^0` (= 1) by mistake — α=0 cannot
    /// distinguish α² from α³. The positive-α regression below
    /// catches the wrong-power case.
    #[test]
    fn collapses_to_pi_eval_when_alpha_beta_gamma_are_zero() {
        let pi_eval = fr(42);
        let l_0 = fr(7);
        let w_evals = [fr(1), fr(2), fr(3), fr(4), fr(5)];
        let sigma_evals = [fr(10), fr(11), fr(12), fr(13)];
        let perm_next = fr(99);

        let result = compute_lin_poly_constant_term(
            Fr::zero(),
            Fr::zero(),
            Fr::zero(),
            pi_eval,
            l_0,
            &w_evals,
            &sigma_evals,
            perm_next,
        );

        // permutation_term = 0·perm_next·(0 + w_4)·Π(...) = 0
        // r_0 = pi_eval - 0²·l_0 - 0 = pi_eval
        assert_eq!(result, pi_eval);
    }

    /// Positive-α regression: pin the exact α-power on the L_0
    /// term. With α = 2, β = γ = 0, perm_next = 0, the formula
    /// reduces to `r_0 = pi_eval − α²·l_0`. If the implementation
    /// ever drifted to α¹ or α³ on the L_0 term, the hand-computed
    /// value would not match.
    #[test]
    fn pins_alpha_squared_on_l_0_term() {
        let alpha = fr(2);
        let beta = Fr::zero();
        let gamma = Fr::zero();
        let pi_eval = fr(100);
        let l_0 = fr(7);
        let w_evals = [fr(1); 5];
        let sigma_evals = [fr(1); 4];
        let perm_next = Fr::zero();

        let result = compute_lin_poly_constant_term(
            alpha, beta, gamma, pi_eval, l_0, &w_evals, &sigma_evals, perm_next,
        );

        // permutation_term = 2·0·(0 + w_4)·… = 0  (perm_next = 0 zeroes the seed)
        // r_0 = pi_eval - α²·l_0 = 100 - 4·7 = 72
        let expected = pi_eval - alpha.square() * l_0;
        assert_eq!(result, expected);

        // Sanity: using α¹ instead would give pi_eval - α·l_0 = 100 - 14 = 86.
        // Using α³ instead would give pi_eval - α³·l_0 = 100 - 56 = 44.
        // Both are distinguishable from 72 — the test would catch either drift.
        assert_ne!(result, pi_eval - alpha * l_0, "α¹ would mis-match");
        assert_ne!(result, pi_eval - alpha.pow([3u64]) * l_0, "α³ would mis-match");
    }

    /// The wires_evals length guard panics on the wrong shape so a
    /// mis-sized proof eval set fails loudly. Pinned with
    /// `#[should_panic]` so a future relaxation (e.g. swap to `>=`)
    /// is caught.
    #[test]
    #[should_panic(expected = "wires_evals length must match NUM_WIRE_TYPES")]
    fn rejects_wrong_wires_evals_length() {
        let wires_too_short = [Fr::zero(); 4];
        let sigmas_ok = [Fr::zero(); 4];
        let _ = compute_lin_poly_constant_term(
            Fr::zero(),
            Fr::zero(),
            Fr::zero(),
            Fr::zero(),
            Fr::zero(),
            &wires_too_short,
            &sigmas_ok,
            Fr::zero(),
        );
    }

    /// The wire_sigma_evals length guard panics on the wrong shape.
    #[test]
    #[should_panic(expected = "wire_sigma_evals length must match NUM_WIRE_SIGMA_EVALS")]
    fn rejects_wrong_wire_sigma_evals_length() {
        let wires_ok = [Fr::zero(); 5];
        let sigmas_too_long = [Fr::zero(); 5];
        let _ = compute_lin_poly_constant_term(
            Fr::zero(),
            Fr::zero(),
            Fr::zero(),
            Fr::zero(),
            Fr::zero(),
            &wires_ok,
            &sigmas_too_long,
            Fr::zero(),
        );
    }

    /// Symbolic spot-check: with α = 1, β = 0, γ = 0, the
    /// permutation term is `1·perm_next·w_4·Π(w_i)` and `r_0
    /// = pi_eval - l_0 - perm_next·w_4·w_0·w_1·w_2·w_3`. Computed
    /// by hand; catches bugs in the fold seed or product order.
    #[test]
    fn matches_hand_computed_value_with_alpha_one_beta_zero_gamma_zero() {
        let w_evals = [fr(2), fr(3), fr(5), fr(7), fr(11)];
        let sigma_evals = [fr(1), fr(1), fr(1), fr(1)];
        let perm_next = fr(13);
        let pi_eval = fr(100);
        let l_0 = fr(99);

        let result = compute_lin_poly_constant_term(
            Fr::one(),
            Fr::zero(),
            Fr::zero(),
            pi_eval,
            l_0,
            &w_evals,
            &sigma_evals,
            perm_next,
        );

        // expected = pi_eval - 1·l_0 - 1·perm_next·w_4·w_0·w_1·w_2·w_3
        //          = 100 - 99 - 13·11·2·3·5·7
        //          = 1 - 30030
        let permutation = fr(13) * fr(11) * fr(2) * fr(3) * fr(5) * fr(7);
        let expected = pi_eval - l_0 - permutation;
        assert_eq!(result, expected);
    }

    /// End-to-end-ish: build a real proof, compute lin_poly_constant
    /// using my full verifier prerequisite chain (challenges + polys
    /// + this), and assert it equals what the in-test reference
    /// produces fed with the same Fr inputs. This sanity-checks the
    /// **integration** of [`compute_challenges`] →
    /// [`evaluate_pi_poly`] / [`first_and_last_lagrange_coeffs`] →
    /// [`compute_lin_poly_constant_term`].
    #[test]
    fn matches_reference_when_fed_real_proof_artifacts_for_all_tiers() {
        for &depth in &[5usize, 8, 11] {
            let vk_bytes = bake_membership_vk(depth).expect("bake vk");
            let oracle_vk: VerifyingKey<Bls12_381> =
                VerifyingKey::deserialize_uncompressed(&vk_bytes[..]).unwrap();

            let witness = build_canonical_membership_witness(depth);
            let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
            synthesize_membership(&mut circuit, &witness).unwrap();
            circuit.finalize_for_arithmetization().unwrap();
            let keys = plonk::preprocess(&circuit).unwrap();
            let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
            let oracle_proof = plonk::prove(&mut rng, &keys.pk, &circuit).unwrap();

            let mut proof_bytes = Vec::new();
            oracle_proof
                .serialize_uncompressed(&mut proof_bytes)
                .unwrap();

            let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
            let parsed_proof = parse_proof_bytes(&proof_bytes).expect("parse proof");

            let mut srs_g2_compressed = [0u8; G2_COMPRESSED_LEN];
            oracle_vk
                .open_key
                .powers_of_h[1]
                .serialize_compressed(&mut srs_g2_compressed[..])
                .unwrap();

            let public_inputs_fr =
                vec![witness.commitment, Fr::from(witness.epoch)];
            let public_inputs_be: Vec<[u8; FR_LEN]> = public_inputs_fr
                .iter()
                .map(|fr| {
                    let bytes = fr.into_bigint().to_bytes_be();
                    let mut arr = [0u8; FR_LEN];
                    arr.copy_from_slice(&bytes);
                    arr
                })
                .collect();

            let challenges = compute_challenges(
                &parsed_vk,
                &srs_g2_compressed,
                &public_inputs_be,
                &parsed_proof,
            );

            // Reduce challenges to Fr.
            let alpha = Fr::from_be_bytes_mod_order(&challenges.alpha);
            let beta = Fr::from_be_bytes_mod_order(&challenges.beta);
            let gamma = Fr::from_be_bytes_mod_order(&challenges.gamma);
            let zeta = Fr::from_be_bytes_mod_order(&challenges.zeta);

            // Domain + polynomial evaluations at zeta.
            let params = DomainParams::for_size(parsed_vk.domain_size);
            let z_h = evaluate_vanishing_poly(zeta, &params);
            let (l_0, _l_n) = first_and_last_lagrange_coeffs(zeta, z_h, &params);
            let pi_eval = evaluate_pi_poly(&public_inputs_fr, zeta, z_h, &params);

            // Convert proof evaluations LE→Fr.
            let w_evals: [Fr; 5] = std::array::from_fn(|i| {
                Fr::deserialize_uncompressed(&parsed_proof.wires_evals[i][..]).unwrap()
            });
            let sigma_evals: [Fr; 4] = std::array::from_fn(|i| {
                Fr::deserialize_uncompressed(&parsed_proof.wire_sigma_evals[i][..])
                    .unwrap()
            });
            let perm_next =
                Fr::deserialize_uncompressed(&parsed_proof.perm_next_eval[..]).unwrap();

            let ours = compute_lin_poly_constant_term(
                alpha, beta, gamma, pi_eval, l_0, &w_evals, &sigma_evals, perm_next,
            );
            let theirs = reference_lin_poly_constant(
                alpha, beta, gamma, pi_eval, l_0, &w_evals, &sigma_evals, perm_next,
            );

            assert_eq!(
                ours, theirs,
                "depth={depth} lin_poly_constant_term mismatch with reference",
            );

            // Sanity: result is non-zero (a real proof's lin-poly
            // constant is overwhelmingly unlikely to be zero by
            // accident; this catches a "result is always zero" bug).
            assert_ne!(
                ours,
                Fr::zero(),
                "depth={depth} lin_poly_constant_term unexpectedly zero",
            );
        }
    }
}
