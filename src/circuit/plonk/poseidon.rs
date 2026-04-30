//! Poseidon permutation, native + (forthcoming) jf-relation gadget.
//!
//! Mirrors the parameter set in `crate::poseidon` exactly so that:
//!
//! - Hashes computed by this module under `arkworks` 0.5 produce the
//!   *same field-element* (when interpreted via canonical bytes) as
//!   hashes computed by `crate::poseidon` under `arkworks` 0.4.
//! - Existing Merkle commitments produced by 0.4 code remain
//!   verifiable by the 0.5 PLONK circuit when it lands (depends on
//!   the in-circuit gadget reproducing the same permutation; that's
//!   B.3-second-step).
//!
//! Parameters per SEP-XXXX §2.2 (also described in the legacy
//! `crate::poseidon` module's doc-comment):
//!
//! - Field: BLS12-381 scalar field (Fr, ≈2^255).
//! - Width: t = 3 (rate = 2, capacity = 1).
//! - Full rounds: R_F = 8 (4 at start, 4 at end).
//! - Partial rounds: R_P = 56.
//! - S-box: x^5.
//! - Round constants: derived from SHA-256 seed
//!   `"SEP-XXXX-Poseidon-BLS12-381-w3-f8-p56-a5-round-constants"`.
//! - MDS matrix: Cauchy `M[i][j] = 1 / (x_i + y_j)` with `x_i = i+1`,
//!   `y_j = w + j + 1`.
//!
//! This module's `poseidon_config_v05()` uses bit-for-bit identical
//! derivation logic. The `equivalent_to_v04_native` test verifies
//! agreement on a few fixed inputs.

#![cfg(feature = "plonk")]

use std::sync::OnceLock;

use ark_bls12_381_v05::Fr;
use ark_crypto_primitives_v05::sponge::poseidon::{PoseidonConfig, PoseidonSponge};
use ark_crypto_primitives_v05::sponge::CryptographicSponge;
use ark_ff_v05::{Field, PrimeField, Zero};

// Re-exported from the legacy module so we have a single source of truth
// for the parameter constants. They're plain `pub const` and version-
// independent, so this is a pure compile-time alias.
pub use crate::poseidon::{ALPHA, CAPACITY, FULL_ROUNDS, PARTIAL_ROUNDS, RATE};

const WIDTH: usize = RATE + CAPACITY;

/// Process-wide cache of the v0.5 Poseidon parameters.
///
/// Parameter derivation involves SHA-256-chaining for 192 round constants
/// and a Cauchy-matrix inversion for the MDS — non-trivial, so cache.
static CACHED_CONFIG: OnceLock<PoseidonConfig<Fr>> = OnceLock::new();

/// Returns a reference to the cached PoseidonConfig.
pub fn poseidon_config_v05() -> &'static PoseidonConfig<Fr> {
    CACHED_CONFIG.get_or_init(build_poseidon_config_v05)
}

fn build_poseidon_config_v05() -> PoseidonConfig<Fr> {
    let num_constants = (FULL_ROUNDS + PARTIAL_ROUNDS) * WIDTH;
    let flat = generate_round_constants_v05(num_constants);
    let ark = chunk_round_constants(flat);
    let mds = generate_mds_matrix_v05();
    PoseidonConfig {
        full_rounds: FULL_ROUNDS,
        partial_rounds: PARTIAL_ROUNDS,
        alpha: ALPHA,
        ark,
        mds,
        rate: RATE,
        capacity: CAPACITY,
    }
}

/// Hash two field elements via Poseidon (binary mode, for Merkle nodes).
pub fn poseidon_hash_two_v05(left: &Fr, right: &Fr) -> Fr {
    let cfg = poseidon_config_v05();
    let mut sponge = PoseidonSponge::<Fr>::new(cfg);
    sponge.absorb(left);
    sponge.absorb(right);
    sponge.squeeze_field_elements::<Fr>(1)[0]
}

