---
title: "Metadata Without Observers: A Postmortem-Driven Case Study of a SNARK-Gated Group Registry on a Public Ledger"
author: Rinat Enikeev
venue: IEEE Security & Privacy magazine (feature article)
status: Shortened draft (~30% reduction) per review comment 4282505699
date: 2026-04-20
word_count_target: ~4,000
---

# Metadata Without Observers: A Postmortem-Driven Case Study of a SNARK-Gated Group Registry on a Public Ledger

## Abstract

End-to-end encryption closed the content half of the online-communication privacy problem. The metadata half — *who is in a group with whom* — still leaks to at least one operator in almost every deployed messaging system. We describe Onym, a testnet system that anchors each group's state on the Stellar blockchain as a 32-byte commitment and gates every state change on a Groth16 zero-knowledge Succinct Non-Interactive Argument of Knowledge (SNARK) over BLS12-381. This article is scoped to a single mode of operation: a group where any current member may unilaterally authorize any next state with a single membership proof. No member address, updater identity, or group-size signal beyond a coarse tier appears on the ledger. We introduce the construction formally — relations, instance-commitment (IC) binding, and game-based security statements — and report four postmortems from its development. Onym is not a mainnet system and does not yet have a production customer. This is a case study in what it took to build toward those properties, and in the specific ways the work kept going wrong.

---

## 1. The metadata problem

Content-level encryption is now a commodity. Signal's Double Ratchet, iMessage, WhatsApp, Google Messages' Messaging-Layer-Security (MLS) rollout, and Matrix's Olm/Megolm all provide end-to-end content encryption. None hide metadata from the operator. iCloud backups hold message metadata for every Apple user. WhatsApp provides subscriber records and message metadata under stored-communications-act orders. Matrix homeservers see every room's membership list by construction. Telegram "secret chats" leave groups, channels, and cloud chats server-readable. Signal is the outlier: repeated grand-jury subpoenas have forced it to disclose only registration time and last-seen date, and its transparency reports substantiate that list is exhaustive.

Metadata is not a secondary concern. Two of the largest law-enforcement operations of the last five years — the 2020 EncroChat takedown and the 2021 Sky ECC operation — recovered content from compromised devices, but the *targeting* was metadata-driven. A former NSA General Counsel's remark — "we kill people based on metadata" [10] — is a statement about operational reality.

The structural cause is that every messaging architecture we know of has at least one operator positioned to observe the metadata. In centralized systems it is the server operator. In federated systems (XMPP, Matrix, email) every federated server learns its tenants' metadata. In MLS deployments the Delivery Service (DS) is the ordering authority and therefore the metadata authority — it sees each Commit, each Welcome, and which device-pseudonyms are present in each group. The MLS specification [1] is explicit that the DS is trusted for ordering.

This paper is not a proposal to replace these deployed systems. Signal's Private Group System [9] is an existence proof that strong metadata privacy is compatible with a commercial messenger at scale. The question Onym asks is narrower: **can we build a group-membership registry where no operator — not even the anchor operator — is structurally positioned to observe the metadata, and where the privacy model is externally auditable from the public spec and the published bytecode?** Signal is the gold standard within a trust-the-operator frame; Onym explores the trust-nobody frame.

## 2. Why blockchain + SNARKs

Four candidate approaches exist.

1. **Transparency logs** (Certificate / Key Transparency [11]) hide content but do not hide who is writing to which key — not suited to a group-registry setting.
2. **Federated delivery** (Matrix, XMPP, email) distributes trust but does not eliminate per-server metadata observation.
3. **Anonymous-credential-backed central servers** (Signal PGS [9]) give excellent metadata privacy under an honest-but-curious server. A third party auditing Onym can verify, from public chain state and the deployed contract bytecode alone, that every state transition was authorized by a valid SNARK — without trusting any server binary to match any spec. For Signal PGS, the equivalent verification requires trusting that the running binary faithfully implements the published protocol.
4. **Public-ledger anchoring plus zero-knowledge (ZK) membership proofs** commit group state on a public chain and verify membership with a SNARK. Every observer reads the same chain; no operator is positioned to observe more. The trust assumptions that remain are on chain censorship resistance, SNARK soundness, and the SNARK's trusted setup.

Onym takes the fourth approach. Within it, we chose Stellar over Ethereum because Stellar Protocol 22 [14] introduced BLS12-381 host functions, reducing Groth16 verification to three pairings and a small multi-scalar multiplication at negligible fee cost. BN254's effective security dropped to an estimated 100–103 bits after improved number-field-sieve attacks [12]; BLS12-381 is at roughly 120 bits of pairing security. Our threshold is 120 bits.

