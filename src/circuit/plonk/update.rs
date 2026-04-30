//! `UpdateCircuit` ported to jf-relation TurboPlonk.
//!
//! Mirrors the legacy R1CS `crate::circuit::update::UpdateCircuit`
//! constraint-for-constraint:
//!
//!   Public inputs:  c_old, epoch_old, c_new
//!   Witness:        secret_key, poseidon_root_old, salt_old,
//!                   merkle_path_old[d], leaf_index_old_bits[d],
//!                   poseidon_root_new, salt_new
//!
//!   Constraints:
//!     1. leaf = Poseidon(secret_key); MerkleOpen(leaf, path_old,
//!        index_old_bits, depth) == poseidon_root_old             — key + old-tree membership
//!     2. Poseidon(Poseidon(poseidon_root_old, epoch_old), salt_old)
//!        == c_old                                                — old-commitment binding
//!     3. epoch_new = epoch_old + 1 (in-circuit constant);
//!        Poseidon(Poseidon(poseidon_root_new, epoch_new), salt_new)
//!        == c_new                                                — new-commitment binding
//!
//! Public-input ordering, salt encoding (LE-mod-order), and key-ownership
//! model are bit-for-bit identical to the legacy circuit. The on-chain
//! verifier consumes the same `(c_old, epoch_old, c_new)` BE-encoded
//! triple.
//!
//! Gate cost is roughly 2× the membership circuit (two commitment-binding
//! Poseidons + one shared Merkle path). All three tiers (depth 5/8/11)
//! finalise comfortably below the n=32768 EF KZG SRS ceiling — verified
//! by `gate_count_per_tier` below.
//!
//! ## Security model — new-tree binding is *commitment-only*
//!
//! The circuit binds `c_new` to `(poseidon_root_new, epoch_old + 1,
//! salt_new)` via Constraint 3, but it does **not** constrain the
//! prover's knowledge of any leaf in `poseidon_root_new` or the shape
//! of the new tree. A prover can pick any `poseidon_root_new` and any
//! `salt_new`, derive the resulting `c_new`, and submit a valid update.
//!
//! That is the legacy Groth16 reference's design too (see
//! `src/circuit/update.rs`'s "constraints" docstring) and the contract
//! relies on it: the **on-chain** path explicitly does not check
//! membership in the new tree — only that the transition is bound to
//! a `c_new` the prover authorised. Downstream consumers (clients
//! reading commitments off-chain) must therefore not interpret
//! `poseidon_root_new` as an authenticated roster — only `c_new` itself
//! is authenticated, and only as "the current commitment after this
//! update."
//!
//! ## Public-input range
//!
//! `epoch_old` is allocated as a `Variable` with no in-circuit range
//! check. A malicious prover can submit an Fr larger than `2^64` and
//! produce a valid proof at the verifier; off-chain code that
//! interprets the public input as a `u64` could be tricked. This
//! matches the legacy Groth16 circuit's behaviour and is intentional
//! at this layer. Callers — specifically the Soroban contract
//! `update_commitment` entrypoint — MUST enforce the `u64` range
//! out-of-circuit (the natural way: take `epoch: u64` as the
//! entrypoint argument and BE-encode it as a 32-byte scalar before
//! handing it to the verifier).

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use ark_ff_v05::PrimeField;
use jf_relation::{BoolVar, Circuit, CircuitError, PlonkCircuit, Variable};

use super::merkle::compute_merkle_root_gadget;
use super::poseidon::{poseidon_hash_one_gadget, poseidon_hash_two_gadget};

/// Witness inputs for the update circuit.
///
/// `c_old`, `epoch_old`, `c_new` are the **public** inputs allocated in
/// `synthesize`; the rest are private witnesses. New-tree membership is
/// not enforced here — only that `c_new` is well-formed against
/// `(poseidon_root_new, epoch_old + 1, salt_new)`.
pub struct UpdateWitness {
    /// Old binding commitment — public.
    pub c_old: Fr,
    /// Group epoch at the old commitment — public.
    pub epoch_old: u64,
    /// New binding commitment — public.
    pub c_new: Fr,

