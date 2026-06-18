# C1 Row Benchmark Methodology

This document specifies how the first-party numbers in the SoK's
C1 row (Stellar + BLS12-381 + Groth16) are measured. It is the contract that
`run.sh` must satisfy. Re-running `run.sh` regenerates `results.json`,
which the LaTeX tables `\input{}` from.

## What is measured

| Field | Definition | Where |
|---|---|---|
| `prover_time_ms_p50` | Median wall-clock of `prover::prove(...)` over 10 runs, single configuration size | `src/prover/` (Update relation, medium ring size) |
| `prover_time_ms_p95` | 95th-percentile wall-clock over the same 10 runs | same |
| `proof_size_bytes` | Length of the serialized Groth16 proof (3 G1 + 1 G2 in compressed form) | `Proof::serialize_compressed` |
| `proving_key_size_bytes` | Length of the serialized proving key for the medium ring size | `ProvingKey::serialize_compressed` |
| `verifying_key_size_bytes` | Length of the serialized verifying key | `VerifyingKey::serialize_compressed` |
| `verifier_gas_stroops` | Soroban `fee_charged` for a `submit_update` invocation on testnet, median of 5 invocations | Stellar testnet RPC |
| `verifier_usd_at_2026_04` | `verifier_gas_stroops` × 1e-7 × XLM/USD spot at the snapshot timestamp | spot rate captured at run time |

## Configuration measured

The "medium ring size" referenced above is the canonical size from the
SEP draft: 64 members. Small (16) and large (256) are measured as
secondary rows in `results.json` but the primary table cell uses
medium. The relation is the *Update* relation
(R\_{Mem}+R\_{Upd}-style); the membership-only relation is faster but
not the load-bearing operation.

## What is *excluded* from `prover_time_ms`

- Trusted setup time. The Phase-2 ceremony runs once per circuit
  size; it is reported separately as `setup_time_minutes_amortized`,
  which is the ceremony wall-clock divided by the expected per-circuit
  proof count (an editorial estimate, flagged in the table).
- Proving key load time from disk. The harness pre-loads the proving
  key; only the `prove` call is timed.
- Witness generation time spent outside the SNARK (commitment hashing
  in SHA-256, off-chain Ed25519 attestation building). These are
  reported in a footnote, not in the headline number, because they
  are deployment-specific and not properties of the SNARK.

## Hardware pinning

The harness records `uname -m -r -s`, `sysctl -n machdep.cpu.brand_string`
(macOS) or `lscpu | grep "Model name"` (Linux), `sysctl -n hw.memsize` /
`/proc/meminfo`, and Rust toolchain version. These land in
`results.json` under the `host` field.

The numbers in the SoK's Section 4 tables are measured on the
**reference workstation**: a single defined machine documented in the
final paper's caption. Numbers from any other host are clearly
labeled. Re-running the harness on a different machine still produces
a valid `results.json`; the SoK simply cites whichever measurement is
flagged as `reference: true`.

## Software pinning

- Rust toolchain: pinned by `rust-toolchain.toml` at the repository
  root. The harness asserts this matches and aborts on mismatch.
- Crate versions: `Cargo.lock` is the authoritative pin. The harness
  records the resolved versions of `ark-groth16`, `ark-bls12-381`,
  `ark-ff`, `ark-ec`, and `ark-serialize` in `results.json`.
- Build profile: `--release` with default optimization level. The
  harness asserts on `cargo build --release` exit code.

## Measurement protocol

```
1. cargo build --release -p sep-xxxx-circuits --bin generate_contract_testnet_fixtures
2. Pre-warm: run prover once, discard timing.
3. For i in 1..10:
     start = monotonic_now()
     proof = prove(...)
     end   = monotonic_now()
     record (end - start)
4. p50, p95 across the ten samples.
5. Write `results.json`.
```

The `verifier_gas_stroops` field requires testnet access. The harness
attempts a testnet measurement when `STELLAR_RPC_URL` is set; otherwise
it sets the field to `null` and the LaTeX renders "—" with a footnote
that on-chain gas requires re-running with testnet credentials.

## `results.json` schema

```json
{
  "schema_version": "1",
  "snapshot_iso8601": "...",
  "git_rev": "...",
  "host": {
    "kernel": "...",
    "cpu": "...",
    "memory_bytes": 0,
    "rust": "..."
  },
  "crates": {
    "ark-groth16": "0.4.x",
    "ark-bls12-381": "0.4.x"
  },
  "ring_sizes": {
    "small": 16,
    "medium": 64,
    "large": 256
  },
  "primary_size": "medium",
  "samples": {
    "small":  { "prover_time_ms": [/* 10 numbers */] },
    "medium": { "prover_time_ms": [/* 10 numbers */] },
    "large":  { "prover_time_ms": [/* 10 numbers */] }
  },
  "summary": {
    "primary": {
      "prover_time_ms_p50": 0,
      "prover_time_ms_p95": 0,
      "proof_size_bytes": 0,
      "proving_key_size_bytes": 0,
      "verifying_key_size_bytes": 0,
      "verifier_gas_stroops": null,
      "verifier_usd_at_2026_04": null
    }
  },
  "reference": false
}
```

LaTeX table cells reference `summary.primary.*` fields by `\input`-ing
small generated `.tex` snippets that the harness writes alongside
`results.json`. This decouples prose from numbers.

## When this document changes

If the measurement protocol changes (different ring size as primary,
different sample count, different relation), bump `schema_version` and
note the change in the SoK's §4 methodology paragraph. Reviewers can
verify a number's provenance by the schema version on its source
record.
