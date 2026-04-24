# Proof of Correctness -- SEP-XXXX Implementation

**SEP-XXXX Zero-Knowledge Group Membership Protocol**

*Version 1.0 -- April 7, 2026*

---

## 1. Preamble

### 1.1 Scope

This document provides a systematic correctness argument for the SEP-XXXX reference implementation. It proves that the **code faithfully implements the protocol** specified in `docs/sep.md`. This is complementary to -- and distinct from -- the Proof of Soundness (`docs/proof-of-soundness.md`), which proves that the **mathematical constructions are secure** under standard cryptographic assumptions.

The Proof of Soundness establishes, for example, that the Poseidon Merkle tree is collision-resistant under Assumption 3.2. This document establishes that the Rust implementation of `PoseidonMerkleTree::build_from_members()` actually constructs the tree described in the proof -- correct depth, correct padding, correct node computation, correct canonical ordering -- and that the test suite provides evidence for each claim.

**In scope.** Functional correctness of the core Rust library, FFI bridges (C and JNI), Soroban smart contract, relayer, PN-Relay, and mobile clients. Cross-platform determinism. Protocol conformance against the SEP specification.

**Out of scope.** Cryptographic security reductions (covered by the Proof of Soundness), performance optimization arguments, deployment procedures, and network-layer privacy analysis.

### 1.2 Relationship to Proof of Soundness

The two documents form a layered assurance argument:

| Layer | Document | Question Answered |
|-------|----------|-------------------|
| Mathematical security | Proof of Soundness | Are the cryptographic constructions secure? |
| Implementation correctness | **This document** | Does the code implement those constructions correctly? |

A secure protocol implemented incorrectly is insecure. Conversely, a correct implementation of an insecure protocol inherits the protocol's weaknesses. Both documents are necessary for end-to-end assurance.

Each theorem in the Proof of Soundness depends on implementation assumptions. For example, Theorem 1 (Merkle Tree Collision Resistance) assumes that the implementation uses the Poseidon hash function with the specified parameters and constructs the tree at the specified depth with zero-padded leaves. Section 5.2 of this document maps each theorem to the code that realizes its assumptions.

### 1.3 Codebase Version

This analysis covers commit `1ed6e212f06fe742129c3d12dd61651772e61ca9` on the `main` branch of the `stellar-mls` repository. All file paths and line numbers reference this commit. Subsequent changes may invalidate specific line references but the structural arguments remain valid unless the module interfaces change.

---

## 2. Correctness Model

### 2.1 Definition of Implementation Correctness

An implementation is **correct** with respect to a specification if, for all inputs in the specification's domain:

1. **Functional equivalence.** The implementation's outputs match the specification's defined outputs.
2. **Constraint adherence.** The implementation enforces all constraints and invariants required by the specification.
3. **Cross-platform determinism.** All platform-specific implementations (Rust, iOS via FFI, Android via JNI) produce identical outputs for identical inputs.
4. **Failure mode correctness.** The implementation rejects all inputs that the specification defines as invalid, and does so with the correct error classification.

We do not require formal verification. Instead, we provide a structured argument for each module combining code inspection, specification tracing, and empirical evidence from the test suite.

### 2.2 Correctness Properties

The following 14 properties collectively define what it means for the implementation to be correct. Each property is traced to specific code, specification sections, and tests in Sections 3--6.

| ID | Property | Description | Spec Reference |
|----|----------|-------------|----------------|
| C-1 | Poseidon parameters | Seed string, round counts (8 full, 56 partial), MDS matrix construction, and alpha exponent (5) match SEP-XXXX Section 2.2 | SEP Section 2.2 |
| C-2 | Poseidon determinism | `poseidon_hash_one` and `poseidon_hash_two` are deterministic pure functions: identical inputs always produce identical outputs | SEP Section 2.2 |
| C-3 | Leaf computation | Each leaf is computed as `Poseidon(sk)` where `sk` is the member's BLS12-381 private key scalar | SEP Section 2.2 |
| C-4 | Member ordering | Members are sorted in ascending lexicographic order by their 48-byte compressed G1 public key representation; duplicates are rejected | SEP Section 2.1 |
| C-5 | Merkle tree construction | Binary tree of fixed depth `d` (5, 8, or 11), zero-padded unused leaves, internal nodes computed as `Poseidon(left, right)` | SEP Section 2.2 |
| C-6 | Commitment Variant B | `Poseidon(Poseidon(root, epoch), salt)` where salt is interpreted as a field element via `from_le_bytes_mod_order` | SEP Section 2.3 |
| C-7 | Commitment Variant A | `SHA-256(root_be \|\| epoch_be \|\| salt)` with fixed 72-byte preimage (32 + 8 + 32) | SEP Section 2.3 |
| C-8 | Circuit constraints | Exactly 3 logical constraints (key ownership, Merkle membership, commitment binding) with 2 public inputs (commitment, epoch) | SEP Section 3.3 |
| C-9 | Proof format | 192 bytes compressed: G1 (48 bytes) \|\| G2 (96 bytes) \|\| G1 (48 bytes); 384 bytes uncompressed for on-chain submission | SEP Section 3.4 |
| C-10 | Epoch enforcement | `new_epoch == stored_epoch + 1` enforced by the contract with `checked_add` overflow protection | SEP Section 4 |
| C-11 | Proof replay prevention | SHA-256 hash of uncompressed proof components (384 bytes) stored and checked before every state-changing operation | SEP Section 4 |
| C-12 | Groth16 pairing | Verification equation `e(-piA, piB) * e(alpha, beta) * e(vk_x, gamma) * e(piC, delta) = 1_GT` implemented via Soroban BLS12-381 host functions | SEP Section 3.5 |
| C-13 | FFI canonicality | Field elements received from C/JNI callers are checked for canonical form (value < field modulus) via roundtrip encoding; non-canonical values are rejected | Appendix B of Proof of Soundness |
| C-14 | Cross-platform determinism | Given identical inputs (secret keys, member sets, salts, epochs), the Rust library, iOS FFI, and Android JNI produce byte-identical outputs (leaf hashes, roots, commitments, proofs) | SEP Section 2 (implicit) |