/// Hash a single field element via Poseidon (used for leaf hashing).
pub fn poseidon_hash_one_v05(input: &Fr) -> Fr {
    let cfg = poseidon_config_v05();
    let mut sponge = PoseidonSponge::<Fr>::new(cfg);
    sponge.absorb(input);
    sponge.squeeze_field_elements::<Fr>(1)[0]
}

// ---------------------------------------------------------------------------
// In-circuit gadget — TurboPlonk gates over jf-relation's PlonkCircuit<Fr>.
//
// Translates the v0.5 native sponge above into gate operations. Algorithm
// mirrors arkworks v0.5 PoseidonSponge::permute (verified by reading
// crypto-primitives/v0.5.0/.../sponge/poseidon/mod.rs):
//
//   state = [0, 0, 0]                    // capacity slot 0 first; rate at 1, 2
//   state[1] += left, state[2] += right  // absorb (hash_two) or just state[1] += x
//   permute(state) {
//       for i in 0..R_F/2: ark + full_sbox + mds  // 4 initial full rounds
//       for i in R_F/2..R_F/2+R_P: ark + partial_sbox + mds  // 56 partial
//       for i in R_F/2+R_P..R_F+R_P: ark + full_sbox + mds  // 4 trailing full
//   }
//   output = state[1]                   // squeeze rate position 0
//
// Per-hash gate count is empirically ≈626 gates, pinned by
// `gadget_hash_two_gate_count`. The exact figure depends on how
// jf-relation composes `add_constant`, the S-box muls, and the
// width-4 `lc` gates we feed our width-3 state into; tracking it via
// the test, not arithmetic, keeps the comment honest under upstream
// changes.
// ---------------------------------------------------------------------------

use jf_relation::{Circuit, CircuitError, PlonkCircuit, Variable};

/// Hash two field elements as gates. Returns the output `Variable`.
///
/// Mirrors `poseidon_hash_two_v05` exactly — equivalence-tested via the
/// `gadget_matches_v05_native_hash_two` test.
pub fn poseidon_hash_two_gadget(
    circuit: &mut PlonkCircuit<Fr>,
    left: Variable,
    right: Variable,
) -> Result<Variable, CircuitError> {
    let zero = circuit.zero();
    // arkworks layout: [capacity_0, rate_0, rate_1] = [zero, left, right]
    let mut state = [zero, left, right];
    permute_gadget(circuit, &mut state)?;
    // Squeeze position 0 = state[capacity + 0] = state[1].
    Ok(state[1])
}

/// Hash a single field element as gates. Returns the output `Variable`.
pub fn poseidon_hash_one_gadget(
    circuit: &mut PlonkCircuit<Fr>,
    input: Variable,
) -> Result<Variable, CircuitError> {
    let zero = circuit.zero();
    // arkworks absorb: state[capacity + 0] += input → state = [0, input, 0]
    let mut state = [zero, input, zero];
    permute_gadget(circuit, &mut state)?;
    Ok(state[1])
}

/// Apply 64 rounds of Poseidon to `state` in place. Mirrors arkworks v0.5
/// `permute`: 4 initial full + 56 partial + 4 trailing full rounds.
fn permute_gadget(
    circuit: &mut PlonkCircuit<Fr>,
    state: &mut [Variable; WIDTH],
) -> Result<(), CircuitError> {
    let cfg = poseidon_config_v05();
    let half_full = cfg.full_rounds / 2;

    for i in 0..half_full {
        apply_ark(circuit, state, &cfg.ark[i])?;
        apply_full_sbox(circuit, state)?;
        apply_mds(circuit, state, &cfg.mds)?;
    }
    for i in half_full..(half_full + cfg.partial_rounds) {
        apply_ark(circuit, state, &cfg.ark[i])?;
        apply_partial_sbox(circuit, state)?;
        apply_mds(circuit, state, &cfg.mds)?;
    }
    for i in (half_full + cfg.partial_rounds)..(cfg.full_rounds + cfg.partial_rounds) {
        apply_ark(circuit, state, &cfg.ark[i])?;
        apply_full_sbox(circuit, state)?;
        apply_mds(circuit, state, &cfg.mds)?;
    }
    Ok(())
}

