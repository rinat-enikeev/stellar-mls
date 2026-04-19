//! DemocracyUpdateCircuit — Groth16 circuit that binds a Democracy-group Commit
//! to a quorum of co-openings from at least half of the current members.
//!
//! See `docs/group-governance-types-design.md §6.4.2` for the formal relation
//! and `docs/democracy-circuit-ceremony.md` for the rollout plan.
//!
//! ## Public inputs (fixed allocation order)
//! 1. `c_old`
//! 2. `epoch_old`
//! 3. `c_new`
//! 4. `member_count_old`
//! 5. `member_count_new`
//!
//! ## Witnesses
//! - `signers`: zero-padded to `k_max` slots. Each slot i carries `(sk_i,
//!   merkle_path_i, leaf_idx_i, active_i)`. Active slots MUST be a prefix
//!   (active_0 ≥ active_1 ≥ … ≥ active_{k_max-1} as booleans).
//! - `root_old`, `root_new`, `salt_old`, `salt_new`.
//! - `delta_position`, `delta_path`, `old_leaf_at_delta`, `new_leaf_at_delta`,
//!   `is_insert`, `is_remove` — the single-leaf change witness.
//!
//! ## Constraints (§6.4.2)
//! 0. `K ≥ 1` — enforced as `active_bits[0] = 1`. Combined with the prefix
//!    constraint (#2), this is equivalent to `K = Σ active_i ≥ 1`. Closes
//!    the zero-opening-Commit attack surface independent of any
//!    contract-side `member_count_old` check.
//! 1. Each active signer slot: `Poseidon(sk_i)` opens to `root_old` at
//!    `leaf_idx_i`. Inactive slots are vacuous.
//! 2. Active slots are a strict prefix: `active_i ⇒ active_{i-1}`.
//! 3. Strict ascending indices across active slots:
//!    `leaf_idx_i > leaf_idx_{i-1}` when both `i` and `i-1` are active.
//! 4. `2·K ≥ member_count_old`, where `K = Σ active_i`.
//! 5. `member_count_old`, `member_count_new` each fit in `depth` bits.
//! 6. `|member_count_new − member_count_old| ≤ 1`.
//! 7. Single-leaf delta: both `old_leaf_at_delta` and `new_leaf_at_delta` fold
//!    along `delta_path` at `delta_position` to `root_old` and `root_new`
//!    respectively.
//! 8. Delta kind ↔ count change:
//!    - `is_insert=1` ⇒ `old_leaf_at_delta=0` and `member_count_new =
//!      member_count_old + 1`.
//!    - `is_remove=1` ⇒ `new_leaf_at_delta=0` and `member_count_new =
//!      member_count_old − 1`.
//!    - else (replace) ⇒ `member_count_new = member_count_old`.
//!    - `is_insert · is_remove = 0` (mutually exclusive).
//! 9. `c_old = Poseidon(Poseidon(root_old, epoch_old), salt_old)`.
//! 10. `c_new = Poseidon(Poseidon(root_new, epoch_old + 1), salt_new)`.
//!
//! ## Ceremony status
//!
//! The constraint body below is a first-draft implementation intended for
//! local testing and property verification. It has NOT been reviewed for
//! R1CS-level soundness by a second set of ZK-literate eyes, and the Phase 2
//! trusted-setup ceremony MUST NOT be run against it until that review
//! concludes. `generate_keyset` paths for this circuit should remain behind a
//! dev-only flag (see `docs/democracy-circuit-ceremony.md §3`).

use ark_bls12_381::Fr;
use ark_crypto_primitives::sponge::poseidon::PoseidonConfig;
use ark_crypto_primitives::sponge::Absorb;
use ark_ff::{BigInteger, PrimeField};
use ark_r1cs_std::fields::fp::FpVar;
use ark_r1cs_std::prelude::*;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::circuit::{poseidon_hash_one_gadget, poseidon_hash_two_gadget};
use crate::poseidon::poseidon_config;

/// One signer's witness bundle: secret key + Merkle opening into the current
/// member tree.
#[derive(Clone)]
pub struct DemocracySigner<F: PrimeField + Absorb> {
    pub secret_key: F,
    pub merkle_path: Vec<F>,
    pub leaf_index: usize,
}

