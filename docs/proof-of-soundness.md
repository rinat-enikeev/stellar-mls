# Proof of Soundness

**SEP-XXXX Zero-Knowledge Group Membership Protocol**

*Version 1.0 — April 6, 2026*

---

## 1. Preamble

### 1.1 Scope

This document provides formal security proofs for the SEP-XXXX zero-knowledge group membership protocol as implemented in the Onym system. We prove eight theorems that together establish the following properties:

1. **Membership soundness** — only holders of a secret key in the committed member set can produce an accepting proof.
2. **Membership privacy** — the proof reveals nothing about the prover's identity or position in the group.
3. **Commitment confidentiality** — on-chain commitments reveal no information about group members.
4. **State integrity** — group state advances monotonically and cannot be forked.
5. **Proof non-replayability** — an accepted proof cannot be replayed to any other contract call.
6. **Fee-payer unlinkability** — the relayer pattern decouples the transaction submitter from the prover.

We also reproduce the impossibility proof for symmetric member removal from the fourth security audit, establishing that post-compromise forward secrecy requires an asymmetric rekey mechanism.

**Out of scope.** Network-layer traffic analysis (IP correlation, timing attacks on Nostr relays), side-channel attacks on client implementations, and denial-of-service attacks against the Stellar network or relay infrastructure.

### 1.2 Notation

| Symbol | Meaning |
|--------|---------|
| $\mathbb{F}_r$ | BLS12-381 scalar field, $r \approx 2^{255}$, $r = \texttt{0x73eda753...00000001}$ |
| $\mathbb{G}_1, \mathbb{G}_2$ | BLS12-381 source groups (subgroups of $E(\mathbb{F}_q)$ and $E'(\mathbb{F}_{q^2})$) |
| $\mathbb{G}_T$ | Target group of the pairing $e : \mathbb{G}_1 \times \mathbb{G}_2 \to \mathbb{G}_T$ |
| $H_P$ | Poseidon hash function over $\mathbb{F}_r$ (parameters in Appendix A) |
| $H_S$ | SHA-256 hash function |
| $\|\|$ | Byte concatenation |
| $\lambda$ | Security parameter ($\lambda = 128$ for BLS12-381) |
| $\text{negl}(\lambda)$ | A function negligible in $\lambda$ |
| PPT | Probabilistic polynomial-time |
| $\textsf{Adv}^X_Y(A)$ | Advantage of adversary $A$ in game $X$ against scheme $Y$ |

### 1.3 Protocol Version

This analysis covers the **Variant B (Poseidon-only commitment)** of SEP-XXXX as implemented in the reference codebase. The circuit computes:

$$\text{commitment} = H_P(H_P(\text{root}, \text{epoch}), \text{salt})$$

The on-chain contract verifies Groth16 proofs over BLS12-381 using the Soroban BLS12-381 host functions. Variant A (SHA-256 outer commitment) is not analyzed here.

---

## 2. System Model

### Definition 2.1: Group State

A group state is a tuple $(\mathcal{G}, C, e, t)$ where:

- $\mathcal{G} = \{sk_1, \ldots, sk_n\} \subset \mathbb{F}_r$ is the member set (each member holds a BLS12-381 secret key).
- $C \in \mathbb{F}_r$ is the on-chain commitment.
- $e \in \mathbb{N}$ is the epoch (monotonically increasing, starting at 0).
- $t \in \{0, 1, 2\}$ is the tier, determining tree depth $d \in \{5, 8, 11\}$ and maximum membership $|\mathcal{G}| \leq 2^d$.

### Definition 2.2: Membership Predicate

The membership predicate $M(sk, \mathcal{G}, C, e) = 1$ if and only if:

1. $sk \in \mathcal{G}$
2. Let $\text{root}$ be the Poseidon Merkle root of $\mathcal{G}$ under canonical ordering (Definition 2.5).
3. There exists a salt $s \in \mathbb{F}_r$ such that $C = H_P(H_P(\text{root}, e), s)$.

### Definition 2.3: Adversary Model

We consider a PPT adversary $\mathcal{A}$ with oracle access to:

- $\textsf{CreateGroup}(\text{id}, C, t, \pi)$ — create a group with commitment $C$, tier $t$, and proof $\pi$.
- $\textsf{UpdateCommitment}(\text{id}, C', e', \pi)$ — update group to new commitment $C'$ at epoch $e'$.
- $\textsf{VerifyMembership}(\text{id}, \pi)$ — verify a membership proof (read-only).
- $\textsf{Corrupt}(i)$ — adaptively corrupt member $i$, revealing $sk_i$.
- $\textsf{Observe}(\text{relay})$ — observe encrypted events, timing, and topic tags on Nostr relays.

### Definition 2.4: Security Goals

We formalize five security games:

1. **Soundness** (Game $\textsf{SND}$): $\mathcal{A}$ wins if it produces an accepting proof $\pi$ for commitment $C$ without knowing any $sk \in \mathcal{G}$.
2. **Zero-Knowledge** (Game $\textsf{ZK}$): $\mathcal{A}$ wins if it can distinguish a real proof from a simulated one.
3. **Commitment Hiding** (Game $\textsf{HIDE}$): $\mathcal{A}$ wins if it can recover the Merkle root from the on-chain commitment.
4. **Epoch Integrity** (Game $\textsf{EPOCH}$): $\mathcal{A}$ wins if it can revert, skip, or fork the epoch sequence.
5. **Prover Privacy** (Game $\textsf{PRIV}$): $\mathcal{A}$ wins if it can determine which member produced a given proof.

### Definition 2.5: Canonical Member Ordering

Given member set $\mathcal{G} = \{sk_1, \ldots, sk_n\}$, define the canonical ordering as:

1. For each $sk_i$, compute the compressed BLS12-381 G1 public key: $pk_i = [sk_i] \cdot G_1 \in \mathbb{G}_1$, serialized to 48 bytes.
2. Sort members by lexicographic (big-endian) order of their 48-byte compressed public key representations.
3. Reject duplicate public keys (and thus duplicate secret keys, since $sk \mapsto [sk] \cdot G_1$ is injective for $sk \in \mathbb{F}_r \setminus \{0\}$).

This ensures that for any permutation of the same member set, the resulting Merkle tree is identical.

---

## 3. Cryptographic Assumptions

### Assumption 3.1: Poseidon Preimage Resistance

For the Poseidon hash function $H_P : \mathbb{F}_r^k \to \mathbb{F}_r$ instantiated with the parameters in Appendix A (width 3, 8 full rounds, 56 partial rounds, $\alpha = 5$ over BLS12-381 $\mathbb{F}_r$), for any PPT adversary $\mathcal{A}$:

$$\textsf{Adv}^{\text{Pre}}_{H_P}(\mathcal{A}) = \Pr[y \leftarrow H_P(x); x' \leftarrow \mathcal{A}(y) : H_P(x') = y] \leq \text{negl}(\lambda)$$

