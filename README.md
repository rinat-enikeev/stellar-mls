# Stellar MLS

Private group membership on Stellar using zero-knowledge proofs.

Members prove they belong to a group without revealing who they are. The blockchain stores opaque commitments — never names, keys, or member lists. Any member can create, update, verify, or deactivate a group by presenting a Groth16 proof. The proof is constant-size regardless of group size: a 2,048-member group costs the same to verify as a 2-member group.

## What's in the box

| Component | Language | What it does |
|-----------|----------|-------------|
| `src/` | Rust | ZK circuits, Poseidon Merkle trees, Groth16 prover, trusted setup ceremony, C FFI + JNI bridge |
| `contracts/sep-xxxx/` | Rust (Soroban) | On-chain group state, BLS12-381 proof verification via host functions |
| `swift-mls/` | Swift | SDK for iOS/macOS — proof generation, contract client, Nostr transport |
| `kotlin-mls/` | Kotlin | SDK for Android — JNI bridge, proof generation, commitment builder |
| `clients/ios/` | Swift (SwiftUI) | Reference chat app with group creation, invitations, on-chain verification, encrypted persistence |
| `clients/android/` | Kotlin (Compose) | Reference chat app — feature-parity with iOS, Room persistence, EncryptedSharedPreferences |
| `relayer/` | Rust | Fee-decoupling HTTP relayer — signs and submits transactions so users don't need funded accounts |
| `scripts/` | Shell | Build automation (XCFramework, Android NDK, testnet/mainnet deployment) |
| `docs/` | Markdown | SEP specification, design docs, phase implementation guides, security audit reports |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  Mobile Apps (iOS / Android)                                │
│  SwiftUI / Jetpack Compose                                  │
│  Group management, invitations, encrypted chat              │
├─────────────────────────────────────────────────────────────┤
│  Nostr Relays (kind 24113 invitations, kind 24114 messages) │
│  Encrypted transport. Protocol messages + chat.             │
├─────────────────────────────────────────────────────────────┤
│  SDKs (SwiftMLS / kotlin-mls)                               │
│  Proof generation, commitment building, contract client     │
├─────────────────────────────────────────────────────────────┤
│  Rust Core (sep-xxxx-circuits)                              │
│  Groth16 circuits, Poseidon hash, BLS12-381, FFI/JNI       │
├─────────────────────────────────────────────────────────────┤
│  Soroban Contract (sep-xxxx-contract)                       │
│  Stores commitments + epochs. Verifies proofs on-chain.     │
│  Never sees member identities.                              │
└─────────────────────────────────────────────────────────────┘
```

The prover generates a Groth16 proof that:

1. The prover knows a BLS12-381 secret key `sk`
2. `Poseidon(sk)` is a leaf in a canonicalized Poseidon Merkle tree
3. The Merkle root is bound into an on-chain commitment: `Poseidon(Poseidon(root, epoch), salt)`

The contract stores only opaque 32-byte commitments and epoch counters. Proof verification uses 4 BLS12-381 host function calls (MSM + add + negate + multi-pairing check).

## Repository layout

```
stellar-mls/
├── src/
│   ├── lib.rs                 Tier definitions, public API
│   ├── circuit/mod.rs         MembershipCircuit (R1CS constraints)
│   ├── prover/mod.rs          Groth16 setup, prove, verify
│   ├── merkle/mod.rs          Poseidon Merkle tree with canonical ordering
│   ├── poseidon/mod.rs        Poseidon hash (BLS12-381 scalar field)
│   ├── commitment/mod.rs      SHA-256 and Poseidon commitment builders
│   ├── ceremony/mod.rs        Powers of Tau Phase 1 (reference impl)
│   ├── ffi.rs                 C FFI (13 functions for Swift/iOS)
│   ├── jni_ffi.rs             JNI bridge (7 methods for Android)
│   └── bin/                   Fixture generators
├── contracts/sep-xxxx/
│   └── src/lib.rs             Soroban contract (~990 lines)
├── swift-mls/
│   ├── Package.swift
│   └── Sources/SwiftMLS/      11 Swift modules (~1,360 lines)
├── kotlin-mls/
│   └── src/main/java/         6 Kotlin classes (~370 lines)
├── clients/
│   ├── ios/StellarChat/       iOS app (27 Swift files, ~4,460 lines)
│   └── android/StellarChat/   Android app (40 Kotlin files, ~5,270 lines)
├── relayer/                   Fee-decoupling HTTP relayer (axum + stellar CLI)
├── scripts/
│   ├── build-xcframework.sh   Apple targets → XCFramework
│   ├── build-android.sh       Android NDK cross-compilation
│   ├── deploy-mainnet.sh      One-command mainnet/testnet contract deployment
│   └── deploy_sep_xxxx_testnet.sh  Contract deployment + integration test
└── docs/                      15 documents (spec, design, audit reports)
```

## Contract ABI

The SEP specification, Soroban contract, Swift SDK, and Kotlin SDK all expose the same interface:

| Method | Parameters | Purpose |
|--------|-----------|---------|
| `create_group` | `group_id, commitment, tier, proof, public_inputs` | Register a new group at epoch 0 |
| `update_commitment` | `group_id, new_commitment, new_epoch, proof, public_inputs` | Advance to next epoch after membership change |
| `verify_membership` | `group_id, proof, public_inputs` | Read-only membership check |
| `deactivate_group` | `group_id, proof, public_inputs` | Permanently freeze a group |
| `get_state` | `group_id` | Query current state |
| `get_history` | `group_id, max_entries` | Query historical states (max 64) |

`PublicInputs = { commitment: BytesN<32>, epoch: u64 }`. The contract verifies these match on-chain state before running the pairing check.

## Proof format

The Rust prover emits **192-byte compressed** proofs (G1 48B + G2 96B + G1 48B). The Soroban contract expects **384-byte uncompressed** proof components (G1 96B + G2 192B + G1 96B). The FFI/JNI bridge handles decompression.

## Group tiers

| Tier | Max members | Merkle depth | Circuit constraints |
|------|------------|-------------|-------------------|
| Small | 32 | 5 | ~5,000 |
| Medium | 256 | 8 | ~8,000 |
| Large | 2,048 | 11 | ~11,000 |

Tier is set at group creation and cannot change. The contract stores a separate verification key per tier.

## Cryptographic keys per user

Each user holds four independent key types:

| Key | Curve | Purpose |
|-----|-------|---------|
| secp256k1 | Koblitz | Nostr identity (event signing via Schnorr) |
| BLS12-381 | BLS | Group membership (ZK proofs, Merkle leaves) |
| Ed25519 | Twisted Edwards | Stellar on-chain identity (transaction signing) |
| X25519 | Montgomery | Key agreement (ECDH for encrypted invitations) |

Key attestations bind BLS to Ed25519: `Ed25519_sign(SHA-256("SEP-XXXX:key-binding" || bls_pubkey))`.

---

## Building

### Prerequisites

- Rust 1.75+ with `cargo`
- Xcode 15+ (for iOS/macOS)
- Android Studio with NDK 27+ (for Android)
- `stellar` CLI (for contract deployment)

### Rust core

```bash
# Run all tests (circuits, prover, Merkle, commitment, ceremony)
cargo test