### 2.3 Cross-Component Invariants

These invariants span module boundaries and must hold throughout any execution path.

| ID | Invariant | Components Involved | Description |
|----|-----------|-------------------|-------------|
| INV-1 | Commitment pipeline consistency | Poseidon, Merkle, Commitment, Circuit | The commitment computed by `compute_poseidon_commitment()` outside the circuit equals the value computed by the circuit's Constraint 3 for the same inputs |
| INV-2 | Proof decompression format | Prover, FFI, Contract | The 192-byte compressed proof produced by `proof_to_bytes()` decompresses to the same 384-byte representation expected by the Soroban contract |
| INV-3 | Epoch chain monotonicity | Contract, Relayer | The contract enforces strict `epoch + 1` sequencing; the relayer does not interfere with this invariant |
| INV-4 | Key consistency across platforms | FFI (C), FFI (JNI), iOS, Android | BLS12-381 secret key bytes interpreted by the C FFI, JNI FFI, iOS, and Android all produce the same field element |
| INV-5 | Leaf consistency across platforms | Poseidon, FFI (C), FFI (JNI) | `Poseidon(sk)` computed via the C FFI equals `Poseidon(sk)` computed via the JNI FFI for the same `sk` bytes |
| INV-6 | Root consistency across platforms | Merkle, FFI (C), FFI (JNI) | Given the same member set, the Merkle root computed via C FFI equals the root computed via JNI FFI |
| INV-7 | Relayer transparency | Relayer, Contract | The relayer wraps and submits transactions but does not modify proof bytes, public inputs, or group identifiers |
| INV-8 | PN-Relay confidentiality | PN-Relay, Nostr | The PN-Relay cannot read the content of encrypted Nostr events; it only observes topic tags for notification routing |

### 2.4 Testing Hierarchy

The test suite is organized in four layers, from unit tests to integration tests:

| Layer | Scope | Test Count | Location |
|-------|-------|------------|----------|
| Unit | Individual functions within a module | ~104 | `src/*/mod.rs` (`#[cfg(test)]` modules) |
| FFI | C and JNI bridge functions | 34 | `tests/ffi_tests.rs` and related |
| Contract | Soroban smart contract logic | 56 | `contracts/sep-xxxx/src/lib.rs` |
| Service | Relayer and PN-Relay HTTP handlers | 73 | `relayer/src/`, `pn-relay/src/` |
| Client | iOS and Android end-to-end | ~78 | Swift and Kotlin test suites |
| Cross-platform | Determinism across FFI boundaries | 2 | `tests/cross_platform_tests.rs` |
| **Total** | | **~345** | |

---

## 3. Module-by-Module Correctness Arguments

### 3.1 Poseidon Hash

**Source.** `src/poseidon/mod.rs` (283 lines)

**What it does.** Implements the Poseidon hash function over the BLS12-381 scalar field with the parameters specified in SEP-XXXX Section 2.2. Provides two entry points: `poseidon_hash_one(config, x)` for leaf hashing and `poseidon_hash_two(config, left, right)` for internal Merkle nodes and commitment binding.

**Correctness argument.**

1. **Parameter fidelity (C-1).** The constants `FULL_ROUNDS = 8`, `PARTIAL_ROUNDS = 56`, `RATE = 2`, `CAPACITY = 1`, `ALPHA = 5` at lines 17--29 match SEP-XXXX Section 2.2 verbatim. The `poseidon_config()` function (line 49) assembles these into an `ark_crypto_primitives::PoseidonConfig` struct, which is the standard arkworks representation.

2. **Round constant derivation (C-1).** The `generate_round_constants()` function (line 102) uses the seed `"SEP-XXXX-Poseidon-BLS12-381-w3-f8-p56-a5-round-constants"` and derives 192 constants via iterated SHA-256 with 64-byte extension for uniform field element sampling. This matches the procedure described in SEP-XXXX Section 2.2 and Proof of Soundness Appendix A.

3. **MDS matrix (C-1).** The `generate_mds_matrix()` function (line 128) implements the Cauchy construction `M[i][j] = 1 / (x_i + y_j)` with `x_i = i + 1`, `y_j = width + j + 1`, matching Proof of Soundness Appendix A.

4. **Determinism (C-2).** Both hash functions are pure: they allocate a fresh `PoseidonSponge`, absorb inputs, and squeeze output. No mutable state persists between calls. No randomness is consumed.

**Test coverage.** 11 tests covering: config creation, determinism (same inputs yield same output), hash-one and hash-two non-triviality, distinct inputs yield distinct outputs, round constant count verification, and MDS matrix invertibility.

### 3.2 Merkle Tree

**Source.** `src/merkle/mod.rs` (536 lines)

**What it does.** Constructs a binary Poseidon Merkle tree of fixed depth from a set of member records. Provides canonical ordering of members, tree construction, root computation, and opening proof generation/verification.

**Correctness argument.**

