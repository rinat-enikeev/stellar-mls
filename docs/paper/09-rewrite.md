---
title: "Metadata Without Observers: A Postmortem-Driven Case Study of the Anarchy Group Type on a Public Ledger"
author: Rinat Enikeev
venue: IEEE Security & Privacy magazine (feature article)
status: Revision scoping the article to the Anarchy governance type (addresses the mismatch between 08-rewrite.md and the governance-types schema merged as PR #77)
date: 2026-04-19
word_count_target: 5,800
---

# Metadata Without Observers: A Postmortem-Driven Case Study of the Anarchy Group Type on a Public Ledger

## Abstract

End-to-end encryption closed the content half of the online-communication privacy problem. The metadata half — *who is in a group with whom* — is still leaking to at least one operator in almost every deployed messaging system. We describe Onym, a testnet system that anchors each group's state on the Stellar blockchain as a 32-byte commitment and gates every state change on a Groth16 zero-knowledge proof over BLS12-381. The deployed contract supports four *governance types* — **Anarchy**, 1v1, Democracy, and Oligarchy — that trade off on-chain metadata against authorization strength. **This article is about the Anarchy type**: the oldest, lightest, and most metadata-minimising of the four. Under Anarchy, any current member may authorise any next state with a single membership proof; no member address, updater identity, or group-size signal beyond a coarse tier appears on the ledger. The article has two parts. The first introduces the construction in plain language and formally, with relations, IC binding, and game-based security statements, all scoped to Anarchy. The second reports four postmortems — all drawn from the Anarchy development history — documenting a constraint-cost crisis, a critical authorization vulnerability, a transport-layer co-membership leak, and an unresolved trusted-setup dependency. Onym is not a mainnet system and does not yet have a production customer. This is a case study in what it took to build toward those properties, and in the specific ways the work kept going wrong.

---

## 1. The metadata problem for online communication

The last decade made content-level encryption a commodity. Signal's Double Ratchet, Apple's iMessage, WhatsApp, Google Messages' RCS-with-MLS rollout, and Matrix's Olm/Megolm all provide some form of end-to-end content encryption. None of them hide metadata from the operator. iCloud backups hold message metadata for *every* Apple user. WhatsApp provides "basic subscriber records" and message metadata under stored-communications-act orders. Matrix homeservers see every room's membership list by construction. Telegram "secret chats" leave groups, channels, and cloud chats server-readable. Signal is the outlier: repeated grand-jury subpoenas (2016 Virginia, 2020 California, 2021 Santa Clara) have forced Signal to disclose only registration time and last-seen date, and its transparency reports substantiate that list is exhaustive. That is a deliberate and unusual engineering achievement. It is not the industry norm.

Metadata is not a secondary concern. Two of the largest law-enforcement operations of the last five years — the 2020 EncroChat takedown (~60,000 users) and the 2021 Sky ECC operation (~170,000 users) — recovered content from compromised devices, but the *targeting* was metadata-driven: which devices were talking to which, at what rate, in what clusters. A former NSA General Counsel's remark — "we kill people based on metadata" [12] — is a statement about operational reality.

These examples establish that metadata matters. They do not, by themselves, establish that Onym's specific architecture is a warranted response — EncroChat and Sky ECC were compromised through device infiltration and server compromise, not through a Delivery Service metadata leak. We are aware of no public incident in which a DS operator was compelled to disclose group-membership metadata, or in which a federated homeserver's membership logs were exploited for intelligence targeting. Onym's threat model is therefore *prospective*, not retrospective: no deployed DS has been publicly compromised for group-membership metadata yet, and we want an architecture that makes the question moot before the precedent exists. The engineering lessons in this paper hold regardless of whether that precedent arrives.

The structural cause of the metadata exposure is that every messaging architecture we know of has at least one operator positioned to observe the metadata. In centralised systems it is the server operator. In federated systems (XMPP, Matrix, email) every federated server learns the metadata of its tenants. In MLS deployments the Delivery Service is the ordering authority and therefore the metadata authority — it sees each Commit, each Welcome, and which device-pseudonyms are present in each group. MLS's specification [1] is explicit that the Delivery Service is trusted for ordering; most deployments (Google Messages, Cisco Webex, Wire) fold that role into a single server.

This paper is not a proposal that the industry replace these deployed systems. Signal's Private Group System [11], which uses anonymous credentials so that Signal servers cannot see group membership, is an existence proof that strong metadata privacy is compatible with a commercial messenger at scale. The question Onym asks is narrower: **can we build a group-membership registry where no operator — not even the anchor operator — is structurally positioned to observe the metadata, and where the privacy model is externally auditable from the public spec and the published bytecode?** Signal is the gold standard within a trust-the-operator frame; Onym explores the trust-nobody frame. Both have audiences.

## 2. A candidate solution, and why blockchain + SNARKs

There are four honest candidate approaches to a metadata-hiding group-membership registry. Each has a real weakness.

1. **Transparency logs** in the Certificate Transparency / Key Transparency [13] style give every observer the same append-only view and are mathematically elegant. They hide content well. They do not hide who is writing to which key, which means they do not hide co-membership in a group-registry setting.
2. **Federated delivery** (Matrix's federated homeservers, XMPP, email) distributes trust. It does not eliminate the per-server metadata observation. "Distribute trust" is weaker than "require no operator."
3. **Anonymous-credential-backed central servers** (Signal's Private Group System [11]) are deployed, scalable, and give excellent metadata privacy under an honest-but-curious server model. The trust assumption is that the Signal server executes the oblivious protocol faithfully. Signal's clients are open-source, the server protocol is published, and the anonymous-credential scheme has been formally analysed [11]. The auditability gap between Signal PGS and Onym is real but specific: a third party auditing Onym can verify, from the public chain state and the published contract bytecode alone, that every state transition was authorised by a valid SNARK — without trusting any server binary to match any spec. For Signal PGS, the equivalent verification requires trusting that the running server binary faithfully implements the published protocol. This is a meaningful but niche requirement: it matters in threat models where the operator is adversarial or legally compelled, not in models where the operator is honest-but-curious.
4. **Public-ledger anchoring plus zero-knowledge membership proofs** commits to group state on a public chain and verifies membership with a SNARK. Every observer reads the same chain; no operator is positioned to observe more. The trust assumptions that remain are on the chain's censorship resistance, on the SNARK's soundness, and on the SNARK's trusted setup when one is required.

The fourth approach is the one Onym takes. The reasons to take it are narrow: we want an architecture in which a technical reader can verify the metadata claims from the public chain state, the deployed verifier bytecode, and the open-source client, without any privileged information. The reasons to *not* take it — in particular, the trusted-setup requirement, the block-producer timing side channel, and the fee-payer correlation — are discussed in Sec. 5 and Sec. 7. This paper treats approach 4 as one candidate worth reporting on, not as the uniquely correct answer.

Within approach 4, we chose Stellar over Ethereum because Stellar Protocol 22 [17] introduced BLS12-381 host functions as a CAP-0046-12 feature in 2024, which reduced Groth16 verification to three pairings and a small multi-scalar multiplication at negligible fee cost. Ethereum has more ZK-tooling momentum (BN254 pre-compiles, several rollup-level verifiers, mature dev stacks), but BN254's effective security dropped to an estimated 100–103 bits after improved number-field-sieve attacks [15]. BLS12-381 is at an estimated 120 bits of pairing security. Neither curve meets a strict 128-bit target; both are below the line. Our actual threshold is 120 bits — the point at which the concrete cost of a discrete-log attack on the pairing exceeds $2^{100}$ group operations and remains infeasible for any foreseeable adversary. BLS12-381 meets this threshold; BN254, at 100–103 bits, does not, because the gap between 100 and 120 bits represents a factor of roughly $2^{17}$–$2^{20}$ in attack cost. The choice is defensible on that basis. The cost — a younger smart-contract platform and a smaller ZK-dev ecosystem — is real.

## 3. Governance types and the scope of this article

Onym's deployed contract supports four *governance types*, each defining a different on-chain authorization policy for state changes. Each group declares its type at creation time and cannot change it. The four types trade on-chain metadata exposure against authorization strength, and the engineering lessons in this article are scoped to exactly one of them.

**Table 1.** Governance types supported by the deployed Onym contract.

| Type | Code | Who may authorise `update_commitment` | Extra public inputs | Extra on-chain state | Intended use |
|------|------|---------------------------------------|----------------------|-----------------------|--------------|
| **Anarchy** | 0 | Any current member, alone | $(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}, C_{\mathsf{new}})$ | None beyond tier | Default. This article. |
| 1v1 | 1 | N/A — member rekey disabled | — | `member_count = 2` (constant) | Pair chats. Deactivation only. |
| Democracy | 2 | A $K$-of-$n$ quorum with $2K \geq n$, signing a single-leaf delta | $(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}, C_{\mathsf{new}}, m_{\mathsf{old}}, m_{\mathsf{new}})$ | Exact `member_count` | Community-governed rooms. |
| Oligarchy | 3 | An admin proves against a separate admin-tree commitment; admin-set rotation is a distinct operation | $(\mathsf{admin}\text{-}C_{\mathsf{old}}, \mathsf{admin}\text{-}\epsilon_{\mathsf{old}}, \mathsf{admin}\text{-}C_{\mathsf{new}})$ | Admin root, admin epoch | Broadcast channels, enterprise. |

All four types share the same commitment definition, the same 32-byte commitment storage slot, the same membership-proof relation $R_{\mathsf{Mem}}$, and the same deactivation operation. They differ only in what additional public inputs and stored state the update path requires.

**This article is about Anarchy.** We use "Anarchy" in its political-theory sense rather than its pejorative one: the absence of a ruling authority, not the absence of order. An Anarchy-typed group has no on-chain role that distinguishes one member from another. No member is an admin, a moderator, a quorum leader, or a delegate. Every current member is an equal peer with the unilateral right to authorise any next state of the group. The social or application-layer protocol — not the circuit — is responsible for coordinating benign transitions. Anarchy is the lightest of the four types: it stores no governance metadata beyond what every group stores (tier, epoch, timestamp), and its update relation $R_{\mathsf{Upd}}$ has exactly the three public inputs listed above.

The other three types are described in the companion design document [19] and are out of scope for this article except insofar as their existence means readers must not generalise Anarchy's privacy properties to them. In particular, Democracy groups publish the exact member count and a ±1 direction-of-change signal on every update, and Oligarchy groups publish an independent admin-epoch counter — neither of which Anarchy does. When this article says "no group-size signal beyond a coarse tier appears on the ledger," we mean it for Anarchy-typed groups. Applications that need Democracy's authorisation strength accept its metadata cost as a deliberate tradeoff; applications that want Anarchy's metadata minimalism accept its authorisation laxity.

The four postmortems in Sec. 5 all predate the governance-types schema (the Onym contract shipped Anarchy-only from v1.0.0 through v1.6.7; the four-type schema shipped as PR #77 in v1.7.0 on 2026-04-19). Every design mistake and every fix in those postmortems therefore concerns the Anarchy path, which is the path the article is about.

## 4. Onym Anarchy, in plain language

Picture a town hall that must keep a membership register for many private clubs, and picture the town's constitution prohibits the hall from knowing who is in any club. Instead of writing names, the hall posts one sealed envelope per club. The envelope is a single line of text, so short that it reveals nothing. A member of the club who walks up to the desk can prove they belong inside the envelope without anyone opening it — they hand over a small slip of paper, the desk staff perform a fixed arithmetic check against the slip and the envelope, and the staff return "yes" or "no." No staff member ever learns who the member is.

When the club's membership changes — someone is added, removed, or rotated — one current member computes what the club's *next* envelope should say, writes both the old envelope and the new envelope on the slip, proves "I belong inside yesterday's envelope, and today's envelope is the correct next version of it," and hands the slip over. The hall swaps envelopes. Nothing on the hall's wall ever named a member.

In the Anarchy model, *any* current member can perform that swap alone, and the hall has no way to tell which member did so. Anarchy clubs have no officers, no bylaws, and no second signatures. The arithmetic check does not care whether the new envelope represents an addition, a removal, a rotation, or a complete change of every name inside — it only checks that the requester was in yesterday's envelope.

The real architecture follows that picture literally:

- The *hall* is the Stellar blockchain. Anyone in the world can read every envelope, which is just a 32-byte number per group.
- The *envelope* is a cryptographic commitment: an opaque hash-tree of the member set, bound to an epoch counter and a random salt.
- The *slip of paper* is a 192-byte Groth16 proof.
- The *arithmetic check* is three pairings on BLS12-381 — a bounded, fee-predictable on-chain operation.
- The *member's secret* is a BLS12-381 private key held on their phone. It never leaves the phone and never appears on the ledger.
- The *clerk behind the desk* is the public Soroban smart contract `CBKKEZU3CEAXZNJ4RSLDSVCXNJFQK4WMAOGF26SUBT2QUKHWQE2PRFSO`.

One subtlety matters even for the plain-language picture. If a member pays the Stellar transaction fee from their own Stellar account, that account's public key appears in the transaction envelope and re-links a real-world identity to group activity. To prevent this, Onym members submit their proof to a public relayer; the relayer wraps the proof in a Stellar transaction signed by its own account. The relayer cannot forge a proof or change which group is being updated, but it can withhold submissions. We treat it as a liveness, not a safety, dependency.

What an outside observer of the public chain therefore learns, **in the Anarchy case**, is: a club of governance-type Anarchy exists (distinguished by its opaque identifier); its envelope has been swapped $k$ times at timestamps $t_1, \ldots, t_k$; the club is in a size tier with an upper bound of 32, 256, or 2,048 members, whichever circuit was used; the relayer paying the fee is a particular public Stellar account. The observer does not learn who is in the club, who initiated any swap, whether the group grew or shrank, or — beyond the declared Anarchy type — anything about the group's authorisation structure.

## 5. Onym Anarchy, formally

Let $\mathbb{F}_r$ be the BLS12-381 scalar field and $\mathbb{G}_1, \mathbb{G}_2, \mathbb{G}_T$ the pairing groups with generator $G_1 \in \mathbb{G}_1$. Let $H: \mathbb{F}_r^* \to \mathbb{F}_r$ denote Poseidon [9] instantiated at the standard BLS12-381 parameter set.

### 5.1 Commitment (shared by all governance types)

For a member set $S = \{\mathsf{pk}_1, \ldots, \mathsf{pk}_n\} \subseteq \mathbb{G}_1$ with $n \leq 2^d$, let $\sigma(S)$ be the lexicographic ordering of $S$ under compressed-G1 byte encoding and let $\mathsf{MT}_H$ denote the Poseidon Merkle tree of depth $d$ with leaf $H(\mathsf{pk})$ and a zero padding leaf $H(0)$. The *commitment* is

$$
\mathsf{Commit}(S, \epsilon, s) \;=\; H\!\bigl( H(\mathsf{MerkleRoot}(\mathsf{MT}_H(\sigma(S))),\, \epsilon),\, s \bigr) \in \mathbb{F}_r,
$$

where $\epsilon \in \mathbb{F}_r$ is the epoch counter and $s \in \mathbb{F}_r$ is a per-epoch salt uniformly distributed in $\mathbb{F}_r$. The canonicalisation $\sigma$ ensures $\mathsf{Commit}$ is well-defined on unordered sets. The other three governance types reuse this commitment definition unchanged.

### 5.2 Relations used by Anarchy

Two NP relations, both compiled to R1CS and proved with Groth16 [2], cover every Anarchy operation.

$R_{\mathsf{Mem}}$: Public input $(C, \epsilon) \in \mathbb{F}_r^2$; witness $(sk, \mathsf{root}, s, \mathsf{path}, \mathsf{idx})$.

$$
R_{\mathsf{Mem}}(C, \epsilon; \, sk, \mathsf{root}, s, \mathsf{path}, \mathsf{idx}) \iff
\begin{cases}
\mathsf{path}\text{ opens }H(sk \cdot G_1)\text{ at }\mathsf{idx}\text{ to }\mathsf{root}\\
H(H(\mathsf{root}, \epsilon), s) = C.
\end{cases}
$$

$R_{\mathsf{Mem}}$ is shared with all four governance types (`create_group`, `verify_membership`, `deactivate_group`).

$R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$: Public input $(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}, C_{\mathsf{new}}) \in \mathbb{F}_r^3$; witness $(sk, \mathsf{root}_{\mathsf{old}}, s_{\mathsf{old}}, \mathsf{path}, \mathsf{idx}, \mathsf{root}_{\mathsf{new}}, s_{\mathsf{new}})$.

$$
R_{\mathsf{Upd}}^{\mathsf{Anarchy}} \iff
\begin{cases}
R_{\mathsf{Mem}}(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}; \, sk, \mathsf{root}_{\mathsf{old}}, s_{\mathsf{old}}, \mathsf{path}, \mathsf{idx})\\
H(H(\mathsf{root}_{\mathsf{new}}, \epsilon_{\mathsf{old}} + 1), s_{\mathsf{new}}) = C_{\mathsf{new}}.
\end{cases}
$$

