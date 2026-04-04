# Privacy-Preserving Push Notifications for StellarChat

## Preamble

```
Document: Push Notification Design
Title: Privacy-Preserving Push Notifications with Encrypted APNs Delivery
Author: @rinat-enikeev
Status: Draft
Created: 2026-04-04
Requires: SEP-XXXX, NIP-XX (Private Group Relay Transport)
Platform: iOS / Apple Push Notification service (APNs)
```

---

## Simple Summary

An extension to the StellarChat system that delivers push notifications when the app is backgrounded or closed, without revealing user identity to the push notification relay operator or Apple. Device tokens are encrypted, registration uses ephemeral X25519 keys, and notification payloads are decrypted on-device by a Notification Service Extension. The PN relay watches Nostr hidden group topics and dispatches opaque pushes — it never learns which user is in which group or what messages say.

---

## Motivation

StellarChat currently relies on persistent WebSocket connections to Nostr relays for real-time message delivery. When a user closes the app or the OS suspends it, the WebSocket disconnects and messages queue at the relay until the next foreground session. Users miss time-sensitive messages, invitations, and call signals.

Push notifications solve this by waking the app or displaying an alert, but they introduce a significant privacy risk. Traditional push architectures require the server to know:

- which device token belongs to which user
- which groups the user is in (to filter relevant events)
- the message content (to construct the notification text)

These requirements directly contradict StellarChat's privacy model, where relays are dumb, group_id never appears in cleartext, and membership is hidden behind zero-knowledge proofs.

This design eliminates all three leaks by using ephemeral-key registration, encrypted device tokens, and on-device payload decryption.

---

## Design Principles

1. **Identity separation.** The PN relay never learns the Nostr pubkey, BLS identity, or Stellar address of any subscriber. Registration uses disposable ephemeral X25519 keys.
2. **Content opacity.** The PN relay forwards encrypted SealedEnvelopes. It cannot decrypt message content — only the device, using group keys stored in the shared Keychain, can.
3. **Reuse existing cryptography.** Registration encryption reuses the `x25519-aes-256-gcm-v1` scheme from invitation encryption (`GroupCrypto.encryptInvitation`). No new crypto primitives.
4. **Relays remain dumb.** The Nostr relay (strfry) is unaware of push notifications. The PN relay is a separate service that subscribes to events like any other Nostr client.
5. **Graceful degradation.** If the Notification Service Extension cannot decrypt (locked device, memory pressure), a generic "New message" alert is shown. No content leaks on failure.
6. **Minimal metadata.** Each group subscription is cryptographically independent. The PN relay cannot correlate which subscriptions belong to the same device.

---

## Threat Model

### Adversaries

| Actor | Capability | Goal |
|-------|-----------|------|
| **PN relay operator** | Full access to PN relay server, database, logs, memory | Learn user identities, group membership, message content |
| **Apple (APNs)** | Observes device tokens, push timing, encrypted payloads | Correlate push activity with Apple ID, infer social graph |
| **Network observer** | Observes TLS-encrypted traffic between client ↔ PN relay | Correlate registration timing with user activity |
| **Nostr relay operator** | Observes all Nostr events (kinds, tags, timing) | Already addressed by NIP-XX; unchanged by this design |

### What each party learns

**PN relay operator learns (accepted):**
- A set of hidden group topics are active (opaque hex strings)
- How many subscriptions exist and their creation timestamps
- Event timing and volume per topic
- That a push was dispatched to a particular encrypted token blob

**PN relay operator does NOT learn:**
- Which Nostr pubkey owns any subscription
- Which `group_id` any topic corresponds to
- Message content (double-encrypted: group key + notification key)
- Relationships between subscriptions (each uses independent ephemeral keys)
- The plaintext APNs device token (stored encrypted, decrypted only transiently in memory)

**Apple (APNs) learns (accepted):**
- A device token received a push at a given time
- The encrypted payload size

