//! Soroban-side port of `sep-xxxx-circuits::circuit::plonk::verifier_lin_poly`
//! (PR #181). Computes the constant term `r_0` of the linearisation
//! polynomial used by the TurboPlonk verifier.
//!
//! Mirrors jf-plonk's `Verifier::compute_lin_poly_constant_term`
//! for the no-Plookup, single-instance case our membership circuits
//! use:
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
//! - `σ_i = wire_sigma_evals[i]` (4 entries; the product runs over
//!   `i = 0, 1, …, n − 2`),
//! - `z(ζ·g) = perm_next_eval`,
//! - `α, β, γ` are challenges, `L_0(ζ)` and `PI(ζ)` come from
//!   [`super::verifier_polys`].
//!
//! Each arithmetic op is a Soroban host call (`fr_add`, `fr_sub`,
//! `fr_mul`); same algorithm as the off-chain reference, just with
//! `Fr::clone()` between consumes since Soroban `Fr` doesn't impl
//! `Copy`.

use soroban_sdk::crypto::bls12_381::Fr;

use crate::proof_format::{NUM_WIRE_SIGMA_EVALS, NUM_WIRE_TYPES};

/// Compute the constant term `r_0` of the linearisation polynomial.
///
/// Caller is responsible for converting byte-form inputs (challenges,
/// proof evaluations, public inputs) into the `Fr` arguments and for
/// precomputing `pi_eval` and `lagrange_1_eval` via
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
        "wires_evals length must match NUM_WIRE_TYPES"
    );
    assert_eq!(
        wire_sigma_evals.len(),
        NUM_WIRE_SIGMA_EVALS,
        "wire_sigma_evals length must match NUM_WIRE_SIGMA_EVALS"
    );

    // α² via direct multiplication (cheaper than `pow(2)`'s host call
    // dispatch + repeated-squaring overhead for tiny exponents).
    let alpha_squared = alpha.clone() * alpha.clone();

    let n_minus_1 = NUM_WIRE_TYPES - 1;
    let last_w_eval = wires_evals[n_minus_1].clone();
    let first_w_evals = &wires_evals[..n_minus_1];

    // permutation_term = α · perm_next · (γ + w_{n-1})
    //                  · Π_{i=0..n-1} (γ + w_i + β · σ_i)
    //
    // jf-plonk fold: seed = α · perm_next · (γ + last_w_eval); for
    // each (w_i, σ_i) in zip(first_w_evals, wire_sigma_evals):
    //     acc *= (γ + w_i + β · σ_i)
    let mut permutation_term =
        alpha.clone() * perm_next_eval * (gamma.clone() + last_w_eval);
    for (w_eval, sigma_eval) in first_w_evals.iter().zip(wire_sigma_evals.iter()) {
        let factor =
            gamma.clone() + w_eval.clone() + beta.clone() * sigma_eval.clone();
        permutation_term = permutation_term * factor;
    }

    pi_eval - alpha_squared * lagrange_1_eval - permutation_term
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{BytesN, Env};

    /// Helper: build an Fr from a u64 (BE-encoded into a 32-byte
    /// slot's tail).  Same as `verifier_polys::fr_from_u64`'s logic
    /// but local because we don't want a public API on that helper.
    fn fr(env: &Env, value: u64) -> Fr {
        let mut bytes = [0u8; 32];
        bytes[24..32].copy_from_slice(&value.to_be_bytes());
        Fr::from_bytes(BytesN::from_array(env, &bytes))
    }

    fn fr_one(env: &Env) -> Fr {
        fr(env, 1)
    }

    fn fr_zero(env: &Env) -> Fr {
        fr(env, 0)
    }

    /// Inline reference transcribed from jf-plonk's source for the
    /// no-Plookup, single-instance case. Used as oracle so a typo
    /// in the port (wrong index, wrong sign, missing perm_next)
    /// shows up.
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
        let mut acc = alpha.clone() * perm_next * (gamma.clone() + w_evals[4].clone());
        for i in 0..4 {
            acc = acc
                * (gamma.clone()
                    + w_evals[i].clone()
                    + beta.clone() * sigma_evals[i].clone());
        }
        let alpha_squared = alpha.clone() * alpha.clone();
        pi_eval - alpha_squared * l_0 - acc
    }

    /// Random-ish Fr inputs (drawn from u64 seeds) — port matches
    /// the inline reference for 5 reps. Catches typos in the loop,
    /// the sign of the terms, or the alpha-squared multiplier.
    #[test]
    fn matches_inline_reference_for_diverse_inputs() {
        let env = Env::default();
        // Five (alpha, beta, gamma, pi_eval, l_0, perm_next) seed sets,
        // each with distinct w_evals and sigma_evals slot patterns.
        let seeds: &[(u64, u64, u64, u64, u64, u64)] = &[
            (1, 2, 3, 100, 7, 99),
            (11, 13, 17, 1234, 555, 9001),
            (1_000_000, 7, 31, 8675309, 42, 24),
            (97, 89, 83, 1, 1, 1),
            (0xDEADBEEF, 0xCAFEBABE, 0xFEEDFACE, 0x12345678, 0x9ABCDEF0, 0x55555555),
        ];

        for (i, &(a, b, g, pe, l, pn)) in seeds.iter().enumerate() {
            let alpha = fr(&env, a);
            let beta = fr(&env, b);
            let gamma = fr(&env, g);
            let pi_eval = fr(&env, pe);
            let l_0 = fr(&env, l);
            let perm_next = fr(&env, pn);
            let w_evals = [
                fr(&env, 100 + i as u64),
                fr(&env, 200 + i as u64),
                fr(&env, 300 + i as u64),
                fr(&env, 400 + i as u64),
                fr(&env, 500 + i as u64),
            ];
            let sigma_evals = [
                fr(&env, 600 + i as u64),
                fr(&env, 700 + i as u64),
                fr(&env, 800 + i as u64),
                fr(&env, 900 + i as u64),
            ];

            let ours = compute_lin_poly_constant_term(
                alpha.clone(),
                beta.clone(),
                gamma.clone(),
                pi_eval.clone(),
                l_0.clone(),
                &w_evals,
                &sigma_evals,
                perm_next.clone(),
            );
            let theirs = reference_lin_poly_constant(
                alpha,
                beta,
                gamma,
                pi_eval,
                l_0,
                &w_evals,
                &sigma_evals,
                perm_next,
            );
            assert_eq!(
                ours.to_bytes().to_array(),
                theirs.to_bytes().to_array(),
                "formula mismatch at seed-set #{i}",
            );
        }
    }

    /// Symbolic spot-check: with α = β = γ = 0, both the L_0 term
    /// and the permutation term vanish (α=0 kills both: 0² = 0
    /// and perm_term seed is α·perm_next·(γ + w_4) = 0). So
    /// `r_0 = pi_eval`. Catches a bug where L_0 is multiplied by
    /// `α^0` (= 1) by mistake — α=0 cannot distinguish α² from α³
    /// (both vanish). The positive-α test below pins the exponent.
    #[test]
    fn collapses_to_pi_eval_when_alpha_beta_gamma_are_zero() {
        let env = Env::default();
        let pi_eval = fr(&env, 42);
        let l_0 = fr(&env, 7);
        let w_evals = [
            fr(&env, 1),
            fr(&env, 2),
            fr(&env, 3),
            fr(&env, 4),
            fr(&env, 5),
        ];
        let sigma_evals = [fr(&env, 10), fr(&env, 11), fr(&env, 12), fr(&env, 13)];
        let perm_next = fr(&env, 99);

        let result = compute_lin_poly_constant_term(
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
            pi_eval.clone(),
            l_0,
            &w_evals,
            &sigma_evals,
            perm_next,
        );
        assert_eq!(
            result.to_bytes().to_array(),
            pi_eval.to_bytes().to_array(),
            "α=β=γ=0 should collapse r_0 to pi_eval",
        );
    }

    /// Positive-α regression: pin the exact α-power on the L_0
    /// term. With α = 2, β = γ = 0, perm_next = 0 the formula
    /// reduces to `r_0 = pi_eval − α²·l_0`. If the implementation
    /// ever drifted to α¹ or α³, the hand-computed value would not
    /// match.
    #[test]
    fn pins_alpha_squared_on_l_0_term() {
        let env = Env::default();
        let alpha = fr(&env, 2);
        let pi_eval = fr(&env, 100);
        let l_0 = fr(&env, 7);
        let w_evals = [
            fr_one(&env),
            fr_one(&env),
            fr_one(&env),
            fr_one(&env),
            fr_one(&env),
        ];
        let sigma_evals = [
            fr_one(&env),
            fr_one(&env),
            fr_one(&env),
            fr_one(&env),
        ];
        let perm_next = fr_zero(&env);

        let result = compute_lin_poly_constant_term(
            alpha.clone(),
            fr_zero(&env),
            fr_zero(&env),
            pi_eval.clone(),
            l_0.clone(),
            &w_evals,
            &sigma_evals,
            perm_next,
        );

        // expected = pi_eval − α²·l_0 = 100 − 4·7 = 72
        // (perm_term seed is α·perm_next·… = α·0·… = 0)
        let alpha_squared = alpha.clone() * alpha.clone();
        let expected = pi_eval.clone() - alpha_squared * l_0.clone();
        assert_eq!(
            result.to_bytes().to_array(),
            expected.to_bytes().to_array(),
        );

        // Sanity: α¹ and α³ would give different values.
        let alpha_first = alpha.clone();
        let wrong_alpha_one = pi_eval.clone() - alpha_first * l_0.clone();
        assert_ne!(
            result.to_bytes().to_array(),
            wrong_alpha_one.to_bytes().to_array(),
            "α¹ would mis-match",
        );
        let alpha_cubed = alpha.clone() * alpha.clone() * alpha;
        let wrong_alpha_cubed = pi_eval - alpha_cubed * l_0;
        assert_ne!(
            result.to_bytes().to_array(),
            wrong_alpha_cubed.to_bytes().to_array(),
            "α³ would mis-match",
        );
    }

    /// Hand-computed value with α=1, β=γ=0, σ=[1,1,1,1]:
    /// permutation_term = 1·perm_next·w_4·Π(w_i)
    /// r_0 = pi_eval − l_0 − perm_next·w_4·w_0·w_1·w_2·w_3.
    /// Catches bugs in the fold seed or product order.
    #[test]
    fn matches_hand_computed_value_with_alpha_one_beta_zero_gamma_zero() {
        let env = Env::default();
        let w_evals = [
            fr(&env, 2),
            fr(&env, 3),
            fr(&env, 5),
            fr(&env, 7),
            fr(&env, 11),
        ];
        let sigma_evals = [fr_one(&env), fr_one(&env), fr_one(&env), fr_one(&env)];
        let perm_next = fr(&env, 13);
        let pi_eval = fr(&env, 100);
        let l_0 = fr(&env, 99);

        let result = compute_lin_poly_constant_term(
            fr_one(&env),
            fr_zero(&env),
            fr_zero(&env),
            pi_eval.clone(),
            l_0.clone(),
            &w_evals,
            &sigma_evals,
            perm_next,
        );

        // expected = 100 − 99 − 13·11·2·3·5·7 = 1 − 30030
        let permutation = fr(&env, 13)
            * fr(&env, 11)
            * fr(&env, 2)
            * fr(&env, 3)
            * fr(&env, 5)
            * fr(&env, 7);
        let expected = pi_eval - l_0 - permutation;
        assert_eq!(
            result.to_bytes().to_array(),
            expected.to_bytes().to_array(),
        );
    }

    /// Length guard — `wires_evals.len() != NUM_WIRE_TYPES` panics
    /// loudly in dev/release.
    #[test]
    #[should_panic(expected = "wires_evals length must match NUM_WIRE_TYPES")]
    fn rejects_wrong_wires_evals_length() {
        let env = Env::default();
        let too_short = [
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
        ];
        let sigmas_ok = [fr_zero(&env), fr_zero(&env), fr_zero(&env), fr_zero(&env)];
        let _ = compute_lin_poly_constant_term(
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
            &too_short,
            &sigmas_ok,
            fr_zero(&env),
        );
    }

    /// Length guard for `wire_sigma_evals`.
    #[test]
    #[should_panic(expected = "wire_sigma_evals length must match NUM_WIRE_SIGMA_EVALS")]
    fn rejects_wrong_wire_sigma_evals_length() {
        let env = Env::default();
        let wires_ok = [
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
        ];
        let too_long = [
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
        ];
        let _ = compute_lin_poly_constant_term(
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
            fr_zero(&env),
            &wires_ok,
            &too_long,
            fr_zero(&env),
        );
    }
}
