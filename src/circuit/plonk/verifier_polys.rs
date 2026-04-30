//! Polynomial-evaluation utilities for the verifier core.
//!
//! Phase C.2 verifier needs three Fr-arithmetic primitives the
//! TurboPlonk verification equation depends on:
//!
//! - **Vanishing polynomial** evaluation: `Z_H(ζ) = ζ^n − 1` where
//!   `n` is the evaluation domain size.
//! - **First and last Lagrange coefficients** at ζ:
//!   `L_0(ζ) = Z_H(ζ) / (n · (ζ − 1))` and
//!   `L_{n−1}(ζ) = Z_H(ζ) · g^{−1} / (n · (ζ − g^{−1}))`,
//!   used by `compute_lin_poly_constant_term`.
//! - **Public-input polynomial** evaluation:
//!   `PI(ζ) = Σᵢ Lᵢ(ζ) · publicᵢ`,
//!   used by `compute_lin_poly_constant_term` for the PI term.
//!
//! All formulas are for jf-plonk's non-coset `Radix2EvaluationDomain`
//! (offset = 1) — the only domain shape our preprocessing produces.
//!
//! ## Soroban portability
//!
//! API takes/returns `Fr` (arkworks) for now since these are internal
//! helpers consumed by the verifier core. The Soroban contract port
//! re-implements the same formulas using whatever Fr arithmetic the
//! host provides (`env.crypto().bls12_381()` Fr ops or u256 emulation
//! over the field modulus). The byte-equivalence anchor for that
//! re-implementation is the test oracle here, run against
//! `ark_poly::Radix2EvaluationDomain`.

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use ark_ff_v05::{FftField, Field, One, Zero};

/// Bundle of an evaluation-domain's tier-dependent constants.
///
/// `for_size(n)` derives the primitive `n`-th root of unity (the
/// domain generator) and its inverse from BLS12-381 Fr's two-adicity.
/// Domain size must be a power of two ≤ `2^Fr::TWO_ADICITY = 2^32`.
#[derive(Clone, Copy, Debug)]
pub struct DomainParams {
    /// Domain size. Power of two.
    pub size: u64,
    /// Primitive `size`-th root of unity in Fr.
    pub group_gen: Fr,
    /// `group_gen.inverse()`.
    pub group_gen_inv: Fr,
}

impl DomainParams {
    /// Build the parameters for a domain of size `domain_size`.
    /// Panics if `domain_size` is not a power of two within Fr's
    /// two-adicity bound.
    pub fn for_size(domain_size: u64) -> Self {
        assert!(
            domain_size.is_power_of_two(),
            "domain_size {domain_size} is not a power of two"
        );
        let log_size = domain_size.trailing_zeros();
        assert!(
            log_size <= <Fr as FftField>::TWO_ADICITY,
            "domain_size 2^{log_size} exceeds Fr::TWO_ADICITY = 2^{}",
            <Fr as FftField>::TWO_ADICITY
        );

        // BLS12-381 Fr: `r − 1 = 2^TWO_ADICITY · odd`. Start with the
        // primitive 2^TWO_ADICITY-th root of unity, then square down
        // to a primitive 2^log_size-th root.
        let mut omega = <Fr as FftField>::TWO_ADIC_ROOT_OF_UNITY;
        for _ in log_size..<Fr as FftField>::TWO_ADICITY {
            omega = omega.square();
        }

        let group_gen_inv = omega
            .inverse()
            .expect("primitive root of unity is non-zero");

        Self {
            size: domain_size,
            group_gen: omega,
            group_gen_inv,
        }
    }
}

/// Evaluate the vanishing polynomial of a non-coset radix-2 domain at
/// ζ: `Z_H(ζ) = ζ^n − 1`.
pub fn evaluate_vanishing_poly(zeta: Fr, params: &DomainParams) -> Fr {
    zeta.pow([params.size]) - Fr::one()
}

