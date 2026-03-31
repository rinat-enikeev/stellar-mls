# Phase 1: Private Group Membership Proof — Explained Simply

## What problem are we solving?

Imagine a group chat. Today, if you store the member list on a blockchain, everyone in the world can see who is in the group. That is a privacy problem.

We want to store the group on Stellar so that:

- Nobody looking at the blockchain can tell who is in the group.
- A member can prove "I am in this group" without revealing which member they are.
- The group can change its membership, and each change is authorized by a current member.

The only thing visible on-chain is a 32-byte fingerprint (the **commitment**) and a 192-byte **proof**. No names, no keys, no member count.

---

## The building blocks

### 1. Secret keys

Each member has a secret number (`sk`) — a BLS12-381 private key scalar, roughly 255 bits long. Nobody else knows it.

### 2. Poseidon hash

Poseidon is a hash function, like SHA-256, but designed to be cheap inside a zero-knowledge circuit. It takes one or two numbers in and produces one number out. It has the standard hash property: given the output, you cannot find the input.

### 3. Merkle tree

A Merkle tree is a binary tree of hashes. You put the member data at the leaves, then repeatedly hash pairs of children to get parent nodes, until you reach a single root hash at the top.

```
        root
       /    \
     h01     h23
    /   \   /   \
  L0   L1  L2   L3
```

The key property: you can prove that a specific leaf is in the tree by showing only the sibling hashes along the path from that leaf to the root. For a tree with 32 leaves, that is just 5 hashes — not the entire list of 32 members.

### 4. Groth16 (the zero-knowledge proof system)

Groth16 lets you prove a statement about secret data without revealing the data. You write the statement as a set of equations (a "circuit"), and the proof system produces a short proof (192 bytes) that anyone can verify with three pairing operations on the BLS12-381 elliptic curve.

The proof reveals **nothing** about the secret data — not the member's key, not their position in the tree, not even the number of members.

---

## How it all fits together

### Registration (one-time setup)

1. Each member computes their **leaf hash**: `leaf = Poseidon(sk)`.
2. Members share their leaf hashes with each other (not their secret keys).
3. The leaf hashes are sorted and placed into a Merkle tree of fixed depth.
4. The Merkle root, combined with the epoch number and a random salt, produces the **commitment** that goes on-chain.

```
commitment = Poseidon(Poseidon(root, epoch), salt)
```

The salt is a 32-byte random value shared privately among members. It prevents anyone from guessing the root by trying all possible member combinations.

### Proving membership

When a member wants to prove they belong to the group, they produce a Groth16 proof that demonstrates three things simultaneously, without revealing any of the secret values:

**Statement 1 — "I know the secret key"**

> I know a value `sk` such that `Poseidon(sk)` equals a specific leaf in the tree.

Since Poseidon is a one-way function, only someone who knows `sk` can produce the matching leaf hash. This is how the circuit verifies key ownership.

**Statement 2 — "That key is in the Merkle tree"**

> Starting from my leaf `Poseidon(sk)`, walking up the tree using the sibling hashes, I arrive at `root`.

The prover supplies the sibling hashes (the "Merkle path") as secret witness data. The circuit re-computes the root from the leaf upward and checks it matches.

**Statement 3 — "The tree matches what is on-chain"**

> `Poseidon(Poseidon(root, epoch), salt) == commitment`

The commitment is a public input (visible to the verifier). The root and salt are secret. This equation ties the Merkle tree to the specific on-chain commitment for this epoch.

### Verification

The Stellar smart contract checks:

1. Does the submitted `commitment` match what is stored on-chain for this group and epoch?
2. Does the Groth16 proof verify against the verification key?

That is it. The contract never sees the member list, the salt, or any secret key.

---

## Why is it mathematically correct?

We need to argue two things: **soundness** (a non-member cannot fake a proof) and **zero-knowledge** (the proof reveals nothing about the member).

### Soundness — a non-member cannot cheat

Suppose an attacker wants to produce a valid proof without being a group member. They need to satisfy all three constraints simultaneously. Here is why each one blocks them:

**Constraint 1 blocks key fabrication.** The attacker needs `Poseidon(sk) = leaf` for some leaf that is in the tree. If they do not know a real member's `sk`, they must find a value whose Poseidon hash matches an existing leaf. This is a **preimage attack** on Poseidon — computationally infeasible (would take ~2^255 attempts).

**Constraint 2 blocks tree forgery.** Even if the attacker picked their own `sk` and computed `Poseidon(sk)`, that leaf is not in the real Merkle tree. They would need to construct a Merkle path leading to the real root, but changing any leaf changes every hash above it. Finding a different leaf that reconstructs the same root is a **collision attack** on Poseidon — also infeasible.

