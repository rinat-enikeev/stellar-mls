//! `MembershipCircuit` ported to jf-relation TurboPlonk.
//!
//! Mirrors the legacy R1CS `crate::circuit::MembershipCircuit` exactly:
//!
//!   Public inputs:  commitment, epoch
//!   Witness:        secret_key, poseidon_root, salt, merkle_path[d],
//!                   leaf_index_bits[d]
//!
//!   Constraints:
//!     1. leaf = Poseidon(secret_key)                              — key ownership
//!     2. MerkleOpen(leaf, path, index_bits, depth) == poseidon_root — membership
//!     3. Poseidon(Poseidon(poseidon_root, epoch), salt) == commitment — binding
//!
//! Public-input ordering, salt encoding (LE-mod-order), and key
//! ownership model are bit-for-bit identical to the legacy circuit.
//! Wire format and downstream contracts therefore don't change with
//! the proving-system swap.
//!
//! Gate cost (logged via the `gate_count` test):
//!
//!   small  (depth=5):  ≈5,300 gates
//!   medium (depth=8):  ≈7,300 gates
//!   large  (depth=11): ≈9,400 gates
//!
//! All three fit the n=16384 EF KZG SRS with comfortable headroom.

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use ark_ff_v05::PrimeField;
use jf_relation::{BoolVar, Circuit, CircuitError, PlonkCircuit, Variable};

use super::merkle::compute_merkle_root_gadget;
use super::poseidon::{poseidon_hash_one_gadget, poseidon_hash_two_gadget};

/// Witness inputs for the membership circuit.
///
/// `commitment` and `epoch` are the **public** inputs allocated in
/// `synthesize`; the rest are private witnesses.
pub struct MembershipWitness {
    /// `Poseidon(Poseidon(poseidon_root, epoch), salt)` — public.
    pub commitment: Fr,
    /// Group epoch — public.
    pub epoch: u64,
    /// Prover's BLS12-381 scalar — private.
    pub secret_key: Fr,
    /// Poseidon Merkle root the leaf belongs to — private.
    pub poseidon_root: Fr,
    /// 32-byte salt; reduced mod r inside the circuit — private.
    pub salt: [u8; 32],
    /// Sibling hashes from leaf to root, length = depth — private.
    pub merkle_path: Vec<Fr>,
    /// Leaf position in the tree — private. (Decomposed into bits in `synthesize`.)
    pub leaf_index: usize,
    /// Tree depth.
    pub depth: usize,
}

