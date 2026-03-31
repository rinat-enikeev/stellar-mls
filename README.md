# stellar-mls

Reference implementation for a private group membership architecture on Stellar:

- Groth16 circuits over BLS12-381
- Poseidon Merkle commitments
- A Soroban contract that stores commitments and verifies proofs
- A Swift SDK with a Rust bridge for proving, commitment construction, and partial relay-layer support

This repository is useful for design validation, circuit work, contract development, and client-side prototyping. It is not yet a production-ready end-to-end system.

## Repository Layout

- `src/` — Rust circuits, Merkle logic, commitments, prover pipeline, ceremony code, and FFI bridge
- `contracts/sep-xxxx/` — Soroban contract for group creation, updates, verification, deactivation, and history
- `swift-mls/` — Swift package for proof generation, commitment construction, contract request models, and Nostr invitation publishing
- `docs/` — SEP, design notes, and phase-by-phase implementation docs

## Architecture

The current implementation proves:

1. the prover knows a BLS12-381 secret scalar `sk`
2. `Poseidon(sk)` is a leaf in a canonicalized Poseidon Merkle tree
3. the Merkle root is bound into an on-chain commitment together with `(epoch, salt)`

The contract stores only opaque commitments and epochs. It does not store member identities, salts, or the member list.

## Current Status

What is in reasonably good shape:

- canonical member ordering is enforced in code, not left to caller discipline
- duplicate member public keys are rejected during Merkle construction
- the public ceremony API stops at Phase 1 output and does not pretend to expose production-safe Groth16 keys
- the Soroban contract verifies Groth16 proofs against stored state rather than caller-supplied public inputs
- the Swift package can generate proofs through the local Rust bridge and publish Nostr invitation events

What is not production-ready:

- a real Groth16 Phase 2 MPC pipeline is not implemented in the public API; the ceremony code remains reference-grade
- the SEP, contract, and Swift SDK do not currently expose one consistent contract ABI
- the Rust/Swift proof pipeline emits compressed 192-byte proofs, while the contract expects uncompressed 384-byte proof structs
- there is no committed end-to-end path that generates a proof, adapts it to the contract ABI, submits it to Soroban, and verifies it successfully
- the Swift package is not packaged as an XCFramework or other Apple-ready binary distribution; it links against a locally built Rust static library
- `main` does not currently include testnet deployment automation, a relayer service implementation, or MLS integration

## Audit Summary

As of the latest review of `main`, the main issues were:

1. Contract interface drift
   The normative SEP and Swift request models still include `public_inputs`, `new_epoch`, and `get_history(max_entries)`, while the actual Soroban contract derives verification inputs from storage and exposes a different ABI.

2. Proof serialization gap
   The client-side prover returns compressed proofs, but the Soroban contract expects uncompressed proof points. The repository does not currently ship the conversion layer needed to bridge those two formats.

3. Missing integration coverage
   Contract tests cover basic logic and initialization paths, but not real Groth16 verification with contract-shaped proof and verification-key artifacts. Swift tests are also mostly local and mock-based.

4. Packaging and deployment gaps
   The Swift package depends on a locally built Rust archive, and the repository does not yet contain the deployment and relayer pieces described in the higher-level phase plan.

## Cryptographic Assessment

I did not find an obvious current break in the core proving relation on `main`. The strongest current caveat is not the circuit itself, but the boundary between reference code and deployable system:

- the core Rust proving pipeline is coherent for a reference implementation
- canonical roster handling is enforced
- the public ceremony surface is explicit that production-safe key derivation still requires a real Groth16 Phase 2 MPC

That means the repository is better described as:

- cryptographically plausible as a reference implementation
- not yet complete enough to treat as a production deployment candidate

## Testing

What I verified during review:

- `cargo test` in `contracts/sep-xxxx` passed
- `swift test` in `swift-mls` passed after building the Rust bridge

Notes:

- the root Rust suite contains heavy ceremony tests and is slow
- the contract test suite is still shallow relative to the risk of the contract boundary
- there is no committed contract integration test that exercises real proof verification end to end

## Building

Rust core:

```bash
cargo build
```

Soroban contract:

```bash
cd contracts/sep-xxxx
cargo test
```

Swift package:

```bash
cd swift-mls
./scripts/build-rust-bridge.sh
swift test
```

## Recommended Next Steps

- unify the SEP, contract, and Swift SDK around one contract ABI
- add proof decompression / contract-proof encoding so client-generated proofs are actually consumable by Soroban
- add end-to-end integration tests with real VK/proof artifacts
- package the Rust bridge for Apple targets instead of linking to `../target/debug`
- implement or integrate a real production Phase 2 ceremony flow before any mainnet deployment
- add testnet deployment and live integration automation to the main branch

## License

MIT. See [`LICENSE`](LICENSE).
