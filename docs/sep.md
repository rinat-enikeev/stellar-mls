## Preamble

```
SEP: XXXX
Title: Private Group Membership Registry with Zero-Knowledge Proof
Author: @rinat-enikeev
Track: Standard
Status: Draft
Created: 2026-03-30
Updated: 2026-04-17
Version: 0.0.6
Discussion: https://github.com/orgs/stellar/discussions/1903
```

---

## Simple Summary

A standard for anchoring cryptographic group membership state on Stellar using a commitment scheme and a zero-knowledge membership proof, such that no observer of the ledger can determine who is in a group or who is updating its state.

---

## Dependencies

- Stellar Protocol 22 — BLS12-381 host functions in Soroban (`bls12_381_g1_add`, `bls12_381_g2_msm`, `bls12_381_pairing`)
- Soroban persistent storage and `env.crypto().sha256()` host function

---

## Motivation

On-chain group registries can store membership lists in plaintext: `group_id → [address_A, address_B, address_C]`. This permanently exposes the social graph of every group to any Stellar network participant, now and retroactively.

After a contract implementing this standard is deployed:

- The on-chain record for a group reveals only a 32-byte commitment, an epoch counter, and a zero-knowledge proof. No member identity is stored or derivable.
- The party submitting an update proves they are a current group member without revealing which member they are. Their Stellar signing address is structurally decoupled from their group identity.

### Target use cases

- Decentralized end-to-end encrypted group messaging based on MLS (RFC 9420)
- Anonymous DAO membership registries and voting rolls
- Private multi-party credential schemes
- Any application requiring verifiable group membership without public roster disclosure

---

## Specification

### 1. Definitions

| Term | Meaning |
|------|---------|
| `group_id` | A 32-byte opaque identifier for a group. Recommended derivation: `SHA-256(application_namespace \|\| creator_pubkey \|\| nonce)`. |
| `members` | The set of member identity public keys for a given epoch. Each key is a BLS12-381 G1 point (48 bytes compressed). See Section 1.1. |
| `epoch` | A `u64` counter, starting at 0, incremented by exactly 1 on every membership change. |
| `salt` | 32 uniformly random bytes, generated fresh for each epoch by the Commit initiator. Never stored on-chain. |
| `commitment` | `SHA-256(poseidon_root \|\| epoch_be \|\| salt)` — a 32-byte cryptographic binding to the group state. See Section 2. |
| `poseidon_root` | The root of a binary Poseidon Merkle tree over the sorted member key set. 32 bytes (BLS12-381 scalar field element). |
| `zk_proof` | A Groth16 proof over BLS12-381 demonstrating that the prover holds a private key belonging to a committed member, without revealing which member. |

#### 1.1 Member Identity Keys

Member identity keys are BLS12-381 G1 keypairs: the private key is a scalar `sk` in the BLS12-381 scalar field, and the public key is `pk = sk · G1` where `G1` is the standard generator.

**Rationale.** Ed25519 key ownership verification inside a BLS12-381 Groth16 circuit requires non-native field arithmetic emulation (Curve25519 operates over a different prime field), resulting in 500,000+ R1CS constraints for a single scalar multiplication. BLS12-381-native keys reduce key ownership to a single native-field scalar-mul — roughly 1,000 constraints.

**Stellar address binding.** Applications that require a link between a member's BLS12-381 identity key and their Stellar Ed25519 address MUST use an off-chain key attestation: the member signs their BLS12-381 public key with their Ed25519 private key and distributes this attestation to the group via the application's encrypted channel. The attestation is verified by other members at registration time, not inside the ZK circuit. This keeps the circuit small without sacrificing the ability to trace a BLS12-381 key back to a Stellar account when the holder consents.

The attestation format is:

```
KeyAttestation {
    bls_pubkey:     BytesN<48>    -- compressed G1 point
    ed25519_pubkey: BytesN<32>    -- Stellar account public key
    signature:      BytesN<64>    -- Ed25519 signature over SHA-256("SEP-XXXX:key-binding" || bls_pubkey)
}
```

This attestation is a group-level artifact shared among members. It is never submitted on-chain.

---

### 2. Commitment Construction (Dual-Hash Scheme)

The commitment scheme uses two hash functions for different roles:

- **Poseidon** (ZK-friendly, ~300 constraints per hash): used inside the ZK circuit for the Merkle tree over members and for binding the tree root to the epoch and salt.
- **SHA-256** (Soroban-native host function): used as an outer binding that the contract checks on-chain.

This separation gives the circuit a small constraint count while preserving the ability to verify commitments on-chain using a native host function.

#### 2.1 Member ordering

Before building the Merkle tree, members MUST be sorted in ascending lexicographic byte order of their compressed G1 representation (48 bytes). Membership is a set — the commitment must be deterministic regardless of insertion order.

#### 2.2 Poseidon Merkle tree

The sorted member keys are placed as leaves of a binary Merkle tree of fixed depth `d`, where `d` is determined by the circuit tier (see Section 9). Unused leaf slots are filled with a distinguished zero value `0x00...00` (the field element zero, 32 bytes).

The hash function for both leaf hashing and internal nodes is Poseidon over the BLS12-381 scalar field with the following parameters:

```
Poseidon parameters:
    field:          BLS12-381 scalar field (r ≈ 2^255)
    arity:          2 (binary tree)
    full rounds:    8
    partial rounds: 56
    S-box:          x^5
    width:          3 (rate 2 + capacity 1)
```

**Parameter generation.** The reference implementation generates round constants deterministically by repeatedly hashing the seed `"SEP-XXXX-Poseidon-BLS12-381-w3-f8-p56-a5-round-constants"` with SHA-256, extending each iteration to 64 bytes for uniform field element sampling via `from_le_bytes_mod_order`. The seed includes all Poseidon parameters (width 3, full rounds 8, partial rounds 56, alpha 5) for domain separation. The MDS matrix uses the Cauchy construction `M[i][j] = 1 / (x_i + y_j)` with `x_i = i+1` and `y_j = width+j+1`. In production, implementations SHOULD use the Poseidon paper's reference generation script for BLS12-381 to ensure cross-implementation compatibility.

Each leaf is computed as:

```
leaf[i] = Poseidon(sk[i])
```

where `sk[i]` is the member's BLS12-381 private key scalar. Members compute `Poseidon(sk)` locally and share only the resulting leaf hash during group registration — the secret key itself is never transmitted.

Internal nodes are:

```
node = Poseidon(left_child, right_child)
```

The tree root is `poseidon_root`.

#### 2.3 On-chain commitment value

Two commitment binding variants are defined. Implementations MUST use one consistently.

**Variant A — SHA-256 outer binding (original)**

```
commitment = SHA-256(poseidon_root || epoch || salt)
```

where:

```
poseidon_root   -- 32 bytes, big-endian encoding of the field element
epoch           -- u64, big-endian, 8 bytes
salt            -- 32 bytes
```

Total preimage: 72 bytes. No padding, no separators, no length prefix (the structure is fixed-length).

SHA-256 is chosen for the outer binding because it is available as a Soroban host function (`env.crypto().sha256()`), making in-contract verification gas-efficient. However, in-circuit SHA-256 verification costs ~25,000 R1CS constraints, which dominates total circuit size at all tiers.