# Build static library for local Swift development
cd swift-mls && ./scripts/build-rust-bridge.sh
```

### Soroban contract

```bash
# Build WASM
stellar contract build --manifest-path contracts/sep-xxxx/Cargo.toml

# Run contract tests
cd contracts/sep-xxxx && cargo test
```

### Swift SDK

```bash
cd swift-mls

# Build and test (requires Rust bridge to be built first)
swift build && swift test
```

### Kotlin SDK + Android app

```bash
cd clients/android/StellarChat

# Build native library for Android (requires NDK)
../../scripts/build-android.sh

# Copy JNI libs into app
cp -r ../../build/android/jniLibs/ app/src/main/jniLibs/

# Build app
./gradlew :app:assembleDebug

# Run instrumented tests (requires emulator or device)
./gradlew :app:connectedAndroidTest
```

### XCFramework (release distribution)

```bash
# Requires: rustup target add aarch64-apple-darwin x86_64-apple-darwin \
#           aarch64-apple-ios aarch64-apple-ios-sim
./scripts/build-xcframework.sh
# Output: build/SEPMLSFFI.xcframework
```

### Testnet deployment

```bash
# Requires: stellar CLI, funded testnet account
./scripts/deploy_sep_xxxx_testnet.sh
# Deploys contract, generates fixtures, runs full integration test
```

## Testing

```bash
# Everything
cargo test                                    # Rust core (109 tests)
cd contracts/sep-xxxx && cargo test           # Contract
cd swift-mls && swift test                    # Swift SDK
cd clients/android/StellarChat && \
  ./gradlew :app:connectedAndroidTest         # Android (33 instrumented tests)
