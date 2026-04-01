# stellar-mls

Private group membership on Stellar using zero-knowledge proofs.

- Groth16 circuits over BLS12-381 (Poseidon Merkle commitments)
- Soroban smart contract for on-chain group state and proof verification
- Swift SDK with Rust FFI bridge for proving, commitment construction, and Nostr relay transport
- Testnet deployment automation with real proof/VK fixtures

## Repository Layout

```
src/                     Rust: circuits, Merkle, commitments, prover, ceremony, FFI
contracts/sep-xxxx/      Soroban contract (soroban-sdk 25.3.0)
swift-mls/               Swift package: proof generation, contract models, Nostr invitations
scripts/                 Testnet deployment, XCFramework build
docs/                    SEP specification, design doc, phase implementation guides
```

## Architecture

The prover generates a Groth16 proof that:

1. the prover knows a BLS12-381 secret scalar `sk`
2. `Poseidon(sk)` is a leaf in a canonicalized Poseidon Merkle tree
3. the root is bound into an on-chain commitment via `Poseidon(Poseidon(root, epoch), salt)`

The contract stores only opaque 32-byte commitments and epoch counters. No member identities, salts, or rosters appear on-chain. Proof verification uses 4 BLS12-381 host function calls (MSM + add + negate + multi-pairing).

## Contract ABI

The SEP specification, Soroban contract, and Swift SDK all expose one consistent interface:

| Method | Parameters |
|--------|-----------|
| `create_group` | `group_id, commitment, tier, proof, public_inputs` |
| `update_commitment` | `group_id, new_commitment, new_epoch, proof, public_inputs` |
| `verify_membership` | `group_id, proof, public_inputs` |
| `deactivate_group` | `group_id, proof, public_inputs` |
| `get_state` | `group_id` |
| `get_history` | `group_id, max_entries` |

`PublicInputs` is `{ commitment: BytesN<32>, epoch: u64 }`. The contract verifies these match on-chain state before passing them to the pairing check.

## Proof Format

The Rust prover emits **192-byte compressed** proofs (G1 48B + G2 96B + G1 48B). The contract expects **384-byte uncompressed** proof components (G1 96B + G2 192B + G1 96B).

The bridge layer handles conversion:
- Rust: `prover::proof_to_uncompressed_components()`
- FFI: `sep_proof_to_contract_format()`
- Swift: `RustBridge.proofToContractFormat()` → `SEPContractProofComponents`

## Current Status

What works end-to-end:

- Unified ABI across SEP spec, contract, and Swift SDK (`PublicInputs`, `new_epoch`, `max_entries`)
- Proof decompression bridge: compressed prover output → uncompressed contract format with tested round-trips
- End-to-end integration test: setup → prove → decompress → verify (including cross-epoch rejection)
- Testnet deployment script with real Groth16 fixtures (VK + proofs + public inputs)
- Canonical member ordering enforced in code with duplicate rejection
- Ceremony module with explicit safety boundaries (`#[cfg(test)]` for insecure key derivation)
- XCFramework build script for macOS/iOS distribution (`scripts/build-xcframework.sh`)

What remains for production:

- A real Groth16 Phase 2 MPC pipeline (the ceremony module is reference-grade, single-process)
- MLS (RFC 9420) library integration for the primary use case
- Live testnet deployment (script is ready but has not been executed against a network)
- Gas profiling and optimization for the contract
- Traffic analysis mitigations in the Nostr relay layer (padding, dummy events)

## Testing

```bash
# Rust core + prover + ceremony
cargo test

# Soroban contract (fast, focused)
cd contracts/sep-xxxx && cargo test

# Swift SDK
cd swift-mls && ./scripts/build-rust-bridge.sh && swift test
```

Key test coverage:
- 14 prover tests including e2e proof→contract pipeline and epoch transition
- 8 contract tests covering initialization, error paths, and PublicInputs validation
- Swift tests for proof generation, commitment construction, contract client, and Nostr events

## Building

```bash
# Rust static library (debug, for local Swift development)
cd swift-mls && ./scripts/build-rust-bridge.sh

# XCFramework (release, for distribution)
./scripts/build-xcframework.sh

# Soroban contract WASM
stellar contract build --manifest-path contracts/sep-xxxx/Cargo.toml

# Testnet deployment + integration test
./scripts/deploy_sep_xxxx_testnet.sh
```

## Documentation

| Doc | Description |
|-----|-------------|
| `docs/sep.md` | Normative SEP-XXXX specification |
| `docs/design-doc.md` | Architecture overview and phase roadmap |
| `docs/phase-1.md` | Phase 1: Groth16 circuits |
| `docs/phase-2.md` | Phase 2: Trusted setup ceremony |
| `docs/phase-3.md` | Phase 3: Soroban contract |
| `docs/phase-4.md` | Phase 4: Nostr relay transport |
| `docs/testnet-deployment.md` | Testnet deployment guide |
| `docs/relay-design-doc.md` | Relay architecture |

## License

MIT. See [`LICENSE`](LICENSE).