**Variant B — Poseidon-only binding (reference implementation)**

```
commitment = Poseidon(Poseidon(poseidon_root, epoch), salt)
```

where `epoch` is the epoch value cast to a field element, and `salt` is the 32-byte salt interpreted as a BLS12-381 scalar field element via little-endian `mod r` reduction.

This variant eliminates in-circuit SHA-256 entirely, replacing it with two Poseidon hash calls (~600 R1CS constraints total). The resulting circuit is ~14x smaller than Variant A. The tradeoff is that on-chain commitment verification requires a Poseidon host function or off-chain precomputation. The reference implementation uses Variant B.

Note on salt encoding in Variant B: the 32-byte salt is reduced modulo the BLS12-381 scalar field order (`r ≈ 2^255`). Since `r < 2^256`, approximately half of uniformly random 32-byte salts will be reduced, mapping two distinct byte strings to the same field element. This reduces effective salt entropy from 256 bits to ~255 bits, which remains cryptographically sufficient.

#### 2.4 Salt lifecycle

The salt is a group secret shared exclusively among current members via the application's encrypted channel. It is never published on-chain. A fresh 32-byte random salt MUST be generated by the Commit initiator for every epoch transition, ensuring that knowledge of salt `n` grants no information about commitment `n+1` or any prior epoch.

#### 2.5 Salt recovery

If a member loses the salt for the current epoch, they cannot generate proofs or verify the commitment locally. The recovery procedure is:

1. The member requests salt re-delivery from any other current group member via the application's encrypted channel.
2. The responding member encrypts the salt to the requesting member's BLS12-381 public key (using ECIES over BLS12-381 G1) and delivers it.
3. Upon receiving the salt, the recovering member MUST verify locally using the commitment variant in use:

```
-- Variant A:
local_commitment = SHA-256(poseidon_root || epoch || salt)
-- Variant B:
local_commitment = Poseidon(Poseidon(poseidon_root, epoch), salt)

assert local_commitment == contract.get_state(group_id).commitment
```

If no other member is reachable, the member cannot participate until the next epoch transition, at which point a new salt is distributed. Applications SHOULD design their salt distribution to be robust against single points of failure (e.g., every member stores the current salt and can re-share it).

---

### 3. Zero-Knowledge Membership Proof

#### 3.1 Proof statement

The prover demonstrates the following statement to the contract without revealing any witness:

> "I know a secret key `sk` such that `Poseidon(sk)` is a leaf in the Poseidon Merkle tree whose root, combined with the epoch and salt, produces the stored commitment for `(group_id, epoch)`."

Formal relation:

**Variant A (SHA-256 outer binding):**

```
R = {
  public:   commitment ∈ {0,1}^256,  epoch ∈ u64
  witness:  sk, poseidon_root, salt,
            merkle_path[0..d-1], leaf_index

  constraints:
    (1)  leaf = Poseidon(sk)                                          [key ownership]
    (2)  MerklePoseidonOpen(leaf, merkle_path, leaf_index, poseidon_root) == true  [membership]
    (3)  SHA-256(poseidon_root || epoch || salt) == commitment         [commitment binding]
}
```

**Variant B (Poseidon-only binding, reference implementation):**

```
R = {
  public:   commitment ∈ Fr,  epoch ∈ Fr
  witness:  sk, poseidon_root, salt,
            merkle_path[0..d-1], leaf_index

  constraints:
    (1)  leaf = Poseidon(sk)                                          [key ownership]
    (2)  MerklePoseidonOpen(leaf, merkle_path, leaf_index, poseidon_root) == true  [membership]
    (3)  Poseidon(Poseidon(poseidon_root, epoch), salt) == commitment  [commitment binding]
}
```

#### 3.2 Proof system

**Groth16 SNARK over BLS12-381.**

Rationale:

- Stellar Protocol 22 added native BLS12-381 host functions to Soroban specifically to enable ZK proof verification on-chain
- Groth16 produces constant-size proofs (192 bytes) regardless of group size
- Verification requires exactly 3 pairing operations, expressible directly via `bls12_381_pairing`
- Groth16 is well-audited and widely deployed (Zcash, Ethereum ecosystem)

Groth16 requires a circuit-specific trusted setup. See Section 9.

#### 3.3 Circuit structure

Three constraints are encoded in the circuit:

**Constraint 1 — key ownership (Poseidon preimage)**

The circuit computes `leaf = Poseidon(sk)` where `sk` is the prover's private key scalar (witness). This proves knowledge of the preimage: only someone who knows `sk` can produce a leaf hash that matches. Because Poseidon is a native-field hash, this requires ~240 R1CS constraints.

Assert:

```
Poseidon(sk) == leaf   (witness, used as input to Constraint 2)
```

**Rationale.** The original design specified `pk = sk · G1` (in-circuit BLS12-381 scalar multiplication, ~1,000 constraints) as the key ownership proof. The reference implementation uses Poseidon preimage knowledge instead: the leaf is `Poseidon(sk)` rather than a function of the public key. This approach proves "I know the secret key whose hash is a leaf in the Merkle tree" — which is the same security guarantee (knowledge of the secret key), achieved with fewer constraints and no dependency on in-circuit elliptic curve gadgets. The public key `pk = sk · G1` remains the member's external identity; the Poseidon preimage is an internal circuit mechanism only.

**Constraint 2 — Poseidon Merkle membership**

The prover supplies a Merkle opening proof consisting of `d` sibling hashes and a leaf index. The circuit recomputes the Merkle root from the leaf (computed in Constraint 1 as `Poseidon(sk)`) upward using Poseidon hashes.

Assert:

```
MerklePoseidonOpen(Poseidon(sk), merkle_path, leaf_index) == poseidon_root
```

At each level, Poseidon is applied to the pair `(left, right)` determined by the path bit. For a tree of depth `d`, this requires `d` Poseidon hash evaluations (~240 constraints each as measured, ~300 estimated), totaling ~`240 · d` constraints.

**Constraint 3 — commitment binding**

Two variants correspond to the commitment variants in Section 2.3:

*Variant A (SHA-256, fixed-length):* The circuit recomputes `SHA-256(poseidon_root || epoch || salt)` from the witness values. The preimage is exactly 72 bytes (fixed), requiring exactly 1 SHA-256 compression call (~25,000 R1CS constraints).

Assert:

```
SHA-256(poseidon_root || epoch || salt) == commitment   (public input)
```

*Variant B (Poseidon-only, reference implementation):* The circuit computes `Poseidon(Poseidon(poseidon_root, epoch), salt)` using two Poseidon hash calls (~600 R1CS constraints total). The two public inputs are the Poseidon commitment value and the epoch.

Assert:

```
Poseidon(Poseidon(poseidon_root, epoch), salt) == commitment   (public input)
epoch_witness == epoch                                          (public input)
```

**Total circuit size by tier (Variant A — SHA-256, estimated):**

