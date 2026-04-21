# Stellar MLS

Private group membership on Stellar using zero-knowledge proofs.

**Website:** [onym.chat](https://onym.chat)

A naive on-chain group registry leaks the social graph twice over: the member list is public, and every update is signed by a recognizable Stellar account. Stellar MLS replaces the member list with an opaque commitment and replaces the update signature with a Groth16 proof submitted through a fee-decoupling relayer. The blockchain stores 32 bytes per group and verifies membership in 4 BLS12-381 host calls — constant cost whether the group has 2 members or 2,048.

## Architecture

```
      ┌──────────────────┐        ┌──────────────────┐
      │  iOS / macOS app │        │    Android app   │
      │  (SwiftUI ref.)  │        │   (Compose ref.) │
      └────────┬─────────┘        └────────┬─────────┘
               │                           │
      ┌────────▼─────────┐        ┌────────▼─────────┐
      │   swift-mls SDK  │        │  kotlin-mls SDK  │
      │   (XCFramework)  │        │    (JNI .so)     │
      └────────┬─────────┘        └────────┬─────────┘
               │                           │
               └─────────────┬─────────────┘
                             │  Rust core (src/)
                             │  Groth16 prover · Poseidon
                             │  Merkle tree · BLS12-381
                             ▼
            ┌──────────┐          ┌────────────────────┐
            │  Relayer │──proof──▶│  Soroban contract  │
            │  (Axum)  │          │  (SEP-XXXX, BLS)   │
            └──────────┘          └────────────────────┘
                                     Stellar network

     ciphertext side-channel (not trusted for integrity):
     Nostr relay (strfry) + Blossom blob store
```

The relayer pays fees so the transaction signer cannot be linked to a group member. The contract verifies a Groth16 proof that the submitter knows a secret whose Poseidon hash sits in the group's committed Merkle tree, bound to the current epoch and salt. Nostr and Blossom carry end-to-end encrypted messages and blobs out of band — the contract never sees plaintext.

## Getting started

### Integrate the SDK in your app

- **iOS / macOS** — see [`swift-mls/README.md`](swift-mls/README.md) for XCFramework install, `SEPContractClient`, and `SEPInvitationSender`.
- **Android** — see [`kotlin-mls/README.md`](kotlin-mls/README.md) for JNI bridge setup, `SEPProofGenerator`, and `RustBackedNostrSigner`.

### Run the reference chat apps

Fully-worked integrations using both SDKs:

- [`clients/ios/`](clients/ios/) — SwiftUI chat client
- [`clients/android/`](clients/android/) — Jetpack Compose chat client

### Self-host the infrastructure

One command provisions a Digital Ocean droplet, configures Cloudflare DNS, obtains Let's Encrypt certificates, and starts the relayer, Nostr relay (strfry), Blossom, and nginx:

```bash
./deploy/digitalocean/deploy.sh
```

Idempotent; saves state to `.env`. See [`docs/mainnet-deployment.md`](docs/mainnet-deployment.md) for contract deployment.

## Build from source

```bash
git clone https://github.com/rinat-enikeev/stellar-mls.git
cd stellar-mls

cargo test                                                     # Rust core + tests
./scripts/build-xcframework.sh                                 # iOS XCFramework
./scripts/build-android.sh                                     # Android JNI libs
stellar contract build --manifest-path contracts/sep-xxxx/Cargo.toml
```

Configure `relayer/.env` (contract ID, RPC) and optionally `.env` (`DOMAIN=onym.chat`) to auto-wire the apps to your infrastructure.

## Repo layout

| Path | Language | Role |
|------|----------|------|
| [`src/`](src/) | Rust | ZK circuits, Poseidon Merkle trees, Groth16 prover, C FFI + JNI bridge |
| [`contracts/sep-xxxx/`](contracts/sep-xxxx/) | Rust (Soroban) | On-chain group state, BLS12-381 proof verification |
| [`swift-mls/`](swift-mls/) | Swift | iOS/macOS SDK |
| [`kotlin-mls/`](kotlin-mls/) | Kotlin | Android SDK |
| [`clients/`](clients/) | SwiftUI / Compose | Reference chat apps |
| [`relayer/`](relayer/) | Rust (Axum) | Fee-decoupling HTTP relayer |
| [`deploy/`](deploy/) | Docker / Nginx | Self-hosted stack |
| [`docs/`](docs/) | Markdown | Specification, audits, design docs, ceremony runbooks |

The contract ABI, group tier parameters (Small/Medium/Large — 32 / 256 / 2,048 members), and cryptographic construction details live in the [SEP-XXXX specification](docs/sep.md).

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

## For security researchers

The system's privacy claims rest on Groth16 circuit soundness, the Soroban verifier's correct use of BLS12-381 host calls, and key handling inside the SDKs. Start here:

- **[`docs/sep.md`](docs/sep.md)** — SEP-XXXX specification: commitment scheme, Groth16 relations, Soroban interface
- **[`SECURITY.md`](SECURITY.md)** — threat model, in/out scope, private disclosure channels
- **[`docs/audit-report.md`](docs/audit-report.md)** — current audit (2026-04, 6 workstreams)
- **[`docs/protocol-soundness-analysis.md`](docs/protocol-soundness-analysis.md)** — smart-contract layer: commitment canonicalization, epoch monotonicity, replay prevention
- **[`docs/proof-of-soundness.md`](docs/proof-of-soundness.md)** and **[`docs/proof-of-correctness.md`](docs/proof-of-correctness.md)** — circuit-level proofs
- **Postmortems** — real incidents: [co-membership leak](docs/postmortem-co-membership-leak.md), [secure member removal](docs/postmortem-secure-member-removal.md), [unbound new commitment](docs/postmortem-unbound-new-commitment.md)
- **[`paper/venue/ieee/onym-ieee.pdf`](paper/venue/ieee/onym-ieee.pdf)** — IEEE experience report (alpha testnet; load-bearing caveats on trusted-setup participants and small-group unlinkability)

Please disclose vulnerabilities privately via the channels in [`SECURITY.md`](SECURITY.md); do not open public issues for security reports.

## Documentation

Full specification, design docs, ceremony runbooks, deployment guides, and audit reports live in [`docs/`](docs/).

## License

MIT. See [`LICENSE`](LICENSE).
