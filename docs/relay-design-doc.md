## Preamble

```
Document: Relay Layer Design
Title: Nostr Relay Transport for Private Group Messaging Anchored on Stellar
Author: @rinat-enikeev
Status: Draft
Created: 2026-03-31
Updated: 2026-03-31
Version: 0.0.1
Discussion: TBD
```

## 1. Introduction

This document accompanies SEP-XXXX and describes a relay-layer architecture for transporting private group coordination and encrypted group messages over Nostr while keeping group membership state anchored on Stellar.

The design goal is strict separation of concerns:

- **Stellar** is the authoritative source of truth for group state, epochs, and membership commitments
- **MLS** provides group key establishment and message confidentiality/integrity
- **Nostr relays** provide a dumb, global, best-effort transport substrate

The relay layer does not define membership, does not authorize updates, and does not persist canonical group state. It exists to carry invitations and encrypted group traffic between devices that independently verify group validity against on-chain SEP state.

---

## 2. Background

Private group systems need two different properties that are often in tension:

- a globally consistent, auditable group state
- low-friction message transport between devices

The SEP addresses the first property by anchoring group state on Stellar with commitments and zero-knowledge proofs. That solves ordering, integrity, and privacy of the roster at the consensus layer.

But blockchains are not good message buses. Group traffic is frequent, latency-sensitive, and should not be stored on-chain. A separate transport layer is needed for invitations, MLS bootstrap material, and encrypted group messages.

Nostr is a useful candidate for that transport layer because it offers:

- a simple event-based protocol
- global relay interoperability
- multiple-relay fanout
- device-level identities and signatures
- no requirement that relays understand application semantics

This document treats Nostr as a transport substrate only. Group state remains on Stellar; MLS remains the cryptographic messaging layer; the relay layer exists to move opaque encrypted payloads between participants.

---

## 3. Problem

If the transport layer is designed naively, it can reintroduce the privacy leaks the on-chain design removed.

**Visible group activity.** Even if message contents are encrypted, relays can observe event timing, volume, fanout, and event size. If group identifiers or routing metadata are cleartext, relays learn which devices are active in which groups and when those groups change state.

**Sender linkability.** Nostr events are signed. If every group message is wrapped in an outer event signed by a stable device key, relays can correlate a device's activity over time, even when they cannot decrypt the payload.

**Recipient discovery.** Invitation delivery is particularly sensitive. A relay seeing who receives bootstrap material can infer the social graph even if the invitation payload itself is encrypted.

**Spam and flooding.** Because relays are intentionally dumb and global, the system must expect unsolicited events, replay attempts, and volume attacks. Clients need enough structure to filter junk locally without requiring relays to become application-aware.

**State desynchronization.** Since Nostr is not authoritative, clients must not trust relay-observed group state. A transport message referring to a group or epoch is only valid after local verification against Stellar SEP state.

These issues mean the relay layer must be explicit about what metadata is public, what is encrypted, how clients verify state, and which traffic-analysis leaks remain in scope.

---

## 4. Scope

### In scope

- A Nostr-based transport architecture for invitations and encrypted group messages
- Separation between on-chain group state and off-chain message transport
- A device-oriented identity model derived from a single BIP39 seed
- Invitation payload structure and relay broadcast model
- Client-side verification of relay traffic against cached or live Stellar SEP state
- Metadata minimization strategies for a dumb-relay environment
- Relay assumptions for non-persistent, best-effort delivery
- Spam and flooding considerations at the client/protocol level

### Out of scope

- Changes to the authoritative SEP group-state contract on Stellar
- A new relay protocol or relay-side application logic
- Custom relay storage guarantees or archival infrastructure
- Detailed MLS wire format design
- Wallet UX, seed backup UX, or secure enclave implementation details
- Full network-layer anonymity against global passive adversaries
- Token- or fee-based anti-spam systems beyond existing Nostr mechanisms

---

## 5. Solution

The proposed relay layer has five core properties.

**1. Stellar remains authoritative.** Every group has a `group_id`, current epoch, and commitment state stored on Stellar. Relay traffic is never authoritative. Clients validate invitation and message context locally against cached or live SEP state before accepting it.

**2. Nostr relays are dumb transport only.** Relays forward events, apply their normal local policies, and do not understand group semantics. They do not validate epochs, proofs, commitments, or membership. They are not expected to store durable history.

**3. Invitations use per-recipient hidden inbox tags and fully encrypted bootstrap data.** A group creator may publish an invitation event to many relays, but it is routed via an opaque inbox tag derived for the intended receiver. The invitation bootstrap payload is encrypted to that receiver and contains the group coordination material needed to join.

**4. Group messages use hidden group topics derived from the current group secret.** The outer event is a transport envelope signed by the sender's Nostr device key. The payload is encrypted under the current group traffic secret, and routing uses an opaque topic derived from that same secret rather than a cleartext `group_id`.

