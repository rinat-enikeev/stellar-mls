//! Oligarchy `Create` and `Update` circuits.
//!
//! ## Create — `synthesize_oligarchy_create`
//!
//! Verbose binding per design §4.8: c binds member_root, admin_root,
//! salt_initial, occupancy_commitment, and epoch=0 in a single
//! Poseidon chain.
//!
//! ## Update — two circuits
//!
//! - `synthesize_oligarchy_update_quorum` (this PR, issue #202) —
//!   enforces K-of-N admin quorum + count delta + admin-tree
//!   membership, with the commitment chain reconciled to match
//!   create's `Poseidon(occ, admin_root)` mix.
//! - `synthesize_oligarchy_update` (kept verbatim from PR #200's
//!   pre-quorum port) — Poseidon-only commitment chain that does
//!   **not** include `admin_root`, so it's not lineage-compatible
//!   with `synthesize_oligarchy_create`. Retained as a migration
//!   fallback / reference; production VKs bake the quorum variant.
//!
//! ## Quorum + delta semantics (issue #202)
//!
//! `synthesize_oligarchy_update_quorum` enforces:
//!
//!   1. **K-of-N admin quorum** — `OLIGARCHY_K_MAX = 2` admin signer
//!      slots; each `(sk, merkle_path, leaf_idx, active)` against
//!      `admin_root_old` at depth 5. Active slots form a strict
//!      prefix; pairwise-distinct `leaf_idx` for active slots
//!      (anti-double-count); `K = Σ active` and `K ≥ threshold`
//!      with both range-checked into 2 bits.
//!   2. **Member count delta** — `occupancy_commitment_X =
//!      Poseidon(member_count_X, salt_oc_X)` with
//!      `|count_new - count_old| ≤ 1`. Tree-level single-leaf
//!      delta (the new root differs by exactly one leaf) is
//!      **intentionally not enforced** in v0.1.5 — `member_root_new`
//!      is a free witness bounded only by the count. The earlier
//!      draft's tree-delta + `target_tree` dispatcher was withdrawn
//!      with the §3.5 "which tree changed" hiding claim; see the
//!      design doc and the v0.1.5 stanza for rationale.
//!   3. **Commitment chain reconciliation** — c_old / c_new use the
//!      same chain as create:
//!      `c_X = Poseidon(Poseidon(Poseidon(member_root_X, epoch_X),
//!                               salt_X),
//!                      Poseidon(occ_X, admin_root_X))`.
//!      The pre-quorum update was lineage-incompatible with create
//!      (omitted `admin_root` from the third hash), so this is a
//!      **soundness fix** in addition to the quorum upgrade.
//!
//! ## Public inputs (unchanged across both update variants)
//!
//! **Create** (6, fixed order):
//!   1. `commitment`
//!   2. `epoch` (always 0)
//!   3. `occupancy_commitment`
//!   4. `member_root`
//!   5. `admin_root`
//!   6. `salt_initial`
//!
//! **Update** (6, fixed order — same as democracy_update):
//!   1. `c_old`
//!   2. `epoch_old`
//!   3. `c_new`
//!   4. `occupancy_commitment_old`
//!   5. `occupancy_commitment_new`
//!   6. `admin_threshold_numerator`

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use ark_ff_v05::PrimeField;
use jf_relation::{BoolVar, Circuit, CircuitError, PlonkCircuit, Variable};

use super::merkle::compute_merkle_root_gadget;
use super::poseidon::{poseidon_hash_one_gadget, poseidon_hash_two_gadget};

/// Quorum cap for the oligarchy admin tree. Mirrors democracy's
/// `K_MAX = 2`: 2 admin signer slots, each producing a Merkle opening
/// against `admin_root_old`. Raising past 3 requires widening the
/// slack + threshold 2-bit range gates in lockstep.
pub const OLIGARCHY_K_MAX: usize = 2;

/// Admin tree depth. Per design §4.6 the admin tier is fixed at Small
/// across all member tiers: 32 slots, depth 5. Constant rather than
/// witness-driven so `synthesize_oligarchy_update_quorum`'s circuit
/// shape is fixed (single-tier VK across all member tiers).
pub const OLIGARCHY_ADMIN_DEPTH: usize = 5;

const _: () = assert!(
    OLIGARCHY_K_MAX <= 3,
    "widen slack + threshold ranges before raising OLIGARCHY_K_MAX past 3"
);

pub struct OligarchyCreateWitness {
    pub commitment: Fr,
    pub occupancy_commitment: Fr,
    pub member_root: Fr,
    pub admin_root: Fr,
    pub salt_initial: Fr,
}

pub struct OligarchyUpdateWitness {
    pub c_old: Fr,
    pub epoch_old: u64,
    pub c_new: Fr,
    pub occupancy_commitment_old: Fr,
    pub occupancy_commitment_new: Fr,
    pub admin_threshold_numerator: u64,

    pub member_root_old: Fr,
    pub member_root_new: Fr,
    pub salt_old: Fr,
    pub salt_new: Fr,
}

pub fn synthesize_oligarchy_create(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &OligarchyCreateWitness,
) -> Result<(), CircuitError> {
    let commitment_var = circuit.create_public_variable(witness.commitment)?;
    let _epoch_var = circuit.create_public_variable(Fr::from(0u64))?;
    let occ_var = circuit.create_public_variable(witness.occupancy_commitment)?;
    let member_root_var = circuit.create_public_variable(witness.member_root)?;
    let admin_root_var = circuit.create_public_variable(witness.admin_root)?;
    let salt_var = circuit.create_public_variable(witness.salt_initial)?;

    // Simplified binding: c = Poseidon(Poseidon(Poseidon(member_root, 0),
    // salt), Poseidon(occupancy_commitment, admin_root))
    let zero_var = circuit.zero();
    let inner = poseidon_hash_two_gadget(circuit, member_root_var, zero_var)?;
    let mid = poseidon_hash_two_gadget(circuit, inner, salt_var)?;
    let admin_mix = poseidon_hash_two_gadget(circuit, occ_var, admin_root_var)?;
    let computed_c = poseidon_hash_two_gadget(circuit, mid, admin_mix)?;
    circuit.enforce_equal(computed_c, commitment_var)?;

    Ok(())
}

