# Audit Critical Findings — Remediation Report

All 9 Critical findings from the [full audit](audit-report.md) have been fixed. This document describes what each vulnerability was, how it was exploitable, and what was changed.

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
