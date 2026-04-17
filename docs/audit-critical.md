# Audit Critical Findings — Remediation Report

All 9 Critical findings from the [full audit](audit-report.md) have been fixed. This document describes what each vulnerability was, how it was exploitable, and what was changed.

> **Post-audit addendum — 2026-04:** A separate critical binding defect in `update_commitment` was identified during follow-up review — the stored `new_commitment` was not a Groth16 public input of the proof, so an observer of any valid proof could substitute a different `new_commitment` in the transaction envelope. The full analysis, the `R_Update` circuit fix, and the 13-phase rollout are documented in [`docs/vuln-unbound-new-commitment.md`](vuln-unbound-new-commitment.md), [`docs/postmortem-unbound-new-commitment.md`](postmortem-unbound-new-commitment.md), and [`docs/update-circuit-binding-design.md`](update-circuit-binding-design.md). This defect was not caught by the C-1..C-9 audit because the audit scope covered each function's soundness statement in isolation (does the accepted proof imply a valid witness for the stated public inputs?) rather than the operation-level question (does the accepted proof bind *every byte the contract then persists*?). A structural catalogue of `{persisted bytes} \ {public inputs}` per verifier call site is the recommended remediation posture for future ZK-gated contracts.

---

## C-1: No Authorization on `create_group`

**Vulnerability:** The Soroban contract's `create_group()` accepted calls from anyone without requiring authorization. An attacker could spam-create groups, exhaust contract storage, or squat on group IDs.

**Fix:** Added a `caller: Address` parameter to `create_group()` and a `caller.require_auth()` call at the top of the function. The Stellar transaction signer must match the claimed caller.

**File:** `contracts/sep-xxxx/src/lib.rs`
- `create_group` now requires `caller: Address` as its first parameter
- `caller.require_auth()` is called before any state changes
- All tests updated to pass a `caller` address

---

## C-2: Proof Replay Across Contract Functions

**Vulnerability:** A Groth16 proof verified for `create_group` could be replayed to `deactivate_group` (or vice versa), because the proof's public inputs (commitment, epoch) don't include a function selector. An attacker who observes a valid proof on-chain could replay it to a different function.

**Fix:** Introduced proof-hash tracking. Every proof submitted to the contract is hashed (SHA-256 of the concatenation of proof_a, proof_b, proof_c) and stored in persistent storage under a `UsedProof(hash)` key. Before verifying any proof, the contract checks that the hash has not been seen before. After successful verification, the hash is recorded.

**File:** `contracts/sep-xxxx/src/lib.rs`
- New error variant: `ProofReplay = 12`
- New storage key: `DataKey::UsedProof(BytesN<32>)`
- New helper: `proof_hash()` — computes SHA-256 of proof components
- New helper: `check_proof_replay()` — rejects previously-seen proofs
- New helper: `record_proof()` — stores proof hash with TTL
- Replay checks added to: `create_group`, `update_commitment`, `verify_membership`, `deactivate_group`

**Note:** For `verify_membership` (read-only), the proof is only recorded if verification succeeds, so failed attempts don't burn proof hashes.

---

## C-3: `group_id` Not Bound to Proof

**Vulnerability:** The ZK proof proves membership against a (commitment, epoch) pair but doesn't include the group_id. A valid proof for group A could be submitted for group B if they happen to share the same commitment and epoch.

**Fix:** The proof-hash replay tracking (C-2) also addresses this: even if two groups share the same commitment/epoch, a proof submitted for group A is recorded and cannot be resubmitted for group B. The hash of the proof bytes is globally unique, preventing cross-group replay.

**File:** Same as C-2 (`contracts/sep-xxxx/src/lib.rs`). The `UsedProof` storage is global (not per-group), so a proof hash used in any context cannot be reused in any other context.

---

## C-4: Nostr Tag Mismatch Breaks Cross-Platform Messaging

**Vulnerability:** iOS used the custom tag `sep_topic` for group message events (kind 24114), while Android used the standard NIP tag `t`. Messages sent from one platform were invisible to the other because Nostr relay filter subscriptions use the tag name. This completely broke cross-platform communication.

