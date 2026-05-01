//! Democracy `Update` circuit (PLONK port) — K-of-N quorum + count delta.
//!
//! ## Status — quorum + count delta enforced
//!
//! This iteration upgrades the previous "commitment-only" port (PR
//! #199) to enforce a K-of-N admin quorum, occupancy/count binding,
//! and a count-only single-leaf delta on `member_count`. The
//! simplified single-signer circuit below carries forward as the
//! d=11 fallback.
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
//!   1. K-of-N quorum (`K_MAX = 2`):
//!      - 2 signer slots; each `(sk, merkle_path, leaf_idx, active)`.
//!      - `active_i` is boolean per slot.
//!      - Active slots are a strict prefix (`active_i ⇒ active_{i-1}`).
//!      - For each active slot: `Poseidon(sk_i)` opens to
//!        `member_root_old` at `leaf_idx_i`.
//!      - For any pair of active slots, `leaf_idx_i ≠ leaf_idx_j`
//!        (anti-double-count: prevents one signer filling both slots).
//!      - K = Σ active_i.
//!      - K ≥ threshold_numerator. Encoded as `K = threshold + slack`
//!        with both `threshold` and `slack` range-checked into 2 bits
//!        (i.e. `[0, 3]`, sufficient for `K_MAX = 2`). The threshold
//!        range gate closes the underflow path: without it, an
//!        attacker submitting `threshold ≡ -slack (mod p)` would
//!        satisfy the equation with `K = 0` (zero active signers).
//!        Raising `K_MAX` past 3 requires widening *both* bit
//!        decompositions in lockstep — guarded by a `const _: () =
//!        assert!(K_MAX <= 3)` next to the constant.
//!
//!   2. Occupancy / count binding (count delta only):
//!      - `occupancy_commitment_old = Poseidon(member_count_old, salt_oc_old)`.
//!      - `occupancy_commitment_new = Poseidon(member_count_new, salt_oc_new)`.
//!      - `|member_count_new - member_count_old| ≤ 1`, encoded as
//!        `(diff)(diff-1)(diff+1) = 0` over Fr.
//!
//!     **Scope of the delta.** This binds only the *scalar count*, not
//!     the *tree*. `member_root_new` is a free witness — an active
//!     quorum can rotate to any new root as long as the count delta
//!     stays in `{-1, 0, +1}`. A tree-level single-leaf delta proof
//!     (the new root differs from the old by exactly one leaf at
//!     `leaf_idx_target`) is deferred to a follow-up; the contract
//!     surface and PI shape are stable for the upgrade.
//!
//!     **Count range.** `member_count_*` field elements are *not*
//!     range-checked in-circuit. Soundness for u64 semantics relies on
//!     the off-circuit binding: the stored `occupancy_commitment` was
//!     originally produced from a u64 count, and Poseidon collision
//!     resistance forces the witness to recover the same u64. Callers
//!     deriving the count from the witness MUST go through the
//!     commitment binding — never trust the raw witness scalar.
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
//!  - **Strict-ascending `leaf_idx`.** Not enforced; only pairwise
//!    distinctness is. Strict ordering (canonicalisation) is a
//!    follow-up — distinctness alone is sufficient to prevent the
//!    duplicate-leaf double-count attack at K_MAX=2.
//!  - **Tree-level single-leaf delta.** Not enforced; only the count
//!    delta is. See constraint 2.
//!  - **K_MAX = 2.** Caps quorum size at 2 signers in this PR.
//!    Raising to 3+ is straightforward but inflates the circuit
//!    (extra Merkle opening per slot); keeping K_MAX small ensures
//!    the n=32768 SRS budget stays well within reach at depth=8.
//!    Depth=11 still falls back to the simplified single-signer
//!    circuit below.

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use ark_ff_v05::PrimeField;
use jf_relation::{BoolVar, Circuit, CircuitError, PlonkCircuit, Variable};

use super::merkle::compute_merkle_root_gadget;
use super::poseidon::{poseidon_hash_one_gadget, poseidon_hash_two_gadget};

/// Quorum cap in this initial port.
pub const K_MAX: usize = 2;

