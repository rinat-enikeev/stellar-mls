//! `aggregate_poly_commitments` port — produces the `(scalars, bases)`
//! list that, MSM'd, gives the batched-polynomial commitment `[D]_1`
//! the TurboPlonk verifier checks at pairing time.
//!
//! Mirrors jf-plonk's `Verifier::aggregate_poly_commitments`
//! (`plonk/src/proof_system/verifier.rs:439-535`) and
//! `linearization_scalars_and_bases`
//! (`plonk/src/proof_system/verifier.rs:541-713`) for the no-Plookup,
//! single-instance case. With those simplifications the function
//! emits exactly **30 `(scalar, base)` pairs**:
//!
//! | source                                | count | scalar        |
//! |---------------------------------------|------:|---------------|
//! | prod_perm_poly_comm (linearisation)   |     1 | `α²·L_0(ζ) + α·Π_{i=0..4}(β·k_i·ζ + γ + w_i)` |
//! | last sigma_comm (linearisation)       |     1 | `−α·β·z(ζ·g)·Π_{i=0..3}(β·σ_i + γ + w_i)`     |
//! | selector_comms (13)                   |    13 | `q_scalars[i]` (per jf-plonk's TurboPlonk gate map) |
//! | split_quot_comms (5)                  |     5 | `−Z_H(ζ), −Z_H(ζ)·ζ^{n+2}, …` (geometric) |
//! | wire commitments (5, v-combined)      |     5 | `v, v², v³, v⁴, v⁵` |
//! | sigma_comms[0..4] (v-combined)        |     4 | `v⁶ … v⁹` |
//! | prod_perm_poly_comm (uv-combined)     |     1 | `u·v` |
//!
//! and a `v_uv_buffer: Vec<Fr>` of length 10 holding the v-power and
//! u·v-power scalars in the order they were emitted (consumed later
//! by `aggregate_evaluations`).
//!
//! For BLS12-381 single-instance, no-Plookup membership circuits:
//! - `n = NUM_WIRE_TYPES = GATE_WIDTH + 1 = 5` (so 4 = `n − 1` for
//!   the "first wires" / "first sigmas" indices).
//! - `alpha_bases = [1]` (single instance ⇒ no per-instance scaling).
//! - `is_merged = false`.
//! - q_scalars table: 13 selector-polynomial scalars per the
//!   `q_lc, q_mul, q_hash, q_o, q_c, q_ecc` ordering jf-relation
//!   uses (`jf-relation/src/constants.rs::N_TURBO_PLONK_SELECTORS`).
//!
//! Soroban portability: the public surface accepts byte-form `ParsedProof`
//! / `ParsedVerifyingKey` (the parsers from PRs #174 / #177) and
//! Fr-form challenges (caller reduces via `Fr::from_be_bytes_mod_order`
//! upstream). It returns `(Vec<Fr>, Vec<G1Affine>)` plus the v_uv
//! buffer; the contract port re-derives `Vec<Fr>` and an MSM-ready
//! `Vec<BytesN<96>>` using its own host-fn arithmetic.

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::{Fr, G1Affine};
use ark_ec_v05::AffineRepr;
use ark_ff_v05::{Field, One, Zero};
use ark_serialize_v05::CanonicalDeserialize;

use crate::circuit::plonk::proof_format::{
    ParsedProof, NUM_WIRE_SIGMA_EVALS, NUM_WIRE_TYPES,
};
use crate::circuit::plonk::vk_format::{
    ParsedVerifyingKey, NUM_SELECTOR_COMMS, NUM_SIGMA_COMMS,
};

const _: () = assert!(NUM_WIRE_TYPES == 5);
const _: () = assert!(NUM_SIGMA_COMMS == 5);
const _: () = assert!(NUM_WIRE_SIGMA_EVALS == 4);
const _: () = assert!(NUM_SELECTOR_COMMS == 13);

