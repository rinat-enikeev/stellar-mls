# Phase 2 MPC Integration Guide

## Why Phase 2 Is Required

Groth16 trusted setup has two phases:

1. **Phase 1 (Powers of Tau):** Generates a universal structured reference string (SRS) with secret scalars (tau, alpha, beta). Security relies on at least one participant honestly destroying their contribution. This module (`src/ceremony/mod.rs`) implements Phase 1.

2. **Phase 2 (Circuit-Specific):** Takes the Phase 1 SRS and the specific R1CS circuit to derive proving and verifying keys. Without Phase 2 MPC, a single machine that runs key derivation holds the "toxic waste" — the combined secret scalars. Anyone with the toxic waste can forge proofs.

**Bottom line:** If you skip Phase 2 MPC, whoever runs `circuit_specific_setup()` can forge membership proofs for any group.

## Current State

- `src/ceremony/mod.rs` — Phase 1 MPC (Powers of Tau), fully implemented and verified
- `src/ceremony/phase2.rs` — SRS export/import adapter for interop with external Phase 2 tools
- `src/prover/mod.rs` (`derive_insecure_test_keys`) — Single-machine key derivation, `#[cfg(test)]` only

## Integration Workflow

### Step 1: Run Phase 1 Ceremony

```rust
use sep_xxxx_circuits::ceremony;

let mut rng = OsRng;
let degree = ceremony::required_degree(&Tier::Small); // or Medium/Large
let transcript = ceremony::run_ceremony(degree, num_contributors, &mut rng);
// transcript.final_srs contains the Phase 1 output
```

### Step 2: Export SRS

```rust
use sep_xxxx_circuits::ceremony::phase2;

let exported = phase2::export_srs(&transcript.final_srs)
    .expect("SRS export failed");
std::fs::write("phase1_srs.bin", &exported.srs_bytes)
    .expect("failed to write SRS file");
```

### Step 3: Run Phase 2 MPC (External)

Use snarkjs or a similar tool to run the Phase 2 ceremony:

```bash
# Convert SRS to .ptau format (conversion script TBD)
# python3 scripts/srs_to_ptau.py phase1_srs.bin phase1.ptau

# Generate R1CS from the circuit
snarkjs r1cs export json circuit.r1cs circuit.r1cs.json

# Phase 2 setup
snarkjs groth16 setup circuit.r1cs phase1.ptau phase2_0000.zkey

# Contributions (each participant runs this)
snarkjs zkey contribute phase2_0000.zkey phase2_0001.zkey --name="Participant 1"
snarkjs zkey contribute phase2_0001.zkey phase2_0002.zkey --name="Participant 2"
# ... more participants ...

# Apply random beacon
snarkjs zkey beacon phase2_final.zkey phase2_beacon.zkey <beacon_hash> 10

# Export verification key
snarkjs zkey export verificationkey phase2_beacon.zkey verification_key.json

# Verify the ceremony
snarkjs zkey verify circuit.r1cs phase1.ptau phase2_beacon.zkey
```

### Step 4: Import Phase 2 Keys

After Phase 2 completes, serialize the proving and verifying keys in arkworks canonical compressed format and import:

```rust
use sep_xxxx_circuits::ceremony::phase2;

let pk_bytes = std::fs::read("proving_key.bin").unwrap();
let vk_bytes = std::fs::read("verifying_key.bin").unwrap();
let keys = phase2::import_phase2_keys(&pk_bytes, &vk_bytes)
    .expect("key import failed");
// keys.proving_key and keys.verifying_key are ready for use
```

## Verification Checklist

After Phase 2 MPC:

1. Verify the Phase 2 transcript (snarkjs does this automatically)
2. Publish all contribution hashes and attestations
3. Verify proof generation works with the new keys:
   - Generate a proof with `keys.proving_key`
   - Verify it with `keys.verifying_key`
   - Verify it against the on-chain verifier
4. Run the proof against known test vectors to confirm correctness

## Security Considerations

- **Minimum participants:** At least 3 independent participants for Phase 2, ideally from different organizations
- **Randomness sources:** Each participant should use hardware RNG or OS-level CSPRNG
- **Toxic waste disposal:** Participants must destroy their contribution secrets after the ceremony. Use ephemeral VMs or secure enclaves.
- **Public verifiability:** All contribution proofs and the final transcript must be published for anyone to verify
- **Random beacon:** Apply a publicly verifiable random beacon (e.g., drand) as the final contribution to prevent last-participant bias

## Export Format Specification

The `export_srs()` function produces bytes in the following layout (all values in arkworks compressed canonical serialization):

| Field | Type | Description |
|-------|------|-------------|
| tau_g1_count | u64 | Number of tau*G1 points |
| tau_g1[0..n] | G1Affine[] | Compressed G1 points |
| tau_g2_count | u64 | Number of tau*G2 points |
| tau_g2[0..n] | G2Affine[] | Compressed G2 points |
| alpha_tau_g1_count | u64 | Number of alpha*tau*G1 points |
| alpha_tau_g1[0..n] | G1Affine[] | Compressed G1 points |
| beta_tau_g1_count | u64 | Number of beta*tau*G1 points |
| beta_tau_g1[0..n] | G1Affine[] | Compressed G1 points |
| beta_g2 | G2Affine | Single compressed G2 point |

All G1 points are 48 bytes compressed. All G2 points are 96 bytes compressed.
