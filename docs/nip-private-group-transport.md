# NIP-XX: Private Group Relay Transport Anchored on Stellar SEP State

## Preamble

```text
NIP: XX
Title: Private Group Relay Transport Anchored on Stellar SEP State
Author: @rinat-enikeev
Status: Draft
Type: Draft
Created: 2026-04-01
Requires: NIP-01
```

## Abstract

This NIP defines a Nostr transport profile for private group invitations and encrypted group messages whose authoritative group state is anchored on Stellar through SEP-XXXX.

The design keeps Nostr relays dumb. Relays transport opaque events and do not validate group membership, epochs, commitments, or zero-knowledge proofs. Clients verify all group context locally against Stellar state.

The two core routing primitives are:

- a per-recipient hidden inbox tag for invitation delivery
- a hidden group topic derived from the current group secret for ongoing message delivery

In both cases, `group_id` remains inside ciphertext only.

## Motivation

Private group systems need two separate properties:

- globally consistent and auditable group state
- low-latency, off-chain transport for invitations and encrypted group traffic

SEP-XXXX addresses the first property by anchoring group state on Stellar using commitments and zero-knowledge proofs. Nostr is a useful fit for the second property because it already provides:

- a simple event protocol
- broad relay interoperability
- multi-relay publication
- device-level signatures

This NIP defines how to use Nostr as a transport substrate without turning relays into application-aware coordinators.

## Goals

- Use Nostr only as a transport layer
- Keep Stellar authoritative for group state
- Prevent cleartext `group_id` leakage at the relay layer
- Support invitation delivery to a specific recipient
- Support practical filtering without forcing clients to trial-decrypt all traffic
- Reduce, but not eliminate, metadata leakage

## Non-Goals

- Defining SEP-XXXX itself
- Defining MLS wire formats
- Defining a relayer for Stellar transactions
- Requiring relays to store history
- Standardizing BIP39 derivation paths
- Eliminating traffic analysis by a global observer

## Terminology

- `group_id`: The authoritative group identifier defined by SEP-XXXX on Stellar
- `epoch`: The current SEP epoch for the group on Stellar
- `hidden inbox tag`: An opaque recipient-specific routing tag used for invitations
- `hidden group topic`: An opaque group-specific routing tag derived from the current group secret
- `bootstrap payload`: The invitation plaintext before encryption
- `sealed envelope`: The encrypted transport payload carried in the Nostr event `content`

## Relay Model

Relays:

- forward Nostr events
- may apply normal relay-local policies
- are not authoritative for group state
- are not expected to store durable history
- do not validate SEP proofs, commitments, or epochs

Clients:

- publish to one or more relays
- subscribe to hidden inbox tags and hidden group topics
- decrypt payloads locally
- verify referenced group state against local or live Stellar SEP state

## Event Kinds

This NIP defines two application-specific event kinds:

- `24113`: SEP invitation event
- `24114`: SEP encrypted group message event

These values are provisional until assigned or replaced by the implementation community.

## Common Event Rules

All events defined by this NIP:

- MUST be valid NIP-01 events
- MUST use standard Nostr event ID construction and Schnorr signatures
- MUST include `["sep_version", "1"]` in `tags`
- MUST place application payloads only in encrypted form inside `content`
- MUST NOT expose cleartext `group_id` in event tags or content outside ciphertext

`content` MUST be a base64-encoded serialized sealed envelope.

## Sealed Envelope Format

The outer Nostr event carries a serialized encrypted envelope in `content`.

Version 1 envelope fields:

```json
{
  "version": 1,
  "scheme": "application-defined",
  "ephemeral_public_key": "base64-or-null",
  "nonce": "base64-or-null",
  "ciphertext": "base64",
  "authentication_tag": "base64-or-null"
}
```

Rules:

- `version` MUST be present
- `scheme` MUST identify the encryption scheme
- `ciphertext` MUST be present
- the exact encryption scheme is intentionally left to the application
- recipients MUST reject unknown envelope versions

## Invitation Events

### Kind

Invitation events MUST use kind `24113`.

### Tags

Invitation events MUST include:

- `["sep_inbox", <hidden-inbox-tag>]`
- `["sep_version", "1"]`

Invitation events MUST NOT include:

- cleartext `group_id`
- cleartext epoch
- cleartext contract address
- cleartext relay hints

### Bootstrap Payload

After decryption, the invitation payload SHOULD contain:

- `group_id`
- current epoch
- Stellar contract address
- relay hints
- MLS Welcome-like material
- SEP bootstrap material such as salt or equivalent join context

All invitation bootstrap material MUST be encrypted to the intended receiver.

