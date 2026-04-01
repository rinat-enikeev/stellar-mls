# Real-World Readiness: 4-User Cross-Platform Group Chat

**Scenario**: Alice (iOS), Bob (iOS), Carol (Android), Dave (Android) want to create a private group chat and start messaging.

---

## End-to-End Flow Today

```
Alice creates group → gets InviteCode → shares with Bob, Carol, Dave
Bob/Carol/Dave paste InviteCode → join group → everyone chats
```

Here's what works, what breaks, and what's missing at each step.

---

## What Works

The cryptographic and transport foundations are solid on both platforms:

- AES-256-GCM group message encryption/decryption (HKDF-derived keys)
- Nostr relay WebSocket connections with exponential backoff reconnection and heartbeat ping
- Nostr kind 24114 publish/subscribe for group messages
- secp256k1 Schnorr event signing (NIP-01 compliant, no double-hashing)
- BLS12-381 Poseidon Merkle membership commitments
- Groth16 ZK proof generation and on-chain verification
- Key generation: Nostr (secp256k1), BLS12-381, Ed25519 (Stellar), X25519 (invitations)
- HKDF key derivation is deterministic — same Nostr secret produces identical derived keys on iOS and Android
- Secure key storage (iOS Keychain, Android EncryptedSharedPreferences)
- Local persistence (SwiftData on iOS, Room on Android) with field-level encryption
- Soroban contract: create group, update commitment, verify membership, deactivate
- Message size limits (1 MB), event ID verification, WebSocket ping keepalive

---

## What Breaks

### 1. Joiners Have Empty Member Lists (Critical)

`InviteCode` contains only `{groupID, groupSecret, name, relayHints}`. It does **not** include `members`, `epoch`, or `salt`.

When Bob pastes Alice's invite code:
- Bob's local `ChatGroup` is created with `members = []`, `epoch = 0`, random `salt`
- Bob sends a message — it includes his BLS pubkey in the JSON wrapper (H-4 authentication)
- Alice receives it, checks `group.members.contains(bobBlsPubkey)` → **false** → message rejected
- Alice sends a message — Bob receives it, checks his empty member list → **rejected**

**Nobody can read anyone else's messages after joining via invite code.**

This is the same on both platforms. iOS `JoinGroupView` and Android `JoinGroupViewModel` both create groups from `InviteCode` with default empty members.

### 2. No Member Addition After Join (Critical)

Even if Alice manually adds Bob to her member list, there's no protocol for:
- Bob announcing "I joined" to Alice
- Alice broadcasting her member list back to Bob
- Carol and Dave learning about Bob (or each other)
- Reaching a consistent member list across all 4 devices

The `state_update` protocol message type exists in the code but there is no orchestrated join handshake — no message triggers it, no handler processes it into a member-list merge.

### 3. Invitation Transport Tags Not Indexed (Critical)

`InvitationTransport` uses kind 24113 events with a custom `#sep_inbox` tag. Standard Nostr relays (damus, nos.lol, snort, nostr.band) only index NIP-01 single-letter tags (`#e`, `#p`, `#t`, `#d`, etc.).

A subscription filter like `{"#sep_inbox": ["abc..."]}` returns **zero results** on these relays. Invitation events are published but never delivered to recipients.

**The entire encrypted invitation flow (BootstrapPayload via kind 24113) silently fails.**

This means even though `BootstrapPayload` contains the full group state (members, epoch, salt, commitment), it can never reach the recipient on public relays.

### 4. Encryption Key Mismatch Between Creator and Joiners

`ChatGroup.encryptionKey` is derived from `HKDF(groupSecret, epoch, salt)`. Alice's group has her salt and epoch 0. Bob's group has a **different random salt** and epoch 0 (generated fresh on join). Carol and Dave each have yet another salt.

Even if member list issues were fixed, **messages would still be unreadable** because each device derives a different encryption key from different salts.

---

## What's Missing

### 5. Join Handshake Protocol

No defined message sequence for:
```
Joiner connects → announces presence → receives current state → confirms sync
```

The building blocks exist (protocol messages over kind 24114, `state_update` type detection) but the actual handshake is not implemented.

### 6. Invitation UX

Sending an invitation via `InvitationTransport` requires the recipient's X25519 public key — a 64-character hex string. There is no:
- QR code generation or scanning
- Deep link / universal link support
- Contact list or address book
- NFC or nearby sharing
- Human-readable key format (e.g., bech32)

For Alice to invite Bob, Carol, and Dave, she needs each person's 64-char hex key delivered out-of-band.