This is the update relation used *only* by Anarchy-typed groups. Democracy, Oligarchy, and 1v1 have distinct relations with additional public inputs and additional structural constraints — see [19] for their specifications.

Critically, $C_{\mathsf{new}}$ is a *public input* of $R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$: it enters the verifier's IC (instance-commitment) linear combination and is thus bound to the proof by the Groth16 public-input non-malleability property [2, 16]. Section 6 postmortem P2 describes what went wrong when $C_{\mathsf{new}}$ was *not* a public input.

**Authorization semantics of $R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$.** The relation's second conjunct binds $C_{\mathsf{new}}$ to *some* new root and *some* new salt, but it does not constrain the relationship between $\mathsf{root}_{\mathsf{old}}$ and $\mathsf{root}_{\mathsf{new}}$. A valid proof can transition from any old root to any new root. This means a single malicious member can replace the entire member set in one operation, ejecting all other members; there is no in-circuit enforcement of "add one member" or "remove one member" semantics; and the contract has no way to distinguish a legitimate membership change from a hostile takeover. **This is the defining property of the Anarchy type.** Constraining the diff — proving that $\mathsf{root}_{\mathsf{new}}$ differs from $\mathsf{root}_{\mathsf{old}}$ by exactly one leaf — would roughly double the R1CS constraint count (two Merkle-path openings plus an equality check on all unchanged leaves) and would preclude batched operations. It would also expose an on-chain signal (±1 member count) that Anarchy's minimalism is designed to avoid. The Democracy type does take that constraint and that on-chain cost; the design-space tradeoff is explicit in [19]. Applications that cannot tolerate the hostile-takeover risk should choose Democracy or Oligarchy; applications that prize metadata minimalism and can coordinate benign transitions socially choose Anarchy.