pub fn synthesize_oligarchy_update(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &OligarchyUpdateWitness,
) -> Result<(), CircuitError> {
    let c_old_var = circuit.create_public_variable(witness.c_old)?;
    let epoch_old_var = circuit.create_public_variable(Fr::from(witness.epoch_old))?;
    let c_new_var = circuit.create_public_variable(witness.c_new)?;
    let occ_old_var = circuit.create_public_variable(witness.occupancy_commitment_old)?;
    let occ_new_var = circuit.create_public_variable(witness.occupancy_commitment_new)?;
    let _threshold_var =
        circuit.create_public_variable(Fr::from(witness.admin_threshold_numerator))?;

    let member_root_old_var = circuit.create_variable(witness.member_root_old)?;
    let member_root_new_var = circuit.create_variable(witness.member_root_new)?;
    let salt_old_var = circuit.create_variable(witness.salt_old)?;
    let salt_new_var = circuit.create_variable(witness.salt_new)?;

    // c_old = Poseidon(Poseidon(Poseidon(member_root_old, epoch_old), salt_old), occ_old)
    let inner_old = poseidon_hash_two_gadget(circuit, member_root_old_var, epoch_old_var)?;
    let mid_old = poseidon_hash_two_gadget(circuit, inner_old, salt_old_var)?;
    let computed_c_old = poseidon_hash_two_gadget(circuit, mid_old, occ_old_var)?;
    circuit.enforce_equal(computed_c_old, c_old_var)?;

    // c_new = Poseidon(Poseidon(Poseidon(member_root_new, epoch_old+1), salt_new), occ_new)
    let epoch_new_var = circuit.add_constant(epoch_old_var, &Fr::from(1u64))?;
    let inner_new = poseidon_hash_two_gadget(circuit, member_root_new_var, epoch_new_var)?;
    let mid_new = poseidon_hash_two_gadget(circuit, inner_new, salt_new_var)?;
    let computed_c_new = poseidon_hash_two_gadget(circuit, mid_new, occ_new_var)?;
    circuit.enforce_equal(computed_c_new, c_new_var)?;

    Ok(())
}

// ================================================================
// Quorum + delta + admin-membership update circuit (issue #202).
// ================================================================

/// One admin signer's witness bundle. Mirrors `DemocracySigner` but
/// the path opens against the **admin tree** at fixed depth
/// `OLIGARCHY_ADMIN_DEPTH = 5`.
#[derive(Clone)]
pub struct OligarchyAdminSigner {
    pub secret_key: Fr,
    pub merkle_path: Vec<Fr>,
    pub leaf_index: usize,
    pub active: bool,
}

/// Witness for the K-of-N quorum + count-delta update circuit.
pub struct OligarchyUpdateQuorumWitness {
    // Public inputs (6, fixed order — unchanged from the simplified
    // port).
    pub c_old: Fr,
    pub epoch_old: u64,
    pub c_new: Fr,
    pub occupancy_commitment_old: Fr,
    pub occupancy_commitment_new: Fr,
    pub admin_threshold_numerator: u64,

    // K-of-N admin signers. Active slots must form a strict prefix.
    pub admin_signers: [OligarchyAdminSigner; OLIGARCHY_K_MAX],

    // Tree roots — both pairs are private witnesses. `admin_root_old`
    // is the root the K signers open against; the new admin/member
    // roots are bound only via the commitment chain.
    pub member_root_old: Fr,
    pub member_root_new: Fr,
    pub admin_root_old: Fr,
    pub admin_root_new: Fr,

    // Member counts + occupancy salts (occupancy commitment = Poseidon
    // of these). Matches democracy's pattern — count-only single-leaf
    // delta, **not** tree-level. Tree-level delta on the member tree
    // was withdrawn from the v0.1.5 spec (issue #217) — admins are
    // intended to make multi-leaf updates under the quorum check.
    pub member_count_old: u64,
    pub member_count_new: u64,
    pub salt_oc_old: Fr,
    pub salt_oc_new: Fr,

    // Commitment-chain salts.
    pub salt_old: Fr,
    pub salt_new: Fr,
}

