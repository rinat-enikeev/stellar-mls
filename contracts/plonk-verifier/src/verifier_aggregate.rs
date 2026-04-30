//! Soroban-side port of `sep-xxxx-circuits::circuit::plonk::verifier_aggregate`
//! (PR #182). Produces the `(scalars, bases)` list the verifier MSM-folds
//! into the batched-polynomial commitment `[D]_1` plus the 10-element
//! `v_uv_buffer` consumed by the (forthcoming) `aggregate_evaluations`.
//!
//! Mirrors jf-plonk's `Verifier::aggregate_poly_commitments` +
//! `linearization_scalars_and_bases` for the no-Plookup, single-instance
//! case our membership circuits use. Emits exactly **30 (scalar, base)
//! pairs**:
//!
//! | source                                | count | scalar |
//! |---------------------------------------|------:|--------|
//! | prod_perm_poly_comm (linearisation)   |     1 | α²·L_0(ζ) + α·Π(β·k_i·ζ + γ + w_i) |
//! | last sigma_comm                       |     1 | −α·β·z(ζ·g)·Π(β·σ_i + γ + w_i)     |
//! | selector_comms (13)                   |    13 | q_scalars[i] (TurboPlonk gate map) |
//! | split_quot_comms (5)                  |     5 | −Z_H(ζ), −Z_H(ζ)·ζ^{n+2}, … (geom.) |
//! | wire commitments (5, v-combined)      |     5 | v, v², v³, v⁴, v⁵ |
//! | sigma_comms[0..4] (v-combined)        |     4 | v⁶ … v⁹ |
//! | prod_perm_poly_comm (uv-combined)     |     1 | u (NB: not u·v — see ref module docs) |
//!
//! and a 10-entry `v_uv_buffer` holding the same 10 scalars in
//! emission order (the v¹..v⁹ + final `u` sequence).
//!
//! Soroban `Vec<T>` requires an `Env`, so the public surface threads
//! `&Env` through every constructor. Each scalar derivation is a
//! sequence of host calls (`fr_add`, `fr_mul`); base parsing is a
//! single `G1Affine::from_bytes(BytesN<96>)` per slot. The returned
//! `bases`/`scalars` vectors feed directly into
//! `env.crypto().bls12_381().g1_msm(...)`.

use soroban_sdk::crypto::bls12_381::{Fr, G1Affine};
use soroban_sdk::{Env, Vec};

use crate::byte_helpers::{
    decode_fr_array, decode_g1_array, fr_from_le_bytes, fr_one, fr_zero, g1_from_bytes,
};
use crate::proof_format::{ParsedProof, NUM_WIRE_SIGMA_EVALS, NUM_WIRE_TYPES};
use crate::vk_format::{ParsedVerifyingKey, NUM_SELECTOR_COMMS, NUM_SIGMA_COMMS};

/// Bundle of the six Fiat-Shamir challenges already reduced to `Fr`.
/// Caller reduces from raw BE bytes (output of
/// `verifier_challenges::compute_challenges`) via
/// `Fr::from_bytes(BytesN::from_array(env, &raw))`.
#[derive(Clone)]
pub struct ChallengesFr {
    pub beta: Fr,
    pub gamma: Fr,
    pub alpha: Fr,
    pub zeta: Fr,
    pub v: Fr,
    pub u: Fr,
}

/// Output of [`aggregate_poly_commitments`]: an MSM-ready list of 30
/// `(scalar, base)` pairs plus the 10-element `v_uv_buffer` consumed
/// by `aggregate_evaluations`.
#[derive(Clone)]
pub struct AggregatedCommitments {
    pub scalars: Vec<Fr>,
    pub bases: Vec<G1Affine>,
    pub v_uv_buffer: Vec<Fr>,
}

/// Multi-scalar-multiply this output to obtain the batched-polynomial
/// commitment `[D]_1`. Single host call.
impl AggregatedCommitments {
    pub fn multi_scalar_multiply(&self, env: &Env) -> G1Affine {
        env.crypto()
            .bls12_381()
            .g1_msm(self.bases.clone(), self.scalars.clone())
    }
}