## 3. Scope of this article

The deployed Onym contract supports several authorization policies for state changes. This article is scoped to a single one: **a group where any current member, alone, may authorize any next state.** That is the complete characterization needed to follow the rest of the paper; the other policies are out of scope and described in the companion design document [15]. Readers should not generalize the privacy properties reported here to those other policies.

All four postmortems in Sec. 5 occurred before additional policies shipped (v1.0.0–v1.6.7 supported only this mode; other policies shipped in v1.7.0 on 2026-04-19). Every design mistake and every fix therefore concerns the relation we study here.

## 4. In plain language

Picture a town hall that must keep a membership register for many private clubs, and picture the town's constitution prohibits the hall from knowing who is in any club. Instead of writing names, the hall posts one sealed envelope per club. The envelope is a single short line of text that reveals nothing. A member who walks up to the desk can prove they belong inside the envelope without anyone opening it — they hand over a small slip of paper, the desk staff perform a fixed arithmetic check, and the staff return "yes" or "no." No staff member ever learns who the member is.

When the club's membership changes, one current member computes what the club's *next* envelope should say, writes both old and new envelopes on the slip, proves "I belong inside yesterday's envelope, and today's envelope is the correct next version of it," and hands the slip over. The hall swaps envelopes. Nothing on the hall's wall ever named a member. Any current member can perform that swap alone, and the hall has no way to tell which member did so.

The real architecture follows that picture literally:

- The *hall* is the Stellar blockchain. Anyone can read every envelope, which is just a 32-byte number per group.
- The *envelope* is a cryptographic commitment: an opaque hash-tree of the member set, bound to an epoch counter and a random salt.
- The *slip of paper* is a 192-byte Groth16 proof.
- The *arithmetic check* is three pairings on BLS12-381.
- The *member's secret* is a BLS12-381 private key held on their device.
- The *clerk behind the desk* is the public Soroban smart contract `CC6NUUKG25RSFI6D57HISDQ4HRBLXFAUC3GFVZIACHQX3NLRPYTRWWKE`.

One subtlety matters. If a member pays the Stellar transaction fee from their own account, that account's public key appears in the transaction envelope and re-links a real-world identity to group activity. To prevent this, Onym members submit their proof to a public relayer; the relayer wraps the proof in a Stellar transaction signed by its own account. The relayer cannot forge a proof or change which group is updated, but it can withhold submissions — we treat it as a liveness, not a safety, dependency.

What an outside observer of the public chain learns is: a group exists; its envelope has been swapped $k$ times at timestamps $t_1, \ldots, t_k$; the group is in a size tier with an upper bound of 32, 256, or 2,048 members; and the relayer paying the fee is a particular public Stellar account. The observer does not learn who is in the group, who initiated any swap, or whether the group grew, shrank, or rotated.

## 5. Formal construction

Let $\mathbb{F}_r$ be the BLS12-381 scalar field and $\mathbb{G}_1, \mathbb{G}_2, \mathbb{G}_T$ the pairing groups with generator $G_1 \in \mathbb{G}_1$. Let $H: \mathbb{F}_r^* \to \mathbb{F}_r$ denote Poseidon [7] at the standard BLS12-381 parameter set.

### 5.1 Commitment

For a member set $S = \{\mathsf{pk}_1, \ldots, \mathsf{pk}_n\} \subseteq \mathbb{G}_1$ with $n \leq 2^d$, let $\sigma(S)$ be the lexicographic ordering under compressed-G1 byte encoding and $\mathsf{MT}_H$ the Poseidon Merkle tree of depth $d$ with leaf $H(\mathsf{pk})$ and zero padding $H(0)$. The commitment is

$$
\mathsf{Commit}(S, \epsilon, s) \;=\; H\!\bigl( H(\mathsf{MerkleRoot}(\mathsf{MT}_H(\sigma(S))),\, \epsilon),\, s \bigr) \in \mathbb{F}_r,
$$

where $\epsilon \in \mathbb{F}_r$ is the epoch counter and $s \in \mathbb{F}_r$ is a per-epoch salt uniform in $\mathbb{F}_r$. The canonicalization $\sigma$ ensures $\mathsf{Commit}$ is well-defined on unordered sets.

### 5.2 Relations

Two nondeterministic polynomial-time (NP) relations, both compiled to Rank-1 Constraint System (R1CS) and proved with Groth16 [2], cover every operation.