/// Bundle of the six Fiat-Shamir challenges already reduced to `Fr`.
///
/// Caller reduces from raw BE bytes (output of
/// [`super::verifier_challenges::compute_challenges`]) via
/// `Fr::from_be_bytes_mod_order`.
#[derive(Clone, Copy, Debug)]
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
/// by the (forthcoming) `aggregate_evaluations`.
#[derive(Clone, Debug)]
pub struct AggregatedCommitments {
    /// Scalars to multiply the corresponding `bases` by.
    /// Length = 30 for our single-instance, no-Plookup case.
    pub scalars: Vec<Fr>,
    /// G1 commitments parsed from `ParsedProof` / `ParsedVerifyingKey`.
    /// Length = 30, paired with `scalars`.
    pub bases: Vec<G1Affine>,
    /// Powers of v (and u·v^k) in the order they were applied to the
    /// last 10 entries (5 wire commitments + 4 sigma commitments + 1
    /// prod_perm commitment in the uv branch). Consumed by
    /// `aggregate_evaluations` to derive the aggregate evaluation.
    pub v_uv_buffer: Vec<Fr>,
}

/// Multi-scalar-multiply this output to obtain the batched-polynomial
/// commitment `[D]_1`.
impl AggregatedCommitments {
    pub fn multi_scalar_multiply(&self) -> ark_bls12_381_v05::G1Projective {
        debug_assert_eq!(
            self.scalars.len(),
            self.bases.len(),
            "scalars/bases length mismatch"
        );
        let mut acc = ark_bls12_381_v05::G1Projective::zero();
        for (s, b) in self.scalars.iter().zip(self.bases.iter()) {
            acc += b.into_group() * s;
        }
        acc
    }
}

/// Parse an arkworks-uncompressed G1 byte slice into `G1Affine`.
/// Assumes upstream parser already validated structural form;
/// arkworks deserialiser performs on-curve / subgroup checks.
fn parse_g1_uncompressed(bytes: &[u8; 96]) -> G1Affine {
    G1Affine::deserialize_uncompressed(&bytes[..])
        .expect("ParsedVerifyingKey / ParsedProof bytes already structurally validated; arkworks deserialise produces canonical G1Affine")
}

