# Stellar MLS

Private group membership on Stellar using zero-knowledge proofs.

**Website:** [onym.chat](https://onym.chat)

Members prove they belong to a group without revealing who they are. The blockchain stores opaque commitments — never names, keys, or member lists. Any member can create, update, verify, or deactivate a group by presenting a Groth16 proof. The proof is constant-size regardless of group size: a 2,048-member group costs the same to verify as a 2-member group.

## What's in the box

| Component | Language | What it does |
|-----------|----------|-------------|
| `src/` | Rust | ZK circuits, Poseidon Merkle trees, Groth16 prover, C FFI + JNI bridge |
| `contracts/sep-xxxx/` | Rust (Soroban) | On-chain group state, BLS12-381 proof verification |
| `swift-mls/` | Swift | SDK for iOS/macOS |
| `kotlin-mls/` | Kotlin | SDK for Android |
| `clients/ios/` | SwiftUI | Reference chat app |
| `clients/android/` | Jetpack Compose | Reference chat app |
| `relayer/` | Rust (Axum) | Fee-decoupling HTTP relayer |
| `deploy/` | Docker / Nginx | Self-hosted stack (Nostr relay, Blossom, SSL) |

## How it works

The prover generates a Groth16 proof that:

1. The prover knows a BLS12-381 secret key `sk`
2. `Poseidon(sk)` is a leaf in a canonicalized Poseidon Merkle tree
3. The Merkle root is bound into an on-chain commitment: `Poseidon(Poseidon(root, epoch), salt)`

The contract stores only opaque 32-byte commitments and epoch counters. Proof verification uses 4 BLS12-381 host function calls (MSM + add + negate + multi-pairing check).

## Group tiers

| Tier | Max members | Merkle depth | Circuit constraints |
|------|------------|-------------|-------------------|
| Small | 32 | 5 | ~5,000 |
| Medium | 256 | 8 | ~8,000 |
| Large | 2,048 | 11 | ~11,000 |

## Quick start

```bash
git clone https://github.com/rinat-enikeev/stellar-mls.git
cd stellar-mls

# Rust core + tests
cargo test

# iOS XCFramework
./scripts/build-xcframework.sh

# Android JNI libs
./scripts/build-android.sh

# Soroban contract
stellar contract build --manifest-path contracts/sep-xxxx/Cargo.toml
```

Configure `relayer/.env` (contract ID, RPC) and optionally `.env` (`DOMAIN=onym.chat`) to auto-wire the apps to your infrastructure.

## Self-hosted infrastructure

`docker-compose.yml` runs the full stack:

| Service | Subdomain |
|---------|-----------|
| relayer | `relay.{domain}` |
| strfry (Nostr) | `nostr.{domain}` |
| Blossom | `blossom.{domain}` |
| nginx + certbot | SSL for all |

One-command deployment to Digital Ocean:

```bash
./deploy/digitalocean/deploy.sh
```

The script provisions a droplet, configures Cloudflare DNS, obtains Let's Encrypt certificates, and starts all services. It's idempotent and saves state to `.env`.

## Contract ABI

| Method | Purpose |
|--------|---------|
| `create_group` | Register a new group at epoch 0 |
| `update_commitment` | Advance to next epoch after membership change |
| `verify_membership` | Read-only membership check |
| `deactivate_group` | Permanently freeze a group |
| `get_state` / `get_history` | Query current / historical state |

## What the system guarantees

- **Membership privacy**: the contract never learns who is in any group
- **Proof binding**: every group operation requires a valid ZK proof
- **Epoch monotonicity**: no replays, no forks
- **Constant verification cost**: same 4 host function calls regardless of group size
- **End-to-end encryption**: all Nostr traffic is AES-256-GCM; relays see ciphertext only

## What it does NOT guarantee

- Fee-payer anonymity without a relayer
- Traffic analysis resistance on Nostr
- Automatic recovery from BLS key compromise (requires re-keying)

## Documentation

Full specification, design docs, phase guides, and audit reports live in [`docs/`](docs/). Start with [`docs/sep.md`](docs/sep.md) for the normative SEP-XXXX specification or [`docs/design-doc.md`](docs/design-doc.md) for the architecture overview.

## License

MIT. See [`LICENSE`](LICENSE).