/// Aggregate the verifier's polynomial commitments into the
/// MSM-ready `[D]_1`-form list of `(scalar, base)` pairs plus the
/// `v_uv_buffer` for the later evaluation aggregation step.
///
/// Single-instance, no-Plookup. Order of emitted pairs matches
/// jf-plonk's `linearization_scalars_and_bases` followed by the
/// `aggregate_poly_commitments` v/uv combiner loop.
pub fn aggregate_poly_commitments(
    env: &Env,
    challenges: &ChallengesFr,
    vanish_eval: Fr,
    lagrange_1_eval: Fr,
    vk: &ParsedVerifyingKey,
    proof: &ParsedProof,
) -> AggregatedCommitments {
    // -----------------------------------------------------------
    // Decode all Fr / G1 inputs once up front so the loop bodies
    // below are pure Fr arithmetic + base pushes.
    // -----------------------------------------------------------
    let k_constants: [Fr; NUM_WIRE_TYPES] = decode_fr_array(env, &vk.k_constants);
    let w_evals: [Fr; NUM_WIRE_TYPES] = decode_fr_array(env, &proof.wires_evals);
    let sigma_evals: [Fr; NUM_WIRE_SIGMA_EVALS] =
        decode_fr_array(env, &proof.wire_sigma_evals);
    let perm_next_eval = fr_from_le_bytes(env, &proof.perm_next_eval);

    let prod_perm_g1 = g1_from_bytes(env, &proof.prod_perm_commitment);
    let split_quot_g1: [G1Affine; NUM_WIRE_TYPES] =
        decode_g1_array(env, &proof.split_quot_commitments);
    let wire_g1: [G1Affine; NUM_WIRE_TYPES] =
        decode_g1_array(env, &proof.wire_commitments);
    let selector_g1: [G1Affine; NUM_SELECTOR_COMMS] =
        decode_g1_array(env, &vk.selector_commitments);
    let sigma_g1: [G1Affine; NUM_SIGMA_COMMS] =
        decode_g1_array(env, &vk.sigma_commitments);

    let alpha = challenges.alpha.clone();
    let beta = challenges.beta.clone();
    let gamma = challenges.gamma.clone();
    let zeta = challenges.zeta.clone();
    let v = challenges.v.clone();
    let u = challenges.u.clone();

    let mut scalars: Vec<Fr> = Vec::new(env);
    let mut bases: Vec<G1Affine> = Vec::new(env);
    let mut v_uv_buffer: Vec<Fr> = Vec::new(env);

    // -----------------------------------------------------------
    // Linearisation part — `linearization_scalars_and_bases`.
    // -----------------------------------------------------------

    // 1. Permutation product polynomial commitment.
    //    coeff = α²·L_0(ζ) + α·Π_{i=0..n-1}(β·k_i·ζ + γ + w_i)
    let alpha_squared = alpha.clone() * alpha.clone();
    let perm_coeff = {
        let mut prod = alpha.clone();
        for (w, k) in w_evals.iter().zip(k_constants.iter()) {
            // term = β·k·ζ + γ + w
            let term =
                beta.clone() * k.clone() * zeta.clone() + gamma.clone() + w.clone();
            prod = prod * term;
        }
        alpha_squared * lagrange_1_eval.clone() + prod
    };
    scalars.push_back(perm_coeff);
    bases.push_back(prod_perm_g1.clone());

    // 2. Last sigma polynomial commitment (sigma_comms[NUM_SIGMA_COMMS-1]).
    //    coeff = −α·β·z(ζ·g)·Π_{i=0..n-2}(β·σ_i + γ + w_i)
    let last_sigma_coeff = {
        let mut prod = alpha.clone() * beta.clone() * perm_next_eval;
        for (w, sigma) in w_evals
            .iter()
            .take(NUM_WIRE_SIGMA_EVALS)
            .zip(sigma_evals.iter())
        {
            let term =
                beta.clone() * sigma.clone() + gamma.clone() + w.clone();
            prod = prod * term;
        }
        // Negate via `0 - prod` (Soroban `Fr` impls `Sub`).
        fr_zero(env) - prod
    };
    scalars.push_back(last_sigma_coeff);
    bases.push_back(sigma_g1[NUM_SIGMA_COMMS - 1].clone());

    // 3. Selector polynomial commitments — 13 entries per
    //    jf-relation's `N_TURBO_PLONK_SELECTORS` ordering:
    //    q_lc(0..3), q_mul(4..5), q_hash(6..9), q_o(10), q_c(11), q_ecc(12).
    let q_scalars: [Fr; NUM_SELECTOR_COMMS] = {
        let w0 = w_evals[0].clone();
        let w1 = w_evals[1].clone();
        let w2 = w_evals[2].clone();
        let w3 = w_evals[3].clone();
        let w4 = w_evals[4].clone();
        [
            w0.clone(),                                       // q_lc[0]: w_0
            w1.clone(),                                       // q_lc[1]: w_1
            w2.clone(),                                       // q_lc[2]: w_2
            w3.clone(),                                       // q_lc[3]: w_3
            w0.clone() * w1.clone(),                          // q_mul[0]: w_0·w_1
            w2.clone() * w3.clone(),                          // q_mul[1]: w_2·w_3
            w0.clone().pow(5),                                // q_hash[0]: w_0^5
            w1.clone().pow(5),                                // q_hash[1]: w_1^5
            w2.clone().pow(5),                                // q_hash[2]: w_2^5
            w3.clone().pow(5),                                // q_hash[3]: w_3^5
            fr_zero(env) - w4.clone(),                        // q_o: −w_4
            fr_one(env),                                      // q_c: 1
            w0 * w1 * w2 * w3 * w4,                           // q_ecc: w_0·w_1·w_2·w_3·w_4
        ]
    };
    for (s, b) in q_scalars.iter().zip(selector_g1.iter()) {
        scalars.push_back(s.clone());
        bases.push_back(b.clone());
    }

    // 4. Split quotient commitments — 5 entries with geometric scaling.
    //    coeff_0 = −Z_H(ζ); coeff_i = coeff_{i-1} · ζ^{n+2}
    //    where ζ^{n+2} = (1 + Z_H(ζ))·ζ²
    let zeta_to_n_plus_2 =
        (fr_one(env) + vanish_eval.clone()) * zeta.clone() * zeta.clone();
    let mut split_coeff = fr_zero(env) - vanish_eval.clone();
    scalars.push_back(split_coeff.clone());
    bases.push_back(split_quot_g1[0].clone());
    for poly in split_quot_g1.iter().skip(1) {
        split_coeff = split_coeff * zeta_to_n_plus_2.clone();
        scalars.push_back(split_coeff.clone());
        bases.push_back(poly.clone());
    }

    // -----------------------------------------------------------
    // v/uv combiner — `aggregate_poly_commitments` body, no-Plookup.
    // -----------------------------------------------------------

    // 5. Wire polynomial commitments — 5 entries scaled by v¹..v⁵.
    let mut v_base = v.clone();
    for poly in wire_g1.iter() {
        v_uv_buffer.push_back(v_base.clone());
        scalars.push_back(v_base.clone());
        bases.push_back(poly.clone());
        v_base = v_base * v.clone();
    }

    // 6. First (n−1) sigma commitments — scaled by v⁶..v⁹.
    for poly in sigma_g1.iter().take(NUM_WIRE_SIGMA_EVALS) {
        v_uv_buffer.push_back(v_base.clone());
        scalars.push_back(v_base.clone());
        bases.push_back(poly.clone());
        v_base = v_base * v.clone();
    }

    // 7. prod_perm_poly_comm — scaled by `u` (NOT `u·v`; jf-plonk's
    //    `add_poly_comm` pushes `*random_combiner` BEFORE multiplying
    //    by `r`, so the first uv-branch entry sees `uv_base = u`).
    v_uv_buffer.push_back(u.clone());
    scalars.push_back(u);
    bases.push_back(prod_perm_g1);

    debug_assert_eq!(scalars.len(), 30, "expected 30 (scalar, base) pairs");
    debug_assert_eq!(bases.len(), 30);
    debug_assert_eq!(v_uv_buffer.len(), 10, "expected 10 v_uv buffer entries");

    AggregatedCommitments {
        scalars,
        bases,
        v_uv_buffer,
    }
}

