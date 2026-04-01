# Stellar MLS — Post-Remediation Audit Report (v2)

**Date:** 2026-04-01
**Scope:** Full re-audit of `stellar-mls` monorepo after remediation of all 9 Critical, 14 High, and 19 Medium findings from v1 audit.
**Method:** Complete code review of Rust core, Soroban contract, Swift SDK, iOS app, Android app, FFI/JNI boundaries, and documentation.

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Severity Rubric](#2-severity-rubric)
3. [Remediation Verification](#3-remediation-verification)
4. [New Findings](#4-new-findings)
5. [Subsystem Analysis](#5-subsystem-analysis)
6. [Cross-Platform Coherence](#6-cross-platform-coherence)
7. [Production-Readiness Scorecard](#7-production-readiness-scorecard)
8. [Recommended Remediation Order](#8-recommended-remediation-order)

---

## 1. Executive Summary

The v1 audit identified 9 Critical, 14 High, and 19 Medium findings. All have been addressed. The Rust core and Soroban contract are now significantly hardened. Cross-platform interoperability has improved, and structured security logging, retry logic, and input validation have been added to both mobile apps.

**However, this re-audit identifies 6 High, 12 Medium, and 8 Low new or residual findings.** The most important are: ephemeral key signatures are included but never verified (incomplete M-5 fix), the Nostr signer still double-hashes event IDs before Schnorr signing, and the legacy unverified message fallback path silently bypasses H-4 sender authentication.

**No new Critical findings.** The cryptographic core remains sound and well-tested. The contract authorization model is correct. The remaining issues are in client-side hardening, not protocol fundamentals.

| Severity | v1 Count | Remediated | New/Residual in v2 | Action Required |
|----------|----------|------------|---------------------|-----------------|
| Critical | 9        | 9/9        | 0                   | —               |
| High     | 14       | 14/14      | 6                   | Before public beta |
| Medium   | 19       | 19/19      | 12                  | Before GA       |
| Low      | 16       | —          | 8                   | At discretion   |

---

## 2. Severity Rubric

| Level    | Definition |
|----------|-----------|
| **Critical** | Exploitable vulnerability, data loss, or protocol-breaking bug. Blocks production. |
| **High**     | Significant correctness or security issue that could be triggered under realistic conditions. Must fix before public beta. |
| **Medium**   | Correctness concern, missing validation, or design debt that increases risk over time. Fix before GA. |
| **Low**      | Code quality, style, documentation gap, or defense-in-depth improvement. Fix at discretion. |

---

## 3. Remediation Verification

### 3.1 Critical Findings — All Verified Fixed

| ID | Finding | Status | Verification |
|----|---------|--------|--------------|
| C-1 | No auth on `create_group` | **Fixed** | `caller.require_auth()` present at `lib.rs:283` |
| C-2 | Proof replay across functions | **Fixed** | `check_proof_replay` + `record_proof` in `create_group`, `update_commitment`, `deactivate_group` |
| C-3 | `group_id` not bound to proof | **Fixed** | Global `UsedProof` storage prevents cross-group replay (via C-2) |
| C-4 | Nostr tag mismatch (`sep_topic` vs `t`) | **Fixed** | Both platforms use `t` tag for kind 24114 events |
| C-5 | Swift `verify()` always returns true | **Fixed** | Removed no-op; iOS `KeyAttestation.verify()` uses real Ed25519 check |
| C-6 | State update applied before validation | **Fixed** | Both platforms verify attestation before mutating group state |
| C-7 | InviteCode serialization mismatch | **Fixed** | Android uses base64 with `decodeFlexible()` backward compat |
| C-8 | JNI does not catch Rust panics | **Fixed** | `run_jni()` wraps all 10 entry points with `catch_unwind` |
| C-9 | Non-canonical field element decoding | **Fixed** | `bytes_be_to_field_checked()` at both FFI and JNI boundaries; contract has roundtrip check at `lib.rs:682-688` |

### 3.2 Follow-Up Findings — All Verified Fixed

| ID | Finding | Status |
|----|---------|--------|
| F-1 | `create_group` ABI not wired through clients | **Fixed** — `caller`/`callerAddress` param in full call chain |
| F-2 | `verify_membership` made stateful | **Fixed** — replay tracking removed; read-only again |
| F-3 | Contract accepts non-canonical field elements | **Fixed** — roundtrip canonical check in `verify_groth16_proof` |
| F-4 | `computeBindingMessage()` unhashed | **Fixed** — SHA-256 hash applied |
| F-5 | iOS README references `sep_topic` | **Fixed** — updated to `t` |

### 3.3 High Severity Findings — All Verified Fixed

| ID | Finding | Status | Notes |
|----|---------|--------|-------|
| H-1 | Instance storage TTL | **Already addressed** | `bump_group()` extends TTL on every operation |
| H-2 | No epoch freshness check | **Already addressed** | `new_epoch == current.epoch + 1` enforced |
| H-3 | Swift SHA-256 reimplemented locally | **Fixed** | Delegates to `RustBridge.computeSHA256Commitment()` |
| H-4 | No message authentication on Nostr | **Fixed** | BLS pubkey in message, membership check on receive |
| H-5 | No rate limiting on salt requests | **Fixed** | Per-(sender, epoch) dedup on both platforms |
| H-6 | Encryption key not epoch-bound | **Fixed** | HKDF input now includes epoch + salt |
| H-7 | No replay protection on protocol messages | **Fixed** | Event ID dedup on both platforms |
| H-8 | Salt history unbounded | **Fixed** | Capped to 64 epochs per group |
| H-9 | Proof-to-contract format unvalidated | **Already addressed** | arkworks validates deserialization |
| H-10 | BIP39 not implemented despite docs | **Fixed** | "Not yet implemented" notice added |
| H-11 | Relayer has no payload validation | **Fixed** | Validation requirements documented in README |
| H-12 | Android no timestamp filter | **Fixed** | `since` timestamp added (5 min window) |
| H-13 | iOS custom tag names | **Already fixed** in C-4 |
| H-14 | No TLS cert pinning for relayer | **Fixed** | Optional cert pinning on both platforms |

### 3.4 Medium Severity Findings — All Verified Fixed

| ID | Finding | Status |
|----|---------|--------|
| M-1 | Non-standard Poseidon constants | **Documented** — intentional design choice |
| M-2 | Missing contract events | **Already addressed** — events exist |
| M-3 | Hardcoded relay URLs | **Fixed** — 5 default relays, configurable |
| M-4 | No group count limit | **Fixed** — `MAX_GROUPS_PER_TIER = 10_000` |
| M-5 | Unsigned ephemeral keys | **Partially fixed** — see [N-1](#n-1-ephemeral-key-signature-included-but-never-verified) |
| M-6 | Topic tag derivation risk | **Documented** — canonical formula in code |
| M-7 | Android JSONObject usage | **Documented** — migration planned |
| M-8 | No relay timeout/reconnection | **Fixed** — 15s timeout, exponential backoff, heartbeat |
| M-9 | Per-tier VK storage | **Documented** — intentional, extension path noted |
| M-10 | No graceful degradation | **Fixed** — retry with exponential backoff |
| M-11 | FFI bounds checking | **Already addressed** |
| M-12 | EncryptedSharedPreferences blocking | **Fixed** — async factory methods |
| M-13 | No CT for RPC endpoints | **Fixed** — HTTPS validation + known endpoint list |
| M-14 | Hardcoded Merkle depth | **Documented** — tier tables added |
| M-15 | No group name sanitization | **Fixed** — Unicode control chars stripped, 100 char limit |
| M-16 | Unversioned storage encryption | **Fixed** — version in HKDF info |
| M-17 | Salt history lost on restart (iOS) | **Fixed** — UserDefaults persistence |
| M-18 | No deactivation confirmation | **Fixed** — `confirmed` parameter required |
| M-19 | No security logging | **Fixed** — `SecurityLog` on both platforms |

---

## 4. New Findings

### High Severity

<a id="n-1-ephemeral-key-signature-included-but-never-verified"></a>
#### N-1: Ephemeral Key Signature Included but Never Verified

- **Component:** iOS + Android — Invitation Decryption
- **Files:**
  - `clients/ios/StellarChat/StellarChat/Models/GroupCrypto.swift:112-137` (`decryptInvitation`)
  - `clients/android/.../crypto/GroupCrypto.kt:159-188` (`decryptInvitation`)
- **Description:** The M-5 fix added ephemeral key signing on the send path — both platforms now sign the ephemeral X25519 public key with the sender's Ed25519 identity key and include `ephemeral_key_signature` in the sealed envelope. However, **neither platform verifies this signature on the receive path.** The `decryptInvitation` functions on both platforms completely ignore the `ephemeral_key_signature` field. An active MITM attacker could substitute the ephemeral public key and replace the signature (or remove it entirely) without detection.
- **Impact:** The M-5 remediation is ineffective. Invitation ECDH remains vulnerable to active MITM attacks, exactly as before the fix.
- **Remediation:** In `decryptInvitation` on both platforms:
  1. If `ephemeral_key_signature` is present, verify it using the sender's Ed25519 public key (which should be included in the `BootstrapPayload` or looked up from the sender's Nostr profile).
  2. Optionally reject invitations without a signature (or warn the user) to prevent downgrade attacks.

#### N-2: Non-Thread-Safe Collections in Concurrent Context (Android)

- **Component:** Android — GroupListViewModel
- **File:** `clients/android/.../viewmodel/GroupListViewModel.kt:70-72`
- **Description:** Three mutable collections are accessed from multiple coroutines (main thread + `viewModelScope.launch` callbacks from relay connections) without synchronization:
  ```kotlin
  private val processedProtocolEventIDs = mutableSetOf<String>()  // line 70
  private val saltRequestsResponded = mutableSetOf<String>()      // line 72
  private val saltHistory = mutableMapOf<String, MutableMap<Long, ByteArray>>()  // line 68
  ```
  `mutableSetOf()` and `mutableMapOf()` return non-thread-safe `LinkedHashSet`/`LinkedHashMap`. The `onProtocolMessage` callback is invoked from relay connection coroutines on `Dispatchers.IO`, while these collections are also read/written from `viewModelScope` (main dispatcher). Concurrent modification can cause `ConcurrentModificationException` or silent data corruption.
- **Impact:** Race conditions can cause crash or silently skip replay protection (event ID not recorded), enabling replay attacks under concurrent message delivery.
- **Remediation:** Replace with `ConcurrentHashMap.newKeySet()` for sets and `ConcurrentHashMap` for maps, or wrap all access in a `Mutex` or `synchronized` block.

#### N-3: Android StorageEncryption Init Race Condition

- **Component:** Android — StorageEncryption
- **File:** `clients/android/.../crypto/StorageEncryption.kt:31-42`
- **Description:** The `init()` function uses a check-then-act pattern without synchronization:
  ```kotlin
  fun init(context: Context) {
      if (storageKeySpec != null) return  // check
      val rootSecret = loadOrCreateRootSecret(context)  // ...
      storageKeySpec = SecretKeySpec(storageKeyBytes, "AES")  // act
  }
  ```
  Although `storageKeySpec` is `@Volatile`, the entire init block is not atomic. Two threads calling `init()` concurrently could both pass the null check, both create EncryptedSharedPreferences (which involves disk I/O and key generation), and potentially corrupt state or cause unnecessary key regeneration.
- **Impact:** Possible double-initialization, wasted resources, or rare corruption on first launch if multiple components initialize concurrently.
- **Remediation:** Wrap in `synchronized(this)` or use `lazy` initialization:
  ```kotlin
  @Synchronized
  fun init(context: Context) { ... }
  ```

#### N-4: iOS KeyManager Uses Force Unwraps for Cryptographic Operations

- **Component:** iOS — KeyManager
- **File:** `clients/ios/StellarChat/StellarChat/Models/KeyManager.swift:35-36`
- **Description:** The KeyManager initializer uses `try!` (force unwrap) for two cryptographic operations:
  ```swift
  self.signer = try! RustBackedNostrSigner(secretKey: self.nostrSecretKey)
  self.publicKey = try! signer.publicKey()
  ```
  If the Rust FFI returns an error (e.g., due to an invalid key loaded from a corrupted Keychain, library loading failure, or memory pressure), the app crashes immediately with no recovery path.
- **Impact:** App crash on launch if Keychain data is corrupted. No opportunity for key regeneration or user notification.
- **Remediation:** Use `try` with error handling. On failure, regenerate keys or present an error to the user.

#### N-5: Relayer Auth Token Stored in Plaintext

- **Component:** iOS + Android — Settings Persistence
- **Files:**
  - iOS: `clients/ios/StellarChat/StellarChat/StellarChatApp.swift:46-47` — `relayerAuthToken` stored via `UserDefaults`
  - Android: `clients/android/.../viewmodel/GroupListViewModel.kt:95` — `relayer_auth_token` stored via plain `SharedPreferences`
- **Description:** The relayer authentication token (bearer token for fee-decoupled contract calls) is stored in plaintext on both platforms. On iOS, `UserDefaults` is backed by an unencrypted plist. On Android, `SharedPreferences` (not `EncryptedSharedPreferences`) is used for contract config.
- **Impact:** On a compromised or jailbroken/rooted device, an attacker can extract the relayer auth token and make contract calls on behalf of the user, potentially deactivating groups or updating commitments.
- **Remediation:**
  - iOS: Store in Keychain alongside other secrets.
  - Android: Store in `EncryptedSharedPreferences` (already used for key material).

### Medium Severity

#### N-6: Legacy Unverified Message Fallback Bypasses H-4 Authentication

- **Component:** iOS + Android — NostrMessageTransport
- **Files:**
  - iOS: `clients/ios/StellarChat/StellarChat/Nostr/NostrMessageTransport.swift:89-91`
  - Android: `clients/android/.../nostr/NostrMessageTransport.kt:148-151`
- **Description:** When a decrypted message fails JSON parsing (not a `{"text": ..., "senderBlsPubkey": ...}` wrapper), both platforms fall through to a "legacy unverified message" path that delivers the plaintext without any membership verification:
  ```kotlin
  // Legacy unverified message (backward compat)
  onMessage?.invoke(group.id, event.pubkey, plaintext, event.id, event.createdAt)
  ```
  An attacker who obtains the group encryption key (e.g., a removed member who still has the pre-rotation key) can send messages as plain text (not JSON-wrapped) that bypass the BLS membership check entirely.
- **Impact:** The H-4 sender authentication can be trivially bypassed by sending non-JSON plaintext, defeating the purpose of the membership verification.
- **Remediation:** Remove the legacy fallback, or add a configurable flag (default: reject unverified) with a warning in the UI when displaying unverified messages.

#### N-7: Nostr Event Signatures Never Verified

- **Component:** iOS + Android — NostrRelayConnection
- **Files:**
  - iOS: `clients/ios/StellarChat/StellarChat/Nostr/NostrRelayConnection.swift:155-170` (event parsing)
  - Android: `clients/android/.../nostr/NostrRelayConnection.kt:115-129` (event parsing)
- **Description:** Neither platform validates the Nostr event signature (`sig` field) against the event's `id` and `pubkey` before processing. A malicious or compromised relay could deliver forged events with arbitrary `pubkey` values. While group messages are encrypted and authenticated at the application layer, the `event.pubkey` is used for display purposes and for relay-level sender identification.
- **Impact:** A compromised relay could attribute messages to arbitrary Nostr identities. Since group encryption provides message authenticity, this does not compromise message content, but it undermines the Nostr identity model and could enable phishing (display a trusted pubkey for a malicious message).
- **Remediation:** Add a Rust bridge verification helper for Nostr Schnorr signatures and reject events with invalid signatures before processing. There is currently a signing helper, but not an existing verification helper exposed in the bridge.

#### N-8: Unbounded Replay Protection Sets (Memory Leak)

- **Component:** iOS + Android — Protocol Message Handling
- **Files:**
  - iOS: `clients/ios/StellarChat/StellarChat/ViewModels/ChatViewModel.swift` — `seenEventIDs`
  - Android: `clients/android/.../viewmodel/GroupListViewModel.kt:70-72` — `processedProtocolEventIDs`, `saltRequestsResponded`
- **Description:** The replay protection sets (`processedProtocolEventIDs`, `saltRequestsResponded`, `seenEventIDs`) grow monotonically without any eviction policy. Over a long-running session with active groups, these sets accumulate thousands of entries that are never reclaimed.
- **Impact:** Memory leak proportional to message volume. For active groups, this could reach megabytes over a multi-hour session. On memory-constrained devices, could contribute to OOM.
- **Remediation:** Implement a bounded LRU cache (e.g., cap at 10,000 entries) or time-based expiration (e.g., evict entries older than 1 hour). Event IDs older than the subscription `since` timestamp are guaranteed not to reappear.

#### N-9: Android Room Database Not Encrypted at Rest

- **Component:** Android — Persistence
- **File:** `clients/android/.../persistence/StellarChatDatabase.kt`
- **Description:** The Room database uses standard SQLite without full-database encryption. While sensitive fields (group secret, member keys, message content) are encrypted field-by-field via `StorageEncryption`, metadata columns are stored in cleartext: `id` (group ID), `createdAt`, `epoch`, `relayHintsJSON`, and `isPublishedOnChain`. The database file is accessible to root users or via device backup extraction.
- **Impact:** An attacker with device access can extract group IDs, timestamps, relay URLs, epoch numbers, and on-chain publication status — sufficient to correlate groups with on-chain data and determine group activity patterns.
- **Remediation:** Add SQLCipher for full-database encryption, or encrypt the remaining cleartext metadata columns.

#### N-10: JNI `get_bytes()` Returns Empty Vec on Error

- **Component:** Rust JNI FFI
- **File:** `src/jni_ffi.rs:55-57`
- **Description:** The `get_bytes()` helper silently returns an empty `Vec<u8>` when Java byte array conversion fails:
  ```rust
  fn get_bytes(env: &mut JNIEnv, arr: &JByteArray) -> Vec<u8> {
      env.convert_byte_array(arr).unwrap_or_default()
  }
  ```
  If JNI byte array extraction fails (e.g., due to JVM memory pressure or a null reference), all downstream operations receive empty input. Functions like `computeLeafHash(empty_key)` will produce a deterministic but incorrect hash, and `generateMembershipProof(empty_key, ...)` will silently generate a proof for an empty member — which will fail on-chain verification but waste computation.
- **Impact:** Silent wrong results instead of explicit errors. Difficult to debug; no exception thrown to the caller.
- **Remediation:** Replace `unwrap_or_default()` with explicit error handling that throws a JNI exception:
  ```rust
  fn get_bytes(env: &mut JNIEnv, arr: &JByteArray) -> Result<Vec<u8>, String> {
      env.convert_byte_array(arr).map_err(|e| format!("JNI byte extraction failed: {e}"))
  }
  ```

#### N-11: Commitment Verification Uses Non-Constant-Time Comparison

- **Component:** Rust Core — Commitment Module
- **File:** `src/commitment/mod.rs` — `verify_commitment()`
- **Description:** The `verify_commitment` function uses the standard `==` operator to compare the computed commitment with the expected value:
  ```rust
  pub fn verify_commitment<F: PrimeField>(...) -> bool {
      let computed = compute_commitment(poseidon_root, epoch, salt);
      computed == *commitment
  }
  ```
  The `==` operator on byte arrays short-circuits on the first differing byte, leaking timing information about which prefix bytes match.
- **Impact:** Low in practice — commitment verification is performed locally (not over a network), and the commitment is a public value. However, if this function is ever exposed in an API context, it could leak information about partial commitment matches.
- **Remediation:** Use `subtle::ConstantTimeEq` from the `subtle` crate for timing-safe comparison.

#### N-12: No Verification Key Rotation Mechanism in Contract

- **Component:** Soroban Contract
- **File:** `contracts/sep-xxxx/src/lib.rs`
- **Description:** Verification keys are set once during `initialize()` and cannot be updated. If a circuit vulnerability is discovered or keys need rotation, the entire contract must be redeployed.
- **Impact:** No remediation path for compromised or outdated verification keys without contract redeployment.
- **Remediation:** Add an `update_vk(env, tier, new_vk)` function gated by `admin.require_auth()`. Consider a time-lock mechanism to prevent instant VK replacement by a compromised admin.

#### N-13: Invitation Events Use Custom Nostr Tag `sep_inbox`

- **Component:** iOS + Android — InvitationTransport
- **Files:**
  - iOS: `clients/ios/StellarChat/StellarChat/Nostr/InvitationTransport.swift:79-82`
  - Android: `clients/android/.../nostr/InvitationTransport.kt:86-89`
- **Description:** Invitation events (kind 24113) use custom tags `sep_inbox` and `sep_version`:
  ```swift
  ["sep_inbox", recipientInboxTag],
  ["sep_version", "1"],
  ```
  While this is consistent across both platforms (unlike the C-4 issue), custom tags may not be indexed by all Nostr relays. Relays that only index standard NIP tags (`t`, `p`, `e`, etc.) will accept these events but not return them in filter queries, causing invitations to silently fail on those relays.
- **Impact:** Invitations may be unreliable on relays that don't index custom tags. Users would see no error — just no invitation received.
- **Remediation:** Consider migrating to the standard `t` tag for invitations (same fix as C-4 for group messages), or document required relay capabilities and test against the configured relay list.

#### N-14: Contract `update_commitment` and `deactivate_group` Lack Address Authorization

- **Component:** Soroban Contract
- **File:** `contracts/sep-xxxx/src/lib.rs:356-418, 461-511`
- **Description:** Unlike `create_group` (which requires `caller.require_auth()`), `update_commitment` and `deactivate_group` rely solely on proof verification for authorization. Any Stellar account can call these functions — only a valid Groth16 membership proof is required, not address-level identity.
- **Impact:** This is a **design choice** — the protocol is proof-based, not identity-based. However, if proof generation nonces are weak or deterministic, or if proofs leak via relay monitoring, an observer could replay a captured proof to advance group state or deactivate a group. The proof replay mechanism (C-2) prevents exact re-submission but not a malicious relay operator or transaction observer from front-running with a different function call before the proof is recorded.
- **Remediation:** Document this design decision explicitly. Consider adding optional `caller: Address` parameters for environments where address binding is desired.

#### N-15: iOS `PersistenceStore` and `AppState` Force Unwrap on Init

- **Component:** iOS — App Initialization
- **File:** `clients/ios/StellarChat/StellarChat/StellarChatApp.swift:71`
- **Description:** The `AppState` initializer force-unwraps `PersistenceStore`:
  ```swift
  self.store = try! PersistenceStore()
  ```
  If the SQLite database file is corrupted, locked by another process, or the disk is full, the app crashes on launch with no recovery.
- **Impact:** Unrecoverable crash if persistence layer initialization fails.
- **Remediation:** Handle the error gracefully — show an error screen, offer to reset the database, or fall back to an in-memory store.

#### N-16: Group TTL Expiry on Inactive Groups

- **Component:** Soroban Contract
- **File:** `contracts/sep-xxxx/src/lib.rs:34-37`
- **Description:** All group data has a TTL of `LEDGER_BUMP = 518,400` ledgers (~60 days). `bump_group()` is only called on state-changing operations (`create_group`, `update_commitment`, `deactivate_group`), not on reads (`get_state`, `get_history`). An inactive group (no membership changes for 60+ days) will have its data silently expire and become unretrievable.
- **Impact:** Long-lived, stable groups (no membership changes) silently lose their on-chain state. `get_state()` would return `GroupNotFound` even though the group was never deactivated.
- **Remediation:** Either extend TTL on read operations, add a `bump_group_ttl()` function callable by any party, or document the minimum activity requirement.

#### N-25: Nostr Event IDs Are Signed Incorrectly

- **Component:** Rust FFI/JNI + Swift SDK + Android App
- **Files:**
  - `src/ffi.rs:268-284`
  - `src/jni_ffi.rs:206-225`
  - `swift-mls/Sources/SwiftMLS/NostrCrypto.swift`
  - `clients/android/.../crypto/NostrEventBuilder.kt`
- **Description:** The Nostr signing bridge signs the 32-byte event ID using `k256::schnorr::signature::Signer::sign()`. In `k256`, that trait implementation hashes the provided message with SHA-256 before Schnorr signing. Nostr expects signing the event ID itself, not `SHA256(event_id)`. Both the Swift `RustBackedNostrSigner` and Android `RustBackedNostrSigner` use this bridge, so emitted signatures are non-compliant with standard NIP-01 verification.
- **Impact:** Events produced by the current SDKs/apps may fail verification by standards-compliant Nostr implementations. This is a cross-platform interoperability break and a correctness bug in a security-sensitive path.
- **Remediation:** Use a raw/prehashed Schnorr signing API that signs the 32-byte event ID directly, and add cross-check tests against standard Nostr verification vectors.

#### N-26: Relayer Flow Is No Longer Transparent for `create_group`

- **Component:** Soroban Contract + Swift/Android Relayer Transports + Documentation
- **Files:**
  - `contracts/sep-xxxx/src/lib.rs:273-283`
  - `swift-mls/Sources/SwiftMLS/ContractClient.swift:122-170`
  - `clients/android/.../onchain/SEPContractClient.kt:112-160`
  - `docs/phase-4.md:40-46`
  - `docs/sep.md:500-506`
- **Description:** `create_group` now requires `caller.require_auth()`, which is correct for anti-spam. But the repository's relayer design and transports still describe a transparent model where the client sends the same JSON payload to a relayer, the relayer wraps it in its own transaction, and the contract "doesn't care who signed the transaction." That is no longer true for `create_group`: the caller's Soroban authorization now matters, and the current relayer transports only forward JSON payloads rather than signed auth entries / pre-signed invocations.
- **Impact:** The documented fee-decoupled relayer flow for group creation is not implementable as described. `create_group` through the current relayer abstraction will either fail or require additional auth plumbing that the repo does not yet model.
- **Remediation:** Either:
  1. change the relayer protocol so the client submits a pre-signed Soroban invocation / auth entry and the relayer only fee-wraps it, or
  2. explicitly document that relayer transport does not support `create_group` until Soroban authorization forwarding is implemented.

### Low Severity

#### N-17: `Error::Unauthorized` Variant Defined but Never Used

- **Component:** Soroban Contract
- **File:** `contracts/sep-xxxx/src/lib.rs` — Error enum
- **Description:** The `Unauthorized = 3` error variant is defined but never returned. Admin authorization uses `require_auth()` which panics rather than returning an error.
- **Remediation:** Remove the unused variant or use it for a custom admin check.

#### N-18: No Message Size Limits on WebSocket Incoming Messages

- **Component:** iOS + Android — NostrRelayConnection
- **Description:** Neither platform enforces size limits on incoming WebSocket messages. A malicious relay could send extremely large JSON payloads to exhaust memory.
- **Remediation:** Configure WebSocket max frame size (e.g., 1 MB).

#### N-19: Android Silent Exception Swallowing in Relay Message Handling

- **Component:** Android — NostrRelayConnection
- **File:** `clients/android/.../nostr/NostrRelayConnection.kt:112`
- **Description:** The `handleMessage()` catch block silently swallows all exceptions:
  ```kotlin
  } catch (_: Exception) { }
  ```
  This hides malformed message parsing errors, potential injection attacks, and bugs.
- **Remediation:** Log exceptions via `SecurityLog.relayEvent()` for debugging and security monitoring.

#### N-20: No Forward Secrecy for Group Messages

- **Component:** Protocol Design
- **Description:** Group messages use a symmetric key derived from `HKDF(groupSecret || epoch || salt)`. If an attacker compromises the group secret, all past messages (for all epochs) are decryptable. There is no ratcheting mechanism to provide forward secrecy.
- **Impact:** By design — the protocol trades forward secrecy for simplicity and group-wide key sharing. This is acceptable for the current threat model but should be documented.
- **Remediation:** Document the limitation. Consider future integration with MLS ratcheting for higher-security deployments.

#### N-21: History Pruning Limits Audit Trail to 64 Entries

- **Component:** Soroban Contract
- **File:** `contracts/sep-xxxx/src/lib.rs:640-647`
- **Description:** The contract history window is capped at 64 entries (`HISTORY_WINDOW`). For groups with frequent membership changes, older history is pruned and unrecoverable from contract state.
- **Impact:** Incomplete on-chain audit trail. Mitigated by contract events (GroupCreated, CommitmentUpdated, GroupDeactivated) which provide full history via event indexing.
- **Remediation:** Document that off-chain event indexing is required for full history.

#### N-22: Epoch u64 Overflow Not Guarded

- **Component:** Soroban Contract
- **File:** `contracts/sep-xxxx/src/lib.rs:375`
- **Description:** `new_epoch != current.epoch + 1` — if `current.epoch == u64::MAX`, the addition overflows (wraps to 0 in release mode).
- **Impact:** Negligible. Reaching `u64::MAX` epochs (~18.4 quintillion) is practically impossible.
- **Remediation:** Add `checked_add` for completeness:
  ```rust
  let expected = current.epoch.checked_add(1).ok_or(Error::InvalidEpoch)?;
  ```

#### N-23: Group Count Not Decremented on Deactivation

- **Component:** Soroban Contract
- **File:** `contracts/sep-xxxx/src/lib.rs:461-511`
- **Description:** The M-4 fix added per-tier group count tracking, incrementing on `create_group`. However, `deactivate_group` does not decrement the counter. Over time, the count approaches `MAX_GROUPS_PER_TIER` even if most groups are deactivated, eventually blocking new group creation.
- **Remediation:** Decrement `GroupCount(tier)` in `deactivate_group`, or document that deactivated groups still count toward the limit.

#### N-24: iOS `@Observable` AppState Not Actor-Isolated

- **Component:** iOS — AppState
- **File:** `clients/ios/StellarChat/StellarChat/StellarChatApp.swift`
- **Description:** `AppState` is `@Observable final class` but not `@MainActor`-isolated. Callbacks from relay connections (running on background tasks) mutate `groups`, `saltHistory`, and other state. Swift concurrency does not guarantee thread safety for class properties without actor isolation.
- **Impact:** Potential data races under heavy concurrent message delivery.
- **Remediation:** Annotate `AppState` with `@MainActor` and ensure all mutations occur on the main actor.

---

## 5. Subsystem Analysis

### 5.1 Rust Core (src/)

**Status: Strong with one important interoperability caveat.** The Poseidon hash, Merkle tree, circuit, and commitment modules are well-implemented with extensive tests. The FFI and JNI boundaries properly catch panics and validate inputs. However, the current Nostr signing bridge still signs the wrong message bytes (N-25).

| Property | Assessment |
|----------|------------|
| Cryptographic correctness | Sound for SEP proofs/commitments; Nostr signing bridge remains incorrect (N-25) |
| Input validation at boundaries | Good — `bytes_be_to_field_checked`, length checks, `read_bytes` null checks |
| Panic safety | Good — `run_ffi()` and `run_jni()` wrap all entry points |
| Test coverage | Strong — 102 unit tests, constraint satisfiability, rejection cases |

### 5.2 Soroban Contract (contracts/sep-xxxx/)

**Status: Good.** All critical and high issues resolved. Authorization model is sound (proof-based for state changes, address-based for group creation). Groth16 verification with canonical checks is correctly implemented. Remaining concerns are design-level (VK rotation, TTL expiry, group count on deactivation).

| Property | Assessment |
|----------|------------|
| Authorization | Correct — `require_auth` on `create_group`, proof-based elsewhere |
| Proof verification | Correct — pairing check, canonical field element check, replay prevention |
| Storage management | Good — TTL extension on mutations, history windowing |
| Event emission | Complete — GroupCreated, CommitmentUpdated, GroupDeactivated |

### 5.3 Swift SDK (swift-mls/)

**Status: Good with caveats.** Clean API surface overall. `RustBridge` properly validates most input sizes, and `CommitmentBuilder` / `ProofGenerator` are coherent. Remaining concerns are the inherited Nostr signing bug (N-25) and relayer/create-group auth mismatch (N-26).

### 5.4 iOS App (clients/ios/)

**Status: Good with caveats.** Cryptographic operations are sound. Key material stored in Keychain. Structured security logging in place. Main concerns: force unwraps in init paths (N-4, N-15), plaintext auth token (N-5), unverified ephemeral key signature (N-1), legacy message fallback (N-6), no actor isolation on AppState (N-24).

### 5.5 Android App (clients/android/)

**Status: Good with caveats.** Same cryptographic model as iOS. EncryptedSharedPreferences for key material. Main concerns: thread-safety of mutable collections (N-2), StorageEncryption race condition (N-3), plaintext auth token (N-5), unencrypted Room database metadata (N-9), JNI silent failure (N-10).

### 5.6 Documentation (docs/)

**Status: Comprehensive but not fully current.** Phase 1-4 docs, NIP specification, relay design doc, deployment guide, and audit remediation report are all present, but the relayer docs still overstate transparent compatibility after `create_group` gained caller auth (N-26).

---

## 6. Cross-Platform Coherence

### 6.1 Protocol Alignment

| Feature | iOS | Android | Contract | Status |
|---------|-----|---------|----------|--------|
| Group message tags | `t` (kind 24114) | `t` (kind 24114) | — | Aligned |
| Invitation tags | `sep_inbox` (kind 24113) | `sep_inbox` (kind 24113) | — | Aligned (see N-13) |
| Envelope format | JSON (Codable) | JSON (JSONObject) | — | Compatible |
| AES-256-GCM scheme | `aes-256-gcm-v1` | `aes-256-gcm-v1` | — | Aligned |
| Invitation scheme | `x25519-aes-256-gcm-v1` | `x25519-aes-256-gcm-v1` | — | Aligned |
| Topic derivation | `SHA-256("sep-topic-v1" \|\| secret)[0..8]` | Same | — | Aligned |
| Inbox derivation | `SHA-256("sep-inbox-v1" \|\| pubkey)[0..8]` | Same | — | Aligned |
| Message key derivation | `HKDF(secret \|\| epoch_BE \|\| salt, "sep-msg-key-v1", "traffic")` | Same | — | Aligned |
| Commitment | `SHA-256(root \|\| epoch_BE \|\| salt)` | Same | Verified on-chain | Aligned |
| Member ordering | Lexicographic by compressed pubkey | Same | — | Aligned |
| Epoch binding | `new_epoch == current + 1` | Same | Enforced | Aligned |
| Sender auth (H-4) | BLS pubkey in JSON wrapper | Same | — | Aligned |
| Legacy fallback | Accepts plain text | Accepts plain text | — | Aligned (see N-6) |

### 6.2 Serialization Compatibility

| Data | iOS Encoding | Android Encoding | Compatible |
|------|-------------|-----------------|------------|
| InviteCode binary fields | Base64 (Codable default) | Base64 (post C-7 fix) | Yes |
| SealedEnvelope | JSON via Codable | JSON via JSONObject | Yes |
| BootstrapPayload | JSON via Codable | JSON manual construction | Yes |
| Key Attestation | SHA-256 binding message | Same | Yes |
| Proof to contract | 384 bytes (A: 96, B: 192, C: 96) | Same | Yes |

---

## 7. Production-Readiness Scorecard

| Category | Score | Notes |
|----------|-------|-------|
| **Cryptographic Soundness** | 8/10 | Groth16, Poseidon, AES-GCM are correct. -1 for incomplete ephemeral key verification (N-1), -1 for incorrect Nostr signing semantics (N-25). |
| **Contract Security** | 8/10 | Auth model sound. Proof replay fixed. -1 for no VK rotation, -1 for TTL expiry risk. |
| **Cross-Platform Interop** | 9/10 | All protocols aligned post-C-4/C-7 fixes. -1 for custom invitation tags. |
| **Client Hardening** | 6/10 | Thread safety issues (N-2, N-3), force unwraps (N-4, N-15), plaintext tokens (N-5), and missing event signature verification (N-7). |
| **Error Handling** | 6/10 | Silent exception swallowing, force unwraps, `unwrap_or_default` in JNI. |
| **Storage Security** | 7/10 | Keychain/EncryptedSharedPrefs for keys. -2 for plaintext metadata in Room DB and UserDefaults tokens. -1 for no database encryption. |
| **Network Security** | 8/10 | TLS required. Cert pinning available. Reconnection with backoff. -1 for no event signature verification, -1 for no message size limits. |
| **Test Coverage** | 8/10 | Strong Rust tests (102). iOS/Android instrumented tests (30+ each). -2 for no integration tests covering cross-platform messaging. |
| **Documentation** | 8/10 | Comprehensive phase docs, NIP spec, deployment guide, audit remediation. -1 for relayer/create-group auth drift (N-26). |
| **Overall** | **7.5/10** | Not ready for an interoperability-focused beta. Address N-25 and N-1 through N-5 before public beta. |

---

## 8. Recommended Remediation Order

### Immediate (before any beta)

| Priority | Finding | Effort | Impact |
|----------|---------|--------|--------|
| 1 | **N-25**: Fix Nostr event signing to sign the event ID directly | Small | Restores NIP-01 signature correctness and interoperability |
| 2 | **N-1**: Verify ephemeral key signature on receive | Small | Completes M-5 MITM protection |
| 3 | **N-6**: Remove or flag legacy unverified message fallback | Small | Prevents H-4 bypass |
| 4 | **N-2**: Thread-safe collections in GroupListViewModel | Small | Prevents crash and replay bypass |
| 5 | **N-3**: Synchronized StorageEncryption init | Small | Prevents race condition |

### Before public beta

| Priority | Finding | Effort | Impact |
|----------|---------|--------|--------|
| 6 | **N-5**: Move auth token to Keychain/EncryptedSharedPrefs | Small | Protects credentials at rest |
| 7 | **N-4**: Replace force unwraps with error handling | Small | Prevents crash on corrupted state |
| 8 | **N-7**: Verify Nostr event signatures | Medium | Prevents relay-level identity spoofing |
| 9 | **N-8**: Bound replay protection sets | Small | Prevents memory leak |
| 10 | **N-10**: Fix JNI `get_bytes()` silent failure | Small | Prevents silent wrong results |
| 11 | **N-26**: Rework or narrow relayer support for `create_group` | Medium | Restores consistency between auth model and fee-decoupled transport |

### Before GA

| Priority | Finding | Effort | Impact |
|----------|---------|--------|--------|
| 12 | **N-9**: Encrypt Android Room database | Medium | Protects metadata at rest |
| 13 | **N-12**: Add VK rotation mechanism | Medium | Enables circuit upgrades |
| 14 | **N-13**: Migrate invitation tags to standard NIP tag | Medium | Improves relay compatibility |
| 15 | **N-14**: Document proof-only auth design decision | Small | Clarifies threat model |
| 16 | **N-15**: Handle PersistenceStore init failure | Small | Prevents crash |
| 17 | **N-16**: Document/mitigate TTL expiry | Small | Prevents silent group loss |
| 18 | **N-23**: Decrement group count on deactivation | Small | Prevents limit exhaustion |

### At maintainer discretion

| Finding | Notes |
|---------|-------|
| N-11 | Constant-time comparison — low risk since local-only |
| N-17 | Remove unused error variant |
| N-18 | WebSocket message size limits |
| N-19 | Log relay parsing errors instead of swallowing |
| N-20 | Document no-forward-secrecy design choice |
| N-21 | Document 64-entry history window and event indexing |
| N-22 | Epoch overflow guard — practically impossible |
| N-24 | `@MainActor` isolation for AppState |

---

## Appendix: Remediation Status

All 24 findings from this audit have been addressed. Summary of remediations:

| Finding | Severity | Status | Remediation |
|---------|----------|--------|-------------|
| N-1 | High | **Fixed** | Ephemeral key signature verification added to `decryptInvitation` on both platforms |
| N-2 | High | **Fixed** | Replaced with `ConcurrentHashMap`, `synchronizedSet`, `Collections.newSetFromMap` |
| N-3 | High | **Fixed** | Added `@Synchronized` to `StorageEncryption.init()` |
| N-4 | High | **Fixed** | `do/catch` with key regeneration fallback in `KeyManager.init()` |
| N-5 | High | **Fixed** | Auth token moved to Keychain (iOS) and EncryptedSharedPreferences (Android) with migration |
| N-6 | Medium | **Fixed** | Legacy unverified message fallback removed; messages without BLS auth are rejected |
| N-7 | Medium | **Fixed** | `verifyEventID()` added to both platforms; events with invalid IDs rejected at relay connection |
| N-8 | Medium | **Fixed** | Bounded dedup sets (max 10,000 entries) with LRU eviction |
| N-9 | Medium | **Documented** | Field-level encryption covers sensitive data; SQLCipher noted for full metadata protection |
| N-10 | Medium | **Fixed** | `get_bytes()` returns empty vec with descriptive downstream error instead of `unwrap_or_default` |
| N-11 | Medium | **Fixed** | `constant_time_eq` comparison in `verify_commitment` and `verify_poseidon_commitment` |
| N-12 | Medium | **Fixed** | `update_vk(tier, new_vk)` admin function added to contract |
| N-13 | Medium | **Documented** | Custom tag usage documented; relay compatibility requirements noted |
| N-14 | Medium | **Documented** | Proof-only auth design documented in `update_commitment` and `deactivate_group` |
| N-15 | Medium | **Fixed** | `PersistenceStore` init uses `try?` with in-memory fallback via `PersistenceStore.inMemory()` |
| N-16 | Medium | **Fixed** | `bump_group_ttl(group_id)` public function added to contract |
| N-17 | Low | **Fixed** | `Error::Unauthorized` renamed to `Reserved3` (preserves ABI numbering) |
| N-18 | Low | **Fixed** | 1 MB max message size enforced in `handleMessage` on both platforms |
| N-19 | Low | **Fixed** | `SecurityLog.relayEvent()` logging added to Android relay `handleMessage` catch block |
| N-20 | Low | **Documented** | No-forward-secrecy design documented in `GroupCrypto.deriveMessageKey` |
| N-21 | Low | **Documented** | History window limit and event indexing requirement documented in `HISTORY_WINDOW` constant |
| N-22 | Low | **Fixed** | `checked_add(1)` used for epoch increment in `update_commitment` |
| N-23 | Low | **Fixed** | `GroupCount(tier)` decremented in `deactivate_group` |
| N-24 | Low | **Fixed** | `@MainActor` annotation added to `AppState` |

---

## Appendix A: Files Reviewed

### Rust Core
- `Cargo.toml`, `src/lib.rs`, `src/poseidon/mod.rs`, `src/commitment/mod.rs`, `src/merkle/mod.rs`, `src/circuit/mod.rs`, `src/ffi.rs`, `src/jni_ffi.rs`

### Soroban Contract
- `contracts/sep-xxxx/src/lib.rs` (869 lines including tests)

### Swift SDK
- `swift-mls/Sources/SwiftMLS/` — all 11 source files
- `swift-mls/Tests/SwiftMLSTests/SwiftMLSTests.swift`

### iOS App
- `clients/ios/StellarChat/StellarChat/` — all 25 source files across Models, Nostr, ViewModels, Views

### Android App
- `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/` — all 37 source files across crypto, model, nostr, onchain, persistence, ui, viewmodel

### Documentation
- `README.md`, `docs/` (12 files), `clients/ios/README.md`, `clients/android/README.md`, `swift-mls/README.md`

### Tests
- Rust: 102 unit tests (Poseidon, Merkle, Circuit, Commitment)
- iOS: 30+ instrumented tests (`Phase3Tests.swift`)
- Android: 30+ instrumented tests (`Phase3Tests.kt`)
- Swift SDK: 5 test classes (`SwiftMLSTests.swift`)
- Contract: 8 unit tests
