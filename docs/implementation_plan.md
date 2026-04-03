# Secure Member Removal — Implementation Plan

**Date:** 2026-04-03
**Based on:** [secure-member-removal-design.md](secure-member-removal-design.md)
**Status:** Draft

---

## Overview

This document turns the secure member removal design into a concrete implementation plan
with specific file changes, data structures, and verification steps for each phase.

The core problem: the current poisoned-salt + `SEPRekey` flow is a timing-based mitigation,
not cryptographic eviction. A malicious removed client that stays subscribed can decrypt the
rekey message and recover the real salt.

The correct fix: deliver fresh `groupSecret'` and `salt'` to each remaining member's X25519
inbox individually, so the removed member never receives the new secret material regardless
of their subscription behavior.

### Existing Infrastructure to Reuse

| Component | iOS | Android |
|-----------|-----|---------|
| X25519 key agreement | `KeyManager.swift:36,122-138` | `KeyManager.kt:79-82,148-158` |
| Inbox encrypt/decrypt | `GroupCrypto.swift:79-168` | `GroupCrypto.kt:105-216` |
| Inbox transport | `InvitationTransport.swift:104-148` | `InvitationTransport.kt:107-139` |
| Inbox subscription | `InvitationTransport.swift:75-101` | `InvitationTransport.kt:87-104` |
| Attestation | `GroupStateUpdate.swift:68-97` | `GroupStateUpdate.kt:83-102` |
| Hidden inbox tag | `KeyManager.swift:131-133` | `KeyManager.kt:85` |

### Missing Piece

`SEPKeyAttestationPayload` binds BLS pubkey -> Ed25519 pubkey, but does NOT bind the
member's X25519 inbox public key. Without that binding, the remover has no authenticated
encryption target for per-member rekey delivery.

---

## Phase 0 — Truth in Security Claims

**Goal:** Remove inaccurate claims about the current removal flow immediately.

### Changes

1. **UI copy** — Update any strings that claim removed members "will not be able to decrypt
   future messages" to accurately state the mitigation is best-effort.

2. **Code comments** — Add comments to the poisoned-salt and `SEPRekey` code paths on both
   platforms noting this is a best-effort mitigation, not cryptographic eviction:

   - iOS: `StellarChatApp.swift` — `performEpochTransition` removal branch
   - Android: `GroupListViewModel.kt` — `performEpochTransition` removal branch

3. **Documentation** — Add a note to `docs/design-doc.md` pointing to the design doc and
   this implementation plan.

### Verification

- Grep for "will not be able to decrypt" or similar claims in UI strings and comments
- Ensure no misleading security guarantees remain in user-facing text

---

## Phase 1 — Authenticated Member Transport Bundle

**Goal:** Create an authenticated mapping from each group member to their X25519 inbox
encryption key, so the remover knows where to send per-member rekey envelopes.

### 1.1 Define `SEPMemberTransportBundle`

**Swift** — `swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift`:

```swift
public struct SEPMemberTransportBundle: Codable, Equatable, Sendable {
    public let blsPubkey: Data            // 48 bytes, compressed G1
    public let stellarEd25519Pubkey: Data  // 32 bytes
    public let x25519InboxPubkey: Data     // 32 bytes
    public let version: UInt16
    public let signature: Data             // 64 bytes, Ed25519

    public var hasValidStructure: Bool {
        blsPubkey.count == 48 &&
        stellarEd25519Pubkey.count == 32 &&
        x25519InboxPubkey.count == 32 &&
        signature.count == 64
    }

    /// Domain-separated binding message:
    /// SHA-256("SEP-XXXX:member-transport-v1" || bls || stellar || x25519 || version)
    public func computeBindingMessage() -> Data {
        var hasher = SHA256()
        hasher.update(data: Data("SEP-XXXX:member-transport-v1".utf8))
        hasher.update(data: blsPubkey)
        hasher.update(data: stellarEd25519Pubkey)
        hasher.update(data: x25519InboxPubkey)
        var vBytes = version.bigEndian
        hasher.update(data: Data(bytes: &vBytes, count: 2))
        return Data(hasher.finalize())
    }
}
```