**Apple does NOT learn:**
- Message content (encrypted, decrypted on-device by the NSE)
- Group identity or name
- The user's Nostr pubkey, BLS key, or Stellar address
- Which PN relay sent the push (only the bundle ID is visible)

### Accepted residual risks

1. **IP-based correlation.** The PN relay can observe that multiple subscription registrations arrive from the same IP address at similar timestamps. For practical privacy (self-hosted relay), this is acceptable. Stronger unlinkability requires the client to stagger registrations or use a VPN/Tor.
2. **Topic activity timing.** The PN relay sees when events arrive on each topic. A well-resourced adversary with relay access could cross-reference topic activity timing with Nostr relay observations. This is the same metadata leak documented in NIP-XX §Privacy Considerations.
3. **APNs token stability.** APNs tokens are stable per-device per-app until reinstallation. If the encrypted token storage is compromised, past topic-to-device mappings are recoverable. Mitigation: token re-encryption on each registration update.

---

## Architecture

### Components

```
+------------------+        +------------------+        +------------------+
|   iOS Client     |        |   PN Relay (DO)  |        | Nostr Relay      |
|                  |        |                  |        | (strfry)         |
|  +-------------+ |  HTTPS |  +-------------+ |   WS   |                  |
|  | AppState    |------------>| HTTP API    |<--------->|                  |
|  +-------------+ |        |  +-------------+ |        |                  |
|                  |        |  | Event       | |        |                  |
|  +-------------+ |  APNs  |  | Watcher     | |        |                  |
|  | NSE         |<------------|             | |        |                  |
|  | (decrypt)   | |        |  +-------------+ |        |                  |
|  +-------------+ |        |  | APNs Client | |        |                  |
|                  |        |  +-------------+ |        |                  |
|  +-------------+ |        |  | SQLite DB   | |        |                  |
|  | Shared      | |        |  +-------------+ |        |                  |
|  | Keychain +  | |        +------------------+        +------------------+
|  | App Group   | |
|  +-------------+ |
+------------------+
```

### Data Flow

```
1. REGISTRATION (app launch)
   Client ---[encrypted envelope]--> PN Relay
   PN Relay stores: subscription_id, filter, encrypted_token, encrypted_notification_key

2. EVENT ARRIVAL (message sent to group)
   Sender --> Nostr Relay --[kind 44114 event]--> PN Relay (Event Watcher)

3. PUSH DISPATCH
   PN Relay: match event topic → subscriptions
   PN Relay: decrypt APNs token (transiently)
   PN Relay: re-encrypt event content under notification_key
   PN Relay ---[APNs HTTP/2]--> Apple ---[push]--> Device

4. ON-DEVICE DECRYPTION
   NSE: decrypt notification_key layer → SealedEnvelope
   NSE: load group key from shared Keychain
   NSE: decrypt SealedEnvelope → plaintext message
   NSE: display "GroupName: Sender: Hello"
```

---

## Registration Protocol

### Overview

Registration uses the same `x25519-aes-256-gcm-v1` sealed envelope scheme as invitation encryption. This is deliberate: the PN relay is treated like an "invitation recipient" that receives an encrypted payload it can open, but the sender remains anonymous because the X25519 keypair is ephemeral and disposable.

### Server Info

```
GET /v1/info

Response:
{
  "version": 1,
  "x25519_public_key": "<base64: 32-byte X25519 public key>",
  "supported_platforms": ["ios"],
  "max_filters_per_subscription": 10,
  "max_subscriptions": 1000
}
```

The PN relay's X25519 public key is its long-term identity for receiving encrypted registrations. It is generated once on first startup and persisted.

### Subscribe

The client constructs a registration payload, encrypts it to the PN relay's public key, and POSTs the sealed envelope.

**Plaintext payload:**
```json
{
  "action": "subscribe",
  "subscription_id": "<32-byte random hex>",
  "filter": {
    "kinds": [44114],
    "#t": ["<hidden_group_topic>"]
  },
  "apns_token": "<hex-encoded APNs device token>",
  "notification_key": "<base64: 32-byte AES-256-GCM key>",
  "platform": "ios"
}
```