    /// Prover's BLS12-381 scalar — private.
    pub secret_key: Fr,
    /// Poseidon Merkle root of the old tree — private.
    pub poseidon_root_old: Fr,
    /// Old salt; reduced mod r inside the circuit — private.
    pub salt_old: [u8; 32],
    /// Sibling hashes from the leaf to the old root, length = depth — private.
    pub merkle_path_old: Vec<Fr>,
    /// Leaf position in the old tree — private. (Decomposed to bits.)
    pub leaf_index_old: usize,

    /// Poseidon Merkle root of the new tree — private.
    pub poseidon_root_new: Fr,
    /// New salt; reduced mod r inside the circuit — private.
    pub salt_new: [u8; 32],

    /// Tree depth (determines circuit tier).
    pub depth: usize,
}

/// Allocate the update circuit and return the public-input `Variable`s
/// for `(c_old, epoch_old, c_new)` in fixed allocation order.
///
/// Public-input ordering is **`(c_old, epoch_old, c_new)`** — matches
/// the off-chain Groth16 reference and the on-chain verifier. Anything
/// depending on this wire format must use this exact order.
pub fn synthesize_update(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &UpdateWitness,
) -> Result<(Variable, Variable, Variable), CircuitError> {
    if witness.merkle_path_old.len() != witness.depth {
        return Err(CircuitError::ParameterError(format!(
            "merkle_path_old length {} != depth {}",
            witness.merkle_path_old.len(),
            witness.depth
        )));
    }
    // Reject `depth >= usize::BITS` *first* — otherwise the
    // index-bit decomposition loop below would do
    // `witness.leaf_index_old >> i` for `i >= 64`, which is UB in Rust.
    // The previous `depth < usize::BITS && leaf_index >= 1 << depth`
    // gate skipped the bounds check exactly when the UB shift would
    // fire. In practice we only support depth ≤ 11, so this is a
    // defensive guard.
    if witness.depth >= usize::BITS as usize {
        return Err(CircuitError::ParameterError(format!(
            "depth {} >= usize::BITS ({}); update circuit supports depth ≤ {}",
            witness.depth,
            usize::BITS,
            usize::BITS - 1,
        )));
    }
    if witness.leaf_index_old >= (1usize << witness.depth) {
        return Err(CircuitError::ParameterError(format!(
            "leaf_index_old {} out of range for depth {} (max {})",
            witness.leaf_index_old,
            witness.depth,
            (1usize << witness.depth) - 1
        )));
    }

    // ---- Public inputs (allocate first, in fixed order) ----
    let c_old_var = circuit.create_public_variable(witness.c_old)?;
    let epoch_old_var = circuit.create_public_variable(Fr::from(witness.epoch_old))?;
    let c_new_var = circuit.create_public_variable(witness.c_new)?;

    // ---- Witness allocation: old-tree authentication path ----
    let secret_key_var = circuit.create_variable(witness.secret_key)?;
    let poseidon_root_old_var = circuit.create_variable(witness.poseidon_root_old)?;
    // LE-mod-order salt encoding (matches MembershipCircuit + legacy R1CS).
    let salt_old_fr = Fr::from_le_bytes_mod_order(&witness.salt_old);
    let salt_old_var = circuit.create_variable(salt_old_fr)?;
    let path_old_vars: Vec<Variable> = witness
        .merkle_path_old
        .iter()
        .map(|sibling| circuit.create_variable(*sibling))
        .collect::<Result<_, _>>()?;
    let index_old_bits: Vec<BoolVar> = (0..witness.depth)
        .map(|i| circuit.create_boolean_variable(((witness.leaf_index_old >> i) & 1) == 1))
        .collect::<Result<_, _>>()?;

    // ---- Witness allocation: new-tree root + salt ----
    let poseidon_root_new_var = circuit.create_variable(witness.poseidon_root_new)?;
    let salt_new_fr = Fr::from_le_bytes_mod_order(&witness.salt_new);
    let salt_new_var = circuit.create_variable(salt_new_fr)?;

    // ---- Constraint 1: leaf = Poseidon(secret_key) + old-tree membership ----
    let leaf_var = poseidon_hash_one_gadget(circuit, secret_key_var)?;
    let computed_root_old =
        compute_merkle_root_gadget(circuit, leaf_var, &path_old_vars, &index_old_bits)?;
    circuit.enforce_equal(computed_root_old, poseidon_root_old_var)?;

    // ---- Constraint 2: c_old = Poseidon(Poseidon(root_old, epoch_old), salt_old) ----
    let old_root_epoch =
        poseidon_hash_two_gadget(circuit, poseidon_root_old_var, epoch_old_var)?;
    let computed_c_old = poseidon_hash_two_gadget(circuit, old_root_epoch, salt_old_var)?;
    circuit.enforce_equal(computed_c_old, c_old_var)?;

    // ---- Constraint 3: epoch_new = epoch_old + 1; new-commitment binding ----
    //
    // Monotonicity is wired in by deriving `epoch_new_var` from
    // `epoch_old_var + 1` in-circuit; an attacker cannot supply
    // `c_new` against a different epoch_new without breaking the
    // Poseidon binding.
    let epoch_new_var = circuit.add_constant(epoch_old_var, &Fr::from(1u64))?;
    let new_root_epoch =
        poseidon_hash_two_gadget(circuit, poseidon_root_new_var, epoch_new_var)?;
    let computed_c_new = poseidon_hash_two_gadget(circuit, new_root_epoch, salt_new_var)?;
    circuit.enforce_equal(computed_c_new, c_new_var)?;

    Ok((c_old_var, epoch_old_var, c_new_var))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::plonk::poseidon::{poseidon_hash_one_v05, poseidon_hash_two_v05};

    /// Build a Poseidon Merkle tree over `leaf_i = Poseidon(sk_i)` and
    /// return `(root, path_to_index)`. Same flat-array shape as
    /// `MembershipCircuit`'s test helper.
    fn build_tree(secret_keys: &[Fr], depth: usize) -> (Fr, Vec<Vec<Fr>>) {
        let leaves: Vec<Fr> = secret_keys.iter().map(poseidon_hash_one_v05).collect();
        let num_leaves = 1usize << depth;
        assert!(leaves.len() <= num_leaves);
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

    /// Build an `UpdateWitness` that satisfies all three constraints for
    /// the given roster transition. Used by the witness-level tests.
    fn build_update_witness(
        old_keys: &[Fr],
        prover_index: usize,
        new_keys: &[Fr],
        epoch_old: u64,
        salt_old: [u8; 32],
        salt_new: [u8; 32],
        depth: usize,
    ) -> UpdateWitness {
        let (root_old, paths_old) = build_tree(old_keys, depth);
        let (root_new, _paths_new) = build_tree(new_keys, depth);

        // Native commitments (matches the gadget's in-circuit
        // construction, modulo LE-mod-order salt encoding).
        let salt_old_fr = Fr::from_le_bytes_mod_order(&salt_old);
        let salt_new_fr = Fr::from_le_bytes_mod_order(&salt_new);
        let c_old = poseidon_hash_two_v05(
            &poseidon_hash_two_v05(&root_old, &Fr::from(epoch_old)),
            &salt_old_fr,
        );
        let c_new = poseidon_hash_two_v05(
            &poseidon_hash_two_v05(&root_new, &Fr::from(epoch_old + 1)),
            &salt_new_fr,
        );

        UpdateWitness {
            c_old,
            epoch_old,
            c_new,
            secret_key: old_keys[prover_index],
            poseidon_root_old: root_old,
            salt_old,
            merkle_path_old: paths_old[prover_index].clone(),
            leaf_index_old: prover_index,
            poseidon_root_new: root_new,
            salt_new,
            depth,
        }
    }

    /// Witness-level satisfiability: a well-formed update witness
    /// satisfies the circuit at the public-input triple
    /// `(c_old, epoch_old, c_new)`.
    #[test]
    fn synthesize_satisfies_with_valid_witness_small() {
        let depth = 3;
        let old_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let new_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let witness = build_update_witness(
            &old_keys,
            2,
            &new_keys,
            7,
            [0xAA; 32],
            [0xBB; 32],
            depth,
        );

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_update(&mut circuit, &witness).unwrap();

        let public_inputs = vec![
            witness.c_old,
            Fr::from(witness.epoch_old),
            witness.c_new,
        ];
        circuit
            .check_circuit_satisfiability(&public_inputs)
            .expect("valid witness should satisfy the circuit");
    }

    /// Tampered `c_old` makes `check_circuit_satisfiability` fail.
    /// Confirms Constraint 2 (old-commitment binding) actually binds
    /// the public input — symmetric to `synthesize_rejects_tampered_c_new`.
    #[test]
    fn synthesize_rejects_tampered_c_old() {
        let depth = 3;
        let old_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let new_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let witness = build_update_witness(
            &old_keys,
            0,
            &new_keys,
            42,
            [0xCC; 32],
            [0xDD; 32],
            depth,
        );

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_update(&mut circuit, &witness).unwrap();

        let wrong_c_old = witness.c_old + Fr::from(1u64);
        let public_inputs = vec![
            wrong_c_old,
            Fr::from(witness.epoch_old),
            witness.c_new,
        ];
        let result = circuit.check_circuit_satisfiability(&public_inputs);
        assert!(
            result.is_err(),
            "circuit accepted a wrong public c_old — old-commitment binding broken"
        );
    }

    /// Tampered `c_new` makes `check_circuit_satisfiability` fail.
    /// Confirms the new-commitment-binding constraint actually binds.
    #[test]
    fn synthesize_rejects_tampered_c_new() {
        let depth = 3;
        let old_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let new_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let witness = build_update_witness(
            &old_keys,
            0,
            &new_keys,
            42,
            [0xCC; 32],
            [0xDD; 32],
            depth,
        );

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_update(&mut circuit, &witness).unwrap();

        let wrong_c_new = witness.c_new + Fr::from(1u64);
        let public_inputs = vec![
            witness.c_old,
            Fr::from(witness.epoch_old),
            wrong_c_new,
        ];
        let result = circuit.check_circuit_satisfiability(&public_inputs);
        assert!(
            result.is_err(),
            "circuit accepted a wrong public c_new — new-commitment binding broken"
        );
    }

    /// Non-monotonic epoch: the witness commits c_new to `epoch_old + 2`
    /// instead of `epoch_old + 1`. The circuit derives `epoch_new` as
    /// `epoch_old_var + 1`, so Constraint 3 must fail.
    #[test]
    fn synthesize_rejects_non_monotonic_epoch() {
        let depth = 3;
        let old_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let salt_old = [0xAA; 32];
        let salt_new = [0xBB; 32];
        let epoch_old = 0u64;

        let (root_old, paths_old) = build_tree(&old_keys, depth);
        let (root_new, _) = build_tree(&old_keys, depth);
        let salt_old_fr = Fr::from_le_bytes_mod_order(&salt_old);
        let salt_new_fr = Fr::from_le_bytes_mod_order(&salt_new);
        let c_old = poseidon_hash_two_v05(
            &poseidon_hash_two_v05(&root_old, &Fr::from(epoch_old)),
            &salt_old_fr,
        );
        // Bind c_new to epoch_old + 2 instead of the in-circuit
        // epoch_old + 1 — the circuit must reject.
        let c_new_wrong = poseidon_hash_two_v05(
            &poseidon_hash_two_v05(&root_new, &Fr::from(epoch_old + 2)),
            &salt_new_fr,
        );

        let witness = UpdateWitness {
            c_old,
            epoch_old,
            c_new: c_new_wrong,
            secret_key: old_keys[0],
            poseidon_root_old: root_old,
            salt_old,
            merkle_path_old: paths_old[0].clone(),
            leaf_index_old: 0,
            poseidon_root_new: root_new,
            salt_new,
            depth,
        };

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_update(&mut circuit, &witness).unwrap();

        let public_inputs = vec![
            witness.c_old,
            Fr::from(witness.epoch_old),
            witness.c_new,
        ];
        let result = circuit.check_circuit_satisfiability(&public_inputs);
        assert!(
            result.is_err(),
            "circuit accepted a non-monotonic epoch — Constraint 3 broken"
        );
    }

    /// Wrong auth path: corrupt one sibling — leaf no longer opens to
    /// `poseidon_root_old`. Constraint 1 must catch it.
    #[test]
    fn synthesize_rejects_wrong_auth_path() {
        let depth = 3;
        let old_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let new_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let mut witness = build_update_witness(
            &old_keys,
            0,
            &new_keys,
            5,
            [0xAA; 32],
            [0xBB; 32],
            depth,
        );

        // Corrupt one sibling.
        witness.merkle_path_old[0] = witness.merkle_path_old[0] + Fr::from(1u64);

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_update(&mut circuit, &witness).unwrap();

        let public_inputs = vec![
            witness.c_old,
            Fr::from(witness.epoch_old),
            witness.c_new,
        ];
        let result = circuit.check_circuit_satisfiability(&public_inputs);
        assert!(
            result.is_err(),
            "circuit accepted a wrong auth path — Constraint 1 broken"
        );
    }

    /// Negative test for the `merkle_path_old.len() != depth` early-return.
    #[test]
    fn synthesize_rejects_path_length_mismatch() {
        let depth = 4;
        let old_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let new_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let mut witness = build_update_witness(
            &old_keys,
            1,
            &new_keys,
            99,
            [0x11; 32],
            [0x22; 32],
            depth,
        );

        witness.merkle_path_old.truncate(depth - 1);

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        let result = synthesize_update(&mut circuit, &witness);
        assert!(
            matches!(result, Err(CircuitError::ParameterError(_))),
            "synthesize accepted merkle_path_old.len() != depth (got {result:?})"
        );
    }

    /// Gate-count snapshot per tier. Asserts each tier finalises below
    /// the n=32768 EF KZG SRS ceiling (jf-plonk's preprocess needs
    /// strictly more powers than gates).
    #[test]
    fn gate_count_per_tier() {
        for &depth in &[5usize, 8, 11] {
            let old_keys: Vec<Fr> = (1u64..=2).map(Fr::from).collect();
            let new_keys: Vec<Fr> = (1u64..=2).map(Fr::from).collect();
            let witness = build_update_witness(
                &old_keys,
                0,
                &new_keys,
                1,
                [0xDD; 32],
                [0xEE; 32],
                depth,
            );

            let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
            synthesize_update(&mut circuit, &witness).unwrap();
            let raw = circuit.num_gates();
            circuit.finalize_for_arithmetization().unwrap();
            let finalised = circuit.num_gates();
            eprintln!(
                "[gate-count] UpdateCircuit depth={depth}: {raw} raw, {finalised} finalised"
            );
            assert!(
                finalised < 32768,
                "UpdateCircuit at depth={depth} finalises to {finalised} gates, \
                 needs strictly less than n=32768 EF KZG SRS ceiling for jf-plonk preprocess. \
                 Need a larger SRS (transcript indices ≥4)."
            );
        }
    }

    /// Full prove → verify round trip, small tier (depth=5).
    #[test]
    fn update_round_trip_prove_verify_small_tier() {
        run_round_trip_at_depth(5);
    }

    /// Round trip on the medium tier (depth=8).
    #[test]
    fn update_round_trip_prove_verify_medium_tier() {
        run_round_trip_at_depth(8);
    }

    /// Round trip on the large tier (depth=11).
    #[test]
    fn update_round_trip_prove_verify_large_tier() {
        run_round_trip_at_depth(11);
    }

    fn run_round_trip_at_depth(depth: usize) {
        use rand_chacha::rand_core::SeedableRng;

        let old_keys: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let new_keys: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let salt_old = [0xEE; 32];
        let salt_new = [0xFF; 32];
        let witness = build_update_witness(
            &old_keys,
            3,
            &new_keys,
            1234,
            salt_old,
            salt_new,
            depth,
        );

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_update(&mut circuit, &witness).unwrap();
        circuit.finalize_for_arithmetization().unwrap();

        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&circuit).expect("preprocess");
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &circuit).expect("prove");

        let public_inputs = vec![
            witness.c_old,
            Fr::from(witness.epoch_old),
            witness.c_new,
        ];
        crate::prover::plonk::verify(&keys.vk, &public_inputs, &proof).unwrap_or_else(
            |e| panic!("verifier rejected valid update proof at depth={depth}: {e:?}"),
        );

        // Tampered c_new must fail verification.
        let wrong = vec![
            witness.c_old,
            Fr::from(witness.epoch_old),
            witness.c_new + Fr::from(1u64),
        ];
        assert!(
            crate::prover::plonk::verify(&keys.vk, &wrong, &proof).is_err(),
            "verifier accepted update proof against wrong public c_new at depth={depth}"
        );
    }
}