**Constraint 3 blocks commitment forgery.** The attacker could try using a fake root with a fake salt. They would need to find `(root', salt')` such that `Poseidon(Poseidon(root', epoch), salt') = commitment`. This is a **preimage attack** on the nested Poseidon hash — infeasible. And even if they found such a pair, they would still need `root'` to be the root of a tree containing their leaf (blocked by constraint 2).

**All three constraints share variables.** The circuit uses the **same** `root` variable in constraints 2 and 3, and the **same** `Poseidon(sk)` value in constraints 1 and 2. The attacker cannot use a different root for the Merkle proof and the commitment binding — they are the same wire in the circuit.

**The salt is secret.** Without knowing the salt, the attacker cannot even verify their own attempts offline. They would need to break Poseidon to find the salt, and then break it again to forge a leaf.

**Groth16 soundness guarantee.** Under the Knowledge of Exponent assumption on BLS12-381, the Groth16 proof system ensures that anyone producing a valid proof must "know" a valid witness satisfying all constraints. There is no shortcut that avoids actually having the secret data.

### Zero-knowledge — the proof reveals nothing

Groth16 is a **zero-knowledge** proof system. This means:

- The proof is a set of three elliptic curve points (192 bytes). It is computationally indistinguishable from random points to anyone who does not know the proving key's trapdoor (which is destroyed after the trusted setup).
- The verifier learns only that the statement is true. They do not learn `sk`, the leaf index, the Merkle path, the root, or the salt.
- Two different members proving membership in the same group produce proofs that are unlinkable — you cannot tell whether two proofs came from the same member or different members.

### Why Poseidon is safe for key ownership

Sharing `Poseidon(sk)` with other group members during registration does not compromise `sk`. Poseidon is designed to be preimage-resistant: given `h = Poseidon(sk)`, recovering `sk` requires ~2^255 hash evaluations. This is the same security assumption the entire Merkle tree relies on — if Poseidon were broken, the whole scheme would break regardless.

### Why empty tree slots are safe

Unused leaf positions contain the value zero. An attacker would need `Poseidon(sk) = 0` for some `sk` to impersonate an empty slot. This is another preimage attack — infeasible.

---

## What the numbers look like

### Proof size

Every proof is exactly **192 bytes**, regardless of group size:

| Component | Curve element | Size |
|-----------|---------------|------|
| pi_A | G1 point (compressed) | 48 bytes |
| pi_B | G2 point (compressed) | 96 bytes |
| pi_C | G1 point (compressed) | 48 bytes |

### Public inputs

Two field elements (each 32 bytes = 64 bytes total):

| Input | Description |
|-------|-------------|
| `commitment` | The Poseidon binding value |
| `epoch` | The epoch number as a field element |

### Circuit size

The circuit is small because everything uses Poseidon (a ZK-friendly hash) instead of SHA-256 (which costs ~25,000 constraints per call inside a circuit):

| Tier | Max members | Tree depth | Constraints |
|------|-------------|------------|-------------|
| Small | 32 | 5 | ~1,910 |
| Medium | 256 | 8 | ~2,630 |
| Large | 2,048 | 11 | ~3,350 |

The constraint count grows logarithmically with group size — adding 3 more levels of depth (8x more members) only adds ~720 constraints.

### Verification cost

Constant at all group sizes:

- 3 BLS12-381 pairing operations
- Available as Soroban host functions since Protocol 22
- A 2,048-member group costs the same to verify as a 2-member group

---

## Module map

| Module | What it does |
|--------|-------------|
| `poseidon` | Poseidon hash function with deterministic parameter generation |
| `merkle` | Binary Poseidon Merkle tree: build from secret keys or pre-computed leaf hashes, generate and verify opening proofs |
| `commitment` | Commitment construction: SHA-256 variant (for on-chain use) and Poseidon-only variant (for in-circuit use) |
| `circuit` | The Groth16 R1CS circuit encoding all three constraints |
| `prover` | End-to-end pipeline: trusted setup, proof generation, verification, serialization |

### Test coverage

67 tests covering:

- Poseidon hash determinism, collision resistance, non-commutativity
- Merkle tree construction, proof generation, verification, tamper detection
- Commitment determinism, sensitivity to each input, serialization roundtrips
- Circuit satisfiability for valid witnesses at all tiers
- Circuit rejection of wrong secret keys, wrong roots, wrong epochs, wrong salts
- Full Groth16 pipeline: setup, prove, verify, serialize, deserialize
- Epoch transitions and cross-epoch proof rejection
- Wrong-key rejection at the Groth16 level (not just constraint system level)