/// Compute `(L_0(ζ), L_{n−1}(ζ))` for a non-coset radix-2 domain of
/// size `n`. Caller passes the precomputed `vanishing_eval = Z_H(ζ)`
/// to avoid recomputation when both quantities are needed (which is
/// the common case in the verifier).
///
/// Mirrors the non-coset branch of jf-plonk's
/// `LagrangeCoeffs::first_and_last_lagrange_coeffs`
/// (`plonk/src/lagrange.rs:89-115`):
///
/// - ζ = 1: returns (1, 0)
/// - ζ = g^{−1}: returns (0, 1)
/// - ζ at any other domain point (`Z_H(ζ) = 0`): returns (0, 0)
/// - else: `L_0 = Z_H(ζ) / (n·(ζ−1))`,
///        `L_{n−1} = Z_H(ζ)·g^{−1} / (n·(ζ−g^{−1}))`.
pub fn first_and_last_lagrange_coeffs(
    zeta: Fr,
    vanishing_eval: Fr,
    params: &DomainParams,
) -> (Fr, Fr) {
    let one = Fr::one();
    if zeta == one {
        return (one, Fr::zero());
    }
    if zeta == params.group_gen_inv {
        return (Fr::zero(), one);
    }
    if vanishing_eval.is_zero() {
        // ζ is some other domain point (g^i for i ∉ {0, n−1}).
        return (Fr::zero(), Fr::zero());
    }
    let n_fr = Fr::from(params.size);
    let l_0 = vanishing_eval / (n_fr * (zeta - one));
    let l_n_minus_1 =
        vanishing_eval * params.group_gen_inv / (n_fr * (zeta - params.group_gen_inv));
    (l_0, l_n_minus_1)
}