// Internal Fr/G1 byte conversion helpers live in `crate::byte_helpers`
// to avoid the duplication this module previously carried.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proof_format::parse_proof_bytes;
    use crate::test_fixtures::{build_synthetic_proof_bytes, build_synthetic_vk_bytes};
    use crate::vk_format::parse_vk_bytes;
    use soroban_sdk::{BytesN, Env};

    /// Helper: build a deterministic Fr from a u64 (BE-encoded).
    fn fr(env: &Env, value: u64) -> Fr {
        let mut bytes = [0u8; 32];
        bytes[24..32].copy_from_slice(&value.to_be_bytes());
        Fr::from_bytes(BytesN::from_array(env, &bytes))
    }

    fn synthetic_challenges(env: &Env) -> ChallengesFr {
        ChallengesFr {
            beta: fr(env, 11),
            gamma: fr(env, 13),
            alpha: fr(env, 17),
            zeta: fr(env, 19),
            v: fr(env, 23),
            u: fr(env, 29),
        }
    }

    fn synthetic_fixture(_env: &Env) -> (ParsedVerifyingKey, ParsedProof) {
        let vk_bytes = build_synthetic_vk_bytes(8192, 2);
        let proof_bytes = build_synthetic_proof_bytes();
        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let parsed_proof = parse_proof_bytes(&proof_bytes).expect("parse proof");
        (parsed_vk, parsed_proof)
    }

    /// Output has the expected structural shape: 30 (scalar, base)
    /// pairs and a 10-entry v_uv buffer.
    #[test]
    fn output_shape_is_30_pairs_plus_10_buffer() {
        let env = Env::default();
        let (vk, proof) = synthetic_fixture(&env);
        let challenges = synthetic_challenges(&env);
        let agg = aggregate_poly_commitments(
            &env,
            &challenges,
            fr(&env, 7),
            fr(&env, 11),
            &vk,
            &proof,
        );
        assert_eq!(agg.scalars.len(), 30);
        assert_eq!(agg.bases.len(), 30);
        assert_eq!(agg.v_uv_buffer.len(), 10);
    }

    /// The 13 selector scalars match jf-plonk's `q_scalars` formula
    /// exactly. Replicates the formula inline as oracle so a typo in
    /// the q_scalars table or in the wire-evaluation indexing would
    /// be caught.
    #[test]
    fn selector_scalars_match_jf_plonk_formula() {
        let env = Env::default();
        let (vk, proof) = synthetic_fixture(&env);
        let challenges = synthetic_challenges(&env);
        let agg = aggregate_poly_commitments(
            &env,
            &challenges,
            fr(&env, 7),
            fr(&env, 11),
            &vk,
            &proof,
        );

        // Selectors occupy entries [2..15] (after perm + last-sigma).
        let w_evals: [Fr; 5] =
            core::array::from_fn(|i| fr_from_le_bytes(&env, &proof.wires_evals[i]));
        let expected = [
            w_evals[0].clone(),
            w_evals[1].clone(),
            w_evals[2].clone(),
            w_evals[3].clone(),
            w_evals[0].clone() * w_evals[1].clone(),
            w_evals[2].clone() * w_evals[3].clone(),
            w_evals[0].clone().pow(5),
            w_evals[1].clone().pow(5),
            w_evals[2].clone().pow(5),
            w_evals[3].clone().pow(5),
            fr_zero(&env) - w_evals[4].clone(),
            fr_one(&env),
            w_evals[0].clone()
                * w_evals[1].clone()
                * w_evals[2].clone()
                * w_evals[3].clone()
                * w_evals[4].clone(),
        ];

        for (i, want) in expected.iter().enumerate() {
            let got = agg.scalars.get(2 + i as u32).expect("selector slot");
            assert_eq!(
                got.to_bytes().to_array(),
                want.to_bytes().to_array(),
                "selector q_scalars[{i}] mismatch",
            );
        }
    }

    /// `scalars[0]` is `perm_coeff = α²·L_0(ζ) + α·Π(β·k_i·ζ + γ + w_i)`.
    /// The production code computes the product via a `for` fold;
    /// this test computes the same value by **unrolling the 5
    /// per-i terms explicitly**, multiplying them in a different
    /// expression structure, and asserts equality. Catches the
    /// failure modes the reviewer flagged:
    /// - Wrong zip pairing (wires vs k_constants).
    /// - Off-by-one in the slice bound.
    /// - Wrong fold seed (not α).
    /// - Wrong outer addition (the α²·L_0 term).
    ///
    /// Doesn't catch a typo *inside* `β·k·ζ + γ + w` itself (e.g.
    /// `*` instead of `+` between the gamma and w terms) — that
    /// requires hand-computed numerical values, deferred to real
    /// fixture work.
    #[test]
    fn perm_coeff_matches_unrolled_reference() {
        let env = Env::default();
        let (vk, proof) = synthetic_fixture(&env);
        let challenges = synthetic_challenges(&env);
        let vanish_eval = fr(&env, 7);
        let lagrange_1_eval = fr(&env, 11);

        let agg = aggregate_poly_commitments(
            &env,
            &challenges,
            vanish_eval,
            lagrange_1_eval.clone(),
            &vk,
            &proof,
        );

        // Decode wire & k_constants Fr values from the synthetic VK / proof.
        let w: [Fr; 5] = core::array::from_fn(|i| {
            fr_from_le_bytes(&env, &proof.wires_evals[i])
        });
        let k: [Fr; 5] = core::array::from_fn(|i| {
            fr_from_le_bytes(&env, &vk.k_constants[i])
        });

        // Unrolled reference: build the five terms separately and
        // multiply them in a different expression structure than the
        // production for/fold loop.
        let alpha = challenges.alpha.clone();
        let beta = challenges.beta.clone();
        let gamma = challenges.gamma.clone();
        let zeta = challenges.zeta.clone();

        let term = |i: usize| {
            beta.clone() * k[i].clone() * zeta.clone() + gamma.clone() + w[i].clone()
        };
        let prod = alpha.clone()
            * term(0)
            * term(1)
            * term(2)
            * term(3)
            * term(4);
        let alpha_squared = alpha.clone() * alpha;
        let expected = alpha_squared * lagrange_1_eval + prod;

        let got = agg.scalars.get(0).unwrap();
        assert_eq!(
            got.to_bytes().to_array(),
            expected.to_bytes().to_array(),
            "scalars[0] (perm_coeff) diverges from unrolled reference",
        );
    }

    /// `scalars[1]` is `last_sigma_coeff = −α·β·z(ζ·g)·Π(β·σ_i + γ + w_i)`.
    /// Same shape as the perm_coeff test: unroll the 4 per-i terms,
    /// multiply explicitly, negate, compare. Catches:
    /// - Wrong slice (wires_evals takes 4, sigma_evals takes 4).
    /// - Wrong fold seed (must be α·β·z(ζ·g), not just α).
    /// - Missing negation.
    /// - Off-by-one in `take(NUM_WIRE_SIGMA_EVALS)`.
    #[test]
    fn last_sigma_coeff_matches_unrolled_reference() {
        let env = Env::default();
        let (vk, proof) = synthetic_fixture(&env);
        let challenges = synthetic_challenges(&env);
        let vanish_eval = fr(&env, 7);
        let lagrange_1_eval = fr(&env, 11);

        let agg = aggregate_poly_commitments(
            &env,
            &challenges,
            vanish_eval,
            lagrange_1_eval,
            &vk,
            &proof,
        );

        let w: [Fr; 5] = core::array::from_fn(|i| {
            fr_from_le_bytes(&env, &proof.wires_evals[i])
        });
        let sigma: [Fr; 4] = core::array::from_fn(|i| {
            fr_from_le_bytes(&env, &proof.wire_sigma_evals[i])
        });
        let perm_next = fr_from_le_bytes(&env, &proof.perm_next_eval);

        let alpha = challenges.alpha.clone();
        let beta = challenges.beta.clone();
        let gamma = challenges.gamma.clone();

        // Unrolled reference: 4 terms (sigma takes only 4), seeded
        // with α·β·z(ζ·g).
        let term = |i: usize| {
            beta.clone() * sigma[i].clone() + gamma.clone() + w[i].clone()
        };
        let prod = alpha.clone() * beta.clone() * perm_next
            * term(0)
            * term(1)
            * term(2)
            * term(3);
        let expected = fr_zero(&env) - prod;

        let got = agg.scalars.get(1).unwrap();
        assert_eq!(
            got.to_bytes().to_array(),
            expected.to_bytes().to_array(),
            "scalars[1] (last_sigma_coeff) diverges from unrolled reference",
        );
    }

    /// The 5 split-quot scalars form a geometric progression with
    /// ratio `ζ^{n+2} = (1 + Z_H(ζ))·ζ²` starting at `−Z_H(ζ)`.
    #[test]
    fn split_quot_scalars_form_geometric_progression() {
        let env = Env::default();
        let (vk, proof) = synthetic_fixture(&env);
        let challenges = synthetic_challenges(&env);
        let vanish_eval = fr(&env, 7);
        let agg = aggregate_poly_commitments(
            &env,
            &challenges,
            vanish_eval.clone(),
            fr(&env, 11),
            &vk,
            &proof,
        );

        // Split-quot scalars are at positions [15..20].
        let zeta = challenges.zeta.clone();
        let zeta_to_n_plus_2 =
            (fr_one(&env) + vanish_eval.clone()) * zeta.clone() * zeta;

        let split_0 = agg.scalars.get(15).expect("split[0]");
        let neg_z_h = fr_zero(&env) - vanish_eval;
        assert_eq!(
            split_0.to_bytes().to_array(),
            neg_z_h.to_bytes().to_array(),
            "split[0] should be −Z_H(ζ)"
        );
        for i in 1..5u32 {
            let prev = agg.scalars.get(15 + i - 1).unwrap();
            let curr = agg.scalars.get(15 + i).unwrap();
            let expected = prev * zeta_to_n_plus_2.clone();
            assert_eq!(
                curr.to_bytes().to_array(),
                expected.to_bytes().to_array(),
                "split[{i}] not geometric ratio of split[{}]",
                i - 1,
            );
        }
    }

    /// The v/uv combiner section produces the right power sequence:
    /// 5 wires at v..v⁵, 4 sigmas at v⁶..v⁹, 1 prod_perm at u.
    #[test]
    fn v_uv_buffer_powers_match_v_and_uv() {
        let env = Env::default();
        let (vk, proof) = synthetic_fixture(&env);
        let challenges = synthetic_challenges(&env);
        let agg = aggregate_poly_commitments(
            &env,
            &challenges,
            fr(&env, 7),
            fr(&env, 11),
            &vk,
            &proof,
        );

        let v = challenges.v.clone();
        let u = challenges.u.clone();
        let mut expected_v = v.clone();
        for i in 0..9u32 {
            let got = agg.v_uv_buffer.get(i).unwrap();
            assert_eq!(
                got.to_bytes().to_array(),
                expected_v.to_bytes().to_array(),
                "v_uv_buffer[{i}] should be v^{}",
                i + 1
            );
            expected_v = expected_v * v.clone();
        }
        let last = agg.v_uv_buffer.get(9).unwrap();
        assert_eq!(
            last.to_bytes().to_array(),
            u.to_bytes().to_array(),
            "v_uv_buffer[9] should be u"
        );
    }

    /// Bases are correctly drawn from the parsed VK / proof.
    /// Spot-check key positions.
    #[test]
    fn bases_drawn_from_correct_sources() {
        let env = Env::default();
        let (vk, proof) = synthetic_fixture(&env);
        let challenges = synthetic_challenges(&env);
        let agg = aggregate_poly_commitments(
            &env,
            &challenges,
            fr(&env, 7),
            fr(&env, 11),
            &vk,
            &proof,
        );

        let prod_perm = g1_from_bytes(&env, &proof.prod_perm_commitment);
        let last_sigma =
            g1_from_bytes(&env, &vk.sigma_commitments[NUM_SIGMA_COMMS - 1]);

        // bases[0] = prod_perm
        assert_eq!(
            agg.bases.get(0).unwrap().to_bytes().to_array(),
            prod_perm.to_bytes().to_array()
        );
        // bases[1] = last sigma
        assert_eq!(
            agg.bases.get(1).unwrap().to_bytes().to_array(),
            last_sigma.to_bytes().to_array()
        );
        // bases[2..15] = selector_comms[0..13]
        for i in 0..NUM_SELECTOR_COMMS {
            let want = g1_from_bytes(&env, &vk.selector_commitments[i]);
            assert_eq!(
                agg.bases.get(2 + i as u32).unwrap().to_bytes().to_array(),
                want.to_bytes().to_array(),
                "bases[{}] should be selector_comms[{i}]",
                2 + i
            );
        }
        // bases[15..20] = split_quot[0..5]
        for i in 0..NUM_WIRE_TYPES {
            let want = g1_from_bytes(&env, &proof.split_quot_commitments[i]);
            assert_eq!(
                agg.bases.get(15 + i as u32).unwrap().to_bytes().to_array(),
                want.to_bytes().to_array(),
                "bases[{}] should be split_quot[{i}]",
                15 + i
            );
        }
        // bases[20..25] = wire_commitments[0..5]
        for i in 0..NUM_WIRE_TYPES {
            let want = g1_from_bytes(&env, &proof.wire_commitments[i]);
            assert_eq!(
                agg.bases.get(20 + i as u32).unwrap().to_bytes().to_array(),
                want.to_bytes().to_array(),
                "bases[{}] should be wire[{i}]",
                20 + i
            );
        }
        // bases[25..29] = sigma_comms[0..4]
        for i in 0..NUM_WIRE_SIGMA_EVALS {
            let want = g1_from_bytes(&env, &vk.sigma_commitments[i]);
            assert_eq!(
                agg.bases.get(25 + i as u32).unwrap().to_bytes().to_array(),
                want.to_bytes().to_array(),
                "bases[{}] should be sigma[{i}]",
                25 + i
            );
        }
        // bases[29] = prod_perm (uv branch)
        assert_eq!(
            agg.bases.get(29).unwrap().to_bytes().to_array(),
            prod_perm.to_bytes().to_array(),
            "bases[29] should be prod_perm (uv branch)"
        );
    }

    /// Smoke-test for the `multi_scalar_multiply` helper: two
    /// back-to-back calls in the same process on the canonical
    /// depth-5 fixture (real on-curve G1 points, challenges derived
    /// from the real transcript, `vanish_eval` / `L_0(ζ)` computed
    /// the same way `verify` does) produce byte-equal `[D]_1`.
    ///
    /// **Scope is narrow.** Two calls in the same process can only
    /// catch *blatant* nondeterminism — uninitialized memory, an
    /// internal RNG, scratch state leaking across calls. Genuine
    /// cross-execution determinism is established by the
    /// consensus-deterministic Soroban host primitives (`g1_msm`,
    /// `fr_*`), not by this test. The end-to-end
    /// `verifier::tests::accepts_canonical_proof_d{5,8,11}` covers
    /// *correctness* of `[D]_1`; this one is a `≠` floor on
    /// *stability* in the same process — useful as a regression net,
    /// not as a determinism proof.
    #[test]
    fn msm_is_deterministic() {
        use crate::proof_format::{parse_proof_bytes, FR_LEN, PROOF_LEN};
        use crate::verifier_challenges::compute_challenges;
        use crate::verifier_polys::{
            evaluate_vanishing_poly, first_and_last_lagrange_coeffs, DomainParams,
        };
        use crate::vk_format::{parse_vk_bytes, G2_COMPRESSED_LEN, VK_LEN};
        use soroban_sdk::BytesN;

        const FIXTURE_VK: &[u8; VK_LEN] =
            include_bytes!("../tests/fixtures/vk-d5.bin");
        const FIXTURE_PROOF: &[u8; PROOF_LEN] =
            include_bytes!("../tests/fixtures/proof-d5.bin");
        const FIXTURE_SRS_G2: &[u8; G2_COMPRESSED_LEN] =
            include_bytes!("../tests/fixtures/srs-g2-compressed.bin");
        const FIXTURE_PI: &[u8; 2 * FR_LEN] =
            include_bytes!("../tests/fixtures/pi-d5.bin");

        let env = Env::default();
        let vk = parse_vk_bytes(FIXTURE_VK).expect("parse vk");
        let proof = parse_proof_bytes(FIXTURE_PROOF).expect("parse proof");

        let mut public_inputs = [[0u8; FR_LEN]; 2];
        public_inputs[0].copy_from_slice(&FIXTURE_PI[..FR_LEN]);
        public_inputs[1].copy_from_slice(&FIXTURE_PI[FR_LEN..]);

        let raw = compute_challenges(&env, &vk, FIXTURE_SRS_G2, &public_inputs, &proof);
        let challenges = ChallengesFr {
            beta: Fr::from_bytes(BytesN::from_array(&env, &raw.beta)),
            gamma: Fr::from_bytes(BytesN::from_array(&env, &raw.gamma)),
            alpha: Fr::from_bytes(BytesN::from_array(&env, &raw.alpha)),
            zeta: Fr::from_bytes(BytesN::from_array(&env, &raw.zeta)),
            v: Fr::from_bytes(BytesN::from_array(&env, &raw.v)),
            u: Fr::from_bytes(BytesN::from_array(&env, &raw.u)),
        };

        // Compute vanish_eval and L_0(ζ) the same way `verify` does
        // so the determinism check exercises real verify-path inputs,
        // not arbitrary placeholders.
        let params = DomainParams::for_size(&env, vk.domain_size);
        let vanish_eval = evaluate_vanishing_poly(&challenges.zeta, &params);
        let (lagrange_1_eval, _) =
            first_and_last_lagrange_coeffs(&challenges.zeta, &vanish_eval, &params);

        let agg_a = aggregate_poly_commitments(
            &env,
            &challenges,
            vanish_eval.clone(),
            lagrange_1_eval.clone(),
            &vk,
            &proof,
        );
        let agg_b = aggregate_poly_commitments(
            &env,
            &challenges,
            vanish_eval,
            lagrange_1_eval,
            &vk,
            &proof,
        );
        let msm_a = agg_a.multi_scalar_multiply(&env);
        let msm_b = agg_b.multi_scalar_multiply(&env);
        assert_eq!(
            msm_a.to_bytes().to_array(),
            msm_b.to_bytes().to_array(),
            "MSM is non-deterministic"
        );
    }

    /// Sanity check for the helper utilities — fr_one / fr_zero
    /// round-trip through to_bytes.
    #[test]
    fn helper_one_zero_round_trip() {
        let env = Env::default();
        let one_bytes = fr_one(&env).to_bytes().to_array();
        let mut expected_one = [0u8; 32];
        expected_one[31] = 0x01;
        assert_eq!(one_bytes, expected_one);

        let zero_bytes = fr_zero(&env).to_bytes().to_array();
        assert_eq!(zero_bytes, [0u8; 32]);
    }

}