/// K-of-N admin quorum + count-delta + admin-tree-membership oligarchy
/// update circuit. Public-input shape and order are byte-identical to
/// `synthesize_oligarchy_update`, so the Soroban contract surface is
/// unchanged when production VKs are rebaked against this circuit.
pub fn synthesize_oligarchy_update_quorum(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &OligarchyUpdateQuorumWitness,
) -> Result<(), CircuitError> {
    for (i, signer) in witness.admin_signers.iter().enumerate() {
        if signer.merkle_path.len() != OLIGARCHY_ADMIN_DEPTH {
            return Err(CircuitError::ParameterError(format!(
                "admin_signers[{i}].merkle_path length {} != OLIGARCHY_ADMIN_DEPTH {}",
                signer.merkle_path.len(),
                OLIGARCHY_ADMIN_DEPTH,
            )));
        }
        if signer.leaf_index >= (1usize << OLIGARCHY_ADMIN_DEPTH) {
            return Err(CircuitError::ParameterError(format!(
                "admin_signers[{i}].leaf_index {} out of range",
                signer.leaf_index,
            )));
        }
    }

    // ---- Public inputs (fixed order, identical to simplified) ----
    let c_old_var = circuit.create_public_variable(witness.c_old)?;
    let epoch_old_var = circuit.create_public_variable(Fr::from(witness.epoch_old))?;
    let c_new_var = circuit.create_public_variable(witness.c_new)?;
    let occ_old_var = circuit.create_public_variable(witness.occupancy_commitment_old)?;
    let occ_new_var = circuit.create_public_variable(witness.occupancy_commitment_new)?;
    let threshold_var =
        circuit.create_public_variable(Fr::from(witness.admin_threshold_numerator))?;

    // ---- Private witnesses ----
    let member_root_old_var = circuit.create_variable(witness.member_root_old)?;
    let member_root_new_var = circuit.create_variable(witness.member_root_new)?;
    let admin_root_old_var = circuit.create_variable(witness.admin_root_old)?;
    let admin_root_new_var = circuit.create_variable(witness.admin_root_new)?;
    let count_old_var = circuit.create_variable(Fr::from(witness.member_count_old))?;
    let count_new_var = circuit.create_variable(Fr::from(witness.member_count_new))?;
    let salt_oc_old_var = circuit.create_variable(witness.salt_oc_old)?;
    let salt_oc_new_var = circuit.create_variable(witness.salt_oc_new)?;
    let salt_old_var = circuit.create_variable(witness.salt_old)?;
    let salt_new_var = circuit.create_variable(witness.salt_new)?;

    // ---- 1. K-of-N admin quorum ----
    let mut active_vars: Vec<BoolVar> = Vec::with_capacity(OLIGARCHY_K_MAX);
    for signer in witness.admin_signers.iter() {
        active_vars.push(circuit.create_boolean_variable(signer.active)?);
    }

    // Prefix constraint: active_i ⇒ active_{i-1} for i ≥ 1.
    let one_var = circuit.create_constant_variable(Fr::from(1u64))?;
    for i in 1..OLIGARCHY_K_MAX {
        let prev_var: Variable = active_vars[i - 1].into();
        let neg_prev = circuit.sub(one_var, prev_var)?;
        let cur_var: Variable = active_vars[i].into();
        let prod = circuit.mul(cur_var, neg_prev)?;
        circuit.enforce_constant(prod, Fr::from(0u64))?;
    }

    // Active-conditional admin-tree membership + leaf-index decomp.
    let mut leaf_idx_field_vars: Vec<Variable> = Vec::with_capacity(OLIGARCHY_K_MAX);
    for (i, signer) in witness.admin_signers.iter().enumerate() {
        let sk_var = circuit.create_variable(signer.secret_key)?;
        let path_vars: Vec<Variable> = signer
            .merkle_path
            .iter()
            .map(|p| circuit.create_variable(*p))
            .collect::<Result<_, _>>()?;
        let bit_vars: Vec<BoolVar> = (0..OLIGARCHY_ADMIN_DEPTH)
            .map(|j| circuit.create_boolean_variable(((signer.leaf_index >> j) & 1) == 1))
            .collect::<Result<_, _>>()?;

        // leaf_idx_var = Σ_j bit_j · 2^j.
        let leaf_idx_var = circuit.create_variable(Fr::from(signer.leaf_index as u64))?;
        let mut acc = circuit.zero();
        let two = Fr::from(2u64);
        let mut pow = Fr::from(1u64);
        for bit in &bit_vars {
            let bit_v: Variable = (*bit).into();
            let pow_const = circuit.create_constant_variable(pow)?;
            let term = circuit.mul(bit_v, pow_const)?;
            acc = circuit.add(acc, term)?;
            pow *= two;
        }
        circuit.enforce_equal(acc, leaf_idx_var)?;
        leaf_idx_field_vars.push(leaf_idx_var);

        let leaf_var = poseidon_hash_one_gadget(circuit, sk_var)?;
        let computed_root =
            compute_merkle_root_gadget(circuit, leaf_var, &path_vars, &bit_vars)?;

        // active=1 ⇒ computed_root == admin_root_old.
        let diff = circuit.sub(computed_root, admin_root_old_var)?;
        let active_var: Variable = active_vars[i].into();
        let prod = circuit.mul(active_var, diff)?;
        circuit.enforce_constant(prod, Fr::from(0u64))?;
    }

    // Anti-double-count: pairwise leaf_idx distinctness across active
    // slots. Distinctness is on `leaf_idx`, not `Poseidon(sk)` — relies
    // on admin-tree uniqueness (the off-circuit tree builder
    // deduplicates secret keys before populating leaves).
    for i in 1..OLIGARCHY_K_MAX {
        for j in 0..i {
            let eq = circuit.is_equal(leaf_idx_field_vars[j], leaf_idx_field_vars[i])?;
            let prev_a: Variable = active_vars[j].into();
            let cur_a: Variable = active_vars[i].into();
            let both_active = circuit.mul(prev_a, cur_a)?;
            let eq_var: Variable = eq.into();
            let bad = circuit.mul(both_active, eq_var)?;
            circuit.enforce_constant(bad, Fr::from(0u64))?;
        }
    }

    // K = Σ active_i.
    let zero_var = circuit.zero();
    let mut k_var = zero_var;
    for av in active_vars.iter() {
        let av_var: Variable = (*av).into();
        k_var = circuit.add(k_var, av_var)?;
    }

    // K ≥ threshold encoded as `K = threshold + slack` with both sides
    // 2-bit-range-checked (covers `[0, 3]`, sufficient for K_MAX = 2).
    // The threshold range gate closes the underflow path: without it,
    // an attacker could pick `threshold ≡ -slack (mod p)` and pass
    // `K = 0`. Mirrors the soundness fix from PR #201's review round 2
    // for democracy.
    let two_var = circuit.create_constant_variable(Fr::from(2u64))?;

    let thresh_bit0 =
        circuit.create_boolean_variable((witness.admin_threshold_numerator & 1) == 1)?;
    let thresh_bit1 = circuit
        .create_boolean_variable(((witness.admin_threshold_numerator >> 1) & 1) == 1)?;
    let thresh_b0_var: Variable = thresh_bit0.into();
    let thresh_b1_var: Variable = thresh_bit1.into();
    let thresh_b1_scaled = circuit.mul(thresh_b1_var, two_var)?;
    let thresh_decomp = circuit.add(thresh_b0_var, thresh_b1_scaled)?;
    circuit.enforce_equal(thresh_decomp, threshold_var)?;

    let slack_value = (witness.admin_signers.iter().filter(|s| s.active).count() as u64)
        .saturating_sub(witness.admin_threshold_numerator);
    let slack_bit0 = circuit.create_boolean_variable((slack_value & 1) == 1)?;
    let slack_bit1 = circuit.create_boolean_variable(((slack_value >> 1) & 1) == 1)?;
    let slack_b0_var: Variable = slack_bit0.into();
    let slack_b1_var: Variable = slack_bit1.into();
    let slack_b1_scaled = circuit.mul(slack_b1_var, two_var)?;
    let slack_var = circuit.add(slack_b0_var, slack_b1_scaled)?;
    let lhs = circuit.add(threshold_var, slack_var)?;
    circuit.enforce_equal(lhs, k_var)?;

    // ---- 2. Member count + occupancy binding (count-only delta) ----
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

    // ---- 3. Commitment chain (matches synthesize_oligarchy_create) ----
    // c_X = Poseidon(Poseidon(Poseidon(member_root_X, epoch_X), salt_X),
    //               Poseidon(occ_X, admin_root_X))
    let inner_old = poseidon_hash_two_gadget(circuit, member_root_old_var, epoch_old_var)?;
    let mid_old = poseidon_hash_two_gadget(circuit, inner_old, salt_old_var)?;
    let admin_mix_old =
        poseidon_hash_two_gadget(circuit, occ_old_var, admin_root_old_var)?;
    let computed_c_old = poseidon_hash_two_gadget(circuit, mid_old, admin_mix_old)?;
    circuit.enforce_equal(computed_c_old, c_old_var)?;

    let epoch_new_var = circuit.add_constant(epoch_old_var, &Fr::from(1u64))?;
    let inner_new = poseidon_hash_two_gadget(circuit, member_root_new_var, epoch_new_var)?;
    let mid_new = poseidon_hash_two_gadget(circuit, inner_new, salt_new_var)?;
    let admin_mix_new =
        poseidon_hash_two_gadget(circuit, occ_new_var, admin_root_new_var)?;
    let computed_c_new = poseidon_hash_two_gadget(circuit, mid_new, admin_mix_new)?;
    circuit.enforce_equal(computed_c_new, c_new_var)?;

    Ok(())
}

