# Testnet Deployment Guide

This repository includes a repeatable script for deploying the SEP-XXXX Soroban contract to Stellar testnet and running a basic end-to-end integration flow against the live deployed instance.

## What the script does

The script:

1. builds the Soroban contract WASM
2. generates deterministic test-only Groth16 verification keys and proofs
3. creates a fresh Stellar identity in an isolated CLI config directory
4. funds that identity on testnet through the CLI's Friendbot-backed funding flow
5. deploys the contract
6. initializes it with verification keys for all three tiers
7. executes and checks:
   - `create_group` with `public_inputs` for epoch 0
   - `verify_membership` at epoch 0 with matching `public_inputs`
   - `update_commitment` with `new_epoch=1` and current-state `public_inputs`
   - `get_state` (verifies epoch 1, new commitment, active)
   - `verify_membership` at epoch 1 with matching `public_inputs`
   - `deactivate_group` with current-state `public_inputs`
   - `get_history` with `max_entries=64`

## Important limitation

The generated verification keys are **test-only**. They come from local single-party Groth16 setup, not from a production trusted setup ceremony. This is correct for testnet deployment and integration validation, but **not** for mainnet.

## Usage

```bash
./scripts/deploy_sep_xxxx_testnet.sh
```

Optional environment variables:

- `NETWORK` — defaults to `testnet`
- `IDENTITY` — local Stellar CLI identity name
- `ALIAS` — contract alias saved in the temporary CLI config
- `CONFIG_DIR` — custom Stellar CLI config directory
- `WORK_DIR` — custom temporary artifact directory
- `KEEP_ARTIFACTS=1` — keep generated fixtures and CLI config after the script exits

## Generated fixtures

The script relies on:

```bash
cargo run --bin generate_contract_testnet_fixtures -- --out-dir <dir>
```

That helper writes:

- `vk-small.json`, `vk-medium.json`, `vk-large.json` — verification keys (uncompressed BLS12-381 points)
- `proof-epoch-0.json`, `proof-epoch-1.json` — Groth16 proofs (uncompressed `{a, b, c}` components)
- `public-inputs-epoch-0.json`, `public-inputs-epoch-1.json` — `{commitment, epoch}` for each epoch
- `group-id.hex`, `commitment-epoch-0.hex`, `commitment-epoch-1.hex` — hex-encoded 32-byte values
- `tier.txt` — circuit tier (0 = Small)
- `summary.json` — metadata index

These files are formatted specifically for `stellar contract invoke`, using:

- JSON objects for Soroban structs
- JSON arrays for Soroban vectors
- hex strings for `BytesN` fields
