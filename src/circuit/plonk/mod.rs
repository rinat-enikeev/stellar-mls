//! TurboPlonk-flavoured circuits + gadgets, built on `jf-relation` over
//! `arkworks` 0.5.
//!
//! Gated behind `feature = "plonk"`. The legacy R1CS / Groth16 circuits
//! continue to live in their original locations (`src/circuit/mod.rs`,
//! `src/circuit/update.rs`, `src/circuit/democracy.rs`) under
//! `feature = "groth16"`; this module is parallel and additive until
//! Phase B.4 / C ships the cutover.
//!
//! Implementation order tracked in
//! `docs/implementation-plan-fflonk-migration.md` Phase B.3:
//!
//! - `poseidon` — Poseidon permutation, native + jf-relation gadget.
//!   First subtask: equivalence-test the v0.5 native against the
//!   existing v0.4 `crate::poseidon` to lock down parameter
//!   reproduction. Once green, the gadget version goes in.
//! - `merkle` — Merkle membership gadget on top of Poseidon. (Pending.)
//! - `membership` — assembles the two gadgets into the
//!   `MembershipCircuit` port. (Pending.)

#![cfg(feature = "plonk")]

pub mod poseidon;