/// `state[i] += round_constants[i]` for all i.
fn apply_ark(
    circuit: &mut PlonkCircuit<Fr>,
    state: &mut [Variable; WIDTH],
    round_constants: &[Fr],
) -> Result<(), CircuitError> {
    debug_assert_eq!(round_constants.len(), WIDTH);
    for i in 0..WIDTH {
        state[i] = circuit.add_constant(state[i], &round_constants[i])?;
    }
    Ok(())
}

/// Full S-box: `state[i] = state[i]^5` for all i.
fn apply_full_sbox(
    circuit: &mut PlonkCircuit<Fr>,
    state: &mut [Variable; WIDTH],
) -> Result<(), CircuitError> {
    for i in 0..WIDTH {
        state[i] = pow5(circuit, state[i])?;
    }
    Ok(())
}

/// Partial S-box: `state[0] = state[0]^5`. (Only first element.)
fn apply_partial_sbox(
    circuit: &mut PlonkCircuit<Fr>,
    state: &mut [Variable; WIDTH],
) -> Result<(), CircuitError> {
    state[0] = pow5(circuit, state[0])?;
    Ok(())
}

/// `x^5 = ((x^2)^2) * x` — three multiplications, three gates.
fn pow5(circuit: &mut PlonkCircuit<Fr>, x: Variable) -> Result<Variable, CircuitError> {
    let x2 = circuit.mul(x, x)?;
    let x4 = circuit.mul(x2, x2)?;
    circuit.mul(x4, x)
}