**Kotlin** — `kotlin-mls/src/main/java/com/stellarmls/mls/GroupStateUpdate.kt`:

```kotlin
data class SEPMemberTransportBundle(
    val blsPubkey: ByteArray,            // 48 bytes
    val stellarEd25519Pubkey: ByteArray, // 32 bytes
    val x25519InboxPubkey: ByteArray,    // 32 bytes
    val version: Int,                    // u16
    val signature: ByteArray             // 64 bytes
) {
    val hasValidStructure: Boolean
        get() = blsPubkey.size == 48 &&
                stellarEd25519Pubkey.size == 32 &&
                x25519InboxPubkey.size == 32 &&
                signature.size == 64

    fun computeBindingMessage(): ByteArray {
        val digest = MessageDigest.getInstance("SHA-256")
        digest.update("SEP-XXXX:member-transport-v1".toByteArray())
        digest.update(blsPubkey)
        digest.update(stellarEd25519Pubkey)
        digest.update(x25519InboxPubkey)
        digest.update(byteArrayOf((version shr 8).toByte(), version.toByte()))
        return digest.digest()
    }
}
```

### 1.2 Bundle creation and verification

**iOS** — `clients/ios/StellarChat/StellarChat/Models/KeyManager.swift`:

Add a method to create the bundle, signing the binding message with the Stellar Ed25519 key:

```swift
func createTransportBundle(blsPubkey: Data) -> SEPMemberTransportBundle
```

**Android** — `clients/android/.../crypto/KeyManager.kt`:

Same method:

```kotlin
fun createTransportBundle(blsPubkey: ByteArray): SEPMemberTransportBundle
```

Add verification to `GroupCrypto` on both platforms:

```
static func verifyTransportBundle(_ bundle: SEPMemberTransportBundle) -> Bool
```

Verify: recompute binding message, verify Ed25519 signature over it using
`bundle.stellarEd25519Pubkey`.

### 1.3 Wire the bundle into invite/bootstrap

**Files to modify:**

| File | Change |
|------|--------|
| `clients/ios/.../Models/BootstrapPayload.swift` | Add `senderTransportBundle: SEPMemberTransportBundle?` field |
| `clients/android/.../model/BootstrapPayload.kt` | Same |
| `clients/ios/.../StellarChatApp.swift` | Include own bundle when creating BootstrapPayload |
| `clients/android/.../viewmodel/GroupListViewModel.kt` | Same |

When accepting an invite, extract and verify the sender's transport bundle, then persist it.

### 1.4 Wire the bundle into `SEPGroupStateUpdate`

**Files to modify:**

| File | Change |
|------|--------|
| `swift-mls/.../GroupStateUpdate.swift` | Add `senderTransportBundle: SEPMemberTransportBundle?` to `SEPGroupStateUpdate` |
| `kotlin-mls/.../GroupStateUpdate.kt` | Same |
| iOS/Android `applyStateUpdate` | Extract, verify, and persist sender bundle from incoming updates |

### 1.5 Wire the bundle into `SEPMemberJoined`

**Files to modify:**

| File | Change |
|------|--------|
| `swift-mls/.../GroupStateUpdate.swift` | Add `transportBundle: SEPMemberTransportBundle?` to `SEPMemberJoined` |
| `kotlin-mls/.../GroupStateUpdate.kt` | Same |
| iOS/Android `handleMemberJoined` | Include own bundle in join broadcast; extract and persist joiner's bundle |

### 1.6 Persist transport bundles

**iOS** — `clients/ios/StellarChat/StellarChat/Models/PersistedModels.swift`:

Add new SwiftData model:

```swift
@Model
final class PersistedTransportBundle {
    var groupID: String               // FK to PersistedGroup
    var blsPubkeyHex: String          // lookup key
    var encryptedBundle: Data          // encrypted SEPMemberTransportBundle JSON
}
```

Use field-level encryption consistent with existing `PersistedGroup` pattern.

**Android** — `clients/android/.../persistence/PersistedModels.kt`:

```kotlin
@Entity(tableName = "transport_bundles",
    primaryKeys = ["groupID", "blsPubkeyHex"])
data class PersistedTransportBundle(
    val groupID: String,
    val blsPubkeyHex: String,
    val encryptedBundle: ByteArray
)
```

**Migration:**

- iOS: SwiftData handles additive model changes automatically
- Android: Add `MIGRATION_4_5` with `CREATE TABLE transport_bundles ...`

**DAO / Store:**

- `PersistenceStore` on both platforms: add `saveTransportBundle`, `loadTransportBundles(groupID:)`, `deleteTransportBundle(groupID:, blsPubkeyHex:)` methods

### 1.7 In-memory bundle directory

On both platforms, maintain a dictionary alongside group state:

```
transportBundles: [String: [Data: SEPMemberTransportBundle]]
//                groupID    blsPubkey  bundle
```

Load from persistence on app start. Update on invite accept, member join, and state update.

### Verification

- Create a group: creator's transport bundle is persisted
- Send an invite: bootstrap payload includes sender's transport bundle
- Accept an invite: invitee persists the inviter's bundle; invitee broadcasts own bundle
- State updates include sender bundle; receivers verify and persist it
- All members in a group eventually have bundles for all other members
- Invalid bundles (wrong signature, wrong field sizes) are rejected

---

## Phase 2 — Inbox Rekey for Removals

**Goal:** Make removals cryptographically sound by delivering fresh `groupSecret'` and
`salt'` to each remaining member's X25519 inbox individually.

### 2.1 Define new protocol messages

**Swift** — `swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift`:

```swift
/// Non-secret notice on old group channel during removal.
/// Does NOT contain new groupSecret or salt.
public struct SEPRemovalNotice: Codable, Equatable, Sendable {
    public static let messageType = "sep_removal_notice"
    public let type: String
    public let groupID: Data
    public let epoch: UInt64
    public let removedMemberKeys: [Data]
    public let commitment: Data?
    public let rekeyRequired: Bool        // always true for secure removals
}

/// Per-member rekey envelope delivered via X25519 inbox.
public struct SEPRekeyEnvelope: Codable, Equatable, Sendable {
    public static let messageType = "sep_rekey_envelope"
    public let type: String
    public let groupID: Data              // 32 bytes
    public let epoch: UInt64
    public let groupSecret: Data          // 32 bytes — the NEW group secret
    public let salt: Data                 // 32 bytes — the NEW salt
    public let commitment: Data?
    public let memberBundles: [SEPMemberTransportBundle]  // current members
    public let removedMemberKeys: [Data]
    public let relayHints: [String]
    public let senderBundle: SEPMemberTransportBundle
}
```

**Kotlin** — mirror both structs in `GroupStateUpdate.kt`.

### 2.2 Removal flow — sender side

**Files to modify:**

| File | Change |
|------|--------|
| `clients/ios/.../StellarChatApp.swift` | Modify `performEpochTransition` removal branch |
| `clients/android/.../viewmodel/GroupListViewModel.kt` | Same |

New removal flow in `performEpochTransition(.memberRemove)`:

1. Generate fresh `groupSecret'` = `SecureRandom(32)`
2. Generate fresh `salt'` = `SecureRandom(32)`
3. Compute new `commitment'` from updated member list
4. Derive new `trafficKey'` from `HKDF(groupSecret' || epoch || salt')`
5. Compute new `hiddenGroupTopic'` from `groupSecret'`

6. **Check if all remaining members have transport bundles**:
   - If YES → use secure inbox rekey (steps 7-9)
   - If NO → fall back to existing poisoned-salt + `SEPRekey` flow (backward compat)

7. Broadcast `SEPRemovalNotice` on old group topic (no secrets in this message)
8. Build `SEPRekeyEnvelope` with new `groupSecret'`, `salt'`, member bundles, etc.
9. For each remaining member:
   - Look up their `SEPMemberTransportBundle`
   - Encrypt the `SEPRekeyEnvelope` using `GroupCrypto.encryptInvitation` with their
     `x25519InboxPubkey` as recipient
   - Send via `InvitationTransport` (kind 34113) to their inbox tag

