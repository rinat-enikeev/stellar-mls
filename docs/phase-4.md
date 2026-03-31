# Phase 4: Nostr Relay Layer — Explained Simply

## What does it do?

Phase 4 adds the **transport layer** — the part that actually moves invitations and messages between devices. It uses Nostr relays as dumb message buses while keeping all group authority on Stellar.

Think of it as a postal service:
- You write a **sealed letter** (encrypted invitation) addressed to someone's **hidden mailbox** (inbox tag)
- You drop copies at **multiple post offices** (Nostr relays) for redundancy
- The post offices can't read the letter, don't know what group it's about, and don't store it forever
- The recipient picks it up, opens it, and **verifies the contents against the blockchain** before trusting anything

---

## Why Nostr?

Blockchains are great for consensus but terrible for chatting. You can't put real-time group messages on Stellar — it's too slow, too expensive, and too public.

Nostr gives you:
- A simple event protocol (JSON + WebSockets)
- Global relay interoperability (no single server)
- Multi-relay fanout (drop a message at 5 relays, recipient checks any of them)
- No requirement that relays understand your application

The key insight: **relays are dumb pipes**. They forward bytes. Your client decides what's valid.

---

## The three-layer cake

```
┌─────────────────────────────────────────────┐
│  Nostr Relays (Phase 4)                     │
│  Transport only. Dumb. Best-effort.         │
│  Moves encrypted bytes between devices.     │
├─────────────────────────────────────────────┤
│  Stellar + SEP Contract (Phase 3)           │
│  Authoritative group state. Epochs.         │
│  Commitments. ZK proof verification.        │
├─────────────────────────────────────────────┤
│  Circuits + Ceremony (Phases 1–2)           │
│  Groth16 proofs. Poseidon Merkle trees.     │
│  Powers of Tau ceremony.                    │
└─────────────────────────────────────────────┘
```

Each layer has a single job. Nostr never validates group state. Stellar never carries messages. The circuits never touch the network.

---

## How an invitation works

This is the core flow — inviting someone to a private group:

```
Sender (iOS/macOS)                          Nostr Relays
──────────────────                          ────────────

1. Build bootstrap payload:
   { groupID, epoch, contractID,
     relayHints, welcomePayload,
     sepBootstrapMaterial }

2. JSON-encode it → plaintext

3. Derive hidden inbox tag for
   recipient (opaque to relays)

4. Encrypt plaintext → sealed envelope
   (ephemeral key + nonce + ciphertext
    + auth tag)

5. Base64-encode the envelope → content

6. Build Nostr event:
   tags: [["sep_inbox", tag],
          ["sep_version", "1"]]
   kind: 24113
   content: base64 envelope

7. Compute event ID:
   SHA256([0, pubkey, created_at,
           kind, tags, content])

8. Sign event ID with Schnorr
   (secp256k1, via Rust k256)

9. Publish to relay A ──────────────────▶  Relay A stores event
   Publish to relay B ──────────────────▶  Relay B stores event
   (concurrent, best-effort)               (may drop it later)
```

The recipient subscribes to their hidden inbox tag on any relay, decrypts the envelope, and verifies the group state against Stellar before accepting.

---

## What was built

### Rust core (`src/ffi.rs`)

Two new FFI functions using the `k256` crate (secp256k1 with Schnorr):

| Function | Input | Output | Purpose |
|----------|-------|--------|---------|
| `sep_nostr_derive_public_key` | 32-byte secret key | 32-byte x-only public key | Nostr identity |
| `sep_nostr_sign_event_id` | 32-byte secret key + 32-byte event ID | 64-byte Schnorr signature | NIP-01 event signing |

These are exposed through the C header (`sep_ffi.h`) and called from Swift via the `RustBridge`.

**Why Rust for signing?** Nostr uses secp256k1 Schnorr signatures. Rather than adding a separate Swift crypto library, the signing goes through the same Rust FFI bridge that handles BLS12-381 operations. One static library, one trust boundary.

### Swift SDK (`swift-mls/Sources/SwiftMLS/`)

Six new or modified files:

```
SwiftMLS/
├── NostrTypes.swift          ← All types + protocols
├── NostrCrypto.swift         ← RustBackedNostrSigner
├── NostrClient.swift         ← WebSocket relay transport
├── InvitationSender.swift    ← End-to-end invitation assembly
├── RustBridge.swift          ← FFI wrappers (extended)
└── Errors.swift              ← Nostr error cases (extended)
```

---

## The types

### Bootstrap payload — what gets encrypted

```swift
SEPInvitationBootstrap
├── groupID: Data              // which group
├── epoch: UInt64              // current epoch on Stellar
├── stellarContractID: String  // Soroban contract address
├── relayHints: [URL]          // advisory relay list
├── welcomePayload: Data       // MLS Welcome-like material
└── sepBootstrapMaterial: Data // SEP salt / bootstrap data
```