### 7. Relay Configuration Sync

Each device has its own relay list. If Alice uses `relay.damus.io` and Dave uses `relay.snort.social` with no overlap, they can't see each other's messages. There is no:
- Relay list embedded in the group state
- Relay hint negotiation on join
- Automatic relay discovery

`InviteCode.relayHints` exists but joiners may override it with their own defaults.

### 8. Offline Message Delivery

If Carol is offline when Alice sends a message, Carol will miss it. The `since` filter on subscription uses a 5-minute window (Android) or subscription start time. Messages sent while offline and older than this window are lost.

There is no:
- Message persistence on relays beyond the relay's own retention
- Catch-up / history sync protocol
- Read receipts or delivery confirmation

### 9. Group Metadata Sync

Group name changes, relay list updates, and member removals have no propagation mechanism. If Alice renames the group, Bob/Carol/Dave still see the old name.

### 10. Cross-Platform Integration Tests

No tests verify that:
- iOS-encrypted messages are decryptable by Android (and vice versa)
- `BootstrapPayload` JSON serialization is compatible (iOS Codable vs Android JSONObject)
- HKDF derivation produces byte-identical keys on both platforms
- BLS/Poseidon FFI returns identical results across iOS and Android native libraries
- `InviteCode` encoding is cross-platform compatible (iOS base64-only vs Android base64+hex fallback)

### 11. Contract Access Control (N-26)

`create_group` on the Soroban contract has no access control. Anyone can create unlimited groups, potentially filling storage. A relayer auth pattern or admin whitelist is needed for production.

---

## What Actually Happens Today (Step by Step)

| Step | What Happens | Works? |
|------|-------------|--------|
| Alice creates group | Group created with Alice as sole member, commitment computed, invite code generated | Yes |
| Alice shares invite code | Base64 string copied to clipboard, shared via external channel | Yes |
| Bob pastes invite code | Group created with `members=[]`, random salt, epoch 0 | Partially — group exists but state is wrong |
| Bob subscribes to group topic | WebSocket subscription on kind 24114, topic derived from groupSecret | Yes |
| Alice sends message | Encrypted with her key (her salt), published to relays | Yes |
| Bob receives Alice's message | Decryption fails (different salt → different key) | **No** |
| Bob sends message | Encrypted with his key (his salt), includes BLS pubkey | Yes |
| Alice receives Bob's message | Decryption succeeds (same groupSecret but different salt → different key) OR decryption fails | **No** |
| Alice tries invitation transport | Kind 24113 with `#sep_inbox` tag published | Published but **not delivered** (tag not indexed) |

**Bottom line: the group creation and relay subscription work, but no actual messages can be exchanged.**

---

## Implementation Phases

### Phase 4: Make Chat Work (P0 — without this, nothing works)

**Goal**: Alice creates a group, shares an invite code, Bob/Carol/Dave join, everyone chats.

#### Step 1: Enrich InviteCode with group state
- **Modify iOS** `InviteCode` (in `ChatGroup.swift`): add `members: [SEPGroupMemberLeaf]`, `epoch: UInt64`, `salt: Data`, `commitment: Data?`
- **Modify Android** `InviteCode` (in `ChatGroup.kt`): add same fields
- **Modify** `encode()`/`decode()` on both platforms to serialize new fields (members as base64 compressed G1 keys + leaf hashes)
- **Modify** `JoinGroupView` (iOS) and `JoinGroupViewModel` (Android): populate `ChatGroup` from enriched invite code including members, epoch, salt, commitment
- **Modify** `CreateGroupView` (iOS) and `CreateGroupViewModel` (Android): generate invite code with current group state

This alone unblocks encrypted messaging between creator and first joiner (they share the same salt and key).

#### Step 2: Auto-add joiner to member list
- **Define** a new protocol message: `{"type": "member_joined", "blsPubkey": "<base64>", "leafHash": "<base64>"}`
- **Modify iOS** `NostrMessageTransport`: on join, broadcast `member_joined` to the group topic
- **Modify Android** `NostrMessageTransport`: same
- **Modify iOS** `AppState` protocol message handler: on receiving `member_joined`, add the new leaf to group members, recompute commitment, increment epoch, regenerate salt, broadcast `state_update` with new full state
- **Modify Android** `GroupListViewModel` protocol message handler: same
- **Define** `state_update` message: `{"type": "state_update", "epoch": N, "members": [...], "salt": "<base64>", "commitment": "<base64>"}`
- **Modify** both platforms: on receiving `state_update` with higher epoch, adopt the new state

