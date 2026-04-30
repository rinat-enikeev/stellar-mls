//! `aggregate_evaluations` port — folds the proof's polynomial
//! evaluations + the lin-poly constant term into a single Fr scalar
//! `[E]_1` per Plonk paper Section 8.4 step 11
//! (<https://eprint.iacr.org/2019/953.pdf>).
//!
//! Mirrors jf-plonk's `Verifier::aggregate_evaluations`
//! (`plonk/src/proof_system/verifier.rs:718-788`) for the no-Plookup,
//! single-instance case our membership circuits use. With those
//! simplifications the aggregator is a 10-term linear combination:
//!
//! ```text
//!   E = −r_0
//!     + Σ_{i=0..5}  buffer[i]   · wires_evals[i]      // v¹..v⁵
//!     + Σ_{i=0..4}  buffer[5+i] · wire_sigma_evals[i] // v⁶..v⁹
//!     + buffer[9]               · perm_next_eval      // u
//! ```
//!
//! where:
//! - `r_0 = lin_poly_constant` from [`super::verifier_lin_poly`],
//! - `buffer[..]` is `v_uv_buffer` from
//!   [`super::verifier_aggregate::AggregatedCommitments`] (10 Fr
//!   entries, the v¹..v⁹ + final `u` sequence the same module
//!   computes alongside the (scalar, base) MSM list).
//!
//! The output is the scalar that goes into the verifier's final
//! pairing check via `−E·[1]_1` (the `g` G1 generator).
//!
//! Soroban portability: pure Fr arithmetic. The contract port
//! re-implements the same formula using whatever Fr ops the host
//! provides.

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use ark_serialize_v05::CanonicalDeserialize;

use crate::circuit::plonk::proof_format::{
    ParsedProof, NUM_WIRE_SIGMA_EVALS, NUM_WIRE_TYPES,
};

/// Total v_uv_buffer length: 5 wires + 4 sigmas + 1 perm_next.
pub const V_UV_BUFFER_LEN: usize = NUM_WIRE_TYPES + NUM_WIRE_SIGMA_EVALS + 1;

const _: () = assert!(V_UV_BUFFER_LEN == 10);