| Tier | MAX_MEMBERS | Tree depth `d` | Constraint 1 | Constraint 2 | Constraint 3 | Total (approx.) |
|------|-------------|----------------|--------------|--------------|--------------|------------------|
| Small | 32 | 5 | ~240 | ~1,500 | ~25,000 | ~26,740 |
| Medium | 256 | 8 | ~240 | ~2,400 | ~25,000 | ~27,640 |
| Large | 2,048 | 11 | ~240 | ~3,300 | ~25,000 | ~28,540 |

**Total circuit size by tier (Variant B — Poseidon-only, measured from reference implementation):**

| Tier | MAX_MEMBERS | Tree depth `d` | Constraint 1 | Constraint 2 | Constraint 3 | Total (measured) |
|------|-------------|----------------|--------------|--------------|--------------|------------------|
| Small | 32 | 5 | ~240 | ~1,070 | ~600 | **1,910** |
| Medium | 256 | 8 | ~240 | ~1,790 | ~600 | **2,630** |
| Large | 2,048 | 11 | ~240 | ~2,510 | ~600 | **3,350** |

Variant B is ~14x smaller than Variant A. The circuit scales logarithmically with group size in both variants, with ~240 additional constraints per tree level. In Variant A, SHA-256 dominates the constraint count at all tiers. In Variant B, the Merkle proof dominates, followed by the commitment binding and key ownership constraints.

#### 3.4 Public inputs and proof wire

**Variant A (SHA-256 binding):**

```
ProofSubmission {
    group_id:   BytesN<32>  -- identifies the group
    epoch:      u64         -- must match stored epoch
    proof:      Bytes(192)  -- Groth16 proof: π_A (G1) || π_B (G2) || π_C (G1)
    public_inputs: {
        commitment: BytesN<32>   -- must match stored commitment
        epoch:      u64          -- repeated for circuit binding
    }
}
```

**Variant B (Poseidon-only binding, reference implementation):**

```
ProofSubmission {
    group_id:   BytesN<32>  -- identifies the group
    epoch:      u64         -- must match stored epoch
    proof:      Bytes(192)  -- Groth16 proof: π_A (G1) || π_B (G2) || π_C (G1)
    public_inputs: {
        commitment: Fr           -- Poseidon binding value (BLS12-381 scalar field element)
        epoch:      Fr           -- epoch as field element
    }
}
```

In Variant B, the public inputs are two BLS12-381 scalar field elements. The contract must verify that `commitment` is consistent with the stored on-chain state.

#### 3.5 Verification in Soroban

The contract verifies the Groth16 proof using the BLS12-381 host functions introduced in Protocol 22:

```
e(π_A, π_B) == e(α, β) · e(vk_γ^{public_inputs}, γ) · e(π_C, δ)
```

where `(α, β, γ, δ, vk_γ)` are the circuit's verification key, stored in the contract at deployment.

The contract does NOT compute the SHA-256 of the member list during verification — that computation happens inside the ZK circuit. The contract only:

1. Looks up the stored commitment for `(group_id, epoch)`
2. Checks `public_inputs.commitment == stored.commitment`
3. Verifies the Groth16 proof against the verification key

This means the contract never learns the member list, the salt, or the identity of the prover.

#### 3.6 Proof size and cost

| Parameter | Variant A | Variant B |
|-----------|-----------|-----------|
| Proof size | 192 bytes | 192 bytes |
| Public inputs | 40 bytes (32-byte commitment + 8-byte epoch) | 64 bytes (2 × 32-byte field elements) |
| BLS12-381 pairings required | 3 | 3 |
| Estimated Soroban instructions | ~6–10M | ~6–10M |
| Cost scaling with group size | None — constant | None — constant |

The constant verification cost is a core property. A 2,048-member group is indistinguishable from a 2-member group from the contract's perspective.

#### 3.7 Update circuit (`R_Update`)

Sections 3.1–3.6 describe the membership circuit used by `create_group`, `verify_membership`, and `deactivate_group`. State *transitions* (`update_commitment`) require a second circuit, `R_Update`, whose statement binds the new commitment as a *public input* of the proof rather than as an envelope-level parameter. This closes a binding gap where a membership-only proof authorises a caller but does not bind the new state they are permitted to write. See [update-circuit-binding-design.md](update-circuit-binding-design.md) and [vuln-unbound-new-commitment.md](vuln-unbound-new-commitment.md) for the full design and motivation.

`R_Update` takes exactly three public inputs in this fixed order:

```
public_inputs = (C_old, epoch_old, C_new)
```

where `C_old, C_new ∈ Fr` are Poseidon commitments as defined in §2.3 Variant B and `epoch_old ∈ Fr` is the epoch being transitioned *from*. The new epoch is `epoch_old + 1` and is never itself a public input — it is constrained inside the circuit.

The circuit has three constraints:

**Constraint (1) — prover is a member of the old committed state.**

The prover supplies witnesses `sk, root_old, salt_old, merkle_path, leaf_index` and the circuit enforces:

```
Poseidon(sk) == MerkleOpen(leaf_index, merkle_path, root_old)
Poseidon(Poseidon(root_old, epoch_old), salt_old) == C_old      (public)
```

**Constraint (2) — `C_new` is a canonical commitment over `epoch_old + 1`.**

The prover supplies witnesses `root_new, salt_new` and the circuit enforces:

```
Poseidon(Poseidon(root_new, epoch_old + 1), salt_new) == C_new  (public)
```

The `+ 1` is a field-element increment performed inside the circuit; the contract never accepts an attacker-chosen `new_epoch`.

**Constraint (3) — epoch monotonicity.**

Constraint (2) fixes the new-epoch witness to `epoch_old + 1`. The contract additionally compares the on-chain stored epoch against `epoch_old` in `public_inputs`, so any attempt to submit a proof for a different epoch-pair is rejected at state-binding check time without touching the verifier.

Because `C_new` is a public input, the pairing equation rejects any envelope in which `C_new` has been substituted. The attacker-controlled new commitment is therefore cryptographically bound to the proof, not to the transaction envelope.

The update-circuit proof wire format is:

```
UpdatePublicInputs (73 bytes)
    version:    u8         = 0x02
    c_old:      Fr (32B, big-endian)
    epoch_old:  u64 (8B, big-endian)
    c_new:      Fr (32B, big-endian)
```

Parsers MUST reject wrong length, wrong version byte, and any `c_old`/`c_new` that is not a canonical Fr element (i.e., ≥ the BLS12-381 scalar modulus `r`). Canonical test vectors are in [`docs/cross-platform-test-vectors.json`](cross-platform-test-vectors.json) under `update_public_inputs_wire_format`.

The update verification key (`vk_update`) has four `IC` points (one constant + one per public input) and is distinct from the membership verification key for the same tier; a tiered deployment therefore holds *two* verification keys per tier.

---

### 4. Contract Interface

Implementations of this SEP MUST expose the following interface. The normative specification is the interface and its invariants; implementation language is non-normative.

#### `initialize`

Initializes the contract with an admin address and the Groth16 verification keys for the three standard tiers.

Parameters:

| Name | Type | Description |
|------|------|-------------|
| `admin` | `Address` | Deployment admin authorized to manage verification keys and operational flags |
| `vk_small` | `VerificationKeyData` | Verification key for tier 0 (Small) |
| `vk_medium` | `VerificationKeyData` | Verification key for tier 1 (Medium) |
| `vk_large` | `VerificationKeyData` | Verification key for tier 2 (Large) |

Invariants:
- MUST be callable exactly once
- `admin` MUST authorize the invocation
- Each verification key MUST contain exactly 3 input-commitment points (`IC[0]`, `IC[1]`, `IC[2]`) for the 2 public inputs (`commitment`, `epoch`)
- After successful initialization, proof-verifying group operations become available

#### `create_group`

Registers a new group with its epoch-0 commitment and circuit tier.

Parameters:

| Name | Type | Description |
|------|------|-------------|
| `caller` | `Address` | Fee-paying caller for the create transaction |
| `group_id` | `BytesN<32>` | Application-defined 32-byte group identifier |
| `commitment` | `BytesN<32>` | Commitment for epoch 0 |
| `proof` | `Bytes` | ZK proof that the prover knows a valid epoch-0 witness for the submitted commitment (see Section 4.1) |
| `public_inputs` | `PublicInputs` | `{commitment, epoch: 0}` |
| `tier` | `u32` | Circuit tier: 0 = Small (32), 1 = Medium (256), 2 = Large (2048) |

Invariants:
- `caller` MUST authorize the invocation
- `group_id` MUST NOT already exist in storage
- Proof MUST verify against the verification key for the specified tier
- `group_id` MUST be exactly 32 bytes; applications with larger internal identifiers MUST hash or otherwise canonicalize them before contract submission
- Emits a `GroupCreated` event

##### 4.1 Semantics of the epoch-0 proof

At epoch 0, no prior group state exists. The ZK proof submitted with `create_group` proves the following: the prover knows a secret key `sk` and a salt `s` such that `Poseidon(sk)` is a leaf in the Poseidon Merkle tree whose root, combined with epoch 0 and salt `s`, produces the submitted commitment. This is the same circuit and statement used for all subsequent epochs — the creator is proving they are a member of the set they are committing, not that the set existed before.

This means the creator cannot register a group containing only other people's keys without also including their own. Any party who knows the salt and a valid member key can create the group; the contract does not privilege the creator in any way after creation.

#### `update_commitment`

Transitions a group to a new epoch with a new commitment.

Parameters:

| Name | Type | Description |
|------|------|-------------|
| `group_id` | `BytesN<32>` | Identifies the group |
| `proof` | `Bytes` | ZK proof produced by `R_Update` (see §3.7) |
| `public_inputs` | `UpdatePublicInputs` | `{c_old, epoch_old, c_new}` — 73-byte wire format, version byte 0x02 |

Invariants:
- `group_id` MUST exist and MUST NOT be deactivated
- Contract MUST check `public_inputs.c_old == stored.commitment` and `public_inputs.epoch_old == stored.epoch`; otherwise reject with a state-binding error before any pairing.
- Proof MUST verify under the tier's update verification key `vk_update` against the three public inputs `(c_old, epoch_old, c_new)` in fixed order.
- The new stored state is `(commitment := c_new, epoch := epoch_old + 1)`. The contract does NOT accept a caller-provided `new_epoch` or `new_commitment` parameter; both are bound by the proof.
- `c_new` MUST be a canonical Fr element; non-canonical encodings are rejected before verification.
- No address-based authorization is required beyond a valid proof.
- Emits a `CommitmentUpdated` event

Backward-incompatible change from v0.0.5: prior revisions exposed `new_commitment` and `new_epoch` as top-level parameters verified only against the *current* commitment. That encoding allowed a party who observed a valid proof to substitute a different `new_commitment` without invalidating the proof. The `R_Update` circuit defined in §3.7 binds `c_new` as a public input and eliminates this attack. See [vuln-unbound-new-commitment.md](vuln-unbound-new-commitment.md) for the full vulnerability analysis.

#### `verify_membership`

A read-only, non-consuming call that verifies a ZK proof of membership against the current state.

Parameters:

| Name | Type | Description |
|------|------|-------------|
| `group_id` | `BytesN<32>` | Identifies the group |
| `proof` | `Bytes` | ZK proof |
| `public_inputs` | `PublicInputs` | `{commitment, epoch}` |

Returns: `bool`

This function is a pure verification — no state change, no XLM cost when called via `simulateTransaction`. Implementations MUST NOT consume or blacklist the proof when used through `verify_membership`; the same proof may later be presented to a state-changing operation.

**Security note.** Because `verify_membership` does not burn a nullifier, the proof bytes observed in this call remain replayable. Any party that observes the simulation, the mempool, or transaction history can resubmit the same proof to another operation that accepts the same `(commitment, epoch)` public inputs — including `deactivate_group`. Integrators that need an on-chain attestation "the caller holds this proof and nobody else can reuse it" MUST use `consume_membership_proof` instead.

#### `consume_membership_proof`

A state-changing variant of `verify_membership` that records the proof's replay nullifier.

Parameters:

| Name | Type | Description |
|------|------|-------------|
| `group_id` | `BytesN<32>` | Identifies the group |
| `proof` | `Bytes` | ZK proof |
| `public_inputs` | `PublicInputs` | `{commitment, epoch}` |

Returns: unit. The function fails-closed: on verification success the contract records `SHA-256(π_A ‖ π_B ‖ π_C)` in the replay nullifier set and returns success; on verification failure it returns an `InvalidProof` error. There is no `false` return — tx inclusion is equivalent to a successful attestation.

Invariants:
- `group_id` MUST exist and MUST be active
- `public_inputs` MUST match the current on-chain `(commitment, epoch)`
- The proof's nullifier MUST NOT have been burnt before
- On success the proof MUST NOT be accepted by any subsequent state-changing call

Callers MUST submit this as a real transaction; invoking it via `simulateTransaction` or `--send no` does NOT persist the nullifier and therefore provides no replay protection.

**What this does not close.** Even `consume_membership_proof` cannot protect the honest caller from a mempool front-runner that submits the same proof bytes with a competing transaction. Closing pre-inclusion frontrun requires the membership circuit to bind an operation tag and the caller's address as public inputs; see §3 / §3.7 for the circuit rotation roadmap.

#### `deactivate_group`

Marks a group as inactive, preventing further epoch transitions and allowing storage to be reclaimed.

Parameters:

| Name | Type | Description |
|------|------|-------------|
| `group_id` | `BytesN<32>` | Identifies the group |
| `proof` | `Bytes` | ZK proof that the caller is a member of the current epoch's committed set |
| `public_inputs` | `PublicInputs` | `{commitment: current_commitment, epoch: current_epoch}` |

Invariants:
- `group_id` MUST exist and MUST NOT already be deactivated
- Proof MUST verify against the current commitment
- No address-based authorization is required beyond a valid proof
- After deactivation, `update_commitment` calls for this `group_id` MUST be rejected
- `verify_membership` and `get_state` remain functional (the last committed state is preserved)
- Emits a `GroupDeactivated` event
- Persistent storage entries for epoch history MAY be reclaimed by the contract after deactivation

#### `get_state`

Returns the current `CommitmentEntry` for a group.