### Hidden Inbox Tag

The hidden inbox tag:

- MUST be deterministic for the intended recipient
- MUST be opaque to relays
- SHOULD be cheap for the recipient to subscribe to
- SHOULD NOT reveal `group_id`

This NIP does not standardize the derivation function, only the transport role of the tag.

### Example

```json
{
  "kind": 24113,
  "tags": [
    ["sep_inbox", "6a9f..."],
    ["sep_version", "1"]
  ],
  "content": "eyJ2ZXJzaW9uIjoxLCJzY2hlbWUiOiIuLi4ifQ=="
}
```

## Group Message Events

### Kind

Encrypted group message events MUST use kind `24114`.

### Tags

Message events MUST include:

- `["sep_topic", <hidden-group-topic>]`
- `["sep_version", "1"]`

Message events MUST NOT include:

- cleartext `group_id`
- cleartext roster information

Applications MAY include additional non-sensitive tags, but SHOULD minimize stable metadata.

### Hidden Group Topic

The hidden group topic:

- MUST be derived from the current effective group secret
- MUST rotate whenever the effective group secret changes in a way that should exclude former members
- SHOULD enable subscribers to filter traffic without trial-decrypting all events

This topic is a routing hint, not an authority signal.

### Inner Payload

The decrypted message payload MAY contain:

- application message content
- MLS handshake material
- epoch transition context
- group-specific delivery metadata

Such payloads MUST be encrypted under the current MLS or equivalent group symmetric key.

## Client Processing Rules

### Invitation Processing

On receiving a kind `24113` event, a client:

1. filters on `sep_inbox`
2. validates the outer event as NIP-01
3. decodes and decrypts the sealed envelope
4. parses the bootstrap payload
5. verifies the referenced `group_id`, epoch, and contract state against SEP state on Stellar
6. accepts or rejects the invitation locally

An invitation that cannot be reconciled with SEP state MUST be ignored.

### Message Processing

On receiving a kind `24114` event, a client:

1. filters on `sep_topic`
2. validates the outer event as NIP-01
3. decodes and decrypts the sealed envelope
4. validates the decrypted payload using MLS or the application’s group messaging layer
5. verifies any group or epoch references against known SEP state when relevant

Relay-observed state MUST NOT be treated as authoritative.

## Verification Boundary

The Nostr public key on the outer event:

- authenticates the transport envelope
- does not prove SEP membership
- does not authorize SEP state transitions

Membership-sensitive operations remain anchored in:

- SEP proofs for on-chain state transitions
- Stellar SEP contract state for epoch and commitment validity
- MLS or equivalent group cryptography for inner message authenticity

## Relay Publication

Senders SHOULD publish the same invitation or message event to multiple relays.

Reasons:

- better delivery probability in a non-persistent relay environment
- reduced dependence on any single relay
- better censorship resistance

Receivers SHOULD cache accepted invitation and message events locally because relay retention is not guaranteed.

## Spam Handling

This NIP does not define custom anti-spam mechanisms.

Clients SHOULD filter using:

- expected event kind
- presence of required `sep_*` tags
- parseable sealed-envelope structure
- successful decryption
- valid SEP-state reconciliation
- MLS or application-level integrity checks

Relay-local anti-spam controls remain compatible with this NIP.

## Privacy Considerations

This NIP improves privacy by:

- encrypting all invitation bootstrap material
- keeping `group_id` inside ciphertext only
- using hidden inbox tags for invitations
- using hidden group topics for ongoing traffic
- keeping relays non-authoritative

This NIP does not eliminate all metadata leakage. Relays may still observe:

- sender Nostr public keys
- timing
- size
- relay fanout patterns
- repeated use of the same hidden inbox tag or hidden topic

Implementations SHOULD consider:

- coarse ciphertext padding
- rebroadcast cadence control
- batching
- optional dummy traffic where stronger traffic-analysis resistance is required

## Security Considerations

- Clients MUST verify invitation and message context against Stellar SEP state where applicable
- Hidden routing tags are transport hints only and MUST NOT be treated as authority signals
- Rotating hidden group topics on epoch or secret changes is important to reduce ex-member observability
- Stable Nostr device keys improve usability but increase sender linkability
- Device-seed compromise can correlate transport and membership layers if both are derived from the same root

## Compatibility

This NIP is transport-compatible with NIP-01 because it uses standard Nostr event structure, event IDs, and Schnorr signatures.

This NIP is application-specific and assumes the existence of:

- SEP-XXXX for authoritative group state on Stellar
- an MLS or equivalent encrypted group messaging layer

## References

- NIP-01
- SEP-XXXX
- RFC 9420
- BIP39