$R_{\mathsf{Mem}}$: Public input $(C, \epsilon) \in \mathbb{F}_r^2$; witness $(sk, \mathsf{root}, s, \mathsf{path}, \mathsf{idx})$.

$$
R_{\mathsf{Mem}}(C, \epsilon; \, sk, \mathsf{root}, s, \mathsf{path}, \mathsf{idx}) \iff
\begin{cases}
\mathsf{path}\text{ opens }H(sk \cdot G_1)\text{ at }\mathsf{idx}\text{ to }\mathsf{root}\\
H(H(\mathsf{root}, \epsilon), s) = C.
\end{cases}
$$

$R_{\mathsf{Upd}}$: Public input $(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}, C_{\mathsf{new}}) \in \mathbb{F}_r^3$; witness $(sk, \mathsf{root}_{\mathsf{old}}, s_{\mathsf{old}}, \mathsf{path}, \mathsf{idx}, \mathsf{root}_{\mathsf{new}}, s_{\mathsf{new}})$.

$$
R_{\mathsf{Upd}} \iff
\begin{cases}
R_{\mathsf{Mem}}(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}; \, sk, \mathsf{root}_{\mathsf{old}}, s_{\mathsf{old}}, \mathsf{path}, \mathsf{idx})\\
H(H(\mathsf{root}_{\mathsf{new}}, \epsilon_{\mathsf{old}} + 1), s_{\mathsf{new}}) = C_{\mathsf{new}}.
\end{cases}
$$

Critically, $C_{\mathsf{new}}$ is a *public input*: it enters the verifier's instance-commitment (IC) linear combination and is thus bound to the proof by Groth16's public-input non-malleability [2, 13]. Postmortem P2 describes what went wrong when $C_{\mathsf{new}}$ was *not* a public input.

**Authorization semantics.** The relation binds $C_{\mathsf{new}}$ to *some* new root and *some* new salt, but does not constrain the relationship between $\mathsf{root}_{\mathsf{old}}$ and $\mathsf{root}_{\mathsf{new}}$. A valid proof can transition from any old root to any new root. A single current member can therefore replace the entire member set in one operation; that is by design. Constraining the diff would roughly double the R1CS constraint count and expose an on-chain ±1 member-count signal the design is intended to avoid.

### 5.3 Security games

Each is a game between a probabilistic polynomial-time adversary $\mathcal{A}$ and challenger $\mathcal{C}$, with $\lambda = 128$ commensurate with BLS12-381.

*Soundness* (for each of $R_{\mathsf{Mem}}$ and $R_{\mathsf{Upd}}$). $\mathcal{A}$ outputs $(\pi, x)$; wins if the verifier accepts and no witness $w$ satisfies $R(x; w)$. Discharged by Groth16 knowledge-soundness [2] under the generic-group assumption, conditional on the trusted setup (P4).

*Zero-knowledge.* Standard Groth16 ZK [2].

*Commitment hiding.* $\mathcal{A}$ outputs $S_0, S_1$ with $|S_0| = |S_1| \leq 2^d$; $\mathcal{C}$ samples $b \leftarrow \{0,1\}$, $s \leftarrow \mathbb{F}_r$, returns $C = \mathsf{Commit}(S_b, \epsilon, s)$; $\mathcal{A}$ outputs $b'$ and wins if $b = b'$. Discharged by Poseidon's pseudorandomness [7] keyed by the high-entropy salt.