/// Describes how a single leaf position changes between `root_old` and
/// `root_new`. Populated by the prover and constrained in §6.4.2 #6.
#[derive(Clone)]
pub enum DemocracyDelta<F: PrimeField + Absorb> {
    /// A fresh leaf was inserted at `position`. `new_leaf` is its Poseidon hash.
    Insert {
        position: usize,
        path: Vec<F>,
        new_leaf: F,
    },
    /// The leaf at `position` was cleared (member removal). `old_leaf` is its
    /// previous Poseidon hash.
    Remove {
        position: usize,
        path: Vec<F>,
        old_leaf: F,
    },
    /// The leaf at `position` was replaced in place (key rotation). Both the
    /// old and new leaves are captured so constraint #7 can assert the
    /// per-position transition.
    Replace {
        position: usize,
        path: Vec<F>,
        old_leaf: F,
        new_leaf: F,
    },
}

impl<F: PrimeField + Absorb> DemocracyDelta<F> {
    fn position(&self) -> usize {
        match self {
            Self::Insert { position, .. }
            | Self::Remove { position, .. }
            | Self::Replace { position, .. } => *position,
        }
    }

    fn path(&self) -> &[F] {
        match self {
            Self::Insert { path, .. } | Self::Remove { path, .. } | Self::Replace { path, .. } => {
                path
            }
        }
    }

    fn old_leaf(&self) -> F {
        match self {
            Self::Insert { .. } => F::zero(),
            Self::Remove { old_leaf, .. } => *old_leaf,
            Self::Replace { old_leaf, .. } => *old_leaf,
        }
    }

    fn new_leaf(&self) -> F {
        match self {
            Self::Insert { new_leaf, .. } => *new_leaf,
            Self::Remove { .. } => F::zero(),
            Self::Replace { new_leaf, .. } => *new_leaf,
        }
    }

    fn is_insert(&self) -> bool {
        matches!(self, Self::Insert { .. })
    }

    fn is_remove(&self) -> bool {
        matches!(self, Self::Remove { .. })
    }
}

/// `DemocracyUpdateCircuit` — see module-level docs.
#[derive(Clone)]
pub struct DemocracyUpdateCircuit<F: PrimeField + Absorb> {
    // === Public inputs (allocated in fixed order) ===
    pub c_old: Option<F>,
    pub epoch_old: Option<u64>,
    pub c_new: Option<F>,
    pub member_count_old: Option<u32>,
    pub member_count_new: Option<u32>,

    // === Witnesses ===
    pub signers: Option<Vec<DemocracySigner<F>>>,
    pub root_old: Option<F>,
    pub root_new: Option<F>,
    pub salt_old: Option<[u8; 32]>,
    pub salt_new: Option<[u8; 32]>,
    pub delta_witness: Option<DemocracyDelta<F>>,

    // === Circuit tier parameters ===
    /// Tree depth (determines tier / max member count).
    pub depth: usize,
    /// Maximum signer count the circuit is compiled for. Tier × K_max is fixed
    /// at key-generation time; v1 ships `(tier=0, K_max=32)` and
    /// `(tier=1, K_max=256)` per §6.4.2.
    pub k_max: usize,

    /// Poseidon config shared across all membership circuits.
    pub poseidon_config: PoseidonConfig<F>,
}

impl<F: PrimeField + Absorb> DemocracyUpdateCircuit<F> {
    /// Create an empty circuit (for setup/keygen — no witness values).
    pub fn empty(depth: usize, k_max: usize) -> Self {
        Self {
            c_old: None,
            epoch_old: None,
            c_new: None,
            member_count_old: None,
            member_count_new: None,
            signers: None,
            root_old: None,
            root_new: None,
            salt_old: None,
            salt_new: None,
            delta_witness: None,
            depth,
            k_max,
            poseidon_config: poseidon_config::<F>(),
        }
    }
}

impl DemocracyUpdateCircuit<Fr> {
    /// Create a populated circuit for proving.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        c_old: Fr,
        epoch_old: u64,
        c_new: Fr,
        member_count_old: u32,
        member_count_new: u32,
        signers: Vec<DemocracySigner<Fr>>,
        root_old: Fr,
        root_new: Fr,
        salt_old: [u8; 32],
        salt_new: [u8; 32],
        delta_witness: DemocracyDelta<Fr>,
        depth: usize,
        k_max: usize,
    ) -> Self {
        Self {
            c_old: Some(c_old),
            epoch_old: Some(epoch_old),
            c_new: Some(c_new),
            member_count_old: Some(member_count_old),
            member_count_new: Some(member_count_new),
            signers: Some(signers),
            root_old: Some(root_old),
            root_new: Some(root_new),
            salt_old: Some(salt_old),
            salt_new: Some(salt_new),
            delta_witness: Some(delta_witness),
            depth,
            k_max,
            poseidon_config: poseidon_config::<Fr>(),
        }
    }
}

