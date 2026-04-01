# Phase 4: Production Readiness — Explained Simply

Phase 4 takes the cryptographic foundation (Phases 1-3) and wires it into real applications. The contract already works. The ZK proofs already verify. Phase 4 answers the question: *how do real users on real phones actually use this system without leaking their identity or getting stuck?*

Four features, each solving a concrete problem.

---

## 1. Fee Decoupling (The Relayer Pattern)

### The problem

Every Stellar transaction has a fee. Someone pays it. That someone's public key is recorded on the ledger — permanently, publicly, and irrevocably.

If Alice submits a `create_group` transaction directly, the Stellar network records: "Account `GALICE...` paid 100 stroops for a Soroban invocation on contract `CXYZ...`." The ZK proof inside is perfectly private — the contract learns nothing about Alice's membership. But the *ledger* just recorded her account ID right next to the group ID. Anyone watching the chain can correlate "Alice interacted with group X."

This defeats the entire privacy model.

### The solution

Don't let Alice submit the transaction herself. Instead, she sends the payload to a **relayer** — a server that wraps her payload in its own transaction, pays the fee with its own account, and submits it. The chain sees the relayer's account, not Alice's.

```
Alice's phone                  Relayer                     Stellar Network
────────────                   ───────                     ───────────────

1. Build contract payload
   (group_id, commitment,
    proof, public_inputs)

2. POST to relayer ──────────> 3. Wrap in Soroban tx
                                  signed by relayer key
                               4. Submit tx ──────────────> 5. Verify proof
                                                               Store commitment
                               6. Return tx hash <────────── 7. Return result

   <──────────────────────────  Result: accepted
```

The contract doesn't care who signed the transaction. It only checks the ZK proof. The proof proves Alice is a member. The transaction signature proves the relayer authorized the fee. These are completely independent checks.

### How it works under the hood

The relayer is just an HTTP endpoint. The client sends the exact same JSON payload it would send to the Soroban RPC endpoint. The relayer wraps it in a Stellar transaction, signs with its own keypair, submits, and returns the result.

From the client's perspective, the only difference is the URL. A `SEPRelayerTransport` replaces the `URLSessionSEPContractTransport` (iOS) or `OkHttpSEPContractTransport` (Android). All contract operations — create, update, verify, deactivate — transparently route through the relayer with zero code changes in the calling layer.

An optional `Authorization: Bearer <token>` header allows the relayer to authenticate clients (rate limiting, abuse prevention) without learning their on-chain identity.

### What was built

**Swift SDK** (`ContractClient.swift`):
- `SEPRelayerTransport` — a new transport that conforms to `SEPContractTransport`. Same JSON payload format, different destination URL. Optional Bearer auth.

**Kotlin SDK** (`GroupStateUpdate.kt`):
- `SEPRelayerConfig` — a data class holding `relayerURL` and optional `authToken`.

**iOS app** (`OnChainService.swift`, `StellarChatApp.swift`):
- `OnChainService` gained a second initializer: `init(contractID:relayerConfig:)`. When a relayer is configured, all contract calls route through it.
- `AppState.configureContractIfReady()` checks `isRelayerConfigured`. If the user has entered a relayer URL, the service is initialized with `SEPRelayerTransport` instead of `URLSessionSEPContractTransport`.
- Relayer URL and auth token are persisted in `UserDefaults`.

**Android app** (`SEPContractClient.kt`, `OnChainService.kt`, `GroupListViewModel.kt`):
- `OkHttpRelayerTransport` — OkHttp-based transport that routes through the relayer.
- `OnChainService` gained a third constructor: `OnChainService(contractID, relayerURL, authToken)`.
- `GroupListViewModel.configureContract()` picks the relayer transport when configured.
- Relayer config persisted in `SharedPreferences`.

### What the relayer does NOT do

