# Security Audit v4 — Epoch Transition Confidentiality

**Date:** 2026-04-03
**Scope:** Epoch transitions, member removal, state update transport, iOS + Android clients
**Method:** Static code review with call-site tracing against the current source tree

---

## Executive Summary

The repository does not currently provide cryptographic exclusion of removed members. On both clients, the traffic key is deterministically derived from a long-lived `groupSecret`, `epoch`, and `salt`, while the state update that announces the next epoch is encrypted with the previous epoch key and includes the new `epoch` and `salt`. A removed insider who still knows `groupSecret` can decrypt that update, derive the next traffic key, and continue reading or writing on the group channel.

This is not an edge-case bug in one platform. The same transition pattern exists on Android and iOS, and the surrounding code and UI currently overstate the security properties of member removal and key rotation.

---

## Confirmed Findings

### 1. CRITICAL: Removed members can derive the next traffic key after removal

Both clients derive the group traffic key from `HKDF(groupSecret || epoch || salt)`:

- Android: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/crypto/GroupCrypto.kt:41-49`
- iOS: `clients/ios/StellarChat/StellarChat/Models/GroupCrypto.swift:34-52`

Both clients also define the state update as a protocol message carrying the new `epoch` and `salt`:

- Android SDK: `kotlin-mls/src/main/java/com/stellarmls/mls/GroupStateUpdate.kt:5-17`
- Swift SDK: `swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift:6-34`

In the removal flow, both clients:

1. Remove the member from the local member list
2. Increment the epoch
3. Generate a fresh salt
4. Persist the candidate state
5. Broadcast the new state update with the **previous** traffic key

Evidence:

- Android removal and broadcast path: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt:745-805`
- Android JSON encoding includes the new `epoch` and `salt`: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt:1872-1902`
- iOS removal and broadcast path: `clients/ios/StellarChat/StellarChat/StellarChatApp.swift:615-697`

Exploit logic:

- The removed member already knows `groupSecret`.
- The removed member also still knows the previous traffic key, because they were a valid member before eviction.
- The removal update is encrypted with that previous key and reveals the next `epoch` and `salt`.
- Those three inputs are sufficient to derive the next traffic key.

Impact:

- Member removal does not provide post-removal confidentiality.
- A malicious client can continue decrypting group traffic after eviction.
- Because the same key is used for protocol traffic, the removed client can continue consuming later transition messages as well.

Remediation direction:

- Stop treating `epoch`/`salt` rotation alone as an eviction mechanism.
- Redesign transition delivery so excluded members do not learn the next traffic-key material.
- If eviction is meant to be cryptographic, the protocol needs a new secret distribution mechanism, not just a new salt.

### 2. HIGH: Product and code claims overstate the security effect of member removal

The repository currently tells users and developers that key rotation prevents removed members from decrypting future messages, but the traced code does not provide that property.

Evidence:

- Android crypto comment: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/crypto/GroupCrypto.kt:41-43`
- Android removal UI text: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/ui/screens/GroupInfoScreen.kt:190-198`
- iOS removal UI text: `clients/ios/StellarChat/StellarChat/Views/GroupInfoView.swift:114-139`
- iOS crypto comment already says there is no ratcheting mechanism: `clients/ios/StellarChat/StellarChat/Models/GroupCrypto.swift:34-37`

Impact:

- Operators and users are told eviction is cryptographically enforced when it is not.
- Internal documentation is inconsistent across platforms, which increases the chance of future security regressions and false assumptions in new features.

Remediation direction:

- Align UI copy, comments, and docs with the real security model immediately.
- Do not claim that removal blocks future decryption until the underlying protocol actually changes.

### 3. HIGH: On-chain confirmation does not protect salt confidentiality, and both clients degrade to off-chain acceptance

Published groups use chain confirmation for `epoch` and `commitment`, but the chain does not distribute `salt`. Clients still adopt `update.salt` from the off-chain protocol message, and when chain fetches fail both clients explicitly accept remote state anyway.

Evidence:

- Android applies `update.salt` after chain checks and accepts updates when chain verification fails: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt:966-1003`
- Android fork resolution also falls back to accepting remote state and `update.salt`: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt:898-955`
- iOS applies `update.salt` after chain checks and accepts updates when chain verification fails: `clients/ios/StellarChat/StellarChat/StellarChatApp.swift:936-981`
- iOS fork resolution also falls back to remote `update.salt`: `clients/ios/StellarChat/StellarChat/StellarChatApp.swift:880-930`
- Salt is also shared through explicit protocol messages: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt:1782-1801`, `clients/ios/StellarChat/StellarChat/StellarChatApp.swift:1673-1687`