1. **Canonical ordering (C-4).** The `canonicalize_members()` function (line 204) sorts members by their 48-byte compressed G1 public key bytes in ascending lexicographic order via `sort_by(|lhs, rhs| lhs.public_key_bytes.cmp(&rhs.public_key_bytes))`. Duplicate public keys are detected by a linear scan of adjacent pairs (line 210) and rejected with `MerkleError::DuplicatePublicKey`.

2. **Public key derivation.** The `compressed_public_key_bytes()` function (line 192) computes `sk * G1` using arkworks' `G1Projective::generator()` scalar multiplication, converts to affine, and serializes compressed to 48 bytes. This matches SEP-XXXX Section 1.1.

3. **Leaf computation (C-3).** Each leaf is computed as `poseidon_hash_one(config, &sk)` where `sk` is the BLS12-381 secret key scalar, matching SEP-XXXX Section 2.2: "leaf[i] = Poseidon(sk[i])".

4. **Tree construction (C-5).** The `build_from_ordered_leaf_hashes()` method (line 96) creates a flat array of `2^(d+1)` nodes. Leaves occupy indices `[2^d, 2^(d+1))`. Unused slots are `F::zero()` (line 111). Internal nodes are computed bottom-up as `poseidon_hash_two(config, &left, &right)` (line 123). The root is at index 1.

5. **Depth enforcement (C-5).** The function rejects member sets that exceed `2^depth` via the `TooManyMembers` error (line 103).

6. **Opening proof generation.** The `prove()` method (line 140) walks from the leaf to the root, collecting sibling hashes. The `verify()` method (line 165) recomputes the root from the leaf and path, checking `current == *root`.

**Test coverage.** 20 tests covering: single-member trees, full trees, zero padding, opening proof generation and verification, canonical ordering (shuffled inputs produce same root), duplicate rejection, depth overflow rejection, and cross-depth isolation.

### 3.3 Commitment Construction

**Source.** `src/commitment/mod.rs` (374 lines)

**What it does.** Implements both commitment variants from SEP-XXXX Section 2.3.

**Correctness argument.**

1. **Variant A (C-7).** `compute_commitment()` (line 55) constructs a 72-byte preimage: `field_to_bytes_be(root)` (32 bytes) followed by `epoch.to_be_bytes()` (8 bytes) followed by `salt` (32 bytes). The preimage is hashed with SHA-256. This matches SEP-XXXX Section 2.3 Variant A exactly: fixed-length, no padding, no separators.

2. **Variant B (C-6).** `compute_poseidon_commitment()` (line 156) computes `poseidon_hash_two(config, root, Fr::from(epoch))` then `poseidon_hash_two(config, inner, Fr::from_le_bytes_mod_order(salt))`. The two-layer Poseidon structure matches SEP-XXXX Section 2.3 Variant B.

3. **Field element serialization.** `field_to_bytes_be()` (line 91) serializes via arkworks' little-endian canonical form then reverses to big-endian. `bytes_be_to_field()` (line 110) performs the inverse. `bytes_be_to_field_checked()` (line 123) additionally validates canonicality by roundtripping.

4. **Constant-time comparison.** `verify_commitment()` (line 138) uses `constant_time_eq()` (line 27), which accumulates XOR differences across all bytes before returning. This prevents timing side-channel leakage of partial match information.

**Test coverage.** 19 tests covering: preimage length (72 bytes), commitment determinism, Variant A and B consistency, field element serialization roundtrip, canonical check rejection of non-canonical values, constant-time comparison correctness, and salt entropy preservation.

### 3.4 Circuit

**Source.** `src/circuit/mod.rs` (785 lines)

**What it does.** Defines the R1CS circuit for Groth16 proving. The `MembershipCircuit` struct implements `ConstraintSynthesizer<Fr>`, generating constraints for key ownership, Merkle membership, and commitment binding.

**Correctness argument.**

1. **Public inputs (C-8).** Exactly two values are allocated as public inputs via `FpVar::new_input()`: `commitment_var` (line 125) and `epoch_var` (line 129). All other values are witnesses (`FpVar::new_witness()` or `Boolean::new_witness()`).

2. **Constraint 1 -- Key ownership (C-8).** The circuit computes `leaf_var = poseidon_hash_one_gadget(cs, config, &secret_key_var)` at line 192. This is the in-circuit equivalent of `Poseidon(sk)`, matching Constraint 1 in SEP-XXXX Section 3.3.

3. **Constraint 2 -- Merkle membership (C-8).** The loop at lines 205--220 iterates over `self.depth` levels. At each level, `FpVar::conditionally_select()` determines the ordering of current and sibling based on the index bit, then `poseidon_hash_two_gadget()` computes the parent. After the loop, `current.enforce_equal(&poseidon_root_var)` (line 223) asserts the computed root matches the witness root.

4. **Constraint 3 -- Commitment binding (C-8).** Lines 230--245 compute `Poseidon(Poseidon(root, epoch), salt)` using two `poseidon_hash_two_gadget()` calls and assert equality with the public `commitment_var`. This is the Variant B commitment binding from SEP-XXXX Section 3.3.

5. **In-circuit Poseidon gadgets.** The `poseidon_hash_one_gadget()` (line 256) and `poseidon_hash_two_gadget()` (line 271) functions use arkworks' `PoseidonSpongeVar` constraint sponge, ensuring the same Poseidon permutation is enforced in-circuit as out-of-circuit.

**Test coverage.** 18 tests covering: circuit satisfiability with valid witnesses, constraint count per tier (1910, 2630, 3350), rejection of wrong secret key, wrong Merkle path, wrong salt, wrong epoch, wrong commitment, public input count (exactly 2), and empty circuit creation for setup.