**Justification.** The Poseidon hash function was designed for ZK-friendliness while maintaining standard security margins. With 8 full rounds and 56 partial rounds at $\alpha = 5$, the design provides a security margin of approximately 2x over the minimum rounds required to resist known algebraic attacks (Grobner basis, interpolation, differential/linear). See Grassi et al., "Poseidon: A New Hash Function for Zero-Knowledge Proof Systems," USENIX Security 2021.

**Note on round constants.** This implementation derives round constants from the seed `"SEP-XXXX-Poseidon-BLS12-381-w3-f8-p56-a5-round-constants"` via iterated SHA-256, rather than using the reference Sage script. The derivation is deterministic and produces 192 field elements with no exploitable algebraic structure, preserving the security argument. See Appendix A for the complete derivation procedure.

### Assumption 3.2: Poseidon Collision Resistance

For any PPT adversary $\mathcal{A}$:

$$\textsf{Adv}^{\text{CR}}_{H_P}(\mathcal{A}) = \Pr[(x, x') \leftarrow \mathcal{A} : x \neq x' \wedge H_P(x) = H_P(x')] \leq \text{negl}(\lambda)$$

**Justification.** Follows from the same analysis as Assumption 3.1. Collision resistance is a weaker requirement than preimage resistance for random oracles but requires explicit verification for algebraic hash functions. The round parameters provide $\geq 128$-bit security against all known collision-finding attacks.

### Assumption 3.3: Groth16 Knowledge Soundness

For the Groth16 proof system (Groth, "On the Size of Pairing-Based Non-interactive Arguments," EUROCRYPT 2016) instantiated over BLS12-381, assuming the $q$-Strong Diffie-Hellman ($q$-SDH) assumption and the Knowledge of Exponent (KoE) assumption hold in $(\mathbb{G}_1, \mathbb{G}_2)$:

For any PPT prover $\mathcal{P}^*$ that produces an accepting proof $\pi$ with non-negligible probability, there exists a PPT extractor $\mathcal{E}$ that, given access to $\mathcal{P}^*$'s internal state, outputs a valid witness $w$ such that $R(x, w) = 1$ with overwhelming probability.

### Assumption 3.4: Groth16 Zero-Knowledge

The Groth16 proof system provides **perfect zero-knowledge**: there exists a simulator $\mathcal{S}$ that, given only the public inputs $x$ and the simulation trapdoor $\tau$, produces proofs that are identically distributed to honest proofs. That is, for all $x, w$ with $R(x, w) = 1$:

$$\{\pi \leftarrow \textsf{Prove}(\text{pk}, x, w)\} \equiv \{\pi \leftarrow \mathcal{S}(\tau, x)\}$$

### Assumption 3.5: BLS12-381 Pairing Security

The co-Computational Diffie-Hellman (co-CDH) and $q$-Strong Diffie-Hellman ($q$-SDH) assumptions hold for the BLS12-381 pairing $e : \mathbb{G}_1 \times \mathbb{G}_2 \to \mathbb{G}_T$. Specifically, the embedding degree is 12 and the target group order provides $\geq 128$-bit discrete logarithm security.

### Assumption 3.6: HKDF-SHA256 PRF Security

HKDF-SHA256 (RFC 5869) is a secure pseudorandom function (PRF) family. For any PPT distinguisher $\mathcal{D}$:

$$\textsf{Adv}^{\text{PRF}}_{\text{HKDF}}(\mathcal{D}) = |\Pr[\mathcal{D}^{\text{HKDF}(K, \cdot)} = 1] - \Pr[\mathcal{D}^{R(\cdot)} = 1]| \leq \text{negl}(\lambda)$$

where $K$ is a uniformly random key and $R$ is a truly random function.

### Assumption 3.7: AES-256-GCM Security

AES-256-GCM provides IND-CPA (indistinguishability under chosen plaintext attack) and INT-CTXT (integrity of ciphertexts) security when nonces are never reused. Specifically, for unique nonces:

$$\textsf{Adv}^{\text{IND-CPA}}_{\text{AES-GCM}}(\mathcal{A}) \leq \text{negl}(\lambda)$$
$$\textsf{Adv}^{\text{INT-CTXT}}_{\text{AES-GCM}}(\mathcal{A}) \leq \text{negl}(\lambda)$$

### Assumption 3.8: Trusted Setup Integrity

The Groth16 trusted setup ceremony for each circuit tier was performed with at least one honest participant among the $N$ contributors. Under the 1-of-$N$ honest participant assumption, the simulation trapdoor $\tau = (\alpha, \beta, \gamma, \delta, \{x^i\})$ is unknown to any single party, and the CRS is indistinguishable from one generated by a trusted dealer.

**Implementation note.** The reference implementation uses a multi-party computation (MPC) ceremony. Proving and verification keys are serialized and distributed as `keyset-v1/` bundles per tier.

---

## 4. Theorem 1: Merkle Tree Collision Resistance

### Statement

For any PPT adversary $\mathcal{A}$, the probability of finding two distinct canonically-ordered member sets $\mathcal{S}_1 \neq \mathcal{S}_2$ such that $\textsf{MerkleRoot}(\mathcal{S}_1) = \textsf{MerkleRoot}(\mathcal{S}_2)$ is negligible in $\lambda$, assuming Poseidon collision resistance (Assumption 3.2).

### Proof

**Construction.** Given canonically-ordered member set $\mathcal{S} = \{sk_1, \ldots, sk_n\}$ with $n \leq 2^d$, the Merkle tree $T$ of depth $d$ is constructed as follows:

1. Compute leaf hashes: $\ell_i = H_P(sk_i)$ for $i = 1, \ldots, n$.
2. Pad with zeros: $\ell_i = 0$ for $i = n+1, \ldots, 2^d$.
3. For each internal node at position $j$ (traversing bottom-up): $T[j] = H_P(T[2j], T[2j+1])$.
4. The root is $T[1]$.

**Reduction.** Suppose adversary $\mathcal{A}$ outputs $(\mathcal{S}_1, \mathcal{S}_2)$ with $\mathcal{S}_1 \neq \mathcal{S}_2$ and $\textsf{MerkleRoot}(\mathcal{S}_1) = \textsf{MerkleRoot}(\mathcal{S}_2)$. We construct adversary $\mathcal{B}$ that breaks Poseidon collision resistance:

1. $\mathcal{B}$ runs $\mathcal{A}$ to obtain $(\mathcal{S}_1, \mathcal{S}_2)$.
2. $\mathcal{B}$ builds both trees $T_1, T_2$.
3. Since $\mathcal{S}_1 \neq \mathcal{S}_2$, there exists at least one position $i$ where the leaf values differ. That is, either a member is present in one set but not the other, or the members at position $i$ differ after canonical ordering.
4. Starting from the root (where $T_1[1] = T_2[1]$ by assumption), $\mathcal{B}$ walks down the tree: at each internal node $j$ where $T_1[j] = T_2[j]$, examine children. Since the leaf layers differ, there must exist a node $j^*$ where:
   - $T_1[j^*] = T_2[j^*]$ (same hash output), but
   - $(T_1[2j^*], T_1[2j^* + 1]) \neq (T_2[2j^*], T_2[2j^* + 1])$ (different inputs).
5. This gives $\mathcal{B}$ a Poseidon collision: $H_P(a_1, b_1) = H_P(a_2, b_2)$ with $(a_1, b_1) \neq (a_2, b_2)$.

**Security bound.** The tree has at most $2^{d+1} - 1$ internal nodes. By the argument above, if the roots collide, at least one internal node witnesses a Poseidon collision. Therefore:

$$\textsf{Adv}^{\text{TreeCR}}(\mathcal{A}) \leq (2^{d+1} - 1) \cdot \textsf{Adv}^{\text{CR}}_{H_P}(\mathcal{B})$$

For $d = 11$ (maximum depth), this is $\leq 4095 \cdot \textsf{Adv}^{\text{CR}}_{H_P}(\mathcal{B})$, which remains negligible. $\square$

### Corollary 4.1: Canonical Injectivity

Distinct canonically-ordered member sets produce distinct Merkle roots with overwhelming probability. This follows directly from Theorem 1: if $\mathcal{S}_1 \neq \mathcal{S}_2$ then $\textsf{MerkleRoot}(\mathcal{S}_1) \neq \textsf{MerkleRoot}(\mathcal{S}_2)$ except with negligible probability.

### Remark 4.2: Zero-Padding Security

Empty leaf slots use $\ell = 0 \in \mathbb{F}_r$. An adversary could attempt to forge membership by finding $sk^*$ such that $H_P(sk^*) = 0$. This requires inverting Poseidon on 0, which contradicts Assumption 3.1. Additionally, the implementation verifies that $H_P(0) \neq 0$ (i.e., the zero element is not a fixed point of Poseidon under these parameters), ensuring that the zero field element cannot be used as a valid secret key to claim an empty slot.

---

## 5. Theorem 2: Commitment Hiding

### Statement

Given commitment $C = H_P(H_P(\text{root}, e), s)$ where $s$ is sampled uniformly at random from $\{0,1\}^{256}$ and reduced modulo $r$, no PPT adversary $\mathcal{A}$ that observes $(C, e)$ can recover $\text{root}$ with probability better than:

$$\textsf{Adv}^{\text{Pre}}_{H_P}(\mathcal{A}) + \frac{1}{|\mathbb{F}_r|}$$

### Proof

**Game 1: Salt hiding.** The commitment is computed as:

$$C = H_P(\underbrace{H_P(\text{root}, e)}_{h}, s)$$

where $h = H_P(\text{root}, e)$ is a field element and $s = \textsf{reduce}(\hat{s})$ for uniformly random $\hat{s} \leftarrow \{0,1\}^{256}$.

**Salt entropy.** Since $r < 2^{256}$, the reduction $\hat{s} \mapsto \hat{s} \mod r$ is almost uniform over $\mathbb{F}_r$. Specifically:

$$\lfloor 2^{256} / r \rfloor \leq 2$$

This means each element of $\mathbb{F}_r$ has at most 2 preimages under reduction, giving:

$$H_\infty(s) \geq \log_2(r) - 1 \geq 254 \text{ bits}$$

This provides at least 254 bits of min-entropy, far exceeding the 128-bit security level.

**Game 2: Preimage resistance.** Given $(C, e)$, recovering $\text{root}$ requires:

1. Finding $h$ such that there exists $s$ with $H_P(h, s) = C$. Since $s$ is unknown and has 254 bits of min-entropy, the adversary must either:
   - Guess $s$ (probability $\leq 1/|\mathbb{F}_r| \approx 2^{-255}$), or
   - Find a preimage of $C$ under $H_P(\cdot, \cdot)$, contradicting Assumption 3.1.
2. Even if $h$ is recovered, extracting $\text{root}$ from $h = H_P(\text{root}, e)$ requires a second preimage inversion (epoch $e$ is public but $\text{root}$ is not).

**Combining.** By a union bound:

$$\Pr[\mathcal{A} \text{ recovers root}] \leq \textsf{Adv}^{\text{Pre}}_{H_P}(\mathcal{A}) + \frac{1}{|\mathbb{F}_r|}$$

Both terms are negligible in $\lambda$. $\square$

### Corollary 5.1: Commitment Unlinkability

Two commitments $C_1 = H_P(H_P(\text{root}_1, e_1), s_1)$ and $C_2 = H_P(H_P(\text{root}_2, e_2), s_2)$ with independently sampled salts $s_1, s_2$ are computationally indistinguishable even if $\text{root}_1 = \text{root}_2$ (same member set, different epochs), because the fresh salt randomizes each commitment independently.

---

## 6. Theorem 3: ZK Membership Soundness

### Statement

If the Groth16 verification equation (Appendix B) accepts proof $\pi$ for public inputs $(C, e)$, then the prover knows a witness $(sk, \text{root}, s, \text{path}, \text{idx})$ satisfying all three circuit constraints, except with probability negligible in $\lambda$. Formally:

$$\Pr\left[\textsf{Verify}(\text{pvk}, \pi, (C, e)) = 1 \;\wedge\; \nexists\, w : R((C,e), w) = 1\right] \leq \text{negl}(\lambda)$$

### Proof

**Step 1: Knowledge extraction.** By the Groth16 knowledge soundness property (Assumption 3.3, Groth 2016, Theorem 1), for any PPT prover $\mathcal{P}^*$ that produces an accepting proof $\pi$ for statement $(C, e)$, there exists a PPT extractor $\mathcal{E}$ that outputs a witness $w = (sk, \text{root}, s, \text{path}, \text{idx})$ such that:

$$R((C, e), w) = 1$$

with overwhelming probability. Here $R$ is the relation defined by the circuit constraints.

**Step 2: Witness validity.** The extracted witness satisfies the three circuit constraints simultaneously:

**Constraint 1 — Key Ownership:**
$$\ell = H_P(sk)$$

The prover knows a secret key $sk \in \mathbb{F}_r$ whose Poseidon hash equals the leaf $\ell$. By Assumption 3.1 (preimage resistance), $sk$ is the unique preimage of $\ell$ with overwhelming probability.

**Constraint 2 — Merkle Membership:**
$$\textsf{MerkleVerify}(\ell, \text{path}, \text{idx}, \text{root}) = 1$$

Starting from $\ell$ at index $\text{idx}$, the path of sibling hashes leads to $\text{root}$:

$$\text{current}_0 = \ell$$
$$\text{current}_{i+1} = \begin{cases} H_P(\text{current}_i, \text{path}[i]) & \text{if } \text{idx}[i] = 0 \\ H_P(\text{path}[i], \text{current}_i) & \text{if } \text{idx}[i] = 1 \end{cases}$$
$$\text{current}_d = \text{root}$$

By Theorem 1, $\text{root}$ uniquely identifies a member set, and $\ell$ is at position $\text{idx}$ in that set's Merkle tree.

**Constraint 3 — Commitment Binding:**
$$H_P(H_P(\text{root}, e), s) = C$$

The root and epoch are bound to the on-chain commitment $C$ via the two-layer Poseidon hash with salt $s$.

**Step 3: Composition.** Combining the three constraints:

1. The prover knows $sk$ (Constraint 1).
2. $H_P(sk)$ is a leaf in the Merkle tree with root $\text{root}$ (Constraint 2).
3. This root is the one committed on-chain as $C$ at epoch $e$ (Constraint 3).
4. By Corollary 4.1, the root uniquely determines the member set.

**Therefore:** the prover is a member of the group committed on-chain at the stated epoch, and knows the corresponding secret key. $\square$

### Remark 6.1: Soundness Type

Groth16 provides **knowledge soundness** (also called **argument of knowledge**), which is stronger than plain soundness. Not only can a non-member not produce an accepting proof, but a valid proof guarantees that the prover *knows* a secret key — it is not merely an existence proof. This relies on the Knowledge of Exponent assumption (Assumption 3.3).

---

## 7. Theorem 4: Zero-Knowledge Property

### Statement

The proof $\pi$ reveals no information about the witness $(sk, \text{root}, s, \text{path}, \text{idx})$ beyond the public inputs $(C, e)$.

### Proof

By the Groth16 perfect zero-knowledge property (Assumption 3.4), there exists a simulator $\mathcal{S}$ that, given only the simulation trapdoor $\tau$ and public inputs $(C, e)$, produces proofs whose distribution is *identical* to honestly generated proofs:

$$\{\pi \leftarrow \textsf{Prove}(\text{pk}, (C,e), w)\} \equiv \{\pi \leftarrow \mathcal{S}(\tau, (C,e))\}$$

This is perfect (information-theoretic) zero-knowledge, not merely computational.

**Hidden quantities.** The following witness components are perfectly hidden:

| Component | What it reveals | Hidden by ZK |
|-----------|----------------|-------------|
| $sk$ | Which member produced the proof | Yes |
| $\text{idx}$ | Member's position in the tree | Yes |
| $\text{path}$ | Internal tree structure | Yes |
| $s$ | Group salt | Yes |
| $\text{root}$ | Merkle root | Yes |

**Observable quantities.** Only the public inputs are revealed:

| Component | What it reveals |
|-----------|----------------|
| $C$ | Opaque commitment (hidden by Theorem 2) |
| $e$ | Epoch number |

### Corollary 7.1: Constant-Size Proofs

A Groth16 proof over BLS12-381 consists of three group elements:

$$\pi = (\pi_A \in \mathbb{G}_1, \pi_B \in \mathbb{G}_2, \pi_C \in \mathbb{G}_1)$$

Compressed size: 48 + 96 + 48 = **192 bytes**, regardless of group size. This eliminates any side-channel on $|\mathcal{G}|$ from the proof itself.

### Corollary 7.2: Multi-Proof Unlinkability

Given two proofs $\pi_1, \pi_2$ for the same commitment $C$, a verifier cannot determine whether they were produced by the same member or different members. This follows directly from perfect zero-knowledge: both proofs are simulatable without any witness. $\square$

---

## 8. Theorem 5: Epoch Monotonicity and State Integrity

### Statement

Under the contract's epoch enforcement (`new_epoch == stored_epoch + 1`), no PPT adversary can:

(a) Revert to a previous epoch.
(b) Skip an epoch.
(c) Fork the group state (produce two distinct valid states at the same epoch).

### Proof

**Contract invariant.** The Soroban smart contract maintains, for each `group_id`, a single `CommitmentEntry`:

```
CommitmentEntry {
    commitment: BytesN<32>,
    epoch: u64,
    active: bool,
    timestamp: u64,
}
```

stored in persistent storage under key `DataKey::Group(group_id)`.

**Proof of (a): No epoch reversion.**

The `update_commitment` function enforces (contract line 479–481):

```
expected_epoch = current.epoch + 1
require(new_epoch == expected_epoch)
```

Since `new_epoch` must strictly equal `current.epoch + 1`, any attempt to set `new_epoch < current.epoch` or `new_epoch == current.epoch` is rejected. The `checked_add` prevents overflow of the u64 epoch counter.

**Proof of (b): No epoch skipping.**

The same strict equality check `new_epoch == current.epoch + 1` prevents skipping. If the current epoch is $e$, the only accepted next epoch is $e + 1$, not $e + 2$ or any larger value.

**Proof of (c): No state forking.**

Suppose two conflicting updates attempt to transition from epoch $e$ to epoch $e + 1$ with different commitments $C'_1 \neq C'_2$. Both updates require:

1. A valid proof against the *current* commitment $(C_e, e)$ (contract line 483–486):
   ```
   require(public_inputs.commitment == current.commitment)
   require(public_inputs.epoch == current.epoch)
   ```
2. The proof must pass Groth16 verification.
3. The proof must not be replayed (Theorem 6).

Since the contract stores exactly one `CommitmentEntry` per `group_id`, and Stellar's transaction ordering is deterministic within a ledger, exactly one of the two competing transactions will execute first — updating the stored entry to $(C'_1, e+1)$. The second transaction then fails because `current.epoch` is now $e + 1$, not $e$.

**Inductive argument.** By induction on epoch:

- **Base case** ($e = 0$): The group is created via `create_group`, which requires `public_inputs.epoch == 0` (contract line 384) and a valid membership proof. Only a member of the initial set can create the group.
- **Inductive step** ($e \to e + 1$): By the induction hypothesis, at epoch $e$, the stored commitment $C_e$ faithfully represents a specific member set. The `update_commitment` function verifies a proof against $(C_e, e)$ — proving the updater is a member at epoch $e$ — before storing $(C_{e+1}, e+1)$.

**Therefore:** at every epoch, the on-chain state reflects a valid transition authorized by a current member. $\square$

### Remark 8.1: Deactivation Permanence

The `deactivate_group` function sets `active = false` irreversibly (contract line 602). Subsequent calls to `update_commitment` are rejected for inactive groups. The group state is frozen at its final epoch. `verify_membership` remains available for historical verification.

---

## 9. Theorem 6: Proof Non-Replayability

### Statement

A proof $\pi$ accepted for one state-changing contract call (`create_group`, `update_commitment`, or `deactivate_group`) cannot be replayed to any other state-changing call.

### Proof

**Proof hash construction.** For each submitted proof $\pi = (\pi_A, \pi_B, \pi_C)$, the contract computes (contract line 715–722):

$$h_\pi = H_S(\pi_A \| \pi_B \| \pi_C)$$

where $\pi_A, \pi_B, \pi_C$ are serialized in uncompressed format (96 + 192 + 96 = 384 bytes total).

**Recording.** After successful verification in any state-changing function, the contract stores $h_\pi$ in persistent storage under key `DataKey::UsedProof(h_\pi)` (contract line 738–745).

**Checking.** Before verification, every state-changing function calls `check_proof_replay` (contract line 726–736), which returns an error if $h_\pi$ is already stored.

**Cross-function replay prevention.** The hash $h_\pi$ is independent of which function accepted the proof. A proof accepted by `create_group` is recorded, and any subsequent submission to `update_commitment` or `deactivate_group` with the same $\pi$ will be rejected.

**Cross-group replay prevention.** Even if an adversary submits the same proof to a different group, the proof is bound to specific public inputs $(C, e)$. For a different group with commitment $C' \neq C$, the Groth16 verification equation would fail because the proof was generated for $(C, e)$, not $(C', e')$. Additionally, the proof hash check provides a second layer of defense.

**Hash collision resistance.** Two distinct proofs $\pi \neq \pi'$ producing the same hash $h_\pi = h_{\pi'}$ would constitute a SHA-256 collision, which contradicts the collision resistance of SHA-256.

**Read-only exception.** The `verify_membership` function does not record the proof hash, allowing the same proof to be verified multiple times without state changes. This is safe because `verify_membership` is read-only and does not modify group state.

**Therefore:** each proof can be used in at most one state-changing contract call. $\square$

---

## 10. Theorem 7: Fee-Payer Privacy

### Statement

If a relayer submits transactions on behalf of group members, the on-chain record reveals only the relayer's Stellar account — not the identity of the prover.

### Proof

**Transaction structure.** A Stellar/Soroban transaction contains:

- `source_account`: the fee-paying account (the relayer).
- `operations`: the contract invocation with proof and public inputs.
- `fee`: transaction fee in XLM.
- `signature`: Ed25519 signature by `source_account`.

None of these fields contain the prover's identity.

**Contract authorization model.** The `update_commitment` function accepts parameters:

```
(group_id, new_commitment, new_epoch, proof, public_inputs)
```

Notably absent: any `caller: Address` or `source: Address` parameter. The contract does not inspect `env.invoker()` or any caller identity. Authorization is **proof-only**: the zero-knowledge proof serves as the sole authorization mechanism.

**Proof zero-knowledge.** By Theorem 4, the proof $\pi$ reveals nothing about the prover's identity. The verifier (contract) learns only that *some* member of the committed group produced the proof — not *which* member.

**Formal bound.** The adversary's advantage in identifying the prover decomposes as:

$$\textsf{Adv}^{\text{Identify}}(\mathcal{A}) = \textsf{Adv}^{\text{ZK}}(\mathcal{A}) + \textsf{Adv}^{\text{Network}}(\mathcal{A})$$

where:

- $\textsf{Adv}^{\text{ZK}}(\mathcal{A}) \leq \text{negl}(\lambda)$ by Groth16 perfect zero-knowledge.
- $\textsf{Adv}^{\text{Network}}(\mathcal{A})$ captures timing correlation and IP-based deanonymization at the relayer's HTTPS endpoint — this is a network-layer concern outside the scope of the cryptographic protocol.

**Residual leakage.** If the same client IP repeatedly submits proofs to the relayer, traffic analysis may correlate submissions. Mitigations include:

- Using Tor or VPN to connect to the relayer.
- Batching multiple users' proofs through a single relayer endpoint.
- Operating multiple relayer instances.

These are network-layer defenses and are not formally analyzed here. $\square$

---

## 11. Theorem 8: Symmetric Member Removal Impossibility

### Statement

*(Reproduced from Security Audit v4)*

No modification to the symmetric-key-only key derivation protocol can provide cryptographic eviction of a removed member who retains the group secret. Formally:

For any key derivation function $\textsf{KDF}$ that computes the next traffic key as a deterministic function of symmetric shared state:

$$K_{e+1} = \textsf{KDF}(\text{groupSecret}, e+1, s_{e+1}, \text{aux})$$

where $\text{aux}$ is any additional data derivable from the symmetric shared state, a removed member $M_r$ who knows $(\text{groupSecret}, K_e)$ can compute $K_{e+1}$.

### Proof

**Setup.** Consider a group with members $\{M_1, \ldots, M_n\}$ sharing symmetric state:

- Long-lived group secret: $\text{groupSecret}$
- Current traffic key: $K_e = \textsf{HKDF}(\text{groupSecret} \| e \| s_e)$
- Current epoch: $e$, current salt: $s_e$

Member $M_r$ is removed at epoch $e$. The remaining members $\{M_1, \ldots, M_{n-1}\}$ wish to advance to epoch $e + 1$ such that $M_r$ cannot derive $K_{e+1}$.

**Observation 1: State update disclosure.** The epoch transition message (announcing the removal) must be encrypted under $K_e$ (the current traffic key) and sent on the group channel. This message contains, at minimum, the new epoch $e + 1$ and the new salt $s_{e+1}$ (needed for all remaining members to derive $K_{e+1}$).

**Observation 2: Removed member's knowledge.** At the time of removal, $M_r$ possesses:

1. $\text{groupSecret}$ — the long-lived symmetric secret.
2. $K_e$ — the current traffic key (used to decrypt the removal message).
3. The removal message itself — which $M_r$ can decrypt using $K_e$, obtaining $e + 1$ and $s_{e+1}$.
4. The member roster (before and after removal) — observable from the removal announcement.

**Observation 3: Key derivation is deterministic.** The next traffic key is computed as:

$$K_{e+1} = \textsf{HKDF}(\text{groupSecret} \| (e+1) \| s_{e+1})$$

All three inputs are known to $M_r$:
- $\text{groupSecret}$: retained from membership.
- $e + 1$: from the removal message.
- $s_{e+1}$: from the removal message (encrypted under $K_e$, which $M_r$ knows).

**Generalization.** This argument applies to any deterministic function of symmetric shared state. Consider the six natural "fix" attempts:

1. *Omit salt from removal update* — remaining members cannot derive $K_{e+1}$ either.
2. *Derive salt deterministically from groupSecret and epoch* — $M_r$ can compute the same derivation.
3. *Use a second re-key message under a transition key* — if the transition key is derived from $K_e$ or groupSecret, $M_r$ derives it too.
4. *Replace random salt with a function of member set* — member set is public after removal announcement.
5. *Chain multiple KDF rounds* — all intermediate values are computable from the same inputs.
6. *Use epoch-dependent KDF with removal flag* — the flag is observable; the KDF remains deterministic from known inputs.

**Formal principle.** Any function $f$ of values $(v_1, \ldots, v_k)$ all known to $M_r$ is computable by $M_r$. There is no "hidden asymmetry" in symmetric shared state.

### Corollary 11.1: Necessity of Asymmetric Rekey

Secure member removal requires introducing a secret that is delivered *only* to the remaining members and *not* derivable from any state known to the removed member. The necessary protocol extension is:

1. Generate fresh $\text{groupSecret}'$ and $s_{e+1}$.
2. Encrypt $(\text{groupSecret}', s_{e+1})$ individually to each remaining member's X25519 inbox public key.
3. Each remaining member decrypts their envelope, derives $K_{e+1} = \textsf{HKDF}(\text{groupSecret}' \| (e+1) \| s_{e+1})$.
4. $M_r$ cannot decrypt any envelope (lacking the remaining members' X25519 private keys).

This transforms the symmetric protocol into a hybrid symmetric-asymmetric one, where the asymmetric component (X25519 per-recipient encryption) provides the necessary key separation. $\square$

---

## 12. Overall System Soundness

### Statement

Composing Theorems 1–8, the SEP-XXXX protocol provides the following security guarantees:

### 12.1 Membership Soundness

*Only holders of a secret key in the committed member set can produce an accepting proof.*

**Proof chain:** By Theorem 3 (ZK Membership Soundness), an accepting proof implies knowledge of a witness $(sk, \text{root}, s, \text{path}, \text{idx})$ satisfying all three circuit constraints. By Theorem 1 (Merkle Collision Resistance), the root uniquely determines the member set. Therefore, $sk$ belongs to the committed member set.

### 12.2 Membership Privacy

*A verifier learns nothing about the prover's identity beyond the fact that they are a group member.*

**Proof chain:** By Theorem 4 (Zero-Knowledge Property), the proof reveals no information about the witness. By Corollary 7.1, the proof size is constant (192 bytes) regardless of group size. By Corollary 7.2, multiple proofs by the same member are unlinkable.

### 12.3 State Integrity

*Group state advances correctly and cannot be forked or reverted.*

**Proof chain:** By Theorem 5 (Epoch Monotonicity), the epoch sequence is strictly increasing and non-forkable. By Theorem 6 (Proof Non-Replayability), each proof can authorize at most one state transition. Combined, these ensure that the on-chain state reflects a valid, linear sequence of member-authorized transitions.

### 12.4 Commitment Confidentiality

*On-chain commitments reveal no information about group membership.*

**Proof chain:** By Theorem 2 (Commitment Hiding), the commitment $C = H_P(H_P(\text{root}, e), s)$ hides the Merkle root under Poseidon preimage resistance and salt entropy. By Corollary 5.1, commitments at different epochs are unlinkable even for the same member set.

### 12.5 Fee-Payer Unlinkability

*The transaction submitter is decoupled from the prover.*

**Proof chain:** By Theorem 7 (Fee-Payer Privacy), the relayer pattern ensures that the on-chain record contains only the relayer's account. Combined with Theorem 4 (Zero-Knowledge), the proof itself reveals nothing about the prover.

### Acknowledged Limitations

The following properties are **not** established by this proof:

1. **Post-compromise forward secrecy after member removal** — By Theorem 8, the current symmetric protocol cannot cryptographically evict removed members. Secure removal requires the asymmetric rekey extension (Corollary 11.1).

2. **Group size confidentiality** — The tier $t \in \{0, 1, 2\}$ is stored on-chain and reveals an upper bound on group size ($2^5$, $2^8$, or $2^{11}$ members). This is an information-theoretic leakage that cannot be eliminated without a universal circuit.

3. **Network-layer privacy** — Traffic analysis on Nostr relay connections (IP addresses, connection timing, topic subscription patterns) may reveal information about group membership or activity patterns. This is outside the scope of the cryptographic protocol.

4. **Trusted setup compromise** — If all $N$ participants in the MPC ceremony are dishonest, the simulation trapdoor is known and soundness is lost (the adversary could forge proofs). Assumption 3.8 bounds this risk.

---

## Appendix A: Poseidon Parameters

### Field

BLS12-381 scalar field $\mathbb{F}_r$ where:

$$r = \texttt{0x73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001}$$

$$r \approx 2^{255}$$

### Hash Configuration

| Parameter | Value |
|-----------|-------|
| Width ($w$) | 3 |
| Rate ($\rho$) | 2 |
| Capacity ($c$) | 1 |
| Full rounds ($R_f$) | 8 |
| Partial rounds ($R_p$) | 56 |
| Total rounds | 64 |
| S-box ($\alpha$) | $x^5$ |

### Round Constant Derivation

The 192 round constants are derived deterministically:

```
seed_0 = SHA-256("SEP-XXXX-Poseidon-BLS12-381-w3-f8-p56-a5-round-constants")

For i = 0, 1, ..., 191:
    extended[0..32]  = seed
    seed = SHA-256(seed)
    extended[32..64] = seed
    seed = SHA-256(seed)
    constant_i = Fr::from_le_bytes_mod_order(extended[0..64])
```

The 192 constants are arranged as 64 rounds of 3 constants each: `ark[round][position]`.

### MDS Matrix Construction (Cauchy)

The $3 \times 3$ MDS matrix $M$ is constructed as:

$$M[i][j] = \frac{1}{x_i + y_j}$$

where $x_i = i + 1$ and $y_j = w + j + 1$ for $i, j \in \{0, 1, 2\}$:

$$M = \begin{pmatrix} (1+4)^{-1} & (1+5)^{-1} & (1+6)^{-1} \\ (2+4)^{-1} & (2+5)^{-1} & (2+6)^{-1} \\ (3+4)^{-1} & (3+5)^{-1} & (3+6)^{-1} \end{pmatrix} = \begin{pmatrix} 5^{-1} & 6^{-1} & 7^{-1} \\ 6^{-1} & 7^{-1} & 8^{-1} \\ 7^{-1} & 8^{-1} & 9^{-1} \end{pmatrix}$$

All inverses are computed in $\mathbb{F}_r$. The Cauchy construction guarantees that every square submatrix of $M$ is invertible (MDS property), which is essential for the diffusion layer's security.

### Security Margins

Per Grassi et al., the minimum rounds to resist known attacks are approximately:

- Algebraic attacks (Grobner basis): $\sim 6$ full rounds
- Interpolation attacks: $\sim 4$ full rounds
- Differential/linear: $\sim 6$ full rounds

With 8 full rounds and 56 partial rounds, the design provides approximately a 2x safety margin.

### Function Definitions

**Single-input hash:**

$$H_P^{(1)}(x) : \text{Absorb } x \text{ into sponge, squeeze 1 field element}$$

**Two-input hash:**

$$H_P^{(2)}(x, y) : \text{Absorb } x, \text{ then } y, \text{ squeeze 1 field element}$$

Both use the same Poseidon permutation with the parameters above.

---

## Appendix B: Groth16 Verification Equation

### Pairing Equation

The Groth16 verification equation checks:

$$e(-\pi_A, \pi_B) \cdot e(\alpha, \beta) \cdot e(\text{vk}_x, \gamma) \cdot e(\pi_C, \delta) = 1_{\mathbb{G}_T}$$

where the public-input-dependent component $\text{vk}_x$ is computed as:

$$\text{vk}_x = \text{IC}[0] + C \cdot \text{IC}[1] + e \cdot \text{IC}[2]$$

with:
- $C \in \mathbb{F}_r$: the commitment (public input 1)
- $e \in \mathbb{F}_r$: the epoch, encoded as $\textsf{Fr::from}(e_{\text{u64}})$ (public input 2)
- $\text{IC}[0], \text{IC}[1], \text{IC}[2] \in \mathbb{G}_1$: the verification key's input commitment points

### Verification Key Structure

| Component | Group | Serialized Size |
|-----------|-------|----------------|
| $\alpha$ | $\mathbb{G}_1$ | 96 bytes (uncompressed) |
| $\beta$ | $\mathbb{G}_2$ | 192 bytes (uncompressed) |
| $\gamma$ | $\mathbb{G}_2$ | 192 bytes (uncompressed) |
| $\delta$ | $\mathbb{G}_2$ | 192 bytes (uncompressed) |
| $\text{IC}[0..2]$ | $\mathbb{G}_1^3$ | 3 × 96 bytes (uncompressed) |

### Canonical Input Validation

The contract validates that public inputs are canonical field elements (contract line 802–808):

```
commitment_fr = Fr::from_bytes(commitment)
canonical_bytes = commitment_fr.to_bytes()
require(canonical_bytes == commitment)
```

This ensures that commitment bytes represent a valid element of $\mathbb{F}_r$ (i.e., the integer value is less than $r$), preventing malleability attacks from non-canonical encodings.

### Epoch Encoding

The epoch (a u64 value) is converted to a 256-bit big-endian representation and then to a field element:

```
epoch_bytes = u64_to_u256_be(epoch)    // pad u64 to 32 bytes
epoch_fr = Fr::from_u256(epoch_bytes)  // interpret as field element
```

Since all u64 values are less than $r \approx 2^{255}$, this conversion is always canonical.

---

## Appendix C: Contract Verification Pseudocode

The following pseudocode describes the on-chain verification logic extracted from the Soroban smart contract (`contracts/sep-xxxx/src/lib.rs`).

### create_group

```
function create_group(group_id, commitment, tier, proof, public_inputs):
    require(tier <= 2)
    require(!storage.has(Group(group_id)))
    require(public_inputs.commitment == commitment)
    require(public_inputs.epoch == 0)
    
    check_proof_replay(proof)
    
    vk = load_vk(tier)
    require(verify_groth16_proof(vk, proof, commitment, 0))
    
    record_proof(proof)
    
    entry = CommitmentEntry {
        commitment: commitment,
        epoch: 0,
        active: true,
        timestamp: now()
    }
    storage.set(Group(group_id), entry)
    
    count = tier_group_count(tier)
    require(count < MAX_GROUPS_PER_TIER)  // 10,000
    set_tier_group_count(tier, count + 1)
```

### update_commitment

```
function update_commitment(group_id, new_commitment, new_epoch, proof, public_inputs):
    current = storage.get(Group(group_id))
    require(current.active)
    require(new_epoch == current.epoch + 1)
    require(public_inputs.commitment == current.commitment)
    require(public_inputs.epoch == current.epoch)
    
    check_proof_replay(proof)
    
    tier = storage.get(GroupTier(group_id))
    vk = load_vk(tier)
    require(verify_groth16_proof(vk, proof, current.commitment, current.epoch))
    
    record_proof(proof)
    
    current.commitment = new_commitment
    current.epoch = new_epoch
    current.timestamp = now()
    storage.set(Group(group_id), current)
```

### deactivate_group

```
function deactivate_group(group_id, proof, public_inputs):
    current = storage.get(Group(group_id))
    require(current.active)
    require(public_inputs.commitment == current.commitment)
    require(public_inputs.epoch == current.epoch)
    
    check_proof_replay(proof)
    
    tier = storage.get(GroupTier(group_id))
    vk = load_vk(tier)
    require(verify_groth16_proof(vk, proof, current.commitment, current.epoch))
    
    record_proof(proof)
    
    current.active = false
    storage.set(Group(group_id), current)
    
    count = tier_group_count(tier)
    set_tier_group_count(tier, count - 1)
```

### proof_hash

```
function proof_hash(proof):
    preimage = proof.a || proof.b || proof.c    // 96 + 192 + 96 = 384 bytes
    return SHA-256(preimage)

function check_proof_replay(proof):
    h = proof_hash(proof)
    require(!storage.has(UsedProof(h)))

function record_proof(proof):
    h = proof_hash(proof)
    storage.set(UsedProof(h), true)
```

---

## References

1. Groth, J. "On the Size of Pairing-Based Non-interactive Arguments." *EUROCRYPT 2016*. Lecture Notes in Computer Science, vol. 9666.

2. Grassi, L., Khovratovich, D., Rechberger, C., Roy, A., Schofnegger, M. "Poseidon: A New Hash Function for Zero-Knowledge Proof Systems." *USENIX Security 2021*.

3. Boneh, D., Bünz, B., Fisch, B. "A Survey of Two Verifiable Delay Functions." *Cryptology ePrint Archive*, Report 2018/712.

4. Bowe, S., Grigg, J., Hopwood, D. "Halo: Recursive Proof Composition without a Trusted Setup." *Cryptology ePrint Archive*, Report 2019/1021.

5. RFC 5869: HMAC-based Extract-and-Expand Key Derivation Function (HKDF).

6. NIST SP 800-38D: Recommendation for Block Cipher Modes of Operation: Galois/Counter Mode (GCM) and GMAC.

7. arkworks contributors. "arkworks: An Ecosystem for zkSNARKs." https://arkworks.rs

8. Stellar Development Foundation. "Soroban Smart Contracts." https://soroban.stellar.org
