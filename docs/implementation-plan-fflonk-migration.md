# fflonk + EF KZG Migration — Implementation Plan

**Date:** 2026-04-29
**Based on:** [`fflonk-migration-design.md`](fflonk-migration-design.md)
**Motivated by:** [`postmortem-ceremony-data-loss.md`](postmortem-ceremony-data-loss.md)
**Status:** Draft (v1 — turns the design into actionable phases)
**Version:** 1.0
**Target release:** keyset-fflonk-v1 / per-type contracts v2

---

## Overview

This document turns the fflonk migration design into a concrete, phase-by-phase
execution plan. Every phase enumerates the files touched, the shape of the
change, the tests that must pass, the verification step that proves the phase
is done, and the prerequisite phases each step depends on.

The migration replaces the project's Groth16 + per-circuit MPC ceremony with
**fflonk on BLS12-381 consuming Ethereum Foundation's 2023 KZG SRS**, and
decommissions the entire ceremony operational surface
(`ceremony-coordinator/`, `tools/ceremony/`, `crates/ceremony-wasm/`,
`deploy/ceremony/`, three `scripts/install-*-vks-*.sh`, the `phase2_*` schema
in the coordinator DB) along with the legacy monolithic `sep-xxxx` contract.
The five per-type governance contracts (`sep-anarchy`, `sep-democracy`,
`sep-oligarchy`, `sep-oneonone`, `sep-tyranny`) gain a new fflonk verifier
path; their VKs become compiled-in `static` constants instead of per-tier
storage entries.

The plan covers **six phases (A–F) across 38 numbered tasks** spanning the
Rust core, five Soroban contracts, the ceremony decommission, two mobile
client bundles, the FFI/JNI surface, and rollout coordination.

### Design inputs

| Input | Source |
|---|---|
| Proving-system selection (fflonk on BLS12-381) | `fflonk-migration-design.md` §4.1 |
| SRS source (EF 2023 KZG) | `fflonk-migration-design.md` §4.2 |
| Architecture (no coordinator, VK-as-bytecode, deterministic preprocessing) | `fflonk-migration-design.md` §4.3, §4.4 |
| Mobile bundle reshape | `fflonk-migration-design.md` §4.5 |
| Phase boundaries A–F | `fflonk-migration-design.md` §6 |
| Risk register | `fflonk-migration-design.md` §9 |
| Open questions | `fflonk-migration-design.md` §10 |
| Postmortem reference for Phase A | `postmortem-ceremony-data-loss.md` |

### Out of scope

Per the design doc, the following are **deliberately deferred** and are *not*
implementation targets in this plan:

- Mainnet deployment of the fflonk-verifier contracts. Mainnet is downstream of
  [`mainnet-deployment.md`](mainnet-deployment.md) and is its own gate.
- Migrating existing testnet groups created against `sep-xxxx`. They continue
  to verify Groth16 proofs on the legacy contract until Phase F decommissions it.
- New circuit functionality. Circuits ported in Phase B preserve existing
  semantics — same public inputs, same statement, same Poseidon parameters.
  Any *new* circuit (e.g. for an unshipped governance type) is a separate
  follow-up that consumes the SRS produced here.
- Changing the Soroban host function set. The plan assumes
  `bls12_381::{g1_add, g1_msm, g2_msm, pairing_check}` and SHA-256 are the
  available primitives; if any of these is missing on the deploy target, that
  is a Soroban-platform issue, not a migration-plan issue.
- A new threat model for proof replay / front-running. The existing
  `UsedProof` nullifier set + `check_proof_replay` / `record_proof` pattern
  ([`update-circuit-binding-design.md`](update-circuit-binding-design.md)) is
  preserved verbatim by this plan; only the proof's verification primitive
  changes.
- Removing `sep-xxxx` from on-chain ledger state. Phase F drops the crate from
  the workspace and stops new deployments; existing deployed contract
  instances are left to TTL out per Soroban's storage policy.

### Normative invariants

These must hold at every implementation layer. Any departure is a bug, not a
choice.

- **One global universal SRS.** A single 4096-G1 + 65-G2 byte sequence (≈206
  KB) is the source of truth for every circuit. Its SHA-256 hash is hardcoded
  in `src/prover/srs.rs` and checked at build time. Two SRS bundles in the
  workspace ⇒ build break.
- **EF KZG provenance, not self-run ceremony.** The embedded SRS bytes
  reproduce — bit-for-bit — the EF 2023 KZG ceremony's published transcript.
  Any other SRS source is a divergence from this plan and requires its own
  design doc.
- **Verifier keys are static, not stored.** Each per-type contract embeds its
  VK(s) as compile-time `&[u8]` constants (or
  `lazy_static!`-decoded equivalents). `DataKey::VK(_)` and `DataKey::UpdateVK(_)`
  cease to exist. There is no `update_vk` admin entrypoint.
- **Proof wire format is versioned.** `PlonkProof` carries a 1-byte version
  prefix (`0x01` for fflonk-v1). Future proving-system swaps bump the prefix.
- **No per-circuit setup.** `src/prover` does not call any "circuit-specific
  setup" function. Per-circuit preprocessing (deriving the prover key + VK
  from `(circuit, srs)`) is a deterministic build-time computation, run once
  per circuit per workspace build.
- **Public inputs unchanged.** The migration preserves each circuit's
  `(public_inputs)` tuple bit-for-bit — Membership: `(C, epoch)`; Update:
  `(C_old, epoch_old, C_new)`; Democracy: per the existing design doc.
  Wire encoding bytes stay identical.
- **No proof of correctness for SRS at runtime.** The SRS hash check happens
  at build time; runtime trusts it. (We are not running our own ceremony, so
  there are no per-round receipts to verify on the prover/verifier hot path.)
- **No regression in nullifier hygiene.** The nullifier-set TTL bounds, the
  `check_proof_replay` / `record_proof` ordering, and the `UsedProof`
  storage key shape are preserved across the migration unchanged.

### Sub-PR partitioning

The plan is structured around 12 sub-PRs (`fflonk-pr-N`):

| PR | Phase(s) | Mergeable independently? |
|---|---|---|
| 1 | A.1, A.2 | Yes — operational, no code |
| 2 | B.1, B.2 | Yes — workspace deps + SRS embed; behind feature flag |
| 3 | B.3 (membership), B.4 (membership-only prover) | Yes — under `feature = "fflonk"` |
| 4 | B.3 (update + democracy), B.5, B.6 | Yes — depends on PR 3 |
| 5 | C.1, C.6 | Yes — proof type lib + drop sep-xxxx from workspace |
| 6 | C.2 — sep-anarchy verifier | Yes — depends on PR 4 |
| 7 | C.2 — sep-democracy verifier | Yes — depends on PR 4 |
| 8 | C.2 — sep-oligarchy + sep-oneonone + sep-tyranny verifiers | Yes — depends on PR 4 |
| 9 | C.3, C.4, C.5 | Yes — storage cleanup + gas bench, depends on PRs 6–8 |
| 10 | D.1–D.7 | Yes — mobile rebundle, depends on PR 4 |
| 11 | E.1–E.6 | Yes — coordinator decommission, depends on PRs 6–10 |
| 12 | F.1–F.5 | Yes — final decommission, depends on PR 11 |