### 3.5 Prover/Verifier Pipeline

**Source.** `src/prover/mod.rs` (607 lines)

**What it does.** Provides the end-to-end workflow: trusted setup (`setup()`), proof generation (`prove()`), proof verification (`verify()`), and proof serialization/deserialization.

**Correctness argument.**

1. **Setup.** The `setup()` function (line 57) creates an empty circuit via `MembershipCircuit::empty(depth)` and passes it to `Groth16::circuit_specific_setup()`. This produces proving and verification keys tied to the specific circuit depth.

2. **Proof generation pipeline.** The `prove()` function (line 70) follows the complete pipeline:
   - Canonicalizes members via `canonicalize_members()` (line 76)
   - Computes the prover's public key bytes and leaf hash (lines 77--78)
   - Locates the prover in the sorted roster (line 79)
   - Validates the roster's leaf hash matches the prover's secret key (line 84)
   - Builds the Merkle tree from canonicalized members (line 89)
   - Generates an opening proof (line 91)
   - Computes the Poseidon commitment (line 94)
   - Assembles the `MembershipCircuit` with all witness values (line 96)
   - Calls `Groth16::prove()` (line 107)

3. **Proof format (C-9).** `proof_to_bytes()` (line 134) serializes via `serialize_compressed()`, producing 48 + 96 + 48 = 192 bytes. `proof_to_uncompressed_components()` (line 152) serializes each component uncompressed: 96 (G1) + 192 (G2) + 96 (G1) = 384 bytes, matching the Soroban contract's expected format.

4. **Verification.** The `verify()` function (line 118) passes `[commitment, epoch]` as public inputs to `Groth16::verify_with_processed_vk()`, which checks the Groth16 pairing equation internally.

**Test coverage.** 14 tests covering: setup/prove/verify roundtrip, proof serialization roundtrip (192 bytes), uncompressed component sizes (96/192/96), verification failure with wrong inputs, proof determinism (same inputs and RNG seed produce same proof), and multi-member group proofs.

### 3.6 FFI Bridge -- C

**Source.** `src/ffi.rs` (1235 lines)

**What it does.** Provides C-callable functions for iOS integration. Each function follows the pattern: read input buffers, deserialize to Rust types, call core library, serialize output, write to output buffer. Errors are communicated via a `*mut c_char` out-parameter.

**Correctness argument.**

1. **Panic safety.** Every exported function is wrapped in `run_ffi()` (line 34), which catches panics via `std::panic::catch_unwind()` and converts them to C error strings. Unwinding across the FFI boundary is undefined behavior; this wrapper prevents it.

2. **Canonicality enforcement (C-13).** The `read_fr()` function (line 103) calls `bytes_be_to_field_checked()`, which rejects non-canonical field element encodings (values >= the BLS12-381 scalar field modulus). This prevents two different byte representations from mapping to the same field element.

3. **Secret key handling.** The `read_fr_secret_key()` function (line 117) uses `Fr::from_be_bytes_mod_order()` without the canonical check. This is correct: secret keys are private witnesses where any valid scalar works, and rejecting ~50% of random 32-byte keys (those >= the modulus) would cause unnecessary failures.

4. **Buffer management.** `buffer_from_vec()` (line 68) transfers ownership via `mem::forget()`, and a corresponding `sep_free_buffer()` function (exported) reclaims the memory. Length validation via `require_len()` (line 88) occurs before any deserialization.

**Test coverage.** 34 tests covering: all exported functions (hash, build tree, compute commitment, prove, verify, key generation), null pointer handling, wrong-length inputs, non-canonical field element rejection, panic recovery, and memory lifecycle (allocate/free).

### 3.7 FFI Bridge -- JNI

**Source.** `src/jni_ffi.rs` (418 lines)

**What it does.** Provides JNI-callable functions for Android integration. Each function follows the pattern: read `JByteArray` inputs, deserialize, call core library, serialize output, return `jbyteArray`. Errors are communicated by throwing `java.lang.RuntimeException`.

**Correctness argument.**

1. **Panic safety.** Every exported function is wrapped in `run_jni()` (line 39), which catches panics via `std::panic::catch_unwind()` and converts them to Java exceptions. Unwinding across the JNI boundary is undefined behavior; this wrapper prevents it.

2. **Canonicality enforcement (C-13).** The JNI bridge uses the same `bytes_be_to_field_checked()` function as the C FFI for public inputs. The `get_bytes()` helper (line 59) handles JNI byte array conversion failures gracefully.

3. **Structural equivalence.** The JNI functions call the same core library functions as the C FFI. The only difference is the marshalling layer (JNI byte arrays vs. raw pointers). This structural sharing ensures INV-4, INV-5, and INV-6.

**Test coverage.** Android instrumented tests (~50 tests) cover the same functional surface as the C FFI tests, exercising the JNI layer on-device.

### 3.8 Smart Contract

**Source.** `contracts/sep-xxxx/src/lib.rs` (1953 lines)

**What it does.** Soroban smart contract that stores group commitments and verifies Groth16 membership proofs on-chain using BLS12-381 host functions.

**Correctness argument.**

1. **Epoch enforcement (C-10).** In `update_commitment()` (line 479), `checked_add(1)` computes the expected epoch with overflow protection. The strict equality check `new_epoch != expected_epoch` rejects any value other than `stored_epoch + 1`. This implements SEP-XXXX Section 4 invariant: "new_epoch == stored_epoch + 1".