**Encrypted transport:**
```
POST /v1/subscription
Content-Type: application/json

{
  "version": 1,
  "scheme": "x25519-aes-256-gcm-v1",
  "ephemeral_public_key": "<base64: 32-byte ephemeral X25519 pubkey>",
  "nonce": "<base64: 12 bytes>",
  "ciphertext": "<base64: padded to 512 bytes>",
  "authentication_tag": "<base64: 16 bytes>"
}
```

**Server processing:**
1. Derive shared secret: `ECDH(server_x25519_private, ephemeral_public_key)`
2. Derive decryption key: `HKDF-SHA256(shared_secret, salt="sep-invitation-v1", info="aes-256-gcm", 32 bytes)`
3. Decrypt ciphertext using AES-256-GCM with the nonce and authentication tag
4. Parse the plaintext JSON
5. Re-encrypt the `apns_token` under the server's storage key (distinct from the X25519 key) for at-rest protection
6. Store: `subscription_id`, `filter`, `encrypted_apns_token`, `encrypted_notification_key`, `platform`
7. Update the aggregated Nostr relay subscription to include the new topic

**Response:**
```json
{ "status": "ok" }
```

No identifying information in the response. The `subscription_id` is the client's bearer token for future management.

### Update (Topic Rotation)

When a group's secret changes (member removal, rekey), the hidden topic rotates. The client updates its subscription:

```json
{
  "action": "update",
  "subscription_id": "<32-byte hex>",
  "filter": {
    "kinds": [44114],
    "#t": ["<new_hidden_group_topic>"]
  }
}
```

Encrypted and POSTed identically to subscribe. The `subscription_id` authenticates the request (bearer token — only the original registrant knows it).

### Unsubscribe

```json
{
  "action": "unsubscribe",
  "subscription_id": "<32-byte hex>"
}
```

The PN relay deletes the subscription record and updates the aggregated Nostr filter.

### Per-Group Independence

A client with N groups creates N independent subscriptions. Each subscription uses:
- A different ephemeral X25519 keypair (fresh per registration)
- A different random `subscription_id`
- A different random `notification_key`
- An independently encrypted APNs token (same token, different nonce each time)

The PN relay cannot cryptographically link these subscriptions to the same device. The only correlation vector is network-level (IP address, timing), which is acceptable for the self-hosted threat model.

### Invitation Inbox Notifications

To receive push notifications for new group invitations (kind 24113), the client registers an additional subscription using its hidden inbox tag:

```json
{
  "action": "subscribe",
  "subscription_id": "<32-byte random hex>",
  "filter": {
    "kinds": [24113],
    "#sep_inbox": ["<hidden_inbox_tag>"]
  },
  "apns_token": "<hex>",
  "notification_key": "<base64: 32 bytes>",
  "platform": "ios"
}
```

This enables offline invitation delivery notifications. The `hidden_inbox_tag` is derived from the user's X25519 public key (`SHA256("sep-inbox-v1" || pubkey)[0..8]`) and is already public to anyone who has the user's key bundle.

---

## Notification Delivery

### Event Detection

The PN relay maintains a single aggregated WebSocket subscription to the Nostr relay covering all registered topics:

```json
["REQ", "pn-watcher", {
  "kinds": [44114, 24113],
  "#t": ["topic_a", "topic_b", "topic_c", ...]
}]
```

When the Nostr relay filter includes `#sep_inbox` tags (for invitation subscriptions), a second subscription handles those:

```json
["REQ", "pn-inbox-watcher", {
  "kinds": [24113],
  "#sep_inbox": ["inbox_a", "inbox_b", ...]
}]
```

strfry supports up to 500 values per tag filter and 20 subscriptions per connection, allowing a single WebSocket to handle up to 10,000 topics.

### Push Construction

When an event matches a subscription:

1. **Rate limit check.** Max 1 push per subscription per 5 seconds. During active conversations, the app should be foregrounded and receiving via WebSocket — push is only needed to wake the app.

2. **Event deduplication.** Track the last 1,000 event IDs per topic. Multi-relay fanout may deliver the same event multiple times.

3. **Payload construction.** The Nostr event's `content` field (base64-encoded SealedEnvelope) is re-encrypted under the subscription's `notification_key`:

```
notification_payload = AES-256-GCM(
  key: notification_key,
  plaintext: nostr_event.content,  // base64 SealedEnvelope
  aad: nostr_event.id              // bind to specific event
)
```

4. **APNs push:**

```json
{
  "aps": {
    "mutable-content": 1,
    "alert": {
      "title": "StellarChat",
      "body": "New message"
    },
    "sound": "default"
  },
  "enc": "<base64: AES-256-GCM encrypted content>",
  "nonce": "<base64: 12 bytes>",
  "tag": "<base64: 16 bytes>",
  "event_id": "<nostr event id hex>",
  "sub_id": "<subscription_id first 8 chars>"
}
```

The `sub_id` hint (first 8 characters only) helps the NSE locate the correct `notification_key` without exposing the full bearer token. The full `subscription_id` is never transmitted in cleartext.

5. **Token handling.** Decrypt the stored APNs token in memory, send the HTTP/2 request to `api.push.apple.com`, zero the plaintext token immediately after.

### Payload Budget

APNs allows 4,096 bytes per notification payload.

| Component | Size |
|-----------|------|
| `aps` JSON overhead | ~120 bytes |
| `enc` (typical chat message SealedEnvelope) | ~200-500 bytes base64 |
| `nonce` + `tag` | ~40 bytes |
| `event_id` + `sub_id` | ~80 bytes |
| **Total typical** | **~440-740 bytes** |
| **Available headroom** | **~3,350 bytes** |

For events exceeding 3 KB (e.g., image messages with large thumbnails), the PN relay sends a wake-only notification:

```json
{
  "aps": {
    "mutable-content": 1,
    "content-available": 1,
    "alert": {
      "title": "StellarChat",
      "body": "New message"
    }
  },
  "event_id": "<nostr event id hex>",
  "sub_id": "<subscription_id first 8 chars>",
  "fetch": true
}
```

The app fetches the full event from the Nostr relay on launch.

---

## iOS Implementation

### Notification Service Extension (NSE)

A new Xcode target: `StellarChatNotificationService`

**Bundle ID:** `com.stellarmls.chat.NotificationService`

The NSE is a lightweight process that iOS launches when a push with `mutable-content: 1` arrives. It has ~30 seconds and ~24 MB of memory to modify the notification before display.

**Decryption flow:**

```
1. Receive UNNotificationContent with encrypted payload
2. Extract sub_id hint from userInfo
3. Load notification_key for this subscription from shared App Group container
4. Decrypt enc field: AES-256-GCM(notification_key, enc, nonce, tag)
   → base64-encoded SealedEnvelope (the Nostr event content)
5. Load group data from shared container (topic → group mapping)
6. Decode SealedEnvelope from base64
7. Derive group message key: HKDF(groupSecret || epoch || salt)
8. Decrypt SealedEnvelope: AES-256-GCM(group_key, ciphertext)
   → plaintext JSON (v2 message format)
9. Parse: extract "text", "senderBlsPubkey", "type"
10. Look up sender alias from shared contact store
11. Modify notification:
    title: "<group name>"
    body: "<sender alias>: <text>"
12. Call contentHandler(modifiedContent)
```

**Fallback:** If any step fails (key not found, decryption error, timeout), the NSE does not modify the notification. The user sees the generic alert: "StellarChat — New message". No content leaks on failure.

**Memory profile:** CryptoKit AES-256-GCM + HKDF operations use <1 MB. Keychain access and JSON parsing are lightweight. No ZK proof generation, no BLS operations, no WebSocket connections. Well within the 24 MB limit.

### Shared Data Architecture

