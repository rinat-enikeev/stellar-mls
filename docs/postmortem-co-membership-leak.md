# Postmortem: Co-Membership Leak via Stable Outer Nostr Keys

**Date:** 2026-04-16
**Severity:** HIGH (privacy)
**Duration of vulnerability:** From initial group chat implementation through 2026-04-16
**Platforms affected:** iOS and Android
**Status:** Resolved — commit `b36c9dd` on `rinat/ephemerial`

---

## Summary

Every encrypted group message (kind `44114`) and every per-recipient invitation/rekey (kind `34113`) was signed with the sender's **long-term** secp256k1 Nostr identity key. Because these kinds also carry a **stable hidden topic tag** (per group) or **stable inbox tag** (per recipient), any relay — or any observer with a relay's event log — could trivially reconstruct the co-membership graph:

> "These N pubkeys all published to hidden-topic `T` → these N devices are in the same group."

This contradicted the explicit SEP privacy goal: *an observer cannot determine who is in the same group*. The leak had been "known" — the NIP spec called it out as a *trade-off* ("stable device keys improve usability but increase sender linkability") — but it was never escalated to a privacy violation and never fixed.

The incident was not a production outage. It was a **framing failure in the spec**: a hard-to-invert cryptographic property was documented as a usability preference.

---

## Timeline

| When | What |
|------|------|
| Pre-2026-04-16 | Both clients use `KeyManager.publicKeyHex` (long-term identity) to sign every `44114` / `34113` event. NIP §Security Considerations lists sender linkability as a trade-off. |
| 2026-04-16 | User reads `NostrEventBuilder.kt` and asks: *"I'm passing a Nostr pubkey for group messages. Does it mean the group privacy is broken? According to SEP there should not be a possibility to determine who is in the same group."* |
| 2026-04-16 | Audit confirms: yes, co-membership is derivable from any relay that retained events. The "trade-off" framing was wrong — this was a straightforward violation of a load-bearing privacy property. |
| 2026-04-16 | Plan: ephemeral per-event secp256k1 keys for `44114` + `34113`; keep long-term key for `24242` (Blossom upload quotas). Rust FFI already accepts arbitrary 32-byte secrets — no Rust changes needed. |
| 2026-04-16 | Implemented on both platforms: `RustBackedNostrSigner.ephemeral()` factory, `ephemeralSigner` parameter threaded through every send callsite. Receive side switched from `event.pubkey` to BLS pubkey hex for sender identity. NIP §Outer Event Identity added as a normative section. |

---

## What went wrong

### 1. A privacy invariant was documented as a usability trade-off

The NIP had a single bullet in Security Considerations:

> *"Stable Nostr device keys improve usability but increase sender linkability."*

That sentence is technically true and **strategically misleading**. "Sender linkability" sounds like a profile-building concern affecting an individual. The actual consequence was **group linkability**: partitioning the user base into co-membership clusters from public relay data alone. The SEP design explicitly promises the second is not possible. The NIP quietly said otherwise.

No one cross-checked the NIP's "trade-off" language against SEP's "you cannot determine who is in the same group" guarantee. The two documents contradicted each other for the entire lifetime of the transport layer.

### 2. The fix was cheap, and no one tried

`sep_nostr_derive_public_key` and `sep_nostr_sign_event_id` in `src/jni_ffi.rs` / `src/ffi.rs` already accepted arbitrary 32-byte secrets. A 5-line factory on each platform and a default-nil `ephemeralSigner` parameter was all that stood between "stable device-linked group graph" and "per-event ephemeral author." The cost of doing it right was roughly the cost of writing the trade-off bullet that justified not doing it.

### 3. The receive side made an implicit assumption that broke silently

Both clients used `event.pubkey` as the sender identity for:

- `isMine` display logic (comparing against `keyManager.publicKeyHex`)
- Salt-request rate-limit keys (`"${senderPubkey}:${epoch}"`)
- Removal-notice display name lookup (`contactAliasStore.displayName(senderPubkey)`)
- `ChatMessage.senderPubkey` persistence

With ephemeral outer keys, `event.pubkey` becomes random-per-event. Every one of these breaks:

- `isMine` always false → user sees their own messages as if from a stranger
- Rate limiting never coalesces → salt-request DoS amplification
- Alias lookup always misses → names disappear from removal notices
- `senderPubkey` column becomes uncorrelated noise

**This was not in the original plan.** It was caught by reading the receive path during the debug-log audit step, only because the plan included an explicit "audit logs that reference sender pubkey" task. Had the plan only specified the send-side change, the feature would have shipped broken and looked correct in every unit test.

Inner BLS authentication was always the real sender check — the BLS pubkey lives inside the encrypted wrapper and is verified against the MLS roster. The outer Nostr pubkey was **never** security-critical; it was being used as a convenient stable identifier because it happened to be stable. The ephemeral change forced us to notice that the convenience had no principled backing.

---

## Root cause

Two coupled misalignments:

1. **Spec drift.** Two normative documents (SEP + NIP) made contradictory claims about what an observer can learn. The NIP's framing ("trade-off") obscured the contradiction instead of surfacing it.
2. **Identity coupling.** The outer transport key was used both as a signature key *and* as a sender identity. Once those two roles are separated — signature comes from an ephemeral key, identity comes from the inner BLS pubkey — the privacy fix is mechanical. Keeping them coupled made the fix look scarier than it was.

---

## The fix

See commit `b36c9dd`. In short:

- `RustBackedNostrSigner.ephemeral()` (Kotlin + Swift): CSPRNG-backed 32-byte secret, single-use.
- `NostrEventBuilder.build` / `NostrEvent.build` grew an optional `ephemeralSigner` parameter; when provided it replaces the `KeyManager` identity for pubkey + signature.
- Every `44114` / `34113` callsite on both clients now passes `ephemeralSigner = RustBackedNostrSigner.ephemeral()`. Kind `24242` (Blossom) explicitly keeps the long-term key for upload-quota enforcement.
- Transport callbacks reshaped to pass **BLS pubkey hex** as the sender identity. `isMine`, contact aliases, salt-request rate limits, and removal-notice displayName all migrated to BLS hex.
- NIP §Outer Event Identity added as a normative section; §Security Considerations rewritten to require ephemeral outer keys for `44114` / `34113` and to state explicitly that receivers MUST NOT use the outer pubkey for identity.

---

## What prevents recurrence

1. **The NIP is now internally consistent with SEP.** A future reader cannot come away thinking stable outer keys are acceptable on `44114` / `34113`.
2. **A unit test asserts pubkey uniqueness per event.** `NostrEventTests.ephemeralSignerUniqueness` and `SwiftMLSTests.ephemeralNostrSignersAreUniqueAndProduceValidKeys` fail if anyone re-plumbs a stable key through the ephemeral path.
3. **`event.pubkey` is no longer threaded through receive callbacks.** The callback signatures now carry BLS hex, forcing new consumers to use the identity the spec endorses.

---

## What did not prevent this

- **Internal security audits (v1–v4).** Every audit examined the *encryption* story — MLS, BLS, rekey, removal — and none examined the *transport metadata* story. A relay operator's view of the event log was not part of any audit's threat model.
- **The NIP itself.** The document that should have flagged this *did* flag it, then immediately downgraded it to a trade-off without justification.
- **Code review.** The same person wrote the NIP and the implementation; the "trade-off" framing traveled between documents without ever meeting an adversarial reader.

The issue was caught by a single user question that took the SEP guarantee at face value.

---

## Lessons

1. **"Trade-off" is a red flag in a security spec.** If a stated property is weakened in a Security Considerations section, the weakening must cite either an adversary model or an unfixable constraint. "Improves usability" is neither.
2. **Cross-check privacy claims across layers.** SEP and the NIP were developed in parallel and diverged on a load-bearing guarantee for months. A one-page "claims matrix" — *what does SEP promise, what does the transport deliver* — would have caught the gap.
3. **Separate the signing key from the identity claim.** Any protocol that uses a transport-layer signature key as a stable sender identity has coupled two roles that should move independently. The coupling is what made this look expensive to fix.
4. **When ripping out an implicit identity, grep the receive side too.** The send-side change was the obvious part of the plan; the receive-side cleanup was where the feature would have shipped broken. Plans that only describe the write path are half-plans.