10. Update local state: persist new `groupSecret'`, `salt'`, `epoch`, `commitment'`
11. Unsubscribe from old topic, subscribe to new topic derived from `groupSecret'`

### 2.3 Removal flow — receiver side

**Files to modify:**

| File | Change |
|------|--------|
| `clients/ios/.../StellarChatApp.swift` | Add `applyRekeyEnvelope` handler |
| `clients/android/.../viewmodel/GroupListViewModel.kt` | Same |
| `clients/ios/.../Nostr/InvitationTransport.swift` | Dispatch rekey envelopes alongside invitations |
| `clients/android/.../nostr/InvitationTransport.kt` | Same |

Inbox subscription already exists. When a rekey envelope arrives at the member's inbox:

1. Decrypt using own X25519 private key via `GroupCrypto.decryptInvitation`
2. Parse as `SEPRekeyEnvelope`
3. Verify `senderBundle` signature
4. Verify `groupID` matches a known group
5. Verify `epoch` > current local epoch
6. Verify recomputed commitment matches `commitment` field
7. Install new state:
   - Persist new `groupSecret'`, `salt'`, `epoch`, `commitment'`
   - Update member list (remove `removedMemberKeys`, update bundles)
   - Unsubscribe from old topic
   - Subscribe to new topic derived from `groupSecret'`
8. Insert system message: "Member was removed from the group"

### 2.4 Old-channel notice handling

When a remaining member receives `SEPRemovalNotice` on the group channel:

- If they already installed the new epoch via inbox rekey → ignore
- If they haven't received the inbox rekey yet → display "Rekey pending..." state
- Do NOT attempt to derive keys from the notice (it contains no secret material)

When a removed member receives `SEPRemovalNotice`:

- They see themselves in `removedMemberKeys`
- They unsubscribe (same as current self-removal logic)
- They never receive the inbox rekey because it's sent to other members' inboxes

### 2.5 Backward compatibility

**Mixed-version detection:**

Before sending secure inbox rekey, check:

```swift
let allHaveBundles = remainingMembers.allSatisfy { member in
    transportBundles[groupID]?[member.publicKeyCompressed] != nil
}
```

If `allHaveBundles` is false, fall back to the existing poisoned-salt + `SEPRekey` flow.
This ensures groups with older clients that haven't sent their transport bundle yet still
function.

### 2.6 Topic migration

When `groupSecret` changes, `hiddenGroupTopic` changes. Both sender and receivers must:

1. Unsubscribe from old topic: `transport.unsubscribe(topic: oldTopic)`
2. Subscribe to new topic: derived from `GroupCrypto.hiddenGroupTopic(groupSecret')`
3. Update persisted group state with new topic

This reuses the existing `hiddenGroupTopic` derivation — no new crypto needed.

### Verification

- Remove a member from a group where all members have transport bundles
- Verify: removed member sees `SEPRemovalNotice` but NOT the inbox rekey
- Verify: remaining members receive inbox rekey and install new epoch
- Verify: remaining members can send/receive messages on new topic
- Verify: removed member cannot derive new traffic key even if they stay subscribed
- Verify: mixed-version group falls back to poisoned-salt flow gracefully

---

## Phase 3 — Reliability Layer

**Goal:** Make inbox rekey delivery operationally robust against relay failures, offline
members, and network partitions.

### 3.1 Define acknowledgment and resend messages

**Swift/Kotlin** — `GroupStateUpdate.swift` / `GroupStateUpdate.kt`:

```swift
public struct SEPRekeyAck: Codable, Equatable, Sendable {
    public static let messageType = "sep_rekey_ack"
    public let type: String
    public let groupID: Data
    public let epoch: UInt64
    public let senderBlsPubkey: Data      // acker's BLS pubkey
}

public struct SEPRekeyResendRequest: Codable, Equatable, Sendable {
    public static let messageType = "sep_rekey_resend_request"
    public let type: String
    public let groupID: Data
    public let epoch: UInt64
    public let requesterBundle: SEPMemberTransportBundle
}
```

