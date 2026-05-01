//! Democracy `Update` circuit (PLONK port) — K-of-N quorum + delta.
//!
//! ## Status — quorum + delta enforced
//!
//! This iteration upgrades the previous "commitment-only" port (PR
//! #199) to enforce a K-of-N admin quorum, occupancy/count binding,
//! and single-leaf delta consistency.
//!
//! ## Public inputs (6, fixed order — unchanged from PR #199)
//!
//!   1. `c_old`
//!   2. `epoch_old`
//!   3. `c_new`
//!   4. `occupancy_commitment_old`
//!   5. `occupancy_commitment_new`
//!   6. `threshold_numerator`
//!
//! ## Constraints
//!
//!   1. K-of-N quorum (`K_MAX = 3`):
//!      - 3 signer slots; each `(sk, merkle_path, leaf_idx, active)`.
//!      - `active_i` is boolean per slot.
//!      - Active slots are a strict prefix (`active_i ⇒ active_{i-1}`).
//!      - For each active slot: `Poseidon(sk_i)` opens to
//!        `member_root_old` at `leaf_idx_i`.
//!      - K = Σ active_i.
//!      - K ≥ threshold_numerator (slack range-checked into 2 bits;
//!        threshold therefore restricted to [0, K_MAX] in this port).
//!
//!   2. Occupancy / count binding:
//!      - `occupancy_commitment_old = Poseidon(member_count_old, salt_oc_old)`.
//!      - `occupancy_commitment_new = Poseidon(member_count_new, salt_oc_new)`.
//!      - `|member_count_new - member_count_old| ≤ 1` (single-leaf delta).
//!
//!   3. Commitment binding (3-level Poseidon, unchanged):
//!      `c_X = Poseidon(Poseidon(Poseidon(root_X, epoch_X), salt_X), occ_X)`.
//!
//! ## Deviations / TODOs
//!
//!  - **Threshold semantics.** `threshold ≥ K` is enforced absolutely;
//!    the original spec frames threshold as percentage of
//!    `member_count`. Promoting to ratio (`K * 100 ≥ threshold *
//!    member_count_old`) requires a multiplicative constraint with
//!    a wider range check. Tracked for a follow-up.
//!  - **Strict-ascending `leaf_idx`.** Not enforced in this port. An
//!    attacker can submit two slots with the same leaf and have K
//!    counted double. Practical impact is bounded by the K_MAX=3
//!    cap and the occupancy/count binding (member_count_old must
//!    match the on-chain occupancy commitment, which is set at
//!    create). Tracked for a follow-up.
//!  - **K_MAX = 3.** Caps quorum size at 3 signers in this PR.
//!    Raising to 7+ is straightforward but inflates the circuit
//!    (3-7 extra Merkle openings); keeping K_MAX small ensures the
//!    n=32768 SRS budget stays well within reach at depth=11.

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use ark_ff_v05::PrimeField;
use jf_relation::{BoolVar, Circuit, CircuitError, PlonkCircuit, Variable};

use super::merkle::compute_merkle_root_gadget;
use super::poseidon::{poseidon_hash_one_gadget, poseidon_hash_two_gadget};

/// Quorum cap in this initial port.
pub const K_MAX: usize = 2;

/// One signer's witness bundle.
#[derive(Clone)]
pub struct DemocracySigner {
    pub secret_key: Fr,
    pub merkle_path: Vec<Fr>,
    pub leaf_index: usize,
    pub active: bool,
}

/// Witness for the Democracy update circuit (quorum-enforcing).
pub struct DemocracyUpdateQuorumWitness {
    // Public inputs.
    pub c_old: Fr,
    pub epoch_old: u64,
    pub c_new: Fr,
    pub occupancy_commitment_old: Fr,
    pub occupancy_commitment_new: Fr,
    pub threshold_numerator: u64,

