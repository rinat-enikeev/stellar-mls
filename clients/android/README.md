# Stellar Chat — Android

Private group chat over Nostr relays with on-device cryptographic membership verification, implementing the stellar-mls privacy architecture (SEP-XXXX + NIP-XX). Feature-parity with the iOS client.

## Prerequisites

- Android Studio Ladybug (2024.2) or later
- JDK 17+
- Android SDK 35, minSdk 26
- Android NDK 27+ (for native library rebuild)
- Rust toolchain with `aarch64-linux-android` and `x86_64-linux-android` targets (for native library rebuild)

## Build & Run

### Quick start

1. Open `clients/android/StellarChat` in Android Studio
2. Sync Gradle
3. Run on emulator (API 26+) or device

```bash
cd clients/android/StellarChat
./gradlew assembleDebug
```

### Rebuilding the native library

The pre-built `.so` files in `kotlin-mls/src/main/jniLibs/` are checked in. To rebuild from source:

```bash
# From repo root — requires NDK and Rust Android targets
./scripts/build-android.sh

# Copy into the Kotlin MLS module
cp -r build/android/jniLibs/ kotlin-mls/src/main/jniLibs/
```

## Architecture

```
com.stellarmls.chat/
├── model/          ChatGroup, ChatMessage, InviteCode, BootstrapPayload, ChatError
├── crypto/         GroupCrypto (AES-256-GCM, HKDF), KeyManager, KeyAttestation,
│                   NostrEventBuilder (secp256k1 Schnorr), StorageEncryption, StellarStrKey
├── nostr/          NostrRelayConnection (OkHttp WebSocket, M-8 backoff),
│                   NostrMessageTransport (BLS sender auth), InvitationTransport
├── onchain/        SEPContractClient, OnChainService (proof gen + retry),
│                   OkHttpRelayerTransport (fee decoupling), ContractTypes
├── persistence/    PersistenceStore (field-level AES-256-GCM), Room DB, PersistedModels
├── viewmodel/      GroupListViewModel (central state), ChatViewModel,
│                   CreateGroupViewModel, JoinGroupViewModel
├── ui/
│   ├── screens/    GroupList, Chat, CreateGroup, JoinGroup, InviteMember,
│   │               PendingInvitations, Settings, QRScanner
│   └── theme/      Material 3 theme
├── SecurityLog.kt  Structured audit logging (debug-only)
└── MainActivity.kt Navigation host
```

## Cryptographic Keys

Each user holds four independent key types, generated on first launch:

| Key | Curve | Purpose |
|-----|-------|---------|
| secp256k1 | Koblitz | Nostr identity (NIP-01 Schnorr event signing via Rust FFI) |
| BLS12-381 | BLS | Group membership (ZK proofs, Poseidon Merkle leaves) |
| Ed25519 | Twisted Edwards | Stellar on-chain identity (transaction signing) |
| X25519 | Montgomery | Key agreement (ECDH for encrypted invitations) |

Key attestations bind BLS to Ed25519: `Ed25519_sign(SHA-256("SEP-XXXX:key-binding" || bls_pubkey))`.

All secret keys are stored in **EncryptedSharedPreferences** backed by Android Keystore (AES256_SIV key encryption, AES256_GCM value encryption).

## Protocol

| Item | Value |
|------|-------|
| Transport | Nostr NIP-01 WebSocket |
| Message Kind | 24114 |
| Invitation Kind | 24113 |
| Signing | secp256k1 Schnorr (BIP-340) via Rust FFI |
| Encryption | AES-256-GCM |
| Key Derivation | HKDF-SHA256 (`sep-msg-key-v1`) |
| Topic Derivation | `SHA256("sep-topic-v1" \|\| groupSecret).hex[:16]` |
| Inbox Derivation | `SHA256("sep-inbox-v1" \|\| x25519Pubkey).hex[:16]` |
| Default Relays | `relay.damus.io`, `nos.lol`, `relay.nostr.band`, `relay.snort.social`, `nostr.wine` |

## Security

- **BLS sender authentication (H-4)**: Every message includes the sender's BLS public key; receivers verify membership before displaying
- **Replay protection (H-7)**: Bounded LRU set of processed protocol event IDs prevents duplicate processing
- **Salt request rate limiting (H-5)**: Responds to salt requests only once per (sender, epoch) pair
- **Event ID verification (N-7)**: Recomputed before processing to reject tampered events
- **Message size limit (N-18)**: 1 MB cap to prevent relay-based memory exhaustion
- **Encrypted persistence (N-9)**: Field-level AES-256-GCM encryption for groups and messages in Room DB
- **Secure token storage (N-5)**: Relayer auth tokens in EncryptedSharedPreferences, not plaintext
- **Debug-only logging**: All `Log.*` calls guarded by `BuildConfig.DEBUG` — zero console output in release builds
- **Structured security logging (M-19)**: Audit-relevant events logged via `SecurityLog` (debug builds only)
- **Exponential backoff (M-8)**: WebSocket reconnection with capped backoff and heartbeat ping
- **Contract retry (M-10)**: On-chain operations retried 3x with exponential backoff

## Persistence

Groups, messages, and keys persist across app restarts:

- **Groups and messages**: Room database with field-level AES-256-GCM encryption (group name, secret, members, salt, commitment, message content)
- **Secret keys**: EncryptedSharedPreferences (Android Keystore-backed AES256)
- **Relay and contract configuration**: SharedPreferences (non-sensitive settings)

## On-Chain Integration

- **Soroban contract client**: HTTP transport or relayer transport (fee decoupling)
- **ZK proof generation**: Groth16 proofs via Rust FFI with testing proving keys
- **Commitment publishing**: On group creation and membership changes
- **Commitment verification**: Poseidon commitment comparison against on-chain state
- **Group deactivation**: Any member with a valid proof can permanently freeze a group
- **Relayer support**: `OkHttpRelayerTransport` with bearer token auth and certificate pinning

## Interoperability

This app is wire-compatible with the iOS StellarChat client. Both use identical:

- Event kinds (24113 invitations, 24114 messages)
- Tag structure (`t` NIP hashtag tag)
- Sealed envelope JSON format (`version`, `scheme`, `ephemeral_public_key`, `nonce`, `ciphertext`, `authentication_tag`)
- Hidden topic derivation (`SHA256("sep-topic-v1" || secret).hex().prefix(16)`)
- Hidden inbox derivation (`SHA256("sep-inbox-v1" || x25519_pubkey).hex().prefix(16)`)
- AES-256-GCM encryption with HKDF-SHA256 key derivation
- Invite code format (base64 JSON with `groupID`, `groupSecret`, `name`, `relayHints`, `members`, `epoch`, `salt`, `commitment`)
- secp256k1 Schnorr signing via the same Rust FFI library
- BLS12-381 and Poseidon commitment computation via the same Rust circuits
- BLS sender authentication envelope (`text` + `senderBlsPubkey`)
- Protocol message format (SEPMemberJoined, SEPGroupStateUpdate, SEPSaltRequest/Response, SEPGroupRenamed)

## Known Limitations

- **Testing proving keys**: Uses `generateTestingProvingKey` — production should use keys from a multi-party trusted setup ceremony
- **Derived Stellar key**: Ed25519 key is HKDF-derived from Nostr secp256k1 key — Nostr key compromise implies Stellar key compromise
- **No push notifications**: Messages are only received while the app is in the foreground
- **No multi-device sync**: Keys and state are device-local