```
CommitmentEntry {
    commitment:  BytesN<32>
    epoch:       u64
    timestamp:   u64        -- ledger timestamp of last update
    tier:        u32        -- circuit tier
    active:      bool       -- false if deactivated
}
```

Note: no `committer` address is stored. The ZK proof decouples the transaction signer from group identity.

#### `get_history`

Returns the most recent `N` `CommitmentEntry` records for a group, ordered by epoch, where `N` is capped by the `history_window` parameter.

Parameters:

| Name | Type | Description |
|------|------|-------------|
| `group_id` | `BytesN<32>` | Identifies the group |
| `max_entries` | `u32` | Maximum number of entries to return, capped at `history_window` |

The contract SHOULD maintain a rolling window of the last `history_window` entries (default: 64). Older entries are pruned from persistent storage on each `update_commitment` call. Applications that need full history MUST reconstruct it from `CommitmentUpdated` events emitted by the contract.

**Rationale.** An unbounded append-only history in contract storage accumulates rent indefinitely and exposes the complete temporal fingerprint of a group's activity. A rolling window bounds storage cost while preserving enough state for recent dispute resolution. Events provide the complete audit trail for applications that need it.

#### Operational extensions

Implementations MAY expose deployment-management operations in addition to the interoperable interface above. The current reference contract exposes:

- `update_vk(tier, new_vk)` — admin-authorized verification-key rotation for a tier
- `set_restricted_mode(restricted)` — admin-authorized toggle that limits `create_group` to the admin
- `bump_group_ttl(group_id)` — maintenance operation that extends Soroban persistent-storage TTL for a group without changing its state

These operations do not affect proof format or client interoperability, but they are operationally useful for production deployments.

#### Proof replay hardening

Implementations SHOULD reject any state-changing operation (`create_group`, `update_commitment`, `deactivate_group`, `consume_membership_proof`) that reuses an identical serialized proof previously accepted by the contract. This prevents exact-proof replay across groups and functions. A proof presented only to `verify_membership` SHOULD remain reusable because that call is read-only and explicitly non-consuming.

**Note on replay-hash scope.** The replay key is `SHA-256(π_A ‖ π_B ‖ π_C)` — computed over the 384-byte uncompressed proof (π_A 96 + π_B 192 + π_C 96). It does not cover the transaction envelope or the public inputs. Values outside the proof's public-input scope are freely mutable by anyone observing the proof; this is why `update_commitment` MUST use `R_Update` with `c_new` as a public input (§3.7). Without that binding, replay protection alone is insufficient to fix the v0.0.5 gap.

---

### 5. Transaction Submission and Fee Decoupling

The ZK proof ensures that the submitter is a valid group member without revealing which member. However, if the member submits the transaction from their own Stellar account, the fee payer address in the transaction envelope re-links a Stellar identity to group activity — partially defeating the purpose of the ZK proof.

Implementations MUST support and SHOULD encourage one of the following fee decoupling strategies:

#### 5.1 Relayer pattern (RECOMMENDED)

A relayer is a public service that accepts pre-signed Soroban invocations and wraps them in a transaction envelope using the relayer's own Stellar account as the fee source.

The relayer workflow:

1. The group member constructs a signed Soroban `InvokeHostFunction` operation containing the `update_commitment` (or `create_group`) call with the ZK proof.
2. The member submits the signed operation to the relayer via an anonymous transport (e.g., Tor, a public HTTPS endpoint with no authentication).
3. The relayer wraps the operation in a transaction, sets itself as `source_account`, signs the transaction envelope, and submits to the Stellar network.
4. The relayer pays the transaction fee. Fee reimbursement, if any, happens out-of-band.

The relayer does not need to be trusted with any secret material. It sees the proof (which reveals nothing about the prover) and the group_id (which is opaque). It cannot determine which member is submitting the update.

A reference relayer specification is out of scope for this SEP but is expected as a companion document.

#### 5.2 Shared fee account

The group maintains a Stellar account funded by members via anonymous or batched deposits. All `update_commitment` transactions are submitted from this shared account. Members share the signing key for this account via the encrypted channel.

This approach is simpler but has weaker privacy properties: the shared account is publicly associated with the group_id, and funding patterns may leak information.

#### 5.3 Fee sponsorship via Soroban authorization

If future Soroban protocol versions introduce native fee sponsorship (where an invoker and fee payer can be distinct accounts in a single transaction), implementations SHOULD adopt that mechanism. This SEP will be updated to reference the relevant protocol version when available.

---

### 6. Events

Implementations MUST emit the following contract events for indexers.

#### `GroupCreated`

```
topics: ["GroupCreated", group_id]
data:   { commitment: BytesN<32>, epoch: 0, tier: u32, timestamp: u64 }
```

#### `CommitmentUpdated`

```
topics: ["CommitmentUpdated", group_id]
data:   { commitment: BytesN<32>, epoch: u64, timestamp: u64 }
```

#### `GroupDeactivated`

```
topics: ["GroupDeactivated", group_id]
data:   { final_epoch: u64, timestamp: u64 }
```

No member identity appears in any event data.

---

### 7. Salt Distribution Protocol

The salt for each epoch must reach all current group members without appearing on-chain. This SEP is intentionally agnostic about the transport layer. Two compliant approaches are described:

**Approach A — Encrypted group messaging layer (RECOMMENDED)**

The salt is embedded as a custom extension in the group's application-layer key agreement protocol (e.g., MLS RFC 9420 `GroupContext` extension type `0xFF01`). The extension is authenticated by the protocol's existing Commit signature, travels through the encrypted channel, and is decrypted only by current members.

**Approach B — Direct encrypted delivery**

The Commit initiator encrypts `salt` to each member's BLS12-381 public key (using ECIES over BLS12-381 G1) and delivers the ciphertext via any out-of-band channel. Members decrypt and store the salt locally.

In both cases, the receiving member MUST verify locally using the commitment variant in use:

*Variant A:*
```
poseidon_root = MerklePoseidonTree(sorted_members).root
local_commitment = SHA-256(poseidon_root || epoch || salt)
assert local_commitment == contract.get_state(group_id).commitment
```

*Variant B:*
```
poseidon_root = MerklePoseidonTree(sorted_members).root
local_commitment = Poseidon(Poseidon(poseidon_root, epoch), salt)
assert local_commitment == contract.get_state(group_id).commitment
```

If the assertion fails, the member MUST NOT accept the epoch transition and SHOULD alert the user.

For salt recovery procedures when a member loses the current salt, see Section 2.5.

---

### 8. Circuit Tiers and Trusted Setup

#### 8.1 Standard circuit tiers

This SEP defines three standard circuit tiers. Each tier corresponds to a fixed Groth16 circuit with a specific maximum group size, tree depth, and independent trusted setup.

| Tier | Identifier | MAX_MEMBERS | Tree depth `d` | Constraints (Variant A, est.) | Constraints (Variant B, measured) |
|------|------------|-------------|----------------|-------------------------------|-----------------------------------|
| Small | 0 | 32 | 5 | ~27,500 | **1,910** |
| Medium | 1 | 256 | 8 | ~28,400 | **2,630** |
| Large | 2 | 2,048 | 11 | ~29,300 | **3,350** |