/// Allocate the membership circuit and return the public-input
/// `Variable` for `commitment` (already constrained to the witness
/// commitment via `enforce_equal`). Caller can keep the returned
/// variable to wire commitment into a larger composite circuit, or
/// ignore it.
///
/// Public-input ordering, established once: **`(commitment, epoch)`**.
/// Anything depending on this wire format (the Soroban verifier in
/// Phase C; cross-platform test vectors in B.5) must use this exact
/// order.
pub fn synthesize_membership(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &MembershipWitness,
) -> Result<(), CircuitError> {
    if witness.merkle_path.len() != witness.depth {
        return Err(CircuitError::ParameterError(format!(
            "merkle_path length {} != depth {}",
            witness.merkle_path.len(),
            witness.depth
        )));
    }

    // ---- Public inputs (allocate first, in fixed order) ----
    let commitment_var = circuit.create_public_variable(witness.commitment)?;
    let epoch_var = circuit.create_public_variable(Fr::from(witness.epoch))?;

    // ---- Witnesses ----
    let secret_key_var = circuit.create_variable(witness.secret_key)?;
    let poseidon_root_var = circuit.create_variable(witness.poseidon_root)?;
    // Same lossy LE-mod-order encoding the legacy circuit uses; ~50% of
    // random salts lose 1 bit of entropy past 255 bits — acceptable since
    // 255 bits >> 128-bit security target. Documented in the legacy
    // circuit's salt-witness comment.
    let salt_fr = Fr::from_le_bytes_mod_order(&witness.salt);
    let salt_var = circuit.create_variable(salt_fr)?;

    let path_vars: Vec<Variable> = witness
        .merkle_path
        .iter()
        .map(|sibling| circuit.create_variable(*sibling))
        .collect::<Result<_, _>>()?;

    let bit_vars: Vec<BoolVar> = (0..witness.depth)
        .map(|i| circuit.create_boolean_variable(((witness.leaf_index >> i) & 1) == 1))
        .collect::<Result<_, _>>()?;

    // ---- Constraint 1: leaf = Poseidon(secret_key) ----
    let leaf_var = poseidon_hash_one_gadget(circuit, secret_key_var)?;

    // ---- Constraint 2: Merkle membership ----
    let computed_root =
        compute_merkle_root_gadget(circuit, leaf_var, &path_vars, &bit_vars)?;
    circuit.enforce_equal(computed_root, poseidon_root_var)?;

    // ---- Constraint 3: commitment binding ----
    //   inner      = Poseidon(poseidon_root, epoch)
    //   commitment = Poseidon(inner, salt)
    let inner = poseidon_hash_two_gadget(circuit, poseidon_root_var, epoch_var)?;
    let computed_commitment = poseidon_hash_two_gadget(circuit, inner, salt_var)?;
    circuit.enforce_equal(computed_commitment, commitment_var)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::plonk::poseidon::poseidon_hash_two_v05;

    /// Build a small Poseidon Merkle tree over leaves derived from secret
    /// keys (leaf_i = Poseidon(sk_i)), generate a proof for one leaf, and
    /// compute the binding commitment. Mirrors the legacy v0.4 test
    /// helpers but staged at v0.5 so the witness is a `MembershipWitness`.
    fn build_test_witness(
        secret_keys: &[Fr],
        prover_index: usize,
        epoch: u64,
        salt: [u8; 32],
        depth: usize,
    ) -> MembershipWitness {
        use crate::circuit::plonk::poseidon::poseidon_hash_one_v05;

        // 1. Build leaves natively.
        let leaves: Vec<Fr> = secret_keys.iter().map(poseidon_hash_one_v05).collect();

        // 2. Build the tree.
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

        // 3. Generate the opening proof.
        let mut path = Vec::with_capacity(depth);
        let mut cur = num_leaves + prover_index;
        for _ in 0..depth {
            let sib = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
            path.push(nodes[sib]);
            cur /= 2;
        }

        // 4. Compute the binding commitment natively:
        //    Poseidon(Poseidon(root, epoch), salt_fr)
        let salt_fr = Fr::from_le_bytes_mod_order(&salt);
        let inner = poseidon_hash_two_v05(&root, &Fr::from(epoch));
        let commitment = poseidon_hash_two_v05(&inner, &salt_fr);

        MembershipWitness {
            commitment,
            epoch,
            secret_key: secret_keys[prover_index],
            poseidon_root: root,
            salt,
            merkle_path: path,
            leaf_index: prover_index,
            depth,
        }
    }

    /// Witness-level satisfiability: with a well-formed witness, the
    /// circuit's constraints are satisfied at the public-input pair
    /// `(commitment, epoch)`.
    #[test]
    fn synthesize_satisfies_with_valid_witness_small() {
        let depth = 3;
        let secret_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let salt = [0xAA; 32];
        let witness = build_test_witness(&secret_keys, 2, 7, salt, depth);

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_membership(&mut circuit, &witness).unwrap();

        let public_inputs = vec![witness.commitment, Fr::from(witness.epoch)];
        circuit
            .check_circuit_satisfiability(&public_inputs)
            .expect("valid witness should satisfy the circuit");
    }

    /// Tampered public commitment makes `check_circuit_satisfiability`
    /// fail. Confirms the `enforce_equal(computed_commitment,
    /// commitment_var)` constraint actually binds the public input.
    #[test]
    fn synthesize_rejects_tampered_commitment() {
        let depth = 3;
        let secret_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let salt = [0xBB; 32];
        let witness = build_test_witness(&secret_keys, 1, 42, salt, depth);

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_membership(&mut circuit, &witness).unwrap();

        let wrong_commitment = witness.commitment + Fr::from(1u64);
        let public_inputs = vec![wrong_commitment, Fr::from(witness.epoch)];
        let result = circuit.check_circuit_satisfiability(&public_inputs);
        assert!(
            result.is_err(),
            "circuit accepted a wrong public commitment — binding constraint is broken"
        );
    }

    /// Wrong-key witness fails: substitute secret_key with a value not
    /// in the tree — Constraint 2 (Merkle membership) catches it.
    #[test]
    fn synthesize_rejects_wrong_secret_key() {
        let depth = 3;
        let secret_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let salt = [0xCC; 32];
        let mut witness = build_test_witness(&secret_keys, 0, 100, salt, depth);

        // Replace secret_key with something not in the tree (anything not in 1..=4).
        witness.secret_key = Fr::from(99999u64);

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_membership(&mut circuit, &witness).unwrap();

        let public_inputs = vec![witness.commitment, Fr::from(witness.epoch)];
        let result = circuit.check_circuit_satisfiability(&public_inputs);
        assert!(
            result.is_err(),
            "circuit accepted a non-member secret key — Merkle constraint is broken"
        );
    }

    /// Gate-count snapshot per tier, surfaced for budget tracking.
    #[test]
    fn gate_count_per_tier() {
        for &depth in &[5usize, 8, 11] {
            let salt = [0xDD; 32];
            let secret_keys: Vec<Fr> = (1u64..=2).map(Fr::from).collect();
            let witness = build_test_witness(&secret_keys, 0, 1, salt, depth);

            let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
            synthesize_membership(&mut circuit, &witness).unwrap();
            let n = circuit.num_gates();
            eprintln!("[gate-count] MembershipCircuit depth={depth}: {n} gates");
            assert!(
                n < 16384,
                "MembershipCircuit at depth={depth} has {n} gates, exceeds n=16384 \
                 EF KZG SRS ceiling. Need n=32768 (transcript index 3)."
            );
        }
    }

    /// Full prove → verify round trip on the small tier (depth=5). This
    /// is the airtight test: builds a circuit at production tier-size,
    /// runs PlonkKzgSnark::{preprocess, prove, verify} against the
    /// embedded n=16384 EF KZG SRS, and asserts both accept and reject
    /// paths. Catches the class of bugs that satisfiability-only tests
    /// miss (e.g. a wrong gate selector that satisfies the witness but
    /// produces an invalid PLONK proof).
    #[test]
    fn membership_round_trip_prove_verify_small_tier() {
        use rand_chacha::rand_core::SeedableRng;

        let depth = 5;
        let secret_keys: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let salt = [0xEE; 32];
        let witness = build_test_witness(&secret_keys, 3, 1234, salt, depth);

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_membership(&mut circuit, &witness).unwrap();
        circuit.finalize_for_arithmetization().unwrap();

        eprintln!(
            "[gate-count] MembershipCircuit (depth=5) finalised: {} gates",
            circuit.num_gates()
        );

        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&circuit).expect("preprocess");
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &circuit).expect("prove");

        let public_inputs = vec![witness.commitment, Fr::from(witness.epoch)];
        assert!(
            crate::prover::plonk::verify(&keys.vk, &public_inputs, &proof),
            "verifier rejected a valid membership proof at depth=5"
        );

        // Tampered public commitment must fail verification.
        let wrong = vec![witness.commitment + Fr::from(1u64), Fr::from(witness.epoch)];
        assert!(
            !crate::prover::plonk::verify(&keys.vk, &wrong, &proof),
            "verifier accepted membership proof against wrong public commitment"
        );
    }
}