### 5.3 Security games (Anarchy)

Each property is a game between a PPT adversary $\mathcal{A}$ and a challenger $\mathcal{C}$. All games are over security parameter $\lambda = 128$ commensurate with BLS12-381. Each game is stated for Anarchy-typed groups; Democracy and Oligarchy have analogous games with different authorization predicates — see [19].

*Soundness* (for each of $R_{\mathsf{Mem}}$ and $R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$). $\mathcal{A}$ outputs $(\pi, x)$; $\mathcal{A}$ wins if the verifier accepts and there is no witness $w$ such that $R(x; w)$ holds. Discharged by Groth16 computational knowledge-soundness [2] under the generic-group assumption over BLS12-381, conditional on the trusted setup (Sec. 6 P4).

*Zero-knowledge.* Standard Groth16 ZK [2]; the simulator holds the trapdoor and produces proofs indistinguishable from honest ones.

*Commitment hiding.* $\mathcal{A}$ outputs two member sets $S_0, S_1$ with $|S_0| = |S_1| \leq 2^d$; $\mathcal{C}$ samples $b \leftarrow \{0,1\}$ and $s \leftarrow \mathbb{F}_r$ uniformly, computes $C = \mathsf{Commit}(S_b, \epsilon, s)$, returns $C$; $\mathcal{A}$ outputs $b'$ and wins if $b = b'$. Discharged by the pseudorandomness of Poseidon [9]: if $H$ is modelled as a pseudorandom function keyed by the high-entropy salt $s$ (which is sampled uniformly in $\mathbb{F}_r$ and never appears on-chain), the commitment output is computationally indistinguishable from a uniform element of $\mathbb{F}_r$ regardless of the choice of $S_b$. This is a standard assumption in the Poseidon literature [9] and is strictly stronger than the one-wayness property. Note that this game fixes the set cardinality $|S_0| = |S_1|$: in Anarchy, the cardinality is not published separately (unlike Democracy), so the hiding requirement is unconstrained. In Democracy, the same game is only meaningful for $|S_0| = |S_1|$, because the cardinality is on-chain.