/// Compute the aggregated evaluation `[E]_1` scalar.
///
/// `v_uv_buffer` must be exactly 10 elements long and follow the
/// layout produced by
/// [`super::verifier_aggregate::aggregate_poly_commitments`]:
/// `[v, v², v³, v⁴, v⁵, v⁶, v⁷, v⁸, v⁹, u]`. `proof` supplies the
/// 5 wire / 4 sigma / 1 `perm_next_eval` evaluations parsed via
/// `parse_proof_bytes` (PR #174); we deserialise each from its
/// arkworks-LE form here.
///
/// Asserts the buffer length so a misshaped input fails fast rather
/// than silently dropping or duplicating terms.
pub fn aggregate_evaluations(
    lin_poly_constant: Fr,
    proof: &ParsedProof,
    v_uv_buffer: &[Fr],
) -> Fr {
    assert_eq!(
        v_uv_buffer.len(),
        V_UV_BUFFER_LEN,
        "v_uv_buffer length must be {V_UV_BUFFER_LEN}"
    );

    // Deserialise proof evaluations LE→Fr.
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

    // Start with −r_0 (jf-plonk's `result: ScalarField = lin_poly_constant.neg()`).
    let mut result = -lin_poly_constant;

    // 5 wire evaluations × buffer[0..5].
    for i in 0..NUM_WIRE_TYPES {
        result += v_uv_buffer[i] * w_evals[i];
    }
    // 4 sigma evaluations × buffer[5..9].
    for i in 0..NUM_WIRE_SIGMA_EVALS {
        result += v_uv_buffer[NUM_WIRE_TYPES + i] * sigma_evals[i];
    }
    // perm_next_eval × buffer[9].
    result += v_uv_buffer[V_UV_BUFFER_LEN - 1] * perm_next_eval;

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bls12_381_v05::{Bls12_381, Fr};
    use ark_ff_v05::{BigInteger, PrimeField};
    use ark_serialize_v05::CanonicalSerialize;
    use ark_std_v05::UniformRand;
    use jf_plonk::proof_system::structs::VerifyingKey;
    use jf_relation::PlonkCircuit;
    use rand_chacha::rand_core::SeedableRng;

    use crate::circuit::plonk::baker::{
        bake_membership_vk, build_canonical_membership_witness,
    };
    use crate::circuit::plonk::membership::synthesize_membership;
    use crate::circuit::plonk::proof_format::parse_proof_bytes;
    use crate::circuit::plonk::verifier_aggregate::{
        aggregate_poly_commitments, ChallengesFr,
    };
    use crate::circuit::plonk::verifier_challenges::compute_challenges;
    use crate::circuit::plonk::verifier_lin_poly::compute_lin_poly_constant_term;
    use crate::circuit::plonk::verifier_polys::{
        evaluate_pi_poly, evaluate_vanishing_poly, first_and_last_lagrange_coeffs, DomainParams,
    };
    use crate::circuit::plonk::vk_format::{parse_vk_bytes, FR_LEN, G2_COMPRESSED_LEN};
    use crate::prover::plonk;

    /// Reference inline transcription of jf-plonk's
    /// `aggregate_evaluations` for the no-Plookup, single-instance
    /// case. Used as oracle so a typo in the port (wrong index, wrong
    /// sign, missing perm_next term) shows up.
    fn reference_aggregate_evaluations(
        lin_poly_constant: Fr,
        w_evals: &[Fr; 5],
        sigma_evals: &[Fr; 4],
        perm_next: Fr,
        buffer: &[Fr; 10],
    ) -> Fr {
        let mut result = -lin_poly_constant;
        let mut iter = buffer.iter().copied();
        for &w in w_evals.iter() {
            result += iter.next().unwrap() * w;
        }
        for &s in sigma_evals.iter() {
            result += iter.next().unwrap() * s;
        }
        result += iter.next().unwrap() * perm_next;
        assert!(iter.next().is_none(), "buffer not fully consumed");
        result
    }

    /// Random Fr inputs — the port matches the inline reference for
    /// 20 reps. Catches index / sign / off-by-one bugs.
    #[test]
    fn matches_inline_reference_for_random_inputs() {
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([23u8; 32]);
        for _ in 0..20 {
            let lin_const = Fr::rand(&mut rng);
            let w_evals: [Fr; 5] = std::array::from_fn(|_| Fr::rand(&mut rng));
            let sigma_evals: [Fr; 4] = std::array::from_fn(|_| Fr::rand(&mut rng));
            let perm_next = Fr::rand(&mut rng);
            let buffer: [Fr; 10] = std::array::from_fn(|_| Fr::rand(&mut rng));

            // Stuff Fr evaluations into a synthetic ParsedProof.
            let mut synthetic = ParsedProof {
                wire_commitments: [[0u8; 96]; 5],
                prod_perm_commitment: [0u8; 96],
                split_quot_commitments: [[0u8; 96]; 5],
                opening_proof: [0u8; 96],
                shifted_opening_proof: [0u8; 96],
                wires_evals: [[0u8; 32]; 5],
                wire_sigma_evals: [[0u8; 32]; 4],
                perm_next_eval: [0u8; 32],
            };
            for i in 0..5 {
                let mut bytes = [0u8; 32];
                w_evals[i].serialize_uncompressed(&mut bytes[..]).unwrap();
                synthetic.wires_evals[i] = bytes;
            }
            for i in 0..4 {
                let mut bytes = [0u8; 32];
                sigma_evals[i].serialize_uncompressed(&mut bytes[..]).unwrap();
                synthetic.wire_sigma_evals[i] = bytes;
            }
            let mut bytes = [0u8; 32];
            perm_next.serialize_uncompressed(&mut bytes[..]).unwrap();
            synthetic.perm_next_eval = bytes;

            let ours = aggregate_evaluations(lin_const, &synthetic, &buffer);
            let theirs = reference_aggregate_evaluations(
                lin_const,
                &w_evals,
                &sigma_evals,
                perm_next,
                &buffer,
            );
            assert_eq!(ours, theirs, "aggregate_evaluations mismatch on random inputs");
        }
    }

    /// Wrong buffer length panics — so a misshaped input fails fast
    /// rather than silently producing wrong arithmetic.
    #[test]
    #[should_panic(expected = "v_uv_buffer length must be 10")]
    fn rejects_wrong_buffer_length() {
        let synthetic = ParsedProof {
            wire_commitments: [[0u8; 96]; 5],
            prod_perm_commitment: [0u8; 96],
            split_quot_commitments: [[0u8; 96]; 5],
            opening_proof: [0u8; 96],
            shifted_opening_proof: [0u8; 96],
            wires_evals: [[0u8; 32]; 5],
            wire_sigma_evals: [[0u8; 32]; 4],
            perm_next_eval: [0u8; 32],
        };
        let buffer = [Fr::from(1u64); 9]; // one short
        let _ = aggregate_evaluations(Fr::from(0u64), &synthetic, &buffer);
    }

    /// End-to-end-ish: build a real proof, run the full prereq chain
    /// (challenges → polys → lin_poly_constant → aggregate_poly_commitments
    /// → aggregate_evaluations), and assert the output matches the
    /// inline reference for all three tiers. Sanity-checks
    /// integration of every prereq module.
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
            let z_h = evaluate_vanishing_poly(challenges.zeta, &params);
            let (l_0, _l_n) =
                first_and_last_lagrange_coeffs(challenges.zeta, z_h, &params);
            let pi_eval =
                evaluate_pi_poly(&public_inputs_fr, challenges.zeta, z_h, &params);

            let w_evals: [Fr; 5] = std::array::from_fn(|i| {
                Fr::deserialize_uncompressed(&parsed_proof.wires_evals[i][..]).unwrap()
            });
            let sigma_evals: [Fr; 4] = std::array::from_fn(|i| {
                Fr::deserialize_uncompressed(&parsed_proof.wire_sigma_evals[i][..])
                    .unwrap()
            });
            let perm_next =
                Fr::deserialize_uncompressed(&parsed_proof.perm_next_eval[..]).unwrap();

            let lin_const = compute_lin_poly_constant_term(
                challenges.alpha,
                challenges.beta,
                challenges.gamma,
                pi_eval,
                l_0,
                &w_evals,
                &sigma_evals,
                perm_next,
            );

            let agg = aggregate_poly_commitments(
                challenges, z_h, l_0, &parsed_vk, &parsed_proof,
            );
            let buffer: [Fr; 10] = std::array::from_fn(|i| agg.v_uv_buffer[i]);

            let ours = aggregate_evaluations(lin_const, &parsed_proof, &agg.v_uv_buffer);
            let theirs = reference_aggregate_evaluations(
                lin_const,
                &w_evals,
                &sigma_evals,
                perm_next,
                &buffer,
            );
            assert_eq!(
                ours, theirs,
                "depth={depth} aggregate_evaluations diverges from reference",
            );

            // Sanity: result is non-zero (defensive — random Fr is
            // overwhelmingly unlikely to land on 0 by accident).
            assert_ne!(
                ours,
                Fr::from(0u64),
                "depth={depth} aggregate_evaluations unexpectedly zero",
            );
        }
    }

    /// Cross-check vs typed jf-plonk struct fields: same idea as
    /// `verifier_aggregate::msm_matches_typed_oracle_for_all_tiers`,
    /// but for the scalar evaluation. Catches byte-parser indexing /
    /// LE-BE conversion bugs in `wires_evals` / `wire_sigma_evals` /
    /// `perm_next_eval`.
    #[test]
    fn matches_typed_oracle_for_all_tiers() {
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
            let z_h = evaluate_vanishing_poly(challenges.zeta, &params);
            let (l_0, _) = first_and_last_lagrange_coeffs(challenges.zeta, z_h, &params);
            let pi_eval =
                evaluate_pi_poly(&public_inputs_fr, challenges.zeta, z_h, &params);

            let w_evals: [Fr; 5] = std::array::from_fn(|i| {
                Fr::deserialize_uncompressed(&parsed_proof.wires_evals[i][..]).unwrap()
            });
            let sigma_evals: [Fr; 4] = std::array::from_fn(|i| {
                Fr::deserialize_uncompressed(&parsed_proof.wire_sigma_evals[i][..])
                    .unwrap()
            });
            let perm_next =
                Fr::deserialize_uncompressed(&parsed_proof.perm_next_eval[..]).unwrap();

            let lin_const = compute_lin_poly_constant_term(
                challenges.alpha,
                challenges.beta,
                challenges.gamma,
                pi_eval,
                l_0,
                &w_evals,
                &sigma_evals,
                perm_next,
            );
            let agg = aggregate_poly_commitments(
                challenges, z_h, l_0, &parsed_vk, &parsed_proof,
            );

            let ours = aggregate_evaluations(lin_const, &parsed_proof, &agg.v_uv_buffer);

            // Typed oracle: walk jf-plonk's `Proof<E>::poly_evals`
            // directly (no byte parsing) using the same buffer layout.
            let typed_oracle = {
                let pe = &oracle_proof.poly_evals;
                let mut result = -lin_const;
                let mut idx = 0;
                for &w in pe.wires_evals.iter() {
                    result += agg.v_uv_buffer[idx] * w;
                    idx += 1;
                }
                for &s in pe.wire_sigma_evals.iter() {
                    result += agg.v_uv_buffer[idx] * s;
                    idx += 1;
                }
                result += agg.v_uv_buffer[idx] * pe.perm_next_eval;
                result
            };

            assert_eq!(
                ours, typed_oracle,
                "depth={depth} byte-form aggregate_evaluations diverges from typed oracle",
            );
        }
    }
}