/// Slack and threshold are both range-checked into 2 bits — sufficient
/// for `K_MAX <= 3`. Raising `K_MAX` past 3 silently breaks soundness
/// of the `K ≥ threshold` gate; widen both bit decompositions in
/// lockstep before bumping this constant.
const _: () = assert!(K_MAX <= 3, "widen slack + threshold ranges before raising K_MAX past 3");

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
    let one_var = circuit.create_constant_variable(Fr::from(1u64))?;
    for i in 1..K_MAX {
        let prev_var: Variable = active_vars[i - 1].into();
        let neg_prev = circuit.sub(one_var, prev_var)?;
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
    let mut leaf_idx_field_vars: Vec<Variable> = Vec::with_capacity(K_MAX);
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

        // Compose leaf_index as a field var from its bit decomposition,
        // so we can enforce pairwise distinctness across active slots
        // below. Constrains leaf_idx_var = Σ_j bit_j · 2^j.
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

        // active=1 ⇒ computed_root == root_old. Encoded as:
        //   active * (computed_root - root_old) = 0
        let diff = circuit.sub(computed_root, root_old_var)?;
        let active_var: Variable = active_vars[i].into();
        let prod = circuit.mul(active_var, diff)?;
        circuit.enforce_constant(prod, Fr::from(0u64))?;
    }

    // Anti-double-count: forbid two active slots from sharing the
    // same leaf_idx. For every pair (j, i) with i > j, enforce
    //   active_j · active_i · is_equal(leaf_idx_j, leaf_idx_i) = 0
    // i.e. if both slots are active, their leaf indices must differ.
    //
    // **Distinctness is on `leaf_idx`, not on `Poseidon(sk)`.** This
    // assumes member-tree uniqueness (no two distinct leaf positions
    // carry the same `Poseidon(sk)`) — a witness-construction
    // invariant established off-circuit when the tree is built from
    // a deduplicated secret-key set. If that invariant ever weakens,
    // a single signer could occupy multiple `leaf_idx` slots and
    // double-count under this check.
    for i in 1..K_MAX {
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

    // K = sum(active_i)
    let zero_var = circuit.zero();
    let mut k_var = zero_var;
    for av in active_vars.iter() {
        let av_var: Variable = (*av).into();
        k_var = circuit.add(k_var, av_var)?;
    }

    // K ≥ threshold ⇔ K = threshold + slack with slack, threshold ∈
    // [0, K_MAX]. Range-check both via 2-bit boolean decomposition.
    //
    // **Why range-check threshold.** Without it, an attacker submitting
    // `threshold ≡ -slack (mod p)` (e.g. `threshold ≡ -3 (mod p)` with
    // `slack = 3`) satisfies `K = threshold + slack` with `K = 0`
    // active signers. Bounding `threshold ∈ [0, 3]` in-circuit closes
    // the underflow path independently of caller-side validation.
    //
    // `saturating_sub` for slack witness-generation is a convenience
    // only — any wrong slack value still fails the in-circuit
    // `K = threshold + slack` equality below, so saturation is not
    // load-bearing for soundness.
    let two_var = circuit.create_constant_variable(Fr::from(2u64))?;

    let thresh_bit0 =
        circuit.create_boolean_variable((witness.threshold_numerator & 1) == 1)?;
    let thresh_bit1 =
        circuit.create_boolean_variable(((witness.threshold_numerator >> 1) & 1) == 1)?;
    let thresh_b0_var: Variable = thresh_bit0.into();
    let thresh_b1_var: Variable = thresh_bit1.into();
    let thresh_b1_scaled = circuit.mul(thresh_b1_var, two_var)?;
    let thresh_decomp = circuit.add(thresh_b0_var, thresh_b1_scaled)?;
    circuit.enforce_equal(thresh_decomp, threshold_var)?;

    let slack_value = (witness.signers.iter().filter(|s| s.active).count() as u64)
        .saturating_sub(witness.threshold_numerator);
    let slack_bit0 = circuit.create_boolean_variable((slack_value & 1) == 1)?;
    let slack_bit1 = circuit.create_boolean_variable(((slack_value >> 1) & 1) == 1)?;
    let slack_b0_var: Variable = slack_bit0.into();
    let slack_b1_var: Variable = slack_bit1.into();
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
    if witness.leaf_index_old >= (1usize << witness.depth) {
        return Err(CircuitError::ParameterError(format!(
            "leaf_index_old {} out of range",
            witness.leaf_index_old
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

// ================================================================
// Create circuit (issue #5 / Phase C.4.c)
// ================================================================
//
// `synthesize_democracy_create` produces a c with the **same 3-level
// chain** that `synthesize_democracy_update_quorum` and the simplified
// fallback `synthesize_democracy_update` both expect at update time:
//
//   c = Poseidon(Poseidon(Poseidon(member_root, 0), salt),
//                occupancy_commitment_initial)
//
// This closes the lineage gap that left freshly-created democracy
// groups bricked from update_commitment under the migration's
// "simplified initial port" wiring (where create reused anarchy's
// 2-level membership VK and produced a c shape `update_commitment`
// could not satisfy without a Poseidon collision).
//
// **Public inputs (3, fixed order)** — distinct from membership's 2-PI
// shape because `occupancy_commitment_initial` MUST be bound by the
// proof to close the second gap from the contract's "Status —
// simplified initial port" docstring (occupancy was previously
// caller-asserted but unbound):
//
//   1. `commitment`
//   2. `epoch`  (always 0 at create — the contract validates this
//                  matches `BE(0)` so passing anything else fails the
//                  PI gate before the verifier ever runs)
//   3. `occupancy_commitment_initial`
//
// **Private witnesses**: `secret_key, member_root, salt, merkle_path,
// leaf_index, depth`. Plus `member_root` MUST contain the creator's
// `Poseidon(secret_key)` leaf — same key-ownership demonstration as
// the standard membership circuit.

/// Witness for the democracy create circuit.
pub struct DemocracyCreateWitness {
    pub commitment: Fr,
    pub occupancy_commitment_initial: Fr,
    pub secret_key: Fr,
    pub member_root: Fr,
    pub salt: [u8; 32],
    pub merkle_path: Vec<Fr>,
    pub leaf_index: usize,
    pub depth: usize,
}

/// Allocate the democracy create circuit. Returns the public-input
/// `Variable` for `commitment`. Public-input ordering is fixed:
/// `(commitment, epoch=0, occupancy_commitment_initial)`.
pub fn synthesize_democracy_create(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &DemocracyCreateWitness,
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

    // ---- Public inputs (fixed order) ----
    let commitment_var = circuit.create_public_variable(witness.commitment)?;
    let epoch_var = circuit.create_public_variable(Fr::from(0u64))?;
    let occ_var =
        circuit.create_public_variable(witness.occupancy_commitment_initial)?;

    // ---- Private witnesses ----
    let secret_key_var = circuit.create_variable(witness.secret_key)?;
    let member_root_var = circuit.create_variable(witness.member_root)?;
    let salt_fr = Fr::from_le_bytes_mod_order(&witness.salt);
    let salt_var = circuit.create_variable(salt_fr)?;
    let path_vars: Vec<Variable> = witness
        .merkle_path
        .iter()
        .map(|s| circuit.create_variable(*s))
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

    // ---- 3. 3-level commitment chain (matches update_commitment) ----
    let inner = poseidon_hash_two_gadget(circuit, member_root_var, epoch_var)?;
    let mid = poseidon_hash_two_gadget(circuit, inner, salt_var)?;
    let computed_c = poseidon_hash_two_gadget(circuit, mid, occ_var)?;
    circuit.enforce_equal(computed_c, commitment_var)?;

    Ok(commitment_var)
}

// ================================================================
// Membership circuit (issue #5 / Phase C.4.c)
// ================================================================
//
// `synthesize_democracy_membership` mirrors `synthesize_oligarchy_membership`
// — proves "this caller knows a secret_key whose Poseidon-leaf opens
// to `member_root` at some leaf_index, and the resulting commitment
// matches what's stored on-chain" — but binds the commitment under
// democracy's **3-level chain** (the same one
// `synthesize_democracy_create` and `synthesize_democracy_update_quorum`
// both use):
//
//   c = Poseidon(Poseidon(Poseidon(member_root, epoch), salt),
//                occupancy_commitment)
//
// The standard membership circuit's 2-level chain doesn't match the
// stored c after `create_group` (post-issue-#5 fix), so a member
// proving against the wrong chain shape produces a c that the
// contract's `public_inputs[0] == state.commitment` PI gate rejects.
// This circuit closes that gap.
//
// **Public inputs (2, fixed order — matches standard membership for
// wire-shape compatibility):**
//   1. `commitment`
//   2. `epoch`
//
// **Private witnesses:** `secret_key, member_root, salt, merkle_path,
// leaf_index, occupancy_commitment`. The last is a per-state quantity
// the prover gets via off-chain group coordination — same value the
// contract stores in `CommitmentEntry.occupancy_commitment`.

/// Witness inputs for the democracy-specific membership circuit.
pub struct DemocracyMembershipWitness {
    pub commitment: Fr,
    pub epoch: u64,
    pub secret_key: Fr,
    pub member_root: Fr,
    pub salt: [u8; 32],
    pub merkle_path: Vec<Fr>,
    pub leaf_index: usize,
    pub depth: usize,
    /// Per-state occupancy commitment — private. (Same value the
    /// contract stores in `CommitmentEntry.occupancy_commitment`; not
    /// promoted to a public input so wire-PI shape stays compatible
    /// with the standard membership circuit.)
    pub occupancy_commitment: Fr,
}

/// Allocate the democracy-specific membership circuit. Public-input
/// ordering is byte-identical to `synthesize_membership`'s
/// `(commitment, epoch)` so the contract surface (PI count + shape)
/// is unchanged when `verify_membership` swaps to this VK.
pub fn synthesize_democracy_membership(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &DemocracyMembershipWitness,
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

    // ---- Public inputs ----
    let commitment_var = circuit.create_public_variable(witness.commitment)?;
    let epoch_var = circuit.create_public_variable(Fr::from(witness.epoch))?;

    // ---- Private witnesses ----
    let secret_key_var = circuit.create_variable(witness.secret_key)?;
    let member_root_var = circuit.create_variable(witness.member_root)?;
    let salt_fr = Fr::from_le_bytes_mod_order(&witness.salt);
    let salt_var = circuit.create_variable(salt_fr)?;
    let occ_var = circuit.create_variable(witness.occupancy_commitment)?;
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
    let computed_c = poseidon_hash_two_gadget(circuit, mid, occ_var)?;
    circuit.enforce_equal(computed_c, commitment_var)?;

    Ok(commitment_var)
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

    /// Member-removal path: `count_new = count_old - 1`. The
    /// `(diff)(diff-1)(diff+1)` product is symmetric in sign, but
    /// catches witness-generation regressions on the negative branch.
    #[test]
    fn satisfies_count_delta_minus_one() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_witness(&sks, 3, 42, K_MAX as u64, K_MAX, 5, 4);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        c.check_circuit_satisfiability(&pi(&w)).unwrap();
    }

    /// Positive baseline for `rejects_out_of_range_threshold_pi`:
    /// `K=0`, `threshold=0` is a satisfiable configuration. Without
    /// this companion, the negative test could pass for the wrong
    /// reason (e.g. if the K=0 path were silently broken) — pinning
    /// the baseline guarantees the negative-test failure isolates to
    /// the threshold range gate.
    #[test]
    fn satisfies_zero_quorum_zero_threshold() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_witness(&sks, 3, 42, 0, 0, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        c.check_circuit_satisfiability(&pi(&w)).unwrap();
    }

    /// PI-supplied threshold outside `[0, K_MAX]` must reject. Without
    /// the in-circuit threshold range gate, `threshold ≡ -slack (mod
    /// p)` would let an attacker pass `K = threshold + slack` with K=0
    /// active signers. Exercises the gate against a crafted PI vector
    /// that the witness-generation path can't naturally produce.
    /// Companion: `satisfies_zero_quorum_zero_threshold` confirms the
    /// unpatched PI baseline is satisfiable.
    #[test]
    fn rejects_out_of_range_threshold_pi() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_witness(&sks, 3, 42, 0, 0, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        let mut bad_pi = pi(&w);
        // -3 mod p — a "large/negative" Fr that breaks `K = threshold
        // + slack` if the threshold range gate is missing.
        bad_pi[5] = -Fr::from(3u64);
        assert!(c.check_circuit_satisfiability(&bad_pi).is_err());
    }

    /// `threshold = 3` is in-range for the 2-bit threshold gate
    /// (`3 = 0b11`) but unsatisfiable with `K_MAX = 2`: the slack
    /// `K - threshold = 2 - 3 = -1` can't fit a 2-bit unsigned
    /// decomposition. Pins the slack range check independently of the
    /// threshold range check.
    #[test]
    fn rejects_threshold_above_k_max() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_witness(&sks, 3, 42, 3, K_MAX, 5, 5);
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
    fn rejects_tampered_occ_old_pi() {
        // Tampering `occupancy_commitment_old` in the public-input
        // vector must not satisfy the circuit. Guards against PI
        // re-use across different occupancy snapshots.
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_witness(&sks, 3, 42, K_MAX as u64, K_MAX, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        let mut bad_pi = pi(&w);
        bad_pi[3] += Fr::from(1u64); // index 3 = occupancy_commitment_old
        assert!(c.check_circuit_satisfiability(&bad_pi).is_err());
    }

    #[test]
    fn rejects_active_prefix_violation() {
        // active = [false, true] violates the strict-prefix
        // requirement on the active boolean vector.
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let mut w = build_witness(&sks, 3, 42, 1, K_MAX, 5, 5);
        // Both slots are active here; flip slot 0 inactive so the
        // pattern is [false, true]. Threshold=1 still matches K=1
        // arithmetically — the failure must come from the prefix gate.
        w.signers[0].active = false;
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        assert!(c.check_circuit_satisfiability(&pi(&w)).is_err());
    }

    #[test]
    fn inactive_slot_with_valid_different_root_passes() {
        // An inactive slot may carry a perfectly valid Merkle opening
        // to some *other* root — the active-conditional equality gate
        // must mask it. Verifies the gating, not just dummy zeros.
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let mut w = build_witness(&sks, 3, 42, 1, 1, 5, 5);
        // Build a disjoint tree; install a real opening from it in
        // the inactive slot with a distinct leaf_idx.
        let other_sks: Vec<Fr> = (100u64..104).map(Fr::from).collect();
        let (_other_root, other_paths) = build_tree(&other_sks, 3);
        w.signers[1] = DemocracySigner {
            secret_key: other_sks[0],
            merkle_path: other_paths[0].clone(),
            leaf_index: 7,
            active: false,
        };
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        c.check_circuit_satisfiability(&pi(&w)).unwrap();
    }

    #[test]
    fn rejects_duplicate_leaf_double_count() {
        // One signer tries to fill both quorum slots: same sk, same
        // path, same leaf_index, both active. Without the
        // anti-double-count constraint the threshold check would pass
        // (K=2 ≥ 2) collapsing the quorum to 1-of-N.
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let mut w = build_witness(&sks, 3, 42, K_MAX as u64, K_MAX, 5, 5);
        w.signers[1] = w.signers[0].clone();
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        assert!(c.check_circuit_satisfiability(&pi(&w)).is_err());
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

    #[test]
    fn round_trip_d8() {
        use rand_chacha::rand_core::SeedableRng;
        let sks: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let w = build_witness(&sks, 8, 1234, K_MAX as u64, K_MAX, 8, 8);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        c.finalize_for_arithmetization().unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&c).unwrap();
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &c).unwrap();
        crate::prover::plonk::verify(&keys.vk, &pi(&w), &proof).unwrap();
    }

    #[test]
    fn rejects_tampered_active_merkle_path() {
        // Flipping any node of an active signer's Merkle path must
        // break the active-conditional `computed_root == root_old`
        // gate (`democracy.rs` constraint 1, line ~189). Guards
        // against regressions that turn the gate into a no-op.
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let mut w = build_witness(&sks, 3, 42, K_MAX as u64, K_MAX, 5, 5);
        w.signers[0].merkle_path[1] += Fr::from(1u64);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        assert!(c.check_circuit_satisfiability(&pi(&w)).is_err());
    }

    #[test]
    fn rejects_tampered_c_old_pi() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_witness(&sks, 3, 42, K_MAX as u64, K_MAX, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        let mut bad_pi = pi(&w);
        bad_pi[0] += Fr::from(1u64);
        assert!(c.check_circuit_satisfiability(&bad_pi).is_err());
    }

    #[test]
    fn rejects_tampered_epoch_old_pi() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_witness(&sks, 3, 42, K_MAX as u64, K_MAX, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        let mut bad_pi = pi(&w);
        bad_pi[1] += Fr::from(1u64);
        assert!(c.check_circuit_satisfiability(&bad_pi).is_err());
    }

    #[test]
    fn rejects_tampered_c_new_pi() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_witness(&sks, 3, 42, K_MAX as u64, K_MAX, 5, 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update_quorum(&mut c, &w).unwrap();
        let mut bad_pi = pi(&w);
        bad_pi[2] += Fr::from(1u64);
        assert!(c.check_circuit_satisfiability(&bad_pi).is_err());
    }

    // ============================================================
    // Simplified single-signer fallback (synthesize_democracy_update)
    // covers tier 2 (d=11) until the quorum circuit fits the SRS.
    // ============================================================

    fn build_simplified_witness(
        secret_keys: &[Fr],
        depth: usize,
        epoch_old: u64,
        threshold: u64,
    ) -> DemocracyUpdateWitness {
        let (root, paths) = build_tree(secret_keys, depth);
        let salt_old = [0xCCu8; 32];
        let salt_new = [0xDDu8; 32];
        let salt_oc_old = Fr::from(0x77u64);
        let salt_oc_new = Fr::from(0x88u64);
        let count = 5u64;
        let occ_old = poseidon_hash_two_v05(&Fr::from(count), &salt_oc_old);
        let occ_new = poseidon_hash_two_v05(&Fr::from(count), &salt_oc_new);
        let salt_old_fr = Fr::from_le_bytes_mod_order(&salt_old);
        let salt_new_fr = Fr::from_le_bytes_mod_order(&salt_new);
        let inner_old = poseidon_hash_two_v05(&root, &Fr::from(epoch_old));
        let mid_old = poseidon_hash_two_v05(&inner_old, &salt_old_fr);
        let c_old = poseidon_hash_two_v05(&mid_old, &occ_old);
        let inner_new = poseidon_hash_two_v05(&root, &Fr::from(epoch_old + 1));
        let mid_new = poseidon_hash_two_v05(&inner_new, &salt_new_fr);
        let c_new = poseidon_hash_two_v05(&mid_new, &occ_new);

        DemocracyUpdateWitness {
            c_old,
            epoch_old,
            c_new,
            occupancy_commitment_old: occ_old,
            occupancy_commitment_new: occ_new,
            threshold_numerator: threshold,
            secret_key: secret_keys[0],
            member_root_old: root,
            member_root_new: root,
            salt_old,
            salt_new,
            merkle_path_old: paths[0].clone(),
            leaf_index_old: 0,
            depth,
        }
    }

    fn pi_simplified(w: &DemocracyUpdateWitness) -> Vec<Fr> {
        vec![
            w.c_old,
            Fr::from(w.epoch_old),
            w.c_new,
            w.occupancy_commitment_old,
            w.occupancy_commitment_new,
            Fr::from(w.threshold_numerator),
        ]
    }

    /// Satisfiability + gate-count guard for the simplified circuit at
    /// production depth (d=11). The quorum circuit doesn't fit at
    /// d=11; this is the path tier 2 actually verifies against. The
    /// gate-count assertion guards against an accidental change
    /// pushing the simplified circuit past the n=32768 SRS ceiling
    /// (the quorum circuit's `gate_count_per_supported_tier` only
    /// covers d∈{5, 8}).
    #[test]
    fn simplified_satisfies_d11() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_simplified_witness(&sks, 11, 99, 1);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update(&mut c, &w).unwrap();
        c.check_circuit_satisfiability(&pi_simplified(&w)).unwrap();
        c.finalize_for_arithmetization().unwrap();
        eprintln!(
            "[gate-count] simplified democracy_update depth=11: {} gates",
            c.num_gates()
        );
        assert!(
            c.num_gates() < 32768,
            "simplified democracy_update at d=11 ({} gates) over n=32768 SRS ceiling",
            c.num_gates(),
        );
    }

    /// Round-trip the simplified circuit through prove+verify. Run at
    /// d=8 to keep the test snappy; d=11 round-trip is exercised by
    /// the baker fixture regen path (`bake_democracy_update_vk_*`).
    #[test]
    fn simplified_round_trip_d8() {
        use rand_chacha::rand_core::SeedableRng;
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_simplified_witness(&sks, 8, 1234, 1);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update(&mut c, &w).unwrap();
        c.finalize_for_arithmetization().unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&c).unwrap();
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &c).unwrap();
        crate::prover::plonk::verify(&keys.vk, &pi_simplified(&w), &proof)
            .unwrap();
    }
}