*Operation-binding (for $R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$).* $\mathcal{A}$ observes an honestly generated proof $\pi$ over public tuple $(C, \epsilon, C')$ and produces $(\pi^*, C^{**})$ with $C^{**} \neq C'$ such that the verifier accepts $(\pi^*, C, \epsilon, C^{**})$. $\mathcal{A}$ wins if the contract then accepts $C^{**}$ as the next state. Discharged by Groth16 IC binding once $C_{\mathsf{new}}$ is a public input. This is the game that Onym *failed* from v1.0.0 through v1.2.x — see P2.

*Fee-payer unlinkability.* $\mathcal{A}$ observes the Stellar ledger and outputs a member identity $\mathsf{pk}$ and a transaction $T$; $\mathcal{A}$ wins if $\mathsf{pk}$ is the member who generated the proof in $T$ with probability non-negligibly greater than $1/n$ for group size $n$. Discharged by the relayer pattern and by constant-size proofs; an observer sees only the relayer's account. **Caveat:** the $1/n$ bound is a best-case figure that assumes no auxiliary information. For small Anarchy groups (e.g., $n = 3$, baseline $1/3$), timing correlation can degrade this substantially — if a member consistently submits within seconds of coming online, and online presence is observable (via Nostr relay connections, IP metadata, or application-layer signals), an adversary's advantage can far exceed $1/n$ without breaking the relayer abstraction. The game as stated captures the cryptographic guarantee (proof and fee-payer are unlinkable) but does not capture side-channel advantages from timing, traffic analysis, or behavioral patterns. For small groups, the formal guarantee is already weak. Applications with Anarchy groups of fewer than ~10 members should treat fee-payer unlinkability as a soft property that degrades with auxiliary information, not a hard bound. (Note that Democracy groups publish $n$ explicitly, so the baseline becomes the observed $n$ rather than a tier upper bound.)

The full reductions for each game are not yet publicly available. We intend to post a companion document to IACR ePrint prior to the camera-ready deadline. Until that document is accessible, the reader has no independent way to verify the reductions, and the formal claims above rest on our assertion. We acknowledge this limits the strength of these claims for a venue that values verifiable arguments, and we consider the ePrint posting a prerequisite for the final version of this article.

## 6. Four postmortems (all on the Anarchy path)

We report four episodes in which the correctness of the system drifted off the rails. Each one was caught, fixed, and generalised into a design-review checklist item. We think they generalise because they share a single shape: a *local* component (a circuit, a relation, a key, a setup) was correct by the lights of its local specification and was wrong for the larger property the system actually needed. All four predate the governance-types schema and therefore concern the Anarchy relation $R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$ (which was simply called $R_{\mathsf{Upd}}$ at the time).

### P1 — The constraint-cost crisis

**What went wrong.** The original commitment binding composed Poseidon inside the circuit with SHA-256 outside it:

$$
\mathsf{Commit}_A(S, \epsilon, s) = \text{SHA-256}(\mathsf{MerkleRoot}(\mathsf{MT}_H(\sigma(S))) \mathbin\| \epsilon \mathbin\| s).
$$

SHA-256 was chosen because Soroban has a native SHA-256 host function: a contract can recompute the commitment from witness values at negligible fee cost. At the R1CS level it was a disaster. A single SHA-256 compression on a fixed 72-byte preimage costs roughly 25,000 R1CS constraints in the arkworks gadget [18]. The small-tier circuit (32 members) was dominated by SHA-256 by more than an order of magnitude. On a 2024-class smartphone with 6 GB of RAM, proving under multi-app memory pressure failed to complete reliably.

**The fix.** Replace the outer SHA-256 with a two-call Poseidon composition:

$$
\mathsf{Commit}_B(S, \epsilon, s) = H(H(\mathsf{MerkleRoot}, \epsilon), s).
$$

Two Poseidon evaluations cost roughly 600 R1CS constraints in total. Table 2 lists the measured constraint counts. The fix applies identically to all four governance types because $\mathsf{Commit}$ is shared; we measured and reported it first on Anarchy because Anarchy was the only type at the time.

**Table 2.** Measured R1CS constraint counts, by tier, for the two commitment variants, on $R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$.

| Tier | Members | Tree depth $d$ | $\mathsf{Commit}_A$ (SHA-256 outer) | $\mathsf{Commit}_B$ (Poseidon outer) | Reduction |
|------|---------|----------------|-------------------------------------|--------------------------------------|-----------|
| Small | 32 | 5 | ~26,740 | **1,910** | 14.0× |
| Medium | 256 | 8 | ~27,640 | **2,630** | 10.5× |
| Large | 2,048 | 11 | ~28,540 | **3,350** | 8.5× |

The reduction shrinks at higher tiers because SHA-256's contribution is a constant ~25,000 while the Merkle path grows logarithmically. Practitioners rebuilding this system at the large tier should expect the 8.5× figure, not the headline 14×.

**The generalised lesson.** Any cryptographic primitive that sits on both sides of an R1CS boundary must have its in-circuit cost counted *first*. A primitive with a cheap host function but an expensive R1CS footprint will be a constraint-budget landmine.

### P2 — The theorem that proved the wrong thing (a critical authorization vulnerability)

**Severity first.** This was a critical authorization vulnerability, not a design tradeoff. Any party in the network path between a prover and the contract could substitute the new commitment written to storage, retaining the prover's still-valid proof. Every privacy and authorization guarantee in Sec. 5.3 was void for the operation that changes group state. The vulnerability existed only on the Anarchy path because that was the only path; the architectural mistake generalised, and the Democracy and Oligarchy relations later shipped with the same $C_{\mathsf{new}}$-as-public-input discipline as a consequence of this lesson.

**What went wrong.** Through release v1.2.9 all four state-changing contract operations shared one Groth16 circuit whose relation was $R_{\mathsf{Mem}}$ with public inputs $(C, \epsilon)$. The `update_commitment` entry point accepted, *in addition to* the proof and its public inputs, a separate `new_commitment: BytesN<32>` parameter; the contract wrote this parameter to storage as the next epoch's state. The proof authorised knowledge of a member in the *current* state. It said nothing about which next state was being authorised. `new_commitment` was a contract-controlled free parameter.

A code-review question exposed the gap:

> *"Where in the math is the argument that this proof is non-transferable to a different `new_commitment`?"*

The answer was: nowhere. The Groth16 verification equation incorporates the public inputs through the IC linear combination [16], and the proof is cryptographically bound exactly to those public inputs and to nothing else. Since `new_commitment` was not a public input, a relayer, a mempool observer, or any intermediate HTTP proxy could replace `new_commitment` with an arbitrary 32-byte value, keep the proof and the public inputs, and the contract would accept.

**How the theorem deceived.** We had a formal soundness argument — a correct proof of knowledge-soundness for $R_{\mathsf{Mem}}$. The argument was *irrelevant to the update operation's security*, because the update operation's security predicate is over the variables $(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}, C_{\mathsf{new}})$, and $R_{\mathsf{Mem}}$ does not mention $C_{\mathsf{new}}$. In retrospect the theorem was functioning as social proof: auditors saw a completed proof in the repository, registered that the cryptography had been "proved correct," and stopped looking at the binding question.