*Operation-binding.* $\mathcal{A}$ observes honest $\pi$ over $(C, \epsilon, C')$ and produces $(\pi^*, C^{**})$ with $C^{**} \neq C'$ such that the verifier accepts $(\pi^*, C, \epsilon, C^{**})$. Discharged by Groth16 IC binding once $C_{\mathsf{new}}$ is a public input. Onym *failed* this game from v1.0.0 through v1.2.x — see P2.

*Fee-payer unlinkability.* $\mathcal{A}$ outputs a member identity $\mathsf{pk}$ and a transaction $T$; wins if $\mathsf{pk}$ is the prover in $T$ with probability non-negligibly greater than $1/n$ for group size $n$. Discharged by the relayer pattern and constant-size proofs. Caveat: the $1/n$ bound assumes no auxiliary information; timing correlation degrades it for small groups.

Full reductions will be posted to IACR ePrint before the camera-ready deadline.

## 6. Four postmortems

Each episode was caught, fixed, and generalized into a design-review checklist item.

### P1 — The constraint-cost crisis

**What went wrong.** The original commitment binding composed Poseidon inside the circuit with SHA-256 outside it:

$$
\mathsf{Commit}_A(S, \epsilon, s) = \text{SHA-256}(\mathsf{MerkleRoot}(\mathsf{MT}_H(\sigma(S))) \mathbin\| \epsilon \mathbin\| s).
$$

SHA-256 was chosen because Soroban has a native host function for it. At the R1CS level it was a disaster: a single SHA-256 compression on a 72-byte preimage costs roughly 25,000 R1CS constraints. The small-tier (32 members) circuit was dominated by SHA-256 by more than an order of magnitude.

**The fix.** Replace the outer SHA-256 with two-call Poseidon:

$$
\mathsf{Commit}_B(S, \epsilon, s) = H(H(\mathsf{MerkleRoot}, \epsilon), s).
$$

Two Poseidon evaluations cost roughly 600 R1CS constraints total.

**Table 1.** Measured R1CS constraint counts.

| Tier | Members | Tree depth $d$ | $\mathsf{Commit}_A$ (SHA-256 outer) | $\mathsf{Commit}_B$ (Poseidon outer) | Reduction |
|------|---------|----------------|-------------------------------------|--------------------------------------|-----------|
| Small | 32 | 5 | ~26,740 | **1,910** | 14.0x |
| Medium | 256 | 8 | ~27,640 | **2,630** | 10.5x |
| Large | 2,048 | 11 | ~28,540 | **3,350** | 8.5x |

**Lesson.** Any cryptographic primitive that sits on both sides of an R1CS boundary must have its in-circuit cost counted *first*. A primitive with a cheap host function but an expensive R1CS footprint is a constraint-budget landmine.

### P2 — The theorem that proved the wrong thing

**Severity first.** A critical authorization vulnerability. Any party in the network path between a prover and the contract could substitute the new commitment written to storage, retaining the prover's still-valid proof. Every privacy and authorization guarantee was void for the operation that changes group state.

**What went wrong.** Through v1.2.9 all four state-changing contract operations shared one circuit whose relation was $R_{\mathsf{Mem}}$ with public inputs $(C, \epsilon)$. The `update_commitment` entry point accepted, *in addition to* the proof and its public inputs, a separate `new_commitment: BytesN<32>` parameter; the contract wrote this parameter to storage as the next epoch's state. The proof authorized knowledge of a member in the *current* state. It said nothing about which next state was being authorized. `new_commitment` was a contract-controlled free parameter.

A code-review question exposed the gap: *"Where in the math is the argument that this proof is non-transferable to a different `new_commitment`?"* The answer was: nowhere. Groth16's verification equation incorporates public inputs through the IC linear combination [13], and the proof is cryptographically bound exactly to those public inputs and nothing else.

**Why four audit passes missed it.** The system had passed four reviews — circuit soundness, contract ABI, primitive choice, wire-format correctness. None had "for each cryptographically authorized operation, state what the proof binds and what it does not bind" as a line item.

**The fix.** Split the relation: $R_{\mathsf{Mem}}$ unchanged, $R_{\mathsf{Upd}}$ new with $(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}, C_{\mathsf{new}})$ public. $C_{\mathsf{new}}$ now enters the IC linear combination.

**Lesson.** Formal soundness of a relation $R$ implies nothing about the security of an operation whose security predicate is over variables $R$ does not mention. Our design-review template now requires, per authorized operation: (1) a one-sentence predicate stating what the proof binds; (2) an enumeration of every value the contract reads on that path, labeled "in IC" or "contract-controlled"; (3) a side-by-side read of the relation file and the contract entry-point.

### P3 — Transport-layer co-membership leakage

Onym's clients exchange invitations, rekey envelopes, and group ciphertext over Nostr relays [3]. Every Nostr event is signed by a secp256k1 key. Through v1.2.7 each client signed every event with its *long-lived* identity key. The companion transport specification mentioned the implication in a single bullet of its Security Considerations:

> *"Stable Nostr device keys improve usability but increase sender linkability."*

That bullet is technically true and strategically misleading. The actual consequence was *group linkability*: any relay could cluster the user base by co-membership from public relay data — "these $N$ keys all published to hidden-topic $T$ → these $N$ devices are in the same group."

**The fix.** Ephemeral per-event secp256k1 keys, with a receive-side audit switching four places from `event.pubkey` to the inner BLS public key. Topic tags are now derived per-epoch as $H(\mathsf{group\_id} \| \epsilon)$ and rotate on every state change. We do not claim that no traffic-analysis-based co-membership inference remains — relay-level timing correlation is an open problem — but the two concrete mechanisms that made co-membership trivially observable from public data are both eliminated.

