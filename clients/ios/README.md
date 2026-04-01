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
    │   ├── KeyManager.swift        # Triple-key Keychain (secp256k1 + BLS + Ed25519 + X25519)
    │   ├── KeyAttestation.swift    # BLS ↔ Stellar Ed25519 binding (SEP-XXXX §1.1)
    │   ├── ChatGroup.swift         # Group model, SEP membership, InviteCode
    │   ├── BootstrapPayload.swift  # NIP-XX invitation payload + PendingInvitation
    │   ├── GroupCrypto.swift       # AES-256-GCM, HKDF, X25519 ECDH invitation crypto
    │   ├── OnChainService.swift    # ZK proof generation + Soroban contract client
    │   ├── StellarStrKey.swift     # Stellar StrKey encoding (G... account IDs)
    │   ├── StorageEncryption.swift # HKDF-derived AES-256-GCM field encryption
    │   ├── PersistedModels.swift   # SwiftData @Model classes (encrypted fields)
    │   └── PersistenceStore.swift  # SwiftData store with FileProtectionType.complete
    ├── Nostr/
    │   ├── NostrEvent.swift            # NIP-01 event builder with Schnorr signing
    │   ├── NostrRelayConnection.swift  # WebSocket relay (actor)
    │   ├── NostrMessageTransport.swift # Multi-relay message orchestration
    │   └── InvitationTransport.swift   # Kind 24113 invitation send/receive
    ├── ViewModels/
    │   └── ChatViewModel.swift   # Per-group message state, deduplication
    └── Views/
        ├── ContentView.swift
        ├── GroupListView.swift        # Group list + on-chain status badges
        ├── ChatView.swift
        ├── CreateGroupView.swift      # Group creation + on-chain publishing
        ├── JoinGroupView.swift
        ├── InviteMemberView.swift     # Send invitation via Nostr
        ├── PendingInvitationsView.swift # View/accept with on-chain verification
        └── SettingsView.swift         # Keys, relay, contract configuration
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
| Member sorting by compressed G1 key in `addMember()` | SEP-XXXX §2.1 | Implemented |
| Independent secp256k1 and BLS12-381 keypairs | SEP-XXXX §1.1 | Implemented |
| Stellar Ed25519 key (HKDF-derived from Nostr key) | SEP-XXXX §1.1 | Implemented |
| Key attestation (Ed25519 signature binding BLS to Stellar key) | SEP-XXXX §1.1 | Implemented |
| Message persistence (SwiftData + field-level AES-256-GCM) | App-level | Implemented |
| Group persistence (SwiftData + field-level AES-256-GCM) | App-level | Implemented |
| At-rest file protection (`FileProtectionType.complete`) | App-level | Implemented |
| Error surfacing (encryption, relay, decryption failures) | App-level | Implemented |
| Kind 24113 invitation events with `sep_inbox` hidden tag | NIP-XX | Implemented |
| Bootstrap payload (group state, members, epoch, salt in invitation) | NIP-XX | Implemented |
| X25519 ECDH + AES-256-GCM invitation encryption | NIP-XX | Implemented |
| Inbox key derivation (X25519 from Nostr key via HKDF) | NIP-XX | Implemented |
| Inbox subscription (auto-listen for incoming invitations) | NIP-XX | Implemented |
| Relay management UI (add, remove, reorder, persist) | App-level | Implemented |
| Soroban contract client (`SEPContractClient` integration) | SEP-XXXX | Implemented |
| Groth16 ZK proof generation (`SEPProofGenerator` + `OnChainService`) | SEP-XXXX | Implemented |
| Commitment publishing on group creation | SEP-XXXX | Implemented |
| Commitment update publishing on membership change | SEP-XXXX | Implemented |
| On-chain commitment verification (Poseidon commitment comparison) | SEP-XXXX | Implemented |
| Epoch sync validation (local vs on-chain epoch check) | SEP-XXXX | Implemented |
| On-chain membership proof verification (`verify_membership`) | SEP-XXXX | Implemented |
| Invitation acceptance with on-chain verification | NIP-XX + SEP-XXXX | Implemented |
| Contract configuration UI (endpoint, contract ID) | App-level | Implemented |

### What's Not Yet Implemented