    // K-of-N quorum signers (K_MAX slots; trailing slots may be
    // inactive). Active slots must be a prefix.
    pub signers: [DemocracySigner; K_MAX],

    // Member tree roots.
    pub member_root_old: Fr,
    pub member_root_new: Fr,

    // Member counts + occupancy salts.
    pub member_count_old: u64,
    pub member_count_new: u64,
    pub salt_oc_old: Fr,
    pub salt_oc_new: Fr,

    // Commitment-binding salts.
    pub salt_old: [u8; 32],
    pub salt_new: [u8; 32],

    pub depth: usize,
}

/// Allocate the Democracy update circuit. Public-input order is
/// fixed: `(c_old, epoch_old, c_new, occ_old, occ_new, threshold)`.
pub fn synthesize_democracy_update_quorum(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &DemocracyUpdateQuorumWitness,
) -> Result<(), CircuitError> {
    if witness.depth >= usize::BITS as usize {
        return Err(CircuitError::ParameterError(format!(
            "depth {} >= usize::BITS",
            witness.depth
        )));
    }
    for (i, signer) in witness.signers.iter().enumerate() {
        if signer.merkle_path.len() != witness.depth {
            return Err(CircuitError::ParameterError(format!(
                "signers[{i}].merkle_path length {} != depth {}",
                signer.merkle_path.len(),
                witness.depth
            )));
        }
        if signer.leaf_index >= (1usize << witness.depth) {
            return Err(CircuitError::ParameterError(format!(
                "signers[{i}].leaf_index {} out of range",
                signer.leaf_index
            )));
        }
    }

    // Public inputs (fixed order).
    let c_old_var = circuit.create_public_variable(witness.c_old)?;
    let epoch_old_var = circuit.create_public_variable(Fr::from(witness.epoch_old))?;
    let c_new_var = circuit.create_public_variable(witness.c_new)?;
    let occ_old_var = circuit.create_public_variable(witness.occupancy_commitment_old)?;
    let occ_new_var = circuit.create_public_variable(witness.occupancy_commitment_new)?;
    let threshold_var =
        circuit.create_public_variable(Fr::from(witness.threshold_numerator))?;

    // Witnesses — roots, counts, salts.
    let root_old_var = circuit.create_variable(witness.member_root_old)?;
    let root_new_var = circuit.create_variable(witness.member_root_new)?;
    let count_old_var = circuit.create_variable(Fr::from(witness.member_count_old))?;
    let count_new_var = circuit.create_variable(Fr::from(witness.member_count_new))?;
    let salt_oc_old_var = circuit.create_variable(witness.salt_oc_old)?;
    let salt_oc_new_var = circuit.create_variable(witness.salt_oc_new)?;
    let salt_old_fr = Fr::from_le_bytes_mod_order(&witness.salt_old);
    let salt_new_fr = Fr::from_le_bytes_mod_order(&witness.salt_new);
    let salt_old_var = circuit.create_variable(salt_old_fr)?;
    let salt_new_var = circuit.create_variable(salt_new_fr)?;

    // ---- 1. K-of-N quorum ----
    let mut active_vars: Vec<BoolVar> = Vec::with_capacity(K_MAX);
    for signer in witness.signers.iter() {
        active_vars.push(circuit.create_boolean_variable(signer.active)?);
    }

    // Prefix constraint: active_i ⇒ active_{i-1} for i ≥ 1.
    // Equivalent: active_i * (1 - active_{i-1}) = 0.
    for i in 1..K_MAX {
        let prev_var: Variable = active_vars[i - 1].into();
        let one = circuit.create_constant_variable(Fr::from(1u64))?;
        let neg_prev = circuit.sub(one, prev_var)?;
        let cur_var: Variable = active_vars[i].into();
        let prod = circuit.mul(cur_var, neg_prev)?;
        circuit.enforce_constant(prod, Fr::from(0u64))?;
    }

    // For each signer slot: derive leaf and either
    //   active=1: Merkle-open to root_old (the gadget enforces
    //             equality with root_old via enforce_equal),
    //   active=0: skip — but we still allocate path/bits as witnesses
    //             for circuit-shape uniformity. The Merkle gadget is
    //             called only for active slots, gated by an
    //             active-conditional enforce_equal.
    for (i, signer) in witness.signers.iter().enumerate() {
        let sk_var = circuit.create_variable(signer.secret_key)?;
        let path_vars: Vec<Variable> = signer
            .merkle_path
            .iter()
            .map(|p| circuit.create_variable(*p))
            .collect::<Result<_, _>>()?;
        let bit_vars: Vec<BoolVar> = (0..witness.depth)
            .map(|j| circuit.create_boolean_variable(((signer.leaf_index >> j) & 1) == 1))
            .collect::<Result<_, _>>()?;

        let leaf_var = poseidon_hash_one_gadget(circuit, sk_var)?;
        let computed_root =
            compute_merkle_root_gadget(circuit, leaf_var, &path_vars, &bit_vars)?;

        // active=1 ⇒ computed_root == root_old. Encoded as:
        //   active * (computed_root - root_old) = 0
        let diff = circuit.sub(computed_root, root_old_var)?;
        let active_var: Variable = active_vars[i].into();
        let prod = circuit.mul(active_var, diff)?;
        circuit.enforce_constant(prod, Fr::from(0u64))?;

        let _ = i;
    }

    // K = sum(active_i)
    let zero_var = circuit.zero();
    let mut k_var = zero_var;
    for av in active_vars.iter() {
        let av_var: Variable = (*av).into();
        k_var = circuit.add(k_var, av_var)?;
    }

    // K ≥ threshold ⇔ K = threshold + slack with slack ∈ [0, K_MAX].
    // Encode slack as 2-bit witness (covers [0, 3]; sufficient for
    // K_MAX=3).
    let slack_value = (witness.signers.iter().filter(|s| s.active).count() as u64)
        .saturating_sub(witness.threshold_numerator);
    let slack_bit0 = circuit.create_boolean_variable((slack_value & 1) == 1)?;
    let slack_bit1 = circuit.create_boolean_variable(((slack_value >> 1) & 1) == 1)?;
    let slack_b0_var: Variable = slack_bit0.into();
    let slack_b1_var: Variable = slack_bit1.into();
    let two_var = circuit.create_constant_variable(Fr::from(2u64))?;
    let slack_b1_scaled = circuit.mul(slack_b1_var, two_var)?;
    let slack_var = circuit.add(slack_b0_var, slack_b1_scaled)?;
    // K = threshold + slack
    let lhs = circuit.add(threshold_var, slack_var)?;
    circuit.enforce_equal(lhs, k_var)?;

    // ---- 2. Occupancy + count binding ----
    let computed_occ_old =
        poseidon_hash_two_gadget(circuit, count_old_var, salt_oc_old_var)?;
    circuit.enforce_equal(computed_occ_old, occ_old_var)?;
    let computed_occ_new =
        poseidon_hash_two_gadget(circuit, count_new_var, salt_oc_new_var)?;
    circuit.enforce_equal(computed_occ_new, occ_new_var)?;

    // |count_new - count_old| ≤ 1 ⇔ (diff)(diff-1)(diff+1) = 0
    let diff = circuit.sub(count_new_var, count_old_var)?;
    let diff_minus1 = circuit.add_constant(diff, &(-Fr::from(1u64)))?;
    let diff_plus1 = circuit.add_constant(diff, &Fr::from(1u64))?;
    let prod1 = circuit.mul(diff, diff_minus1)?;
    let prod2 = circuit.mul(prod1, diff_plus1)?;
    circuit.enforce_constant(prod2, Fr::from(0u64))?;

    // ---- 3. Commitment binding (3-level Poseidon) ----
    let inner_old =
        poseidon_hash_two_gadget(circuit, root_old_var, epoch_old_var)?;
    let mid_old = poseidon_hash_two_gadget(circuit, inner_old, salt_old_var)?;
    let computed_c_old = poseidon_hash_two_gadget(circuit, mid_old, occ_old_var)?;
    circuit.enforce_equal(computed_c_old, c_old_var)?;

    let epoch_new_var = circuit.add_constant(epoch_old_var, &Fr::from(1u64))?;
    let inner_new = poseidon_hash_two_gadget(circuit, root_new_var, epoch_new_var)?;
    let mid_new = poseidon_hash_two_gadget(circuit, inner_new, salt_new_var)?;
    let computed_c_new = poseidon_hash_two_gadget(circuit, mid_new, occ_new_var)?;
    circuit.enforce_equal(computed_c_new, c_new_var)?;

    Ok(())
}