```

---

## Using Stellar MLS in the real world

This section walks through what it takes to go from this repository to a deployed, working private group membership system.

### Step 1: Run a trusted setup ceremony

The Groth16 proof system requires a one-time trusted setup. The `src/ceremony/mod.rs` module is a single-process reference implementation suitable for development and testing. **Do not use it for production.**

For production, you need a multi-party computation (MPC) ceremony where multiple independent participants contribute randomness. As long as at least one participant is honest and destroys their toxic waste, the setup is secure.

**What to use:**
- [snarkjs](https://github.com/iden3/snarkjs) for a JavaScript-based MPC coordinator
- [Hermez Phase 2 ceremony](https://github.com/hermez-ceremony) as a reference for large-scale ceremonies
- Export the ceremony as a Groth16 proving key and verification key in BLS12-381

**What you get:**
- One proving key per tier (distributed to clients — they generate proofs)
- One verification key per tier (stored on-chain during contract initialization)

The proving key is large (~10-50 MB per tier). Distribute it with your app binary or download it on first launch.

### Step 2: Deploy the Soroban contract

```bash
# 1. Build the WASM
stellar contract build --manifest-path contracts/sep-xxxx/Cargo.toml

# 2. Deploy to the network
stellar contract deploy \
  --network mainnet \
  --source-account YOUR_DEPLOYER_IDENTITY \
  --wasm target/wasm32-unknown-unknown/release/sep_xxxx_contract.wasm

# 3. Initialize with your ceremony's verification keys
stellar contract invoke \
  --id YOUR_CONTRACT_ID \
  --source-account YOUR_DEPLOYER_IDENTITY \
  -- initialize \
  --admin YOUR_ADMIN_ADDRESS \
  --vk-small-file-path vk-small.json \
  --vk-medium-file-path vk-medium.json \
  --vk-large-file-path vk-large.json
```

After initialization, the admin address is only used for contract upgrades — it has no special privileges for group operations. Any valid proof grants access.

### Step 3: Deploy a relayer (recommended)

Without a relayer, the Stellar account paying transaction fees is visible on-chain. This leaks the identity of whoever submits a group operation, undermining the privacy model.

A relayer is a simple HTTP service that:
1. Receives the same JSON payload clients would send to Soroban RPC
2. Wraps it in a Stellar transaction signed with the relayer's keypair
3. Submits to the network and returns the result

The relayer never sees member identities — it just pays fees on behalf of clients. Deploy it behind a rate limiter and optionally require a bearer token for authentication.

**Relayer API contract:**
- `POST /` with `Content-Type: application/json`
- Body: `{ "contractID": "...", "function": "create_group", "payload": {...} }`
- Response: same as Soroban RPC response
- Optional: `Authorization: Bearer <token>` header

**Relayer payload validation (required):**
The relayer MUST validate payloads before submitting to prevent abuse as a fee-paying proxy for arbitrary transactions:
- **Whitelist contract address**: Only accept invocations targeting the specific SEP contract ID
- **Whitelist function names**: Only allow `create_group`, `update_commitment`, `verify_membership`, `deactivate_group`
- **Validate proof structure**: Verify the proof field is exactly 384 bytes (96 + 192 + 96) before submission
- **Rate limiting**: Limit requests per bearer token and per IP address
- **Payload size cap**: Reject payloads exceeding a reasonable size threshold (e.g., 8 KB)

Both mobile SDKs have built-in relayer transport (`SEPRelayerTransport` on iOS, `OkHttpRelayerTransport` on Android). Configure the relayer URL in the app settings and all contract operations route through it transparently.

### Step 4: Set up Nostr relays

Group communication (invitations, encrypted messages, protocol updates) flows over Nostr relays using two event kinds:
- **kind 24113** — encrypted invitations (X25519 ECDH + AES-256-GCM)
- **kind 24114** — encrypted group messages and protocol messages (AES-256-GCM with shared group key)

You can use public relays (`wss://relay.damus.io`, `wss://nos.lol`) or run your own. The relay sees encrypted ciphertext and opaque tags — it cannot read message content or determine group membership.