#### Step 3: Salt and key synchronization
- After Step 2, all members receive the `state_update` with the new salt
- **Verify** `encryptionKey` derivation uses the group's current salt (already does)
- **Add** salt history window to `state_update` so members can decrypt recent messages encrypted under previous salts
- **Modify** decryption on both platforms: try current salt first, then fall back to historical salts

#### Step 4: Cross-platform InviteCode compatibility tests
- **Create** shared test vectors: a known InviteCode encoded on iOS, decoded on Android (and vice versa)
- **Create** shared HKDF test vectors: same secret → same encryption key on both platforms
- **Verify** BLS leaf hash computation is identical for the same secret key

### Phase 5: Fix Invitation Transport (P0 — required for private invitations)

**Goal**: Alice can send an encrypted invitation to Bob's inbox without out-of-band key exchange.

#### Step 1: Switch `sep_inbox` to NIP-01 compatible tag
- **Modify iOS** `InvitationTransport.swift`: change tag from `["sep_inbox", tag]` to `["d", "sep-inbox:" + tag]` (NIP-33 parameterized replaceable, or use `["t", "sep-inbox:" + tag]`)
- **Modify Android** `InvitationTransport.kt`: same change
- **Update** subscription filters on both platforms: `{"#d": ["sep-inbox:<inboxTag>"]}` (or `#t`)

#### Step 2: Verify relay indexing
- Test that damus, nos.lol, and other relays return kind 24113 events when filtering by the new tag
- Document which relays work and which don't

#### Step 3: Fallback to enriched InviteCode
- Since InviteCode now carries full state (Phase 4 Step 1), it serves as a reliable fallback when invitation transport isn't available
- The encrypted invitation via kind 24113 becomes an optional privacy enhancement, not a requirement

### Phase 6: Improve UX (P1 — usable but not pleasant without this)

#### Step 1: QR code for invite codes
- **iOS**: Generate QR code from `InviteCode.encode()` string using `CoreImage` `CIQRCodeGenerator`
- **iOS**: Add QR scanner in `JoinGroupView` using `AVCaptureSession`
- **Android**: Generate QR code using `zxing` or `journeyapps/zxing-android-embedded`
- **Android**: Add QR scanner in `JoinGroupScreen`

#### Step 2: QR code for X25519 inbox key
- **iOS** `SettingsView`: show QR code of `keyAgreementPublicKeyHex` for invitation transport
- **Android** `SettingsScreen`: same
- Scanner in invite-member flow on both platforms

#### Step 3: Deep links
- **iOS**: Register URL scheme `stellarchat://join?code=<base64>`
- **Android**: Register intent filter for same scheme
- Parse invite code from URL on app launch

#### Step 4: Clipboard auto-detect
- On app foreground, check clipboard for valid invite code format
- Prompt user to join if detected

### Phase 7: Reliability (P1 — works but fragile without this)

#### Step 1: Offline message catch-up
- On reconnect, use `since` timestamp of last received event (persisted per group) instead of fixed 5-minute window
- Store last-seen event timestamp in `ChatGroup` / `PersistedGroup`

#### Step 2: Relay overlap enforcement
- On join, merge invite code's `relayHints` with user's relay list (union, not replace)
- Store effective relay set per group
- Warn if a group has zero relay overlap with connected relays

#### Step 3: State conflict resolution
- If two members add people simultaneously (epoch fork), resolve by: highest epoch wins, ties broken by lexicographic commitment comparison
- Broadcast reconciled state as new `state_update`

#### Step 4: Delivery confirmation
- Add optional `{"type": "ack", "eventID": "<id>"}` protocol message
- Track delivery status per message in UI (sent → delivered → read)

### Phase 8: Production Hardening (P2)

#### Step 1: Contract access control
- Add admin whitelist or relayer signature requirement to `create_group`
- Rate-limit group creation per Stellar account

#### Step 2: Cross-platform integration test suite
- Shared test vector file (JSON) with: known keys, expected HKDF outputs, expected encrypted payloads, expected commitments
- iOS XCTest and Android instrumented test that both load the same vectors
- CI pipeline that runs both and compares results

#### Step 3: Group metadata sync
- `{"type": "group_renamed", "name": "<new>"}` protocol message
- `{"type": "member_removed", "blsPubkey": "<base64>"}` protocol message
- Handle in protocol message handlers on both platforms

#### Step 4: Key rotation
- Periodic salt rotation (time-based or message-count-based)
- Automated `state_update` broadcast on rotation
- On-chain commitment update after rotation
