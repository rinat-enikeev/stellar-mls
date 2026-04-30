//! Soroban-side port of `sep-xxxx-circuits::circuit::plonk::verifier_polys`
//! (PR #180).
//!
//! Three Fr-arithmetic primitives the verifier core depends on:
//!
//! - **Vanishing polynomial** at ζ: `Z_H(ζ) = ζⁿ − 1`.
//! - **First and last Lagrange coefficients** at ζ: `L_0(ζ)`, `L_{n-1}(ζ)`.
//! - **Public-input polynomial** at ζ: `PI(ζ) = Σ Lᵢ(ζ)·publicᵢ`.
//!
//! Plus `DomainParams::for_size(env, n)` which derives the primitive
//! n-th root of unity from BLS12-381 Fr's two-adicity (start at the
//! 2^32-nd root, square down to 2^log_size).
//!
//! All formulas mirror the off-chain reference verbatim. Underlying
//! arithmetic uses Soroban's Fr ops (`fr_add/sub/mul/pow/inv` via
//! the `Add/Sub/Mul` trait impls and `Fr::pow/inv` methods); the
//! algorithm is unchanged.
//!
//! ## Pinned constants
//!
//! `FR_TWO_ADICITY = 32` and `FR_TWO_ADIC_ROOT_OF_UNITY_BE` are
//! transcribed from arkworks-bls12-381 0.5's `<Fr as FftField>`
//! constants. Verified one-shot via the prover-side reference (and
//! oracle-tested per-domain-size via `domain_params_match_arkworks`
//! down on the Rust ref). If the Soroban Fr modulus is ever a
//! different curve, the constant must be re-derived.

use soroban_sdk::crypto::bls12_381::Fr;
use soroban_sdk::{BytesN, Env};

/// 2-adicity of BLS12-381 Fr's multiplicative group: r − 1 has 32
/// trailing zero bits in its prime factorisation.
pub const FR_TWO_ADICITY: u32 = 32;

/// Primitive 2^FR_TWO_ADICITY-th root of unity in BLS12-381 Fr,
/// big-endian bytes (Soroban `Fr::from_bytes` consumes BE).
///
/// Source: arkworks-bls12-381 0.5 `<Fr as FftField>::TWO_ADIC_ROOT_OF_UNITY`,
/// dumped via the prover-side reference's
/// `circuit::plonk::verifier_polys::tests::domain_params_match_arkworks`
/// path (which is itself oracle-tested against
/// `ark_poly::Radix2EvaluationDomain::new(n).group_gen` at every
/// supported domain size).
pub const FR_TWO_ADIC_ROOT_OF_UNITY_BE: [u8; 32] = [
    0x16, 0xa2, 0xa1, 0x9e, 0xdf, 0xe8, 0x1f, 0x20, 0xd0, 0x9b, 0x68, 0x19, 0x22, 0xc8, 0x13, 0xb4,
    0xb6, 0x36, 0x83, 0x50, 0x8c, 0x22, 0x80, 0xb9, 0x38, 0x29, 0x97, 0x1f, 0x43, 0x9f, 0x0d, 0x2b,
];

/// Bundle of an evaluation-domain's tier-dependent constants.
///
/// `for_size(env, n)` derives the primitive n-th root of unity by
/// squaring the pinned 2^32-th root down to 2^log_size, then computes
/// its inverse via Soroban's `fr_inv`.
pub struct DomainParams {
    /// Domain size. Power of two.
    pub size: u64,
    /// Primitive `size`-th root of unity in Fr.
    pub group_gen: Fr,
    /// `group_gen.inv()`.
    pub group_gen_inv: Fr,
}

impl DomainParams {
    /// Build the parameters for a domain of size `domain_size`.
    /// Panics if `domain_size` is not a power of two within Fr's
    /// two-adicity bound.
    pub fn for_size(env: &Env, domain_size: u64) -> Self {
        assert!(
            domain_size.is_power_of_two(),
            "domain_size is not a power of two"
        );
        let log_size = domain_size.trailing_zeros();
        assert!(
            log_size <= FR_TWO_ADICITY,
            "domain_size 2^log_size exceeds FR_TWO_ADICITY"
        );

        let two_adic_root_bytes =
            BytesN::<32>::from_array(env, &FR_TWO_ADIC_ROOT_OF_UNITY_BE);
        let mut omega = Fr::from_bytes(two_adic_root_bytes);
        // Square `(FR_TWO_ADICITY - log_size)` times so omega becomes
        // a primitive 2^log_size-th root of unity.
        for _ in log_size..FR_TWO_ADICITY {
            omega = omega.clone() * omega.clone();
        }

        let group_gen_inv = omega.inv();

        Self {
            size: domain_size,
            group_gen: omega,
            group_gen_inv,
        }
    }
}