impl ConstraintSynthesizer<Fr> for DemocracyUpdateCircuit<Fr> {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // ============================================================
        // (A) Public inputs — pinned allocation order.
        //
        // Order IS the serialisation order the contract's Groth16 IC vector
        // indexes by. Changing this without updating every prover, verifier,
        // and wire-format call site will break verification silently.
        // ============================================================
        let c_old_var = FpVar::new_input(cs.clone(), || {
            self.c_old.ok_or(SynthesisError::AssignmentMissing)
        })?;
        let epoch_old_var = FpVar::new_input(cs.clone(), || {
            self.epoch_old
                .map(Fr::from)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let c_new_var = FpVar::new_input(cs.clone(), || {
            self.c_new.ok_or(SynthesisError::AssignmentMissing)
        })?;
        let member_count_old_var = FpVar::new_input(cs.clone(), || {
            self.member_count_old
                .map(|n| Fr::from(n as u64))
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let member_count_new_var = FpVar::new_input(cs.clone(), || {
            self.member_count_new
                .map(|n| Fr::from(n as u64))
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        // ============================================================
        // (B) Root / salt witnesses
        // ============================================================
        let root_old_var = FpVar::new_witness(cs.clone(), || {
            self.root_old.ok_or(SynthesisError::AssignmentMissing)
        })?;
        let root_new_var = FpVar::new_witness(cs.clone(), || {
            self.root_new.ok_or(SynthesisError::AssignmentMissing)
        })?;
        let salt_old_var = FpVar::new_witness(cs.clone(), || {
            self.salt_old
                .map(|s| Fr::from_le_bytes_mod_order(&s))
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let salt_new_var = FpVar::new_witness(cs.clone(), || {
            self.salt_new
                .map(|s| Fr::from_le_bytes_mod_order(&s))
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        // ============================================================
        // (C) Range checks — member counts fit in `depth` bits.
        //     Constraint #5. Also binds the "≤ 2^depth" invariant.
        // ============================================================
        enforce_fits_in_bits(&member_count_old_var, self.depth)?;
        enforce_fits_in_bits(&member_count_new_var, self.depth)?;

        // ============================================================
        // (D) Signer slots — zero-padded to k_max.
        //
        // Each slot has:
        //  - active_i: Boolean
        //  - sk_i: FpVar
        //  - path_i: [FpVar; depth]
        //  - idx_bits_i: [Boolean; depth]  (leaf index as LE bits)
        // ============================================================
        let mut active_bits: Vec<Boolean<Fr>> = Vec::with_capacity(self.k_max);
        let mut leaf_index_vars: Vec<FpVar<Fr>> = Vec::with_capacity(self.k_max);

        for i in 0..self.k_max {
            let signer_i = self.signers.as_ref().and_then(|s| s.get(i));

            let active = Boolean::new_witness(cs.clone(), || {
                Ok(signer_i.is_some())
            })?;

            let sk_var = FpVar::new_witness(cs.clone(), || {
                Ok(signer_i.map(|s| s.secret_key).unwrap_or(Fr::from(0u64)))
            })?;

            let path_i_vars: Vec<FpVar<Fr>> = (0..self.depth)
                .map(|level| {
                    FpVar::new_witness(cs.clone(), || {
                        Ok(signer_i
                            .and_then(|s| s.merkle_path.get(level).copied())
                            .unwrap_or(Fr::from(0u64)))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;

            let idx_i_bits: Vec<Boolean<Fr>> = (0..self.depth)
                .map(|level| {
                    Boolean::new_witness(cs.clone(), || {
                        Ok(signer_i
                            .map(|s| (s.leaf_index >> level) & 1 == 1)
                            .unwrap_or(false))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;

            // ============================================================
            // (E) Active-slot Merkle opening to root_old — constraint #1.
            //
            // When active_i = 1:  Poseidon(sk_i) folded along path_i at
            // idx_bits_i must equal root_old.
            // When active_i = 0:  vacuous — no constraint on (sk, path, idx).
            // ============================================================
            let leaf_var = poseidon_hash_one_gadget(cs.clone(), &self.poseidon_config, &sk_var)?;
            let mut current = leaf_var;
            for level in 0..self.depth {
                let sibling = &path_i_vars[level];
                let is_right = &idx_i_bits[level];
                let left = FpVar::conditionally_select(is_right, sibling, &current)?;
                let right = FpVar::conditionally_select(is_right, &current, sibling)?;
                current = poseidon_hash_two_gadget(cs.clone(), &self.poseidon_config, &left, &right)?;
            }
            // (current - root_old) * active_i == 0
            let root_diff = &current - &root_old_var;
            let active_fp = FpVar::from(active.clone());
            (&root_diff * &active_fp).enforce_equal(&FpVar::zero())?;

            // Pack leaf_idx as a field element for ordering comparisons.
            let idx_fp = bits_to_fp(&idx_i_bits);
            leaf_index_vars.push(idx_fp);

            active_bits.push(active);
        }

        // ============================================================
        // (F) Active slots are a strict prefix — constraint #2.
        //     active_i ⇒ active_{i-1}    ⇔    active_i * (1 - active_{i-1}) = 0
        // ============================================================
        for i in 1..self.k_max {
            let cur = FpVar::from(active_bits[i].clone());
            let prev = FpVar::from(active_bits[i - 1].clone());
            let one_minus_prev = FpVar::one() - &prev;
            (&cur * &one_minus_prev).enforce_equal(&FpVar::zero())?;
        }

        // ============================================================
        // (G) Strict ascending indices across active slots — constraint #3.
        //     Enforced gated by active_i:  if active_i = 1 then
        //     leaf_idx_i > leaf_idx_{i-1}. Encoded as:
        //         diff_i = leaf_idx_i - leaf_idx_{i-1} - 1
        //         witness diff_bits_i: depth-bit LE decomposition
        //         (Σ diff_bits_i * 2^k − diff_i) · active_i = 0
        //     When active_i = 0 the binding is vacuous, so a malicious prover
        //     can cheat there — but an inactive slot doesn't contribute to K,
        //     so it can't inflate the quorum count anyway.
        //     When active_i = 1 the bit sum pins diff_i ∈ [0, 2^depth), which
        //     on the count ≤ 2^depth roster implies leaf_idx_i > leaf_idx_{i-1}.
        //
        //     ── Why `depth` bits suffices for a field-element difference ──
        //     `diff` is computed in the full scalar field `Fr` of BLS12-381,
        //     where `r ≈ 2^254.86` (prime order). A prover trying to cheat
        //     with a descending pair `leaf_idx_i < leaf_idx_{i-1}` produces
        //         diff ≡ (small positive value) mod r
        //     wrapped around: concretely, diff ≡ r − ε for some small
        //     ε ≥ 1. For `diff_from_bits ∈ [0, 2^depth)` to equal that
        //     value we'd need `r − ε < 2^depth`, i.e. `r < 2^depth + ε`.
        //     With `depth ≤ 11` (tier 2 ceiling) and `r ≈ 2^254.86`, the
        //     inequality is violated by more than 240 orders of magnitude —
        //     there is no field-level collision path. The range check
        //     therefore soundly rejects every descending ordering whenever
        //     `active_i = 1`. A larger `depth` (e.g. `depth = 240`) would
        //     narrow this margin and require a stronger argument, but for
        //     any tier we will ever ship on BLS12-381 it is trivially safe.
        // ============================================================
        for i in 1..self.k_max {
            let diff = &leaf_index_vars[i] - &leaf_index_vars[i - 1] - FpVar::one();

            let signers = self.signers.as_ref();
            let diff_value: Option<Fr> = match signers {
                Some(s) if i < s.len() => {
                    let a = s[i - 1].leaf_index as u64;
                    let b = s[i].leaf_index as u64;
                    // Allowed to underflow when inactive — value unused.
                    Some(Fr::from(b.wrapping_sub(a).wrapping_sub(1)))
                }
                _ => Some(Fr::from(0u64)),
            };

            let diff_bits: Vec<Boolean<Fr>> = (0..self.depth)
                .map(|k| {
                    Boolean::new_witness(cs.clone(), || {
                        Ok(diff_value
                            .map(|v| {
                                let repr = v.into_bigint();
                                repr.get_bit(k)
                            })
                            .unwrap_or(false))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let diff_from_bits = bits_to_fp(&diff_bits);

            let active_fp = FpVar::from(active_bits[i].clone());
            let discrepancy = &diff_from_bits - &diff;
            (&discrepancy * &active_fp).enforce_equal(&FpVar::zero())?;
        }

        // ============================================================
        // (H) Quorum — constraints #0 and #4.
        //     #0 K ≥ 1: no zero-opening Commit is ever accepted.
        //     #4 2·K ≥ member_count_old: the ≥50% threshold.
        //     K = Σ active_i.
        //
        //     K ≥ 1 is enforced as `active_bits[0] = 1`. The prefix
        //     structure from (F) makes this equivalent to K ≥ 1 — if
        //     slot 0 is inactive, (F) forces every later slot inactive
        //     too, so K collapses to 0.
        //
        //     Design doc §6.4.2 lists K ≥ 1 as the first Democracy
        //     constraint; it was originally left implicit (the contract
        //     pre-check `member_count ≥ 2` combined with `2·K ≥ n`
        //     happens to block K = 0 in the current code path). Pinning
        //     it in-circuit is defence-in-depth against any future
        //     contract-side regression that could surface
        //     `member_count_old = 0` (e.g. a lazy-migration bug): with
        //     K ≥ 1 enforced here, a zero-opening Commit cannot be
        //     accepted even if the caller and contract agree on a
        //     bogus `member_count_old`.
        //
        //     The 2·K ≥ member_count_old bound is enforced via a
        //     (depth + 1)-bit non-negative bit decomposition of
        //     (2·K − member_count_old).
        // ============================================================
        active_bits[0].enforce_equal(&Boolean::TRUE)?;

        let mut k_sum = FpVar::zero();
        for bit in &active_bits {
            k_sum += FpVar::from(bit.clone());
        }
        let two_k = &k_sum + &k_sum;
        let quorum_diff = &two_k - &member_count_old_var;
        enforce_fits_in_bits(&quorum_diff, self.depth + 1)?;

        // ============================================================
        // (I) Count delta ≤ 1 — constraint #6.
        //     Let d = member_count_new − member_count_old.
        //     Enforce d ∈ {−1, 0, +1} via  d · (d − 1) · (d + 1) = 0.
        // ============================================================
        let d = &member_count_new_var - &member_count_old_var;
        let d_plus_1 = &d + FpVar::one();
        let d_minus_1 = &d - FpVar::one();
        let d_times_d_minus_1 = &d * &d_minus_1;
        (&d_times_d_minus_1 * &d_plus_1).enforce_equal(&FpVar::zero())?;

        // ============================================================
        // (J) Single-leaf delta — constraint #7.
        //     Both old_leaf_at_delta and new_leaf_at_delta fold along the
        //     same delta_path at delta_position to root_old and root_new
        //     respectively.
        // ============================================================
        let delta_pos_bits: Vec<Boolean<Fr>> = (0..self.depth)
            .map(|level| {
                Boolean::new_witness(cs.clone(), || {
                    Ok(self
                        .delta_witness
                        .as_ref()
                        .map(|d| (d.position() >> level) & 1 == 1)
                        .unwrap_or(false))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let delta_path_vars: Vec<FpVar<Fr>> = (0..self.depth)
            .map(|level| {
                FpVar::new_witness(cs.clone(), || {
                    Ok(self
                        .delta_witness
                        .as_ref()
                        .and_then(|d| d.path().get(level).copied())
                        .unwrap_or(Fr::from(0u64)))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let old_leaf_var = FpVar::new_witness(cs.clone(), || {
            Ok(self
                .delta_witness
                .as_ref()
                .map(|d| d.old_leaf())
                .unwrap_or(Fr::from(0u64)))
        })?;
        let new_leaf_var = FpVar::new_witness(cs.clone(), || {
            Ok(self
                .delta_witness
                .as_ref()
                .map(|d| d.new_leaf())
                .unwrap_or(Fr::from(0u64)))
        })?;

        let computed_old_root =
            fold_merkle_path(cs.clone(), &self.poseidon_config, &old_leaf_var, &delta_path_vars, &delta_pos_bits)?;
        let computed_new_root =
            fold_merkle_path(cs.clone(), &self.poseidon_config, &new_leaf_var, &delta_path_vars, &delta_pos_bits)?;
        computed_old_root.enforce_equal(&root_old_var)?;
        computed_new_root.enforce_equal(&root_new_var)?;

        // ============================================================
        // (K) Delta kind ↔ count change — constraint #8.
        //
        //  is_insert * (1 - is_insert) = 0   (boolean; handled by allocation)
        //  is_remove * (1 - is_remove) = 0   (boolean)
        //  is_insert * is_remove = 0          (mutually exclusive)
        //  is_insert * old_leaf_at_delta = 0            (insert: old side zero)
        //  is_remove * new_leaf_at_delta = 0            (remove: new side zero)
        //  is_insert * (member_count_new - member_count_old - 1) = 0
        //  is_remove * (member_count_new - member_count_old + 1) = 0
        //  (1 - is_insert - is_remove) * (member_count_new - member_count_old) = 0
        // ============================================================
        let is_insert = Boolean::new_witness(cs.clone(), || {
            Ok(self
                .delta_witness
                .as_ref()
                .map(|d| d.is_insert())
                .unwrap_or(false))
        })?;
        let is_remove = Boolean::new_witness(cs.clone(), || {
            Ok(self
                .delta_witness
                .as_ref()
                .map(|d| d.is_remove())
                .unwrap_or(false))
        })?;
        let is_insert_fp = FpVar::from(is_insert.clone());
        let is_remove_fp = FpVar::from(is_remove.clone());

        // Mutually exclusive.
        (&is_insert_fp * &is_remove_fp).enforce_equal(&FpVar::zero())?;

        // is_insert ⇒ old_leaf = 0
        (&is_insert_fp * &old_leaf_var).enforce_equal(&FpVar::zero())?;
        // is_remove ⇒ new_leaf = 0
        (&is_remove_fp * &new_leaf_var).enforce_equal(&FpVar::zero())?;

        // is_insert ⇒ member_count_new = member_count_old + 1
        let insert_count_check = &member_count_new_var - &member_count_old_var - FpVar::one();
        (&is_insert_fp * &insert_count_check).enforce_equal(&FpVar::zero())?;
        // is_remove ⇒ member_count_new = member_count_old - 1
        let remove_count_check = &member_count_new_var - &member_count_old_var + FpVar::one();
        (&is_remove_fp * &remove_count_check).enforce_equal(&FpVar::zero())?;

        // (1 - is_insert - is_remove) ⇒ member_count_new = member_count_old
        let replace_flag = FpVar::one() - &is_insert_fp - &is_remove_fp;
        let replace_count_check = &member_count_new_var - &member_count_old_var;
        (&replace_flag * &replace_count_check).enforce_equal(&FpVar::zero())?;

        // ============================================================
        // (L) Commitment binders — constraints #9 and #10.
        // ============================================================
        let root_epoch_old = poseidon_hash_two_gadget(
            cs.clone(),
            &self.poseidon_config,
            &root_old_var,
            &epoch_old_var,
        )?;
        let c_old_derived = poseidon_hash_two_gadget(
            cs.clone(),
            &self.poseidon_config,
            &root_epoch_old,
            &salt_old_var,
        )?;
        c_old_derived.enforce_equal(&c_old_var)?;

        let epoch_new_var = &epoch_old_var + FpVar::Constant(Fr::from(1u64));
        let root_epoch_new = poseidon_hash_two_gadget(
            cs.clone(),
            &self.poseidon_config,
            &root_new_var,
            &epoch_new_var,
        )?;
        let c_new_derived = poseidon_hash_two_gadget(
            cs,
            &self.poseidon_config,
            &root_epoch_new,
            &salt_new_var,
        )?;
        c_new_derived.enforce_equal(&c_new_var)?;

        Ok(())
    }
}

// ================================================================
// Local helpers
// ================================================================

/// Enforce that `value` fits in `n_bits` bits, i.e., that `value ∈ [0, 2^n_bits)`.
/// Uses the canonical bit decomposition and pins upper bits to zero.
fn enforce_fits_in_bits(value: &FpVar<Fr>, n_bits: usize) -> Result<(), SynthesisError> {
    let bits = value.to_bits_le()?;
    for bit in bits.iter().skip(n_bits) {
        bit.enforce_equal(&Boolean::constant(false))?;
    }
    Ok(())
}

/// Pack a LE bit vector into a field element.
fn bits_to_fp(bits: &[Boolean<Fr>]) -> FpVar<Fr> {
    let mut acc = FpVar::zero();
    let mut coeff = Fr::from(1u64);
    for bit in bits {
        acc += FpVar::from(bit.clone()) * FpVar::Constant(coeff);
        coeff += coeff;
    }
    acc
}

/// Fold `leaf` up a Merkle path of `depth` siblings, using `index_bits` to
/// pick left/right at each level. Returns the computed root.
fn fold_merkle_path(
    cs: ConstraintSystemRef<Fr>,
    config: &PoseidonConfig<Fr>,
    leaf: &FpVar<Fr>,
    path: &[FpVar<Fr>],
    index_bits: &[Boolean<Fr>],
) -> Result<FpVar<Fr>, SynthesisError> {
    assert_eq!(path.len(), index_bits.len());
    let mut current = leaf.clone();
    for (sibling, is_right) in path.iter().zip(index_bits.iter()) {
        let left = FpVar::conditionally_select(is_right, sibling, &current)?;
        let right = FpVar::conditionally_select(is_right, &current, sibling)?;
        current = poseidon_hash_two_gadget(cs.clone(), config, &left, &right)?;
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commitment::compute_poseidon_commitment;
    use crate::merkle::{CanonicalMember, PoseidonMerkleTree, COMPRESSED_G1_PUBLIC_KEY_LEN};
    use crate::poseidon::{poseidon_config, poseidon_hash_one};
    use ark_relations::r1cs::ConstraintSystem;

    /// Build `n` members at positions 0..n with synthetic sort bytes `i`-th
    /// byte = `i + 1`. We don't derive real BLS pubkeys here because the
    /// circuit only binds `Poseidon(sk)` to the leaf; sort bytes are an
    /// off-circuit canonicalization concern.
    fn synthetic_sort_bytes(i: usize) -> [u8; COMPRESSED_G1_PUBLIC_KEY_LEN] {
        let mut bytes = [0u8; COMPRESSED_G1_PUBLIC_KEY_LEN];
        bytes[0] = (i + 1) as u8;
        bytes
    }

    /// Build a valid Democracy Replace scenario:
    /// - Old tree holds `sks.len()` members at synthetic sorted positions
    ///   0..n.
    /// - New tree replaces the member at `replace_index`'s secret key with
    ///   `new_sk`, keeping its sort bytes constant — so the tree changes in
    ///   exactly one leaf (the single-leaf-delta invariant holds).
    /// - Signers are `sks[signer_indices]`.
    ///
    /// Replace (rather than Remove) is used because dense-packed
    /// `PoseidonMerkleTree::build` would shift indices on removal; a
    /// position-preserving layout is a §6.4.2-style decision tracked in
    /// `docs/democracy-circuit-ceremony.md`.
    fn build_replace_scenario(
        sks: &[Fr],
        signer_indices: &[usize],
        replace_index: usize,
        new_sk: Fr,
        depth: usize,
        k_max: usize,
    ) -> DemocracyUpdateCircuit<Fr> {
        let config = poseidon_config::<Fr>();

        let old_members: Vec<CanonicalMember<Fr>> = sks
            .iter()
            .enumerate()
            .map(|(i, sk)| CanonicalMember {
                public_key_bytes: synthetic_sort_bytes(i),
                leaf_hash: poseidon_hash_one(&config, sk),
            })
            .collect();
        let tree_old = PoseidonMerkleTree::build_from_members(&config, &old_members, depth).unwrap();
        let root_old = tree_old.root();

        let mut new_members = old_members.clone();
        new_members[replace_index] = CanonicalMember {
            public_key_bytes: synthetic_sort_bytes(replace_index),
            leaf_hash: poseidon_hash_one(&config, &new_sk),
        };
        let tree_new = PoseidonMerkleTree::build_from_members(&config, &new_members, depth).unwrap();
        let root_new = tree_new.root();

        let mut signers: Vec<DemocracySigner<Fr>> = signer_indices
            .iter()
            .map(|&i| {
                let proof = tree_old.prove(i);
                DemocracySigner {
                    secret_key: sks[i],
                    merkle_path: proof.path,
                    leaf_index: i,
                }
            })
            .collect();
        signers.sort_by_key(|s| s.leaf_index);

        let delta_path = tree_old.prove(replace_index).path;
        let delta = DemocracyDelta::Replace {
            position: replace_index,
            path: delta_path,
            old_leaf: poseidon_hash_one(&config, &sks[replace_index]),
            new_leaf: poseidon_hash_one(&config, &new_sk),
        };

        let epoch_old = 0u64;
        let salt_old = [0xAA; 32];
        let salt_new = [0xBB; 32];
        let c_old = compute_poseidon_commitment(&config, &root_old, epoch_old, &salt_old);
        let c_new = compute_poseidon_commitment(&config, &root_new, epoch_old + 1, &salt_new);

        let count = sks.len() as u32;
        DemocracyUpdateCircuit::new(
            c_old, epoch_old, c_new, count, count, signers, root_old, root_new, salt_old,
            salt_new, delta, depth, k_max,
        )
    }

    #[test]
    fn democracy_circuit_accepts_valid_replace() {
        // 3 old members, key rotation at position 2. Quorum requires ≥ 2
        // signers (2·2 ≥ 3).
        let sks = vec![Fr::from(10u64), Fr::from(20u64), Fr::from(30u64)];
        let circuit =
            build_replace_scenario(&sks, &[0, 1], 2, Fr::from(999u64), 5, 4);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        // 1 (constant) + 5 (public inputs) = 6 instance variables.
        assert_eq!(cs.num_instance_variables(), 6);
        assert!(
            cs.is_satisfied().unwrap(),
            "valid Democracy replace must satisfy"
        );
    }

    #[test]
    fn democracy_circuit_rejects_under_quorum() {
        // 3 old members, only 1 signer (2·1 < 3).
        let sks = vec![Fr::from(10u64), Fr::from(20u64), Fr::from(30u64)];
        let circuit = build_replace_scenario(&sks, &[0], 2, Fr::from(999u64), 5, 4);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(
            !cs.is_satisfied().unwrap(),
            "under-quorum proof must NOT satisfy"
        );
    }

    #[test]
    fn democracy_circuit_rejects_wrong_c_new() {
        let sks = vec![Fr::from(10u64), Fr::from(20u64), Fr::from(30u64)];
        let mut circuit =
            build_replace_scenario(&sks, &[0, 1], 2, Fr::from(999u64), 5, 4);
        circuit.c_new = Some(Fr::from(0xDEAD_BEEFu64));

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap(), "rebinding must NOT satisfy");
    }

    #[test]
    fn democracy_circuit_rejects_mismatched_member_count_delta() {
        let sks = vec![Fr::from(10u64), Fr::from(20u64), Fr::from(30u64)];
        // Replace means count_new = count_old. Prover lies: claims count_new
        // = count_old - 1.
        let mut circuit =
            build_replace_scenario(&sks, &[0, 1], 2, Fr::from(999u64), 5, 4);
        circuit.member_count_new = Some(2);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(
            !cs.is_satisfied().unwrap(),
            "member-count / delta-kind mismatch must NOT satisfy"
        );
    }

    #[test]
    fn democracy_circuit_rejects_out_of_range_count_delta() {
        let sks = vec![
            Fr::from(10u64),
            Fr::from(20u64),
            Fr::from(30u64),
            Fr::from(40u64),
        ];
        let mut circuit =
            build_replace_scenario(&sks, &[0, 1, 2], 3, Fr::from(999u64), 5, 4);
        // Claim count_new differs by 2 — out of {-1, 0, +1}.
        circuit.member_count_new = Some(6);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(
            !cs.is_satisfied().unwrap(),
            "|count delta| > 1 must NOT satisfy"
        );
    }

    #[test]
    fn democracy_circuit_empty_for_keygen() {
        let circuit = DemocracyUpdateCircuit::<Fr>::empty(5, 4);
        let cs = ConstraintSystem::<Fr>::new_ref();
        let result = circuit.generate_constraints(cs.clone());
        // An empty (witness-less) circuit must fail with AssignmentMissing so
        // keygen cannot silently produce a VK that accepts anything.
        assert!(result.is_err());
    }
}