**Why four audit passes missed it.** The system had been through four audit passes at the time of discovery:

1. *Internal review, scope: circuit soundness.* Verified the Merkle opening gadget, the Poseidon parameterization, and the constraint layout. Concluded: $R_{\mathsf{Mem}}$ is sound.
2. *External review, scope: contract ABI conformance.* Verified entry-point signatures, access control, and the canonical-bytes guard on public inputs. Did not re-derive what each proof binds.
3. *Internal review, scope: primitive choice.* Reviewed Poseidon parameters, BLS12-381 subgroup checks, and the Ed25519 attestation format. Did not examine call-site semantics.
4. *External review, scope: wire-format correctness.* Verified fixed-width encoding across platforms. Did not examine the circuit-to-contract interface as a binding question.

No pass had "for each cryptographically authorized operation, state what the proof binds and what it does not bind" as a line item.

**The fix.** We split the relation into two: $R_{\mathsf{Mem}}$ (unchanged, used by `create_group`, `verify_membership`, `deactivate_group`) and $R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$ (new, used only by the Anarchy `update_commitment` path). $R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$'s public inputs are $(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}, C_{\mathsf{new}})$; $C_{\mathsf{new}}$ now enters the IC linear combination and is bound to the proof. The circuit additionally enforces $\epsilon_{\mathsf{new}} = \epsilon_{\mathsf{old}} + 1$ in-circuit. Each of the three tiers now ships two verification keys instead of one. The fix shipped as release v1.3.0 on 2026-04-18.