This is the sensitive stuff — it tells the recipient everything they need to join. It's JSON-encoded, encrypted to the recipient's public key, and never visible to relays.

### Sealed envelope — the encryption wrapper

```swift
SEPSealedInvitationEnvelope
├── version: UInt32            // envelope format version
├── scheme: String             // encryption algorithm name
├── ephemeralPublicKey: Data?  // ECDH ephemeral key
├── nonce: Data?               // IV for symmetric cipher
├── ciphertext: Data           // encrypted bootstrap
└── authenticationTag: Data?   // AEAD tag
```

This envelope is base64-encoded and stuffed into the Nostr event's `content` field.

### Nostr event — what goes on the wire

```swift
SEPNostrEvent
├── id: String       // SHA256 of canonical JSON (hex, 64 chars)
├── pubkey: String   // sender's Nostr public key (hex, 64 chars)
├── createdAt: Int64 // Unix timestamp
├── kind: Int        // 24113 for SEP invitations
├── tags: [[String]] // [["sep_inbox", tag], ["sep_version", "1"]]
├── content: String  // base64 sealed envelope
└── sig: String      // Schnorr signature (hex, 128 chars)
```

The event ID follows [NIP-01](https://github.com/nostr-protocol/nips/blob/master/01.md): `SHA256([0, pubkey, created_at, kind, tags, content])`.

---

## The three protocols (injection points)

The invitation sender doesn't hardcode crypto or transport. Instead, three protocols let you swap implementations:

### 1. `SEPNostrEventSigner` — who signs the event

```swift
protocol SEPNostrEventSigner {
    func publicKey() throws -> Data       // 32 bytes
    func signEventID(_ eventID: Data) throws -> Data  // 64 bytes
}
```

**Concrete implementation:** `RustBackedNostrSigner` — uses `k256` Schnorr signing through the Rust FFI bridge.

### 2. `SEPInvitationCryptoProvider` — how the invitation is encrypted

```swift
protocol SEPInvitationCryptoProvider {
    func hiddenInboxTag(recipientPublicKey: Data) throws -> String
    func sealInvitation(_ plaintext: Data, recipientPublicKey: Data) throws -> SEPSealedInvitationEnvelope
}
```

**No concrete implementation shipped.** This is intentionally injected — the encryption scheme (ECDH + ChaCha20, NIP-44, etc.) is an application decision. The SDK provides the type structure; you provide the crypto.

### 3. `SEPNostrRelayTransport` — how events reach relays

```swift
protocol SEPNostrRelayTransport {
    func publish(event: SEPNostrEvent, to relayURL: URL) async throws -> SEPNostrRelaySendResult
}
```

**Concrete implementation:** `URLSessionSEPNostrRelayTransport` — ephemeral WebSocket connections, NIP-01 `["EVENT", {...}]` framing, parses `["OK", ...]` / `["NOTICE", ...]` responses.

---

## How the WebSocket transport works

```
Client                              Relay (wss://relay.example)
──────                              ─────

1. Open WebSocket ──────────────▶   Accept connection

2. Send frame:
   ["EVENT", {
     "id": "abc...",
     "pubkey": "def...",
     "created_at": 1717171717,
     "kind": 24113,
     "tags": [["sep_inbox","..."]],
     "content": "base64...",
     "sig": "ghi..."
   }]                ───────────▶   Store event (best-effort)

3. Receive response:
   ["OK", "abc...", true, ""]  ◀──  Acknowledge

4. Close WebSocket ─────────────▶   Done
```

Key details:
- Ephemeral `URLSession` (no cookies, no cache, no tracking)
- Only `ws://` and `wss://` schemes accepted
- One WebSocket per relay per publish (open → send → receive → close)
- Relay responses: `["OK", event_id, accepted, message?]` or `["NOTICE", message?]`
- All relays published to concurrently via Swift structured concurrency

---

## Concurrent relay fanout

When you publish to multiple relays, they all run in parallel:

```swift
let relayResults = await withTaskGroup(of: SEPNostrRelaySendResult.self) { group in
    for relayURL in relayURLs {
        group.addTask {
            // Each relay is independent — one failing doesn't block others
            try await relayTransport.publish(event: event, to: relayURL)
        }
    }
    // Collect all results
}
```

Each result tells you:
- Which relay (`relayURL`)
- Whether it accepted (`accepted: Bool`)
- Any message from the relay (`message: String?`)

A failed relay returns `accepted: false` with the error message — it doesn't throw or stop the others.

---

## Error handling

| Error | When |
|-------|------|
| `emptyRelayList` | No relay URLs provided |
| `invalidRelayURL` | URL scheme is not `ws` or `wss` |
| `invalidRelayResponse` | Relay sent unparseable or mismatched response |
| `invalidNostrSecretKeyLength` | Secret key is not 32 bytes |
| `invalidNostrPublicKeyLength` | Public key is not 32 bytes |
| `invalidNostrEventIDLength` | Event ID is not 32 bytes |
| `invalidNostrSignatureLength` | Signature is not 64 bytes |
| `ffiFailure` | Rust/k256 error propagated through FFI |

All errors are `Equatable` and `Sendable` — safe for async/await and actor contexts.

---

## What the relay sees vs. what it doesn't

| Visible to relay | Hidden from relay |
|---|---|
| Sender's Nostr public key | Group ID |
| Publication timestamp | Who the recipient is |
| Event size | Member list |
| Hidden inbox tag (opaque string) | Bootstrap payload contents |
| Event kind (24113) | Epoch, contract ID, salt |
| Relay fanout pattern | MLS Welcome material |

The inbox tag is **opaque** — derived from the recipient's secret material. The relay can match events to subscribers but can't reverse-engineer the recipient identity or the group relationship.

---

## Usage example (Swift)

### Sending an invitation

```swift
// 1. Create the bootstrap payload
let bootstrap = SEPInvitationBootstrap(
    groupID: myGroupID,
    epoch: currentEpoch,
    stellarContractID: "CABCDEF...",
    relayHints: [URL(string: "wss://relay.example")!],
    welcomePayload: mlsWelcome,
    sepBootstrapMaterial: saltAndKeys
)

// 2. Set up the signer (Rust-backed Schnorr)
let signer = try RustBackedNostrSigner(secretKey: myNostrSecretKey)

// 3. Send to multiple relays
let result = try await SEPInvitationSender.sendInvitation(
    bootstrap: bootstrap,
    recipientPublicKey: recipientNostrPubkey,
    relayURLs: [
        URL(string: "wss://relay-a.example")!,
        URL(string: "wss://relay-b.example")!,
    ],
    cryptoProvider: myCryptoProvider,  // you implement this
    signer: signer
)

// 4. Check results
for relay in result.relayResults {
    print("\(relay.relayURL): accepted=\(relay.accepted)")
}
```

### Deriving a Nostr public key

```swift
let publicKey = try SEPCommitmentBuilder.computePublicKey(secretKey: nostrSecretKeyData)
// 32 bytes — this is your Nostr identity
```

---

## Security properties

### What Phase 4 guarantees

1. **Payload confidentiality**: Invitation contents are encrypted end-to-end. Relays see ciphertext only.
2. **Sender authenticity at transport layer**: Events are Schnorr-signed. Relays and recipients can verify the sender's Nostr key.
3. **Recipient privacy**: Hidden inbox tags prevent relays from learning who an invitation is for.
4. **No single relay dependency**: Multi-relay fanout means no single point of failure or surveillance.

### What Phase 4 does NOT guarantee

1. **Sender anonymity**: The sender's Nostr public key is visible in every event. Use a per-session ephemeral key if this matters.
2. **Traffic analysis resistance**: Timing, event size, and activity bursts are observable. Optional padding and dummy traffic can help but are not included by default.
3. **Durable delivery**: Relays may drop events. Senders may need to rebroadcast.
4. **Relay honesty**: A relay could silently drop events. Multi-relay fanout is the mitigation.

---

## File map

```
Rust core:
  Cargo.toml                          ← k256 = "0.13" (schnorr feature)
  src/ffi.rs                          ← sep_nostr_derive_public_key, sep_nostr_sign_event_id

Swift SDK:
  swift-mls/Package.swift             ← .iOS(.v15), .macOS(.v13)
  swift-mls/Sources/CSEPMLSFFI/
    include/sep_ffi.h                 ← C header for Nostr FFI functions
  swift-mls/Sources/SwiftMLS/
    NostrTypes.swift                  ← Types + protocols (141 lines)
    NostrCrypto.swift                 ← RustBackedNostrSigner (23 lines)
    NostrClient.swift                 ← URLSessionSEPNostrRelayTransport (103 lines)
    InvitationSender.swift            ← SEPInvitationSender (92 lines)
    RustBridge.swift                  ← deriveNostrPublicKey, signNostrEventID
    Errors.swift                      ← 7 new Nostr error cases
  swift-mls/Tests/SwiftMLSTests/
    SwiftMLSTests.swift               ← 3 Nostr tests + mock implementations

Design:
  docs/relay-design-doc.md            ← Full architecture document
```

---

## How it connects to the other phases

| Phase | Role | Phase 4 interaction |
|-------|------|---------------------|
| Phase 1 (Circuits) | ZK proof generation | Bootstrap material includes the proving context |
| Phase 2 (Ceremony) | Trusted setup | Proving keys distributed to members offline |
| Phase 3 (Contract) | On-chain group state | Recipient verifies invitation against contract state |
| **Phase 4 (Relay)** | **Transport** | **Moves encrypted invitations between devices** |

The invitation carries everything a new member needs to join: group ID, epoch, contract address, MLS Welcome, and SEP bootstrap material. But the recipient doesn't trust any of it until they check the Stellar contract. The relay is just the messenger.
