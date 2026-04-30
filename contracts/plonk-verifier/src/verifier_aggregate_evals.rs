//! Soroban-side port of `sep-xxxx-circuits::circuit::plonk::verifier_aggregate_evals`
//! (PR #183). Folds the proof's polynomial evaluations + the lin-poly
//! constant into a single Fr scalar `[E]_1` per Plonk paper Section
//! 8.4 step 11.
//!
//! For the no-Plookup, single-instance case our membership circuits
//! use, the aggregator is a 10-term linear combination:
//!
//! ```text
//!   E = −r_0
//!     + Σ_{i=0..5}  buffer[i]   · wires_evals[i]      // v¹..v⁵
//!     + Σ_{i=0..4}  buffer[5+i] · wire_sigma_evals[i] // v⁶..v⁹
//!     + buffer[9]               · perm_next_eval      // u
//! ```
//!
//! `buffer` is the 10-element `v_uv_buffer` produced by
//! [`super::verifier_aggregate::aggregate_poly_commitments`];
//! `r_0` is the lin-poly constant from
//! [`super::verifier_lin_poly::compute_lin_poly_constant_term`].

use soroban_sdk::crypto::bls12_381::Fr;
use soroban_sdk::{Env, Vec};

use crate::byte_helpers::{fr_from_le_bytes, fr_zero};
use crate::proof_format::{ParsedProof, NUM_WIRE_SIGMA_EVALS, NUM_WIRE_TYPES};

/// Total v_uv_buffer length: 5 wires + 4 sigmas + 1 perm_next.
pub const V_UV_BUFFER_LEN: u32 = (NUM_WIRE_TYPES + NUM_WIRE_SIGMA_EVALS + 1) as u32;

const _: () = assert!(V_UV_BUFFER_LEN == 10);

/// Compute the aggregated evaluation `[E]_1` scalar.
///
/// `v_uv_buffer` must be exactly 10 elements long and match the
/// `[v¹..v⁹, u]` layout produced by
/// [`super::verifier_aggregate::aggregate_poly_commitments`].
/// `proof` supplies the 5 wire / 4 sigma / 1 `perm_next_eval`
/// evaluations parsed via [`super::proof_format::parse_proof_bytes`];
/// we deserialise each from arkworks-LE form here.
///
/// Asserts the buffer length so a misshaped input fails fast rather
/// than silently dropping or duplicating terms.
pub fn aggregate_evaluations(
    env: &Env,
    lin_poly_constant: Fr,
    proof: &ParsedProof,
    v_uv_buffer: &Vec<Fr>,
) -> Fr {
    assert_eq!(
        v_uv_buffer.len(),
        V_UV_BUFFER_LEN,
        "v_uv_buffer length must be 10"
    );

    // Deserialise proof evaluations LE→Fr.
    let w_evals: [Fr; NUM_WIRE_TYPES] =
        core::array::from_fn(|i| fr_from_le_bytes(env, &proof.wires_evals[i]));
    let sigma_evals: [Fr; NUM_WIRE_SIGMA_EVALS] = core::array::from_fn(|i| {
        fr_from_le_bytes(env, &proof.wire_sigma_evals[i])
    });
    let perm_next_eval = fr_from_le_bytes(env, &proof.perm_next_eval);

    // Start with −r_0 (mirror of jf-plonk's
    // `let mut result: ScalarField = lin_poly_constant.neg();`).
    let mut result = fr_zero(env) - lin_poly_constant;

    // 5 wire evaluations × buffer[0..5].
    for i in 0..NUM_WIRE_TYPES {
        let coeff = v_uv_buffer.get(i as u32).expect("buffer index in range");
        result = result + coeff * w_evals[i].clone();
    }
    // 4 sigma evaluations × buffer[5..9].
    for i in 0..NUM_WIRE_SIGMA_EVALS {
        let coeff = v_uv_buffer
            .get((NUM_WIRE_TYPES + i) as u32)
            .expect("buffer index in range");
        result = result + coeff * sigma_evals[i].clone();
    }
    // perm_next_eval × buffer[9].
    let last_coeff = v_uv_buffer
        .get(V_UV_BUFFER_LEN - 1)
        .expect("buffer last index in range");
    result + last_coeff * perm_next_eval
}