2. **Public inputs match (C-10).** Lines 483--486 verify that the caller-supplied public inputs match the on-chain state: `public_inputs.commitment == current.commitment` and `public_inputs.epoch == current.epoch`.

3. **Proof replay prevention (C-11).** `proof_hash()` (line 716) computes `SHA-256(piA || piB || piC)` where components are in uncompressed format (384 bytes total: π_A 96 + π_B 192 + π_C 96). `check_proof_replay()` (line 726) queries persistent storage for the hash. `record_proof()` (line 739) stores the hash after successful verification. This applies to all four state-changing functions that consume a proof: `create_group`, `update_commitment`, `deactivate_group`, and `consume_membership_proof`. The read-only `verify_membership` deliberately does not burn a nullifier.

4. **Groth16 verification (C-12).** The `verify_groth16_proof()` function (line 780) implements the pairing equation:
   - Computes `vk_x = IC[0] + commitment_fr * IC[1] + epoch_fr * IC[2]` via `g1_msm()` and `g1_add()` (lines 812--815)
   - Negates `proof_a` (line 817)
   - Calls `bls.pairing_check([-piA, alpha, vk_x, piC], [piB, beta, gamma, delta])` (line 822)
   - This checks `e(-piA, piB) * e(alpha, beta) * e(vk_x, gamma) * e(piC, delta) = 1_GT`, matching Proof of Soundness Appendix B.

5. **Canonical input validation (C-13).** Lines 802--808 perform the roundtrip check: `Fr::from_bytes(commitment)` followed by `to_bytes()`, rejecting if the result differs from the input. This prevents non-canonical encodings from being accepted.

6. **Epoch encoding.** `u64_to_u256_be()` (line 825) pads a u64 to 32 bytes (big-endian, zero-extended). Since all u64 values are less than the BLS12-381 scalar field modulus (~2^255), the conversion is always canonical.

7. **Deactivation permanence.** `deactivate_group()` sets `active = false`, and `update_commitment()` checks `current.active` before proceeding. This enforces irreversible deactivation per SEP-XXXX Section 4.

8. **History management.** `archive_entry()` (line 751) maintains a rolling window of `HISTORY_WINDOW = 64` entries, pruning older entries on each update.

**Test coverage.** 56 tests covering: initialization (single and double-init rejection), group creation, epoch transitions (valid and invalid), public input mismatches, proof replay rejection, deactivation (and post-deactivation rejection), tier limit enforcement, VK structure validation, history window behavior, TTL bumping, and mock proof lifecycle.

### 3.9 Relayer

**Source.** `relayer/src/` (4 files: `main.rs`, `config.rs`, `handler.rs`, `validation.rs`)

**What it does.** HTTP service that accepts proof submissions from mobile clients and submits them to the Stellar network via the `stellar` CLI. The relayer pays transaction fees, decoupling the prover's Stellar identity from the on-chain transaction.

**Correctness argument.**

1. **Transparency (INV-7).** The relayer validates the request structure but does not modify the proof bytes, public inputs, or group identifier. The `handle_invoke()` handler (line 43 of `handler.rs`) passes the payload to `stellar contract invoke` verbatim.

2. **Contract whitelist.** `validate_request()` (line 33 of `validation.rs`) rejects requests targeting any contract other than the configured `RELAYER_CONTRACT_ID`. This prevents the relayer from being used as a general-purpose transaction submitter.

3. **Function whitelist.** Only the SEP-XXXX contract functions enumerated in `ALLOWED_FUNCTIONS` (validation.rs:9) are accepted: `create_group`, `create_group_v2`, `create_oligarchy_group`, `update_commitment`, `verify_membership`, `consume_membership_proof`, `deactivate_group`, `get_state`, `get_state_v2`, `get_admin_root`, `get_history`. The allowlist is exhaustive and prevents the relayer from acting as a general-purpose transaction submitter.

4. **Read-only detection.** Read-only functions (`verify_membership`, `get_state`, `get_state_v2`, `get_admin_root`, `get_history`) are submitted with `--send no` (see `READ_ONLY_FUNCTIONS` in validation.rs), preventing unnecessary on-chain state changes. `consume_membership_proof` is deliberately NOT read-only: it is the state-changing twin of `verify_membership` that burns a nullifier, and must be submitted as a real transaction.

5. **Rate limiting.** Per-IP rate limiting prevents denial-of-service against the relayer's fee-paying account.

**Test coverage.** 31 tests covering: request validation (contract whitelist, function whitelist, proof format), rate limiting, configuration parsing, read-only detection, and error response formatting.

### 3.10 PN-Relay

**Source.** `pn-relay/src/` (9 files: `main.rs`, `config.rs`, `handler.rs`, `crypto.rs`, `db.rs`, `nostr_watcher.rs`, `apns.rs`, `fcm.rs`, `unified_push.rs`)

**What it does.** Push notification relay that watches Nostr relay events and delivers platform-native notifications (APNs for iOS, FCM/UnifiedPush for Android) to subscribed devices.

**Correctness argument.**

1. **Confidentiality (INV-8).** The PN-Relay observes only Nostr event topic tags for routing. The `nostr_watcher.rs` module subscribes to events by tag filter, but encrypted event content (NIP-44 encrypted payloads) is opaque to the relay. Push notifications contain only a wake signal, not message content.

2. **Subscription encryption.** The `handle_subscription()` handler (line 49 of `handler.rs`) receives encrypted subscription requests. The `crypto.rs` module (line 13) implements X25519 key agreement with HKDF-SHA256 key derivation and AES-256-GCM encryption. Device tokens and notification keys are encrypted at rest using a storage key.