**Lesson.** A privacy invariant that lives in a *Security Considerations* bullet of a different document is fiction. Identity coupling — a single key serving as both signing key and sender identity — is a recurring antipattern; decoupling the two roles is almost free on day one and a postmortem otherwise.

### P4 — The single-machine trusted setup

The preceding postmortems are about bugs that were fixed. This one is a bug that is *not yet fixed*.

Groth16 requires a per-circuit trusted setup [2]. Each of our six verification keys (two relations × three tiers) must be the output of a multi-party computation (MPC) in which at least one honest participant discarded their contribution; otherwise the party who holds the aggregate trapdoor can forge accepting proofs for arbitrary public inputs. Our reference implementation runs the full Powers-of-Tau Phase-1 protocol with multi-party contributions. But the Phase-2 circuit-specific ceremony currently runs on a single machine operated by one party.

Until the Phase-2 ceremony is run as an MPC, every security game in Sec. 5.3 is conditional on trusting the setup operator. The mainnet rollout criterion is a Phase-2 MPC with at least five independent contributors and a scoped external audit of $R_{\mathsf{Upd}}$. Neither has been done.

**Lesson.** A cryptographic prerequisite that is not yet satisfied does not become satisfied by being documented in a limitations section.

## 7. Evaluation

Reference implementation is deployed on Stellar testnet at contract `CC6NUUKG25RSFI6D57HISDQ4HRBLXFAUC3GFVZIACHQX3NLRPYTRWWKE`. A testnet deployment is not production: no real users, no economic stakes, no adversarial environment.

**On-chain verification cost.** Groth16 proofs are 192 bytes; public inputs add 32 bytes for $R_{\mathsf{Mem}}$ and 96 bytes for $R_{\mathsf{Upd}}$. Verification invokes three pairings via Stellar's BLS12-381 host functions plus a small multi-scalar multiplication over the IC; total Soroban instruction count is 6–10 million, inside testnet fee budgets.

**Privacy.** A public-ledger observer learns: that a group exists; number and timestamps of its state changes; an upper bound on group size from the tier; and the relayer's Stellar account. An observer does *not* learn, conditional on intact trusted setup (P4) and Groth16 soundness: the member set, the updater's identity, the exact member count, or whether the group grew, shrank, or rotated. We have *not* run a formal differential-privacy analysis of the timing channel, nor quantified information recoverable from tier-transition patterns.

## 8. Honest limitations

In addition to the open P4:

**Testnet, not mainnet.** Mainnet is gated on P4's MPC and on an external $R_{\mathsf{Upd}}$ audit.

**No committed production customer.** We have had exploratory conversations with a Matrix-adjacent group and one governance-tooling project. Neither has a signed letter of intent.

**Group-size leakage via tier.** The tier is public and leaks an upper bound on group size. Applications for which the existence of a 2,000-member group is itself sensitive should pad to a higher tier.

**Fee-payer correlation via the relayer.** The relayer's own account is a metadata observation point. A single relayer is better than each member being their own fee payer, but worse than a decentralized relayer pool.

**All members must be online provers.** Every membership change requires a current member to generate a Groth16 proof on their device. The privacy guarantee depends on the proof being generated with locally held key material; delegating to a server would reintroduce operator trust.

**Hostile-takeover exposure.** A single current member can replace the entire member set in one update. This is the defining property of the mode studied here; applications that cannot tolerate it should use a different policy [15].

**MLS integration is partial.** Onym maps an MLS Commit to an `update_commitment` call and distributes the fresh salt over MLS's encrypted GroupContext extension. We have not yet integrated Onym with a stock MLS implementation's DS interface; our reference clients use a bespoke MLS-lite implementation.

## 9. Related work

**Table 2.** Approaches to the metadata-hiding group-membership problem.

| Approach | Example(s) | Trust | Metadata-observable by |
|----------|-----------|-------|------------------------|
| Anonymous credentials on a central server | Signal PGS [9] | Signal server (honest-but-curious, binary-faithful) | Not by Signal in honest-but-curious model; by Signal under active compromise |
| Federated delivery | Matrix, XMPP | Every federated homeserver | Federation peers |
| Transparency logs | CT [8], Key Transparency [11] | Log operator | Log operator |
| Mixnets | Nym, Loopix | Mixnet operators (non-colluding threshold) | Network-layer adversary only |
| Public-ledger anchoring + SNARK (this work) | Onym | Chain + SNARK soundness + trusted setup | No operator in the honest-setup case |
| Per-application ZK primitive | Semaphore [4], Tornado Cash [5], Zcash [6] | As above | No operator |