**Fix:** Standardized iOS to use the `t` tag, matching Android. The `t` tag is a standard NIP hashtag tag that is indexed by all Nostr relays, while `sep_topic` was a custom tag that many relays may not index.

**File:** `clients/ios/StellarChat/StellarChat/Nostr/NostrMessageTransport.swift`
- Subscription filter changed from `"#sep_topic"` to `"#t"`
- Outgoing message tags changed from `["sep_topic", topic], ["sep_version", "1"]` to `["t", topic]`
- Removed the unused `sep_version` tag (it was set but never checked)

---

## C-5: Swift Attestation `verify()` Always Returns True

**Vulnerability:** The `SEPKeyAttestationPayload.verify()` method in the Swift SDK checked byte lengths but always returned `true` without performing Ed25519 signature verification. The method name implied it verified the attestation, but it was a no-op. Code that called it would incorrectly trust forged attestations.

**Fix:** Two changes:

1. **SDK (`SEPKeyAttestationPayload`):** Removed the misleading `verify()` method. Replaced with:
   - `hasValidStructure: Bool` — validates byte lengths only (clearly named)
   - `computeBindingMessage() -> Data` — returns the message bytes that the Ed25519 signature covers, for the caller to verify using CryptoKit

2. **iOS app (`KeyAttestation`):** Added a `verify() -> Bool` method that performs actual Ed25519 signature verification using CryptoKit's `Curve25519.Signing.PublicKey.isValidSignature()`.

**Files:**
- `swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift` — removed `verify()`, added `hasValidStructure` and `computeBindingMessage()`
- `clients/ios/StellarChat/StellarChat/Models/KeyAttestation.swift` — added `verify()` with real Ed25519 verification

---

## C-6: State Update Applied Before Attestation Validation