**The generalised lesson.** Formal soundness of a relation $R$ implies nothing about the security of an operation whose security predicate is over variables $R$ does not mention. Our design review template now requires, for every cryptographically authorised operation: (1) a one-sentence predicate stating what the proof binds; (2) an enumeration of every value the contract reads on that path, labelled "in IC" or "contract-controlled"; (3) a side-by-side read of the relation file and the contract entry-point. The Democracy and AdminUpdate relations introduced in PR #77 (v1.7.0) passed this checklist at design time — a direct return on the P2 lesson.

### P3 — Transport-layer co-membership leakage

Onym's clients exchange invitations, rekey envelopes, and group ciphertext over Nostr relays [3]. Every Nostr event is signed by a secp256k1 key. Through release v1.2.7 each client signed every event with its *long-lived* identity key. The companion transport specification mentioned the implication in a single bullet of its Security Considerations:

> *"Stable Nostr device keys improve usability but increase sender linkability."*

That bullet is technically true and strategically misleading. "Sender linkability" sounds like a profile-building concern affecting one user at a time. The actual consequence, given that Onym's group ciphertext also carried a stable hidden topic tag, was *group linkability*: any relay could cluster the user base by co-membership from public relay data — "these $N$ keys all published to hidden-topic $T$ → these $N$ devices are in the same group."

**The fix — keys and topic tags.** The primary fix was ephemeral per-event secp256k1 keys — a five-line key-factory change — with a receive-side audit switching four places (display logic, rate-limiting, removal notices, a database column) from `event.pubkey` to the inner BLS public key. The hidden topic tag, which the earlier version of this article mentioned but did not resolve, was also addressed in the same release (v1.2.8): topic tags are now derived per-epoch as $H(\mathsf{group\_id} \| \epsilon)$ and rotate on every state change. A relay that observes the tag can cluster events within a single epoch but cannot link across epochs without breaking Poseidon's preimage resistance. The combination of ephemeral signing keys and rotating topic tags closes the co-membership channel that static keys and static tags had opened. We do not claim that no traffic-analysis-based co-membership inference remains — relay-level timing correlation is an open problem we have not formally analysed — but the two concrete mechanisms (stable keys, stable tags) that made co-membership trivially observable from public data are both eliminated. The fix applies to all governance types, since the transport layer is shared; Anarchy is the type on which the bug was observed and reproduced.

**The generalised lesson.** A privacy invariant that lives in a *Security Considerations* bullet of a different document is fiction. Identity coupling — a single key serving as both signing key and sender identity — is a recurring antipattern; decoupling the two roles is almost free on day one and a postmortem otherwise.

### P4 — The single-machine trusted setup

The preceding three postmortems are about bugs that were fixed. This one is a bug that is *not yet fixed* and that caps the system's current security ceiling — across all four governance types, because all four rely on Groth16.

Groth16 requires a per-circuit trusted setup [2]. For the Anarchy path, each of our six verification keys (two relations × three tiers) must be the output of a multi-party computation (MPC) in which at least one honest participant discarded their contribution; otherwise the party who holds the aggregate trapdoor can forge accepting proofs for arbitrary public inputs. The three non-Anarchy types introduce additional circuits with their own per-circuit setups, inheriting the same requirement. Our reference implementation runs the full Powers-of-Tau Phase-1 protocol with multi-party contributions and pairing-based transcript verification. But the Phase-2 circuit-specific ceremony currently runs on a single machine operated by one party. That party holds the aggregate Phase-2 secret.

Until the Phase-2 ceremony is run as a multi-party computation, every security game in Sec. 5.3 is conditional on trusting the setup operator. The system is not, by its own formal statements, zero-trust. The mainnet rollout criterion is a Phase-2 MPC with at least five independent contributors, each of whom publicly destroys their contribution secret, and a scoped external audit of $R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$ (and of the Democracy and AdminUpdate relations, which use dev-only VKs until the equivalent ceremonies run). Neither has been done.

**The generalised lesson.** A cryptographic prerequisite that is not yet satisfied does not become satisfied by being documented in a limitations section. If the prerequisite caps every security game the system claims, it belongs in the system's abstract and in every mention of its security model.

## 7. Evaluation (Anarchy)

Onym's reference implementation is deployed on Stellar testnet at contract ID `CBKKEZU3CEAXZNJ4RSLDSVCXNJFQK4WMAOGF26SUBT2QUKHWQE2PRFSO`. A testnet deployment is not a production deployment: no real users, no economic stakes, no adversarial environment. The measurements below are for the Anarchy path only; Democracy's larger public-input count and its K-of-N signature aggregation change its proving profile and are reported in [19].

**Prover time.** All numbers are median of 50 runs with 10 warmup runs discarded, on a Pixel 8 Pro (Tensor G3, 12 GB RAM) and an iPhone 15 Pro (A17 Pro, 8 GB RAM). Witness generation, multi-scalar multiplication, and proof serialisation are all included.

**Table 3.** End-to-end proof generation latency on the Anarchy path, median (95th percentile), in milliseconds.

| Tier | Members | Depth | Pixel 8 Pro $R_{\mathsf{Mem}}$ | Pixel 8 Pro $R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$ | iPhone 15 Pro $R_{\mathsf{Mem}}$ | iPhone 15 Pro $R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$ |
|------|---------|-------|--------------------------------|---------------------------------------------------|----------------------------------|------------------------------------------------------|
| Small | 32 | 5 | 180 (240) | 235 (310) | 165 (220) | 215 (290) |
| Medium | 256 | 8 | 320 (410) | 415 (530) | 290 (380) | 380 (490) |
| Large | 2,048 | 11 | 760 (920) | 985 (1,200) | 690 (840) | 895 (1,090) |

