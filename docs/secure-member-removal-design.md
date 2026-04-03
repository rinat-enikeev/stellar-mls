# Secure Member Removal Design

**Date:** 2026-04-03
**Status:** Draft
**Scope:** Off-chain protocol and client design for cryptographically sound member removal

---

## 1. Problem

The current protocol derives the group traffic key from:

`HKDF(groupSecret || epoch || salt)`

That means a removed member remains inside the derivation universe as long as they still know:

- the long-lived `groupSecret`
- the next `epoch`
- the next `salt`

If the removal flow reveals those values on the old channel, removal is not cryptographic exclusion. It is only a UI/state transition.

The repository now contains an interim mitigation that tries to avoid leaking the real next salt during removal, but it is still fundamentally timing-based.

---

## 2. What Was Done Already

Both clients now use a two-step removal transition:

1. Broadcast a `sep_state_update` on the old group channel using a **poisoned random salt**
2. Broadcast a follow-up `sep_rekey` carrying the **real salt**, encrypted under the key derived from the poisoned salt

Code paths:

- Android removal broadcast and rekey:
  `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt:804-827`
- Android rekey application:
  `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt:1244-1259`
- iOS removal broadcast and rekey:
  `clients/ios/StellarChat/StellarChat/StellarChatApp.swift:686-733`
- iOS rekey application:
  `clients/ios/StellarChat/StellarChat/StellarChatApp.swift:1112-1128`

The intended effect is:

- a removed member can decrypt the old-channel state update
- but only sees the poisoned salt
- cannot derive the real next traffic key from that state update alone
- unsubscribes after processing self-removal
- therefore misses the follow-up `sep_rekey`

This is a reasonable **best-effort mitigation** against honest clients following the reference app logic.

---

## 3. Limitations of the Current Mitigation

The current mitigation is not the correct security boundary.

### 3.1 It relies on unsubscribe timing

The design assumes the removed client:

- processes the removal update first
- unsubscribes immediately
- does not receive the rekey message after that point

That is not a cryptographic guarantee. It is a race between:

- relay delivery order
- local event buffering
- app scheduling
- subscription teardown timing

### 3.2 A malicious client can ignore the unsubscribe

Any custom client that keeps listening to the old topic can:

- decrypt the poisoned-salt state update
- derive the poisoned-key
- decrypt the `sep_rekey`
- recover the real salt
- continue deriving the new traffic key

This is the central failure mode. Security cannot depend on cooperative client behavior from the party being removed.

### 3.3 The old channel is still carrying future secret material

Even though the real salt moved to a second message, the removal flow still uses the same shared group channel as the transport for future secret material.

As long as the removed member can stay on that transport, the protocol has not created any asymmetric distinction between:

- the remaining members
- the removed member

### 3.4 Symmetric-only variants do not solve this

The following ideas are still insufficient:

- deterministic next salt derivation
- transition keys derived from old traffic state
- hiding the real salt behind a second shared-channel message
- any KDF that only uses inputs already known to all current members

If a removed member has the same symmetric inputs as the remaining members, they can compute the same outputs.

---

## 4. Design Goal

The correct goal is:

> After a removal transition, a removed member must be unable to derive or receive the next epoch’s traffic secret even if they continue listening on the old group topic with a malicious custom client.

This requires a **new secret** that is delivered **only** to the remaining members.

That means the protocol must use asymmetric per-recipient delivery for removal rekeying.

---

## 5. Constraints From the Current Repository

The good news is that the repository already contains most of the transport primitive needed for a correct design.

### Already available

- Per-recipient X25519 inbox keys in both `KeyManager` implementations
- Hidden inbox tags derived from those X25519 public keys
- Per-recipient X25519 ECDH + AES-256-GCM envelope encryption used by invitation transport
- Inbox subscriptions and delivery over Nostr

Relevant code:

- Android `KeyManager`: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/crypto/KeyManager.kt`
- iOS `KeyManager`: `clients/ios/StellarChat/StellarChat/Models/KeyManager.swift`
- Android invitation crypto: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/crypto/GroupCrypto.kt`
- iOS invitation crypto: `clients/ios/StellarChat/StellarChat/Models/GroupCrypto.swift`
- Android `InvitationTransport`: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/nostr/InvitationTransport.kt`
- iOS `InvitationTransport`: `clients/ios/StellarChat/StellarChat/Nostr/InvitationTransport.swift`

### Still missing

The current attestation binds:

- BLS public key
- Stellar Ed25519 public key

It does **not** bind:

- the member’s X25519 inbox public key

That means the remover does not have an authenticated mapping:

`group member BLS key -> verified inbox encryption key`

Without that mapping, the repo cannot safely send per-member rekey envelopes.

---

## 6. Correct End-State Design

The correct design is:

1. Rotate `groupSecret` during removals
2. Stop sending future secret material on the old group channel
3. Deliver the new secret material individually to each remaining member’s inbox
4. Make receivers install the new group state only after validating an authenticated rekey envelope

### 6.1 New authenticated member transport bundle

Add a new off-chain authenticated structure, for example:

```text
SEPMemberTransportBundle {
  blsPubkey: Bytes48,
  stellarEd25519Pubkey: Bytes32,
  x25519InboxPubkey: Bytes32,
  nostrPubkey: Hex32?,   // optional
  version: u16,
  signature: Bytes64
}
```

The signature should cover a domain-separated hash of all fields, for example:

`SHA-256("SEP-XXXX:member-transport-v1" || bls || stellar || x25519 || nostr? || version)`

Signed by the member’s Stellar Ed25519 key.

Why this bundle exists:

- `blsPubkey` identifies the group member cryptographically
- `stellarEd25519Pubkey` preserves the existing attestation trust root
- `x25519InboxPubkey` gives the remover a verified asymmetric delivery target

This is the missing authenticated directory the current protocol needs.

### 6.2 New removal rekey payload

Add a new inbox-delivered payload, for example:

```text
SEPRekeyEnvelope {
  groupID: Bytes32,
  epoch: u64,
  groupSecret: Bytes32,
  salt: Bytes32,
  commitment: Bytes32?,
  memberBundles: [SEPMemberTransportBundle],
  removedMemberKeys: [Bytes48],
  relayHints: [String],
  senderBundle: SEPMemberTransportBundle
}
```

This payload is encrypted per recipient using the existing X25519 inbox transport.

### 6.3 Old-channel state notice becomes non-secret

For removals, the old group topic may still carry a transition notice, but it must not contain:

- the new `groupSecret`
- the new `salt`
- any derivation seed for either one

It may contain:

- group ID
- new epoch
- removed members
- new commitment
- a `rekey_required` indicator

This turns the old group topic into a notice channel, not a secret-distribution channel.

### 6.4 Rotate `groupSecret`, not only `salt`

Removal must generate:

- fresh `groupSecret'`
- fresh `salt'`
- fresh traffic key derived from those values

If `groupSecret` does not change, the removed member still shares the base secret with the rest of the group, and the protocol remains vulnerable to any leakage of the other derivation inputs.

### 6.5 Receiver installation rules

A remaining member installs the new epoch only after:

1. Decrypting a rekey envelope addressed to them
2. Verifying the sender’s authenticated transport bundle
3. Verifying the envelope’s group ID and epoch
4. Verifying the recomputed commitment matches the advertised or chain-confirmed one
5. Persisting the new `groupSecret'` and `salt'`
6. Subscribing to the new topic derived from `groupSecret'`

### 6.6 Recovery model changes

`SEPSaltRequest` / `SEPSaltResponse` are acceptable for missed non-eviction state under the current architecture, but they are not correct for cryptographic removals.

For removal epochs, recovery should be:

- inbox rekey replay
- resend-on-ack-timeout
- explicit inbox resend requests