// Fr byte conversion helpers live in `crate::byte_helpers`.

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::BytesN;

    /// Helper: build an Fr from a u64 (BE-encoded into a 32-byte
    /// slot's tail).
    fn fr(env: &Env, value: u64) -> Fr {
        let mut bytes = [0u8; 32];
        bytes[24..32].copy_from_slice(&value.to_be_bytes());
        Fr::from_bytes(BytesN::from_array(env, &bytes))
    }

    /// Build a synthetic ParsedProof carrying Fr-form evaluations
    /// transcribed via arkworks-LE → BE conversion (i.e. the slot's
    /// last byte holds the value's low byte after our LE→BE flip in
    /// `fr_from_le_bytes`). For values ≤ 255 this means
    /// `wires_evals[i][0] = (value as u8)`, other bytes zero.
    fn proof_with_fr_evals(
        w_values: &[u64; 5],
        sigma_values: &[u64; 4],
        perm_next_value: u64,
    ) -> ParsedProof {
        // Embed each u8-fitting value as the LE-low byte of a 32-byte
        // slot. Larger values would need full LE encoding; we use
        // small values for hand-computability.
        let mut p = ParsedProof {
            wire_commitments: [[0u8; 96]; 5],
            prod_perm_commitment: [0u8; 96],
            split_quot_commitments: [[0u8; 96]; 5],
            opening_proof: [0u8; 96],
            shifted_opening_proof: [0u8; 96],
            wires_evals: [[0u8; 32]; 5],
            wire_sigma_evals: [[0u8; 32]; 4],
            perm_next_eval: [0u8; 32],
        };
        for (i, &v) in w_values.iter().enumerate() {
            assert!(v < 256, "synthetic helper supports values < 256");
            p.wires_evals[i][0] = v as u8;
        }
        for (i, &v) in sigma_values.iter().enumerate() {
            assert!(v < 256);
            p.wire_sigma_evals[i][0] = v as u8;
        }
        assert!(perm_next_value < 256);
        p.perm_next_eval[0] = perm_next_value as u8;
        p
    }

    /// Build a 10-element `v_uv_buffer` from u64s.
    fn buffer_from_u64s(env: &Env, values: &[u64; 10]) -> Vec<Fr> {
        let mut buf: Vec<Fr> = Vec::new(env);
        for &v in values {
            buf.push_back(fr(env, v));
        }
        buf
    }

    /// Inline reference: `E = −r_0 + Σ buffer[i]·w[i] + Σ buffer[5+i]·σ[i] + buffer[9]·perm_next`.
    /// Different expression structure from the production for-loop:
    /// the indices are spelled out and the additions are eagerly
    /// chained. Catches wrong-buffer-index, wrong-slice-mapping, and
    /// wrong-sign-on-r_0 typos.
    fn reference_aggregate_evaluations(
        env: &Env,
        lin_const: Fr,
        w_evals: &[Fr; 5],
        sigma_evals: &[Fr; 4],
        perm_next: Fr,
        buffer: &[Fr; 10],
    ) -> Fr {
        let neg_r0 = fr_zero(env) - lin_const;
        neg_r0
            + buffer[0].clone() * w_evals[0].clone()
            + buffer[1].clone() * w_evals[1].clone()
            + buffer[2].clone() * w_evals[2].clone()
            + buffer[3].clone() * w_evals[3].clone()
            + buffer[4].clone() * w_evals[4].clone()
            + buffer[5].clone() * sigma_evals[0].clone()
            + buffer[6].clone() * sigma_evals[1].clone()
            + buffer[7].clone() * sigma_evals[2].clone()
            + buffer[8].clone() * sigma_evals[3].clone()
            + buffer[9].clone() * perm_next
    }

    /// Several seed sets — port output matches the unrolled inline
    /// reference. Catches the bug classes the reviewer flagged for
    /// the off-chain version: index / sign / off-by-one in the
    /// per-section loops.
    #[test]
    fn matches_inline_reference_for_diverse_inputs() {
        let env = Env::default();
        let cases: &[([u64; 5], [u64; 4], u64, u64, [u64; 10])] = &[
            (
                [10, 20, 30, 40, 50],
                [60, 70, 80, 90],
                100,
                42,
                [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            ),
            (
                [1, 1, 1, 1, 1],
                [1, 1, 1, 1],
                1,
                0,
                [11, 22, 33, 44, 55, 66, 77, 88, 99, 110],
            ),
            (
                [255, 254, 253, 252, 251],
                [250, 249, 248, 247],
                246,
                999,
                [200, 201, 202, 203, 204, 205, 206, 207, 208, 209],
            ),
        ];

        for (i, (ws, sigmas, perm, lin, buf)) in cases.iter().enumerate() {
            let proof = proof_with_fr_evals(ws, sigmas, *perm);
            let buffer = buffer_from_u64s(&env, buf);

            let lin_const = fr(&env, *lin);
            let ours = aggregate_evaluations(&env, lin_const.clone(), &proof, &buffer);

            // Build the reference inputs.
            let w_evals: [Fr; 5] = core::array::from_fn(|j| fr(&env, ws[j]));
            let sigma_evals: [Fr; 4] = core::array::from_fn(|j| fr(&env, sigmas[j]));
            let perm_next = fr(&env, *perm);
            let buf_arr: [Fr; 10] = core::array::from_fn(|j| fr(&env, buf[j]));
            let theirs = reference_aggregate_evaluations(
                &env,
                lin_const,
                &w_evals,
                &sigma_evals,
                perm_next,
                &buf_arr,
            );

            assert_eq!(
                ours.to_bytes().to_array(),
                theirs.to_bytes().to_array(),
                "case #{i}: aggregate_evaluations diverges from unrolled reference",
            );
        }
    }

    /// Symbolic spot-check: with all evals = 0 and all buffer entries
    /// = 0, the result is `−lin_poly_constant`. Pins the seed value.
    #[test]
    fn collapses_to_neg_lin_const_when_all_inputs_zero() {
        let env = Env::default();
        let proof = proof_with_fr_evals(&[0; 5], &[0; 4], 0);
        let buffer = buffer_from_u64s(&env, &[0; 10]);
        let lin_const = fr(&env, 12345);

        let result = aggregate_evaluations(&env, lin_const.clone(), &proof, &buffer);
        let expected = fr_zero(&env) - lin_const;
        assert_eq!(
            result.to_bytes().to_array(),
            expected.to_bytes().to_array(),
            "with zero evals/buffer, result should be −r_0",
        );
    }

    /// Hand-computed value:
    /// w = [10, 20, 30, 40, 50], σ = [60, 70, 80, 90], perm_next = 100,
    /// buffer = [1, 1, 1, 1, 1, 1, 1, 1, 1, 1], lin_const = 0.
    ///
    /// E = 0 + Σ w_i + Σ σ_j + perm_next
    ///   = (10+20+30+40+50) + (60+70+80+90) + 100
    ///   = 150 + 300 + 100 = 550.
    #[test]
    fn hand_computed_value_with_unit_buffer_and_zero_lin_const() {
        let env = Env::default();
        let proof = proof_with_fr_evals(&[10, 20, 30, 40, 50], &[60, 70, 80, 90], 100);
        let buffer = buffer_from_u64s(&env, &[1; 10]);
        let lin_const = fr(&env, 0);

        let result = aggregate_evaluations(&env, lin_const, &proof, &buffer);
        let expected = fr(&env, 550);
        assert_eq!(
            result.to_bytes().to_array(),
            expected.to_bytes().to_array(),
            "hand-computed E mismatch",
        );
    }

    /// Length guard fires on a too-short buffer.
    #[test]
    #[should_panic(expected = "v_uv_buffer length must be 10")]
    fn rejects_wrong_buffer_length() {
        let env = Env::default();
        let proof = proof_with_fr_evals(&[0; 5], &[0; 4], 0);
        let mut short_buf: Vec<Fr> = Vec::new(&env);
        for _ in 0..9 {
            short_buf.push_back(fr(&env, 0));
        }
        let _ = aggregate_evaluations(&env, fr(&env, 0), &proof, &short_buf);
    }
}