3. **Multi-platform delivery.** Separate modules (`apns.rs`, `fcm.rs`, `unified_push.rs`) implement platform-specific notification delivery, with the handler routing based on the subscription's platform field.

4. **Storage encryption.** The `load_or_create_storage_key()` function (line 48 of `crypto.rs`) generates or loads a 32-byte key used to encrypt sensitive data (device tokens, notification keys) in the SQLite database.

**Test coverage.** 42 tests covering: subscription lifecycle (create, update, delete), encrypted request handling, Nostr event filtering, notification routing, APNs/FCM payload formatting, rate limiting, storage encryption roundtrip, key management, and error handling.

### 3.11 iOS Client

**Source.** Swift package at `swift-mls/` with tests at `swift-mls/Tests/SwiftMLSTests/`.

**What it does.** Provides the iOS-facing API for group membership operations. Calls into the Rust core library via the C FFI bridge.

**Correctness argument.**

1. **FFI binding.** The Swift wrapper calls the C functions exported by `src/ffi.rs`. Input/output marshalling converts between Swift `Data` and C buffer pointers. Memory is managed via the `sep_free_buffer()` function.

2. **Key management.** BLS12-381 key generation, Poseidon leaf hash computation, and Merkle tree operations all delegate to the Rust implementation, ensuring identical cryptographic behavior to the core library.

3. **Determinism (C-14).** The cross-platform test vectors (`docs/cross-platform-test-vectors.json`) define expected outputs for specific inputs. The iOS test suite verifies that Swift wrapper outputs match these vectors.

**Test coverage.** ~28 tests covering: key generation, leaf hash computation, Merkle tree construction, commitment computation, proof generation and verification, and cross-platform vector validation.

### 3.12 Android Client

**Source.** Kotlin code at `clients/android/` with tests at `clients/android/StellarChat/app/src/androidTest/`.

**What it does.** Provides the Android-facing API for group membership operations. Calls into the Rust core library via the JNI bridge.

**Correctness argument.**

1. **JNI binding.** The Kotlin wrapper calls the JNI functions exported by `src/jni_ffi.rs`. Input/output marshalling converts between Kotlin `ByteArray` and JNI byte arrays. Errors are propagated as `RuntimeException`.

2. **Key management.** As with iOS, all cryptographic operations delegate to the Rust implementation, ensuring identical behavior.

3. **Determinism (C-14).** The same cross-platform test vectors used for iOS are also validated in the Android test suite.

**Test coverage.** ~50 tests covering: key generation, leaf hash computation, Merkle tree construction, commitment computation, proof generation and verification, cross-platform vector validation, error handling, and Schnorr signature operations.

---

## 4. Cross-Platform Consistency Proof

### 4.1 Rust to iOS FFI Correctness

The iOS integration uses C-linkage FFI. The correctness chain is:

1. **Shared implementation.** The C FFI functions in `src/ffi.rs` call the same core library functions as Rust-native callers. There is no platform-specific cryptographic code.

2. **Serialization format.** All values cross the FFI boundary as byte arrays with defined endianness (big-endian for field elements, as specified by `field_to_bytes_be()` and `bytes_be_to_field_checked()`). The Swift side performs no mathematical operations on these bytes -- it passes them opaquely to and from the Rust layer.

3. **Canonicality at boundary (C-13).** Every public field element received from Swift is validated via `read_fr()` -> `bytes_be_to_field_checked()`. Non-canonical values are rejected before reaching the core library.

4. **Test evidence.** The cross-platform test vectors (`docs/cross-platform-test-vectors.json`) provide known-answer tests that are validated independently on each platform. If the Rust library and the iOS app both produce the expected output for the same input, they agree.

### 4.2 Rust to Android JNI Correctness

The Android integration uses JNI. The correctness chain mirrors Section 4.1:

1. **Shared implementation.** The JNI functions in `src/jni_ffi.rs` call the same core library functions.

2. **Serialization format.** JNI byte arrays use the same big-endian convention as the C FFI.

3. **Canonicality at boundary (C-13).** The same `bytes_be_to_field_checked()` function validates inputs from the JNI layer.

4. **Panic safety.** Both the C FFI (`run_ffi()`) and JNI (`run_jni()`) catch Rust panics before they cross the language boundary.

### 4.3 Cross-Platform Determinism

**Claim (C-14).** For identical inputs (secret keys, member rosters, salts, epochs), the following outputs are byte-identical across all three platforms (Rust, iOS/C FFI, Android/JNI):

- Poseidon leaf hashes
- Merkle roots
- Poseidon commitments (Variant B)
- SHA-256 commitments (Variant A)
- Groth16 proofs (given the same RNG seed)
- Proof serializations (192-byte and 384-byte formats)

**Argument.** All three platforms execute the same compiled Rust code (as a static library on iOS, as a shared library on Android). The FFI/JNI layers perform only serialization/deserialization with no mathematical operations. The canonicality checks at the boundaries ensure that the same logical value is always represented by the same byte sequence.

The two cross-platform tests in the test suite (`tests/cross_platform_tests.rs`) validate this claim for a representative set of inputs. The cross-platform test vectors file provides additional known-answer tests for independent validation.

---

## 5. Protocol Conformance Map

### 5.1 SEP-XXXX Section to Code Mapping

