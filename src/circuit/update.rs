//! UpdateCircuit — Groth16 circuit that binds a membership proof at the old
//! epoch to the specific C_new being authorized on-chain.
//!
//! Public inputs: c_old, epoch_old, c_new (in this fixed allocation order)
//! Witness: secret_key, poseidon_root_old, salt_old, merkle_path_old,
//!          leaf_index_old, poseidon_root_new, salt_new
//!
//! Constraints (populated in Phase 1 of the update-circuit-binding plan):
//! 1. leaf = Poseidon(secret_key), and it opens along path_old at idx_old to
//!    poseidon_root_old
//! 2. c_old = Poseidon(Poseidon(poseidon_root_old, epoch_old), salt_old)
//! 3. c_new = Poseidon(Poseidon(poseidon_root_new, epoch_old + 1), salt_new)
//!
//! See docs/update-circuit-binding-design.md §5 for the formal relation and
//! docs/vuln-unbound-new-commitment.md for the defect this circuit closes.

use ark_crypto_primitives::sponge::poseidon::PoseidonConfig;
use ark_crypto_primitives::sponge::Absorb;
use ark_ff::PrimeField;
use ark_relations::r1cs::{
    ConstraintSynthesizer, ConstraintSystemRef, SynthesisError,
};

use crate::poseidon::poseidon_config;

/// The SEP-XXXX update circuit.
///
/// Proves: "I know a secret key that is a leaf in the old tree of commitment
/// C_old at epoch e_old, and I know (root_new, salt_new) such that
/// C_new = Poseidon(Poseidon(root_new, e_old + 1), salt_new)."
#[derive(Clone)]
pub struct UpdateCircuit<F: PrimeField + Absorb> {
    // === Public inputs (allocated in fixed order: c_old, epoch_old, c_new) ===
    pub c_old: Option<F>,
    pub epoch_old: Option<u64>,
    pub c_new: Option<F>,

    // === Witnesses ===
    pub secret_key: Option<F>,
    pub poseidon_root_old: Option<F>,
    pub salt_old: Option<[u8; 32]>,
    pub merkle_path_old: Option<Vec<F>>,
    pub leaf_index_old: Option<usize>,
    pub poseidon_root_new: Option<F>,
    pub salt_new: Option<[u8; 32]>,

    /// Tree depth (determines circuit tier).
    pub depth: usize,

    /// Poseidon config (not a witness, but needed for constraint generation).
    pub poseidon_config: PoseidonConfig<F>,
}

impl<F: PrimeField + Absorb> UpdateCircuit<F> {
    /// Create an empty circuit (for setup/keygen — no witness values).
    pub fn empty(depth: usize) -> Self {
        Self {
            c_old: None,
            epoch_old: None,
            c_new: None,
            secret_key: None,
            poseidon_root_old: None,
            salt_old: None,
            merkle_path_old: None,
            leaf_index_old: None,
            poseidon_root_new: None,
            salt_new: None,
            depth,
            poseidon_config: poseidon_config::<F>(),
        }
    }
}

impl ConstraintSynthesizer<ark_bls12_381::Fr> for UpdateCircuit<ark_bls12_381::Fr> {
    fn generate_constraints(
        self,
        _cs: ConstraintSystemRef<ark_bls12_381::Fr>,
    ) -> Result<(), SynthesisError> {
        // Populated in Phase 1 of the implementation plan.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bls12_381::Fr;
    use ark_relations::r1cs::ConstraintSystem;

    #[test]
    fn update_circuit_compiles_with_no_constraints() {
        let cs = ConstraintSystem::<Fr>::new_ref();
        let circuit = UpdateCircuit::<Fr>::empty(5);
        circuit.generate_constraints(cs.clone()).unwrap();
        assert_eq!(cs.num_constraints(), 0);
    }
}