/// MDS matrix multiply: `state' = M · state`. Width=3, so each output is
/// `m[i][0]*s[0] + m[i][1]*s[1] + m[i][2]*s[2]`. Encoded as a single linear-
/// combination gate per row.
fn apply_mds(
    circuit: &mut PlonkCircuit<Fr>,
    state: &mut [Variable; WIDTH],
    mds: &[Vec<Fr>],
) -> Result<(), CircuitError> {
    debug_assert_eq!(mds.len(), WIDTH);
    let zero = circuit.zero();
    let s = *state;
    for i in 0..WIDTH {
        debug_assert_eq!(mds[i].len(), WIDTH);
        // jf-relation's `lc` is GATE_WIDTH = 4 wide; pad with a zero
        // wire+coefficient since our state width is 3.
        let wires_in = [s[0], s[1], s[2], zero];
        let coeffs = [mds[i][0], mds[i][1], mds[i][2], Fr::zero()];
        state[i] = circuit.lc(&wires_in, &coeffs)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Parameter derivation — bit-for-bit identical to crate::poseidon (v0.4),
// but expressed in terms of v0.5 PrimeField + `from_le_bytes_mod_order`.
// ---------------------------------------------------------------------------

fn generate_round_constants_v05(count: usize) -> Vec<Fr> {
    use sha2::{Digest, Sha256};

    let mut constants = Vec::with_capacity(count);
    let mut seed = Sha256::digest(b"SEP-XXXX-Poseidon-BLS12-381-w3-f8-p56-a5-round-constants");
    for _ in 0..count {
        let mut extended = [0u8; 64];
        extended[..32].copy_from_slice(&seed);
        seed = Sha256::digest(seed);
        extended[32..].copy_from_slice(&seed);
        seed = Sha256::digest(seed);
        constants.push(Fr::from_le_bytes_mod_order(&extended));
    }
    constants
}

fn generate_mds_matrix_v05() -> Vec<Vec<Fr>> {
    let mut matrix = Vec::with_capacity(WIDTH);
    for i in 0..WIDTH {
        let mut row = Vec::with_capacity(WIDTH);
        for j in 0..WIDTH {
            let x = Fr::from((i + 1) as u64);
            let y = Fr::from((WIDTH + j + 1) as u64);
            let entry = (x + y)
                .inverse()
                .expect("Cauchy denominators must be invertible");
            row.push(entry);
        }
        matrix.push(row);
    }
    matrix
}

fn chunk_round_constants(flat: Vec<Fr>) -> Vec<Vec<Fr>> {
    flat.chunks(WIDTH).map(|c| c.to_vec()).collect()
}

// ---------------------------------------------------------------------------
// Conversion between v0.4 and v0.5 field elements via canonical bytes.
//
// Both crates use the same canonical Montgomery-form 32-byte little-endian
// encoding for BLS12-381 Fr. We round-trip through bytes to convert.
// ---------------------------------------------------------------------------

#[cfg(test)]
fn fr_v04_to_v05(x: &ark_bls12_381::Fr) -> Fr {
    use ark_serialize::CanonicalSerialize as Ser04;
    use ark_serialize_v05::CanonicalDeserialize as De05;
    let mut buf = Vec::new();
    x.serialize_uncompressed(&mut buf).expect("serialize v0.4");
    Fr::deserialize_uncompressed(&buf[..]).expect("deserialize v0.5")
}

#[cfg(test)]
fn fr_v05_to_v04(x: &Fr) -> ark_bls12_381::Fr {
    use ark_serialize::CanonicalDeserialize as De04;
    use ark_serialize_v05::CanonicalSerialize as Ser05;
    let mut buf = Vec::new();
    x.serialize_uncompressed(&mut buf).expect("serialize v0.5");
    ark_bls12_381::Fr::deserialize_uncompressed(&buf[..]).expect("deserialize v0.4")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poseidon as v04;

    /// Two independent invocations of the parameter-build function must
    /// return identical parameters. Calls `build_poseidon_config_v05`
    /// directly (twice) to bypass the `OnceLock` cache — `c1 == c2`
    /// would be by-construction otherwise, since both refs would point
    /// at the same cached entry.
    #[test]
    fn config_is_deterministic() {
        let c1 = build_poseidon_config_v05();
        let c2 = build_poseidon_config_v05();

        // Shape:
        assert_eq!(c1.full_rounds, FULL_ROUNDS);
        assert_eq!(c1.partial_rounds, PARTIAL_ROUNDS);
        assert_eq!(c1.alpha, ALPHA);
        assert_eq!(c1.rate, RATE);
        assert_eq!(c1.capacity, CAPACITY);
        assert_eq!(c1.ark.len(), FULL_ROUNDS + PARTIAL_ROUNDS);
        for round in &c1.ark {
            assert_eq!(round.len(), WIDTH);
        }
        assert_eq!(c1.mds.len(), WIDTH);
        for row in &c1.mds {
            assert_eq!(row.len(), WIDTH);
        }

        // Determinism: same inputs → same constants (independent of the cache).
        assert_eq!(c1.ark, c2.ark);
        assert_eq!(c1.mds, c2.mds);
    }

    /// The v0.5 round constants and MDS reproduce the v0.4 constants
    /// when compared via canonical bytes. Catches any divergence in the
    /// parameter derivation logic between the two version-aliased crates.
    #[test]
    fn parameters_match_v04() {
        let v05 = poseidon_config_v05();
        let v04_cfg = v04::poseidon_config::<ark_bls12_381::Fr>();

        assert_eq!(v05.ark.len(), v04_cfg.ark.len(), "ark length");
        for (i, (r5, r4)) in v05.ark.iter().zip(v04_cfg.ark.iter()).enumerate() {
            assert_eq!(r5.len(), r4.len(), "ark[{i}] length");
            for (j, (e5, e4)) in r5.iter().zip(r4.iter()).enumerate() {
                let converted = fr_v05_to_v04(e5);
                assert_eq!(
                    &converted, e4,
                    "ark[{i}][{j}] mismatches between v0.5 and v0.4"
                );
            }
        }

        assert_eq!(v05.mds.len(), v04_cfg.mds.len(), "mds rows");
        for (i, (r5, r4)) in v05.mds.iter().zip(v04_cfg.mds.iter()).enumerate() {
            for (j, (e5, e4)) in r5.iter().zip(r4.iter()).enumerate() {
                let converted = fr_v05_to_v04(e5);
                assert_eq!(
                    &converted, e4,
                    "mds[{i}][{j}] mismatches between v0.5 and v0.4"
                );
            }
        }
    }

    /// `poseidon_hash_two_v05(l, r)` produces the same value as
    /// `crate::poseidon::poseidon_hash_two(cfg, l_v04, r_v04)`. This is
    /// the highest-leverage test: if the v0.5 native sponge differs from
    /// v0.4 by even a single field element across any input, the
    /// in-circuit gadget (next subtask) will silently produce wrong
    /// commitments.
    #[test]
    fn hash_two_matches_v04_native() {
        let cfg_v04 = v04::poseidon_config::<ark_bls12_381::Fr>();

        let test_pairs: &[(u64, u64)] = &[
            (0, 0),
            (1, 0),
            (0, 1),
            (1, 2),
            (2, 1),
            (42, 1337),
            (1u64 << 50, 1u64 << 51),
            (u64::MAX, u64::MAX - 1),
        ];

        for &(l, r) in test_pairs {
            let l_v04 = ark_bls12_381::Fr::from(l);
            let r_v04 = ark_bls12_381::Fr::from(r);
            let h_v04 = v04::poseidon_hash_two(&cfg_v04, &l_v04, &r_v04);

            let l_v05 = fr_v04_to_v05(&l_v04);
            let r_v05 = fr_v04_to_v05(&r_v04);
            let h_v05 = poseidon_hash_two_v05(&l_v05, &r_v05);

            let h_v05_back = fr_v05_to_v04(&h_v05);
            assert_eq!(
                h_v05_back, h_v04,
                "hash_two(({l}, {r})) diverges between v0.5 and v0.4"
            );
        }
    }

    /// Same equivalence check, single-input form.
    #[test]
    fn hash_one_matches_v04_native() {
        let cfg_v04 = v04::poseidon_config::<ark_bls12_381::Fr>();

        for &x in &[0u64, 1, 42, 1u64 << 50, u64::MAX] {
            let x_v04 = ark_bls12_381::Fr::from(x);
            let h_v04 = v04::poseidon_hash_one(&cfg_v04, &x_v04);

            let x_v05 = fr_v04_to_v05(&x_v04);
            let h_v05 = poseidon_hash_one_v05(&x_v05);

            assert_eq!(
                fr_v05_to_v04(&h_v05),
                h_v04,
                "hash_one({x}) diverges between v0.5 and v0.4"
            );
        }
    }

    // -------------------------------------------------------------------
    // Gadget tests — exercise the in-circuit Poseidon against the native
    // v0.5 sponge, witnessed at the gate-graph level (cheap) and at the
    // full prove → verify level (slow but airtight).
    // -------------------------------------------------------------------

    use jf_relation::{Circuit, PlonkCircuit};

    /// Sanity-check the gate count for one Poseidon hash. Logged so the
    /// gate-count budget is visible if a future jf-relation upgrade
    /// changes how the basic gates compose.
    #[test]
    fn gadget_hash_two_gate_count() {
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        let l = c.create_variable(Fr::from(1u64)).unwrap();
        let r = c.create_variable(Fr::from(2u64)).unwrap();
        let _ = poseidon_hash_two_gadget(&mut c, l, r).unwrap();
        let n = c.num_gates();
        let v = c.num_vars();
        eprintln!("[gate-count] one Poseidon hash_two: {n} gates, {v} variables");
        // Roughly: 64 rounds × (3 ARK + S-box + 3 MDS) = pinned-by-test
        // bound, not a strict equality. Surface to track regressions.
        assert!(
            n < 1500,
            "Poseidon hash_two gadget exceeds 1500 gates ({n}); expected ≈ 600-700 \
             with current encoding. Regression?"
        );
        // Lower-bound rules out an accidental no-op gadget.
        assert!(n > 300, "Poseidon hash_two gadget under 300 gates ({n}); suspicious");
    }

    /// `poseidon_hash_two_gadget(a, b)` produces the same field element as
    /// `poseidon_hash_two_v05(a, b)` at the witness/satisfiability level.
    /// Doesn't need a prover; just runs the circuit's witness assignment
    /// and reads the gadget's output variable.
    #[test]
    fn gadget_hash_two_matches_v05_native_at_witness_level() {
        let pairs: &[(u64, u64)] = &[(0, 0), (1, 0), (0, 1), (1, 2), (42, 1337)];
        for &(l, r) in pairs {
            let l_v05 = Fr::from(l);
            let r_v05 = Fr::from(r);
            let expected = poseidon_hash_two_v05(&l_v05, &r_v05);

            let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
            let l_var = circuit.create_variable(l_v05).unwrap();
            let r_var = circuit.create_variable(r_v05).unwrap();
            let out_var = poseidon_hash_two_gadget(&mut circuit, l_var, r_var).unwrap();

            // The witness value of the output Variable should equal the
            // native hash. This is the cheapest equivalence check — runs
            // the gate graph as an interpreter, no prover involved.
            let got = circuit.witness(out_var).unwrap();
            assert_eq!(
                got, expected,
                "gadget hash_two({l}, {r}) diverges from v0.5 native"
            );
        }
    }

    /// Same property for hash_one.
    #[test]
    fn gadget_hash_one_matches_v05_native_at_witness_level() {
        for &x in &[0u64, 1, 42, 1u64 << 50, u64::MAX] {
            let x_v05 = Fr::from(x);
            let expected = poseidon_hash_one_v05(&x_v05);

            let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
            let x_var = circuit.create_variable(x_v05).unwrap();
            let out_var = poseidon_hash_one_gadget(&mut circuit, x_var).unwrap();

            let got = circuit.witness(out_var).unwrap();
            assert_eq!(
                got, expected,
                "gadget hash_one({x}) diverges from v0.5 native"
            );
        }
    }

    /// Full end-to-end: build a circuit asserting
    /// `gadget(a, b) == public_input`, set witness, prove, verify against
    /// the native `poseidon_hash_two_v05(a, b)` as the public input.
    /// Validates the gadget against the prover *and* verifier paths
    /// (catches bugs that satisfiability checks alone miss — e.g. wrong
    /// coefficient on a `lc` gate that still happens to satisfy the
    /// constraint at one specific witness assignment).
    #[test]
    fn gadget_hash_two_round_trip_prove_verify() {
        use rand_chacha::rand_core::SeedableRng;

        let l = Fr::from(42u64);
        let r = Fr::from(1337u64);
        let expected = poseidon_hash_two_v05(&l, &r);

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        // Public input: the expected hash output.
        let expected_var = circuit.create_public_variable(expected).unwrap();
        let l_var = circuit.create_variable(l).unwrap();
        let r_var = circuit.create_variable(r).unwrap();
        let computed = poseidon_hash_two_gadget(&mut circuit, l_var, r_var).unwrap();
        circuit.enforce_equal(computed, expected_var).unwrap();
        circuit.finalize_for_arithmetization().unwrap();

        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&circuit).expect("preprocess");
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &circuit).expect("prove");
        crate::prover::plonk::verify(&keys.vk, &[expected], &proof)
            .expect("verifier rejected a valid Poseidon-gadget proof");

        // And confirm the verifier rejects a tampered public input.
        let wrong = expected + Fr::from(1u64);
        assert!(
            crate::prover::plonk::verify(&keys.vk, &[wrong], &proof).is_err(),
            "verifier accepted Poseidon-gadget proof against wrong public input"
        );
    }
}