Impact:

- The chain anchor does not repair the key-disclosure problem once the next salt is revealed on the old channel.
- Transition correctness and confidentiality still depend on the off-chain protocol path.
- “Published on chain” should not be interpreted as meaning the next traffic key is protected from removed insiders.

Remediation direction:

- Treat on-chain anchoring and traffic-key distribution as separate security problems.
- Document that chain confirmation validates state progression, not secrecy of next-epoch key material.
- Remove graceful-degradation language that could be read as preserving the same security properties when chain checks fail.

### 4. MEDIUM: The same design flaw weakens manual key rotation, not just member removal

This issue is most severe during eviction, but the underlying design is broader: any transition that publishes the next `epoch` and `salt` under the current traffic key is recoverable by anyone who still knows `groupSecret` and can decrypt the current channel.

Evidence:

- Android key rotation uses the same transition helper and the same “broadcast with previous key” path: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt:755-805`
- iOS key rotation uses the same transition helper and the same “previous key” transport: `clients/ios/StellarChat/StellarChat/StellarChatApp.swift:624-697`

Impact:

- Manual key rotation does not remove an already-informed insider from the channel.
- Once a removed client learns one post-removal epoch, later rotations do not restore exclusion unless the shared secret itself changes out of band.

Remediation direction:

- Treat current “key rotation” as an epoch refresh, not a membership-eviction primitive.
- If the desired property is exclusion, later rotations must not be recoverable from the previously shared secret.

---

## Security Model Clarification

The current implementation behaves as follows:

- `groupSecret` is the long-lived shared secret used to derive group traffic keys.
- Traffic keys are derived deterministically from `groupSecret`, `epoch`, and `salt`.
- Epoch transitions rotate `salt` and increment `epoch`, but do not rotate `groupSecret`.
- State updates and salt recovery messages distribute next-epoch salt over the group protocol channel.
- iOS explicitly documents that there is no ratcheting mechanism in `clients/ios/StellarChat/StellarChat/Models/GroupCrypto.swift:34-37`.

What this model can provide:

- Confidentiality against outsiders who do not know `groupSecret` and cannot decrypt the current channel.
- Deterministic traffic-key agreement among honest, synchronized members.

What this model cannot provide:

- Cryptographic eviction of removed insiders
- Forward secrecy across member removal
- Post-compromise security
- Recovery of exclusion through salt rotation alone

---

## Why Symmetric-Only Fixes Fail

Several intuitive patches do **not** fix this class of bug:

- Omitting `salt` from the removal update and deriving it deterministically from values like `groupSecret`, `epoch`, removed-member key, or the remaining member set
- Sending a second “re-key” message encrypted under a transition key derived from the old traffic key plus public membership data
- Replacing the current random next salt with any function of values already known to the removed member

These fail for the same reason: at removal time, the evicted member still has all of the symmetric inputs that the remaining members share today:

- the old traffic key
- the long-lived `groupSecret`
- the current epoch state
- the membership roster before and after the removal, once the removal notice is observed

If the next traffic secret is derivable solely from that shared symmetric state, the removed member can derive it too. There is no hidden asymmetry to exploit with a clever KDF.

The only clean way to make removal cryptographically meaningful is to introduce a secret that is delivered **only** to the remaining members. In this repository, that means asymmetric per-recipient delivery.

---

## Clean Cryptographic Removal Plan

The cleanest design that fits the current codebase is:

1. Rotate the **group secret**, not only `epoch` and `salt`.
2. Deliver the new secret material **individually** to each remaining member.
3. Use the existing X25519 inbox encryption path for those rekey envelopes, instead of trying to salvage the old group channel.

This repository already has a per-recipient encrypted transport:

- X25519 inbox key generation in Android `KeyManager`: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/crypto/KeyManager.kt:75-85,149-158`
- X25519 invitation encryption in Android: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/crypto/GroupCrypto.kt:96-210`
- X25519 invitation encryption in iOS: `clients/ios/StellarChat/StellarChat/Models/GroupCrypto.swift:72-168`
- Nostr inbox delivery on both clients: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/nostr/InvitationTransport.kt`, `clients/ios/StellarChat/StellarChat/Nostr/InvitationTransport.swift`

What is missing is not the cryptographic envelope. What is missing is authenticated membership metadata that tells the remover which inbox key belongs to which BLS member.

### Required protocol changes

#### 1. Add a per-member transport key to the authenticated member record