| SEP Section | Requirement | Implementation File | Key Lines | Properties |
|-------------|-------------|-------------------|-----------|------------|
| 1.1 | BLS12-381 G1 member identity keys | `src/merkle/mod.rs` | 192--200 | C-4 |
| 2.1 | Member ordering by compressed G1 | `src/merkle/mod.rs` | 204--217 | C-4 |
| 2.2 | Poseidon parameters (w3, f8, p56, a5) | `src/poseidon/mod.rs` | 17--29, 49--73 | C-1 |
| 2.2 | Poseidon round constant derivation | `src/poseidon/mod.rs` | 102--123 | C-1 |
| 2.2 | Poseidon MDS matrix (Cauchy) | `src/poseidon/mod.rs` | 128--145 | C-1 |
| 2.2 | Leaf = Poseidon(sk) | `src/merkle/mod.rs` | via `poseidon_hash_one` | C-3 |
| 2.2 | Binary Merkle tree, zero-padded | `src/merkle/mod.rs` | 96--127 | C-5 |
| 2.3 | Variant A: SHA-256(root \|\| epoch \|\| salt) | `src/commitment/mod.rs` | 55--65 | C-7 |
| 2.3 | Variant B: Poseidon(Poseidon(root, epoch), salt) | `src/commitment/mod.rs` | 156--170 | C-6 |
| 3.3 | Constraint 1: key ownership | `src/circuit/mod.rs` | 184--196 | C-8 |
| 3.3 | Constraint 2: Merkle membership | `src/circuit/mod.rs` | 198--223 | C-8 |
| 3.3 | Constraint 3: commitment binding | `src/circuit/mod.rs` | 225--245 | C-8 |
| 3.4 | 2 public inputs (commitment, epoch) | `src/circuit/mod.rs` | 125--136 | C-8 |
| 3.4 | Proof format: 192 bytes compressed | `src/prover/mod.rs` | 132--139 | C-9 |
| 3.5 | Groth16 pairing equation | `contracts/sep-xxxx/src/lib.rs` | 780--823 | C-12 |
| 4 | create_group (epoch = 0) | `contracts/sep-xxxx/src/lib.rs` | 370--445 | C-10 |
| 4 | update_commitment (epoch + 1) | `contracts/sep-xxxx/src/lib.rs` | 459--500 | C-10 |
| 4 | deactivate_group | `contracts/sep-xxxx/src/lib.rs` | 530--600 | C-10 |
| 4 | Proof replay hardening | `contracts/sep-xxxx/src/lib.rs` | 715--749 | C-11 |
| 5.1 | Relayer pattern (fee decoupling) | `relayer/src/handler.rs` | 43--end | INV-7 |
| B (Appendix) | Canonical input validation | `contracts/sep-xxxx/src/lib.rs` | 802--808 | C-13 |
| B (Appendix) | vk_x = IC[0] + C*IC[1] + e*IC[2] | `contracts/sep-xxxx/src/lib.rs` | 812--815 | C-12 |

### 5.2 Proof-of-Soundness Theorem to Implementation Mapping

Each theorem in the Proof of Soundness depends on the code correctly implementing specific constructions. This table maps each theorem to the code that realizes its assumptions.

| Theorem | Statement | Implementation Dependency | Code Location | Properties Verified |
|---------|-----------|--------------------------|---------------|-------------------|
| Theorem 1 | Merkle tree collision resistance | Poseidon hash with correct parameters; fixed-depth binary tree with zero padding | `src/poseidon/mod.rs:49--73`, `src/merkle/mod.rs:96--127` | C-1, C-5 |
| Theorem 2 | Commitment hiding | Two-layer Poseidon commitment with fresh random salt; salt interpreted via `from_le_bytes_mod_order` | `src/commitment/mod.rs:156--170` | C-6 |
| Theorem 3 | ZK membership soundness | Circuit constraints correctly encode key ownership, Merkle membership, and commitment binding | `src/circuit/mod.rs:184--245` | C-8 |
| Theorem 4 | Zero-knowledge property | Groth16 proof generation using arkworks; proof reveals only public inputs | `src/prover/mod.rs:70--114` | C-8, C-9 |
| Theorem 5 | Epoch monotonicity | Contract enforces `new_epoch == stored_epoch + 1` with `checked_add` | `contracts/sep-xxxx/src/lib.rs:479--482` | C-10 |
| Theorem 6 | Proof non-replayability | SHA-256 hash of uncompressed proof stored in persistent storage; checked before every state-changing call | `contracts/sep-xxxx/src/lib.rs:715--749` | C-11 |
| Theorem 7 | Fee-payer privacy | Relayer submits transactions; contract does not inspect caller identity; proof-only authorization | `relayer/src/handler.rs`, `contracts/sep-xxxx/src/lib.rs:459--466` | INV-7 |
| Theorem 8 | Symmetric removal impossibility | Not an implementation property; theoretical result about protocol design | N/A (design document) | N/A |

---

## 6. Test Coverage Analysis

### 6.1 Module Coverage Matrix