/// Helper: BLS12-381 Fr `1` as a Soroban `Fr`.
fn fr_one(env: &Env) -> Fr {
    let mut bytes = [0u8; 32];
    bytes[31] = 0x01; // BE: high bytes zero, low byte 1
    Fr::from_bytes(BytesN::from_array(env, &bytes))
}

/// Helper: BLS12-381 Fr `0` as a Soroban `Fr`.
fn fr_zero(env: &Env) -> Fr {
    Fr::from_bytes(BytesN::from_array(env, &[0u8; 32]))
}

/// Helper: turn a `u64` into a Soroban Fr (canonical BE encoding).
fn fr_from_u64(env: &Env, value: u64) -> Fr {
    let mut bytes = [0u8; 32];
    bytes[24..32].copy_from_slice(&value.to_be_bytes());
    Fr::from_bytes(BytesN::from_array(env, &bytes))
}

/// Evaluate the vanishing polynomial of a non-coset radix-2 domain
/// at ζ: `Z_H(ζ) = ζⁿ − 1`.
pub fn evaluate_vanishing_poly(zeta: &Fr, params: &DomainParams) -> Fr {
    let zeta_n = zeta.pow(params.size);
    zeta_n - fr_one(zeta.env())
}

/// Compute `(L_0(ζ), L_{n−1}(ζ))` for a non-coset radix-2 domain.
///
/// Mirrors the non-coset branch of jf-plonk's
/// `LagrangeCoeffs::first_and_last_lagrange_coeffs`:
///
/// - ζ = 1 → `(1, 0)`
/// - ζ = g^{-1} → `(0, 1)`
/// - ζ at any other domain point (Z_H(ζ) = 0) → `(0, 0)`
/// - else: `L_0 = Z_H(ζ) / (n·(ζ−1))`,
///        `L_{n−1} = Z_H(ζ)·g^{-1} / (n·(ζ−g^{-1}))`.
pub fn first_and_last_lagrange_coeffs(
    zeta: &Fr,
    vanishing_eval: &Fr,
    params: &DomainParams,
) -> (Fr, Fr) {
    let env = zeta.env();
    let one = fr_one(env);
    let zero = fr_zero(env);

    // ζ = 1 → (1, 0).
    if zeta_eq(zeta, &one) {
        return (one, zero);
    }
    // ζ = g_inv → (0, 1).
    if zeta_eq(zeta, &params.group_gen_inv) {
        return (zero, one);
    }
    // ζ ∈ H but not at the {first, last} positions → both zero.
    if zeta_eq(vanishing_eval, &zero) {
        return (zero.clone(), zero);
    }

    let n_fr = fr_from_u64(env, params.size);
    // L_0 = Z_H(ζ) · (n · (ζ−1))⁻¹
    let denom_l_0 = n_fr.clone() * (zeta.clone() - one.clone());
    let l_0 = vanishing_eval.clone() * denom_l_0.inv();
    // L_{n−1} = Z_H(ζ) · g⁻¹ · (n · (ζ−g⁻¹))⁻¹
    let denom_l_n = n_fr * (zeta.clone() - params.group_gen_inv.clone());
    let l_n_minus_1 =
        vanishing_eval.clone() * params.group_gen_inv.clone() * denom_l_n.inv();
    (l_0, l_n_minus_1)
}