The relayer never sees secret keys, never sees the member list, and never learns who is in any group. It receives the same opaque payload the contract receives. A compromised relayer can:
- **Refuse to submit** (denial of service — client retries or switches relayers)
- **Log the payload** (but the payload reveals nothing — it's a ZK proof and an opaque commitment)
- **Correlate IP addresses to submissions** (mitigate with Tor/VPN)

A compromised relayer **cannot**: forge proofs, modify commitments, impersonate members, or decrypt any group communication.

---

## 2. Salt Distribution

### The problem

Every time a group's membership changes (someone joins or leaves), the group generates a new random salt and computes a new commitment:

```
commitment = Poseidon(Poseidon(merkle_root, epoch), salt)
```

The salt is 32 random bytes. It serves a critical privacy purpose: it makes the commitment unpredictable. Without it, an attacker who guesses the member set could compute the expected commitment and verify their guess against the chain. The salt makes this a brute-force search over 2^256 possibilities — computationally impossible.

But here's the catch: **every member needs the salt** to verify their own commitment and generate proofs. If Alice adds Bob and generates a new salt, she needs to tell Carol, Dave, and everyone else what the new salt is. And if Carol was offline when the salt changed, she's stuck — she can't generate proofs until she learns the new salt.

### The solution

Salt is distributed through the same encrypted group channel (Nostr kind 24114) that carries chat messages. After a membership change, the initiator broadcasts a **state update** message containing the new salt, epoch, and member delta.

For members who were offline and missed the update, there's a **salt recovery protocol**: they send a salt request, and any online member who has the salt responds.

### State update flow (normal case)

```
Alice (initiates add)          Encrypted Group Channel        Bob, Carol, Dave
─────────────────────          ───────────────────────        ──────────────────

1. Add Eve to group
2. Increment epoch (2 -> 3)
3. Generate new salt
4. Recompute commitment
5. Build state update:
   { type: "sep_state_update",
     epoch: 3,
     salt: <32 bytes>,
     addedMembers: [Eve],
     commitment: <32 bytes> }
6. Encrypt & broadcast ───────> kind 24114 event ──────────> 7. Decrypt
                                                              8. Parse as protocol msg
                                                              9. Apply: add Eve,
                                                                 set epoch=3,
                                                                 set salt=new,
                                                                 set commitment=new
                                                             10. Store salt in history
```

### Salt recovery flow (offline member)

If Carol was offline and comes back at epoch 5, but her local state is at epoch 2:

```
Carol (behind)                 Encrypted Group Channel        Dave (up to date)
──────────────                 ───────────────────────        ─────────────────

1. Discover epoch mismatch
2. Send salt request:
   { type: "sep_salt_request",
     epoch: 3 }
   ─────────────────────────>  kind 24114 event ────────────> 3. Check salt history
                                                               4. Found salt for epoch 3
                                                               5. Send salt response:
                                                                  { type: "sep_salt_response",
                                                                    epoch: 3,
                                                                    salt: <32 bytes> }
                               kind 24114 event <────────────
6. Receive salt
7. Store in history
   <────────────────────────
```

### Protocol message disambiguation

Chat messages and protocol messages travel on the same encrypted channel. They're distinguished by trying to parse the decrypted text as JSON with a `type` field:

| `type` field | Message type | Action |
|---|---|---|
| `sep_state_update` | State update | Apply member delta, update epoch/salt/commitment |
| `sep_salt_request` | Salt request | Reply with salt if we have it |
| `sep_salt_response` | Salt response | Store in local salt history |
| (no `type` / not JSON) | Plain text chat | Display as chat message |

This happens in `NostrMessageTransport` on both platforms. After decrypting a kind 24114 event, the transport checks if the plaintext is a protocol message. If yes, it's dispatched to the protocol handler. If no, it's dispatched to the chat UI.

### What was built

**Swift SDK** (`GroupStateUpdate.swift`):
- `SEPGroupStateUpdate` — the state update message carrying epoch, salt, member delta, optional commitment, and optional sender attestation.
- `SEPSaltRequest` / `SEPSaltResponse` — salt recovery protocol messages.
- `SEPProtocolMessage` — parser that extracts the `type` field from JSON to decide if a message is a protocol message.

**Kotlin SDK** (`GroupStateUpdate.kt`):
- Equivalent Kotlin data classes: `SEPGroupStateUpdate`, `SEPSaltRequest`, `SEPSaltResponse`.

**iOS app**:
- `NostrMessageTransport` — added `onProtocolMessage` callback alongside existing `onMessage`. After decryption, calls `SEPProtocolMessage.parse()` to dispatch. Added `sendProtocolMessage()` for broadcasting protocol messages.
- `ChatViewModel` — sets the `onProtocolMessage` handler. State updates are forwarded to `AppState.applyStateUpdate()`. Salt requests trigger a response if the salt is in local history. Salt responses are stored in history.
- `AppState` — `saltHistory: [String: [UInt64: Data]]` stores salts per group per epoch. `storeSalt()` and `getSalt()` manage the history. `buildStateUpdate()` constructs a state update with the current group state and sender attestation. `applyStateUpdate()` applies received state updates: adds/removes members, updates epoch/salt/commitment, verifies sender attestation, persists to storage.

**Android app**:
- `NostrMessageTransport` — added `onProtocolMessage` callback, `isProtocolMessage()` check, and `sendProtocolMessage()`. Decrypted messages are checked for a JSON `type` field to distinguish protocol from chat.
- `GroupListViewModel` — `saltHistory` map, `storeSalt()`/`getSalt()`, `applyStateUpdate()`, `broadcastStateUpdate()`, `setupProtocolMessageHandler()` with full JSON parsing and dispatch of state updates, salt requests, and salt responses.

### Why salt history is per-group, per-epoch

Each member keeps a local map: `groupID -> (epoch -> salt)`. When a state update arrives, the salt is stored. When a salt request arrives, the member looks up the requested epoch and responds if found.

This means: **the more members are online, the more resilient the system is.** Even if the original updater goes offline, any member who received the state update can serve the salt to latecomers. The system degrades gracefully — you only lose salt recovery when *all* members who received the update go offline and purge their history.

---

## 3. Key Attestation Distribution

### The problem

Every user in Stellar MLS has four types of cryptographic keys:

| Key type | Curve | Purpose | Size |
|----------|-------|---------|------|
| secp256k1 | Koblitz | Nostr identity (public events, Schnorr signatures) | 32 bytes |
| BLS12-381 | BLS | Group membership (ZK proofs, Merkle leaves) | 32 bytes (secret) / 48 bytes (public) |
| Ed25519 | Twisted Edwards | Stellar on-chain identity (account key, transaction signing) | 32 bytes |
| X25519 | Montgomery | Key agreement (encrypted invitations via ECDH) | 32 bytes |

The BLS key proves you're in a group. The Ed25519 key proves you control a Stellar account. But how do other members know that the same person controls both keys? Without a cryptographic binding, an attacker could:
- Steal or intercept a BLS key
- Claim it's bound to their own Stellar account
- Push malicious state updates appearing to come from the legitimate member

### The solution

A **key attestation** cryptographically binds the BLS public key to the Ed25519 public key:

```
binding_message = SHA-256("SEP-XXXX:key-binding" || bls_pubkey_48_bytes)
signature = Ed25519_sign(stellar_private_key, binding_message)

attestation = {
  bls_pubkey:     48 bytes (compressed G1 point),
  ed25519_pubkey: 32 bytes (Stellar public key),
  signature:      64 bytes (Ed25519 signature over binding_message)
}
```

To verify: recompute the binding message from the claimed BLS key, then verify the Ed25519 signature using the claimed Stellar public key. If it passes, the holder of the Stellar private key has endorsed "this BLS key is mine."

The prefix `"SEP-XXXX:key-binding"` prevents the binding message from colliding with any other protocol message. The attestation is 144 bytes total.

### Where attestations are distributed

Attestations travel through two channels:

**1. Invitations (kind 24113)**

When Alice invites Bob, the `BootstrapPayload` includes Alice's attestation:

```json
{
  "groupID": "...",
  "groupSecret": "...",
  "members": [...],
  "senderNostrPubkey": "...",
  "senderAttestation": {
    "blsPubkey": "<48 bytes, base64>",
    "ed25519Pubkey": "<32 bytes, base64>",
    "signature": "<64 bytes, base64>"
  }
}
```

Bob can verify: "Alice's BLS key (which appears in the member list) is bound to the Stellar account she claims to control."

**2. State updates (kind 24114)**

When Alice changes the group membership, the state update includes her attestation:

```json
{
  "type": "sep_state_update",
  "epoch": 3,
  "salt": "...",
  "addedMembers": [...],
  "senderAttestation": {
    "blsPubkey": "...",
    "ed25519Pubkey": "...",
    "signature": "..."
  }
}
```

All recipients verify the attestation before applying the state update. If verification fails, the update is **silently discarded**. This prevents an attacker who intercepted a BLS key from pushing malicious state updates.

### What was built

**Both platforms**:
- `BootstrapPayload` gained an optional `senderAttestation` field (iOS: `KeyAttestation?`, Android: `KeyAttestation?`). The `from()` factory method accepts an optional attestation parameter.
- `SEPGroupStateUpdate` includes an optional `senderAttestation: SEPKeyAttestationPayload?` field.
- `applyStateUpdate()` verifies the attestation before applying changes. Invalid attestation = update discarded.
- JSON serialization and deserialization of the attestation in `BootstrapPayload.toJson()` / `fromJson()`.

### What attestation does NOT do

An attestation proves **key binding**. It does **not** prove identity. Knowing that BLS key X and Stellar account Y belong to the same entity tells you nothing about *who* that entity is — unless you can independently map the Stellar account to a real-world identity.

Attestation also does **not** prove the BLS key is still secret. If a key is compromised, the attestation remains valid. Key compromise requires group re-keying (new epoch, new members, new salt), not just attestation revocation.

---

## 4. Group Deactivation

### The problem

Groups need to end. A project wraps up, a team disbands, or a group is compromised and should be frozen. But who gets to shut it down?

In a traditional system, you'd have an admin role. But Stellar MLS has no admin — the contract doesn't know who the members are. It can't enforce "only the group creator can deactivate" because it doesn't know who created the group. The ZK proof hides that information by design.

### The solution

**Any member can deactivate.** The only requirement is a valid ZK proof of membership — the same proof type used for creating and updating the group. If you can prove you're in the group, you can deactivate it.

```
Member's phone                              Soroban Contract
──────────────                              ────────────────

1. Generate membership proof
   (same as for create/update)
2. Decompress to 384 bytes

3. Submit deactivate_group(   ──────────>   4. Load current state
     group_id,                               5. Verify proof against state
     proof,                                  6. Set active = false
     public_inputs                           7. Emit GroupDeactivated event
   )                          <──────────   8. Return accepted
```

After deactivation:
- `verify_membership` still works — you can prove you were a member at the final epoch
- `update_commitment` is rejected — no more membership changes
- `get_state` still works — the group's final state remains readable

Deactivation is permanent and irreversible. There's no "reactivate" function. If a group needs to resume, create a new group.

### What was built

**The contract already had `deactivate_group` from Phase 3.** No contract changes were needed. Phase 4 just exposes this capability through the app layer.

**iOS app** (`OnChainService.swift`, `StellarChatApp.swift`):
- `OnChainService.deactivateGroup()` — generates a membership proof, decompresses from 192 bytes (compressed) to 384 bytes (uncompressed BLS12-381 points), submits `deactivate_group` to the contract.
- `AppState.deactivateGroupOnChain()` — calls the service and handles errors.

**Android app** (`ContractTypes.kt`, `SEPContractClient.kt`, `OnChainService.kt`, `GroupListViewModel.kt`):
- `buildDeactivateGroupPayload()` — JSON builder for the deactivation request.
- `SEPContractClient.deactivateGroup()` — typed method wrapping the transport call.
- `OnChainService.deactivateGroup()` — orchestrates proof generation, format conversion, and contract call.
- `GroupListViewModel.deactivateGroupOnChain()` — ViewModel method with IO dispatching and result callback.

---

## How all four features work together

Here's a complete flow showing all four Phase 4 features in a single scenario:

```
Timeline: Alice adds Bob to a group that's published on-chain
──────────────────────────────────────────────────────────────

1. Alice's phone:
   a. Add Bob's leaf to member list
   b. Increment epoch (0 -> 1)
   c. Generate new 32-byte salt
   d. Recompute Poseidon commitment
   e. Generate ZK proof against OLD state (epoch 0)
   f. Decompress proof: 192 bytes -> 384 bytes

2. Alice's phone -> Relayer:                                 <-- FEE DECOUPLING
   POST update_commitment payload to relayer URL
   (Relayer pays the Stellar fee. Alice's account is invisible.)

3. Relayer -> Stellar/Soroban:
   Submit transaction. Contract verifies proof. Stores new commitment.

4. Alice's phone -> Encrypted channel (kind 24114):          <-- SALT DISTRIBUTION
   Broadcast state update with:
   - epoch: 1
   - salt: <new 32 bytes>
   - addedMembers: [Bob's leaf]
   - senderAttestation: {bls, ed25519, sig}                  <-- KEY ATTESTATION

5. Carol's phone (online):
   a. Decrypt state update from channel
   b. Verify Alice's key attestation                         <-- KEY ATTESTATION
   c. Apply: add Bob, set epoch=1, set salt=new
   d. Store salt in history for epoch 1                      <-- SALT DISTRIBUTION

6. Dave's phone (was offline, comes back later):
   a. Discover local epoch (0) < group epoch (1)
   b. Send salt request for epoch 1 on group channel         <-- SALT DISTRIBUTION
   c. Carol responds with salt for epoch 1
   d. Dave stores salt and applies update

7. Later, Alice decides to end the group:
   a. Generate ZK membership proof
   b. Submit deactivate_group via relayer                    <-- FEE DECOUPLING
   c. Contract verifies proof, sets active=false             <-- GROUP DEACTIVATION
   d. No more updates allowed. verify_membership still works.
```

---

## The message flow diagram

Here's how protocol messages and chat messages coexist on the same encrypted channel:

```
                    Encrypted Group Channel (kind 24114)
                    ────────────────────────────────────
                                    |
                              [Decrypt]
                                    |
                          [Is it JSON with "type"?]
                           /                    \
                         Yes                     No
                          |                       |
                    [Parse "type"]          [Plain text chat]
                    /      |       \              |
           state_update  salt_req  salt_resp   Display in UI
                |           |          |
          Apply delta    Reply if    Store in
          to group       we have     salt history
                         the salt
```

All messages — chat and protocol — are encrypted with the same AES-256-GCM key derived from the group secret. Relays see identical ciphertext regardless of whether the content is "hey everyone" or a state update. No metadata leakage.

---

## File map

### SDK layer

| File | Platform | What was added |
|------|----------|----------------|
| `swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift` | Swift | `SEPGroupStateUpdate`, `SEPSaltRequest`, `SEPSaltResponse`, `SEPKeyAttestationPayload`, `SEPProtocolMessage`, `SEPRelayerConfig` |
| `swift-mls/Sources/SwiftMLS/ContractClient.swift` | Swift | `SEPRelayerTransport` — HTTP transport routing through relayer |
| `kotlin-mls/src/main/java/com/stellarmls/mls/GroupStateUpdate.kt` | Kotlin | `SEPGroupStateUpdate`, `SEPSaltRequest`, `SEPSaltResponse`, `SEPKeyAttestationPayload`, `SEPRelayerConfig` |

### iOS app

| File | What changed |
|------|-------------|
| `Nostr/NostrMessageTransport.swift` | `onProtocolMessage` callback, `sendProtocolMessage()`, protocol/chat dispatch after decryption |
| `ViewModels/ChatViewModel.swift` | Protocol message handling — forwards state updates to AppState, responds to salt requests, stores salt responses |
| `Models/OnChainService.swift` | `init(contractID:relayerConfig:)` relayer constructor, `deactivateGroup()` method |
| `StellarChatApp.swift` | Relayer config + persistence, salt history (`storeSalt`/`getSalt`), `buildStateUpdate()`, `applyStateUpdate()`, `deactivateGroupOnChain()`, relayer-aware `configureContractIfReady()` |
| `Models/BootstrapPayload.swift` | `senderAttestation: KeyAttestation?` field, updated `from()` factory |

### Android app

| File | What changed |
|------|-------------|
| `nostr/NostrMessageTransport.kt` | `onProtocolMessage` callback, `sendProtocolMessage()`, `isProtocolMessage()` JSON check |
| `onchain/SEPContractClient.kt` | `deactivateGroup()` method, `OkHttpRelayerTransport` class |
| `onchain/ContractTypes.kt` | `buildDeactivateGroupPayload()` JSON builder |
| `onchain/OnChainService.kt` | Relayer constructor, `deactivateGroup()` method |
| `viewmodel/GroupListViewModel.kt` | Relayer config + persistence, salt history, state update build/apply/broadcast, protocol message handler, `deactivateGroupOnChain()`, JSON helpers |
| `model/BootstrapPayload.kt` | `senderAttestation: KeyAttestation?` field, JSON serialization, updated `from()` factory |

---

## Security summary

| Feature | What it protects | What it doesn't protect |
|---------|-----------------|------------------------|
| Fee decoupling | On-chain identity (fee payer != member) | IP-level tracking (use Tor/VPN) |
| Salt distribution | Commitment unpredictability (prevents member-set guessing) | Availability (needs at least one online member with the salt) |
| Key attestation | Key binding integrity (BLS <-> Ed25519) | Key compromise (compromised key still has valid attestation) |
| Group deactivation | Frozen state (no further updates) | Reversibility (deactivation is permanent) |

---

## What's NOT in Phase 4

Two features were deferred from the original production readiness plan:

- **Push notifications** — alerting users to new messages/invitations when the app is backgrounded. Requires platform-specific infrastructure (APNs for iOS, FCM for Android) and a notification relay service.
- **Sync device state** — syncing group membership, salt history, and key material across multiple devices owned by the same user. Requires a secure device-linking protocol and encrypted cross-device sync channel.

Both are user-experience features. The cryptographic and protocol foundations are complete — these are deployment concerns rather than security concerns.