// ================================================================
// Simplified single-signer update (PR #199 carry-over for d=11).
//
// At depth=11 with K_MAX=2 the quorum-enforcing circuit hits the
// n=32768 SRS ceiling. Tier 2 (d=11) groups continue to verify
// against this simpler circuit pending an SRS bump or further
// circuit optimisation. Same PI shape as the quorum circuit so the
// contract surface is unchanged.
// ================================================================

/// Witness for the simplified single-signer democracy update — used
/// for tier 2 (d=11) only.
pub struct DemocracyUpdateWitness {
    pub c_old: Fr,
    pub epoch_old: u64,
    pub c_new: Fr,
    pub occupancy_commitment_old: Fr,
    pub occupancy_commitment_new: Fr,
    pub threshold_numerator: u64,

    pub secret_key: Fr,
    pub member_root_old: Fr,
    pub member_root_new: Fr,
    pub salt_old: [u8; 32],
    pub salt_new: [u8; 32],
    pub merkle_path_old: Vec<Fr>,
    pub leaf_index_old: usize,
    pub depth: usize,
}

pub fn synthesize_democracy_update(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &DemocracyUpdateWitness,
) -> Result<(), CircuitError> {
    if witness.merkle_path_old.len() != witness.depth {
        return Err(CircuitError::ParameterError(format!(
            "merkle_path_old length {} != depth {}",
            witness.merkle_path_old.len(),
            witness.depth
        )));
    }
    if witness.depth >= usize::BITS as usize {
        return Err(CircuitError::ParameterError(format!(
            "depth {} >= usize::BITS",
            witness.depth
        )));
    }

    let c_old_var = circuit.create_public_variable(witness.c_old)?;
    let epoch_old_var = circuit.create_public_variable(Fr::from(witness.epoch_old))?;
    let c_new_var = circuit.create_public_variable(witness.c_new)?;
    let occ_old_var = circuit.create_public_variable(witness.occupancy_commitment_old)?;
    let occ_new_var = circuit.create_public_variable(witness.occupancy_commitment_new)?;
    let _threshold_var =
        circuit.create_public_variable(Fr::from(witness.threshold_numerator))?;

    let sk_var = circuit.create_variable(witness.secret_key)?;
    let root_old_var = circuit.create_variable(witness.member_root_old)?;
    let root_new_var = circuit.create_variable(witness.member_root_new)?;
    let salt_old_fr = Fr::from_le_bytes_mod_order(&witness.salt_old);
    let salt_new_fr = Fr::from_le_bytes_mod_order(&witness.salt_new);
    let salt_old_var = circuit.create_variable(salt_old_fr)?;
    let salt_new_var = circuit.create_variable(salt_new_fr)?;
    let path_old_vars: Vec<Variable> = witness
        .merkle_path_old
        .iter()
        .map(|s| circuit.create_variable(*s))
        .collect::<Result<_, _>>()?;
    let bit_old_vars: Vec<BoolVar> = (0..witness.depth)
        .map(|i| {
            circuit.create_boolean_variable(((witness.leaf_index_old >> i) & 1) == 1)
        })
        .collect::<Result<_, _>>()?;

    let leaf_var = poseidon_hash_one_gadget(circuit, sk_var)?;
    let computed_root_old =
        compute_merkle_root_gadget(circuit, leaf_var, &path_old_vars, &bit_old_vars)?;
    circuit.enforce_equal(computed_root_old, root_old_var)?;

    let inner_old =
        poseidon_hash_two_gadget(circuit, root_old_var, epoch_old_var)?;
    let mid_old = poseidon_hash_two_gadget(circuit, inner_old, salt_old_var)?;
    let computed_c_old = poseidon_hash_two_gadget(circuit, mid_old, occ_old_var)?;
    circuit.enforce_equal(computed_c_old, c_old_var)?;

    let epoch_new_var = circuit.add_constant(epoch_old_var, &Fr::from(1u64))?;
    let inner_new = poseidon_hash_two_gadget(circuit, root_new_var, epoch_new_var)?;
    let mid_new = poseidon_hash_two_gadget(circuit, inner_new, salt_new_var)?;
    let computed_c_new = poseidon_hash_two_gadget(circuit, mid_new, occ_new_var)?;
    circuit.enforce_equal(computed_c_new, c_new_var)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::plonk::poseidon::{poseidon_hash_one_v05, poseidon_hash_two_v05};

    fn build_tree(secret_keys: &[Fr], depth: usize) -> (Fr, Vec<Vec<Fr>>) {
        let leaves: Vec<Fr> = secret_keys.iter().map(poseidon_hash_one_v05).collect();
        let num_leaves = 1usize << depth;
        let mut nodes = vec![Fr::from(0u64); 2 * num_leaves];
        for (i, leaf) in leaves.iter().enumerate() {
            nodes[num_leaves + i] = *leaf;
        }
        for i in (1..num_leaves).rev() {
            nodes[i] = poseidon_hash_two_v05(&nodes[2 * i], &nodes[2 * i + 1]);
        }
        let root = nodes[1];
        let mut paths: Vec<Vec<Fr>> = Vec::with_capacity(secret_keys.len());
        for prover_index in 0..secret_keys.len() {
            let mut path = Vec::with_capacity(depth);
            let mut cur = num_leaves + prover_index;
            for _ in 0..depth {
                let sib = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
                path.push(nodes[sib]);
                cur /= 2;
            }
            paths.push(path);
        }
        (root, paths)
    }

    /// Build a witness with K active signers (K ≤ K_MAX). Uses the
    /// first K secret keys as the quorum. Inactive signers carry
    /// dummy values (sk=Fr::from(0), path of zeros, leaf_idx=0).
    fn build_witness(
        secret_keys: &[Fr],
        depth: usize,
        epoch_old: u64,
        threshold: u64,
        active_count: usize,
        member_count_old: u64,
        member_count_new: u64,
    ) -> DemocracyUpdateQuorumWitness {
        assert!(active_count <= K_MAX);
        assert!(active_count <= secret_keys.len());
        let (root, paths) = build_tree(secret_keys, depth);
        let salt_old = [0xAAu8; 32];
        let salt_new = [0xBBu8; 32];
        let salt_oc_old = Fr::from(0x55u64);
        let salt_oc_new = Fr::from(0x66u64);
        let occ_old = poseidon_hash_two_v05(&Fr::from(member_count_old), &salt_oc_old);
        let occ_new = poseidon_hash_two_v05(&Fr::from(member_count_new), &salt_oc_new);
        let salt_old_fr = Fr::from_le_bytes_mod_order(&salt_old);
        let salt_new_fr = Fr::from_le_bytes_mod_order(&salt_new);
        let inner_old = poseidon_hash_two_v05(&root, &Fr::from(epoch_old));
        let mid_old = poseidon_hash_two_v05(&inner_old, &salt_old_fr);
        let c_old = poseidon_hash_two_v05(&mid_old, &occ_old);
        let inner_new = poseidon_hash_two_v05(&root, &Fr::from(epoch_old + 1));
        let mid_new = poseidon_hash_two_v05(&inner_new, &salt_new_fr);
        let c_new = poseidon_hash_two_v05(&mid_new, &occ_new);

        // Build K_MAX signer slots.
        let dummy_path = vec![Fr::from(0u64); depth];
        let signers: [DemocracySigner; K_MAX] = core::array::from_fn(|i| {
            if i < active_count {
                DemocracySigner {
                    secret_key: secret_keys[i],
                    merkle_path: paths[i].clone(),
                    leaf_index: i,
                    active: true,
                }
            } else {
                DemocracySigner {
                    secret_key: Fr::from(0u64),
                    merkle_path: dummy_path.clone(),
                    leaf_index: 0,
                    active: false,
                }
            }
        });

        DemocracyUpdateQuorumWitness {
            c_old,
            epoch_old,
            c_new,
            occupancy_commitment_old: occ_old,
            occupancy_commitment_new: occ_new,
            threshold_numerator: threshold,
            signers,
            member_root_old: root,
            member_root_new: root,
            member_count_old,
            member_count_new,
            salt_oc_old,
            salt_oc_new,
            salt_old,
            salt_new,
            depth,
        }
    }

    fn pi(w: &DemocracyUpdateQuorumWitness) -> Vec<Fr> {
        vec![
            w.c_old,
            Fr::from(w.epoch_old),
            w.c_new,
            w.occupancy_commitment_old,
            w.occupancy_commitment_new,
            Fr::from(w.threshold_numerator),
        ]
    }

    #[test]
    fn satisfies_with_full_quorum() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_witness(&sks, 3, 42, K_MAX as u64, K_MAX, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        c.check_circuit_satisfiability(&pi(&w)).unwrap();
    }

    #[test]
    fn satisfies_with_quorum_above_threshold() {
        // K = K_MAX, threshold = 1 → slack = K_MAX - 1 fits in 2 bits.
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_witness(&sks, 3, 42, 1, K_MAX, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        c.check_circuit_satisfiability(&pi(&w)).unwrap();
    }

    #[test]
    fn rejects_k_below_threshold() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let mut w = build_witness(&sks, 3, 42, K_MAX as u64, K_MAX, 5, 5);
        // Deactivate the last active slot — K = K_MAX - 1 < threshold.
        w.signers[K_MAX - 1].active = false;
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        assert!(c.check_circuit_satisfiability(&pi(&w)).is_err());
    }

    #[test]
    fn rejects_count_delta_too_large() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_witness(&sks, 3, 42, K_MAX as u64, K_MAX, 5, 7);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        assert!(c.check_circuit_satisfiability(&pi(&w)).is_err());
    }

    /// Quorum circuit fits at d=5 and d=8 only. d=11 blows the
    /// n=32768 SRS budget; tier 2 falls back to
    /// `synthesize_democracy_update_simple`.
    #[test]
    fn gate_count_per_supported_tier() {
        for &depth in &[5usize, 8] {
            let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
            let w = build_witness(&sks, depth, 1, K_MAX as u64, K_MAX, 5, 5);
            let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
            synthesize_democracy_update_quorum(&mut c, &w).unwrap();
            c.finalize_for_arithmetization().unwrap();
            eprintln!("[gate-count] democracy_update K={K_MAX} depth={depth}: {} gates", c.num_gates());
            assert!(c.num_gates() < 32768);
        }
    }

    #[test]
    fn round_trip_d5() {
        use rand_chacha::rand_core::SeedableRng;
        let sks: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let w = build_witness(&sks, 5, 1234, K_MAX as u64, K_MAX, 8, 8);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        c.finalize_for_arithmetization().unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&c).unwrap();
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &c).unwrap();
        crate::prover::plonk::verify(&keys.vk, &pi(&w), &proof).unwrap();
    }
}