// ================================================================
// Oligarchy-specific membership circuit (issue #208).
// ================================================================
//
// `synthesize_oligarchy_membership` mirrors the standard membership
// circuit's role — proving "this caller knows a secret_key whose
// Poseidon-leaf opens to `member_root` at some leaf_index, and the
// resulting commitment matches what's stored on-chain" — but binds
// the commitment under oligarchy's **3-level chain** (the same one
// `synthesize_oligarchy_create` and `synthesize_oligarchy_update_quorum`
// use):
//
//   c = Poseidon(Poseidon(Poseidon(member_root, epoch), salt),
//                Poseidon(occupancy_commitment, admin_root))
//
// The standard membership circuit's 2-level chain
// (`Poseidon(Poseidon(member_root, epoch), salt)`) doesn't match the
// stored c after `create_oligarchy_group`, so a member proving
// against the wrong chain shape produces a c that the contract's
// `public_inputs[0] == state.commitment` PI gate rejects. This
// circuit closes that gap.
//
// **Public inputs (2, fixed order — matches standard membership for
// wire-shape compatibility):**
//   1. `commitment`
//   2. `epoch`
//
// **Private witnesses:** `secret_key, member_root, salt, merkle_path,
// leaf_index, occupancy_commitment, admin_root`. The last two are
// per-state quantities the prover gets via off-chain group
// coordination (admin_root specifically is documented in
// `oligarchy.rs`'s create-circuit doc as a "circuit-internal private
// witness reconstructed off-chain by group members").
//
// **Soundness:** the 4-input chain
// `c = Poseidon(Poseidon(Poseidon(root, epoch), salt),
//               Poseidon(occ, admin_root))`
// has Poseidon collision-resistance: given a fixed PI `c`, finding
// any `(root, salt, occ, admin_root)` that hashes to it is infeasible
// regardless of the prover's freedom over the witnesses. The prover
// MUST use the real values the legitimate group uses.
//
// Per-tier (same 3 depths as standard membership: 5 / 8 / 11). The
// circuit shape is identical to standard membership plus 2 extra
// Poseidon hashes (the `H(occ, admin_root)` step + the outer wrap),
// so gate count grows by ~500 over standard membership and fits
// comfortably in the n=32768 SRS.

/// Witness inputs for the oligarchy-specific membership circuit.
///
/// `commitment` and `epoch` are the **public** inputs allocated in
/// `synthesize_oligarchy_membership`; the rest are private witnesses.
pub struct OligarchyMembershipWitness {
    /// 3-level Poseidon-bound oligarchy commitment — public.
    pub commitment: Fr,
    /// Group epoch — public.
    pub epoch: u64,
    /// Prover's BLS12-381 scalar — private.
    pub secret_key: Fr,
    /// Member-tree Poseidon Merkle root — private.
    pub member_root: Fr,
    /// 32-byte per-state salt; reduced mod r in-circuit — private.
    pub salt: [u8; 32],
    /// Sibling hashes from leaf to member_root, length = depth — private.
    pub merkle_path: Vec<Fr>,
    /// Leaf position in the member tree — private.
    pub leaf_index: usize,
    /// Member tree depth.
    pub depth: usize,
    /// Per-state occupancy commitment — private. (Same value the
    /// contract stores in `CommitmentEntry.occupancy_commitment`; not
    /// promoted to a public input so wire-PI shape stays compatible
    /// with the standard membership circuit.)
    pub occupancy_commitment: Fr,
    /// Per-state admin tree root — private. Off-chain quantity per
    /// design v0.1.4 §3.5 (admin_root is reconstructed off-chain by
    /// group members; never stored on-chain).
    pub admin_root: Fr,
}