---

## Dependency DAG

```
                ┌──────────────────────────┐
                │  Phase A — receipt       │   (parallel, operational)
                │  salvage                 │
                └──────────────────────────┘

Phase B.1 (Cargo deps)
    │
Phase B.2 (SRS embed + hash check)
    │
Phase B.3 (port circuits to PLONK gates)
    │
Phase B.4 (replace prover module)
    │
Phase B.5 (cross-platform test vectors)
    │
Phase B.6 (build-time SRS hash assertion)
    │
    ├─────────────────────────────────────┐
    ▼                                     ▼
Phase C.1 (PlonkProof type in stellar-sdk)
    │                                Phase D.1 (iOS resources)
    ├──────┬──────┬──────┬───────┐     │
    ▼      ▼      ▼      ▼       ▼     Phase D.2 (Android assets)
   C.2.a  C.2.b  C.2.c  C.2.d  C.2.e   │
  anarchy demo. olig.  1on1   tyran.   Phase D.3 (FFI surface trim)
    │      │      │      │       │       │
    └──────┴──────┴──────┴───────┘     Phase D.4 (Swift bridge)
             │                          │
       Phase C.3 (drop VK storage)      Phase D.5 (Kotlin bridge)
             │                          │
       Phase C.4 (drop update_vk)       Phase D.6 (test vectors)
             │                          │
       Phase C.5 (gas benchmark)        Phase D.7 (on-device tests)
             │                          │
       Phase C.6 (drop sep-xxxx)        │
             │                          │
             └──────────────┬───────────┘
                            ▼
                    Phase E (decommission)
                            │
                            ▼
                    Phase F (close out)
```

Status indicators: ☐ not started, 🔧 in progress, ✅ done.

---

## Phase A — Receipt salvage (parallel, operational)

**Goal.** Recover as many of the 22 missing phase-1 sidecars
(`state.txt` + `receipt.txt`) as possible from the 7 contributors before
proceeding with the architectural migration. This phase is strictly cleanup
of the historical record; it does not gate any later phase.

**Effort.** ≤1 week elapsed; ≤4 hours engineering.

### Status: ☐

### A.1 — Compose participant outreach

**Files.** None (this is operational work; documented in `ceremony-backup/20260429T163349Z/MISSING.txt`).

**Changes.**