| Module | File | Tests | C-Properties Covered | Notes |
|--------|------|-------|---------------------|-------|
| Poseidon | `src/poseidon/mod.rs` | 11 | C-1, C-2 | Includes determinism, parameter, and MDS tests |
| Merkle | `src/merkle/mod.rs` | 20 | C-3, C-4, C-5 | Includes canonical ordering and duplicate rejection |
| Commitment | `src/commitment/mod.rs` | 19 | C-6, C-7 | Both variants tested; canonicality roundtrip |
| Circuit | `src/circuit/mod.rs` | 18 | C-8 | Constraint count per tier; wrong-witness rejection |
| Prover | `src/prover/mod.rs` | 14 | C-9 | Full prove/verify cycle; serialization format |
| Ceremony | `src/ceremony/` | 20 | (setup) | MPC contribution and verification |
| Cross-platform | `tests/` | 2 | C-14 | Known-answer vectors |
| FFI (C) | `tests/ffi_tests.rs` | 34 | C-13, C-14 | Panic safety, canonical rejection, null pointers |
| Contract | `contracts/sep-xxxx/src/lib.rs` | 56 | C-10, C-11, C-12, C-13 | Epoch, replay, pairing, canonical checks |
| Relayer | `relayer/src/` | 31 | INV-7 | Transparency, whitelisting, rate limiting |
| PN-Relay | `pn-relay/src/` | 42 | INV-8 | Confidentiality, encryption, routing |
| iOS | `swift-mls/Tests/` | ~28 | C-14 | FFI integration, cross-platform vectors |
| Android | `clients/android/` | ~50 | C-14 | JNI integration, cross-platform vectors |
| **Total** | | **~345** | | |

### 6.2 Invariant Coverage Matrix

| Invariant | Tested By | Test Type | Evidence |
|-----------|----------|-----------|----------|
| INV-1 (Commitment pipeline consistency) | Prover module tests | Unit | `prove()` computes commitment identically to `compute_poseidon_commitment()` and the circuit verifies it |
| INV-2 (Proof decompression format) | Prover + FFI tests | Unit + Integration | `proof_to_uncompressed_components()` produces 96/192/96 bytes matching contract expectations |
| INV-3 (Epoch chain monotonicity) | Contract tests | Unit | Tests verify epoch+1 accepted, epoch+0 rejected, epoch+2 rejected, epoch overflow rejected |
| INV-4 (Key consistency) | Cross-platform tests, FFI tests | Integration | Same secret key bytes produce same field element on all platforms |
| INV-5 (Leaf consistency) | Cross-platform tests | Integration | Same `sk` produces same `Poseidon(sk)` via C FFI and JNI |
| INV-6 (Root consistency) | Cross-platform tests | Integration | Same member set produces same Merkle root via C FFI and JNI |
| INV-7 (Relayer transparency) | Relayer tests | Unit | Payload passed verbatim to `stellar contract invoke` |
| INV-8 (PN-Relay confidentiality) | PN-Relay tests | Unit | Encrypted events are not decrypted; only topic tags inspected |

### 6.3 Gap Analysis

The following areas have limited or indirect test coverage:

| Area | Current Coverage | Risk | Mitigation |
|------|-----------------|------|------------|
| End-to-end on-chain proof verification | No live testnet tests in CI | Medium | Contract tests use mock BLS12-381 host functions; manual testnet deployment tests documented in `docs/testnet-deployment.md` |
| Concurrent epoch transitions | Not tested (single-threaded contract tests) | Low | Stellar's sequential transaction ordering within a ledger prevents true concurrency; the contract's state read-then-write pattern is atomic within a single invocation |
| Very large member sets (2048 members, tier Large) | Tested at small scale; large-scale performance not regression-tested | Low | The Merkle tree construction is `O(n log n)` and the circuit is depth-parameterized; no algorithmic difference between tiers |
| Network-layer privacy of relayer communication | Out of scope for code-level testing | Medium | Documented in Proof of Soundness Theorem 7 as a residual risk; mitigated by Tor/VPN recommendations |
| Soroban host function fidelity | Assumed correct (opaque BLS12-381 host functions) | Low | Soroban host functions are part of the Stellar protocol and undergo independent validation |
| Non-canonical G1/G2 point rejection | Implicitly tested via proof verification failures | Low | The `G1Affine::from_bytes()` and `G2Affine::from_bytes()` host functions perform subgroup checks |

---

## 7. Known Limitations

1. **No formal verification.** This document provides a structured argument, not a machine-checked proof. The correctness claims depend on manual code inspection and empirical testing.

2. **Trusted setup simulation.** The reference implementation's key derivation (Section 8.2.3 of the SEP) uses a simulation approach where the ceremony scalars are hashed into a CSPRNG seed for arkworks' `circuit_specific_setup`. The 1-of-N trust guarantee of the ceremony does not carry through to the derived keys. Production deployments require Phase 2 MPC key derivation.

3. **arkworks dependency.** The Poseidon hash, Groth16 prover/verifier, and BLS12-381 arithmetic are provided by the arkworks library suite. This document does not prove the correctness of arkworks itself; it assumes arkworks correctly implements the algorithms described in the referenced papers.

4. **Soroban host function opacity.** The contract's Groth16 verification depends on Soroban's `bls12_381_g1_msm`, `bls12_381_pairing_check`, and related host functions. These are opaque from the contract's perspective; their correctness is an assumption of the Stellar protocol.

5. **Cross-platform test vector coverage.** The cross-platform test vectors (`docs/cross-platform-test-vectors.json`) cover a representative but not exhaustive set of inputs. Edge cases (e.g., field elements near the modulus boundary, trees at exactly 2^d members) are tested in module-specific tests but not all are represented in the cross-platform vectors.

6. **Post-compromise forward secrecy.** As established by Theorem 8 of the Proof of Soundness, the current symmetric protocol cannot cryptographically evict removed members. This is a protocol design limitation, not an implementation bug.

7. **Timing side channels.** The commitment comparison uses constant-time byte comparison (`constant_time_eq` in `src/commitment/mod.rs:27`). However, the broader Poseidon hash and Groth16 proving operations use arkworks' standard (non-constant-time) field arithmetic. In the protocol's threat model, timing side channels on the prover are not exploitable because the prover runs locally on the member's device, but this should be documented for any server-side deployment.