The main app and NSE share data through two mechanisms:

**1. Shared Keychain (via Keychain Access Group)**

Access group: `$(TeamIdentifierPrefix)com.stellarmls.chat.shared`

Shared items:
- Storage root key (`com.stellarmls.chat.storageRootKey`)
- Nostr secret key (for future use — not needed in NSE v1)

Accessibility: `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`

This is a change from the current `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`. The NSE runs in the background when the device may be locked (but has been unlocked at least once since boot). This is standard practice for all apps with notification decryption (Signal, WhatsApp, iMessage).

**2. Shared App Group Container**

App Group: `group.com.stellarmls.chat`

The main app maintains a JSON file at the App Group container path:

```
{containerURL}/push_subscriptions.json
```

```json
{
  "subscriptions": {
    "<subscription_id>": {
      "sub_id_hint": "<first 8 chars>",
      "notification_key": "<base64: encrypted under storage root key>",
      "group_id": "<encrypted under storage root key>",
      "group_name": "<encrypted under storage root key>",
      "group_secret": "<encrypted under storage root key>",
      "epoch": 5,
      "salt": "<encrypted under storage root key>"
    }
  },
  "contacts": {
    "<bls_pubkey_base64>": "<encrypted alias>"
  }
}
```

All sensitive fields are encrypted under the storage root key (accessed via shared Keychain). The NSE decrypts them using `StorageEncryption.decrypt()`.

The main app updates this file whenever:
- A new group is joined (add subscription entry)
- A group is left (remove subscription entry)
- A rekey occurs (update epoch, salt, group_secret, notification_key; re-register with PN relay)
- A contact alias changes (update contacts map)

### Entitlements

**Main app target** (`StellarChat.entitlements`):
```xml
<key>aps-environment</key>
<string>production</string>
<key>com.apple.security.application-groups</key>
<array>
    <string>group.com.stellarmls.chat</string>
</array>
<key>keychain-access-groups</key>
<array>
    <string>$(AppIdentifierPrefix)com.stellarmls.chat.shared</string>
</array>
```

**NSE target** (`StellarChatNotificationService.entitlements`):
```xml
<key>com.apple.security.application-groups</key>
<array>
    <string>group.com.stellarmls.chat</string>
</array>
<key>keychain-access-groups</key>
<array>
    <string>$(AppIdentifierPrefix)com.stellarmls.chat.shared</string>
</array>
```

**Info.plist** (main app):
```xml
<key>UIBackgroundModes</key>
<array>
    <string>voip</string>
    <string>remote-notification</string>
</array>
```

### Registration Lifecycle

Push registration integrates into the existing `AppState` lifecycle:

```
App Launch
  ├── Request notification permission (UNUserNotificationCenter)
  ├── Register for remote notifications (UIApplication)
  ├── Receive APNs device token (AppDelegate callback)
  ├── Fetch PN relay info (GET /v1/info → X25519 pubkey)
  └── For each group:
        ├── Generate ephemeral X25519 key, subscription_id, notification_key
        ├── Encrypt registration payload to PN relay pubkey
        ├── POST /v1/subscription
        └── Store subscription_id + notification_key in shared container

Group Join
  └── Register new subscription with PN relay

Group Leave
  └── Unsubscribe (POST with action: "unsubscribe")

Rekey / Epoch Change
  ├── Update subscription filter with new hidden topic
  ├── Update shared container with new group key material
  └── POST /v1/subscription with action: "update"

APNs Token Refresh
  └── Re-register all subscriptions with new token
```

---

## PN Relay Server

### Technology

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Language | Rust | Consistent with existing relayer |
| HTTP framework | axum | Already used in the relayer |
| Nostr client | tokio-tungstenite | WebSocket to strfry |
| APNs client | HTTP/2 via hyper + rustls | Direct to api.push.apple.com |
| Crypto | x25519-dalek, aes-gcm | Same algorithms as client |
| Database | SQLite via rusqlite | Lightweight, file-based |