### 3.2 Sender-side ack tracking

**Files to modify:**

| File | Change |
|------|--------|
| `clients/ios/.../StellarChatApp.swift` | Track pending rekeys, handle acks, schedule resends |
| `clients/android/.../viewmodel/GroupListViewModel.kt` | Same |

After sending per-member rekey envelopes:

1. Store pending rekey state: `{ groupID, epoch, unackedMembers: Set<blsPubkey> }`
2. Listen for `SEPRekeyAck` on the NEW group topic
3. On ack: remove member from unacked set
4. On timeout (e.g., 30 seconds): resend rekey envelope to unacked members
5. Bounded retry: max 3 attempts, then give up (member can request resend later)
6. Persist pending state across app restarts

### 3.3 Receiver-side ack and resend request

After installing a rekey envelope:

1. Broadcast `SEPRekeyAck` on the NEW group topic
2. If a member misses the rekey and later discovers an epoch gap (via messages on new topic
   they can't decrypt), send `SEPRekeyResendRequest` via inbox to the remover's inbox

### 3.4 Persistence for pending rekeys

**iOS** — `PersistedModels.swift`:

```swift
@Model
final class PersistedPendingRekey {
    var groupID: String
    var epoch: Int
    var encryptedEnvelope: Data           // the full SEPRekeyEnvelope, encrypted
    var unackedMemberKeysJSON: Data       // JSON array of hex BLS pubkeys
    var retryCount: Int
    var createdAt: Date
}
```

**Android** — `PersistedModels.kt`:

```kotlin
@Entity(tableName = "pending_rekeys")
data class PersistedPendingRekey(
    @PrimaryKey val id: String,           // groupID-epoch
    val groupID: String,
    val epoch: Int,
    val encryptedEnvelope: ByteArray,
    val unackedMemberKeysJSON: String,
    val retryCount: Int,
    val createdAt: Long
)
```

### Verification

- Remove a member; verify remaining members send ack after receiving rekey
- Kill app before receiving rekey; restart; verify rekey is resent and installed
- Simulate relay failure; verify bounded retry (max 3 attempts)
- Verify pending rekeys are cleaned up after all acks received or after expiry

---

## Phase 4 — Remove Secret Recovery From Shared Group Channel

**Goal:** Eliminate residual shared-channel secret leakage for eviction epochs by retiring
`SEPSaltRequest` / `SEPSaltResponse` for removal epochs.

### 4.1 Epoch type tagging

**Files to modify:**

| File | Change |
|------|--------|
| `swift-mls/.../GroupStateUpdate.swift` | Add `isRemovalEpoch: Bool` to persisted epoch metadata |
| `kotlin-mls/.../GroupStateUpdate.kt` | Same |
| iOS/Android persistence | Tag persisted epochs as removal or non-removal |

### 4.2 Salt request filtering

**Files to modify:**

| File | Change |
|------|--------|
| `clients/ios/.../StellarChatApp.swift` | In `SEPSaltRequest` handler: refuse to respond for removal epochs |
| `clients/android/.../viewmodel/GroupListViewModel.kt` | Same |

When receiving `SEPSaltRequest` for an epoch tagged as a removal epoch:

- Do NOT respond with `SEPSaltResponse`
- Instead respond with a pointer: "use inbox rekey for this epoch"
- The requester should send `SEPRekeyResendRequest` via inbox instead

### 4.3 Keep salt request/response for non-removal epochs

`SEPSaltRequest` / `SEPSaltResponse` remain valid for:

- Key rotation epochs (no member removed)
- Member-add epochs
- Normal epoch bumps

Only removal epochs are excluded from shared-channel salt disclosure.

### Verification

- Remove a member; another member misses the rekey
- Verify: `SEPSaltRequest` for the removal epoch does NOT get a `SEPSaltResponse`
- Verify: the late member uses `SEPRekeyResendRequest` instead and successfully recovers
- Verify: `SEPSaltRequest` for non-removal epochs still works normally

---

## Phase 5 — Hardening and Compatibility

**Goal:** Make the design production-grade with key rotation, multi-device, expiry, and
adversarial testing.

### 5.1 Inbox key rotation

If a member rotates their X25519 inbox key:

1. Create new `SEPMemberTransportBundle` with new `x25519InboxPubkey` and incremented `version`
2. Broadcast updated bundle via state update or dedicated `sep_bundle_update` message
3. Other members update their persisted bundle for this member
4. Version field prevents replay of old bundles

### 5.2 Multi-device semantics

Defer full multi-device support to a later release. For now:

- Each device has its own X25519 key (derived from same Nostr secret via HKDF, so
  deterministic — same key on all devices for the same account)
- Since `keyAgreementKey` is HKDF-derived from Nostr secret with fixed salt/info, all
  devices for the same account produce the same X25519 key → single inbox, no fanout needed

### 5.3 Replay and expiry windows

- Rekey envelopes include `epoch` — reject if `epoch <= currentEpoch`
- Pending rekeys expire after 24 hours (configurable)
- Deduplicate by `(groupID, epoch)` — at most one rekey per epoch per group

### 5.4 Integration tests for malicious removal scenarios

Add test cases:

1. **Malicious eavesdropper**: removed member stays subscribed to old topic → verify they
   see only `SEPRemovalNotice` (no secrets) and cannot derive new traffic key
2. **Replay attack**: attacker replays old rekey envelope → rejected because epoch <= current
3. **Bundle substitution**: attacker sends fake transport bundle with their own X25519 key →
   rejected because Ed25519 signature verification fails
4. **Selective delivery**: remover sends rekey to only some members → unacked members
   request resend via inbox

### 5.5 Consistency binding (optional)

Optionally bind a hash of the transport bundle directory into the off-chain signed state
for stronger consistency guarantees:

```
bundleDirectoryHash = SHA-256(sort(memberBundles.map { $0.computeBindingMessage() }))
```

Include in `SEPGroupStateUpdate.commitment` computation to detect bundle tampering.

### Verification

- Rotate inbox key; verify new bundle propagates and old one is replaced
- Multiple devices for same account; verify rekey received on all devices
- Send expired rekey envelope; verify rejection
- Run all malicious-scenario integration tests

---

## Implementation Order and Dependencies

```
Phase 0 (can start immediately, no code dependencies)
   │
Phase 1 (transport bundle — foundational)
   │
Phase 2 (inbox rekey — depends on Phase 1 bundles)
   │
Phase 3 (reliability — depends on Phase 2 rekey flow)
   │
Phase 4 (salt request filtering — depends on Phase 2 epoch tagging)
   │
Phase 5 (hardening — depends on all previous phases)
```

Phase 0 can be done in parallel with Phase 1. All other phases are sequential.

---

## Files Summary

| Phase | iOS Files | Android Files | SDK Files |
|-------|-----------|---------------|-----------|
| 0 | `StellarChatApp.swift` | `GroupListViewModel.kt` | — |
| 1 | `KeyManager.swift`, `BootstrapPayload.swift`, `StellarChatApp.swift`, `PersistedModels.swift`, `PersistenceStore.swift` | `KeyManager.kt`, `BootstrapPayload.kt`, `GroupListViewModel.kt`, `PersistedModels.kt`, `PersistenceStore.kt`, `StellarChatDatabase.kt` | `GroupStateUpdate.swift`, `GroupStateUpdate.kt` |
| 2 | `StellarChatApp.swift`, `InvitationTransport.swift` | `GroupListViewModel.kt`, `InvitationTransport.kt` | `GroupStateUpdate.swift`, `GroupStateUpdate.kt` |
| 3 | `StellarChatApp.swift`, `PersistedModels.swift`, `PersistenceStore.swift` | `GroupListViewModel.kt`, `PersistedModels.kt`, `PersistenceStore.kt`, `StellarChatDatabase.kt` | `GroupStateUpdate.swift`, `GroupStateUpdate.kt` |
| 4 | `StellarChatApp.swift` | `GroupListViewModel.kt` | — |
| 5 | All of the above | All of the above | Both SDKs |