Each member needs a stable inbox key bundle that is distributed to the group and bound to the member identity. The current attestation binds BLS to Stellar Ed25519 only:

- iOS: `clients/ios/StellarChat/StellarChat/Models/KeyAttestation.swift`

The protocol needs a new attested member transport bundle along the lines of:

- `blsPubkey`
- `stellarEd25519Pubkey`
- `x25519InboxPubkey`
- optional `nostrPubkey` if the app wants message-sender UX continuity
- signature by the Stellar Ed25519 key over a domain-separated hash of the whole bundle

That bundle must be distributed when a member is added and stored as part of the authenticated group state, so future removals have a verified inbox destination for each remaining member.

#### 2. Rekey by generating a fresh `groupSecret'` and fresh `salt'`

For a removal transition:

- remove the member from the roster
- increment `epoch`
- generate a brand-new `groupSecret'`
- generate a brand-new `salt'`
- recompute commitment from the new member set and new epoch material

This is the critical difference from the current design. If `groupSecret` stays constant, the removed member stays in the derivation universe forever.

#### 3. Stop sending next-epoch secret material on the old group channel

The old-topic state update may still announce:

- group ID
- removed member list
- new epoch
- new commitment
- a flag that a rekey is required

But it must **not** contain:

- `groupSecret'`
- `salt'`
- any derivation seed from which either can be reconstructed

At that point the old group channel becomes only a public transition notice for former members, not the carrier of future secrets.

#### 4. Send per-recipient rekey envelopes to the remaining members’ inboxes

After the chain update is accepted, the remover sends one encrypted rekey envelope per remaining member using the existing X25519 inbox transport.

Each rekey envelope should contain:

- `groupID`
- `epoch'`
- `groupSecret'`
- `salt'`
- `commitment'`
- current member list or authenticated delta
- relay hints if needed for topic migration
- sender attestation / transport-bundle proof needed to authenticate the sender

Because each envelope is encrypted to a specific remaining member’s X25519 inbox key, the removed member cannot decrypt it.

#### 5. Make receivers install the new state only after rekey-envelope validation

A remaining member should install the new state only after:

- decrypting a rekey envelope addressed to them
- verifying the sender’s authenticated transport bundle
- verifying `groupID`, `epoch`, and `commitment` match the chain-confirmed transition

Only then should the client:

- persist `groupSecret'`
- persist `salt'`
- derive the new traffic key
- subscribe to the new topic derived from `groupSecret'`
- stop using the old topic

#### 6. Treat removal recovery as inbox replay, not salt replay

Today missed transitions are repaired with `SEPSaltRequest` / `SEPSaltResponse`. That mechanism is incompatible with cryptographic eviction because it republishes key material on the shared group channel.

The clean recovery model after this redesign is:

- durable inbox rekey envelopes
- resend-on-ack-timeout
- optional explicit “please resend rekey for epoch N” inbox requests between authenticated remaining members

Not shared-channel salt disclosure.

### Why this is cleaner than NIP-44 here

NIP-44 could be added, but it is not necessary for a clean fix in this repository:

- the repo already has X25519 ECDH envelope encryption for invitations
- the repo already has hidden inbox tags and inbox subscriptions
- the missing piece is authenticated mapping from group member identity to inbox key

Reusing the existing invitation transport as a general “member inbox transport” is the smallest clean protocol extension.

### Implementation order

1. Introduce the authenticated member transport bundle and persist it with each member.
2. Define a new `SEPRekeyEnvelope` payload for per-recipient inbox delivery.
3. Change removal flow to rotate `groupSecret` and `salt`.
4. Stop placing next-epoch secret material in `SEPGroupStateUpdate`.
5. Replace salt-recovery for removal epochs with inbox rekey replay / resend.
6. Update UI and docs so “member removal” means exclusion only after the inbox rekey protocol is in place.

---

## Recommended Remediation Order

1. Stop claiming that member removal or key rotation prevents removed members from decrypting future messages.
2. Introduce an authenticated member transport bundle that binds each BLS member to an inbox encryption key.
3. Redesign epoch-transition transport so excluded members do not learn the next traffic-key material from the old channel.
4. Rotate `groupSecret` during removals and deliver the new epoch material per-recipient over inbox transport.
5. Retire shared-channel salt recovery as the recovery mechanism for removal transitions.
6. Align Android, iOS, SDK docs, and product copy around the actual guarantees until that redesign lands.

---

## Bottom Line

The current repository implements epoch advancement, not cryptographic eviction. As long as the next `epoch` and `salt` are delivered under the old traffic key while `groupSecret` remains unchanged, a removed insider can stay on the channel.