A group is bound to a single tier at creation time (the `tier` parameter in `create_group`). The contract stores the verification key for each supported tier and uses the appropriate key during proof verification.

Applications SHOULD choose the smallest tier that accommodates their expected group size. Unused leaf slots in the Merkle tree are filled with zero-valued leaves and do not affect proof validity or privacy.

If a group outgrows its tier, the application MUST create a new group at a higher tier and migrate members. The old group SHOULD be deactivated.

#### 8.2 Trusted setup ceremony

Groth16 requires a circuit-specific trusted setup ceremony producing a proving key `pk` and verification key `vk`. The security guarantee is: the setup is sound if at least one participant honestly discards their toxic waste.

A separate MPC ceremony MUST be conducted for each circuit tier before any mainnet deployment of a contract implementing this standard.

##### 8.2.1 Powers of Tau protocol

The ceremony uses the Powers of Tau MPC protocol over BLS12-381. Each ceremony accumulates three secret scalars (τ, α, β) across N sequential participants. No single participant knows the final accumulated values.

**Structured Reference String (SRS).** The SRS produced by the ceremony contains:

| Element | Description | Count |
|---------|-------------|-------|
| τ^i · G1 | Powers of τ in G1 | 2·degree − 1 |
| τ^i · G2 | Powers of τ in G2 | degree |
| α · τ^i · G1 | Alpha-shifted powers in G1 | degree |
| β · τ^i · G1 | Beta-shifted powers in G1 | degree |
| β · G2 | Beta in G2 | 1 |

where `degree` is determined by the circuit tier's constraint count.

**Contribution protocol.** Each participant j:

1. Generates random update factors (δ_τ, δ_α, δ_β) from a cryptographic RNG
2. Updates every SRS element: τ_new^i = δ_τ^i · τ_old^i, similarly for α and β series
3. Produces a Schnorr-like proof of knowledge for each update factor
4. Publishes the updated SRS and proof; destroys the update factors

**Verification.** Each contribution is verified via BLS12-381 pairing checks:

- **Ratio proofs**: e(s·G1, τ_new·G2) == e(δ_τ·s·G1, τ_old·G2) proves knowledge of δ_τ (similarly for α, β)
- **Cross-consistency**: e(τ·G1, G2) == e(G1, τ·G2) ensures G1 and G2 series use the same τ
- **Power-sequence**: e(τ^i·G1, τ·G2) == e(τ^{i+1}·G1, G2) verifies the power structure

##### 8.2.2 Ceremony requirements

Each ceremony MUST:

- Use the Powers of Tau format compatible with `snarkjs`, `bellman`, or `arkworks`
- Include a minimum of 10 independent participants
- Verify each contribution via pairing checks before accepting
- Record a SHA-256 hash chain of SRS states as a public transcript
- Publish all contribution hashes and attestations publicly
- Derive the final Groth16 keys via Phase 2 of Groth16 MPC (see Section 8.2.3)

##### 8.2.3 Key derivation

**Production deployments** MUST derive Groth16 keys via Phase 2 of Groth16 MPC: the QAP is evaluated directly on the SRS curve points, so that no single machine ever learns the accumulated toxic waste scalars (τ, α, β, γ, δ). This preserves the ceremony's 1-of-N trust guarantee through to the final keys.

**The reference implementation** uses a simulation approach: the accumulated ceremony scalars are hashed (domain-separated SHA-256) into a ChaCha20 CSPRNG seed, which drives arkworks' standard Groth16 `circuit_specific_setup`. This means the machine executing key derivation sees the accumulated scalars and can recover the Groth16 toxic waste. The 1-of-N trust guarantee of the ceremony does NOT carry through to the derived keys in this simulation. The reference implementation demonstrates the ceremony protocol (contribution, pairing-based verification, transcript) but relies on a placeholder key derivation that is not production-secure.

##### 8.2.4 Circuit identifier

Each circuit is uniquely identified by:

```
circuit_id = SHA-256("SEP-XXXX" || tier_id || tree_depth || poseidon_params_hash)
```

where `poseidon_params_hash = SHA-256("poseidon-params" || full_rounds || partial_rounds || alpha || rate || capacity || round_constants)`.

##### 8.2.5 Transcript verification

A ceremony transcript consists of an ordered list of contribution records, each containing:

- Contribution index
- Schnorr-like proof of knowledge
- SHA-256 hash of the SRS after the contribution

Anyone can verify a transcript by checking the hash chain and the final SRS internal consistency via pairing equations.

The verification keys for all supported tiers are stored in the Soroban contract at deployment time and are immutable. A contract upgrade that changes any verification key constitutes a new deployment and requires a new ceremony.

The proving keys are distributed to client applications. They are public — possession of a proving key does not endanger security.

---

### 9. Security Analysis

#### 9.1 What an on-chain observer learns

From the ledger, an observer can determine:

- That a group identified by `group_id` exists
- How many times the group's membership changed (epoch count)
- The timestamp of each change (within the rolling history window)
- The circuit tier, which reveals an upper bound on group size (32, 256, or 2,048)
- A 192-byte proof blob and 32-byte commitment per epoch — neither reveals membership or prover identity

An observer cannot determine:

- How many members are in the group (only the tier upper bound)
- The identity of any member
- Which member submitted any given update
- Whether membership grew or shrank in a given epoch
- The identity of the group creator (if `group_id` is derived as specified in Section 1)

#### 9.2 Commitment hiding

**Variant A:** The commitment is `SHA-256(poseidon_root || epoch || salt)`. Given a fresh 32-byte random salt and SHA-256's preimage resistance, an observer cannot recover the Poseidon root (or the underlying member set) without `2^256` operations.

**Variant B:** The commitment is `Poseidon(Poseidon(poseidon_root, epoch), salt)`. The hiding property relies on Poseidon's preimage resistance over the BLS12-381 scalar field. The salt provides ~255 bits of effective entropy (32 bytes reduced mod `r`), which is cryptographically sufficient.

In both variants, the salt prevents offline dictionary attacks even when the universe of possible members is small. The Poseidon Merkle tree adds a second layer of hiding: even if an attacker somehow obtained the Poseidon root, recovering the individual leaves requires breaking Poseidon's preimage resistance.

#### 9.3 ZK soundness

Groth16 is computationally sound under the knowledge-of-exponent assumption over BLS12-381. A party without a valid witness (i.e., a non-member) cannot produce an accepting proof except with negligible probability.

#### 9.4 ZK zero-knowledge

Groth16 is zero-knowledge: the proof reveals nothing about the witness beyond the validity of the statement. In particular, the prover's leaf index in the Merkle tree and their public key `pk` are not recoverable from the proof or the public inputs.

#### 9.5 Epoch monotonicity

`update_commitment` persists the new epoch as `epoch_old + 1`, with `epoch_old` taken from the public-input scope of the proof and cross-checked against the on-chain stored epoch. The `+ 1` increment is performed inside `R_Update` (§3.7), not accepted from the caller. This prevents replay attacks (resubmitting an old proof for a past epoch) and fork attacks (two conflicting epoch-N commitments).