/// Evaluate the public-input polynomial at ζ:
/// `PI(ζ) = Σᵢ Lᵢ(ζ)·publicᵢ`, for `i ∈ 0..public_inputs.len()`.
///
/// For a non-coset radix-2 domain, `Lᵢ(ζ) = Z_H(ζ)·gⁱ / (n·(ζ−gⁱ))`.
/// We compute these directly (no batch inversion) since the public-
/// input count is small (membership circuit has 2 public inputs).
///
/// Special case: if ζ equals a domain point `gⁱ` for some `i` in range,
/// `Lᵢ(ζ) = 1` and all other `Lⱼ(ζ) = 0` for `j ≠ i`, so
/// `PI(ζ) = publicᵢ`. We detect this without dividing by zero.
///
/// Mirrors jf-plonk's `evaluate_pi_poly` for the `is_merged = false`
/// case (`plonk/src/proof_system/verifier.rs:883-915`).
pub fn evaluate_pi_poly(
    public_inputs: &[Fr],
    zeta: Fr,
    vanishing_eval: Fr,
    params: &DomainParams,
) -> Fr {
    if public_inputs.is_empty() {
        return Fr::zero();
    }

    let n_fr = Fr::from(params.size);
    let mut sum = Fr::zero();
    let mut g_i = Fr::one(); // g^0 = 1; updated to g^i each iteration.

    for &pi in public_inputs.iter() {
        if zeta == g_i {
            // ζ is exactly the i-th domain point: Lᵢ(ζ) = 1, others = 0.
            return pi;
        }
        // Lᵢ(ζ) = Z_H(ζ) · gⁱ / (n · (ζ − gⁱ))
        let l_i = vanishing_eval * g_i / (n_fr * (zeta - g_i));
        sum += l_i * pi;
        g_i *= params.group_gen;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_poly_v05::{EvaluationDomain, Radix2EvaluationDomain};
    use ark_std_v05::UniformRand;
    use rand_chacha::rand_core::SeedableRng;

    /// Generate a deterministic Fr for tests.
    fn fr(seed: u64) -> Fr {
        Fr::from(seed)
    }

    /// Sizes our circuits use, plus a few smaller for breadth.
    const TEST_DOMAIN_SIZES: &[u64] = &[8, 16, 32, 8192, 16384, 32768];

    /// `DomainParams::for_size` matches `Radix2EvaluationDomain::group_gen`.
    #[test]
    fn domain_params_match_arkworks_radix2_generator() {
        for &n in TEST_DOMAIN_SIZES {
            let ours = DomainParams::for_size(n);
            let theirs = Radix2EvaluationDomain::<Fr>::new(n as usize)
                .expect("ark_poly domain");

            assert_eq!(ours.size, n, "size mismatch n={n}");
            assert_eq!(ours.group_gen, theirs.group_gen, "group_gen mismatch n={n}");
            assert_eq!(
                ours.group_gen_inv, theirs.group_gen_inv,
                "group_gen_inv mismatch n={n}"
            );
            // Sanity: g^n = 1.
            assert_eq!(
                ours.group_gen.pow([n]),
                Fr::one(),
                "group_gen is not n-th root of unity for n={n}"
            );
        }
    }

    /// `evaluate_vanishing_poly` matches `Radix2EvaluationDomain::evaluate_vanishing_polynomial`
    /// for both random ζ and ζ at domain points.
    #[test]
    fn evaluate_vanishing_poly_matches_arkworks() {
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([7u8; 32]);

        for &n in TEST_DOMAIN_SIZES {
            let params = DomainParams::for_size(n);
            let domain = Radix2EvaluationDomain::<Fr>::new(n as usize).unwrap();

            // Random ζ
            for _ in 0..5 {
                let zeta = Fr::rand(&mut rng);
                assert_eq!(
                    evaluate_vanishing_poly(zeta, &params),
                    domain.evaluate_vanishing_polynomial(zeta),
                    "n={n} random ζ mismatch"
                );
            }

            // ζ = 1 → Z_H(1) = 0
            assert_eq!(
                evaluate_vanishing_poly(Fr::one(), &params),
                Fr::zero(),
                "n={n} ζ=1 should vanish"
            );

            // ζ = g (a domain point) → Z_H(g) = 0
            assert_eq!(
                evaluate_vanishing_poly(params.group_gen, &params),
                Fr::zero(),
                "n={n} ζ=g should vanish"
            );
        }
    }

    /// `first_and_last_lagrange_coeffs` matches arkworks for random ζ.
    #[test]
    fn first_and_last_lagrange_coeffs_match_arkworks_for_random_zeta() {
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([11u8; 32]);

        for &n in TEST_DOMAIN_SIZES {
            let params = DomainParams::for_size(n);
            let domain = Radix2EvaluationDomain::<Fr>::new(n as usize).unwrap();

            for _ in 0..5 {
                let zeta = Fr::rand(&mut rng);
                let z_h = evaluate_vanishing_poly(zeta, &params);
                let (l_0, l_n_minus_1) =
                    first_and_last_lagrange_coeffs(zeta, z_h, &params);

                let coeffs = domain.evaluate_all_lagrange_coefficients(zeta);
                assert_eq!(l_0, coeffs[0], "n={n} L_0 mismatch");
                assert_eq!(
                    l_n_minus_1,
                    coeffs[n as usize - 1],
                    "n={n} L_{{n-1}} mismatch"
                );
            }
        }
    }

    /// Edge cases: ζ = 1, ζ = g, ζ = g^{n-1}, ζ at other domain points.
    #[test]
    fn first_and_last_lagrange_coeffs_handle_domain_points() {
        let n = 16u64;
        let params = DomainParams::for_size(n);
        let domain = Radix2EvaluationDomain::<Fr>::new(n as usize).unwrap();

        // ζ = 1: L_0 = 1, L_{n-1} = 0
        let z_h_at_1 = evaluate_vanishing_poly(Fr::one(), &params);
        let (l_0, l_n) = first_and_last_lagrange_coeffs(Fr::one(), z_h_at_1, &params);
        assert_eq!(l_0, Fr::one());
        assert_eq!(l_n, Fr::zero());

        // ζ = g^{n-1} = g_inv: L_0 = 0, L_{n-1} = 1
        let zeta = params.group_gen_inv;
        let z_h = evaluate_vanishing_poly(zeta, &params);
        let (l_0, l_n) = first_and_last_lagrange_coeffs(zeta, z_h, &params);
        assert_eq!(l_0, Fr::zero());
        assert_eq!(l_n, Fr::one());

        // ζ = g (interior domain point): both should be 0
        let zeta = params.group_gen;
        let z_h = evaluate_vanishing_poly(zeta, &params);
        let (l_0, l_n) = first_and_last_lagrange_coeffs(zeta, z_h, &params);
        assert_eq!(l_0, Fr::zero(), "L_0 at interior domain point");
        assert_eq!(l_n, Fr::zero(), "L_{{n-1}} at interior domain point");
        // Cross-check: arkworks agrees.
        let coeffs = domain.evaluate_all_lagrange_coefficients(zeta);
        assert_eq!(coeffs[0], Fr::zero());
        assert_eq!(coeffs[n as usize - 1], Fr::zero());
    }

    /// `evaluate_pi_poly` matches direct computation via arkworks
    /// `evaluate_all_lagrange_coefficients` for random ζ and a small
    /// public-input vector.
    #[test]
    fn evaluate_pi_poly_matches_arkworks_for_random_zeta() {
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([19u8; 32]);

        for &n in TEST_DOMAIN_SIZES {
            let params = DomainParams::for_size(n);
            let domain = Radix2EvaluationDomain::<Fr>::new(n as usize).unwrap();

            // Test with the membership circuit's 2 public inputs and
            // also with 1, 4 inputs for breadth.
            for &n_pi in &[1usize, 2, 4] {
                let public_inputs: Vec<Fr> =
                    (0..n_pi).map(|i| fr(100 + i as u64)).collect();

                for _ in 0..3 {
                    let zeta = Fr::rand(&mut rng);
                    let z_h = evaluate_vanishing_poly(zeta, &params);
                    let ours = evaluate_pi_poly(&public_inputs, zeta, z_h, &params);

                    let coeffs = domain.evaluate_all_lagrange_coefficients(zeta);
                    let expected: Fr = public_inputs
                        .iter()
                        .zip(coeffs.iter())
                        .map(|(&pi, &l)| pi * l)
                        .sum();

                    assert_eq!(
                        ours, expected,
                        "n={n} n_pi={n_pi} PI(ζ) mismatch"
                    );
                }
            }
        }
    }

    /// PI(ζ) at a domain point: when ζ = g^i for some i in range,
    /// PI(ζ) = public_inputs[i]. Catches the early-return special case.
    #[test]
    fn evaluate_pi_poly_returns_public_input_at_domain_point() {
        let n = 16u64;
        let params = DomainParams::for_size(n);
        let public_inputs: Vec<Fr> = (0..3).map(|i| fr(50 + i as u64)).collect();

        // ζ = g^0 = 1 → PI = public_inputs[0]
        let zeta = Fr::one();
        let z_h = evaluate_vanishing_poly(zeta, &params);
        assert_eq!(
            evaluate_pi_poly(&public_inputs, zeta, z_h, &params),
            public_inputs[0],
        );

        // ζ = g^2 → PI = public_inputs[2]
        let zeta = params.group_gen.square();
        let z_h = evaluate_vanishing_poly(zeta, &params);
        assert_eq!(
            evaluate_pi_poly(&public_inputs, zeta, z_h, &params),
            public_inputs[2],
        );
    }

    /// Empty public-input vector → PI = 0 (no terms).
    #[test]
    fn evaluate_pi_poly_empty_returns_zero() {
        let params = DomainParams::for_size(16);
        let zeta = fr(7);
        let z_h = evaluate_vanishing_poly(zeta, &params);
        assert_eq!(evaluate_pi_poly(&[], zeta, z_h, &params), Fr::zero());
    }
}