Not shared-channel salt disclosure.

---

## 7. Why This Is the Correct Direction

This design fixes the root problem because it introduces a real asymmetry:

- remaining members have private inbox keys that the removed member does not control
- the new epoch secret material is delivered only to those inboxes

This is a real cryptographic distinction.

By contrast, the poisoned-salt design only introduces:

- a timing assumption
- a client-behavior assumption

Those are not sufficient against a malicious removed client.

---

## 8. Phased Implementation Plan

The correct design is larger than a one-file patch. It should be implemented in phases.

### Phase 0 — Truth in Security Claims

Goal:

- remove inaccurate claims immediately

Changes:

- document that the current poisoned-salt + `sep_rekey` flow is a best-effort mitigation, not cryptographic eviction
- update UI copy that currently says removed members “will not be able to decrypt future messages”
- point readers to this design doc and the audit

Outcome:

- users and developers stop relying on a guarantee the current protocol does not provide

### Phase 1 — Authenticated Member Transport Bundle

Goal:

- create an authenticated mapping from each group member to an inbox encryption key

Changes:

- define `SEPMemberTransportBundle`
- extend invite/bootstrap and member-add flows to distribute the bundle
- persist the bundle alongside group membership state
- reject malformed or unverified bundles

Open choice:

- keep `nostrPubkey` optional in v1 unless the app needs it for UX or future sender binding

Outcome:

- every remaining member has a verified inbox key available for later rekey

### Phase 2 — Inbox Rekey for Removals

Goal:

- make removals cryptographically sound

Changes:

- define `SEPRekeyEnvelope`
- on removal, generate fresh `groupSecret'` and `salt'`
- send per-recipient rekey envelopes over inbox transport
- old group-topic state notice no longer carries next-epoch secret material
- receivers only install the new epoch from validated inbox rekey

Outcome:

- a removed member who stays on the old group topic still cannot derive the new epoch secret

### Phase 3 — Reliability Layer

Goal:

- make the correct protocol operationally robust

Changes:

- add `SEPRekeyAck`
- add resend scheduling and bounded retry logic
- add optional `SEPRekeyResendRequest`
- persist pending outgoing rekeys until acked or expired
- define relay replay/idempotency rules for inbox rekeys

Outcome:

- correct cryptography without fragile delivery assumptions

### Phase 4 — Remove Secret Recovery From Shared Group Channel

Goal:

- eliminate residual shared-channel secret leakage for eviction epochs

Changes:

- retire `SEPSaltRequest` / `SEPSaltResponse` for removal epochs
- keep them only for legacy or non-eviction transitions if needed
- optionally generalize inbox-delivered rekeying to all epoch transitions, not only removals

Outcome:

- one consistent rule: future epoch secrets are never sent on channels that removed members can still observe

### Phase 5 — Hardening and Compatibility

Goal:

- make the design production-grade

Changes:

- handle key rotation for a member’s inbox key
- define multi-device semantics
- define expiration and replay windows for rekey envelopes
- add integration tests for malicious-client removal scenarios
- optionally bind a hash of the member transport directory into off-chain signed state for stronger consistency guarantees

Outcome:

- the design survives real clients, reconnects, and malicious behavior

---

## 9. Deliberate Non-Goals for the First Correct Version

The first correct version should **not** attempt all of the following at once:

- full MLS tree math
- NIP-44 adoption everywhere
- on-chain storage of inbox keys
- multi-device rekey fanout in the initial patch

The repo already has enough to build a correct v1 removal protocol without those additions. The minimum hard requirement is authenticated per-member asymmetric delivery of fresh removal secrets.

---

## 10. Recommended Next Step

The next concrete implementation step should be **Phase 1**:

- define the authenticated member transport bundle
- wire it into bootstrap / invite / member-add state
- persist it on both clients

Without that step, the repository still has no verified destination key for per-member rekey delivery, and the rest of the design cannot be implemented correctly.