#### 9.6 Proof binds old and new state

The ZK proof in `update_commitment` is verified under `R_Update` with the three public inputs `(c_old, epoch_old, c_new)`. The contract additionally cross-checks `(c_old, epoch_old)` against on-chain stored state. Consequently:

- The updater must prove membership in the group as it existed before the transition — they cannot forge a new roster without holding a valid current member key.
- The new commitment is a public input of the proof. Substituting `c_new` in the transaction envelope invalidates the pairing equation. A legitimate proof cannot be re-purposed to commit a different `c_new`.

This is a stricter binding than v0.0.5, where the new commitment was an envelope-level parameter checked only against the current state.

#### 9.7 Fee payer correlation

If the transaction fee payer is the same Stellar account as a group member, an observer can correlate that address with group activity over time, partially defeating the ZK proof's privacy guarantee. Section 5 defines normative mitigations. The relayer pattern (Section 5.1) eliminates this correlation entirely. Implementations that do not implement fee decoupling MUST document this as a known privacy limitation.

#### 9.8 Tier upper bound leakage

The circuit tier reveals an upper bound on group size (e.g., a tier-0 group has at most 32 members). Applications for which this is sensitive SHOULD use a higher tier than strictly necessary.

#### 9.9 Residual leakage summary

| Observable | Severity | Mitigation |
|------------|----------|------------|
| `group_id` existence | Low | Derive `group_id` as a hash (Section 1) |
| Epoch frequency and timing | Low | Reveals activity patterns, not identity |
| Transaction fee payer | **Medium** if unmitigated | Relayer pattern (Section 5.1) — normative |
| Circuit tier (group size upper bound) | Low | Use a higher tier than necessary |
| Proof size is constant | Positive | Group size is not inferrable |
| History window length | Low | Rolling window bounds exposure (Section 4) |

---

### 10. Test Vectors

Implementations MUST produce the following commitment values from the given inputs. These vectors allow independent validation of the Poseidon Merkle tree construction, the dual-hash commitment, and the canonical serialization.

Note: Poseidon hash outputs below use the BLS12-381 scalar field parameters specified in Section 2.2. All byte strings are hex-encoded.

#### Vector 1 — epoch 0, 2 members, tier Small

```
members (compressed G1, sorted):
    member_0 = 0x010101...01  (48 bytes, all 0x01)
    member_1 = 0x020202...02  (48 bytes, all 0x02)

Poseidon Merkle tree (depth 5, 32 leaves):
    leaf[0] = Poseidon(member_0_x)
    leaf[1] = Poseidon(member_1_x)
    leaf[2..31] = 0x00...00  (zero leaves)
    poseidon_root = [to be filled by reference implementation]

salt = 0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
epoch = 0x0000000000000000

commitment_preimage (72 bytes):
    poseidon_root (32 bytes) || epoch (8 bytes) || salt (32 bytes)

commitment = SHA-256(commitment_preimage)
           = [to be filled by reference implementation]
```

#### Vector 2 — epoch 1, 3 members, tier Small

```
members (compressed G1, sorted):
    member_0 = 0x010101...01  (48 bytes)
    member_1 = 0x020202...02  (48 bytes)
    member_2 = 0x030303...03  (48 bytes)

Poseidon Merkle tree (depth 5, 32 leaves):
    leaf[0..2] = Poseidon(member_i_x)
    leaf[3..31] = 0x00...00
    poseidon_root = [to be filled by reference implementation]

salt = 0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
epoch = 0x0000000000000001

commitment = SHA-256(poseidon_root || epoch || salt)
           = [to be filled by reference implementation]
```

#### Vector 3 — sort order enforcement

Identical inputs to Vector 2 but with `member_0` and `member_2` swapped in the input (wrong order). After sorting, the Poseidon Merkle tree and commitment MUST be identical to Vector 2. Any implementation that produces a different commitment has a sorting bug.

---

### 11. Rationale

**Why a dual-hash scheme (Poseidon + SHA-256) rather than SHA-256 throughout?**

SHA-256 inside a Groth16 R1CS circuit costs ~25,000 constraints per compression call. In the v0.1 draft, the circuit hashed the entire canonical member list under SHA-256, making circuit size grow linearly with group size — a 100-member group required multiple compression calls totaling hundreds of thousands of constraints just for the hash.

The dual-hash scheme confines SHA-256 to a single fixed-length (72-byte) compression call for the outer commitment binding, and uses Poseidon (~300 constraints per hash) for the Merkle tree. This makes the in-circuit cost logarithmic in group size (one Poseidon call per tree level) while preserving on-chain verifiability via the SHA-256 host function.

**Why does the reference implementation use Poseidon-only binding (Variant B) instead of in-circuit SHA-256 (Variant A)?**

In-circuit SHA-256 costs ~25,000 R1CS constraints even for a single fixed-length compression call, making it the dominant cost in the circuit by a factor of 8-16x over all other constraints combined. The Phase 1 reference implementation replaced the in-circuit SHA-256 with two Poseidon hash calls (~600 constraints), reducing total circuit size from ~27,500 to ~1,911 constraints for the Small tier — a ~14x reduction. This dramatically improves prover time and memory requirements, particularly on resource-constrained clients (mobile devices). The tradeoff is that on-chain commitment verification cannot use the native `env.crypto().sha256()` host function directly. If Soroban adds a Poseidon host function in a future protocol version, Variant B becomes strictly superior. Until then, the contract can verify Poseidon-only commitments by accepting them as opaque values validated solely through ZK proof verification, without independent on-chain recomputation of the commitment.

**Why BLS12-381 identity keys rather than Ed25519?**

Ed25519 key ownership verification inside a BLS12-381 Groth16 circuit requires emulating Curve25519 field arithmetic in a non-native field, costing 500,000+ constraints for a single scalar multiplication. BLS12-381 native keys allow key ownership to be proven via `Poseidon(sk)` preimage knowledge (~240 constraints) since the private key scalar is a native BLS12-381 field element. The Stellar Ed25519 address link is preserved via an off-chain attestation (Section 1.1) that is verified at group registration, not on every proof.

**Why a Poseidon Merkle tree rather than hashing the flat member array?**

The v0.1 draft included both a flat-array hash (for commitment binding) and a Merkle tree (for membership proof) — a redundancy, since the flat hash already required the full member array as witness. By making the Poseidon Merkle root the canonical representation of the member set, the circuit only needs a logarithmic-depth Merkle opening proof. The prover supplies only their leaf and `d` sibling hashes, not the full member list. This eliminates the redundancy and makes proving time sublinear in group size.

**Why Groth16 rather than PLONK or STARKs?**

Protocol 22 provides BLS12-381 pairing operations, which directly accelerate Groth16 verification. PLONK also uses pairings but requires polynomial commitment verification not yet available as host functions. STARKs are pairing-free but produce larger proofs and have no dedicated host function acceleration. Groth16 is the best fit for the current Soroban host environment.

**Why define standard circuit tiers?**