### API

```
GET  /v1/info           → PN relay public key and metadata
POST /v1/subscription   → encrypted registration envelope
GET  /v1/health         → health check for monitoring
```

No authentication on endpoints. The subscription endpoint accepts only encrypted envelopes — without the PN relay's X25519 private key, the payload is useless. The `subscription_id` inside the envelope serves as authorization for updates and deletes.

### Database Schema

```sql
CREATE TABLE subscriptions (
  subscription_id    TEXT PRIMARY KEY,
  filter_json        TEXT NOT NULL,
  encrypted_token    BLOB NOT NULL,       -- APNs token encrypted under server storage key
  encrypted_notif_key BLOB NOT NULL,      -- notification_key encrypted under server storage key
  platform           TEXT NOT NULL DEFAULT 'ios',
  created_at         INTEGER NOT NULL,
  last_pushed_at     INTEGER DEFAULT 0
);

CREATE INDEX idx_subscriptions_filter ON subscriptions(filter_json);
```

The PN relay uses a separate server-side storage key (AES-256-GCM, generated on first startup, stored in a local file or environment variable) to encrypt APNs tokens and notification keys at rest. The X25519 private key is used only for decrypting incoming registration envelopes, not for storage encryption.

### Event Processing Pipeline

```
Nostr Relay (strfry)
       │
  WebSocket subscription
  (aggregated filter: all registered topics)
       │
       ▼
Event Router
       │
  For each matching subscription:
       │
       ├── Rate limit check (1 push / 5s / subscription)
       ├── Event dedup check (last 1000 event IDs)
       ├── Decrypt APNs token (transiently, from server storage key)
       ├── Decrypt notification_key (transiently)
       ├── Re-encrypt event content under notification_key
       ├── Send APNs HTTP/2 request
       ├── Zero plaintext token and key from memory
       └── Update last_pushed_at
```

### APNs Configuration

The PN relay authenticates with APNs using token-based authentication (`.p8` key from Apple Developer portal):

```
APNS_KEY_PATH=/app/apns-key.p8
APNS_KEY_ID=ABCDEF1234
APNS_TEAM_ID=TEAMID1234
APNS_BUNDLE_ID=com.stellarmls.chat
APNS_ENVIRONMENT=production
```

### Docker Integration

New service in `docker-compose.yml`:

```yaml
pn-relay:
  build:
    context: ./pn-relay
    dockerfile: Dockerfile
  environment:
    - PN_BIND=0.0.0.0:8090
    - PN_NOSTR_RELAY=ws://nostr-relay:7777
    - APNS_ENVIRONMENT=production
  env_file: ./pn-relay/.env
  volumes:
    - pn-relay-data:/app/data
    - ./pn-relay/apns-key.p8:/app/apns-key.p8:ro
  restart: unless-stopped
  networks:
    - internal
  depends_on:
    - nostr-relay
```

Nginx reverse proxy exposes `push.${DOMAIN}` with TLS (matching existing certbot setup).

### Subscription Garbage Collection