For better privacy, run your own relay and access it over Tor. For reliability, configure multiple relays — both apps support multi-relay fanout.

### Step 5: Integrate the SDK into your app

#### iOS (Swift)

Add `SwiftMLS` as a Swift Package dependency:

```swift
// Package.swift
dependencies: [
    .package(path: "../swift-mls")  // or a remote URL
]
```

Create a group:
```swift
import SwiftMLS

// Generate a proving key (or load from bundle)
let provingKey = try SEPProofGenerator.generateTestingProvingKey(tier: .small)

// Build the member list
let myLeaf = SEPGroupMemberLeaf(
    publicKeyCompressed: myBLSPublicKey,  // 48 bytes
    leafHash: SEPCommitmentBuilder.computeLeafHash(secretKey: myBLSSecretKey)
)

// Generate a proof
let proofBundle = try SEPProofGenerator.generateMembershipProof(
    provingKey: provingKey,
    members: [myLeaf],
    secretKey: myBLSSecretKey,
    epoch: 0,
    salt: SEPCommitmentBuilder.generateSalt(),
    tier: .small
)

// Submit to the contract (direct or via relayer)
let transport = URLSessionSEPContractTransport(endpoint: sorobanRPCURL)
// or: SEPRelayerTransport(config: SEPRelayerConfig(relayerURL: relayerURL))
let client = SEPContractClient(contractID: contractID, transport: transport)

let response = try await client.createGroup(SEPCreateGroupRequest(
    groupID: groupIDData,
    commitment: proofBundle.publicInputs.commitment,
    proof: uncompressedProof,  // 384 bytes, decompressed via proofToContractFormat
    publicInputs: proofBundle.publicInputs,
    tier: UInt32(SEPTier.small.rawValue)
))
```

#### Android (Kotlin)

Add the `kotlin-mls` module as a dependency:

```kotlin
// settings.gradle.kts
include(":kotlin-mls")
project(":kotlin-mls").projectDir = file("../../kotlin-mls")

// app/build.gradle.kts
dependencies {
    implementation(project(":kotlin-mls"))
}
```

Create a group:
```kotlin
import com.stellarmls.mls.*

// Generate a proving key (or load from assets)
val provingKey = SEPProofGenerator.generateTestingProvingKey(SEPTier.SMALL)

// Build the member list
val myLeaf = SEPGroupMemberLeaf(
    publicKeyCompressed = SEPCommitmentBuilder.computePublicKey(myBLSSecretKey),
    leafHash = SEPCommitmentBuilder.computeLeafHash(myBLSSecretKey)
)

// Generate a proof
val proofBundle = SEPProofGenerator.generateMembershipProof(
    provingKey = provingKey,
    members = listOf(myLeaf),
    secretKey = myBLSSecretKey,
    epoch = 0,
    salt = SEPCommitmentBuilder.generateSalt(),
    tier = SEPTier.SMALL
)

// Submit via OnChainService (handles proof format conversion)
val service = OnChainService(contractID, sorobanRPCEndpoint)
// or: OnChainService(contractID, relayerURL, authToken)
val response = service.publishGroupCreation(
    groupIDData = groupID,
    members = listOf(myLeaf),
    blsSecretKey = myBLSSecretKey,
    epoch = 0,
    salt = salt,
    tier = SEPTier.SMALL
)
```

### Step 6: Verify groups against the chain

Users should verify their local group state matches what's on-chain. The SDKs provide this:

```swift
// iOS
let result = await onChainService.verifyCommitment(
    groupIDData: group.groupIDData,
    members: group.members,
    epoch: group.epoch,
    salt: group.salt,
    tier: group.tier
)
// result: .verified, .epochMismatch, .commitmentMismatch, .notPublished, .inactive, .error
```

```kotlin
// Android
val result = onChainService.verifyCommitment(
    groupIDData = group.groupIDData,
    members = group.members,
    epoch = group.epoch,
    salt = group.salt,
    tier = group.tier
)
// result: Verified, EpochMismatch, CommitmentMismatch, NotPublished, Inactive, Error
```

### Security checklist for production deployment

| Item | Why it matters |
|------|---------------|
| Run a proper MPC ceremony | The single-process ceremony module is insecure for production |
| Deploy a relayer | Without it, fee-payer identity leaks on-chain |
| Use multiple Nostr relays | Single relay = single point of surveillance / failure |
| Distribute proving keys securely | Tampered proving keys produce invalid proofs (but can't break privacy) |
| Store BLS secret keys in secure enclave | Key compromise = impersonation in ZK proofs |
| Encrypt local storage | Both apps use platform secure storage (Keychain / EncryptedSharedPreferences) |
| Access relayer over Tor/VPN | Relayer can log IP addresses |
| Rotate groups periodically | Limits the window if a key is compromised |
| Verify on-chain state on join | Don't trust invitation payloads — verify commitment against the contract |

### What the system guarantees

- **Membership privacy**: The contract never learns who is in any group. Proofs reveal nothing about the prover.
- **Proof binding**: Every group operation (create, update, deactivate) requires a valid ZK proof. No one modifies a group without proving membership.
- **Epoch monotonicity**: Epochs increase strictly by 1. No replays, no forks.
- **Constant verification cost**: Same 4 host function calls regardless of group size.
- **End-to-end encryption**: All communication over Nostr is AES-256-GCM encrypted. Relays see ciphertext only.

### What the system does NOT guarantee

- **Fee-payer anonymity** without a relayer (use one)
- **Traffic analysis resistance** (timing and event patterns are observable on Nostr)
- **Key compromise recovery** (compromised BLS key requires re-keying the group)
- **Relay honesty** (relays can drop events — mitigated by multi-relay fanout)
- **Group-ID opacity** (if the group ID is derived carelessly, it might reveal the creator)

---

## Documentation

| Document | Description |
|----------|-------------|
| [`docs/sep.md`](docs/sep.md) | Normative SEP-XXXX specification |
| [`docs/design-doc.md`](docs/design-doc.md) | Architecture overview and phase roadmap |
| [`docs/phase-1.md`](docs/phase-1.md) | Groth16 circuits and Poseidon hashing |
| [`docs/phase-2.md`](docs/phase-2.md) | Trusted setup ceremony |
| [`docs/phase-2-mpc-integration.md`](docs/phase2-mpc-integration.md) | MPC ceremony integration guide |
| [`docs/phase-3.md`](docs/phase-3.md) | Soroban contract design |
| [`docs/phase-4.md`](docs/phase-4.md) | Production readiness (fee decoupling, salt distribution, attestation, deactivation) |
| [`docs/relay-design-doc.md`](docs/relay-design-doc.md) | Relay architecture and confidentiality |
| [`docs/nip-private-group-transport.md`](docs/nip-private-group-transport.md) | NIP proposal for Nostr group transport |
| [`docs/testnet-deployment.md`](docs/testnet-deployment.md) | Testnet deployment guide |
| [`docs/mainnet-deployment.md`](docs/mainnet-deployment.md) | **Mainnet deployment guide** (contract + relayer + app config) |
| [`docs/real-world-gap-analysis.md`](docs/real-world-gap-analysis.md) | Gap analysis for production deployment |
| [`docs/audit-report.md`](docs/audit-report.md) | Security audit report |
| [`docs/audit-report-v2.md`](docs/audit-report-v2.md) | Security audit report (round 2) |
| [`docs/audit-critical.md`](docs/audit-critical.md) | Critical audit findings and resolutions |
| [`docs/audit-c1-c9-findings.md`](docs/audit-c1-c9-findings.md) | Detailed C1–C9 findings |

## License

MIT. See [`LICENSE`](LICENSE).
