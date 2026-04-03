# Security Audit v3 — SEP Protocol & Stellar Chat App

**Date:** 2026-04-03
**Scope:** Cryptography, protocol, transport, storage (iOS + Android)
**Method:** Static code review with claim verification against source

> This audit supersedes v2. The v2 report contained 4 CRITICAL findings, 3 of which
> were false positives due to incomplete code tracing. This version verifies every
> claim against the actual codebase.

---

## v2 Retraction Summary

| v2 Claim | v2 Severity | v3 Verdict | Reason |
|----------|-------------|------------|--------|
| C2: `deriveSalt` lacks domain separation | CRITICAL | **False positive** | Fixed-length inputs (32+48 bytes) — no ambiguous boundary |
| C3: Fork resolution skips attestation | CRITICAL | **False positive** | Attestation verified BEFORE fork resolution runs |
| C4: No sender authentication | CRITICAL | **False positive** | Schnorr sig on all Nostr events + BLS membership check |
| H2: No integrity on invite codes | HIGH | **False positive** | Poseidon commitment provides cryptographic integrity |
| M2: Clipboard not auto-cleared | MEDIUM | **Not applicable** | Inbox keys are X25519 public keys, not secrets |
| M5: Schnorr skipped in some paths | MEDIUM | **False positive** | All events signed and verified without exception |
| L2: No rate limiting on invitations | LOW | **Not applicable** | Relay-side concern, not client-side |

---

## Confirmed Findings

### 1. `ChatGroup.addMember()` uses random salt — MEDIUM

`addMember()` on both platforms calls `generateSalt()` (random), while `handleMemberJoined`
correctly uses `deriveSalt()`. If `addMember()` is called from any other code path, it will
produce a non-deterministic salt, causing an epoch fork.

The fork IS resolved deterministically (lexicographic salt comparison), so this doesn't
break the protocol — but it creates unnecessary forks and network chatter.

**Files:**
- `swift-mls/Sources/SwiftMLS/ChatGroup.swift:61`
- `kotlin-mls/src/main/java/com/stellarmls/mls/ChatGroup.kt:62`

**Fix:** Change `addMember()` to accept a salt parameter or call `deriveSalt` internally.

---

### 2. HTTP ATS exception for `onym.programyzer.com` — MEDIUM

iOS Info.plist allows cleartext HTTP to `onym.programyzer.com`. If this domain serves
any user data or auth endpoints in production, traffic is unencrypted.

**File:** `clients/ios/StellarChat/StellarChat/Info.plist:18-34`

Also allows `localhost` (standard for dev, acceptable).

**Fix:** Enable TLS on the server and remove the ATS exception. Or confirm this is
dev-only and strip it from release builds.

---

### 3. Ephemeral key signature is optional at send time — LOW

The invitation encryption API accepts an optional `senderSigningKey`. If nil, no ephemeral
key signature is included. On the receiving side, if a signature IS present, it's always
verified — so a MITM can't substitute an ephemeral key when the signature exists.

In practice, the app always passes the signing key, so this is a code hygiene issue, not
an active vulnerability.

**Files:**
- `clients/ios/.../Models/GroupCrypto.swift:104-111`
- `clients/android/.../crypto/GroupCrypto.kt:140-150`

**Fix:** Make `senderSigningKey` non-optional; require signature on all invitations.

---

### 4. Hardcoded TURN server credentials — LOW

TURN credentials (`stellarchat` / `stellarchat-turn-2026`) for `eu-turn.metered.ca` are
hardcoded. These appear to be shared Metered relay credentials, not private API keys.
User-configured TURN credentials take precedence when set.

**File:** `clients/ios/.../Call/CallManager.swift:86-94`

**Fix:** Fetch short-lived TURN credentials from a token endpoint.

---

### 5. Blossom allows HTTP if user-configured — LOW

The Blossom client accepts any URL scheme. Default server is HTTPS (`nostr.download`).
Content is E2E encrypted with per-file AES-256-GCM keys and integrity-verified via
SHA-256 hash, so HTTP transport doesn't expose plaintext media.

**Files:**
- `clients/ios/.../Models/BlossomClient.swift`
- `clients/android/.../blossom/BlossomClient.kt`

**Fix:** Validate URL scheme is `https://` before upload/download, or document the
risk in settings UI.

---

### 6. Salt history in plaintext UserDefaults (iOS only) — LOW

iOS stores up to 64 epoch→salt mappings per group in `UserDefaults` (unencrypted).
Android keeps this in-memory only.

The salt is NOT a secret — it's part of the on-chain Poseidon commitment and is
derivable by any group member. Plaintext storage is acceptable for non-secret data.

**File:** `clients/ios/.../StellarChatApp.swift:560-588`

**Fix:** No action needed. The salt is public by design.

---

### 7. Message dedup set is in-memory only — INFORMATIONAL

`seenMessageIDs` resets on app restart. After restart, the app may re-process relay
history. However:
- Database unique constraint prevents duplicate message insertion
- The set is repopulated from persisted messages on startup

No data corruption is possible. The in-memory set is a performance optimization.

---

## What the App Gets Right

These were investigated and found to be correctly implemented:

- **Nostr event signing:** All events are Schnorr-signed (secp256k1) at creation; all
  received events are verified via `verifyEventID()` before processing.
- **Sender authentication:** BLS pubkey in encrypted payload is verified against group
  membership. Combined with Nostr Schnorr signatures, this provides two-layer sender auth.
- **State update attestation:** Verified BEFORE fork resolution logic executes.
  Invalid attestations are rejected and logged.
- **Invite code integrity:** `InviteCode` contains full group state including Poseidon
  commitment, which is verified on-chain. Tampering is detectable.
- **HKDF key derivation:** Uses RFC 5869 with proper salt and info strings
  (`"sep-msg-key-v1"`, `"traffic"`).
- **AES-256-GCM encryption:** 12-byte random nonces, authenticated encryption.
- **Deterministic salt derivation:** `SHA256(previousSalt || memberKey)` with fixed-length
  inputs (32+48 bytes). No domain separation needed given fixed lengths — adding one would
  be defense-in-depth but is not a vulnerability.

---

## Remediation Priority

| Priority | Item | Effort |
|----------|------|--------|
| **Should fix** | #1 `addMember()` random salt | Small — add salt param |
| **Should fix** | #2 HTTP ATS exception | Trivial — remove or gate on build config |
| **Nice to have** | #3 Mandatory ephemeral sig | Small — make param non-optional |
| **Nice to have** | #4 TURN credential rotation | Medium — needs token endpoint |
| **No action** | #5 Blossom HTTP | Content is E2E encrypted |
| **No action** | #6 Salt in UserDefaults | Salt is non-secret |
| **No action** | #7 Dedup set | DB constraint is sufficient |