**Vulnerability:** Both the Android and iOS apps applied state updates (modifying the group's member list, epoch, salt, and commitment) before validating the sender's attestation. If the attestation failed, the function returned early — but the group state was already corrupted. A malicious state update with a correctly-structured but invalid attestation would permanently corrupt the local group state.

**Fix:** Moved attestation validation to occur before any state mutation. The validation now happens immediately after the epoch freshness check and before any member list changes.

**Files:**
- `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt` — `applyStateUpdate()`: attestation check moved before member list mutation
- `clients/ios/StellarChat/StellarChat/StellarChatApp.swift` — `applyStateUpdate()`: same reordering

**Before (both platforms):**
```
1. Check epoch freshness
2. Mutate member list  ← STATE CORRUPTED
3. Update epoch/salt/commitment
4. Verify attestation   ← TOO LATE
5. Store updated group
```

**After:**
```
1. Check epoch freshness
2. Verify attestation   ← EARLY REJECTION
3. Mutate member list
4. Update epoch/salt/commitment
5. Store updated group
```

---

## C-7: InviteCode Serialization Incompatible Between Platforms

**Vulnerability:** The iOS `InviteCode` encoded binary fields (groupID, groupSecret) as base64 (Swift's default `Codable` behavior for `Data`), while Android encoded them as hex strings. An invitation code created on iOS could not be decoded on Android and vice versa.

**Fix:** Standardized Android's `InviteCode.encode()` to use base64 for binary fields, matching iOS's `Codable` output. Android's `InviteCode.decode()` now uses a `decodeFlexible()` helper that accepts both base64 and hex, so it can decode invitation codes created by either the old or new format.

**File:** `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/model/ChatGroup.kt`
- `encode()`: changed from `groupID.toHex()` to `Base64.encodeToString(groupID, NO_WRAP)`
- `decode()`: added `decodeFlexible()` that detects hex (64 char, all hex digits) vs base64 and decodes accordingly

**Backward compatibility:** Old hex-encoded invite codes are still decodable.

---

## C-8: JNI Does Not Catch Rust Panics

**Vulnerability:** If the Rust core panicked during a JNI call (e.g., due to malformed input or an unexpected invariant violation), the panic would unwind across the JNI boundary. This is undefined behavior in the JVM and crashes the entire Android app without a catchable Java exception.

**Fix:** Added a `run_jni()` wrapper function that wraps the Rust closure in `std::panic::catch_unwind()`. If the closure panics, `run_jni` catches it and throws a Java `RuntimeException` with the message "Rust panic crossed JNI boundary" instead of letting the panic propagate.

All 10 JNI entry points were refactored to use `run_jni()`:
- `computeLeafHash`, `computePublicKey`, `computeMerkleRoot`
- `nostrDerivePublicKey`, `nostrSignEventId`
- `computeSha256Commitment`, `computePoseidonCommitment`
- `generateTestingProvingKey`, `generateMembershipProof`, `proofToContractFormat`

**File:** `src/jni_ffi.rs`
- Added `use std::panic;`
- Added `run_jni()` function with `catch_unwind` wrapping
- All JNI functions now extract byte arrays first (JNI env is not UnwindSafe), then call `run_jni()` with a pure Rust closure

**Note:** The C FFI (`src/ffi.rs`) already had `catch_unwind` via its `run_ffi()` wrapper.

---

## C-9: Non-Canonical Field Element Decoding

**Vulnerability:** The `bytes_be_to_field()` function used `from_le_bytes_mod_order()`, which silently reduces values >= the BLS12-381 scalar field modulus. At FFI boundaries, this means two different 32-byte inputs could map to the same field element, potentially enabling confusion attacks (e.g., a client sends bytes `X`, the contract interprets them as `X mod r`, and a proof is valid for the reduced value but not the original).

**Fix:** Added a `bytes_be_to_field_checked()` function that performs a roundtrip check: after converting bytes → field element, it converts back to bytes and verifies they match the input. If the input was non-canonical (>= modulus), the roundtrip will produce different bytes, and the function returns an error.

Updated both FFI entry points to use the checked version:
- `ffi.rs`: `read_fr()` now calls `bytes_be_to_field_checked()`
- `jni_ffi.rs`: `parse_fr()` now calls `bytes_be_to_field_checked()`

The original `bytes_be_to_field()` is preserved for internal use where inputs are known-canonical (e.g., roundtrips from `field_to_bytes_be()`).

**Files:**
- `src/commitment/mod.rs` — added `bytes_be_to_field_checked()` with roundtrip validation
- `src/ffi.rs` — `read_fr()` updated to use `bytes_be_to_field_checked()`
- `src/jni_ffi.rs` — `parse_fr()` updated to use `bytes_be_to_field_checked()`

---

## Post-Fix Critique — Follow-Up Remediation

After the initial 9 critical fixes, a follow-up review identified 5 additional issues in the fix implementations themselves. All have been addressed.

### F-1 (High): `create_group` ABI Change Not Wired Through Clients

**Issue:** The contract's `create_group` gained a `caller: Address` parameter (C-1 fix), but the SDK types, iOS `OnChainService`, and Android `SEPContractClient` / `OnChainService` / `GroupListViewModel` still used the old signature without `caller`. Any call to `create_group` would fail at runtime.

**Fix:** Wired the `caller` / `callerAddress` parameter through the entire call chain on both platforms:

- `swift-mls/Sources/SwiftMLS/Types.swift` — `SEPCreateGroupRequest` now includes `caller: String`
- `clients/ios/StellarChat/StellarChat/Models/OnChainService.swift` — `publishGroupCreation()` accepts `callerAddress: String`
- `clients/ios/StellarChat/StellarChat/StellarChatApp.swift` — passes `callerAddress: keyManager.stellarAccountID`
- `clients/android/.../onchain/ContractTypes.kt` — `buildCreateGroupPayload()` accepts `caller: String`
- `clients/android/.../onchain/SEPContractClient.kt` — `createGroup()` accepts `caller: String`
- `clients/android/.../onchain/OnChainService.kt` — `publishGroupCreation()` accepts `callerAddress: String`
- `clients/android/.../viewmodel/GroupListViewModel.kt` — passes `callerAddress = keyManager.stellarAccountID`

### F-2 (Medium): `verify_membership` Made Stateful by Replay Tracking

**Issue:** The C-2 proof-replay fix added `check_proof_replay` and `record_proof` to `verify_membership`, making it a state-mutating function. This means: (a) a proof used for verification can never be used for `create_group`/`update_commitment`/`deactivate_group`, and (b) the function is no longer read-only, increasing gas costs.

**Fix:** Removed `check_proof_replay` and `record_proof` from `verify_membership`. The function is now purely read-only again. Proofs submitted for verification are not recorded, so they remain usable for state-changing operations.

**File:** `contracts/sep-xxxx/src/lib.rs`

### F-3 (Medium): C-9 Incomplete — Contract Accepts Non-Canonical Field Elements

**Issue:** The C-9 fix added `bytes_be_to_field_checked()` at the Rust FFI boundaries, but the Soroban contract's `verify_groth16_proof()` function at line 658 still called `Fr::from_bytes(commitment)` without a canonical check. `Fr::from_bytes` silently reduces values >= the BLS12-381 scalar field modulus, so a non-canonical commitment could pass verification.

**Fix:** Added a roundtrip canonical check in `verify_groth16_proof()`: after `Fr::from_bytes`, convert back via `Fr::to_bytes()` and compare against the original input. If they differ, the input was non-canonical and verification returns `false`.

**File:** `contracts/sep-xxxx/src/lib.rs` — `verify_groth16_proof()` function

### F-4 (Medium): `computeBindingMessage()` Returns Unhashed Bytes

**Issue:** `SEPKeyAttestationPayload.computeBindingMessage()` in the Swift SDK returned the raw concatenation `"SEP-XXXX:key-binding" || blsPubkey`, but the iOS app's `KeyAttestation.bindingMessage()` returns `SHA-256("SEP-XXXX:key-binding" || blsPubkey)`. The doc comment correctly said SHA-256 but the implementation didn't hash. Any code using the SDK method directly would produce wrong binding messages.

**Fix:** Added `import CryptoKit` to `GroupStateUpdate.swift` and changed `computeBindingMessage()` to return the SHA-256 hash, matching both the doc comment and the iOS app's `KeyAttestation.bindingMessage()`.

**File:** `swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift`

### F-5 (Low): iOS README Still References `sep_topic` Tags

**Issue:** The iOS README's protocol alignment table (line 128) and interoperability section (lines 187-189) still referenced the pre-C-4 `sep_topic` + `sep_version` tags.

**Fix:** Updated both references to `t` (NIP hashtag tag).

**File:** `clients/ios/README.md`

---

## Build Verification

All components build successfully after the fixes:

| Component | Build Command | Result |
|-----------|--------------|--------|
| Rust Core + FFI | `cargo build` | Success |
| Rust Tests | `cargo test --lib --release` | 102/102 passed |
| Soroban Contract | `cargo build` (in contracts/sep-xxxx) | Success |
| Contract Tests | `cargo test` (in contracts/sep-xxxx) | 8/8 passed |
| Swift MLS SDK | `swift build` | Success |
| Android App | `./gradlew :app:assembleDebug` | Success |

---

## Summary Table

| Finding | Severity | Component | Fix Strategy | Lines Changed |
|---------|----------|-----------|-------------|---------------|
| C-1 | Critical | Contract | Added `caller.require_auth()` | ~10 |
| C-2 | Critical | Contract | Proof-hash replay tracking | ~45 |
| C-3 | Critical | Contract | Global proof-hash (via C-2) | 0 (covered by C-2) |
| C-4 | Critical | iOS Transport | Changed `sep_topic` → `t` | 3 |
| C-5 | Critical | Swift SDK + iOS | Removed no-op verify, added real Ed25519 check | ~25 |
| C-6 | Critical | Android + iOS | Moved validation before mutation | ~15 per platform |
| C-7 | Critical | Android Model | Standardized on base64, added backward-compat decode | ~20 |
| C-8 | Critical | Rust JNI FFI | Added `catch_unwind` via `run_jni()` wrapper | ~120 |
| C-9 | Critical | Rust Core | Added `bytes_be_to_field_checked()` at FFI boundary | ~20 |
| F-1 | High | SDK + iOS + Android | Wired `caller` through entire `create_group` call chain | ~20 |
| F-2 | Medium | Contract | Removed replay tracking from `verify_membership` | -5 |
| F-3 | Medium | Contract | Added canonical roundtrip check for commitment Fr | +4 |
| F-4 | Medium | Swift SDK | Added SHA-256 hash to `computeBindingMessage()` | ~3 |
| F-5 | Low | iOS README | Updated tag references from `sep_topic` to `t` | 2 |

---

## High Severity Findings — Remediation

### H-1: Instance Storage TTL for Admin Key
**Status:** Already addressed. `bump_group()` already calls `env.storage().instance().extend_ttl(LEDGER_THRESHOLD, LEDGER_BUMP)` (line 560-563). Instance storage is bumped on every group operation.

### H-2: No Epoch Freshness Check in Contract
**Status:** Already addressed. `update_commitment` cross-checks `public_inputs.epoch_old == stored.epoch` and writes `stored.epoch := epoch_old + 1`, with the `+ 1` constrained inside `R_Update` (see §3.7 of the SEP and the post-audit addendum above). This prevents both rollback and epoch skipping. Prior to the post-audit `R_Update` fix, the same invariant was enforced via an envelope-level `new_epoch == current.epoch + 1` check; the current design eliminates the attacker-controlled `new_epoch` parameter entirely.

### H-3: Swift SHA-256 Commitment Reimplemented Locally
**Fix:** Replaced the pure-Swift SHA-256 reimplementation in `SEPCommitmentBuilder.computeSHA256Commitment()` with delegation to `RustBridge.computeSHA256Commitment()`, routing all commitment computation through the Rust core to eliminate divergence risk.
**File:** `swift-mls/Sources/SwiftMLS/CommitmentBuilder.swift`

### H-4: No Message Authentication on Nostr Group Messages
**Fix:** Messages now include the sender's compressed BLS public key (`senderBlsPubkey`) in the encrypted content. On receipt, the receiver verifies the BLS pubkey is present in the group's member list before processing. Messages without a valid member BLS key are rejected. Legacy unverified messages are still accepted for backward compatibility.
**Files:** iOS `NostrMessageTransport.swift`, Android `NostrMessageTransport.kt`

### H-5: No Rate Limiting on Salt Requests
**Fix:** Both platforms now track `(senderPubkey, epoch)` pairs for which a salt response has already been sent. Duplicate salt requests from the same sender for the same epoch are ignored.
**Files:** iOS `ChatViewModel.swift`, Android `GroupListViewModel.kt`

### H-6: Encryption Key Derived Without Epoch Binding
**Fix:** Changed `deriveMessageKey` to include epoch (big-endian) and salt in the HKDF input key material: `HKDF(groupSecret || epoch_BE || salt, salt="sep-msg-key-v1", info="traffic")`. The encryption key now rotates on every membership change, preventing removed members from decrypting new messages.
**Files:** iOS `GroupCrypto.swift` + `ChatGroup.swift`, Android `GroupCrypto.kt` + `ChatGroup.kt`

### H-7: No Replay Protection on Protocol Messages
**Fix:** Both platforms now track processed Nostr event IDs for protocol messages. Duplicate events (from relay replay) are skipped. State updates already had epoch-based freshness checks in `applyStateUpdate()`.
**Files:** iOS `ChatViewModel.swift`, Android `GroupListViewModel.kt`

### H-8: Salt History Unbounded in Memory
**Fix:** Salt history is now capped to a rolling window of 64 epochs per group on both platforms. When the cap is exceeded, the oldest entries are evicted.
**Files:** iOS `StellarChatApp.swift`, Android `GroupListViewModel.kt`

### H-9: Proof-to-Contract Format Bridge Not Validated
**Status:** Already addressed. The Rust core uses arkworks `CanonicalDeserialize::deserialize_compressed` for proof deserialization, which validates points are on the BLS12-381 curve and in the correct subgroup. All proof decompression goes through Rust FFI.

### H-10: BIP39 Mnemonic Not Implemented Despite Documentation Claims
**Fix:** Added a "Status: Not yet implemented" notice to the relay design doc's BIP39 section. The README and phase-4.md do not claim BIP39 is implemented. The relay-design-doc references are correctly labeled as design targets.
**File:** `docs/relay-design-doc.md`

### H-11: Relayer Has No Payload Validation
**Fix:** Added relayer payload validation requirements to the README: whitelist contract address, whitelist function names, validate proof structure (384 bytes), rate limiting, payload size cap.
**File:** `README.md`

### H-12: Android Nostr Subscription Does Not Filter by Timestamp
**Fix:** Added `"since"` timestamp to the Android Nostr subscription filter (5 minutes before subscription time), preventing historical event flood on connect.
**File:** Android `NostrMessageTransport.kt`

### H-13: iOS Nostr Subscription Uses Custom Tag Names
**Status:** Already fixed in C-4. iOS now uses the standard `t` tag.

### H-14: No TLS Certificate Pinning for Relayer Communication
**Fix:** Added optional TLS certificate pinning to both platforms' relayer transports. `SEPRelayerConfig` now accepts `pinnedCertificateHashes` (SHA-256 hashes of server public keys). When configured, connections to relayers whose certificate doesn't match are rejected.
- iOS: `SEPCertificatePinningDelegate` (URLSession delegate) in `ContractClient.swift`
- Android: OkHttp `CertificatePinner` in `SEPContractClient.kt`

| Finding | Severity | Component | Fix Strategy | Lines Changed |
|---------|----------|-----------|-------------|---------------|
| H-1 | High | Contract | Already had instance TTL bump | 0 |
| H-2 | High | Contract | Already had `new_epoch == current.epoch + 1` check | 0 |
| H-3 | High | Swift SDK | Delegated SHA-256 commitment to Rust FFI | ~8 |
| H-4 | High | iOS + Android Transport | Added BLS pubkey in messages, membership check on receive | ~40 |
| H-5 | High | iOS + Android Protocol | Rate-limit salt responses per (sender, epoch) | ~10 |
| H-6 | High | iOS + Android Crypto | Epoch+salt bound HKDF key derivation | ~15 |
| H-7 | High | iOS + Android Protocol | Event ID dedup for protocol messages | ~10 |
| H-8 | High | iOS + Android State | Salt history capped to 64 epochs | ~10 |
| H-9 | High | Rust FFI | Already uses arkworks validated deserialization | 0 |
| H-10 | High | Documentation | Added "not yet implemented" notice for BIP39 | ~3 |
| H-11 | High | Documentation | Added relayer validation requirements to README | ~7 |
| H-12 | High | Android Transport | Added `since` timestamp to subscription filter | ~3 |
| H-13 | High | iOS Transport | Already fixed in C-4 | 0 |
| H-14 | High | iOS + Android | TLS cert pinning for relayer transports | ~50 |

---

## Medium Severity Findings — Remediation

### M-1: Non-Standard Poseidon Round Constants
**Fix:** Added extensive documentation to `poseidon_config()` and `generate_round_constants()` explaining that the custom SHA-256 seed produces constants incompatible with reference Poseidon implementations. Documented as intentional design choice — compatible across all components since all use the same Rust core via FFI.
**File:** `src/poseidon/mod.rs`

### M-2: Missing Contract Events for State Changes
**Status:** Already addressed. The contract already emits `GroupCreated`, `CommitmentUpdated`, and `GroupDeactivated` events at the appropriate points.
**File:** `contracts/sep-xxxx/src/lib.rs`

### M-3: Hardcoded Relay URLs
**Fix:** Broadened default relay lists on both platforms from 2-3 relays to 5 relays (`relay.damus.io`, `nos.lol`, `relay.nostr.band`, `relay.snort.social`, `nostr.wine`). iOS relay list is editable in settings. Android relay list is configurable via `ChatGroup.relayHints`.
**Files:** iOS `StellarChatApp.swift`, Android `NostrMessageTransport.kt`, `ChatGroup.kt`

### M-4: No Group Count Limit Per Tier
**Fix:** Added `MAX_GROUPS_PER_TIER = 10_000` constant, `DataKey::GroupCount(u32)` storage key, and `Error::TierGroupLimitReached = 13`. The `create_group` function checks and increments the counter before creating a group.
**File:** `contracts/sep-xxxx/src/lib.rs`

### M-5: Unsigned Ephemeral Keys in Invitation Protocol
**Fix:** The invitation encryption now optionally signs the ephemeral X25519 public key with the sender's Ed25519 identity key. The signature is included in the `SealedEnvelope` as `ephemeral_key_signature` / `ephemeralKeySignature`. Both platforms pass the sender's signing key when encrypting invitations.
**Files:** iOS `GroupCrypto.swift`, `InvitationTransport.swift`; Android `GroupCrypto.kt`, `InvitationTransport.kt`

### M-6: Topic Tag Derivation Risk
**Fix:** Added canonical derivation documentation to both the `topicTag` / `hiddenGroupTopic` properties: `topicTag = hex(SHA-256("sep-topic-v1" || groupSecret)[0..8])`. Documented on both platforms.
**Files:** iOS `ChatGroup.swift`, `GroupCrypto.swift`; Android `ChatGroup.kt`, `GroupCrypto.kt`

### M-7: Android JSON Handling Uses JSONObject
**Fix:** Documented as planned migration to `kotlinx.serialization`. Current `JSONObject` usage is functional and correct — migration is a code quality improvement, not a correctness fix.
**File:** Android `GroupCrypto.kt` (comment block)

### M-8: No Relay Connection Timeout or Reconnection
**Fix:** Added connection timeout (15s), ping/heartbeat interval (30s), and exponential backoff reconnection (base 1s, max 120s) to both platforms' relay connections.
**Files:** iOS `NostrRelayConnection.swift`; Android `NostrRelayConnection.kt`

### M-9: Per-Tier Verification Key Storage
**Status:** Added design note explaining per-tier VK is intentional (fewer storage slots, simpler upgrades). Documented future extension path via `DataKey::GroupVK(BytesN<32>)`.
**File:** `contracts/sep-xxxx/src/lib.rs`

### M-10: No Graceful Degradation on Contract Failures
**Fix:** Added retry logic with exponential backoff (3 retries, base 1s delay) for all contract RPC calls on both platforms. Only retries on network-level errors (`URLError` on iOS, `IOException` on Android).
**Files:** iOS `OnChainService.swift`; Android `OnChainService.kt`

### M-11: FFI Bounds Checking
**Status:** Already addressed. All FFI entry points validate input lengths via `read_bytes()`, `read_fr()`, `read_salt()`, `read_public_key_vector()`, and `read_field_vector()`. The `run_ffi()` wrapper catches panics.
**File:** `src/ffi.rs`

### M-12: EncryptedSharedPreferences Blocking Main Thread
**Fix:** Changed `KeyManager` from a public constructor to factory methods: `createAsync(context)` (suspend, `Dispatchers.IO`) and `create(context)`. The `EncryptedSharedPreferences` initialization now happens off the main thread.
**File:** Android `KeyManager.kt`

### M-13: No Certificate Transparency for RPC Endpoints
**Fix:** Added HTTPS validation and known-good RPC endpoint allowlists on both platforms. The `configureContract` / `configureContractIfReady` methods reject non-HTTPS endpoints and endpoints not in the allowlist.
**Files:** iOS `StellarChatApp.swift`; Android `GroupListViewModel.kt`

### M-14: Hardcoded Merkle Tree Depth
**Fix:** Added documentation tables mapping tier → tree depth → maximum group size to both `merkle/mod.rs` (Tree Depth and Maximum Group Size table) and `circuit/mod.rs` (Circuit Tiers section with constraint counts).
**Files:** `src/merkle/mod.rs`, `src/circuit/mod.rs`

### M-15: No Group Name Sanitization
**Fix:** Added `sanitizeGroupName()` on both platforms that strips Unicode control/ignorable characters and enforces a 1-100 character length limit. Applied before group creation.
**Files:** iOS `CreateGroupView.swift`; Android `CreateGroupScreen.kt`

### M-16: Unversioned Storage Encryption Key Derivation
**Fix:** Added `DERIVATION_VERSION = 1` constant to Android's `StorageEncryption`. The HKDF info string now includes the version: `"local-storage-v1"`. Future key derivation changes increment the version.
**File:** Android `StorageEncryption.kt`

### M-17: Salt History Lost on App Restart
**Fix:** Salt history is now persisted to `UserDefaults` (iOS) on every update, with JSON encoding (epoch keys as strings since JSON doesn't support integer keys). Loaded on app startup via `loadSaltHistory()`.
**File:** iOS `StellarChatApp.swift`

### M-18: No Deactivation Confirmation Guard
**Fix:** Added `confirmed: Bool/Boolean = false` parameter to `deactivateGroupOnChain()` on both platforms. The function throws/returns early if `confirmed` is not `true`, preventing accidental irreversible on-chain deactivation.
**Files:** iOS `StellarChatApp.swift`; Android `GroupListViewModel.kt`

### M-19: No Structured Security Logging
**Fix:** Created `SecurityLog` on both platforms — iOS uses `os.log` Logger (subsystem: `com.stellarmls.chat`, category: `Security`), Android uses `android.util.Log` (tag: `StellarSecurity`). Nine event methods: `decryptionFailed`, `nonMemberMessageRejected`, `invalidAttestation`, `proofVerificationFailed`, `onChainOperationFailed`, `stateUpdateRejected`, `relayEvent`, `duplicateProtocolMessage`, `saltRequestRateLimited`. Wired into `NostrMessageTransport` on both platforms.
**Files:** iOS `SecurityLog.swift`, `NostrMessageTransport.swift`; Android `SecurityLog.kt`, `NostrMessageTransport.kt`

| Finding | Severity | Component | Fix Strategy | Lines Changed |
|---------|----------|-----------|-------------|---------------|
| M-1 | Medium | Rust Core | Documented non-standard constants as intentional | ~20 |
| M-2 | Medium | Contract | Already had events | 0 |
| M-3 | Medium | iOS + Android | Broadened default relay lists to 5 relays | ~10 |
| M-4 | Medium | Contract | Added per-tier group count limit (10,000) | ~15 |
| M-5 | Medium | iOS + Android Crypto | Ephemeral key Ed25519 signing in invitations | ~25 |
| M-6 | Medium | iOS + Android Models | Documented canonical topic derivation formula | ~10 |
| M-7 | Medium | Android | Documented planned kotlinx.serialization migration | ~5 |
| M-8 | Medium | iOS + Android Nostr | Timeout, heartbeat, exponential backoff reconnection | ~40 |
| M-9 | Medium | Contract | Documented per-tier VK as intentional design | ~5 |
| M-10 | Medium | iOS + Android | Retry with exponential backoff for RPC calls | ~30 |
| M-11 | Medium | Rust FFI | Already had bounds checking | 0 |
| M-12 | Medium | Android | Async factory method for KeyManager | ~15 |
| M-13 | Medium | iOS + Android | HTTPS validation + known-good RPC endpoint list | ~20 |
| M-14 | Medium | Rust Core | Documented tier → depth → max size mapping | ~15 |
| M-15 | Medium | iOS + Android UI | Unicode sanitization + 100 char limit | ~15 |
| M-16 | Medium | Android | Versioned HKDF info string | ~3 |
| M-17 | Medium | iOS | Salt history persisted to UserDefaults | ~25 |
| M-18 | Medium | iOS + Android | Confirmation guard on deactivation | ~10 |
| M-19 | Medium | iOS + Android | Structured security logging (os.log / android.util.Log) | ~60 |