The small and medium tiers are sub-second on both platforms. The large tier is sub-1.2 s.

**On-chain verification cost.** Groth16 proofs are 192 bytes. Public inputs add 32 bytes for $R_{\mathsf{Mem}}$ and 96 bytes for $R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$. Verification invokes three pairings via Stellar's BLS12-381 host functions plus a small multi-scalar multiplication over the IC; total Soroban instruction count is 6–10 million, inside testnet fee budgets.

**Privacy evaluation (Anarchy).** A public-ledger observer of an Anarchy group learns: that a group of governance-type 0 exists; the number and timestamps of its state changes; an upper bound on group size from the tier; the relayer's Stellar account; and the fact that no admin-commitment updates or democracy updates have occurred (since those use distinct entry points). An observer does *not* learn, conditional on intact trusted setup (P4) and Groth16 soundness: the member set, the updater's identity, the exact member count, or whether the group grew, shrank, or rotated. The direction-of-change signal that Democracy exposes is not available for Anarchy: every Anarchy update is opaque at the ±1 level. We have *not* run a formal differential-privacy analysis of the timing channel, nor quantified the information an observer recovers from tier-transition patterns.

The privacy enumeration above applies to Anarchy alone. A Democracy-typed group additionally publishes the exact `member_count` and, via the single-leaf-delta constraint, a ±1 direction-of-change signal on every update. An Oligarchy-typed group additionally publishes an admin-epoch counter. Readers should not transfer Anarchy's "does not learn" list to the other three types.

## 8. Honest limitations

In addition to the open P4:

**Testnet, not mainnet.** The system has processed no real transaction. Mainnet is gated on P4's MPC and on the external $R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$ audit.

**No committed production customer.** We have had exploratory conversations with a Matrix-adjacent group and with one governance-tooling project. Neither has a signed letter of intent. The article's contribution is engineering lessons, not market validation.

**Group-size leakage via tier.** The choice of tier is public and leaks an upper bound on Anarchy group size. Applications in which the existence of a 2,000-member group is itself sensitive should pad to a higher tier. (For Democracy-typed groups this mitigation is ineffective because the exact count is also public — another reason to choose Anarchy when metadata minimalism dominates.)

**Fee-payer correlation via the relayer.** The relayer's own account is a metadata observation point. A single relayer is better than each member being their own fee payer, but worse than a fully decentralised relayer pool. We consider this mitigable with relayer rotation and a reason to revisit the design when Soroban's fee-sponsorship proposals mature. This limitation is shared by all four governance types.

**Governance-type leakage.** The declared governance type (0/1/2/3) is stored in plaintext in `CommitmentEntryV2` and is observable from any state-changing invocation. An Anarchy group is indistinguishable on-chain from other Anarchy groups, but distinguishable from Democracy, Oligarchy, and 1v1 groups. An application that wants to hide *which* governance type a group uses cannot: all four types publish the type in the clear, by design, so the dispatcher can verify the correct circuit. If indistinguishability across governance types is required, the only current recourse is to use only one type across the deployment.

**All members must be online provers.** Every Anarchy membership change requires a current member to generate a Groth16 proof on their device. A member whose phone is off cannot authorize a group change. There is no "admin adds member while member is offline" flow unless the admin holds the new member's key material. Traditional group-membership systems where a server processes changes on behalf of offline members do not have this constraint. For Onym Anarchy, the tradeoff is structural: the privacy guarantee depends on the proof being generated on the member's device with locally held key material, and delegating proof generation to a server would reintroduce the operator-trust assumption the system is designed to eliminate. (The Oligarchy type, described in [19], partially lifts this constraint by letting a pre-rotated admin authorise changes without the affected member online; it accepts the corresponding metadata cost.)

**Anarchy is vulnerable to hostile takeover by any current member.** A single current member can replace the entire member set in one Anarchy update. This is the defining property of the type, not a bug; applications that cannot tolerate this should choose Democracy (which requires a K-of-N quorum and single-leaf diffs) or Oligarchy (which restricts authorisation to a separately-rotated admin set). The article's privacy and correctness claims apply regardless of whether a hostile takeover occurs — the chain state reflects whatever the last authorised proof attested — but the *semantic* integrity of the group is the caller's responsibility to enforce socially.

**MLS integration is partial.** Onym maps an MLS Commit to an `update_commitment` call and distributes the fresh salt over MLS's encrypted GroupContext extension. We have not yet integrated Onym with a stock MLS implementation's Delivery Service interface; our reference clients use a bespoke MLS-lite implementation. The mapping is defined for Anarchy; for Democracy and Oligarchy the Commit-to-update mapping is more elaborate and is specified separately in [19].

## 9. Related work and alternatives

Table 4 locates Onym Anarchy within the space of approaches to the metadata-hiding group-membership problem.

**Table 4.** Approaches to the metadata-hiding group-membership problem.

| Approach | Example(s) | Trust | Metadata-observable by |
|----------|-----------|-------|------------------------|
| Anonymous credentials on a central server | Signal PGS [11] | Signal server (honest-but-curious, binary-faithful) | Not by Signal in honest-but-curious model; by Signal under active compromise (server-side code modification) |
| Federated delivery | Matrix, XMPP | Every federated homeserver | Federation peers |
| Transparency logs | CT [10], Key Transparency [13] | Log operator | Log operator |
| Mixnets | Nym, Loopix | Mixnet operators (non-colluding threshold) | Network-layer adversary only |
| Public-ledger anchoring + SNARK, Anarchy type (this work) | Onym Anarchy | Chain + SNARK soundness + trusted setup | No operator in the honest-setup case |
| Per-application ZK primitive | Semaphore [4], Tornado Cash [5], Zcash [6] | As above | No operator |

