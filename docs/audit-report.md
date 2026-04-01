# Stellar MLS — Full Repository Audit Report

**Date:** 2026-04-01
**Scope:** All code in `stellar-mls` monorepo — Rust core, Soroban contract, Swift SDK, Kotlin SDK, iOS app, Android app, documentation, and tests.
**Method:** Six-workstream review covering baseline coherence, cryptography/protocol soundness, Soroban contract security, SDK/FFI boundaries, mobile apps/transport, and test coverage.

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Severity Rubric](#2-severity-rubric)
3. [Findings by Severity](#3-findings-by-severity)
4. [Subsystem-by-Subsystem Status](#4-subsystem-by-subsystem-status)
5. [Coherence Matrix](#5-coherence-matrix)
6. [Production-Readiness Scorecard](#6-production-readiness-scorecard)
7. [Open Questions and Assumptions](#7-open-questions-and-assumptions)
8. [Recommended Remediation Order](#8-recommended-remediation-order)

---

## 1. Executive Summary

The Stellar MLS repository implements a privacy-preserving group messaging protocol using BLS12-381 Poseidon Merkle commitments, Groth16 zero-knowledge proofs, Nostr for message transport, and a Soroban smart contract for on-chain state anchoring. The codebase spans Rust (core cryptography + FFI), Swift (SDK + iOS app), Kotlin (SDK + Android app), and Soroban (on-chain contract).

**Overall assessment: The cryptographic foundations are sound and well-tested, but the system has several critical gaps that must be addressed before production deployment.** The most severe issues are in the contract authorization model, cross-platform interoperability, and attestation verification. The Rust core is the strongest component; the mobile apps are the weakest.

**Totals:** 9 Critical, 14 High, 19 Medium, 16 Low findings across all workstreams.

| Severity | Count | Immediate Action Required |
|----------|-------|--------------------------|
| Critical | 9     | Yes — blocks production  |
| High     | 14    | Yes — before public beta |
| Medium   | 19    | Recommended before GA    |
| Low      | 16    | At maintainer discretion |

---

## 2. Severity Rubric

| Level    | Definition |
|----------|-----------|
| **Critical** | Exploitable vulnerability, data loss, or protocol-breaking bug. Blocks production. |
| **High**     | Significant correctness or security issue that could be triggered under realistic conditions. Must fix before public beta. |
| **Medium**   | Correctness concern, missing validation, or design debt that increases risk over time. Fix before GA. |
| **Low**      | Code quality, style, documentation gap, or defense-in-depth improvement. Fix at discretion. |

---

## 3. Findings by Severity

### 3.1 Critical Findings

#### C-1: No Authorization on `create_group`
- **Component:** Soroban Contract
- **File:** `contracts/sep-xxxx/src/lib.rs:252-309`
- **Description:** The `create_group()` function does not call `require_auth()`. Any Stellar account can create groups at will, potentially exhausting contract storage or squatting on group IDs.
- **Impact:** Denial-of-service via storage exhaustion. Group ID squatting. No cost barrier to spam.
- **Remediation:** Add `require_auth()` on a caller address, or require a valid ZK proof that the caller is a member of the group being created (already passed as a parameter but not verified for authorization purposes). Consider rate limiting or deposit requirements.

#### C-2: Proof Replay Across Contract Functions
- **Component:** Soroban Contract
- **File:** `contracts/sep-xxxx/src/lib.rs:252-500`
- **Description:** The same Groth16 proof can be replayed across `create_group`, `update_commitment`, and `deactivate_group` because the proof's public inputs do not include a function selector or nonce. A proof generated for `create_group` can be submitted to `deactivate_group`.
- **Impact:** An attacker who observes a valid proof on-chain can replay it to deactivate the group or overwrite its commitment.
- **Remediation:** Include a function-specific domain separator in the public inputs (e.g., hash of the function name), or add a nonce/epoch that is checked and incremented on each call.

#### C-3: `group_id` Not Bound to Proof
- **Component:** Soroban Contract
- **File:** `contracts/sep-xxxx/src/lib.rs:252-309`
- **Description:** The `group_id` is passed as a separate parameter and stored alongside the commitment, but the ZK proof does not commit to which group it pertains to. A valid membership proof for group A can be used to create or modify group B.
- **Impact:** Proof portability across groups. Breaks the authorization model entirely.
- **Remediation:** Include `group_id` (or its hash) as a public input to the circuit, and verify it matches the parameter.

#### C-4: Nostr Tag Mismatch Breaks Cross-Platform Messaging
- **Component:** Mobile Apps / Transport
- **File:** iOS: `clients/ios/StellarChat/StellarChat/Nostr/NostrMessageTransport.swift:51,121` — Android: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/nostr/NostrMessageTransport.kt:69,90`
- **Description:** iOS publishes and subscribes to group messages using the `sep_topic` tag. Android uses the `t` tag. Messages sent from one platform are invisible to the other because Nostr relay filter subscriptions use the tag name.
- **Impact:** Complete cross-platform messaging failure. iOS users cannot communicate with Android users in the same group.
- **Remediation:** Standardize on a single tag name across both platforms. The `t` tag is a standard NIP tag; `sep_topic` is custom. Choose one and update both clients. If backward compatibility is needed, subscribe to both during a transition period.

#### C-5: Swift Attestation `verify()` Always Returns True
- **Component:** Swift SDK
- **File:** `swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift:79-93`
- **Description:** `SEPKeyAttestationPayload.verify()` validates byte lengths but always returns `true` without performing Ed25519 signature verification. The comment says "verification is performed by the app layer," but the iOS app calls `verify()` and trusts the result.
- **Impact:** Any forged attestation with correct byte lengths is accepted. An attacker can bind any BLS key to any Ed25519 identity.
- **Remediation:** Either implement actual Ed25519 verification in the SDK (using CryptoKit or a bundled library), or change the API to return a `BindingMessage` that the caller must verify, and remove the misleading `verify() -> Bool` signature.

#### C-6: Android Applies State Update Before Validating Attestation
- **Component:** Android App
- **File:** `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt`
- **Description:** In `applyStateUpdate()`, the group's member list and epoch are mutated before the sender's attestation is checked. If the attestation fails, the group state is already corrupted.
- **Impact:** Malformed or malicious state updates permanently corrupt the local group state. Recovery requires re-syncing from scratch.
- **Remediation:** Validate the attestation (and all other invariants) before mutating any state. Use a copy-on-write pattern: build the new state, validate it, then swap atomically.

#### C-7: InviteCode Serialization Incompatible Between Platforms
- **Component:** Mobile Apps
- **File:** iOS: `clients/ios/StellarChat/StellarChat/Models/InviteCode.swift` — Android: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/model/BootstrapPayload.kt`
- **Description:** The iOS `InviteCode` and Android `BootstrapPayload` use different JSON field names, different base-encoding schemes (hex vs. base64), and different member-list serialization formats. An invitation created on iOS cannot be parsed on Android and vice versa.
- **Impact:** Cross-platform invitation exchange is broken.
- **Remediation:** Define a canonical JSON schema in the specification. Implement it identically on both platforms with round-trip tests.

#### C-8: JNI Does Not Catch Rust Panics
- **Component:** Kotlin SDK / FFI
- **File:** `mls-rs/src/ffi.rs` (JNI entry points)
- **Description:** If the Rust core panics during a JNI call (e.g., due to malformed input), the panic unwinds across the JNI boundary, which is undefined behavior in the JVM. This can crash the entire Android app without a catchable exception.
- **Impact:** Denial-of-service. Any malformed input that triggers a Rust panic kills the app.
- **Remediation:** Wrap all JNI entry points in `std::panic::catch_unwind()` and translate panics into Java exceptions.

#### C-9: Non-Canonical Field Element Decoding in Rust Core
- **Component:** Rust Crypto Core
- **File:** `src/poseidon/mod.rs`, `src/circuit/mod.rs`
- **Description:** When deserializing field elements from byte arrays (e.g., from FFI inputs), the code does not explicitly reject non-canonical encodings (values >= field modulus). Arkworks may handle this internally, but the absence of explicit checks at the boundary creates ambiguity.
- **Impact:** Potential for subtle soundness bugs if non-canonical values are accepted and produce different hashes than canonical equivalents.
- **Remediation:** Add explicit range checks at all FFI deserialization boundaries. Assert that every `Fr::from_le_bytes_mod_order()` call is preceded by a check that the input is < field modulus, or use `Fr::deserialize_compressed()` which rejects non-canonical inputs.

---

### 3.2 High Findings

#### H-1: Instance Storage TTL for Admin Key
- **Component:** Soroban Contract
- **File:** `contracts/sep-xxxx/src/lib.rs:215,224`
- **Description:** The admin address is stored in Instance storage. Instance storage has a TTL that must be bumped by calling the contract. If the contract goes unused for ~30 days, the instance storage (including admin key) can be archived. The `bump_group()` function bumps Persistent storage but does NOT bump Instance storage.
- **Impact:** After TTL expiry, the admin key is lost. The contract cannot be re-initialized (double-init guard), and no administrative actions are possible.
- **Remediation:** Add `env.storage().instance().extend_ttl()` in `bump_group()` or in a dedicated `bump_instance()` function. Consider bumping instance TTL in every public function.

#### H-2: No Epoch Freshness Check in Contract
- **Component:** Soroban Contract
- **File:** `contracts/sep-xxxx/src/lib.rs:310-380` (`update_commitment`)
- **Description:** The `update_commitment` function accepts any epoch value without checking that it is strictly greater than the current epoch. This allows replaying old commitments.
- **Impact:** Epoch rollback attack — an attacker can revert the group to a previous state.
- **Remediation:** Store the current epoch per group and require `new_epoch > current_epoch`.

#### H-3: Swift SHA-256 Commitment Reimplemented Locally
- **Component:** Swift SDK
- **File:** `swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift:96-101` and commitment computation in the SDK
- **Description:** The Swift SDK contains a `CryptoHasher` stub that doesn't actually hash — it concatenates data and returns it raw. The actual SHA-256 commitment logic is reimplemented in Swift rather than delegating to the Rust core, creating divergence risk.
- **Impact:** If the Swift and Rust implementations ever disagree on commitment computation, groups will fragment by platform.
- **Remediation:** Route all commitment computation through the Rust core via FFI. Remove the Swift-side reimplementation.

#### H-4: No Message Authentication on Nostr Group Messages
- **Component:** Mobile Apps / Transport
- **File:** iOS: `clients/ios/StellarChat/StellarChat/Nostr/NostrMessageTransport.swift` — Android: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/nostr/NostrMessageTransport.kt`
- **Description:** Group messages (kind 24114) are encrypted with AES-256-GCM (which provides authentication), but the sender identity is the Nostr pubkey — not the BLS membership key. Any participant who knows the group encryption key can impersonate any other member by using a different Nostr key.
- **Impact:** Sender impersonation within the group. The Nostr pubkey is not bound to group membership.
- **Remediation:** Include a BLS signature or membership proof in each message, or bind the Nostr identity to the BLS key via attestation and verify it on receipt.

#### H-5: No Rate Limiting on Salt Requests
- **Component:** Mobile Apps / Protocol
- **File:** iOS: `clients/ios/StellarChat/StellarChat/ViewModels/ChatViewModel.swift` — Android: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt`
- **Description:** Any group member can broadcast unlimited `sep_salt_request` messages. Every online member responds with the salt. This can be used to flood the group channel.
- **Impact:** Denial-of-service on the group channel. Network amplification (1 request → N responses).
- **Remediation:** Rate-limit salt request processing per sender. Respond only once per epoch per sender. Consider designating a single "salt server" per epoch.

#### H-6: Encryption Key Derived from Group Secret Without Epoch Binding
- **Component:** Mobile Apps / Crypto
- **File:** iOS: `clients/ios/StellarChat/StellarChat/Models/ChatGroup.swift` — Android: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/model/ChatGroup.kt`
- **Description:** The group encryption key is derived from the group secret but does not incorporate the current epoch or salt. When members are removed, the encryption key does not rotate. Removed members can continue to decrypt new messages.
- **Impact:** Forward secrecy violation. Removed members retain read access to the group.
- **Remediation:** Derive the encryption key as `HKDF(group_secret || epoch || salt)` and rotate on every membership change.

#### H-7: No Replay Protection on Protocol Messages
- **Component:** Mobile Apps / Protocol
- **File:** Both iOS and Android `NostrMessageTransport` files
- **Description:** Protocol messages (state updates, salt requests, salt responses) have no deduplication mechanism. A Nostr relay that replays old events will cause clients to reprocess state updates.
- **Impact:** Epoch confusion. Potential state corruption if an old state update is replayed after a newer one has been applied.
- **Remediation:** Track processed event IDs. Only process state updates with epoch > current epoch.

#### H-8: Salt History Unbounded in Memory
- **Component:** Android App
- **File:** `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt`
- **Description:** The `saltHistory` map grows without bound as new epochs are created. There is no eviction policy.
- **Impact:** Memory exhaustion over time for long-lived groups.
- **Remediation:** Cap salt history to a rolling window (e.g., last 100 epochs). Persist to Room database for older epochs.

#### H-9: Proof-to-Contract Format Bridge Not Validated
- **Component:** Mobile Apps / On-Chain
- **File:** iOS: `clients/ios/StellarChat/StellarChat/Models/OnChainService.swift` — Android: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/onchain/OnChainService.kt`
- **Description:** The proof format bridge (192-byte compressed → 384-byte uncompressed) is implemented in app code without validation that the decompressed points are on the curve and in the correct subgroup.
- **Impact:** Malformed proofs could bypass contract verification or cause contract-side panics.
- **Remediation:** Perform point validation after decompression, or move the format bridge into the Rust core where arkworks provides validated deserialization.

#### H-10: BIP39 Mnemonic Not Implemented Despite Documentation Claims
- **Component:** Documentation / Implementation
- **File:** `README.md`, `docs/phase-4.md`
- **Description:** Documentation references BIP39 mnemonic backup for key recovery, but neither the iOS nor Android app implements mnemonic generation, display, or recovery.
- **Impact:** Users have no key backup mechanism. Key loss is permanent and unrecoverable.
- **Remediation:** Either implement BIP39 mnemonic backup or remove the claim from documentation. This is a critical UX gap for production.

#### H-11: Relayer Has No Payload Validation
- **Component:** Architecture / Design
- **File:** Design only — relayer is not implemented in the repo
- **Description:** The relayer pattern (Phase 4) is specified but the relayer server itself is not implemented. The design does not specify what validation the relayer should perform on payloads before submitting them.
- **Impact:** A malicious client could submit arbitrary Soroban invocations through the relayer, using it as a fee-paying proxy for unrelated transactions.
- **Remediation:** Implement the relayer with strict payload validation: whitelist only the SEP contract address, only the expected function names, and validate proof structure before submission.

#### H-12: Android Nostr Subscription Does Not Filter by Timestamp
- **Component:** Android App
- **File:** `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/nostr/NostrMessageTransport.kt:50-55`
- **Description:** The Nostr subscription filter for group messages does not include a `since` timestamp. On subscription, the relay sends all historical events for the group, which are all decrypted and processed.
- **Impact:** Processing flood on subscription. Old protocol messages are replayed, potentially corrupting state (see H-7).
- **Remediation:** Include `"since": <last_known_timestamp>` in the subscription filter.

#### H-13: iOS Nostr Subscription Uses Custom Tag Names
- **Component:** iOS App
- **File:** `clients/ios/StellarChat/StellarChat/Nostr/NostrMessageTransport.swift:47-53`
- **Description:** iOS uses `sep_topic` and `sep_version` custom tags. Standard Nostr relays may not index custom tags, causing filter-based subscriptions to return empty results on some relays.
- **Impact:** Message delivery failure on relays that don't index custom tags.
- **Remediation:** Use standard NIP tag names or ensure relay compatibility. The `t` tag (used by Android) is a standard hashtag tag indexed by all relays.

#### H-14: No TLS Certificate Pinning for Relayer Communication
- **Component:** Mobile Apps
- **File:** iOS: `clients/ios/StellarChat/StellarChat/Models/OnChainService.swift` — Android: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/onchain/SEPContractClient.kt`
- **Description:** HTTP communication with the relayer uses standard URL loading without certificate pinning. A MITM attacker on the network path could intercept proofs.
- **Impact:** Proof interception. While proofs are zero-knowledge, the metadata (group ID, timing) leaks identity information — exactly what the relayer pattern is meant to prevent.
- **Remediation:** Implement TLS certificate pinning for the relayer endpoint, or use a Tor hidden service for the relayer.

---

### 3.3 Medium Findings

#### M-1: Non-Standard Poseidon Round Constants
- **File:** `src/poseidon/mod.rs:93`
- **Description:** Round constants are generated from a custom SHA-256 seed string rather than the standard Poseidon paper's method. This means proofs are not compatible with other Poseidon implementations.
- **Remediation:** Document this as intentional, or switch to standard Poseidon parameters for interoperability.

#### M-2: Contract Does Not Emit Events
- **File:** `contracts/sep-xxxx/src/lib.rs`
- **Description:** No `env.events().publish()` calls. Off-chain indexers cannot track group lifecycle.
- **Remediation:** Emit events for `create_group`, `update_commitment`, `deactivate_group`.

#### M-3: Hardcoded Nostr Relay URLs
- **File:** iOS: `clients/ios/StellarChat/StellarChat/Nostr/NostrMessageTransport.swift:15-18` — Android: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/nostr/NostrMessageTransport.kt:20-23`
- **Description:** Default relay URLs (`relay.damus.io`, `nos.lol`) are hardcoded. While configurable at runtime, the defaults may become unavailable.
- **Remediation:** Ship with a broader default set or implement relay discovery.

#### M-4: No Group Size Limit in Contract
- **File:** `contracts/sep-xxxx/src/lib.rs`
- **Description:** The contract stores commitments without limiting the number of groups or commitment history depth.
- **Remediation:** Add configurable limits to prevent storage abuse.

#### M-5: Invitation Encryption Uses Ephemeral Key Without Identity Binding
- **File:** iOS: `clients/ios/StellarChat/StellarChat/Models/InvitationSender.swift` — Android: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/nostr/InvitationTransport.kt`
- **Description:** Invitation encryption uses an ephemeral X25519 key pair, but the ephemeral public key is not signed by the sender's identity key. A MITM could substitute their own ephemeral key.
- **Remediation:** Sign the ephemeral public key with the sender's Ed25519 identity key.

#### M-6: Topic Tag Derivation Not Specified
- **File:** Both iOS and Android `ChatGroup` models
- **Description:** The `topicTag` derivation from group ID is not specified in a common place, creating risk of platform divergence.
- **Remediation:** Specify the derivation in the SDK or specification document.

#### M-7: Android Uses JSONObject Instead of Kotlinx Serialization
- **File:** Multiple Android files
- **Description:** JSON parsing uses `org.json.JSONObject` throughout, which is untyped and error-prone. Missing fields produce runtime exceptions rather than compile-time errors.
- **Remediation:** Migrate to `kotlinx.serialization` with `@Serializable` data classes.

#### M-8: No Timeout on Relay Connections
- **File:** iOS and Android `NostrRelayConnection` files
- **Description:** WebSocket connections to Nostr relays have no configurable timeout or reconnection strategy.
- **Remediation:** Add connection timeout, heartbeat, and exponential backoff reconnection.

#### M-9: Contract Verification Key Storage Is Per-Tier, Not Per-Group
- **File:** `contracts/sep-xxxx/src/lib.rs:225-233`
- **Description:** Verification keys are stored per tier (small/medium/large), not per group. All groups of the same tier share a VK. If the VK needs to be rotated (e.g., due to a circuit bug), all groups of that tier are affected simultaneously.
- **Remediation:** Document this as intentional design. Consider adding per-group VK override capability for migration.

#### M-10: No Graceful Degradation When Contract Is Unavailable
- **File:** iOS and Android `OnChainService` files
- **Description:** On-chain operations fail hard when the Soroban RPC endpoint is unavailable. There is no offline queue or retry mechanism.
- **Remediation:** Queue operations and retry with backoff. Allow groups to function in "unverified" mode when the contract is unreachable.

#### M-11: Swift FFI Uses Raw Pointers Without Bounds Checking
- **File:** `swift-mls/Sources/SwiftMLS/` (C FFI bridge files)
- **Description:** The Swift-to-Rust FFI bridge passes raw pointers and lengths. If the Swift side passes incorrect lengths, the Rust side may read out of bounds.
- **Remediation:** Add length validation on the Rust side of every FFI entry point.

#### M-12: Android EncryptedSharedPreferences May Block Main Thread
- **File:** `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/crypto/KeyManager.kt`
- **Description:** `EncryptedSharedPreferences` initialization and access are synchronous and may block the main thread, especially on first launch when the master key is generated.
- **Remediation:** Initialize `EncryptedSharedPreferences` on a background thread.

#### M-13: No Certificate Transparency for Soroban RPC Endpoint
- **File:** Both platforms' contract client code
- **Description:** The Soroban RPC endpoint URL is user-configurable but not validated. Users could be directed to a malicious RPC endpoint that returns fake verification results.
- **Remediation:** Consider maintaining a list of known-good RPC endpoints or implementing result cross-validation.

#### M-14: Merkle Tree Depth Hardcoded
- **File:** `src/merkle/mod.rs`, `src/circuit/mod.rs`
- **Description:** The Merkle tree depth is fixed at compile time. Changing the depth requires regenerating the proving/verification keys and migrating all groups.
- **Remediation:** Document the maximum group size implied by the tree depth. Plan for depth migration if needed.

#### M-15: No Input Sanitization on Group Names
- **File:** iOS and Android group creation screens
- **Description:** Group names are stored and transmitted without sanitization. Unicode control characters, excessively long names, or names containing JSON special characters could cause display or parsing issues.
- **Remediation:** Validate and sanitize group names on creation.

#### M-16: Persistent Storage Encryption Key Derivation Path
- **File:** `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/crypto/StorageEncryption.kt`
- **Description:** The storage encryption key is derived via HKDF from a root secret, but the derivation path is not versioned. If the derivation needs to change, existing encrypted data becomes unreadable.
- **Remediation:** Include a version number in the HKDF info string and store the version alongside encrypted data.

#### M-17: iOS App Does Not Persist Salt History
- **File:** `clients/ios/StellarChat/StellarChat/ViewModels/AppState.swift`
- **Description:** Salt history is held in memory only. App restart loses all epoch salts, requiring re-request from peers.
- **Remediation:** Persist salt history to the local database.

#### M-18: Group Deactivation Has No Confirmation/Delay
- **File:** Both platforms' UI code
- **Description:** Group deactivation is immediate and irreversible on-chain. There is no confirmation dialog or grace period.
- **Remediation:** Add a confirmation dialog. Consider a two-phase deactivation (mark pending, then finalize after delay).

#### M-19: No Logging or Audit Trail
- **File:** Entire codebase
- **Description:** There is no structured logging for security-relevant events (failed decryption, invalid attestation, proof verification failure, etc.).
- **Remediation:** Add structured logging for security events. Ensure logs do not contain sensitive data (keys, plaintexts).

---

### 3.4 Low Findings

#### L-1: `CryptoHasher` Stub in Swift SDK
- **File:** `swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift:97-101`
- **Description:** The `CryptoHasher` struct concatenates data without actually hashing. It exists to compute the binding message but is misleadingly named.
- **Remediation:** Rename to `BindingMessageBuilder` or similar.

#### L-2: Magic Numbers in Contract
- **File:** `contracts/sep-xxxx/src/lib.rs:36-37`
- **Description:** TTL constants (17,280 and 518,400) are defined but not documented with their real-world meaning.
- **Remediation:** Add comments: "~1 day at 5s ledger close" and "~30 days."

#### L-3: Unused `senderNostrPubkey` in BootstrapPayload
- **File:** Android: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/model/BootstrapPayload.kt`
- **Description:** The `senderNostrPubkey` field is serialized but never used for validation on receipt.
- **Remediation:** Either validate that the sender matches the Nostr event pubkey, or remove the field.

#### L-4: Test Count vs. Coverage Gap
- **File:** All test files
- **Description:** 96 total tests across the repo. Strong coverage for crypto primitives and commitment construction. Weak or absent coverage for: protocol message round-trips, invitation encryption/decryption, cross-platform serialization, relayer transport, UI state management.
- **Remediation:** See Section 4.6 for specific missing test categories.

#### L-5: Contract Tests Don't Exercise Real Groth16 Verification
- **File:** `contracts/sep-xxxx/src/lib.rs:652-762`
- **Description:** All 8 contract tests use synthetic/empty proofs. No test submits a real Groth16 proof generated by the Rust core and verifies it through the contract.
- **Remediation:** Add an integration test that generates a real proof in the Rust core and verifies it through the contract.

#### L-6: Inconsistent Error Handling Patterns
- **File:** Android codebase (various)
- **Description:** Some functions throw exceptions, others return nullable types, and some use sealed classes. No consistent error handling pattern.
- **Remediation:** Standardize on `Result<T>` or sealed error classes.

#### L-7: No API Versioning in Protocol Messages
- **File:** `swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift`
- **Description:** Protocol messages (state updates, salt requests) have a `type` field but no `version` field. Future protocol changes will be indistinguishable from current ones.
- **Remediation:** Add a `version` field to all protocol messages.

#### L-8: iOS Uses `sep_version` Tag but Doesn't Check It
- **File:** `clients/ios/StellarChat/StellarChat/Nostr/NostrMessageTransport.swift:122`
- **Description:** iOS attaches a `sep_version: "1"` tag to outgoing messages but never checks it on incoming messages.
- **Remediation:** Either check the version on receipt or remove the tag.

#### L-9: Android Coroutine Scope Not Lifecycle-Aware
- **File:** `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/nostr/NostrMessageTransport.kt:27`
- **Description:** Uses `CoroutineScope(Dispatchers.IO)` instead of a lifecycle-aware scope. Jobs may leak if the transport is not explicitly disconnected.
- **Remediation:** Use `viewModelScope` or a `SupervisorJob` with proper cancellation.

#### L-10: Soroban Contract Has No Upgrade Path
- **File:** `contracts/sep-xxxx/src/lib.rs`
- **Description:** No `upgrade()` function. Contract code cannot be updated after deployment.
- **Remediation:** Add an admin-guarded `upgrade()` function, or document the redeployment strategy.

#### L-11: Ed25519 Key Derivation Not Cross-Validated
- **File:** iOS: `StellarChat/Models/KeyManager.swift` — Android: `crypto/KeyManager.kt`
- **Description:** Both platforms derive Ed25519 keys from the Nostr secret via HKDF, but there are no cross-platform test vectors to verify identical derivation.
- **Remediation:** Add shared test vectors (Nostr secret → Ed25519 pubkey) and validate on both platforms.

#### L-12: Unused Imports in Several Files
- **File:** Various
- **Description:** Several files import modules that are not used (e.g., `Foundation` imports in Swift files that don't use Foundation types).
- **Remediation:** Clean up unused imports.

#### L-13: No Code Signing or Reproducible Build Configuration
- **File:** Build configuration files
- **Description:** Neither the iOS nor Android build configuration includes deterministic/reproducible build settings.
- **Remediation:** Configure reproducible builds for release artifacts.

#### L-14: README Claims Features Not Yet Implemented
- **File:** `README.md`
- **Description:** The README describes BIP39 backup, relay discovery, and multi-device sync — none of which are implemented.
- **Remediation:** Clearly mark unimplemented features as "Planned" or remove them.

#### L-15: No Contribution Guidelines
- **File:** Repository root
- **Description:** No CONTRIBUTING.md, no PR template, no issue templates.
- **Remediation:** Add contribution guidelines if the project will accept external contributions.

#### L-16: Contract Tests Use `#[cfg(test)]` Module Without Test Utilities
- **File:** `contracts/sep-xxxx/src/lib.rs:648-762`
- **Description:** Test setup is duplicated across test functions (creating env, registering contract, initializing). No shared test fixture.
- **Remediation:** Extract a `setup()` helper for test fixture creation.

---

## 4. Subsystem-by-Subsystem Status

### 4.1 Rust Crypto Core (`src/`)

| Aspect | Status | Notes |
|--------|--------|-------|
| Poseidon hash | Working | Custom parameters (M-1), well-tested (11 tests) |
| Merkle tree | Working | 20 tests, depth hardcoded (M-14) |
| Groth16 circuit | Working | 18 tests, zero-key rejection tested |
| Field element handling | Needs review | Non-canonical decoding risk (C-9) |
| FFI exports | Working | Panic handling missing for JNI (C-8) |

**Overall: Strong.** The Rust core is the most mature component. Address C-8 and C-9 for production.

### 4.2 Soroban Contract (`contracts/sep-xxxx/`)

| Aspect | Status | Notes |
|--------|--------|-------|
| Group lifecycle | Working | create, update, deactivate, get_state |
| Authorization | **Broken** | No auth on create_group (C-1) |
| Proof verification | Working | But not bound to group_id (C-3), replayable (C-2) |
| Storage management | Partial | TTL for persistent OK, instance TTL at risk (H-1) |
| Events | Missing | No events emitted (M-2) |
| Tests | Incomplete | No real proof tested (L-5) |

**Overall: Functional but insecure.** The three Critical contract findings (C-1, C-2, C-3) must be fixed before any mainnet deployment.

### 4.3 Swift SDK (`swift-mls/`)

| Aspect | Status | Notes |
|--------|--------|-------|
| Core types | Working | GroupStateUpdate, protocol messages |
| Attestation | **Broken** | verify() always true (C-5) |
| Commitment | Risk | Local reimplementation (H-3) |
| FFI bridge | Working | Raw pointer risk (M-11) |
| Tests | 6 tests | Good for core, missing protocol round-trips |

**Overall: Usable but has a critical attestation gap.** Fix C-5 before trusting attestations.

### 4.4 Kotlin SDK (`mls-rs/`)

| Aspect | Status | Notes |
|--------|--------|-------|
| JNI bridge | Working | But panics unhandled (C-8) |
| Attestation | Working | Actual Ed25519 verification |
| Key derivation | Working | HKDF-based, Bouncy Castle |

**Overall: Functional.** Fix C-8 for production stability.

### 4.5 Mobile Apps

| Aspect | iOS | Android | Notes |
|--------|-----|---------|-------|
| Group messaging | Working | Working | Tag mismatch (C-4) breaks cross-platform |
| Invitations | Working | Working | Serialization incompatible (C-7) |
| State updates | Working | **Bug** | Android mutates before validating (C-6) |
| On-chain ops | Working | Working | Proof format bridge unvalidated (H-9) |
| Relayer support | Working | Working | No TLS pinning (H-14) |
| Persistence | Working | Working | Salt not persisted on iOS (M-17) |
| Key backup | Missing | Missing | Documented but not implemented (H-10) |

**Overall: Functional individually but not interoperable.** The cross-platform issues (C-4, C-7) are the most impactful findings for real-world deployment.

### 4.6 Test Coverage Analysis

**What is well-tested (high confidence):**
- Poseidon hash: correctness, edge cases, zero-input behavior
- Merkle tree: construction, proof generation, verification, depth limits
- Groth16 circuit: proof generation, constraint satisfaction, zero-key rejection
- Commitment construction: member sorting, determinism
- BLS key derivation: deterministic from seed
- Android persistence: Room database CRUD, encryption round-trips

**What is NOT tested (low confidence):**
1. Contract Groth16 verification with real proofs
2. Cross-platform serialization round-trips (invitation, state update)
3. Cross-platform key derivation consistency (same seed → same keys)
4. Protocol message handling (state update apply, salt request/response)
5. Nostr message encryption/decryption round-trips
6. Invitation encryption/decryption (X25519 ECDH)
7. Relayer transport (HTTP request format, error handling)
8. Proof format bridge (192 → 384 byte conversion)
9. Attestation creation and verification round-trip
10. Group deactivation flow
11. Epoch advancement and salt rotation
12. Concurrent state update handling
13. Nostr relay reconnection and message deduplication
14. Storage encryption key rotation

---

## 5. Coherence Matrix

This matrix tracks whether each specified feature is consistently implemented across all components that need it.

| Feature | Specification | Rust Core | Contract | Swift SDK | iOS App | Kotlin SDK | Android App | Status |
|---------|:---:|:---:|:---:|:---:|:---:|:---:|:---:|--------|
| Poseidon Merkle commitment | phase-2.md | Yes | Verifies | Delegates | Uses | Delegates | Uses | **Coherent** |
| Groth16 ZKP generation | phase-2.md | Yes | Verifies | Delegates | Uses | Delegates | Uses | **Coherent** |
| Nostr group messaging | phase-1.md | N/A | N/A | N/A | `sep_topic` | N/A | `t` | **INCOHERENT (C-4)** |
| Invitation protocol | phase-2.md | N/A | N/A | Custom | iOS format | N/A | Android format | **INCOHERENT (C-7)** |
| Key attestation verify | phase-4.md | N/A | N/A | No-op | Trusts SDK | Actual | Actual | **INCOHERENT (C-5)** |
| State update protocol | phase-4.md | N/A | N/A | Defined | Uses | N/A | Uses | **Coherent** (except C-6) |
| Fee decoupling / relayer | phase-4.md | N/A | N/A | Defined | Uses | N/A | Uses | **Coherent** |
| Group deactivation | phase-4.md | N/A | Yes | N/A | Uses | N/A | Uses | **Coherent** |
| BIP39 key backup | README | N/A | N/A | No | No | No | No | **MISSING (H-10)** |
| Ed25519 key derivation | phase-1.md | N/A | N/A | HKDF | Uses | HKDF | Uses | **Untested** (L-11) |

---

## 6. Production-Readiness Scorecard

| Dimension | Score | Rationale |
|-----------|:-----:|-----------|
| **Cryptographic correctness** | 7/10 | Rust core is solid. Attestation verification gap (C-5) and non-canonical input risk (C-9) reduce score. |
| **Contract security** | 3/10 | Three critical authorization/replay vulnerabilities (C-1, C-2, C-3). Not safe for mainnet. |
| **Cross-platform interop** | 2/10 | Tag mismatch (C-4) and serialization incompatibility (C-7) mean iOS and Android cannot communicate. |
| **Privacy guarantees** | 6/10 | ZK proofs work. Relayer pattern designed. But sender impersonation (H-4) and no forward secrecy (H-6) weaken the model. |
| **Test coverage** | 5/10 | Strong primitive-level tests. No integration or cross-platform tests. No real proof→contract test. |
| **Documentation** | 7/10 | Phase docs are thorough and well-written. Some claims not backed by code (H-10, L-14). |
| **Operational readiness** | 3/10 | No logging (M-19), no monitoring, no relay fallback, no key backup, no contract upgrade path. |
| **Overall** | **4.7/10** | **Not production-ready.** Fix Criticals and High interop issues for beta. |

---

## 7. Open Questions and Assumptions

1. **SEP Number:** The protocol uses `SEP-XXXX` as a placeholder throughout. Is there a designated SEP number? All binding messages, contract names, and HKDF info strings reference it.

2. **Poseidon Parameter Compatibility:** The custom round constant generation means proofs are not portable to other Poseidon implementations. Is this intentional (internal-only use) or a future interop concern?

3. **Relayer Trust Model:** The relayer sees the full contract payload including group_id. Is this acceptable for the privacy model, or should the relayer operate on encrypted/blinded payloads?

4. **Nostr Relay Selection:** The system depends on public Nostr relays. What is the fallback if `relay.damus.io` and `nos.lol` become unavailable or start censoring kind 24114 events?

5. **Key Rotation Strategy:** If a user's Nostr secret key is compromised, all derived keys (BLS, Ed25519, X25519) are compromised simultaneously. Is there a key rotation protocol?

6. **Circuit Upgrade Path:** If the Groth16 circuit needs to be updated (bug fix, depth change), how are existing groups migrated? The contract stores VKs per tier, not per circuit version.

7. **Soroban BLS12-381 Host Functions:** The contract relies on Soroban host functions for BLS12-381 operations. Are these stable/finalized in the Soroban SDK, or still experimental?

8. **Group Size Limits:** The Merkle tree depth implies a maximum group size. What is the configured depth and what is the resulting maximum? Is this sufficient for the target use cases?

---

## 8. Recommended Remediation Order

Priority is determined by: (1) severity, (2) blast radius, (3) ease of fix.

### Immediate (blocks any deployment)

| # | Finding | Effort | Why First |
|---|---------|--------|-----------|
| 1 | **C-4**: Fix Nostr tag mismatch | Small | iOS and Android literally cannot talk to each other. One-line fix on one platform. |
| 2 | **C-7**: Unify invitation serialization | Medium | Same issue — platforms can't invite each other. Define canonical JSON schema. |
| 3 | **C-1**: Add auth to `create_group` | Small | One `require_auth()` call. Prevents storage spam. |
| 4 | **C-3**: Bind `group_id` to proof | Medium | Requires circuit change + VK regeneration. Blocks C-2 fix. |
| 5 | **C-2**: Add function selector to proof | Medium | Depends on C-3. Same circuit change. |

### Before Beta

| # | Finding | Effort | Why |
|---|---------|--------|-----|
| 6 | **C-5**: Fix Swift attestation verify | Small | Either implement real verification or change the API. |
| 7 | **C-6**: Validate before mutating state | Small | Move validation before mutation in Android. |
| 8 | **C-8**: Catch JNI panics | Small | Wrap in `catch_unwind()`. Prevents app crashes. |
| 9 | **C-9**: Validate field element inputs | Small | Add range checks at FFI boundaries. |
| 10 | **H-1**: Bump instance storage TTL | Small | One line in contract. Prevents admin key loss. |
| 11 | **H-2**: Add epoch freshness check | Small | Prevents commitment rollback. |
| 12 | **H-6**: Epoch-bind encryption key | Medium | Requires key derivation change + salt integration. |
| 13 | **H-7**: Add protocol message replay protection | Medium | Track processed event IDs per group. |

### Before GA

| # | Finding | Effort | Why |
|---|---------|--------|-----|
| 14 | **H-3**: Route commitments through Rust | Medium | Eliminates platform divergence risk. |
| 15 | **H-4**: Bind sender identity to messages | Large | Requires protocol-level change. |
| 16 | **H-9**: Validate proof decompression | Medium | Move to Rust core. |
| 17 | **H-10**: Implement BIP39 or remove docs claim | Medium | UX-critical for production. |
| 18 | **H-11**: Implement relayer server | Large | Required for the privacy model to work. |
| 19 | **M-2**: Add contract events | Small | Enables off-chain indexing. |
| 20 | **L-5**: Add real proof→contract integration test | Medium | Highest-value missing test. |

---

*End of audit report.*