Onym is most directly comparable to Semaphore and Tornado Cash in construction — all three anchor a Merkle tree of commitments on a public ledger and verify membership with a Groth16 SNARK — and distinguishes itself by being a general group-registry primitive coupled to MLS Commit semantics rather than a single-application identity tree.

## 10. Conclusion

Metadata observability is the unsolved half of online-communication privacy. The best-deployed solution today, Signal's Private Group System, closes it under a trust-the-operator model. Onym explores the trust-nobody frame — a public anchor, SNARK-gated state changes, no operator positioned to observe group membership. The mode studied here is the minimalist extreme: it stores the minimum on-chain metadata the architecture can support, at the cost of placing all authorization-strength responsibility on the social layer.

The contribution of this article is the engineering lessons from building toward it on testnet: count R1CS cost before host-function cost when a primitive straddles the circuit boundary; a relation's soundness theorem says nothing about operations whose security predicate is over variables the relation does not mention; privacy invariants that live in Security Considerations bullets of other documents are fiction; and a trusted-setup prerequisite does not become discharged by being mentioned in a later section.

We would prefer that readers take the postmortems more seriously than the system. The system is one more point in a design space. The failure modes generalize to any system in this neighborhood.

## Acknowledgments

We thank the Stellar Development Foundation for Protocol 22's BLS12-381 host functions and the arkworks contributors for the Rust Groth16 and Poseidon gadgets. The reviewer who asked *"where in the math is the argument that this proof is non-transferable?"* is the reason P2 is a postmortem rather than a zero-day.

**Use of generative AI.** This manuscript was co-drafted with Claude Opus (Anthropic, 2025). The system was used across all sections at the level of prose generation, technical exposition, and structural editing. The first author directed the technical content, verified all formal statements, and is responsible for the correctness of the work. All AI-generated text was reviewed and revised by the first author.

## References

[1] R. Barnes et al., "The Messaging Layer Security (MLS) Protocol," IETF RFC 9420, July 2023.

[2] J. Groth, "On the Size of Pairing-Based Non-Interactive Arguments," in *Proc. EUROCRYPT*, 2016, pp. 305-326.

[3] fiatjaf, "NIP-01: Basic Protocol Flow Description," Nostr Implementation Possibilities, 2023.

[4] Privacy & Scaling Explorations, "Semaphore Protocol Specification v4," 2024.

[5] L. Wang, X. Tang, and S. Meiklejohn, "An Empirical Analysis of Privacy in the Tornado Cash Mixer," in *Proc. ACM CCS*, 2022.

[6] D. Hopwood, S. Bowe, T. Hornby, and N. Wilcox, "Zcash Protocol Specification," version 2024.5.0, Electric Coin Company, 2024.

[7] L. Grassi, D. Khovratovich, C. Rechberger, A. Roy, and M. Schofnegger, "Poseidon: A New Hash Function for Zero-Knowledge Proof Systems," in *Proc. USENIX Security*, 2021, pp. 519-535.

[8] B. Laurie, A. Langley, and E. Kasper, "Certificate Transparency," IETF RFC 6962, June 2013.

[9] M. Chase, T. Perrin, and G. Zaverucha, "The Signal Private Group System and Anonymous Credentials Supporting Efficient Verifiable Encryption," in *Proc. ACM CCS*, 2020.

[10] M. V. Hayden, "The Price of Privacy: Re-Evaluating the NSA," Johns Hopkins University, Apr. 7, 2014.

[11] M. S. Melara et al., "CONIKS: Bringing Key Transparency to End Users," in *Proc. USENIX Security*, 2015, pp. 383-398.

[12] R. Barbulescu and S. Duquesne, "Updating Key Size Estimations for Pairings," *J. Cryptol.*, vol. 32, no. 4, pp. 1298-1336, 2019.

[13] H. Lipmaa, "Simulation-Extractable SNARKs Revisited," Cryptology ePrint Archive, Report 2019/612, 2019.

[14] Stellar Development Foundation, "CAP-0046-12: BLS12-381 Host Functions," Stellar Core Advancement Proposal, 2024.

[15] R. Enikeev, "Configurable Group Policies for Onym," design document, Apr. 2026.
