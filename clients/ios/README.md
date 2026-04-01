# StellarChat iOS

Private group chat over Nostr relays, demonstrating the stellar-mls privacy architecture.

## Features

- Create encrypted chat groups with shared secrets
- Invite members via shareable invite codes
- End-to-end encrypted messages (AES-256-GCM)
- Hidden group topics (relays can't see which group a message belongs to)
- Multiple relay support with concurrent publishing
- NIP-01 compliant event format (kinds 24113/24114)

## Build & Run

```bash
# Open in Xcode
open clients/ios/StellarChat/StellarChat.xcodeproj

# Select an iPhone simulator, then Build & Run (Cmd+R)
```

## Architecture

```
Sender                      Nostr Relay                   Recipient
------                      -----------                   ---------
1. Encrypt message          3. Stores event               5. Filter by sep_topic
   (AES-256-GCM)               (opaque to relay)          6. Decrypt message
2. Build kind 24114 event   4. Forward to subscribers      7. Display in chat
   with hidden sep_topic
```

The relay sees:
- Sender's public key
- A hidden topic tag (opaque 16-char hex string)
- Base64-encoded encrypted ciphertext
- Event timestamp

The relay does NOT see:
- Group ID
- Message content
- Group membership
- Who the recipients are

## Protocol Details

- **Message events**: Kind 24114 with `["sep_topic", <hidden_topic>]` tag
- **Invitation events**: Kind 24113 with `["sep_inbox", <hidden_inbox>]` tag
- **Encryption**: AES-256-GCM with HKDF-SHA256 key derivation
- **Topic derivation**: `SHA256("sep-topic-v1" || group_secret).hex().prefix(16)`
- **Signing**: Simplified HMAC-based signatures for demo (production uses secp256k1 Schnorr via Rust bridge)

## Interoperability

This app is wire-compatible with the Android StellarChat app. Both use identical:
- Event kinds (24113, 24114)
- Tag structure (`sep_topic`, `sep_version`)
- Sealed envelope JSON format
- Hidden topic derivation
- AES-256-GCM encryption with HKDF key derivation
- Invite code format (base64 JSON)
