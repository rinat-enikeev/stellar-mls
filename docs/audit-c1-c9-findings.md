# Audit Findings for Fixes C-1 through C-9

Scope: read-only audit of the current unstaged diff on `main` at `68c2376`. I did not run Rust tests per request; findings below are from source review and call-site tracing.

## Findings

### 1. High: `create_group` ABI/auth change is not wired through any client or SDK

The contract now requires a `caller: Address` parameter and calls `caller.require_auth()` in [`contracts/sep-xxxx/src/lib.rs:257-267`](../contracts/sep-xxxx/src/lib.rs). None of the client-facing request types or payload builders were updated to supply that new argument:

- Swift SDK `SEPCreateGroupRequest` still only contains `groupID`, `commitment`, `proof`, `publicInputs`, and `tier` in [`swift-mls/Sources/SwiftMLS/Types.swift:92-105`](../swift-mls/Sources/SwiftMLS/Types.swift).
- Swift app `publishGroupCreation()` still constructs that old request shape in [`clients/ios/StellarChat/StellarChat/Models/OnChainService.swift:126-133`](../clients/ios/StellarChat/StellarChat/Models/OnChainService.swift).
- Android `buildCreateGroupPayload()` still emits the old JSON without a caller field in [`clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/onchain/ContractTypes.kt:52-68`](../clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/onchain/ContractTypes.kt).
- Android `SEPContractClient.createGroup()` and `OnChainService.publishGroupCreation()` still use that old shape in [`clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/onchain/SEPContractClient.kt:52-64`](../clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/onchain/SEPContractClient.kt) and [`clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/onchain/OnChainService.kt:84-100`](../clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/onchain/OnChainService.kt).

As written, this patch breaks `create_group` end to end. It also conflicts with the current fee-decoupled transport story: the repo’s relayer/direct HTTP transports do not expose a way to express or satisfy Soroban address authorization for this new argument.

### 2. Medium: `verify_membership` is no longer read-only and now consumes proofs

The contract comment still says `verify_membership` is "Read-only — does not modify contract state" in [`contracts/sep-xxxx/src/lib.rs:391-394`](../contracts/sep-xxxx/src/lib.rs), but the implementation now checks replay state and records successful proofs in [`contracts/sep-xxxx/src/lib.rs:415-428`](../contracts/sep-xxxx/src/lib.rs). That changes the API semantics materially:

- the same proof can no longer be verified twice
- a verification call can now fail with `ProofReplay`
- proof verification is no longer a pure boolean check

The app/service layers still document and model this as a read-only verification path in [`clients/ios/StellarChat/StellarChat/Models/OnChainService.swift:266-270`](../clients/ios/StellarChat/StellarChat/Models/OnChainService.swift) and [`clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/onchain/OnChainService.kt:218-223`](../clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/onchain/OnChainService.kt).

This may be an intentional tradeoff, but it is not a safe silent change. At minimum, the contract/API/docs need to describe `verify_membership` as a stateful proof-consumption endpoint, not a read-only verifier.

### 3. Medium: C-9 is incomplete because the contract still accepts non-canonical field encodings

The new off-chain validation is correct at the Rust FFI/JNI boundary: `bytes_be_to_field_checked` rejects reduced encodings in [`src/commitment/mod.rs:104-115`](../src/commitment/mod.rs), and the FFI/JNI now call it through `read_fr`/`parse_fr`.

But the on-chain verifier still parses the commitment with `Fr::from_bytes(commitment.clone())` in [`contracts/sep-xxxx/src/lib.rs:658`](../contracts/sep-xxxx/src/lib.rs), which means the contract-side public input path still accepts non-canonical field encodings modulo the scalar field order. So the patch improves external SDK input validation, but it does not fully close the canonical-encoding issue in the actual verification boundary that matters most.

### 4. Medium: `SEPKeyAttestationPayload.computeBindingMessage()` returns the wrong bytes

The new public Swift SDK API claims to return the signed binding message `SHA-256("SEP-XXXX:key-binding" || bls_pubkey)` in [`swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift:84-89`](../swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift), but the implementation returns the unhashed concatenation in [`swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift:90-93`](../swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift).

That does not match the app-side attestation implementation, which actually hashes before verifying in [`clients/ios/StellarChat/StellarChat/Models/KeyAttestation.swift:22-27`](../clients/ios/StellarChat/StellarChat/Models/KeyAttestation.swift).

So C-5 moved the verification burden to the caller, but the helper now gives callers the wrong message bytes to verify. A consumer following the SDK docstring will verify the wrong payload and reject valid attestations.

### 5. Low: the iOS relay docs are now stale about message tags

The iOS message transport now publishes and subscribes on `["t", topic]` / `#t` in [`clients/ios/StellarChat/StellarChat/Nostr/NostrMessageTransport.swift:49-52`](../clients/ios/StellarChat/StellarChat/Nostr/NostrMessageTransport.swift) and [`clients/ios/StellarChat/StellarChat/Nostr/NostrMessageTransport.swift:120-122`](../clients/ios/StellarChat/StellarChat/Nostr/NostrMessageTransport.swift).

But the iOS README still claims group messages use `sep_topic` + `sep_version` and that Android interop uses that same structure in [`clients/ios/README.md:128`](../clients/ios/README.md) and [`clients/ios/README.md:187-189`](../clients/ios/README.md). The code is now aligned with Android transport, but the docs are not.

## Checked Items Without a New Finding

- `C-6` Android state update validation moved before mutation. On read, that change fixes the earlier ordering bug in [`clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt:418-429`](../clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt).
- `C-7` InviteCode base64 standardization looks directionally correct and appears compatible with iOS `Codable` `Data` encoding in [`clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/model/ChatGroup.kt:82-121`](../clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/model/ChatGroup.kt) and [`clients/ios/StellarChat/StellarChat/Models/ChatGroup.swift:70-85`](../clients/ios/StellarChat/StellarChat/Models/ChatGroup.swift).
- `C-8` JNI panic catching appears to cover all exported JNI entrypoints in [`src/jni_ffi.rs:143-347`](../src/jni_ffi.rs).

## Summary

The most important issue is that C-1 introduces a contract ABI/auth requirement that the rest of the repository does not satisfy. After that, the two main correctness problems are:

1. `verify_membership` silently changed from read-only verification to proof-consuming state mutation.
2. The Swift SDK attestation helper now returns bytes that do not match the documented signed payload.

C-9 is only partially fixed because the contract still accepts non-canonical field encodings on-chain.