/// Aggregate the verifier's polynomial commitments into the
/// MSM-ready `[D]_1`-form list of `(scalar, base)` pairs plus the
/// `v_uv_buffer` for the later evaluation aggregation step.
///
/// Single-instance, no-Plookup. Order of emitted pairs matches
/// jf-plonk's `linearization_scalars_and_bases` followed by the
/// `aggregate_poly_commitments` v/uv combiner loop.
pub fn aggregate_poly_commitments(
    challenges: ChallengesFr,
    vanish_eval: Fr,
    lagrange_1_eval: Fr,
    vk: &ParsedVerifyingKey,
    proof: &ParsedProof,
) -> AggregatedCommitments {
    // Convert challenges' Fr representations into Fr `k_i` constants
    // via the parser. k_constants are arkworks-LE in the parsed VK.
    let k_constants: [Fr; NUM_WIRE_TYPES] = std::array::from_fn(|i| {
        Fr::deserialize_uncompressed(&vk.k_constants[i][..])
            .expect("ParsedVerifyingKey::k_constants are already structurally validated")
    });

    // Wire and sigma evaluations (proof-side, LE Fr bytes).
    let w_evals: [Fr; NUM_WIRE_TYPES] = std::array::from_fn(|i| {
        Fr::deserialize_uncompressed(&proof.wires_evals[i][..])
            .expect("ParsedProof::wires_evals already structurally validated")
    });
    let sigma_evals: [Fr; NUM_WIRE_SIGMA_EVALS] = std::array::from_fn(|i| {
        Fr::deserialize_uncompressed(&proof.wire_sigma_evals[i][..])
            .expect("ParsedProof::wire_sigma_evals already structurally validated")
    });
    let perm_next_eval = Fr::deserialize_uncompressed(&proof.perm_next_eval[..])
        .expect("ParsedProof::perm_next_eval already structurally validated");

    // Parse G1 bases up-front so the scalar-loop body is pure Fr arith.
    let prod_perm_g1 = parse_g1_uncompressed(&proof.prod_perm_commitment);
    let split_quot_g1: [G1Affine; NUM_WIRE_TYPES] =
        std::array::from_fn(|i| parse_g1_uncompressed(&proof.split_quot_commitments[i]));
    let wire_g1: [G1Affine; NUM_WIRE_TYPES] =
        std::array::from_fn(|i| parse_g1_uncompressed(&proof.wire_commitments[i]));
    let selector_g1: [G1Affine; NUM_SELECTOR_COMMS] =
        std::array::from_fn(|i| parse_g1_uncompressed(&vk.selector_commitments[i]));
    let sigma_g1: [G1Affine; NUM_SIGMA_COMMS] =
        std::array::from_fn(|i| parse_g1_uncompressed(&vk.sigma_commitments[i]));

    let alpha = challenges.alpha;
    let beta = challenges.beta;
    let gamma = challenges.gamma;
    let zeta = challenges.zeta;
    let v = challenges.v;
    let u = challenges.u;

    // Capacity = 30 entries (1 perm + 1 last-sigma + 13 selectors + 5
    // split-quot + 5 wires + 4 sigmas + 1 prod_perm-uv).
    let mut scalars: Vec<Fr> = Vec::with_capacity(30);
    let mut bases: Vec<G1Affine> = Vec::with_capacity(30);
    let mut v_uv_buffer: Vec<Fr> = Vec::with_capacity(10);

    // ---------------------------------------------------------------
    // Linearisation part — `linearization_scalars_and_bases`.
    // ---------------------------------------------------------------

    // 1. Permutation product polynomial commitment.
    //    coeff = α²·L_0(ζ)
    //          + α · Π_{i=0..n-1} (β·k_i·ζ + γ + w_i)
    let alpha_squared = alpha.square();
    let perm_coeff = {
        let mut c = alpha_squared * lagrange_1_eval;
        c += w_evals
            .iter()
            .zip(k_constants.iter())
            .fold(alpha, |acc, (w, k)| acc * (beta * k * zeta + gamma + w));
        c
    };
    scalars.push(perm_coeff);
    bases.push(prod_perm_g1);

    // 2. Last sigma polynomial commitment (sigma_comms[NUM_WIRE_TYPES-1]).
    //    coeff = −α·β·z(ζ·g)·Π_{i=0..n-2} (β·σ_i + γ + w_i)
    //    (jf-plonk skips the last sigma in the product because it's the
    //    one being multiplied by this whole coefficient.)
    let last_sigma_coeff = {
        let init = alpha * beta * perm_next_eval;
        let prod = w_evals
            .iter()
            .take(NUM_WIRE_SIGMA_EVALS)
            .zip(sigma_evals.iter())
            .fold(init, |acc, (w, sigma)| acc * (beta * sigma + gamma + w));
        -prod
    };
    scalars.push(last_sigma_coeff);
    bases.push(sigma_g1[NUM_SIGMA_COMMS - 1]);

    // 3. Selector polynomial commitments — 13 entries per
    //    jf-relation's `N_TURBO_PLONK_SELECTORS` ordering.
    //    Layout: q_lc(0..3), q_mul(4..5), q_hash(6..9), q_o(10), q_c(11), q_ecc(12).
    let q_scalars: [Fr; NUM_SELECTOR_COMMS] = {
        let w0 = w_evals[0];
        let w1 = w_evals[1];
        let w2 = w_evals[2];
        let w3 = w_evals[3];
        let w4 = w_evals[4];
        [
            w0,                        // q_lc[0]:  w_0
            w1,                        // q_lc[1]:  w_1
            w2,                        // q_lc[2]:  w_2
            w3,                        // q_lc[3]:  w_3
            w0 * w1,                   // q_mul[0]: w_0·w_1
            w2 * w3,                   // q_mul[1]: w_2·w_3
            w0.pow([5u64]),            // q_hash[0]: w_0^5
            w1.pow([5u64]),            // q_hash[1]: w_1^5
            w2.pow([5u64]),            // q_hash[2]: w_2^5
            w3.pow([5u64]),            // q_hash[3]: w_3^5
            -w4,                       // q_o:      −w_4
            Fr::one(),                 // q_c:      1
            w0 * w1 * w2 * w3 * w4,    // q_ecc:    w_0·w_1·w_2·w_3·w_4
        ]
    };
    for (s, b) in q_scalars.iter().zip(selector_g1.iter()) {
        scalars.push(*s);
        bases.push(*b);
    }

    // 4. Split quotient commitments — 5 entries with geometric scaling.
    //    coeff_0 = −Z_H(ζ)
    //    coeff_i = coeff_{i-1} · ζ^{n+2}  (where ζ^{n+2} = (1 + Z_H(ζ))·ζ²)
    let zeta_to_n_plus_2 = (Fr::one() + vanish_eval) * zeta * zeta;
    let mut split_coeff = -vanish_eval;
    scalars.push(split_coeff);
    bases.push(split_quot_g1[0]);
    for poly in split_quot_g1.iter().skip(1) {
        split_coeff *= zeta_to_n_plus_2;
        scalars.push(split_coeff);
        bases.push(*poly);
    }

    // ---------------------------------------------------------------
    // v/uv combiner part — `aggregate_poly_commitments` body, no-Plookup.
    // ---------------------------------------------------------------

    // 5. Wire polynomial commitments — 5 entries scaled by v, v², …
    let mut v_base = v;
    for poly in wire_g1.iter() {
        v_uv_buffer.push(v_base);
        scalars.push(v_base);
        bases.push(*poly);
        v_base *= v;
    }

    // 6. First (n-1) sigma commitments — scaled by v^6, v^7, v^8, v^9.
    for poly in sigma_g1.iter().take(NUM_WIRE_SIGMA_EVALS) {
        v_uv_buffer.push(v_base);
        scalars.push(v_base);
        bases.push(*poly);
        v_base *= v;
    }

    // 7. prod_perm_poly_comm — scaled by u (the scalar `*= v`
    //    update only applies if subsequent uv-branch entries follow,
    //    e.g. Plookup commitments — none in our case).
    //    jf-plonk's `add_poly_comm` pushes `*random_combiner` BEFORE
    //    multiplying by `r`, so the first uv-branch entry sees the
    //    initial `uv_base = u`.
    let uv_base = u;
    v_uv_buffer.push(uv_base);
    scalars.push(uv_base);
    bases.push(prod_perm_g1);

    debug_assert_eq!(scalars.len(), 30, "expected 30 (scalar, base) pairs");
    debug_assert_eq!(bases.len(), 30);
    debug_assert_eq!(v_uv_buffer.len(), 10, "expected 10 v_uv buffer entries");

    AggregatedCommitments {
        scalars,
        bases,
        v_uv_buffer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bls12_381_v05::{Bls12_381, Fr};
    use ark_ff_v05::{BigInteger, PrimeField};
    use ark_serialize_v05::CanonicalSerialize;
    use jf_plonk::proof_system::structs::VerifyingKey;
    use jf_relation::PlonkCircuit;
    use rand_chacha::rand_core::SeedableRng;

    use crate::circuit::plonk::baker::{
        bake_membership_vk, build_canonical_membership_witness,
    };
    use crate::circuit::plonk::membership::synthesize_membership;
    use crate::circuit::plonk::proof_format::parse_proof_bytes;
    use crate::circuit::plonk::verifier_challenges::compute_challenges;
    use crate::circuit::plonk::verifier_polys::{
        evaluate_pi_poly, evaluate_vanishing_poly, first_and_last_lagrange_coeffs, DomainParams,
    };
    use crate::circuit::plonk::vk_format::{parse_vk_bytes, FR_LEN, G2_COMPRESSED_LEN};
    use crate::prover::plonk;

    /// Build a real proof at the given depth and return everything the
    /// verifier needs in one bundle.
    struct VerifierFixture {
        challenges: ChallengesFr,
        vanish_eval: Fr,
        lagrange_1_eval: Fr,
        parsed_vk: ParsedVerifyingKey,
        parsed_proof: ParsedProof,
        oracle_vk: VerifyingKey<Bls12_381>,
        oracle_proof: jf_plonk::proof_system::structs::Proof<Bls12_381>,
    }

    fn fixture(depth: usize) -> VerifierFixture {
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
        oracle_proof.serialize_uncompressed(&mut proof_bytes).unwrap();

        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let parsed_proof = parse_proof_bytes(&proof_bytes).expect("parse proof");

        let mut srs_g2_compressed = [0u8; G2_COMPRESSED_LEN];
        oracle_vk.open_key.powers_of_h[1]
            .serialize_compressed(&mut srs_g2_compressed[..])
            .unwrap();

        let public_inputs_fr = vec![witness.commitment, Fr::from(witness.epoch)];
        let public_inputs_be: Vec<[u8; FR_LEN]> = public_inputs_fr
            .iter()
            .map(|fr| {
                let bytes = fr.into_bigint().to_bytes_be();
                let mut arr = [0u8; FR_LEN];
                arr.copy_from_slice(&bytes);
                arr
            })
            .collect();

        let raw = compute_challenges(
            &parsed_vk,
            &srs_g2_compressed,
            &public_inputs_be,
            &parsed_proof,
        );
        let challenges = ChallengesFr {
            beta: Fr::from_be_bytes_mod_order(&raw.beta),
            gamma: Fr::from_be_bytes_mod_order(&raw.gamma),
            alpha: Fr::from_be_bytes_mod_order(&raw.alpha),
            zeta: Fr::from_be_bytes_mod_order(&raw.zeta),
            v: Fr::from_be_bytes_mod_order(&raw.v),
            u: Fr::from_be_bytes_mod_order(&raw.u),
        };

        let params = DomainParams::for_size(parsed_vk.domain_size);
        let vanish_eval = evaluate_vanishing_poly(challenges.zeta, &params);
        let (lagrange_1_eval, _) =
            first_and_last_lagrange_coeffs(challenges.zeta, vanish_eval, &params);

        // pi_eval is consumed by the lin-poly constant; we don't need
        // it for `aggregate_poly_commitments`.
        let _ = evaluate_pi_poly(&public_inputs_fr, challenges.zeta, vanish_eval, &params);

        VerifierFixture {
            challenges,
            vanish_eval,
            lagrange_1_eval,
            parsed_vk,
            parsed_proof,
            oracle_vk,
            oracle_proof,
        }
    }

    /// Output has the expected structural shape: 30 (scalar, base)
    /// pairs and a 10-entry v_uv buffer.
    #[test]
    fn output_shape_is_30_pairs_plus_10_buffer() {
        let f = fixture(5);
        let agg = aggregate_poly_commitments(
            f.challenges,
            f.vanish_eval,
            f.lagrange_1_eval,
            &f.parsed_vk,
            &f.parsed_proof,
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
        let f = fixture(5);
        let agg = aggregate_poly_commitments(
            f.challenges,
            f.vanish_eval,
            f.lagrange_1_eval,
            &f.parsed_vk,
            &f.parsed_proof,
        );

        // Selectors occupy entries [2..15] (after perm + last-sigma).
        let selector_scalars = &agg.scalars[2..15];

        // Reference: re-derive from raw wire evaluations.
        let w_evals: [Fr; 5] = std::array::from_fn(|i| {
            Fr::deserialize_uncompressed(&f.parsed_proof.wires_evals[i][..]).unwrap()
        });
        let expected = [
            w_evals[0],
            w_evals[1],
            w_evals[2],
            w_evals[3],
            w_evals[0] * w_evals[1],
            w_evals[2] * w_evals[3],
            w_evals[0].pow([5u64]),
            w_evals[1].pow([5u64]),
            w_evals[2].pow([5u64]),
            w_evals[3].pow([5u64]),
            -w_evals[4],
            Fr::one(),
            w_evals[0] * w_evals[1] * w_evals[2] * w_evals[3] * w_evals[4],
        ];

        for (i, (got, want)) in selector_scalars.iter().zip(expected.iter()).enumerate() {
            assert_eq!(got, want, "selector q_scalars[{i}] mismatch");
        }
    }

    /// The 5 split-quot scalars form a geometric progression with
    /// ratio `ζ^{n+2} = (1 + Z_H(ζ))·ζ²` starting at `−Z_H(ζ)`.
    #[test]
    fn split_quot_scalars_form_geometric_progression() {
        let f = fixture(5);
        let agg = aggregate_poly_commitments(
            f.challenges,
            f.vanish_eval,
            f.lagrange_1_eval,
            &f.parsed_vk,
            &f.parsed_proof,
        );

        // Split-quot scalars are at positions [15..20].
        let split = &agg.scalars[15..20];
        let zeta = f.challenges.zeta;
        let zeta_to_n_plus_2 = (Fr::one() + f.vanish_eval) * zeta * zeta;

        assert_eq!(split[0], -f.vanish_eval, "split[0] should be −Z_H(ζ)");
        for i in 1..5 {
            let expected = split[i - 1] * zeta_to_n_plus_2;
            assert_eq!(split[i], expected, "split[{i}] = split[{}]·ζ^(n+2)", i - 1);
        }
    }

    /// The v/uv combiner section produces the right power sequence:
    /// 5 wires at v..v⁵, 4 sigmas at v⁶..v⁹, 1 prod_perm at u
    /// (since `add_poly_comm` pushes the scalar before multiplying by
    /// v, the first uv-branch entry sees `uv_base = u`).
    #[test]
    fn v_uv_buffer_powers_match_v_and_uv() {
        let f = fixture(5);
        let agg = aggregate_poly_commitments(
            f.challenges,
            f.vanish_eval,
            f.lagrange_1_eval,
            &f.parsed_vk,
            &f.parsed_proof,
        );

        let v = f.challenges.v;
        let u = f.challenges.u;
        let mut expected_v = v;
        for i in 0..9 {
            assert_eq!(
                agg.v_uv_buffer[i], expected_v,
                "v_uv_buffer[{i}] should be v^{}",
                i + 1
            );
            expected_v *= v;
        }
        assert_eq!(agg.v_uv_buffer[9], u, "v_uv_buffer[9] should be u");
    }

    /// Bases are correctly drawn from `ParsedProof` and
    /// `ParsedVerifyingKey`. Spot-check key positions:
    /// - bases[0] = prod_perm_commitment from proof
    /// - bases[1] = sigma_comms[NUM_SIGMA_COMMS-1] from VK (last sigma)
    /// - bases[2..15] = selector_comms[0..13] from VK
    /// - bases[15..20] = split_quot_commitments[0..5] from proof
    /// - bases[20..25] = wire_commitments[0..5] from proof
    /// - bases[25..29] = sigma_comms[0..4] from VK (first 4 sigmas)
    /// - bases[29] = prod_perm_commitment from proof (again)
    #[test]
    fn bases_drawn_from_correct_sources() {
        let f = fixture(5);
        let agg = aggregate_poly_commitments(
            f.challenges,
            f.vanish_eval,
            f.lagrange_1_eval,
            &f.parsed_vk,
            &f.parsed_proof,
        );

        let prod_perm =
            G1Affine::deserialize_uncompressed(&f.parsed_proof.prod_perm_commitment[..]).unwrap();
        let last_sigma = G1Affine::deserialize_uncompressed(
            &f.parsed_vk.sigma_commitments[NUM_SIGMA_COMMS - 1][..],
        )
        .unwrap();

        assert_eq!(agg.bases[0], prod_perm, "bases[0] = prod_perm");
        assert_eq!(agg.bases[1], last_sigma, "bases[1] = last sigma");

        for i in 0..NUM_SELECTOR_COMMS {
            let sel = G1Affine::deserialize_uncompressed(
                &f.parsed_vk.selector_commitments[i][..],
            )
            .unwrap();
            assert_eq!(agg.bases[2 + i], sel, "bases[{}] = selector[{i}]", 2 + i);
        }

        for i in 0..NUM_WIRE_TYPES {
            let sq = G1Affine::deserialize_uncompressed(
                &f.parsed_proof.split_quot_commitments[i][..],
            )
            .unwrap();
            assert_eq!(agg.bases[15 + i], sq, "bases[{}] = split_quot[{i}]", 15 + i);

            let w =
                G1Affine::deserialize_uncompressed(&f.parsed_proof.wire_commitments[i][..])
                    .unwrap();
            assert_eq!(agg.bases[20 + i], w, "bases[{}] = wire[{i}]", 20 + i);
        }

        for i in 0..NUM_WIRE_SIGMA_EVALS {
            let sig =
                G1Affine::deserialize_uncompressed(&f.parsed_vk.sigma_commitments[i][..])
                    .unwrap();
            assert_eq!(agg.bases[25 + i], sig, "bases[{}] = sigma[{i}]", 25 + i);
        }

        assert_eq!(agg.bases[29], prod_perm, "bases[29] = prod_perm (uv branch)");
    }

    /// MSM result is non-zero on a real proof for all three tiers
    /// (sanity catch for "implementation always returns identity").
    /// Determinism: re-running on the same inputs produces an
    /// identical accumulator point.
    #[test]
    fn msm_is_nonzero_and_deterministic_for_all_tiers() {
        for &depth in &[5usize, 8, 11] {
            let f = fixture(depth);
            let agg = aggregate_poly_commitments(
                f.challenges,
                f.vanish_eval,
                f.lagrange_1_eval,
                &f.parsed_vk,
                &f.parsed_proof,
            );
            let msm_a = agg.multi_scalar_multiply();
            let msm_b = aggregate_poly_commitments(
                f.challenges,
                f.vanish_eval,
                f.lagrange_1_eval,
                &f.parsed_vk,
                &f.parsed_proof,
            )
            .multi_scalar_multiply();

            assert_eq!(msm_a, msm_b, "depth={depth} MSM is non-deterministic");
            assert!(
                !msm_a.is_zero(),
                "depth={depth} MSM is unexpectedly the identity element",
            );
        }
    }

    /// Tampering with a wire commitment changes the MSM result.
    /// Catches a bug where wire_commitments aren't actually consumed.
    #[test]
    fn msm_changes_when_wire_commitment_is_tampered() {
        let f = fixture(5);
        let agg_a = aggregate_poly_commitments(
            f.challenges,
            f.vanish_eval,
            f.lagrange_1_eval,
            &f.parsed_vk,
            &f.parsed_proof,
        );
        let mut tampered_proof = f.parsed_proof.clone();
        // Replace wire_commitments[0] with the prod_perm bytes — still
        // an on-curve G1 point (different from the original) so
        // arkworks deserialise won't reject. MSM must change.
        tampered_proof.wire_commitments[0] = tampered_proof.prod_perm_commitment;
        let agg_b = aggregate_poly_commitments(
            f.challenges,
            f.vanish_eval,
            f.lagrange_1_eval,
            &f.parsed_vk,
            &tampered_proof,
        );

        let msm_a = agg_a.multi_scalar_multiply();
        let msm_b = agg_b.multi_scalar_multiply();
        assert_ne!(
            msm_a, msm_b,
            "MSM didn't change when a wire commitment was swapped",
        );
    }

    // Suppress unused-field warnings from the fixture.
    #[allow(dead_code)]
    fn _silence(f: &VerifierFixture) {
        let _ = (&f.oracle_vk, &f.oracle_proof);
    }
}