/// Evaluate the public-input polynomial at ζ:
/// `PI(ζ) = Σ Lᵢ(ζ)·publicᵢ`.
///
/// `public_inputs` are pre-reduced Fr scalars (caller converts from
/// BE bytes via `Fr::from_bytes(BytesN<32>)`).
///
/// Special case: if ζ equals a domain point `gⁱ` for some `i` in
/// `0..public_inputs.len()`, we short-circuit `PI(ζ) = publicᵢ`
/// without dividing by zero.
pub fn evaluate_pi_poly(
    public_inputs: &[Fr],
    zeta: &Fr,
    vanishing_eval: &Fr,
    params: &DomainParams,
) -> Fr {
    if public_inputs.is_empty() {
        return fr_zero(zeta.env());
    }

    let env = zeta.env();
    let n_fr = fr_from_u64(env, params.size);
    let mut sum = fr_zero(env);
    let mut g_i = fr_one(env); // g^0 = 1; updated to g^i each iteration.

    for pi in public_inputs.iter() {
        if zeta_eq(zeta, &g_i) {
            // ζ is exactly the i-th domain point: Lᵢ(ζ) = 1, others = 0.
            return pi.clone();
        }
        // Lᵢ(ζ) = Z_H(ζ) · gⁱ · (n · (ζ − gⁱ))⁻¹
        let denom = n_fr.clone() * (zeta.clone() - g_i.clone());
        let l_i = vanishing_eval.clone() * g_i.clone() * denom.inv();
        sum = sum + l_i * pi.clone();
        g_i = g_i.clone() * params.group_gen.clone();
    }
    sum
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Equality check via byte comparison. Soroban `Fr` doesn't expose
/// a direct `PartialEq`; comparing byte representations is the
/// portable way (and `Fr::from_bytes` reduces mod r so the byte
/// representation is canonical).
fn zeta_eq(a: &Fr, b: &Fr) -> bool {
    a.to_bytes() == b.to_bytes()
}

// Soroban `Fr` doesn't impl `Div` (the SDK's `Add`, `Sub`, `Mul`
// op-trait surface is finite). The reference's `/` operations are
// rewritten in this module as `* x.inv()` — same semantics, since
// `inv()` is `fr_inv` on the host.

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    /// Sentinels: ensure helper conversions match Soroban Fr's BE
    /// `from_bytes` semantics.
    #[test]
    fn fr_one_round_trips_to_known_be_bytes() {
        let env = Env::default();
        let one = fr_one(&env);
        let bytes = one.to_bytes().to_array();
        let mut expected = [0u8; 32];
        expected[31] = 0x01;
        assert_eq!(bytes, expected);
    }

    #[test]
    fn fr_from_u64_matches_be_encoding() {
        let env = Env::default();
        let x = fr_from_u64(&env, 0x1234_5678_9abc_def0u64);
        let bytes = x.to_bytes().to_array();
        let mut expected = [0u8; 32];
        expected[24..32].copy_from_slice(&0x1234_5678_9abc_def0u64.to_be_bytes());
        assert_eq!(bytes, expected);
    }

    /// `DomainParams::for_size(n)` produces a primitive n-th root of
    /// unity: `g^n = 1`. Sanity check that the squaring-down loop
    /// reaches the right power.
    #[test]
    fn domain_generator_is_nth_root_of_unity() {
        let env = Env::default();
        for &n in &[8u64, 16, 32, 8192, 16384, 32768] {
            let params = DomainParams::for_size(&env, n);
            let g_n = params.group_gen.pow(n);
            assert_eq!(
                g_n.to_bytes().to_array(),
                fr_one(&env).to_bytes().to_array(),
                "g^{} != 1 for n={n}",
                n
            );
        }
    }

    /// `group_gen * group_gen_inv = 1`.
    #[test]
    fn group_gen_inv_is_inverse() {
        let env = Env::default();
        let params = DomainParams::for_size(&env, 16384);
        let prod = params.group_gen.clone() * params.group_gen_inv.clone();
        assert_eq!(
            prod.to_bytes().to_array(),
            fr_one(&env).to_bytes().to_array()
        );
    }

    /// Vanishing poly evaluates to 0 at any domain point.
    #[test]
    fn vanishing_poly_is_zero_at_domain_points() {
        let env = Env::default();
        let n = 16u64;
        let params = DomainParams::for_size(&env, n);
        let one = fr_one(&env);
        let zero = fr_zero(&env);

        // ζ = 1 (= g^0).
        assert_eq!(
            evaluate_vanishing_poly(&one, &params).to_bytes().to_array(),
            zero.to_bytes().to_array()
        );
        // ζ = g (= g^1).
        assert_eq!(
            evaluate_vanishing_poly(&params.group_gen, &params)
                .to_bytes()
                .to_array(),
            zero.to_bytes().to_array()
        );
        // ζ = g_inv (= g^{n-1}).
        assert_eq!(
            evaluate_vanishing_poly(&params.group_gen_inv, &params)
                .to_bytes()
                .to_array(),
            zero.to_bytes().to_array()
        );
    }

    /// Vanishing poly is non-zero at non-domain-point ζ.
    #[test]
    fn vanishing_poly_is_nonzero_at_random_zeta() {
        let env = Env::default();
        let n = 16u64;
        let params = DomainParams::for_size(&env, n);
        let zeta = fr_from_u64(&env, 12345);
        let z_h = evaluate_vanishing_poly(&zeta, &params);
        assert_ne!(
            z_h.to_bytes().to_array(),
            fr_zero(&env).to_bytes().to_array()
        );
    }

    /// `first_and_last_lagrange_coeffs` early-return cases.
    #[test]
    fn lagrange_coeffs_at_domain_endpoints() {
        let env = Env::default();
        let params = DomainParams::for_size(&env, 16);
        let one = fr_one(&env);
        let zero = fr_zero(&env);

        // ζ = 1 → (1, 0)
        let z_h = evaluate_vanishing_poly(&one, &params);
        let (l_0, l_n) = first_and_last_lagrange_coeffs(&one, &z_h, &params);
        assert_eq!(l_0.to_bytes().to_array(), one.to_bytes().to_array());
        assert_eq!(l_n.to_bytes().to_array(), zero.to_bytes().to_array());

        // ζ = g_inv → (0, 1)
        let z_h = evaluate_vanishing_poly(&params.group_gen_inv, &params);
        let (l_0, l_n) = first_and_last_lagrange_coeffs(&params.group_gen_inv, &z_h, &params);
        assert_eq!(l_0.to_bytes().to_array(), zero.to_bytes().to_array());
        assert_eq!(l_n.to_bytes().to_array(), one.to_bytes().to_array());

        // ζ = g (an interior domain point) → (0, 0)
        let z_h = evaluate_vanishing_poly(&params.group_gen, &params);
        let (l_0, l_n) = first_and_last_lagrange_coeffs(&params.group_gen, &z_h, &params);
        assert_eq!(l_0.to_bytes().to_array(), zero.to_bytes().to_array());
        assert_eq!(l_n.to_bytes().to_array(), zero.to_bytes().to_array());
    }

    /// `evaluate_pi_poly` short-circuit at a domain point: PI(g^i) = pubᵢ.
    #[test]
    fn evaluate_pi_poly_short_circuits_at_domain_point() {
        let env = Env::default();
        let params = DomainParams::for_size(&env, 16);
        let pi_a = fr_from_u64(&env, 42);
        let pi_b = fr_from_u64(&env, 137);
        let pi_c = fr_from_u64(&env, 9000);
        let inputs = [pi_a.clone(), pi_b.clone(), pi_c.clone()];

        // ζ = 1 → PI = pi_a (i=0)
        let one = fr_one(&env);
        let z_h = evaluate_vanishing_poly(&one, &params);
        let result = evaluate_pi_poly(&inputs, &one, &z_h, &params);
        assert_eq!(
            result.to_bytes().to_array(),
            pi_a.to_bytes().to_array(),
            "PI(g^0) should be public_inputs[0]"
        );

        // ζ = g^2 → PI = pi_c (i=2)
        let g_squared = params.group_gen.clone() * params.group_gen.clone();
        let z_h = evaluate_vanishing_poly(&g_squared, &params);
        let result = evaluate_pi_poly(&inputs, &g_squared, &z_h, &params);
        assert_eq!(
            result.to_bytes().to_array(),
            pi_c.to_bytes().to_array(),
            "PI(g^2) should be public_inputs[2]"
        );
    }

    /// Empty PI vector → 0.
    #[test]
    fn evaluate_pi_poly_empty_returns_zero() {
        let env = Env::default();
        let params = DomainParams::for_size(&env, 16);
        let zeta = fr_from_u64(&env, 7);
        let z_h = evaluate_vanishing_poly(&zeta, &params);
        let result = evaluate_pi_poly(&[], &zeta, &z_h, &params);
        assert_eq!(
            result.to_bytes().to_array(),
            fr_zero(&env).to_bytes().to_array()
        );
    }

    /// Hand-computed L_0(ζ) at random ζ ∉ H matches our formula.
    /// L_0(ζ) = Z_H(ζ) / (n · (ζ − 1)). For n=4, ζ=2:
    /// Z_H(2) = 2⁴ − 1 = 15. Denom = 4 · (2 − 1) = 4.
    /// L_0(2) = 15 / 4 (mod r). We assert via L_0 · 4 = 15.
    #[test]
    fn first_lagrange_coeff_hand_computed() {
        let env = Env::default();
        let params = DomainParams::for_size(&env, 4);
        let zeta = fr_from_u64(&env, 2);
        let z_h = evaluate_vanishing_poly(&zeta, &params);
        let (l_0, _) = first_and_last_lagrange_coeffs(&zeta, &z_h, &params);

        // L_0 · n · (ζ - 1) == Z_H(ζ)
        let n_fr = fr_from_u64(&env, 4);
        let one = fr_one(&env);
        let lhs = l_0.clone() * n_fr * (zeta - one);
        assert_eq!(
            lhs.to_bytes().to_array(),
            z_h.to_bytes().to_array(),
            "L_0 doesn't satisfy its defining equation"
        );
    }
}