Subscriptions are cleaned up when:
- The client explicitly unsubscribes
- APNs returns a `410 Gone` response (token invalidated — app uninstalled)
- No push has been sent in 30 days (the subscription's topic has been inactive)

On `410 Gone`, the PN relay deletes the subscription immediately and removes the topic from the aggregated filter if no other subscriptions reference it.

---

## Privacy Analysis

### Registration Privacy

| Property | Guarantee | Mechanism |
|----------|-----------|-----------|
| Sender anonymity | PN relay cannot identify registrant | Ephemeral X25519 key, no NIP-98 auth |
| Token confidentiality | APNs token never stored in plaintext | Encrypted under server storage key |
| Subscription unlinkability | Different subscriptions from same device are cryptographically independent | Different ephemeral keys, subscription IDs, notification keys, nonces |
| Forward secrecy | Compromise of PN relay's X25519 key does not expose past registrations | Each registration uses a fresh ephemeral key; past shared secrets are not recoverable from the server's long-term key alone without the ephemeral private key (which the client discards) |

### Delivery Privacy

| Property | Guarantee | Mechanism |
|----------|-----------|-----------|
| Content confidentiality (vs PN relay) | PN relay cannot read message content | SealedEnvelope encrypted under group key (which PN relay does not have) |
| Content confidentiality (vs Apple) | Apple cannot read message content | Payload re-encrypted under notification_key; decrypted on-device by NSE |
| Group identity confidentiality | Neither PN relay nor Apple knows which group a push belongs to | Hidden topic is opaque hex; group_id is inside ciphertext |
| Sender identity confidentiality | Neither party knows who sent the message | senderBlsPubkey is inside the encrypted SealedEnvelope |

### Comparison with Existing Nostr Push Implementations

| Property | Damus/NotePush | Amethyst | This Design |
|----------|---------------|----------|-------------|
| Server knows user pubkey | Yes (NIP-98 auth) | Yes | No |
| Server can read content | No (encrypted) | Partial | No |
| Device token stored plaintext | Yes | Yes | No (encrypted at rest) |
| Cross-subscription linkability | Yes (same pubkey) | Yes | No (independent ephemeral keys) |
| Auth mechanism | NIP-98 (identity-linked) | NIP-98 | Bearer token (subscription_id) |

---

## Security Considerations

### Subscription ID as Bearer Token

The `subscription_id` is a 32-byte random value that serves as both identifier and authorization. Knowledge of the `subscription_id` allows updating or deleting the subscription. This is acceptable because:
- It is generated by the client using `SecRandomCopyBytes`
- It is transmitted only inside encrypted envelopes (never in cleartext)
- It is stored only on the client device (shared container) and the PN relay database
- Brute-forcing a 256-bit random value is computationally infeasible

### Replay Protection

An attacker who captures an encrypted registration envelope cannot replay it meaningfully:
- Replaying a subscribe creates a duplicate subscription (same `subscription_id`), which the PN relay handles as an upsert — no harm
- Replaying an unsubscribe deletes the subscription, which is a denial-of-service vector, but requires capturing the envelope (TLS protects in transit)

For stronger replay protection, the PN relay can reject envelopes with `subscription_id` values it has seen before for unsubscribe actions.

### Server Key Compromise

If the PN relay's X25519 private key is compromised:
- **Future registrations** can be decrypted by the attacker (active MITM or passive capture)
- **Past registrations** are safe if the ephemeral private keys have been discarded by clients (which they are, immediately after registration)
- **Mitigation**: Key rotation. The PN relay generates a new X25519 keypair periodically. Clients re-register with the new key on next app launch.

### Keychain Accessibility Change

Changing from `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` to `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` means keys are available after the first unlock since boot, even if the device re-locks. This is required for NSE functionality and is the same accessibility level used by Signal, WhatsApp, and Telegram for notification decryption. The Secure Enclave still protects the keys; they are not extractable without the device passcode.

### APNs Token Rotation

When APNs issues a new device token (app reinstall, OS update), the client must re-register all subscriptions with the new token. The old encrypted tokens become stale. The PN relay detects this via APNs error responses (`BadDeviceToken`) and marks the subscription for cleanup.

### Notification Content Fallback

If the NSE fails to decrypt, the user sees "StellarChat — New message". This reveals only that:
- The user has StellarChat installed
- A push was received

It does not reveal the group, sender, or content. The same information is already visible to Apple via the APNs push itself.

---

## NIP Sketch: Privacy-Preserving Push Notification Relay

This section outlines a NIP proposal that could standardize the registration protocol for interoperability across Nostr clients.

### Event Watcher Registration

```
NIP: XX
Title: Privacy-Preserving Push Notification Subscriptions
Status: Draft
Requires: NIP-01, NIP-44
```

**Abstract:** Defines a protocol for clients to register push notification subscriptions with event-watching servers using ephemeral X25519 key agreement. The server monitors Nostr relays for events matching client-specified filters and dispatches platform push notifications with encrypted payloads. The protocol ensures the server cannot identify subscribers or correlate subscriptions.

**Key differences from the Event Watcher API (PR #1528):**
- No NIP-98 authentication (identity-revealing)
- Ephemeral X25519 keys instead of stable Nostr identity keys
- Bearer-token authorization via random subscription_id
- Per-subscription notification encryption key for payload confidentiality
- No NIP-59 Gift Wrap dependency (simpler; uses direct X25519 ECDH)

**Registration envelope format:** Identical to the SealedEnvelope format defined in NIP-XX (Private Group Relay Transport), using scheme `x25519-aes-256-gcm-v1`.

**Server info endpoint:** `GET /v1/info` returns the server's X25519 public key and supported platforms.

**Subscription management:** `POST /v1/subscription` accepts encrypted envelopes with `action` field: `subscribe`, `update`, or `unsubscribe`.

---

## Implementation Phases

### Phase 1: iOS Push Infrastructure

- Add Push Notifications capability and entitlements to main app target
- Add `remote-notification` background mode to `Info.plist`
- Create Notification Service Extension target (`StellarChatNotificationService`)
- Configure App Group (`group.com.stellarmls.chat`) on both targets
- Configure Keychain Access Group on both targets
- Modify `StorageEncryption` for shared Keychain accessibility
- Implement shared data store (`push_subscriptions.json` in App Group container)
- Implement APNs token registration in `AppState`

### Phase 2: PN Relay Server

- Create `pn-relay/` Rust project with axum
- Implement X25519 key generation and `/v1/info` endpoint
- Implement `/v1/subscription` endpoint (decrypt envelope, store subscription)
- Implement Nostr event watcher (WebSocket to strfry, aggregated filter)
- Implement APNs HTTP/2 dispatch with `.p8` token auth
- Implement rate limiting and event deduplication
- Add Docker service and nginx upstream
- Deploy to DigitalOcean

### Phase 3: Client Registration Protocol

- Implement `PNRelayClient` in iOS app (encrypt registration, POST envelope)
- Wire registration into `AppState` lifecycle (launch, join, leave, rekey)
- Implement subscription update on topic rotation
- Implement APNs token refresh handling
- Store subscription state in shared App Group container

### Phase 4: On-Device Decryption

- Implement `NotificationService.swift` (decrypt notification_key layer, decrypt SealedEnvelope, resolve sender, modify notification)
- Handle edge cases (large payloads, decryption failure, epoch transitions)
- Test end-to-end flow

### Phase 5: Hardening

- End-to-end testing (registration, delivery, decryption, rotation, multi-group)
- APNs sandbox and production testing
- Privacy audit (no plaintext tokens persisted, ephemeral keys discarded, subscription unlinkability)
- Subscription garbage collection (410 Gone, 30-day inactivity)
- Monitoring and alerting for PN relay health

---

## References

- NIP-XX: Private Group Relay Transport Anchored on Stellar SEP State (this project)
- SEP-XXXX: Private Group Membership on Stellar
- [Push Notification Event Watcher API (PR #1528)](https://github.com/nostr-protocol/nips/pull/1528)
- [Damus NotePush](https://github.com/damus-io/notepush)
- [Amethyst Push Notification Server](https://github.com/vitorpamplona/amethyst-push-notif-server)
- [Apple: Notification Service Extension](https://developer.apple.com/documentation/usernotifications/modifying-content-in-newly-delivered-notifications)
- [Apple: APNs Token-Based Authentication](https://developer.apple.com/documentation/usernotifications/establishing-a-token-based-connection-to-apns)
- [NIP-98: HTTP Authentication](https://github.com/nostr-protocol/nips/blob/master/98.md)
- [NIP-59: Gift Wraps](https://github.com/nostr-protocol/nips/blob/master/59.md)
- [RFC 9420: Messaging Layer Security](https://datatracker.ietf.org/doc/rfc9420/)