/// Allocate the oligarchy-specific membership circuit. Public-input
/// ordering is byte-identical to `synthesize_membership`'s
/// `(commitment, epoch)` so the contract surface (PI count + shape)
/// is unchanged when `verify_membership` swaps to this VK.
pub fn synthesize_oligarchy_membership(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &OligarchyMembershipWitness,
) -> Result<Variable, CircuitError> {
    if witness.merkle_path.len() != witness.depth {
        return Err(CircuitError::ParameterError(format!(
            "merkle_path length {} != depth {}",
            witness.merkle_path.len(),
            witness.depth
        )));
    }
    if witness.depth >= usize::BITS as usize {
        return Err(CircuitError::ParameterError(format!(
            "depth {} >= usize::BITS",
            witness.depth
        )));
    }
    if witness.leaf_index >= (1usize << witness.depth) {
        return Err(CircuitError::ParameterError(format!(
            "leaf_index {} out of range for depth {}",
            witness.leaf_index, witness.depth,
        )));
    }

    // ---- Public inputs (fixed order, matches standard membership) ----
    let commitment_var = circuit.create_public_variable(witness.commitment)?;
    let epoch_var = circuit.create_public_variable(Fr::from(witness.epoch))?;

    // ---- Private witnesses ----
    let secret_key_var = circuit.create_variable(witness.secret_key)?;
    let member_root_var = circuit.create_variable(witness.member_root)?;
    let salt_fr = Fr::from_le_bytes_mod_order(&witness.salt);
    let salt_var = circuit.create_variable(salt_fr)?;
    let occ_var = circuit.create_variable(witness.occupancy_commitment)?;
    let admin_root_var = circuit.create_variable(witness.admin_root)?;

    let path_vars: Vec<Variable> = witness
        .merkle_path
        .iter()
        .map(|sibling| circuit.create_variable(*sibling))
        .collect::<Result<_, _>>()?;
    let bit_vars: Vec<BoolVar> = (0..witness.depth)
        .map(|i| circuit.create_boolean_variable(((witness.leaf_index >> i) & 1) == 1))
        .collect::<Result<_, _>>()?;

    // ---- 1. leaf = Poseidon(secret_key) ----
    let leaf_var = poseidon_hash_one_gadget(circuit, secret_key_var)?;

    // ---- 2. Merkle membership against member_root ----
    let computed_root =
        compute_merkle_root_gadget(circuit, leaf_var, &path_vars, &bit_vars)?;
    circuit.enforce_equal(computed_root, member_root_var)?;

    // ---- 3. 3-level commitment chain — matches create + update_quorum ----
    let inner = poseidon_hash_two_gadget(circuit, member_root_var, epoch_var)?;
    let mid = poseidon_hash_two_gadget(circuit, inner, salt_var)?;
    let admin_mix = poseidon_hash_two_gadget(circuit, occ_var, admin_root_var)?;
    let computed_c = poseidon_hash_two_gadget(circuit, mid, admin_mix)?;
    circuit.enforce_equal(computed_c, commitment_var)?;

    Ok(commitment_var)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::plonk::poseidon::poseidon_hash_two_v05;

    fn native_create_commitment(
        member_root: Fr,
        salt: Fr,
        occ: Fr,
        admin_root: Fr,
    ) -> Fr {
        let inner = poseidon_hash_two_v05(&member_root, &Fr::from(0u64));
        let mid = poseidon_hash_two_v05(&inner, &salt);
        let admin_mix = poseidon_hash_two_v05(&occ, &admin_root);
        poseidon_hash_two_v05(&mid, &admin_mix)
    }

    fn native_update_c(
        member_root: Fr,
        epoch: u64,
        salt: Fr,
        occ: Fr,
    ) -> Fr {
        let inner = poseidon_hash_two_v05(&member_root, &Fr::from(epoch));
        let mid = poseidon_hash_two_v05(&inner, &salt);
        poseidon_hash_two_v05(&mid, &occ)
    }

    #[test]
    fn create_satisfies() {
        let occ = Fr::from(100u64);
        let member_root = Fr::from(200u64);
        let admin_root = Fr::from(300u64);
        let salt = Fr::from(400u64);
        let c = native_create_commitment(member_root, salt, occ, admin_root);
        let w = OligarchyCreateWitness {
            commitment: c,
            occupancy_commitment: occ,
            member_root,
            admin_root,
            salt_initial: salt,
        };
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_create(&mut circuit, &w).unwrap();
        circuit
            .check_circuit_satisfiability(&[c, Fr::from(0u64), occ, member_root, admin_root, salt])
            .unwrap();
    }

    #[test]
    fn update_satisfies() {
        let member_root = Fr::from(11u64);
        let salt_old = Fr::from(22u64);
        let salt_new = Fr::from(33u64);
        let occ_old = Fr::from(44u64);
        let occ_new = Fr::from(55u64);
        let epoch_old = 7u64;
        let threshold = 5u64;
        let c_old = native_update_c(member_root, epoch_old, salt_old, occ_old);
        let c_new = native_update_c(member_root, epoch_old + 1, salt_new, occ_new);
        let w = OligarchyUpdateWitness {
            c_old,
            epoch_old,
            c_new,
            occupancy_commitment_old: occ_old,
            occupancy_commitment_new: occ_new,
            admin_threshold_numerator: threshold,
            member_root_old: member_root,
            member_root_new: member_root,
            salt_old,
            salt_new,
        };
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update(&mut circuit, &w).unwrap();
        circuit
            .check_circuit_satisfiability(&[
                c_old,
                Fr::from(epoch_old),
                c_new,
                occ_old,
                occ_new,
                Fr::from(threshold),
            ])
            .unwrap();
    }

    #[test]
    fn create_round_trip() {
        use rand_chacha::rand_core::SeedableRng;
        let occ = Fr::from(100u64);
        let member_root = Fr::from(200u64);
        let admin_root = Fr::from(300u64);
        let salt = Fr::from(400u64);
        let c = native_create_commitment(member_root, salt, occ, admin_root);
        let w = OligarchyCreateWitness {
            commitment: c,
            occupancy_commitment: occ,
            member_root,
            admin_root,
            salt_initial: salt,
        };
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_create(&mut circuit, &w).unwrap();
        circuit.finalize_for_arithmetization().unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&circuit).unwrap();
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &circuit).unwrap();
        let pi = vec![c, Fr::from(0u64), occ, member_root, admin_root, salt];
        crate::prover::plonk::verify(&keys.vk, &pi, &proof).unwrap();
    }

    #[test]
    fn update_round_trip() {
        use rand_chacha::rand_core::SeedableRng;
        let member_root = Fr::from(11u64);
        let salt_old = Fr::from(22u64);
        let salt_new = Fr::from(33u64);
        let occ_old = Fr::from(44u64);
        let occ_new = Fr::from(55u64);
        let epoch_old = 7u64;
        let threshold = 5u64;
        let c_old = native_update_c(member_root, epoch_old, salt_old, occ_old);
        let c_new = native_update_c(member_root, epoch_old + 1, salt_new, occ_new);
        let w = OligarchyUpdateWitness {
            c_old,
            epoch_old,
            c_new,
            occupancy_commitment_old: occ_old,
            occupancy_commitment_new: occ_new,
            admin_threshold_numerator: threshold,
            member_root_old: member_root,
            member_root_new: member_root,
            salt_old,
            salt_new,
        };
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update(&mut circuit, &w).unwrap();
        circuit.finalize_for_arithmetization().unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&circuit).unwrap();
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &circuit).unwrap();
        let pi = vec![
            c_old,
            Fr::from(epoch_old),
            c_new,
            occ_old,
            occ_new,
            Fr::from(threshold),
        ];
        crate::prover::plonk::verify(&keys.vk, &pi, &proof).unwrap();
    }

    // ============================================================
    // Quorum + delta + admin-membership update circuit (issue #202)
    // ============================================================

    use crate::circuit::plonk::poseidon::poseidon_hash_one_v05;

    /// Build a complete admin tree from `secret_keys` and return the
    /// root + every leaf's Merkle path. Mirrors `democracy.rs`'s
    /// helper but with a fixed depth.
    fn build_admin_tree(secret_keys: &[Fr], depth: usize) -> (Fr, Vec<Vec<Fr>>) {
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

    /// Native equivalent of the quorum circuit's commitment chain —
    /// matches `synthesize_oligarchy_create` so create→update lineage
    /// is consistent.
    fn native_quorum_c(
        member_root: Fr,
        epoch: u64,
        salt: Fr,
        occ: Fr,
        admin_root: Fr,
    ) -> Fr {
        let inner = poseidon_hash_two_v05(&member_root, &Fr::from(epoch));
        let mid = poseidon_hash_two_v05(&inner, &salt);
        let admin_mix = poseidon_hash_two_v05(&occ, &admin_root);
        poseidon_hash_two_v05(&mid, &admin_mix)
    }

    /// Build a deterministic K-active quorum witness. `admin_count`
    /// determines how many slots are flagged active (must be
    /// `<= OLIGARCHY_K_MAX`). Inactive slots get dummy values.
    fn build_quorum_witness(
        admin_secret_keys: &[Fr],
        admin_count: usize,
        threshold: u64,
        epoch_old: u64,
        member_count_old: u64,
        member_count_new: u64,
    ) -> OligarchyUpdateQuorumWitness {
        assert!(admin_count <= OLIGARCHY_K_MAX);
        assert!(admin_count <= admin_secret_keys.len());
        let (admin_root, paths) = build_admin_tree(admin_secret_keys, OLIGARCHY_ADMIN_DEPTH);
        let member_root = Fr::from(0xCAFEu64);
        let salt_old = Fr::from(0xEEEEu64);
        let salt_new = Fr::from(0xFFFFu64);
        let salt_oc_old = Fr::from(0x55u64);
        let salt_oc_new = Fr::from(0x66u64);
        let occ_old =
            poseidon_hash_two_v05(&Fr::from(member_count_old), &salt_oc_old);
        let occ_new =
            poseidon_hash_two_v05(&Fr::from(member_count_new), &salt_oc_new);
        let c_old = native_quorum_c(member_root, epoch_old, salt_old, occ_old, admin_root);
        let c_new =
            native_quorum_c(member_root, epoch_old + 1, salt_new, occ_new, admin_root);

        let dummy_path = vec![Fr::from(0u64); OLIGARCHY_ADMIN_DEPTH];
        let admin_signers: [OligarchyAdminSigner; OLIGARCHY_K_MAX] =
            core::array::from_fn(|i| {
                if i < admin_count {
                    OligarchyAdminSigner {
                        secret_key: admin_secret_keys[i],
                        merkle_path: paths[i].clone(),
                        leaf_index: i,
                        active: true,
                    }
                } else {
                    OligarchyAdminSigner {
                        secret_key: Fr::from(0u64),
                        merkle_path: dummy_path.clone(),
                        leaf_index: 0,
                        active: false,
                    }
                }
            });

        OligarchyUpdateQuorumWitness {
            c_old,
            epoch_old,
            c_new,
            occupancy_commitment_old: occ_old,
            occupancy_commitment_new: occ_new,
            admin_threshold_numerator: threshold,
            admin_signers,
            member_root_old: member_root,
            member_root_new: member_root,
            admin_root_old: admin_root,
            admin_root_new: admin_root,
            member_count_old,
            member_count_new,
            salt_oc_old,
            salt_oc_new,
            salt_old,
            salt_new,
        }
    }

    fn pi_quorum(w: &OligarchyUpdateQuorumWitness) -> Vec<Fr> {
        vec![
            w.c_old,
            Fr::from(w.epoch_old),
            w.c_new,
            w.occupancy_commitment_old,
            w.occupancy_commitment_new,
            Fr::from(w.admin_threshold_numerator),
        ]
    }

    #[test]
    fn quorum_satisfies_with_full_quorum() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 42, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        c.check_circuit_satisfiability(&pi_quorum(&w)).unwrap();
    }

    #[test]
    fn quorum_satisfies_with_quorum_above_threshold() {
        // K = K_MAX, threshold = 1 → slack = 1 fits 2 bits.
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, 1, 42, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        c.check_circuit_satisfiability(&pi_quorum(&w)).unwrap();
    }

    /// Positive baseline for `quorum_rejects_out_of_range_threshold_pi`.
    #[test]
    fn quorum_satisfies_zero_quorum_zero_threshold() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, 0, 0, 42, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        c.check_circuit_satisfiability(&pi_quorum(&w)).unwrap();
    }

    #[test]
    fn quorum_satisfies_count_delta_plus_one() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 42, 5, 6);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        c.check_circuit_satisfiability(&pi_quorum(&w)).unwrap();
    }

    /// Member-removal path: count_new = count_old - 1.
    #[test]
    fn quorum_satisfies_count_delta_minus_one() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 42, 5, 4);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        c.check_circuit_satisfiability(&pi_quorum(&w)).unwrap();
    }

    #[test]
    fn quorum_rejects_k_below_threshold() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let mut w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 42, 5, 5);
        w.admin_signers[OLIGARCHY_K_MAX - 1].active = false;
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        assert!(c.check_circuit_satisfiability(&pi_quorum(&w)).is_err());
    }

    #[test]
    fn quorum_rejects_count_delta_too_large() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 42, 5, 7);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        assert!(c.check_circuit_satisfiability(&pi_quorum(&w)).is_err());
    }

    #[test]
    fn quorum_rejects_active_prefix_violation() {
        // active = [false, true] — prefix violated.
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let mut w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, 1, 42, 5, 5);
        w.admin_signers[0].active = false;
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        assert!(c.check_circuit_satisfiability(&pi_quorum(&w)).is_err());
    }

    /// An inactive slot can hold a perfectly valid Merkle opening to
    /// some *other* admin tree — the active-conditional gate must
    /// mask it. Verifies the gating, not just dummy zeros.
    #[test]
    fn quorum_inactive_slot_with_valid_different_admin_root_passes() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let mut w = build_quorum_witness(&sks, 1, 1, 42, 5, 5);
        let other_sks: Vec<Fr> = (100u64..104).map(Fr::from).collect();
        let (_other_root, other_paths) =
            build_admin_tree(&other_sks, OLIGARCHY_ADMIN_DEPTH);
        w.admin_signers[1] = OligarchyAdminSigner {
            secret_key: other_sks[0],
            merkle_path: other_paths[0].clone(),
            leaf_index: 7,
            active: false,
        };
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        c.check_circuit_satisfiability(&pi_quorum(&w)).unwrap();
    }

    #[test]
    fn quorum_rejects_duplicate_leaf_double_count() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let mut w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 42, 5, 5);
        w.admin_signers[1] = w.admin_signers[0].clone();
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        assert!(c.check_circuit_satisfiability(&pi_quorum(&w)).is_err());
    }

    #[test]
    fn quorum_rejects_tampered_active_merkle_path() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let mut w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 42, 5, 5);
        w.admin_signers[0].merkle_path[1] += Fr::from(1u64);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        assert!(c.check_circuit_satisfiability(&pi_quorum(&w)).is_err());
    }

    /// Tampering `admin_root_old` (private witness) must break the
    /// active-conditional Merkle gate even though it isn't a public
    /// input. Pins the signer→`admin_root_old` binding.
    #[test]
    fn quorum_rejects_tampered_admin_root_old() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let mut w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 42, 5, 5);
        w.admin_root_old += Fr::from(1u64);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        assert!(c.check_circuit_satisfiability(&pi_quorum(&w)).is_err());
    }

    #[test]
    fn quorum_rejects_tampered_c_old_pi() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 42, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        let mut bad_pi = pi_quorum(&w);
        bad_pi[0] += Fr::from(1u64);
        assert!(c.check_circuit_satisfiability(&bad_pi).is_err());
    }

    #[test]
    fn quorum_rejects_tampered_epoch_old_pi() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 42, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        let mut bad_pi = pi_quorum(&w);
        bad_pi[1] += Fr::from(1u64);
        assert!(c.check_circuit_satisfiability(&bad_pi).is_err());
    }

    #[test]
    fn quorum_rejects_tampered_c_new_pi() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 42, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        let mut bad_pi = pi_quorum(&w);
        bad_pi[2] += Fr::from(1u64);
        assert!(c.check_circuit_satisfiability(&bad_pi).is_err());
    }

    #[test]
    fn quorum_rejects_tampered_occ_old_pi() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 42, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        let mut bad_pi = pi_quorum(&w);
        bad_pi[3] += Fr::from(1u64);
        assert!(c.check_circuit_satisfiability(&bad_pi).is_err());
    }

    #[test]
    fn quorum_rejects_tampered_occ_new_pi() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 42, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        let mut bad_pi = pi_quorum(&w);
        bad_pi[4] += Fr::from(1u64);
        assert!(c.check_circuit_satisfiability(&bad_pi).is_err());
    }

    /// `threshold ≡ -3 (mod p)` would let an attacker pass `K = 0`
    /// without the threshold range gate. Companion:
    /// `quorum_satisfies_zero_quorum_zero_threshold` confirms the
    /// unpatched PI baseline is satisfiable.
    #[test]
    fn quorum_rejects_out_of_range_threshold_pi() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, 0, 0, 42, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        let mut bad_pi = pi_quorum(&w);
        bad_pi[5] = -Fr::from(3u64);
        assert!(c.check_circuit_satisfiability(&bad_pi).is_err());
    }

    /// `threshold = 3` is in-range for the 2-bit threshold gate but
    /// unsatisfiable with `K_MAX = 2`: slack underflows. Pins the
    /// slack range check.
    #[test]
    fn quorum_rejects_threshold_above_k_max() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, 3, 42, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        assert!(c.check_circuit_satisfiability(&pi_quorum(&w)).is_err());
    }

    /// Gate-count guard: the quorum circuit must fit n=32768 SRS for
    /// the on-chain verifier to accept it. Admin tree depth is fixed
    /// at 5 across all member tiers, so the circuit shape (and gate
    /// count) is independent of member tier.
    #[test]
    fn quorum_gate_count_under_srs_ceiling() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 42, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        c.finalize_for_arithmetization().unwrap();
        eprintln!(
            "[gate-count] oligarchy_update_quorum K={OLIGARCHY_K_MAX} admin_depth={OLIGARCHY_ADMIN_DEPTH}: {} gates",
            c.num_gates()
        );
        assert!(
            c.num_gates() < 32768,
            "oligarchy quorum ({} gates) over SRS ceiling",
            c.num_gates(),
        );
    }

    #[test]
    fn quorum_round_trip() {
        use rand_chacha::rand_core::SeedableRng;
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_quorum_witness(&sks, OLIGARCHY_K_MAX, OLIGARCHY_K_MAX as u64, 1234, 5, 6);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update_quorum(&mut c, &w).unwrap();
        c.finalize_for_arithmetization().unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&c).unwrap();
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &c).unwrap();
        crate::prover::plonk::verify(&keys.vk, &pi_quorum(&w), &proof).unwrap();
    }

    // ============================================================
    // Oligarchy-specific membership circuit (issue #208)
    // ============================================================

    /// Build a member tree from `secret_keys` and return root + paths.
    fn build_member_tree(secret_keys: &[Fr], depth: usize) -> (Fr, Vec<Vec<Fr>>) {
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

    /// Native equivalent of the oligarchy 3-level commitment chain.
    fn native_oligarchy_c(
        member_root: Fr,
        epoch: u64,
        salt: Fr,
        occ: Fr,
        admin_root: Fr,
    ) -> Fr {
        let inner = poseidon_hash_two_v05(&member_root, &Fr::from(epoch));
        let mid = poseidon_hash_two_v05(&inner, &salt);
        let admin_mix = poseidon_hash_two_v05(&occ, &admin_root);
        poseidon_hash_two_v05(&mid, &admin_mix)
    }

    fn build_membership_witness(
        secret_keys: &[Fr],
        prover_index: usize,
        depth: usize,
        epoch: u64,
    ) -> OligarchyMembershipWitness {
        let (root, paths) = build_member_tree(secret_keys, depth);
        let salt = [0xAAu8; 32];
        let salt_fr = Fr::from_le_bytes_mod_order(&salt);
        let occ = Fr::from(0xA110u64);
        let admin_root = Fr::from(0xADADu64);
        let commitment = native_oligarchy_c(root, epoch, salt_fr, occ, admin_root);
        OligarchyMembershipWitness {
            commitment,
            epoch,
            secret_key: secret_keys[prover_index],
            member_root: root,
            salt,
            merkle_path: paths[prover_index].clone(),
            leaf_index: prover_index,
            depth,
            occupancy_commitment: occ,
            admin_root,
        }
    }

    #[test]
    fn oligarchy_membership_satisfies() {
        let sks: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let w = build_membership_witness(&sks, 3, 5, 1234);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_membership(&mut c, &w).unwrap();
        c.check_circuit_satisfiability(&[w.commitment, Fr::from(w.epoch)]).unwrap();
    }

    #[test]
    fn oligarchy_membership_rejects_wrong_admin_root() {
        let sks: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let mut w = build_membership_witness(&sks, 3, 5, 1234);
        // Tampering admin_root flips the H(occ, admin_root) leg, breaking
        // the 3-level commitment chain. Pins that admin_root is bound
        // into the on-chain commitment.
        w.admin_root += Fr::from(1u64);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_membership(&mut c, &w).unwrap();
        assert!(c
            .check_circuit_satisfiability(&[w.commitment, Fr::from(w.epoch)])
            .is_err());
    }

    #[test]
    fn oligarchy_membership_rejects_wrong_occupancy() {
        let sks: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let mut w = build_membership_witness(&sks, 3, 5, 1234);
        w.occupancy_commitment += Fr::from(1u64);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_membership(&mut c, &w).unwrap();
        assert!(c
            .check_circuit_satisfiability(&[w.commitment, Fr::from(w.epoch)])
            .is_err());
    }

    #[test]
    fn oligarchy_membership_rejects_non_member_secret_key() {
        let sks: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let mut w = build_membership_witness(&sks, 3, 5, 1234);
        // Replace the secret key with one not in the tree — the Merkle
        // gate must reject regardless of the c-chain reconciliation.
        w.secret_key = Fr::from(999u64);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_membership(&mut c, &w).unwrap();
        assert!(c
            .check_circuit_satisfiability(&[w.commitment, Fr::from(w.epoch)])
            .is_err());
    }

    #[test]
    fn oligarchy_membership_rejects_tampered_merkle_path() {
        let sks: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let mut w = build_membership_witness(&sks, 3, 5, 1234);
        w.merkle_path[1] += Fr::from(1u64);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_membership(&mut c, &w).unwrap();
        assert!(c
            .check_circuit_satisfiability(&[w.commitment, Fr::from(w.epoch)])
            .is_err());
    }

    #[test]
    fn oligarchy_membership_rejects_tampered_commitment_pi() {
        let sks: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let w = build_membership_witness(&sks, 3, 5, 1234);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_membership(&mut c, &w).unwrap();
        let bad = w.commitment + Fr::from(1u64);
        assert!(c
            .check_circuit_satisfiability(&[bad, Fr::from(w.epoch)])
            .is_err());
    }

    #[test]
    fn oligarchy_membership_rejects_tampered_epoch_pi() {
        let sks: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let w = build_membership_witness(&sks, 3, 5, 1234);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_membership(&mut c, &w).unwrap();
        assert!(c
            .check_circuit_satisfiability(&[w.commitment, Fr::from(w.epoch + 1)])
            .is_err());
    }

    /// Gate-count guard at every supported tier — fits n=32768 SRS.
    #[test]
    fn oligarchy_membership_gate_count_per_tier() {
        for &depth in &[5usize, 8, 11] {
            let sks: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
            let w = build_membership_witness(&sks, 3, depth, 1234);
            let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
            synthesize_oligarchy_membership(&mut c, &w).unwrap();
            c.finalize_for_arithmetization().unwrap();
            std::eprintln!(
                "[gate-count] oligarchy_membership depth={}: {} gates",
                depth,
                c.num_gates()
            );
            assert!(c.num_gates() < 32768);
        }
    }

    #[test]
    fn oligarchy_membership_round_trip() {
        use rand_chacha::rand_core::SeedableRng;
        let sks: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let w = build_membership_witness(&sks, 3, 5, 1234);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_membership(&mut c, &w).unwrap();
        c.finalize_for_arithmetization().unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&c).unwrap();
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &c).unwrap();
        crate::prover::plonk::verify(
            &keys.vk,
            &[w.commitment, Fr::from(w.epoch)],
            &proof,
        )
        .unwrap();
    }
}
