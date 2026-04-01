# Stellar Chat — Android

A demo Android chat app using **Nostr** relays with **AES-256-GCM** encrypted group messaging, built with Jetpack Compose.

## Build & Run

### Prerequisites
- Android Studio Ladybug (2024.2) or later
- JDK 17+
- Android SDK 35

### Steps
1. Open `clients/android/StellarChat` in Android Studio
2. Sync Gradle
3. Run on emulator (API 26+) or device

```bash
cd clients/android/StellarChat
./gradlew assembleDebug
```

## Architecture

```
com.stellarmls.chat/
├── model/          ChatGroup, ChatMessage, InviteCode
├── crypto/         GroupCrypto (AES-256-GCM, HKDF), KeyManager, NostrEventBuilder
├── nostr/          NostrRelayConnection (OkHttp WebSocket), NostrMessageTransport
├── viewmodel/      GroupListViewModel, ChatViewModel, Create/JoinGroupViewModel
├── ui/
│   ├── screens/    GroupList, Chat, CreateGroup, JoinGroup, Settings
│   └── theme/      Material 3 theme
└── MainActivity.kt Navigation host
```

## Protocol

| Item | Value |
|------|-------|
| Transport | Nostr NIP-01 WebSocket |
| Message Kind | 24114 |
| Invitation Kind | 24113 |
| Encryption | AES-256-GCM |
| Key Derivation | HKDF-SHA256 (`sep-msg-key-v1`) |
| Topic Derivation | `SHA256("sep-topic-v1" \|\| groupSecret).hex[:16]` |
| Relays | `wss://relay.damus.io`, `wss://nos.lol` |

## Interoperability

This app uses the same cryptographic algorithms and envelope format as the iOS client (`clients/ios/`). Users on both platforms can:

- Create groups and share invite codes (base64 JSON)
- Exchange encrypted messages via the same Nostr relays
- Derive identical topic tags and encryption keys from the same group secret

## Limitations

- **Signing**: Uses HMAC-SHA256 (simplified) instead of secp256k1 Schnorr. Public relays forward custom-kind events without signature verification.
- **Key storage**: SharedPreferences (hex-encoded). Production should use Android Keystore.
- **No persistence**: Messages and groups are in-memory only. Restarting the app clears state.
- **No ZK proofs**: The Rust ZK proving system is not integrated into the mobile demo.