**5. Metadata minimization is a first-class design goal.** The relay layer accepts that some metadata remains observable, but reduces leakage by using opaque inbox/topic tags, encrypted payloads, non-authoritative relays, multiple-relay fanout, client-side verification, optional padding, and transport patterns that avoid exposing roster state directly.

---

## 6. Relay Model

### 6.1 Transport semantics

Relays are assumed to provide:

- best-effort publication
- best-effort delivery to subscribers
- no application-specific validation
- no required durable storage
- standard Nostr anti-spam and policy controls

Relays are **not** trusted with:

- group membership state
- invitation decryption
- message decryption
- epoch validation
- SEP proof validation

### 6.2 Global relay topology

Clients may publish to and subscribe from many relays simultaneously. This serves two purposes:

- higher delivery probability in a non-persistent relay environment
- reduced dependence on any single relay for availability or censorship resistance

Relay hints may be included in invitation events, but those hints are advisory only. A client may use any relay set it chooses.

### 6.4 Hidden routing metadata

The relay layer does not use cleartext `group_id` as a routing key.

Instead it uses two opaque routing primitives:

- a **per-recipient hidden inbox tag** for invitation delivery
- a **hidden group topic** for ongoing group messages

Both are designed to be cheap for clients to filter on, while revealing less to relays than a stable cleartext group identifier.

### 6.3 No relay-side history assumption

The architecture assumes relays may drop traffic, prune old events quickly, or refuse storage entirely. This means:

- invitations may need repeated publication
- senders may need multi-relay fanout
- clients should cache relevant events locally
- loss recovery depends on peers and fresh retransmission, not relay archives

---

## 7. Identity and Key Derivation

### 7.1 Single-seed device model

> **Status: Not yet implemented.** The current apps derive keys from a Nostr secp256k1 secret via HKDF. BIP39 mnemonic generation, display, and recovery are planned but not yet available. Users currently have no key backup mechanism.

Each device has a BIP39 seed from which it derives all required identities:

- a Stellar key for interacting with the SEP ecosystem
- a BLS12-381 identity key for SEP membership proofs
- a Nostr device identity key for outer transport signatures
- MLS device-specific secrets as required by the application

The design target is operational simplicity: a user restoring a device from its BIP39 seed can recover all identity roots needed to rejoin the transport and verification stack.

### 7.2 Device identity

The relay layer uses one Nostr identity per BIP39 seed on the device. This means:

- outer Nostr events are signed by a stable device-level transport identity
- MLS sender authenticity is still enforced inside the encrypted group payload
- transport identity and group membership identity are intentionally distinct, but derived from the same seed root

This coupling improves recoverability but increases the consequence of device-seed compromise. That is an accepted tradeoff in this design.

### 7.3 Verification boundary

The Nostr key does not prove SEP membership. It only authenticates transport events at the relay layer. Membership authorization remains:

- on Stellar for group state transitions
- in SEP proofs for membership-sensitive operations
- in MLS for encrypted group messaging

---

## 8. Invitation Flow

### 8.1 Invitation event contents

An invitation event is published under a per-recipient hidden inbox tag and carries an encrypted bootstrap payload intended for a specific receiver.

That encrypted payload may contain:

- `group_id`
- current epoch
- Stellar contract address
- relay hints
- an MLS Welcome-like payload
- SEP salt/bootstrap material

Only the receiver should be able to decrypt this bootstrap material. The relay sees an invitation event, its hidden inbox tag, and generic outer transport metadata, but not the group coordination data needed to join.

The hidden inbox tag should be deterministically derived from receiver-held secret material so that:

- the intended receiver can subscribe to it efficiently
- relays can match and route events without learning the receiver's actual group relationships
- unrelated observers cannot derive the recipient mapping without the receiver secret

### 8.2 Broadcast model

Invitations are broadcast to many relays rather than routed through a private inbox relay. This maximizes reach in a global, dumb-relay environment and avoids requiring relay specialization.

The cost is metadata leakage:

- relays can observe that an invitation event exists
- relays can correlate broadcast timing

This design accepts that tradeoff and compensates through ciphertext opacity, multi-relay distribution, and local validation rather than relay trust.

### 8.3 Client processing

When a client receives an invitation event, it:

1. parses the outer event
2. attempts to decrypt the receiver-targeted bootstrap material
3. verifies the referenced `group_id`, epoch, and contract state against cached or live Stellar SEP data
4. validates the MLS bootstrap data
5. accepts or rejects the invitation locally

An invitation that cannot be reconciled with SEP state is ignored.

---

## 9. Message Transport

### 9.1 Outer envelope

Messages are transported in Nostr events signed by the sender's Nostr device key. The outer envelope exists for routing, relay acceptance, and transport-level authenticity.

The outer envelope should be treated as transport metadata only. It must not be trusted as proof of SEP membership or group validity.

### 9.2 Inner payload

The inner payload is encrypted with the current MLS/group symmetric key. This payload may contain:

- application message content
- MLS handshake material
- epoch transition context
- delivery hints understood only by clients

Relays cannot interpret this content.

### 9.3 Group binding

In this design, `group_id` is kept inside ciphertext only. Ongoing message traffic is routed using a hidden group topic derived from the current group secret rather than a cleartext group identifier.

This hidden group topic serves as a practical middle ground:

- clients can subscribe efficiently without trying to decrypt all traffic
- relays can forward matching events without understanding the underlying group
- observers do not learn the actual `group_id` from the transport layer alone

The hidden group topic should rotate when the effective group secret changes, especially on epoch transitions that remove members. This prevents former members from continuing to track future traffic purely from stale routing metadata.

This means the privacy posture is:

- **message content confidentiality:** strong
- **group roster confidentiality:** strong relative to relay contents and on-chain state
- **transport metadata confidentiality:** improved, but still partial

Clients must still verify that any message claiming to belong to a given group is consistent with locally known SEP state.

---

## 10. Client Verification Model

Relay traffic is advisory. Clients are required to verify group context locally against Stellar SEP state.

At minimum, a client receiving an invitation or message should confirm:

- the `group_id` exists
- the referenced epoch is consistent with known SEP state
- the group commitment for that epoch matches local expectations
- any epoch transition material is not stale or conflicting

Clients may use cached Stellar state for performance, but the blockchain remains the final authority when conflicts arise.

This separation is crucial: relays transport bytes; clients decide validity.

---

## 11. Metadata Protection

The relay layer does not eliminate metadata leakage, so mitigation must be explicit.

### 11.1 What remains visible

Depending on the final event schema, relays may still observe:

- sender Nostr key
- publication time
- event size
- relay fanout pattern
- stable or semi-stable hidden inbox/topic tags
- burst patterns during group formation or epoch changes

### 11.2 Mitigations

Recommended mitigations include:

- encrypt all invitation bootstrap material end-to-end
- use per-recipient hidden inbox tags for invitation delivery
- use hidden group topics derived from current group secret for message delivery
- keep relay semantics dumb so relays never learn authoritative state
- publish to many relays to reduce single-relay visibility
- pad invitation and message ciphertexts into coarse size buckets
- allow periodic rebroadcast and batching to reduce exact timing disclosure
- consider optional dummy traffic if traffic analysis resistance becomes a primary requirement
- keep membership state and authorization off the relay layer entirely

### 11.3 Residual leakage

Even with these mitigations, a relay observer may still infer:

- that a given device is active
- that some events belong to the same opaque topic over time
- that invitation or epoch-related traffic occurred at a given time

This architecture is therefore designed to protect **content and roster privacy first**, while reducing but not eliminating **traffic-analysis leakage**.

---

## 12. Spam and Flooding

Because relays are dumb and global, the system must expect spam and flooding.

This design deliberately relies on existing Nostr mechanisms rather than relay-side group logic. Clients should assume they will receive irrelevant or malicious traffic and should filter locally using:

- expected event kinds
- parseability of the outer envelope
- successful decryption of invitation/message payloads
- consistency with known `group_id` and SEP epoch state
- MLS authenticity checks

The relay layer should not require special anti-spam semantics beyond what the Nostr ecosystem already supports.

---

## 13. Implementation Plan

### Phase 1 — Event schema

- Define invitation and message event kinds
- Specify which fields are cleartext versus encrypted
- Specify ciphertext padding rules and relay-hint format

### Phase 2 — Key derivation

- Standardize BIP39 derivation paths for Stellar, BLS12-381, Nostr, and MLS device material
- Document device recovery behavior and seed portability assumptions

### Phase 3 — Client verification

- Implement local verification against cached or live Stellar SEP state
- Define invalid-event handling rules
- Add replay, stale-epoch, and conflicting-state tests

### Phase 4 — Relay interoperability

- Test publication and reception across many public relays
- Measure invitation/message delivery under non-persistent relay behavior
- Benchmark padding and batching tradeoffs

### Phase 5 — Privacy hardening

- Evaluate optional cover traffic
- Measure metadata leakage under realistic relay observation
- Refine batching, retransmission, and relay-selection strategies

---

## Appendices

### A. Design principles

- Stellar is authoritative
- MLS protects message contents
- Nostr is transport only
- Relays are dumb
- Clients verify locally
- Metadata should be minimized, not assumed away

### B. Open questions

- How aggressive ciphertext padding should be by default
- Whether invitation rebroadcast cadence should be standardized
- Whether optional dummy traffic is necessary for the first version
- Whether relay selection should be fully client-defined or partially suggested by the creator

### C. References

- **SEP-XXXX** — Private Group Membership Registry with Zero-Knowledge Proof
- **RFC 9420** — The Messaging Layer Security (MLS) Protocol
- **Nostr** — Notes and Other Stuff Transmitted by Relays
- **BIP39** — Mnemonic code for generating deterministic keys