Onym Anarchy is most directly comparable to Semaphore and Tornado Cash in construction — all three anchor a Merkle tree of commitments on a public ledger and verify membership with a Groth16 SNARK — and distinguishes itself by being a general group-registry primitive coupled to MLS Commit semantics rather than a single-application identity tree. Within the Onym contract itself, Anarchy trades off against Democracy and Oligarchy (see [19]) along a metadata-vs-authorization axis that is, as far as we know, novel in the deployed ZK-membership literature: Semaphore and Tornado Cash do not offer per-group authorisation policies, and Signal PGS does not let applications choose policies without re-running an anonymous-credential issuance ceremony. Aztec [7] uses PLONK on Ethereum for confidential transactions and is the closest non-Groth16 deployed system; we did not adopt PLONK because Stellar Protocol 22 provides BLS12-381 pairing host functions that directly accelerate Groth16 verification but does not yet provide the polynomial-commitment host functions PLONK would require. Signal's Sealed Sender [8] addresses a related decoupling problem (sender unlinkability) at the messaging layer rather than the registry layer.

## 10. Conclusion

Metadata observability is the unsolved half of online-communication privacy. The best-deployed solution today, Signal's Private Group System, closes it under a trust-the-operator model and has earned that trust through a decade of public engineering. Onym explores the trust-nobody frame — a public anchor, SNARK-gated state changes, no operator positioned to observe group membership. Within Onym, the **Anarchy** type is the extreme of that frame: it stores the minimum on-chain metadata the architecture can support, at the cost of placing all authorisation-strength responsibility on the social layer. The three sister types — 1v1, Democracy, Oligarchy — offer stronger authorisation at the cost of additional on-chain signals, and occupy different points of the same design space.

The contribution of this article is not that Anarchy is the right choice for every threat model; it is not. It is the engineering lessons from building toward it on testnet: count R1CS cost before host-function cost when a primitive straddles the circuit boundary; a relation's soundness theorem says nothing about operations whose security predicate is over variables the relation does not mention; privacy invariants that live in Security Considerations bullets of other documents are fiction; and a trusted-setup prerequisite does not become discharged by being mentioned in a later section. Each of the four postmortems happened on the Anarchy path, and each of them informed the design of the later three types — most visibly the P2 lesson, which is the reason the Democracy and AdminUpdate relations shipped with $C_{\mathsf{new}}$ as a public input from day one.

We would prefer that readers take the postmortems more seriously than the system. The system is one more point in a design space. The failure modes generalise to any system in this neighbourhood — including the three non-Anarchy Onym types and anything else built on Groth16 over a public ledger.

## Acknowledgements

We thank the Stellar Development Foundation for Protocol 22's BLS12-381 host functions, without which constant-cost verification would not be feasible at this gas level, and the arkworks contributors for the Rust Groth16 and Poseidon gadgets. The reviewer who asked *"where in the math is the argument that this proof is non-transferable?"* is the reason P2 is a postmortem rather than a zero-day.

## References

[1] R. Barnes et al., "The Messaging Layer Security (MLS) Protocol," IETF RFC 9420, July 2023.

[2] J. Groth, "On the Size of Pairing-Based Non-Interactive Arguments," in *Proc. EUROCRYPT*, 2016, pp. 305–326.

[3] fiatjaf, "NIP-01: Basic Protocol Flow Description," Nostr Implementation Possibilities, 2023. [Online]. Available: https://github.com/nostr-protocol/nips/blob/master/01.md

[4] Privacy & Scaling Explorations, "Semaphore Protocol Specification v4," 2024. [Online]. Available: https://docs.semaphore.pse.dev/

[5] L. Wang, X. Tang, and S. Meiklejohn, "An Empirical Analysis of Privacy in the Tornado Cash Mixer," in *Proc. ACM CCS*, 2022.

[6] D. Hopwood, S. Bowe, T. Hornby, and N. Wilcox, "Zcash Protocol Specification," version 2024.5.0, Electric Coin Company, 2024.

[7] J. Williamson, "The Aztec Protocol," Aztec Network technical report, 2022.

[8] J. Lund, "Sealed Sender for Signal," Signal blog, October 2018. [Online]. Available: https://signal.org/blog/sealed-sender/

[9] L. Grassi, D. Khovratovich, C. Rechberger, A. Roy, and M. Schofnegger, "Poseidon: A New Hash Function for Zero-Knowledge Proof Systems," in *Proc. USENIX Security*, 2021, pp. 519–535.

[10] B. Laurie, A. Langley, and E. Kasper, "Certificate Transparency," IETF RFC 6962, June 2013.

[11] M. Chase, T. Perrin, and G. Zaverucha, "The Signal Private Group System and Anonymous Credentials Supporting Efficient Verifiable Encryption," in *Proc. ACM CCS*, 2020.

[12] M. V. Hayden, "The Price of Privacy: Re-Evaluating the NSA," Johns Hopkins University, Apr. 7, 2014. Publicly quoted: "We kill people based on metadata."

[13] M. S. Melara et al., "CONIKS: Bringing Key Transparency to End Users," in *Proc. USENIX Security*, 2015, pp. 383–398.

[14] A. Cooper et al., "Privacy Considerations for Internet Protocols," IETF RFC 6973, July 2013.

[15] R. Barbulescu and S. Duquesne, "Updating Key Size Estimations for Pairings," *J. Cryptol.*, vol. 32, no. 4, pp. 1298–1336, 2019.

[16] H. Lipmaa, "Simulation-Extractable SNARKs Revisited," Cryptology ePrint Archive, Report 2019/612, 2019. [Online]. Available: https://eprint.iacr.org/2019/612

[17] Stellar Development Foundation, "CAP-0046-12: BLS12-381 Host Functions," Stellar Core Advancement Proposal, 2024.

[18] arkworks contributors, "r1cs-std: SHA-256 gadget constraint benchmarks," 2023. [Online]. Available: https://github.com/arkworks-rs/r1cs-std

[19] R. Enikeev, "Configurable Group Governance Types for Onym," internal design document, Apr. 2026. [Online]. Available in-repo at `docs/group-governance-types-design.md`.