Groth16 circuits are fixed at setup time. Without standard tiers, every application would define its own MAX_MEMBERS and conduct its own trusted setup, fragmenting the ecosystem. Standard tiers allow shared trusted setup ceremonies, shared proving keys, and interoperable tooling.

**Why is there no `committer` field in `CommitmentEntry`?**

Storing the committer address would partially defeat the purpose of the ZK proof — an observer could still correlate Stellar addresses with group activity over time. The ZK proof is sufficient authorization. Fee decoupling (Section 5) ensures the fee payer is not linkable to the group member.

**Why a rolling history window rather than unbounded append-only history?**

Unbounded on-chain history accumulates Soroban storage rent indefinitely and exposes the complete temporal fingerprint of a group. A rolling window bounds both cost and leakage. The complete audit trail remains available via contract events, which are stored by Horizon indexers and do not incur persistent contract storage rent.

**Why enforce strict epoch monotonicity rather than allowing out-of-order updates?**

Strict monotonicity provides a simple, auditable invariant: the history is a linear sequence. It prevents replay, fork, and interleaving attacks without requiring the contract to maintain a nonce map per member. Applications requiring concurrent or parallel group state machines should use separate `group_id` values.

---

### 12. Reference Implementation

The reference implementation spans the full repository:

- `src/` — Rust cryptographic core written on arkworks (v0.4)
- `contracts/sep-xxxx/` — Soroban contract for on-chain verification and state management
- `relayer/` — fee-decoupling HTTP relayer
- `swift-mls/` and `kotlin-mls/` — mobile/client SDKs
- `clients/ios/` and `clients/android/` — reference applications integrating transport, attestation, salt recovery, and on-chain submission

The Rust cryptographic core uses:

- `ark-groth16` — Groth16 prover and verifier
- `ark-bls12-381` — BLS12-381 curve and field implementations
- `ark-crypto-primitives` — Poseidon sponge, R1CS gadgets
- `ark-r1cs-std` — R1CS standard gadgets (field variables, boolean, conditionals)
- `sha2` — SHA-256 for off-circuit commitment computation and SRS hashing
- `rand_chacha` — ChaCha20 CSPRNG for deterministic key derivation

**Modules:**

| Module | Purpose |
|--------|---------|
| `poseidon` | Poseidon hash function with deterministic parameter generation |
| `merkle` | Binary Poseidon Merkle tree: build from scalars or pre-computed leaf hashes, prove, verify |
| `commitment` | Dual commitment construction: SHA-256 (Variant A) and Poseidon-only (Variant B) |
| `circuit` | Groth16 R1CS circuit (Section 3.3, Variant B with Poseidon preimage key ownership) |
| `prover` | End-to-end pipeline: trusted setup, proof generation, verification, serialization |
| `ceremony` | Powers of Tau MPC ceremony: initialize, contribute, verify, derive keys, compute circuit IDs |

**Implementation choices:**

1. **Commitment binding uses Variant B (Poseidon-only).** The `commitment` module implements both Variant A (SHA-256) and Variant B (Poseidon-only). The circuit uses Variant B for in-circuit binding. This is a deliberate optimization that reduces circuit size by ~14x compared to in-circuit SHA-256.
2. **Key ownership via Poseidon preimage.** The circuit proves `Poseidon(sk) == leaf`, establishing knowledge of the secret key whose hash is a Merkle tree leaf. This replaces the originally specified `sk · G1` in-circuit scalar multiplication, achieving the same security guarantee (knowledge of `sk`) with fewer constraints (~240 vs ~1,000) and no dependency on elliptic curve gadgets.
3. **Two public inputs.** The circuit exposes 2 public inputs (`commitment`, `epoch`) as BLS12-381 field elements. The earlier 3-input design (`commitment_high`, `commitment_low`, `epoch`) was simplified by removing the redundant `commitment_low == epoch` binding.
4. **Member sorting is enforced by the implementation.** The Merkle tree builder accepts member records containing `{compressed G1 public key, Poseidon(sk)}` and sorts them internally by compressed G1 bytes. Duplicate public keys are rejected so the commitment remains a canonical encoding of a set.
5. **Pre-computed leaf hashes.** The `merkle` module supports building trees from `{compressed G1 public key, Poseidon(sk)}` member records (via `build_from_members`), enabling the production workflow where members share canonical sort keys and leaf hashes during registration without revealing secret keys.
6. **Ceremony API stops at public Phase 1 output.** The `ceremony` module implements Powers of Tau initialization, sequential contributions with pairing-based verification, SRS consistency checks, transcript verification, and circuit identification. The public `run_ceremony` function returns only the final SRS, transcript, and `circuit_id`; the old scalar-based Groth16 key derivation remains test-only because it does not preserve the 1-of-N trust guarantee.
7. **Contract boundary uses uncompressed proofs.** The SDKs and FFI/JNI bridges convert the prover's 192-byte compressed Groth16 proof into the contract's 384-byte uncompressed `(A, B, C)` format before submission, avoiding decompression work on-chain.
8. **The deployed contract fixes `group_id` at 32 bytes.** Applications derive a canonical 32-byte identifier off-chain and submit that value consistently to the contract, relayer, and SDKs.
9. **State-changing proofs are replay-tracked.** The Soroban contract records a SHA-256 hash of each accepted `(A, B, C)` proof and rejects exact reuse for `create_group`, `update_commitment`, `deactivate_group`, and `consume_membership_proof`. `verify_membership` is read-only and explicitly non-consuming; callers that need on-chain replay protection MUST use `consume_membership_proof` instead.
10. **Operational hooks are part of the implementation.** The contract includes one-time initialization, admin-controlled verification-key rotation, restricted create mode, and TTL bump maintenance in addition to the interoperable group operations.
11. **Proof size confirmed at 192 bytes** (48 + 96 + 48 compressed BLS12-381 points).

---

### 13. Changelog

| Version | Date | Notes |
|---------|------|-------|
| 0.0.1 | 2026-03-30 | Initial draft |
| 0.0.2 | 2026-03-31 | Added Poseidon-only commitment binding variant (Variant B) based on Phase 1 reference implementation; added measured constraint counts; documented Poseidon parameter generation; added reference implementation section; updated circuit tiers table with both variant counts |
| 0.0.3 | 2026-03-31 | Key ownership changed from `sk · G1` to `Poseidon(sk)` preimage proof; reduced public inputs from 3 to 2 (`commitment`, `epoch`); updated Poseidon seed to include all parameters; updated formal relations for both variants; updated constraint tables with measured values; documented salt verification for both variants; updated reference implementation section to reflect 67 tests and current API |
| 0.0.4 | 2026-03-31 | Added Phase 2: Powers of Tau ceremony implementation; expanded Section 8.2 with ceremony protocol details (SRS structure, contribution verification via pairing checks, transcript format, deterministic key derivation); added `ceremony` module to reference implementation; 93 tests total |
| 0.0.5 | 2026-04-02 | Aligned Section 4 with the implemented Soroban ABI: added `initialize`, documented operational extensions (`update_vk`, restricted mode, TTL bump), fixed `group_id` to `BytesN<32>`, clarified proof-based authorization and proof-replay hardening, and expanded the reference-implementation section to cover the contract, relayer, SDKs, and reference apps |