| Feature | Spec | Notes |
|---------|------|-------|
| Fee decoupling / relayer pattern | SEP-XXXX | Transaction submitter identity is visible on-chain |
| Salt distribution and recovery | SEP-XXXX | Salt generated locally but not shared with other members |
| Group deactivation | SEP-XXXX | No mechanism to deactivate or archive groups |
| Key attestation distribution | SEP-XXXX | Attestations created locally but not shared with group |
| Push notifications | App-level | No background notification support |
| Multi-device sync | App-level | Keys and state are device-local |

### Known Design Deviations

1. **Testing proving keys**: The app uses `generateTestingProvingKey` for ZK proof generation. For production, proving keys from a multi-party trusted setup ceremony should be bundled or downloaded.

2. **Derived Stellar key**: The Ed25519 Stellar key is deterministically derived from the Nostr secp256k1 secret via HKDF (`info: "stellar-ed25519-v1"`). This means Nostr key compromise implies Stellar key compromise. Acceptable for group state anchoring; for production with significant on-chain value, use an independent master seed.

3. **Contract transport**: The Soroban contract client uses HTTP transport (`URLSessionSEPContractTransport`). For production, direct Soroban RPC or a fee-decoupled relayer pattern should be used.

## Interoperability

This app is wire-compatible with the Android StellarChat app. Both use identical:

- Event kinds (24113, 24114)
- Tag structure (`sep_topic`, `sep_version`)
- Sealed envelope JSON format (`version`, `scheme`, `ephemeral_public_key`, `nonce`, `ciphertext`, `authentication_tag`)
- Hidden topic derivation (`SHA256("sep-topic-v1" || secret).hex().prefix(16)`)
- AES-256-GCM encryption with HKDF-SHA256 key derivation
- Invite code format (base64 JSON with `groupID`, `groupSecret`, `name`, `relayHints`)
- Kind 24113 invitation events with `sep_inbox` hidden tag and X25519 ECDH envelope encryption
- Hidden inbox derivation (`SHA256("sep-inbox-v1" || x25519_pubkey).hex().prefix(16)`)
- secp256k1 Schnorr signing via the same Rust FFI library
- BLS12-381 and Poseidon commitment computation via the same Rust circuits

## Next Steps

### Phase 1: Core Improvements (completed)

- ~~**Message persistence**: SwiftData with field-level AES-256-GCM encryption + `FileProtectionType.complete`~~
- ~~**Member sorting**: Sort by compressed G1 public key per SEP-XXXX §2.1~~
- ~~**Separate key derivation**: Independent secp256k1 and BLS12-381 keypairs with `KeyAttestation`~~
- ~~**Error handling**: Errors surfaced as alerts in ChatView, relay publish failures reported~~

### Phase 2: Nostr Protocol Completion (completed)

- ~~**Kind 24113 invitations**: Send and receive invitations over Nostr relays using the `sep_inbox` hidden tag, with X25519 ECDH + AES-256-GCM encryption~~
- ~~**Bootstrap payload**: Full NIP-XX payload including `group_id`, `group_secret`, `epoch`, `members`, `salt`, `commitment`, and `relay_hints`~~
- ~~**Relay management UI**: Add, remove, and reorder relay URLs with UserDefaults persistence~~

### Phase 3: On-Chain Integration (completed)

- ~~**Soroban contract client**: `OnChainService` actor wrapping `SEPContractClient` for Stellar testnet interaction~~
- ~~**Commitment verification**: Poseidon commitment comparison against on-chain state on invitation acceptance and manual verify~~
- ~~**Epoch sync**: Local epoch validated against on-chain epoch before accepting membership changes~~
- ~~**ZK proof generation**: Groth16 proofs via `SEPProofGenerator` with cached proving keys, auto-published on group creation~~
- ~~**Proof verification**: On-chain `verify_membership` via contract, commitment update proofs against current state~~

### Phase 4: Production Readiness

- **Fee decoupling**: Implement the relayer pattern so the Stellar account paying transaction fees is not the group member
- **Salt distribution**: Distribute the per-epoch salt to all group members via the encrypted channel, and implement salt recovery for members who were offline
- **Key attestation distribution**: Share `KeyAttestation` with group members via encrypted channel; verify attestations on member join
- **Group deactivation**: Allow authorized members to deactivate groups (with ZK-proof authorization as defined in SEP-XXXX)
- **Push notifications**: Notify users of new messages when the app is backgrounded
- **Multi-device support**: Sync group state and keys across devices