- Per the postmortem's "How this was found" §, group the 22 missing
  hashes by participant pubkey (use `rounds.participant_pk` as the join
  key). Produce a per-participant message of the form:

  > "Round R (tier T) of the Onym phase-1 ceremony lost two small
  > artifacts to a server-side retention bug (see PR #161). Your local
  > directory `state-R/` likely still contains:
  >
  >   - `receipt.txt` — SHA-256 should be `<expected_hash>`
  >   - `state.txt` — SHA-256 should be `<expected_hash>`
  >
  > Could you `curl -X PUT --data-binary @receipt.txt
  > https://blossom.onym.chat/upload …` (full BUD-02 invocation in the
  > attached snippet)? The hash will match what's in our DB and the file
  > will land back in its original slot."

- Send via whichever channel the participant signed up through. Names
  are in `participants` table; `email_optional` may be present.

**Verification.** Outreach sent within 48 hours of Phase A start.

**Dependencies.** PR #161 (`deploy/blossom/config.yml`) merged. Already done.

### A.2 — Validate and re-publish recovered blobs

**Files.** None (Blossom is content-addressed; uploads from the
participant's pubkey land natively).

**Changes.**

- For each blob a participant returns, validate by SHA-256 against the
  expected hash from `rounds.{state_txt_hash, receipt_hash}` before
  acknowledging.
- If the hash matches, `curl -X PUT` to `blossom.onym.chat/upload`
  using the coordinator's BUD-02 auth (or accept the participant's own
  signed upload if their pubkey is added to the
  `deploy/blossom/config.yml` rules).
- Re-test by `curl https://blossom.onym.chat/<sha256>` returning 200.

**Verification.** Per recovered round, the public URLs for both
sidecars return 200. The `MISSING.txt` line for that round is removed.

**Dependencies.** A.1.

### A.3 — Close the historical record

**Files.** `docs/postmortem-ceremony-data-loss.md` (§Blast radius)

**Changes.** After two weeks of A.1 outreach (or earlier if 100%
recovery), update the postmortem's "Blast radius" section with the
final recovery state:

```markdown
### Final recovery state (2026-MM-DD)

Recovered N of 22 sidecars from M participants. Surviving gaps:
- <tier>/r<round> {state.txt, receipt.txt}: irrecoverable.

Auditability impact: ...
```

Move the postmortem header from "Decided" to "Complete."

**Verification.** Postmortem PR merged.

**Dependencies.** A.2 (or two-week timeout).

---

## Phase B — Proving-system swap

**Goal.** Replace `ark-groth16` with a fflonk-on-BLS12-381 implementation,
embed EF KZG SRS, port the three circuits to PLONK custom gates, regenerate
test vectors. Land everything behind a `feature = "fflonk"` flag so it can
merge incrementally without breaking existing testnet flows.

**Effort.** 2–3 weeks engineering.

### Status: ☐

### B.1 — Cargo deps + feature flag

**Files.**

- `/Users/programyzer/Developer/stellar-mls/Cargo.toml` (workspace root, lines 13–40)
- `/Users/programyzer/Developer/stellar-mls/src/Cargo.toml` (sep-mls crate, if present)

**Changes.**

```toml
# Add (workspace deps):
jellyfish = { version = "0.4", default-features = false, features = ["bls12-381"] }
# Or arkworks-fflonk equivalent if jellyfish lacks fflonk. See §10 of the design doc.

# Make Groth16 optional behind a feature gate (do NOT remove yet):
ark-groth16 = { version = "0.4", optional = true }
ark-snark   = { version = "0.4", optional = true }

[features]
default = ["groth16"]
groth16 = ["ark-groth16", "ark-snark"]
fflonk = ["jellyfish"]
```

- Both `groth16` and `fflonk` features exist simultaneously through the
  end of Phase C. Groth16 is the default until C.2 ships for all five
  contracts.
- `ark-bls12-381`, `ark-ff`, `ark-ec`, `ark-serialize` stay (jellyfish
  reuses arkworks types).
- `ark-relations`, `ark-r1cs-std` stay during transition; deleted in
  Phase E with the rest of the Groth16 surface.

**Tests.** `cargo build --no-default-features --features groth16` and
`cargo build --no-default-features --features fflonk` both succeed.
Default build (`cargo build`) still passes.

**Verification.** CI green for all three feature combinations.

**Dependencies.** None.

### B.2 — Embed EF KZG SRS + build-time hash check

**Files.**

- **(new)** `/Users/programyzer/Developer/stellar-mls/src/prover/srs.rs`
- **(new)** `/Users/programyzer/Developer/stellar-mls/src/prover/srs/ef-kzg-2023.bin` (≈206 KB)
- `/Users/programyzer/Developer/stellar-mls/src/prover/mod.rs:1-50` (add `pub mod srs;`)
- **(new)** `/Users/programyzer/Developer/stellar-mls/build.rs` (workspace root)

**Changes.**

Step 1 — fetch + verify the SRS once, manually:

```bash
# Download the EF 2023 KZG ceremony transcript and convert to the
# arkworks-compatible byte layout. Document the exact command + hash
# in src/prover/srs/README.md so it's reproducible.
curl -fsSL https://ceremony.ethereum.org/api/v1/transcript \
  | scripts/build/convert-kzg-to-arkworks.py \
  > src/prover/srs/ef-kzg-2023.bin
shasum -a 256 src/prover/srs/ef-kzg-2023.bin
# expected: <hash recorded in src/prover/srs.rs>
```

Step 2 — `src/prover/srs.rs`:

```rust
//! Universal SRS for the fflonk prover.
//!
//! Source: Ethereum Foundation 2023 KZG ceremony, finalised
//! 2023-11-14, ≈141k contributors. Powers-of-tau over BLS12-381,
//! 4096 G1 + 65 G2 elements, ≈206 KB on disk in arkworks
//! uncompressed encoding.
//!
//! The hash check below makes it structurally impossible to ship the
//! wrong SRS — the build fails on a mismatch.

const SRS_BYTES: &[u8] = include_bytes!("srs/ef-kzg-2023.bin");
const SRS_SHA256: [u8; 32] = hex_literal::hex!(
    "<EF KZG transcript hash, captured during B.2 download>"
);

pub fn srs() -> &'static [u8] {
    SRS_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn embedded_srs_matches_ef_kzg_ceremony_hash() {
        let actual = Sha256::digest(SRS_BYTES);
        assert_eq!(
            actual.as_slice(),
            &SRS_SHA256[..],
            "SRS bytes do not match EF KZG ceremony hash"
        );
    }
}
```

Step 3 — `build.rs` (also enforces at compile time, not just test time):

```rust
fn main() {
    println!("cargo:rerun-if-changed=src/prover/srs/ef-kzg-2023.bin");
    let bytes = std::fs::read("src/prover/srs/ef-kzg-2023.bin")
        .expect("EF KZG SRS not present at src/prover/srs/ef-kzg-2023.bin");
    let actual: [u8; 32] = sha2::Sha256::digest(&bytes).into();
    let expected = include!("src/prover/srs/expected-hash.in");
    assert_eq!(actual, expected, "SRS hash mismatch — refusing to build");
}
```

(`expected-hash.in` is a one-line `[u8; 32]` literal, kept separately so a
file-replace attack on `srs/ef-kzg-2023.bin` requires also touching the
hash file in the same commit.)

**Tests.**

- `cargo test embedded_srs_matches_ef_kzg_ceremony_hash` — passes.
- Tampering test: flip a byte in `ef-kzg-2023.bin`; `cargo build`
  fails with "SRS hash mismatch."

**Verification.** Both checks (build-time and test-time) green; tamper
test reproduces the build failure.

**Dependencies.** B.1.

### B.3 — Port circuits to PLONK custom gates

**Files.**

- `/Users/programyzer/Developer/stellar-mls/src/circuit/mod.rs` (788 LOC) — `MembershipCircuit`
- `/Users/programyzer/Developer/stellar-mls/src/circuit/update.rs` (440 LOC) — `UpdateCircuit`
- `/Users/programyzer/Developer/stellar-mls/src/circuit/democracy.rs` (851 LOC) — `DemocracyUpdateCircuit`

**Changes (per circuit).**

Each circuit currently expresses constraints as
`ConstraintSynthesizer<Bls12_381>::generate_constraints(&self, cs)`. Port to
jellyfish's `Circuit::synthesize(&self, builder: &mut PlonkCircuit<Fr>)`.

Mechanical mapping:

| arkworks R1CS | jellyfish PLONK |
|---|---|
| `cs.new_witness_variable(\|\| Ok(value))?` | `builder.create_variable(value)?` |
| `cs.enforce_constraint(lc1, lc2, lc3)?` | `builder.mul_gate(a, b, c)?` or `builder.add_gate(...)` |
| `Boolean::new_witness(...)?` | `builder.create_boolean_variable(...)?` |
| Poseidon gadget from `ark-crypto-primitives` | `jellyfish::gadgets::poseidon` (already BLS12-381) |
| Merkle membership gadget | `jellyfish::gadgets::merkle_tree` |

For each circuit, allocate **all public inputs first, in the same order
as the existing R1CS** (preserves wire format compatibility):

- `MembershipCircuit`: `(commitment, epoch)` — 2 public inputs.
- `UpdateCircuit`: `(C_old, epoch_old, C_new)` — 3 public inputs.
- `DemocracyUpdateCircuit`: per the existing design — preserve verbatim.

Use Plookup tables for Poseidon round constants where supported; this
typically halves PLONK row counts vs. naive multiplicative encoding. Final
row count target per circuit, after porting:

| Circuit | Tier | R1CS today | Target PLONK rows | EF SRS headroom |
|---|---|---|---|---|
| Membership | small | ~1,910 | ≤1,024 | 4× |
| Membership | medium | ~2,630 | ≤1,536 | 2.7× |
| Membership | large | ~3,350 | ≤2,048 | 2× |
| Update | small | ~2,400 | ≤1,280 | 3.2× |
| Update | medium | ~3,100 | ≤1,792 | 2.3× |
| Update | large | ~3,820 | ≤2,048 | 2× |
| DemocracyUpdate | (single) | ~5,200 | ≤2,048 | 2× |

If the port pushes any circuit over 2,048 rows, switch to Aztec Ignition
SRS (n=2^21) — a one-line change to the SRS source. **All current circuits
are expected to fit.**

**Tests.**

- `src/circuit/tests.rs` — existing R1CS-correctness tests
  (well-formed witness ⇒ verifies; tampered witness ⇒ rejects). Port
  each test to use jellyfish's `prove` + `verify` instead of
  `ConstraintSystem::is_satisfied()`.
- New test: `prove_then_verify_round_trip<C: Circuit>` parameterised
  on each of the three circuits + each tier.
- New test: `cross-public-input-rejection` — generate a proof for
  `(C, epoch)` then call `verify` with `(C', epoch)` — must reject.

**Verification.** All tests green; row-count targets in the table above
hit (logged via `builder.num_rows()`). The wire encoding of public
inputs is byte-identical to the existing implementation, asserted by a
new test that round-trips through the existing
`PublicInputs::serialize()` / `deserialize()`.

**Dependencies.** B.2.

### B.4 — Replace prover module

**Files.**

- `/Users/programyzer/Developer/stellar-mls/src/prover/mod.rs` (1,030 LOC)
- **(new)** `/Users/programyzer/Developer/stellar-mls/src/prover/fflonk.rs`
- `/Users/programyzer/Developer/stellar-mls/src/prover/groth16.rs` (extracted from current `mod.rs`, kept under `feature = "groth16"`)

**Changes.**

Step 1 — split current `mod.rs` into two:

```rust
// src/prover/mod.rs (new shape)
pub mod srs;

#[cfg(feature = "groth16")]
pub mod groth16;
#[cfg(feature = "fflonk")]
pub mod fflonk;

#[cfg(all(feature = "groth16", not(feature = "fflonk")))]
pub use groth16::*;
#[cfg(feature = "fflonk")]
pub use fflonk::*;
```

Step 2 — `src/prover/fflonk.rs`:

```rust
use crate::circuit::{MembershipCircuit, UpdateCircuit, DemocracyUpdateCircuit};
use crate::prover::srs::srs;
use jellyfish::{plonk::Proof, PlonkType};

/// Preprocessed prover key + verifier key for one circuit.
/// Computed once at startup; cached in a `OnceCell`.
pub struct CircuitKeys {
    pub pk: jellyfish::plonk::ProvingKey,
    pub vk: jellyfish::plonk::VerifyingKey,
}

pub fn preprocess<C: jellyfish::Circuit<...>>(circuit: &C) -> CircuitKeys {
    let srs_bytes = srs();
    let srs_obj = jellyfish::plonk::Srs::deserialize(srs_bytes).unwrap();
    let (pk, vk) = jellyfish::plonk::preprocess(&srs_obj, circuit).unwrap();
    CircuitKeys { pk, vk }
}

pub fn prove<C>(circuit: &C, witness: &C::Witness) -> Proof
where C: jellyfish::Circuit<...> { /* ... */ }

pub fn verify<C>(vk: &VerifyingKey, public_inputs: &[Fr], proof: &Proof) -> bool
{ /* ... */ }
```

Step 3 — wire the public-API functions (`generate_membership_proof()`
etc.) to call the fflonk path under the feature flag. The function
*signatures* don't change — only internals. This keeps
`src/ffi.rs` untouched until Phase D.3.

**Tests.**

- All existing `src/prover/tests.rs` tests pass under both features.
- New `bench_prove` in `src/prover/benches/` measuring prover time per
  circuit, per tier; logged for the rollout doc.

**Verification.** End-to-end round trip — generate witness → prove with
fflonk → verify with fflonk — for all 3 circuits × 3 tiers. Existing
`cargo test --features groth16` still passes (Groth16 unchanged).

**Dependencies.** B.3.

### B.5 — Cross-platform test vectors

**Files.**

- `/Users/programyzer/Developer/stellar-mls/docs/cross-platform-test-vectors.json` (existing — extend, don't replace)
- `/Users/programyzer/Developer/stellar-mls/src/prover/tests/test_vectors.rs`

**Changes.**

The existing test-vectors file pins (witness, public-inputs, expected
proof bytes) for cross-platform verification. Extend with a parallel
`fflonk` block:

```json
{
  "version": 2,
  "groth16": { /* existing */ },
  "fflonk": {
    "membership_small": {
      "witness": "...",
      "public_inputs": ["...", "..."],
      "proof_hex": "01<900-byte hex>...",
      "vk_sha256": "..."
    },
    /* ... 6 more for membership × 3 tiers + update × 3 tiers + democracy */
  }
}
```

Vector generation is deterministic: PLONK is non-deterministic in the
prover by default (Fiat-Shamir with random transcript blinding), so the
test vector pins the *verification* side, not the byte-equality of
proofs. Each vector's expected output is `verify(vk, public_inputs,
proof) == true` plus `verify(vk, tampered_public_inputs, proof) ==
false`.

**Tests.** A parameterised `cargo test cross_platform_vector` that
loads each entry and runs the verify pair.

**Verification.** All 9 fflonk entries verify correctly; tampered
inputs reject.

**Dependencies.** B.4.

### B.6 — Final build-time SRS hash assertion (gate)

**Files.**

- `/Users/programyzer/Developer/stellar-mls/build.rs`
- `/Users/programyzer/Developer/stellar-mls/src/prover/srs/expected-hash.in`

**Changes.**

After B.2 lands the build-script skeleton, this task tightens it:

- `expected-hash.in` is committed and reviewed.
- `build.rs` panics if the file is missing or malformed.
- A CI job (`.github/workflows/ci.yml` or equivalent) runs
  `cargo build --features fflonk` on a clean checkout to verify the
  SRS bundle is reachable and the hash check fires.

**Tests.** CI workflow green.

**Verification.** Touching `src/prover/srs/ef-kzg-2023.bin` byte
out-of-band fails the build with a clear error (manually verified).

**Dependencies.** B.2, B.4.

---

## Phase C — Soroban verifier rewrite

**Goal.** Each per-type contract gains a fflonk verifier path; per-tier
VK storage is dropped; `update_vk` admin entrypoint removed; gas measured;
`sep-xxxx` dropped from the workspace.

**Effort.** 2 weeks engineering, parallel after C.1.

### Status: ☐

### C.1 — Define `PlonkProof` type in shared SDK

**Files.**

- **(new)** `/Users/programyzer/Developer/stellar-mls/contracts/sdk/src/plonk_proof.rs` (or wherever the existing `Groth16Proof` lives at the workspace level)
- `/Users/programyzer/Developer/stellar-mls/contracts/sep-anarchy/src/lib.rs:213-234`
  (existing `VerificationKeyData` + `Groth16Proof` — kept until C.2 lands)

**Changes.**

```rust
/// fflonk proof. Wire format ≈900 bytes uncompressed, ≈700 bytes
/// compressed. Version-prefixed.
///
/// Layout (big-endian, uncompressed):
///   [0]      = 0x01  (version)
///   [1..N]   = N commitments, each 96-byte uncompressed G1
///   [N..M]   = M field-element evaluations, each 32 bytes
///   [M..]    = aggregated opening proof (1× G1, 96 bytes)
///
/// Exact layout pinned by jellyfish's serialize, captured here for
/// contract use.
#[contracttype]
#[derive(Clone, Debug)]
pub struct PlonkProof {
    pub version: u32,
    pub commitments: Vec<BytesN<96>>,
    pub evaluations: Vec<BytesN<32>>,
    pub opening: BytesN<96>,
}

#[cfg(test)]
mod tests {
    /// Pinned layout test: a known-good fflonk proof from
    /// docs/cross-platform-test-vectors.json round-trips through
    /// PlonkProof::deserialize without loss.
    #[test]
    fn pin_proof_layout() { ... }
}
```

The exact `commitments.len()` and `evaluations.len()` counts depend on
the fflonk variant chosen in B.4 — pinned at C.1 once jellyfish version is
locked.

**Tests.** Layout test against a real fflonk proof.

**Verification.** Test green.

**Dependencies.** B.4.

### C.2 — Per-contract verifier rewrite

For each contract listed below, follow the same pattern. Status tracked
per contract.

#### C.2.a — `sep-anarchy` (status: ☐)

**Files.**

- `/Users/programyzer/Developer/stellar-mls/contracts/sep-anarchy/src/lib.rs` (975 LOC)
  - Replace lines 213–234 (`VerificationKeyData`, `Groth16Proof`)
  - Replace lines 217–225, 240–260 (storage keys)
  - Replace lines 425–505 (`verify_membership_proof` body)
  - Drop the constructor's 6 VK arguments (lines 275–294)
  - Drop `update_vk()` entrypoint (search for `pub fn update_vk`)
- **(new)** `/Users/programyzer/Developer/stellar-mls/contracts/sep-anarchy/src/vk/membership.rs`
- **(new)** `/Users/programyzer/Developer/stellar-mls/contracts/sep-anarchy/src/vk/update.rs`

**Changes.**

Step 1 — bake VKs as `static` constants:

```rust
// contracts/sep-anarchy/src/vk/membership.rs
//! Membership-circuit verifier key, baked at build time from the
//! universal EF KZG SRS via deterministic preprocessing.
//!
//! See scripts/build/bake-vk.sh — invoked as a build step in
//! contracts/sep-anarchy/build.rs.

pub const VK_BYTES: &[u8] = include_bytes!("membership.vk.bin");
```

Step 2 — `contracts/sep-anarchy/build.rs`:

```rust
fn main() {
    println!("cargo:rerun-if-changed=src/circuit-membership.rs");
    println!("cargo:rerun-if-changed=../../src/prover/srs/ef-kzg-2023.bin");
    let status = std::process::Command::new("cargo")
        .args(&["run", "-p", "sep-mls", "--bin", "bake-vk", "--",
                "--circuit", "membership", "--out", "src/vk/membership.vk.bin"])
        .status()
        .unwrap();
    if !status.success() { panic!("bake-vk failed"); }
}
```

Step 3 — replace `verify_membership_proof()`:

```rust
fn verify_membership_proof(
    env: &Env,
    proof: &PlonkProof,
    public_inputs: &[BytesN<32>],
    tier: u32,
) -> Result<(), Error> {
    use crate::vk::membership::VK_BYTES;

    // 1. Deserialize embedded VK
    let vk = deserialize_vk(VK_BYTES)?;

    // 2. Reconstruct Fiat-Shamir challenges
    let challenges = compute_challenges(env, proof, public_inputs);

    // 3. Build linearization commitment
    let lin = linearize(env, &vk, proof, &challenges)?;

    // 4. Final pairing check on 2 pairs (fflonk aggregation)
    let lhs = env.crypto().bls12_381().g1_msm(/* ~20 scalars */);
    let ok = env.crypto().bls12_381().pairing_check(&[
        (lhs, vk.g2_x),
        (proof.opening.into(), vk.g2_y),
    ]);

    if !ok { return Err(Error::InvalidProof); }
    Ok(())
}
```

Step 4 — drop `DataKey::VK(_)`, `DataKey::UpdateVK(_)`, `update_vk`,
`__constructor` VK args. Update the constructor signature:

```rust
pub fn __constructor(env: Env, admin: Address) -> Result<(), Error> {
    env.storage().instance().set(&DataKey::Admin, &admin);
    env.storage().instance().set(&DataKey::RestrictedMode, &false);
    Ok(())
}
```

**Tests.**

- `tests/membership_proof.rs` — use a real fflonk proof from the test
  vectors; assert `verify_membership_proof` returns `Ok(())`.
- Negative test: tampered public inputs ⇒ `InvalidProof`.
- Constructor test: instantiate with just admin; no VK args.

**Verification.** Soroban-vm test green for membership + update +
deactivate paths. WASM build size ≤ pre-migration size + 50 KB
(VK bytecode addition).

**Dependencies.** C.1, B.4.

#### C.2.b — `sep-democracy` (status: ☐)

Same pattern as C.2.a applied to:
- `/Users/programyzer/Developer/stellar-mls/contracts/sep-democracy/src/lib.rs` (993 LOC)
- Lines 16–18, 34, 221–238, 437–448 (Groth16 references)

In addition: democracy has a quorum-binding circuit; bake an extra VK
at `contracts/sep-democracy/src/vk/democracy_update.vk.bin`.

#### C.2.c — `sep-oligarchy` (status: ☐)

Same pattern applied to:
- `/Users/programyzer/Developer/stellar-mls/contracts/sep-oligarchy/src/lib.rs` (1,257 LOC)
- Lines 17–35, 46, 283–300, plus the 8 verification call sites.

Multi-VK contract: membership + admin-update + member-update.

#### C.2.d — `sep-oneonone` (status: ☐)

Same pattern applied to:
- `/Users/programyzer/Developer/stellar-mls/contracts/sep-oneonone/src/lib.rs` (595 LOC)
- Lines 17, 27, 144–161, 370–374.

Smaller surface; single VK.

#### C.2.e — `sep-tyranny` (status: ☐)

Same pattern applied to:
- `/Users/programyzer/Developer/stellar-mls/contracts/sep-tyranny/src/lib.rs` (1,063 LOC)
- Lines 6, 39, 194–207, plus 12 verification call sites.

Multi-VK: membership + admin-update.

### C.3 — Drop per-tier VK storage

**Files.** Each of the 5 gov contracts (already touched in C.2).

**Changes.**

- Delete `DataKey::VK(u32)` and `DataKey::UpdateVK(u32)` enum variants
  per contract.
- Migration shim: contracts deployed *before* C.2 ship will have stale
  VK rows in persistent storage. Soroban TTL eventually evicts them; we
  do not need to actively delete. New deployments don't write them.

**Tests.** Per-contract: instantiate, attempt `update_vk` ⇒ compile
error (the entrypoint is gone). Existing testnet groups continue to
work (they use the legacy `sep-xxxx`).

**Verification.** `grep -r "DataKey::VK\|DataKey::UpdateVK\|update_vk" contracts/sep-{anarchy,democracy,oligarchy,oneonone,tyranny}/`
returns zero matches.

**Dependencies.** C.2.{a–e}.

### C.4 — Drop `update_vk` admin entrypoint

**Files.** Each of the 5 gov contracts (subsumed by C.2 + C.3).

**Changes.** Remove `pub fn update_vk(...)` from each contract impl
block. Remove the `VkKind` enum.

**Tests.** Compile-time gone.

**Verification.** `grep -r "update_vk" contracts/sep-{anarchy,democracy,oligarchy,oneonone,tyranny}/`
returns zero matches.

**Dependencies.** C.3.

### C.5 — Soroban verify gas benchmark

**Files.**

- **(new)** `/Users/programyzer/Developer/stellar-mls/contracts/sep-anarchy/tests/gas_benchmark.rs`
- Same per gov contract.

**Changes.**

Per-contract benchmark using `env.budget().reset_default()`:

```rust
#[test]
fn bench_verify_membership() {
    let env = Env::default();
    env.budget().reset_default();
    let contract = setup_with_admin(&env);

    let proof = test_vectors::FFLONK_MEMBERSHIP_SMALL.proof();
    let inputs = test_vectors::FFLONK_MEMBERSHIP_SMALL.public_inputs();
    let _ = contract.verify_membership(&proof, &inputs, /* tier */ 0);

    let used = env.budget().cpu_instructions();
    println!("verify_membership_small: {} CPU instructions", used);
    // Pre-migration baseline for sep-anarchy: NNNN
    assert!(used <= 2 * BASELINE_CPU, "verify gas regressed");
}
```

Pre-migration baselines are captured in a separate PR before C.2 lands.

**Tests.** All 5 gov contracts × 3 tiers (membership) + applicable
update circuits ≤ 2× pre-migration baseline.

**Verification.** CI emits a gas-budget table; any regression > 2×
fails the PR.

**Dependencies.** C.2.{a–e}.

### C.6 — Drop `sep-xxxx` from the workspace

**Files.**

- `/Users/programyzer/Developer/stellar-mls/Cargo.toml` (workspace `members` array)
- `/Users/programyzer/Developer/stellar-mls/contracts/sep-xxxx/` — directory remains in git history but is removed from workspace builds.

**Changes.** Remove `"contracts/sep-xxxx"` from the `members` list. The
crate still exists on disk; running `cargo build -p sep-xxxx` directly
still works for legacy testnet redeployments. Phase F deletes it.

**Tests.** `cargo build --workspace` green; sep-xxxx is not in the
build graph.

**Verification.** `cargo metadata --format-version=1 | jq -r '.packages[].name' | grep sep-xxxx`
returns empty.

**Dependencies.** C.2.{a–e}.

---

## Phase D — Mobile rebundle

**Goal.** Replace the 12-file `keyset-v2/` bundles with the universal SRS
+ per-circuit selectors. Update FFI surface, Swift bridge, Kotlin bridge.
On-device tests prove a fresh end-to-end flow.

**Effort.** 1 week engineering, parallel with C after B.4.

### Status: ☐

### D.1 — iOS resources

**Files.**

- Drop: `/Users/programyzer/Developer/stellar-mls/clients/ios/StellarChat/StellarChat/Resources/keyset-v2/` (12 files, ~5 MB)
- Add: `/Users/programyzer/Developer/stellar-mls/clients/ios/StellarChat/StellarChat/Resources/srs.bin` (≈206 KB)
- Add: `/Users/programyzer/Developer/stellar-mls/clients/ios/StellarChat/StellarChat/Resources/circuits/membership-{small,medium,large}.pk.bin`
- Add: `/Users/programyzer/Developer/stellar-mls/clients/ios/StellarChat/StellarChat/Resources/circuits/update-{small,medium,large}.pk.bin`
- Add: `/Users/programyzer/Developer/stellar-mls/clients/ios/StellarChat/StellarChat/Resources/circuits/democracy.pk.bin`

**Changes.**

- The `srs.bin` is identical across iOS, Android, and the server prover —
  same bytes, same SHA-256.
- Per-circuit `pk.bin` is the preprocessed prover key (a few hundred KB
  per tier). VKs are *not* bundled; they live in the contract.
- `clients/ios/StellarChat/StellarChat/StellarChat.xcodeproj` — update
  resource bundle phase to include the new files. Generated by
  `xcodegen` from `project.yml` per the project memory; edit `project.yml`,
  not the pbxproj.

**Tests.** App build succeeds; resources present in the .ipa under
the expected paths (verify via `unzip -l app.ipa`).

**Verification.** App size delta logged; expected ≤0 MB net (gain back
per-tier pk overhead, pay for SRS once).

**Dependencies.** B.4.

### D.2 — Android assets

Same as D.1 for `/Users/programyzer/Developer/stellar-mls/clients/android/StellarChat/app/src/main/assets/`.

The Android `assets/` directory is read-only at runtime; pk and SRS
files load via `AssetManager.open()`.

### D.3 — FFI surface trim

**Files.**

- `/Users/programyzer/Developer/stellar-mls/src/ffi.rs` (1,591 LOC)
- `/Users/programyzer/Developer/stellar-mls/src/jni_ffi.rs` (519 LOC)
- `/Users/programyzer/Developer/stellar-mls/swift-mls/Sources/CSEPMLSFFI/include/sep_ffi.h`

**Changes.**

Drop the per-circuit-setup family entirely:

- `src/ffi.rs:405` — `sep_generate_testing_proving_key`
- `src/ffi.rs:488` — `sep_generate_testing_update_proving_key`
- `src/ffi.rs:517` — `sep_generate_testing_democracy_update_proving_key`
- The corresponding JNI wrappers in `src/jni_ffi.rs:342-505`.
- The C declarations in `sep_ffi.h`.

These existed because Groth16 needed per-circuit setup; fflonk preprocessing
is a deterministic build-time step using the universal SRS.

**Keep, but rewire internals to fflonk:**

- `sep_generate_membership_proof`
- `sep_generate_update_proof`
- `sep_generate_democracy_update_proof`
- `sep_proof_to_contract_format` — change output from 384 B Groth16 to
  ~900 B fflonk wire format. **Bump the FFI ABI version** so old clients
  fail loudly rather than silently producing malformed proofs.

**Tests.** Both FFI and JNI surfaces compile, link, and pass their existing
test harness against the new internal implementations.

**Verification.** `nm libsepmls.dylib | grep sep_generate_testing` returns
empty. `nm libsepmls.dylib | grep sep_generate_.*_proof` returns the three
real proof functions.

**Dependencies.** B.4.

### D.4 — Swift bridge

**Files.**

- `/Users/programyzer/Developer/stellar-mls/swift-mls/Sources/SwiftMLS/ProofGenerator.swift` (74 LOC)
- `/Users/programyzer/Developer/stellar-mls/swift-mls/Sources/SwiftMLS/RustBridge.swift` (422 LOC)

**Changes.**

- Remove `generateTestingProvingKey()`, `generateTestingUpdateProvingKey()`,
  etc. — the Swift wrappers around the dropped FFI surface.
- Update `generateMembershipProof()` etc. to load preprocessed pk from
  bundle: `Bundle.main.url(forResource: "membership-small", withExtension: "pk.bin", subdirectory: "circuits")`.
- Update doc comments — drop "Convert a compressed Groth16 proof (192 bytes)"
  language; use "fflonk proof (~900 bytes)" instead.

**Tests.** Swift unit tests green; XCTest exercising end-to-end proof
generation passes.

**Verification.** `xcodebuild test -derivedDataPath /tmp/swiftmls-test ...` green.

**Dependencies.** D.3.

### D.5 — Kotlin bridge

**Files.**

- `/Users/programyzer/Developer/stellar-mls/kotlin-mls/src/main/java/com/stellarmls/mls/SEPProofGenerator.kt` (143 LOC)
- `/Users/programyzer/Developer/stellar-mls/kotlin-mls/src/main/java/com/stellarmls/mls/RustBridge.kt` (87 LOC)

**Changes.**

Same shape as D.4 applied to Kotlin/JNI. Load pk via `assets.open(...)`.

**Tests.** Kotlin unit tests green via `./gradlew :kotlin-mls:test`. Per
project memory, do **not** run Android Studio; CLI builds only.

**Verification.** Test report green.

**Dependencies.** D.3.

### D.6 — Cross-platform test vectors regenerate

**Files.**

- `/Users/programyzer/Developer/stellar-mls/docs/cross-platform-test-vectors.json` (extended in B.5)

**Changes.**

After D.4 + D.5 land, regenerate vectors *from the mobile prover paths*
to verify byte-exact agreement between Rust, Swift, and Kotlin
implementations:

```bash
cargo run --bin gen-test-vectors --features fflonk --release \
  > docs/cross-platform-test-vectors.json
```

The Swift and Kotlin sides verify:

```
swift run swiftmls-verify-vectors docs/cross-platform-test-vectors.json
./gradlew :kotlin-mls:run --args="verify-vectors docs/cross-platform-test-vectors.json"
```

Both must report all 9 fflonk entries verifying.

**Verification.** Three-platform agreement.

**Dependencies.** D.4, D.5.

### D.7 — On-device tests

**Files.** None (operational test).

**Changes.**

- iOS: real-iPhone proof on A-series and M-series. Measure prover
  latency + memory. Submit to Soroban testnet against the new contract;
  verify acceptance.
- Android: same on a low-end (Pixel 4a class) and mid-tier (Pixel 7
  class) device.

Log results in [`mainnet-deployment.md`](mainnet-deployment.md) under
a new "Mobile prover envelope (fflonk)" §.

**Verification.** All 4 device-classes succeed. Prover memory ≤ 256 MB
on iOS, ≤ 128 MB on Android low-end (per design §3.4).

**Dependencies.** D.4, D.5, C.2 (testnet contract deployment).

---

## Phase E — Coordinator decommission

**Goal.** Delete the entire ceremony surface. The migration's
operational simplification is realised here.

**Effort.** 1 week engineering, after Phases B + C + D ship.

### Status: ☐

### E.1 — Delete `ceremony-coordinator/`

**Files.**

- `/Users/programyzer/Developer/stellar-mls/ceremony-coordinator/` — entire directory (3 kLOC HTTP service)
- `/Users/programyzer/Developer/stellar-mls/Cargo.toml` — remove from workspace `members` array

**Changes.** `git rm -r ceremony-coordinator/`. Drop the `ceremony-coordinator`
service from `docker-compose.yml`. Note: the `nostr-relay` (strfry) service
**stays** — `relayer` and `pn-relay` both use it.

**Tests.** `cargo build --workspace` green; `docker compose config` valid.

**Verification.** `ls ceremony-coordinator 2>&1` ⇒ "No such file."

**Dependencies.** Phases C + D shipped; no active ceremony round in
flight.

### E.2 — Delete client tooling

**Files.**

- `/Users/programyzer/Developer/stellar-mls/tools/ceremony/` — participant CLI
- `/Users/programyzer/Developer/stellar-mls/crates/ceremony-wasm/` — browser verifier
- `/Users/programyzer/Developer/stellar-mls/deploy/ceremony/` — frontend
- `/Users/programyzer/Developer/stellar-mls/Cargo.toml` — remove `crates/ceremony-wasm` from workspace

**Changes.** `git rm -r` on each. Remove the nginx mount for
`/var/www/ceremony` from `deploy/nginx/conf.d/ceremony.onym.chat.conf` and
delete that conf file.

**Tests.** `cargo build --workspace` green.

**Verification.** `ls tools/ceremony crates/ceremony-wasm deploy/ceremony 2>&1` ⇒ all "No such file."

**Dependencies.** E.1.

### E.3 — Delete operational scripts and src/ceremony

**Files.**

- `/Users/programyzer/Developer/stellar-mls/scripts/install-membership-vks-testnet.sh` (152 LOC)
- `/Users/programyzer/Developer/stellar-mls/scripts/install-democracy-vks-testnet.sh` (182 LOC)
- `/Users/programyzer/Developer/stellar-mls/scripts/install-adminupdate-vk-testnet.sh` (177 LOC)
- `/Users/programyzer/Developer/stellar-mls/scripts/generate-keyset.sh`
- `/Users/programyzer/Developer/stellar-mls/scripts/generate-democracy-vk-dev.sh`
- `/Users/programyzer/Developer/stellar-mls/scripts/verify-ceremony-tool.sh`
- `/Users/programyzer/Developer/stellar-mls/scripts/update-ceremony-downloads.py`
- `/Users/programyzer/Developer/stellar-mls/src/ceremony/` (entire dir, 1.4 kLOC)

**Changes.** `git rm` each. The contracts deploy without VK args (Phase
C.2 already changed the constructor); install-*-vks scripts are no longer
called by anything.

**Tests.** `grep -r "install-membership-vks\|install-democracy-vks\|install-adminupdate-vk" .`
returns zero matches in deploy + CI scripts.

**Verification.** `ls scripts/install-*-vks-*` ⇒ "No such file."

**Dependencies.** E.2.

### E.4 — `docker-compose.yml` cleanup

**Files.**

- `/Users/programyzer/Developer/stellar-mls/docker-compose.yml`
- `/Users/programyzer/Developer/stellar-mls/deploy/digitalocean/deploy.sh`
- `/Users/programyzer/Developer/stellar-mls/deploy/certbot/init-certs.sh`

**Changes.**

Remove the `ceremony-coordinator` service block. Update Cloudflare DNS
records and certbot domains to drop `ceremony.onym.chat`:

```diff
- DOMAINS=("$DOMAIN" "relay.$DOMAIN" "nostr.$DOMAIN" "blossom.$DOMAIN" "push.$DOMAIN" "ceremony.$DOMAIN")
+ DOMAINS=("$DOMAIN" "relay.$DOMAIN" "nostr.$DOMAIN" "blossom.$DOMAIN" "push.$DOMAIN")
```

The `blossom` service stays — the Blossom retention rules from
PR #161 continue to govern chat-app uploads. Coordinator-pinned rule
becomes effectively dead since the coordinator pubkey no longer uploads.
Drop the coordinator-pinned rule from `deploy/blossom/config.yml` for
hygiene; chat retention rules are unchanged.

**Tests.** `docker compose config --quiet` passes. Live droplet smoke
test post-deploy: chat works, push works, nostr-relay works.

**Verification.** `docker ps` on the live droplet shows no
`onym-ceremony-coordinator-1`.

**Dependencies.** E.1, E.2.

### E.5 — DNS + static replacement page

**Files.**

- `/Users/programyzer/Developer/stellar-mls/deploy/website/ceremony-decommissioned.html` (new, optional)

**Changes.**

Optionally publish a static page at `/ceremony` (under the main domain)
linking to the postmortem and design doc. Cloudflare Workers or a simple
nginx redirect from the old `ceremony.onym.chat` to the static URL is
fine. After 30 days, drop the redirect entirely.

**Verification.** Old URL either 410 Gone or redirects to the static
page.

**Dependencies.** E.4.

### E.6 — Archive ceremony backup to cold storage

**Files.** None.

**Changes.**

- Move `/Users/programyzer/Developer/stellar-mls/ceremony-backup/20260429T163349Z/`
  to long-term storage (S3 Glacier, encrypted disk, or similar).
- Replace with a `ceremony-backup/README.md` pointing to the archive
  location.

**Verification.** Archive accessible by the operator; local working
copy reduced to a README.

**Dependencies.** A.3 (postmortem closed).

---

## Phase F — `sep-xxxx` decommission and ledger close

**Goal.** Final removal of the legacy contract; mark the postmortem
complete.

**Effort.** 1 week engineering, last.

### Status: ☐

### F.1 — Migrate testnet groups (if any)

**Files.** None.

**Changes.**

- Audit testnet for groups still pinned to `sep-xxxx` deployment. If any
  are owned by team members or known testers, ask them to recreate against
  the per-type contracts. Production users do not exist by §3.6.
- For groups that won't migrate, accept that they continue to function on
  the legacy contract (which still verifies Groth16 proofs against
  `keyset-v2`) until they age out.

**Verification.** No outstanding testnet group migrations blocking
the decommission.

**Dependencies.** Phases C, D, E shipped.

### F.2 — Drop `sep-xxxx` source

**Files.**

- `/Users/programyzer/Developer/stellar-mls/contracts/sep-xxxx/` — entire crate
- `/Users/programyzer/Developer/stellar-mls/scripts/deploy_sep_xxxx_testnet.sh`
- `/Users/programyzer/Developer/stellar-mls/scripts/deploy-pr84-testnet.sh` if it references sep-xxxx

**Changes.** `git rm -r contracts/sep-xxxx`. The crate already left the
workspace in C.6; this removes the source entirely. Git history retains it.

**Tests.** `cargo build --workspace` green; `grep -r "sep-xxxx" .` returns
only postmortem and design-doc context (acceptable historical references).

**Verification.** `ls contracts/sep-xxxx 2>&1` ⇒ "No such file."

**Dependencies.** F.1.

### F.3 — Update parent design doc

**Files.**

- `/Users/programyzer/Developer/stellar-mls/docs/group-governance-types-design.md`

**Changes.** Drop the "implemented in sep-xxxx" hedges; update language
to reference the per-type contracts as authoritative. Add a §"History"
note: "v0–v1 of this design lived in `contracts/sep-xxxx`; that
contract was decommissioned 2026-MM-DD per
[`fflonk-migration-design.md`](fflonk-migration-design.md) and
[`postmortem-ceremony-data-loss.md`](postmortem-ceremony-data-loss.md)."

**Verification.** Doc PR merged.

**Dependencies.** F.2.

### F.4 — Tag release + announce

**Files.** None.

**Changes.**

- Tag the release per project conventions (`gh workflow run Release -f tag=vX.Y.Z`
  per the project memory — release via manual dispatch, not tag push).
- Publish a brief announcement on whatever channel users follow,
  cross-linking the design doc and postmortem.

**Verification.** Release tag visible on GitHub; announcement posted.

**Dependencies.** F.2, F.3.

### F.5 — Close out the postmortem

**Files.**

- `/Users/programyzer/Developer/stellar-mls/docs/postmortem-ceremony-data-loss.md` (header block)

**Changes.**

```diff
- **Status:** Decided 2026-04-29 · prune fix landed in PR #161 ...
+ **Status:** Complete 2026-MM-DD · all phases shipped per implementation-plan-fflonk-migration.md ...
```

**Verification.** PR merged.

**Dependencies.** F.4.

---

## Open questions tracker

These mirror `fflonk-migration-design.md` §10 with implementation-time
resolutions:

| # | Question | Resolution gate | Owner |
|---|---|---|---|
| Q1 | jellyfish vs. arkworks-fflonk vs. vanilla PLONK fallback | Phase B.1 spike, before B.3 begins | TBD |
| Q2 | VK-as-bytecode vs. VK-as-storage | Decided in design doc; revisited if a deploy-without-recompile use-case emerges | TBD |
| Q3 | Phase A outreach window | Two weeks then close (`A.3`) | TBD |
| Q4 | EF KZG vs. Aztec Ignition primary | EF; revisit only if a circuit needs n>2048 | TBD |
| Q5 | Browser verifier replacement | None — `crates/ceremony-wasm` deleted in E.2 | n/a |

Each open question gets resolved (or escalated) by the gate listed. None
of them block Phase A.

---

## Risks tracker

Mirrors `fflonk-migration-design.md` §9; column "trigger phase" indicates
where the risk surfaces in this plan.

| Risk | Trigger phase | Mitigation |
|---|---|---|
| Rust fflonk implementation maturity | B.1 | Spike before B.3; vanilla PLONK fallback |
| EF KZG SRS coverage edge case | B.3 | Switch SRS source to Aztec Ignition (one-line change) |
| Soroban verify gas exceeds 2× budget | C.5 | Optimise linearization; precompute more constants in contract |
| Mobile prover memory regression | D.7 | Plookup table reuse; sub-circuit split |
| Two proving systems live concurrently | E.1 (delayed) | Wire-format-distinguishable proof types; transition window if needed |
| Audit gap on PLONK gadgets | B.3 | Use jellyfish audited gadgets; per-circuit review; cross-platform vectors |

---

## Test plan summary

By phase, the canonical test that proves "phase done":

| Phase | Canonical test |
|---|---|
| A | Recovered sidecars return 200 from `https://blossom.onym.chat/<sha256>`; postmortem updated |
| B | `cargo test --features fflonk --workspace` green; SRS hash assertion fires on tamper; cross-platform vectors verify |
| C | Per-contract Soroban-vm test: real fflonk proof verifies; tampered inputs reject; gas ≤ 2× baseline |
| D | iOS + Android on-device round trip: prove → submit to testnet → verify accepts |
| E | `docker compose up -d` on the live droplet succeeds without `ceremony-coordinator`; chat + push + nostr unaffected |
| F | `cargo build --workspace` green with no `sep-xxxx`; postmortem header reads "Complete" |

CI gates (proposed addition to `.github/workflows/ci.yml`):

- `cargo build --features fflonk` (after B.1)
- `cargo test --features fflonk --workspace` (after B.4)
- Per-contract gas-budget bench (after C.5) — fail PR on > 2× baseline
- iOS + Android FFI link checks (after D.3)
- SRS-hash assertion regression test (after B.6)

---

## Branch and PR conventions

Per the project memory ("Substantive changes via PR — behavioral/cross-client changes go through a PR"):

- Each numbered task lands on a topic branch named `fflonk/<phase-letter>-<task-id>` (e.g. `fflonk/b3-circuit-port`, `fflonk/c2a-sep-anarchy-verifier`).
- Each PR rolls up one bullet from the "Sub-PR partitioning" table near the top of this document.
- PR descriptions cross-link the relevant phase header in this plan.
- Phase E and F can roll up their tasks into single PRs each (operational, scoped to deletions).
- Releases follow `gh workflow run Release -f tag=vX.Y.Z` per project memory; do not push tags.

---

## References

- [`fflonk-migration-design.md`](fflonk-migration-design.md) — the design this plan executes
- [`postmortem-ceremony-data-loss.md`](postmortem-ceremony-data-loss.md) — the incident motivating it
- [`implementation-plan-update-circuit-binding.md`](implementation-plan-update-circuit-binding.md) — format reference; this plan mirrors its structure
- [`group-governance-types-design.md`](group-governance-types-design.md) — parent design defining the per-type contracts that gain the new verifier
- [Ethereum Foundation KZG ceremony](https://ceremony.ethereum.org/) — SRS source
- [Espresso jellyfish](https://github.com/EspressoSystems/jellyfish) — preferred PLONK implementation
- [`docs/cross-platform-test-vectors.json`](cross-platform-test-vectors.json) — to be extended in B.5
- PR #161 — `deploy/blossom/config.yml` hot-fix
- PR #162 — postmortem + design doc landing
