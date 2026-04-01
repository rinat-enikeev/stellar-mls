# StellarChat iOS

Private group chat over Nostr relays with on-device cryptographic membership verification, implementing the stellar-mls privacy architecture (SEP-XXXX + NIP-XX).

## Prerequisites

- Xcode 15.4+
- iOS 17.0+ simulator or device
- [XcodeGen](https://github.com/yonaskolb/XcodeGen) (`brew install xcodegen`)
- Pre-built `SEPMLSFFI.xcframework` at `build/SEPMLSFFI.xcframework` (from the Rust FFI layer)

## Build & Run

```bash
# From repo root — build the Rust XCFramework (requires Rust toolchain)
./scripts/build-xcframework.sh

# Generate the Xcode project
cd clients/ios/StellarChat
xcodegen generate

# Open and run
open StellarChat.xcodeproj
# Select an iPhone 17.0+ simulator → Cmd+R
```

If XcodeGen is not available, you can open the checked-in `.xcodeproj` directly, but regenerating is recommended after any `project.yml` change.

## Usage

### Create a Group

1. Tap **+** → **Create Group**
2. Enter a group name → tap **Create**
3. The app generates a 32-byte group ID, a 32-byte shared secret, computes the initial Poseidon Merkle commitment with you as the first member, and displays an invite code
4. Tap **Copy to Clipboard** and share the invite code with other members

### Join a Group

1. Tap **+** → **Join Group**
2. Paste the base64 invite code → tap **Join**
3. The invite code contains the group ID, shared secret, group name, and relay hints

### Chat

- Select a group from the list to open the chat view
- Messages are encrypted with AES-256-GCM before leaving the device
- The app connects to Nostr relays via WebSocket and subscribes to the group's hidden topic
- Messages from other members are decrypted and displayed in real time

### Settings

- View your secp256k1 public key (Nostr identity)
- See connected relay URLs
- View protocol details (encryption, signing, ZK backend)

## Architecture

```
┌─────────────────────────────────────────────────┐
│                    SwiftUI Views                 │
│  GroupListView · ChatView · CreateGroupView · …  │
├─────────────────────────────────────────────────┤
│              AppState / ChatViewModel            │
│         (Observable, group + message state)       │
├──────────────┬──────────────┬────────────────────┤
│  KeyManager  │  ChatGroup   │ NostrMessageTransport│
│  (Keychain,  │  (SEP state, │ (multi-relay pub/sub)│
│   signing)   │  commitment) │                      │
├──────────────┴──────────────┴────────────────────┤
│                   GroupCrypto                     │
│  AES-256-GCM · HKDF-SHA256 · SealedEnvelope     │
│  Hidden topic/inbox derivation                   │
├─────────────────────────────────────────────────┤
│               SwiftMLS (Rust FFI)                │
│  RustBackedNostrSigner · SEPCommitmentBuilder    │
│  secp256k1 Schnorr · BLS12-381 · Poseidon hash  │
└─────────────────────────────────────────────────┘
```

### Source Layout

```
StellarChat/
├── project.yml                  # XcodeGen project definition
└── StellarChat/
    ├── StellarChatApp.swift      # @main entry, AppState, group creation
    ├── Models/
    │   ├── KeyManager.swift      # Keychain storage, Rust-backed signing
    │   ├── ChatGroup.swift       # Group model, SEP membership, InviteCode
    │   └── GroupCrypto.swift     # AES-256-GCM, HKDF, topic derivation
    ├── Nostr/
    │   ├── NostrEvent.swift      # NIP-01 event builder with Schnorr signing
    │   ├── NostrRelayConnection.swift  # WebSocket relay (actor)
    │   └── NostrMessageTransport.swift # Multi-relay orchestration
    ├── ViewModels/
    │   └── ChatViewModel.swift   # Per-group message state, deduplication
    └── Views/
        ├── ContentView.swift
        ├── GroupListView.swift
        ├── ChatView.swift
        ├── CreateGroupView.swift
        ├── JoinGroupView.swift
        └── SettingsView.swift
```

## Protocol Alignment with Specifications

### What's Implemented

| Feature | Spec | Status |
|---------|------|--------|
| AES-256-GCM message encryption | NIP-XX | Implemented |
| HKDF-SHA256 key derivation (`"sep-msg-key-v1"` salt, `"traffic"` info) | NIP-XX | Implemented |
| Sealed envelope format (`version`, `scheme`, `nonce`, `ciphertext`, `authentication_tag`) | NIP-XX | Implemented |
| Hidden group topic: `SHA256("sep-topic-v1" \|\| groupSecret).hex().prefix(16)` | NIP-XX | Implemented |
| Hidden inbox tag: `SHA256("sep-inbox-v1" \|\| recipientPubkey).hex().prefix(16)` | NIP-XX | Implemented |
| Kind 24114 group message events with `sep_topic` + `sep_version` tags | NIP-XX | Implemented |
| NIP-01 event serialization and SHA256 event ID computation | NIP-01 | Implemented |
| secp256k1 Schnorr signature via Rust FFI (`RustBackedNostrSigner`) | NIP-01 | Implemented |
| BLS12-381 key derivation (`SEPCommitmentBuilder.computePublicKey`) | SEP-XXXX | Implemented |
| Poseidon leaf hash (`SEPCommitmentBuilder.computeLeafHash`) | SEP-XXXX | Implemented |
| Poseidon Merkle root (`SEPCommitmentBuilder.computeMerkleRoot`) | SEP-XXXX | Implemented |
| SHA256 commitment: `SHA256(poseidonRoot \|\| epoch_be \|\| salt)` | SEP-XXXX | Implemented |
| Tier-based tree depths (Small/32, Medium/256, Large/2048) | SEP-XXXX | Implemented |
| Multi-relay WebSocket connections with auto-reconnect | NIP-XX | Implemented |
| Base64 JSON invite codes with group secret + relay hints | App-level | Implemented |
| Keychain secret key storage | App-level | Implemented |
| Commitment recomputation on `addMember()` | SEP-XXXX | Implemented |

### What's Not Yet Implemented

| Feature | Spec | Notes |
|---------|------|-------|
| Kind 24113 invitation events | NIP-XX | Invitations are shared via clipboard/out-of-band, not over Nostr relays |
| Soroban contract interaction | SEP-XXXX | No on-chain commitment publishing or verification against ledger state |
| Groth16 ZK proof generation | SEP-XXXX | Rust FFI supports it (`SEPProofGenerator`), but not wired into the app flow |
| Groth16 proof verification | SEP-XXXX | No on-device or on-chain verification of membership proofs |
| Fee decoupling / relayer pattern | SEP-XXXX | No transaction submission at all yet |
| Key attestation (Ed25519 ↔ BLS12-381 binding) | SEP-XXXX §1.1 | BLS key derived from same secret as Nostr key; no separate attestation |
| Salt distribution and recovery | SEP-XXXX | Salt generated locally but not shared with other members |
| Member sorting enforcement in `addMember()` | SEP-XXXX §2 | Rust `computeMerkleRoot` sorts internally, but Swift-side `members` array is append-order |
| Epoch validation against Stellar ledger | SEP-XXXX | Epoch incremented locally, not verified against on-chain state |
| Group deactivation | SEP-XXXX | No mechanism to deactivate or archive groups |
| Message persistence | App-level | Messages are in-memory only; lost on app restart |

### Known Design Deviations

1. **Shared key derivation**: The secp256k1 Nostr signing key and BLS12-381 group membership key are both derived from the same 32-byte secret. The SEP spec envisions these as independent keypairs with an explicit attestation linking them. This is acceptable for a demo but should be separated for production use.

2. **No contract integration**: The app operates as a pure Nostr messaging client. Commitments are computed locally but never published to or verified against Stellar. This means group membership is trusted, not cryptographically proven on-chain.

3. **Append-only member list**: `ChatGroup.addMember()` appends to the array without sorting by compressed public key. The Rust `computeMerkleRoot` function sorts internally before building the tree, so commitments are correct, but the Swift-side ordering doesn't match the canonical sort order defined in the spec.

## Interoperability

This app is wire-compatible with the Android StellarChat app. Both use identical:

- Event kinds (24113, 24114)
- Tag structure (`sep_topic`, `sep_version`)
- Sealed envelope JSON format (`version`, `scheme`, `ephemeral_public_key`, `nonce`, `ciphertext`, `authentication_tag`)
- Hidden topic derivation (`SHA256("sep-topic-v1" || secret).hex().prefix(16)`)
- AES-256-GCM encryption with HKDF-SHA256 key derivation
- Invite code format (base64 JSON with `groupID`, `groupSecret`, `name`, `relayHints`)
- secp256k1 Schnorr signing via the same Rust FFI library
- BLS12-381 and Poseidon commitment computation via the same Rust circuits

## Next Steps

### Phase 1: Core Improvements

- **Message persistence**: Store decrypted messages in a local database (SwiftData or SQLite) so chat history survives app restarts
- **Member sorting**: Sort `members` array by compressed BLS public key in `addMember()` to match the canonical order defined in SEP-XXXX, rather than relying on Rust-side sorting
- **Separate key derivation**: Generate independent secp256k1 and BLS12-381 keypairs, linked by an explicit `KeyAttestation` as defined in SEP-XXXX §1.1
- **Error handling**: Surface encryption/decryption failures and relay connection errors in the UI

### Phase 2: Nostr Protocol Completion

- **Kind 24113 invitations**: Send and receive invitations over Nostr relays using the `sep_inbox` hidden tag, instead of clipboard-only sharing
- **Bootstrap payload**: Include `group_id`, `group_secret`, `epoch`, `members`, and `relay_hints` in the encrypted invitation payload per NIP-XX
- **Relay management UI**: Allow users to add, remove, and prioritize relay URLs

### Phase 3: On-Chain Integration

- **Soroban contract client**: Publish commitments to the SEP-XXXX contract on Stellar testnet after each membership change
- **Commitment verification**: On receiving a group update, fetch the on-chain commitment and verify it matches the locally computed value
- **Epoch sync**: Validate that the local epoch matches the on-chain epoch before accepting membership changes
- **ZK proof generation**: Generate Groth16 membership proofs using `SEPProofGenerator` when submitting on-chain updates
- **Proof verification**: Verify received proofs locally before accepting group state transitions

### Phase 4: Production Readiness

- **Fee decoupling**: Implement the relayer pattern so the Stellar account paying transaction fees is not the group member
- **Salt distribution**: Distribute the per-epoch salt to all group members via the encrypted channel, and implement salt recovery for members who were offline
- **Key attestation flow**: UI for creating and verifying `KeyAttestation` bindings between Stellar addresses and BLS12-381 group keys
- **Group deactivation**: Allow authorized members to deactivate groups (with ZK-proof authorization as defined in SEP-XXXX)
- **Push notifications**: Notify users of new messages when the app is backgrounded
- **Multi-device support**: Sync group state and keys across devices
