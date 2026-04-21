---
title: "Metadata Without Observers: Postmortems from a SNARK-Gated Group Registry on Stellar"
author: Rinat Enikeev
venue: IEEE Security & Privacy magazine (feature article)
status: Hard-critique revision of shortened draft (~30% reduction per review comment 4282505699)
date: 2026-04-21
word_count_target: ~4,200
---

# Metadata Without Observers: Postmortems from a SNARK-Gated Group Registry on Stellar

## Abstract

End-to-end encryption closed the content half of the online-communication privacy problem. The metadata half — *who is in a group with whom* — still leaks to at least one operator in almost every deployed messaging system. We describe Onym, a testnet system that anchors each group's state on the Stellar blockchain as a 32-byte commitment and gates every state change on a Groth16 zero-knowledge Succinct Non-Interactive Argument of Knowledge (SNARK) over BLS12-381. This article is scoped to a single mode of operation: a group where any current member may unilaterally authorize any next state with a single membership proof. No member address or updater identity appears on the ledger. The group's size tier (≤32, ≤256, or ≤2,048 members) is public; tier transitions therefore monotonically disclose that a group crossed those thresholds, a caveat we keep explicit throughout. The deployed contract supports other policies out of scope here [15]. We state the construction formally — relations, instance-commitment (IC) binding, and a security model discharged under the algebraic group model (AGM) with a random-oracle heuristic for Poseidon, with *operation-binding* discharged under simulation-extractability rather than plain knowledge-soundness. We report **three** postmortems of bugs the project caught and fixed, and **one open security condition** — the Phase-2 trusted-setup ceremony — that currently invalidates every formal guarantee in §5.3 until it is met. Onym is a testnet prototype with no committed production customer. This is a case study in what it took to build toward those properties, and in the specific ways the work kept going wrong.

---

## 1. The metadata problem

Content-level encryption is now a commodity. Signal's Double Ratchet, iMessage, WhatsApp, Google Messages' Messaging-Layer-Security (MLS) rollout, and Matrix's Olm/Megolm all provide end-to-end content encryption. None hide metadata from the operator. iCloud backups hold message metadata for every Apple user. WhatsApp provides subscriber records and message metadata under stored-communications-act orders. Matrix homeservers see every room's membership list by construction. Telegram "secret chats" leave groups, channels, and cloud chats server-readable. Signal is the outlier: repeated grand-jury subpoenas have forced it to disclose only registration time and last-seen date, and its transparency reports substantiate that the list is exhaustive.

Metadata is not a secondary concern. Two of the largest law-enforcement operations of the last five years — the 2020 EncroChat takedown and the 2021 Sky ECC operation — recovered content from compromised devices, but the *targeting* was metadata-driven. Former NSA Director Michael Hayden's remark — "we kill people based on metadata" [10] — is a statement about operational reality.

The structural cause is that every messaging architecture we know of has at least one operator positioned to observe the metadata. In centralized systems it is the server operator. In federated systems (XMPP, Matrix, email) every federated server learns its tenants' metadata. In MLS deployments the Delivery Service (DS) is the ordering authority and therefore the metadata authority — it sees each Commit, each Welcome, and which device-pseudonyms are present in each group. The MLS specification [1] is explicit that the DS is trusted for ordering. Recent work on private MLS Delivery Services [23] is the closest prior work on that half of the problem.

This paper is not a proposal to replace these deployed systems. Signal's Private Group System [9] is an existence proof that strong metadata privacy is compatible with a commercial messenger at scale. The question Onym asks is narrower: **can we build a group-membership registry in which no application-layer operator observes membership, and in which the authorization rules are externally auditable from the public contract bytecode alone?** Signal is the gold standard within a trust-the-operator frame; Onym explores an adjacent design point. We are careful not to advertise that design point as "trust nobody": as §2 and §8 make explicit, a public-ledger anchor reduces the trusted party to the chain and its validator set, which is materially weaker than an application-server position but not cryptographically absent.

## 2. Why blockchain + SNARKs

Four candidate approaches exist.

1. **Transparency logs** (Certificate / Key Transparency [11]) hide content but do not hide who is writing to which key — not suited to a group-registry setting.
2. **Federated delivery** (Matrix, XMPP, email) distributes trust but does not eliminate per-server metadata observation.
3. **Anonymous-credential-backed central servers** (Signal PGS [9]) give excellent metadata privacy under an honest-but-curious server. A third party auditing Onym can verify, from public chain state and the deployed contract bytecode alone, that every state transition was authorized by a valid SNARK — without trusting any server binary to match any spec. For Signal PGS, the equivalent verification requires trusting that the running binary faithfully implements the published protocol.
4. **Public-ledger anchoring plus zero-knowledge (ZK) membership proofs** commit group state on a public chain and verify membership with a SNARK. All observers eventually read the same chain, but not all observers read it at the same time: block producers see submitted transactions seconds before the rest of the network, and a relayer that submits to a specific validator exposes submission-time/inclusion-time correlation on that timescale. The trust assumptions are therefore on the chain's censorship resistance, on SNARK soundness, on the SNARK's trusted setup — and, at finer granularity, on the validator set not colluding with the relayer to correlate submissions.

Onym takes the fourth approach. Within it, we chose Stellar over Ethereum because Stellar Protocol 22 [14] introduced BLS12-381 host functions, reducing Groth16 verification to three pairings and a small multi-scalar multiplication at negligible fee cost. BN254's effective security dropped to an estimated 100–103 bits after improved number-field-sieve attacks [12]; BLS12-381 is at roughly 120 bits of pairing security. Our threshold is 120 bits. Stellar's tier-1 validator set is public, known, and numbers on the order of tens of organizations; we treat it as a **semi-trusted party** distinct from ordinary observers, and we revisit what that means concretely in §8.

## 3. Scope of this article

The deployed Onym contract supports several authorization policies for state changes. This article is scoped to a single one, which we call the **unanimous-member mode**: a group where any current member, alone, may authorize any next state. That is the complete characterization needed to follow the rest of the paper; the other policies are out of scope and described in the companion design document [15]. Readers should not generalize the privacy properties reported here to those other policies.

All three postmortems in §6 and the open condition in §8 concern v1.0.0–v1.6.7, which supported only the unanimous-member mode; other policies shipped in v1.7.0 on 2026-04-19.

## 4. In plain language

Picture a town hall that must keep a membership register for many private clubs, and picture the town's constitution prohibiting the hall from knowing who is in any club. Instead of writing names, the hall posts one sealed envelope per club. The envelope is a single short line of text that reveals nothing about members. A member who walks up to the desk can prove they belong inside the envelope without anyone opening it — they hand over a small slip of paper, the desk staff perform a fixed arithmetic check, and the staff return "yes" or "no." No staff member ever learns who the member is.

When the club's membership changes, one current member computes what the club's *next* envelope should say, writes both old and new envelopes on the slip, proves "I belong inside yesterday's envelope, and today's envelope is the correct next version of it," and hands the slip over. The hall swaps envelopes. Nothing on the hall's wall ever named a member. Any current member can perform that swap alone, and the hall has no way to tell which member did so.

The real architecture follows that picture literally:

- The *hall* is the Stellar blockchain. Anyone can read every envelope, which is just a 32-byte number per group.
- The *envelope* is a cryptographic commitment: an opaque hash-tree of the member set, bound to an epoch counter and a random salt.
- The *slip of paper* is a 192-byte Groth16 proof.
- The *arithmetic check* is three pairings on BLS12-381.
- The *member's secret* is a BLS12-381 private key held on their device.
- The *clerk behind the desk* is the public Soroban smart contract `CC6NUUKG25RSFI6D57HISDQ4HRBLXFAUC3GFVZIACHQX3NLRPYTRWWKE`.

Two subtleties matter. First, if a member pays the Stellar transaction fee from their own account, that account's public key appears in the transaction envelope and re-links a real-world identity to group activity. Onym members therefore submit their proof to a public relayer; the relayer wraps the proof in a Stellar transaction signed by its own account. The relayer cannot forge a proof or change which group is updated, but it can withhold submissions — we treat it as a liveness, not a safety, dependency.

Second, each group is created inside one of three fixed *size tiers*: ≤32, ≤256, or ≤2,048 members. The tier is public and fixed at creation; if a group outgrows its tier it must be re-created at a larger one, and that re-creation is observable on-chain. The abstraction does *not* hide the fact that a group grew past 32 or past 256 members — it hides only the exact count within a tier. We state this explicitly because phrasings like "no group-size signal beyond a tier" obscure the fact that tier transitions are a **monotonic** disclosure: once a group crosses a threshold, that crossing is public forever.

What an outside observer of the public chain learns is: a group exists; its envelope has been swapped *k* times at timestamps *t₁, …, tₖ*; the group's current size tier; any prior tier the group graduated out of; and the relayer paying the fee. The observer does not learn who is in the group, who initiated any swap, the exact count within a tier, or whether the group shrank or rotated (neither of those changes the tier). A Stellar validator additionally learns the relayer's submission time before block inclusion; §8 discusses why that extra observation window is qualitatively weaker than an application-server observation point, without claiming it is absent.

## 5. Formal construction

Let $\mathbb{F}_r$ be the BLS12-381 scalar field and $\mathbb{G}_1, \mathbb{G}_2, \mathbb{G}_T$ the pairing groups with generator $G_1 \in \mathbb{G}_1$. Let $H: \mathbb{F}_r^* \to \mathbb{F}_r$ denote Poseidon [7] at the standard BLS12-381 parameter set. Throughout the security analysis, we treat *out-of-circuit* evaluations of $H$ as a random oracle and model *in-circuit* evaluations concretely, inside Groth16's knowledge-soundness argument. We flag where each modelling choice is load-bearing.

### 5.1 Commitment

For a member set $S = \{\mathsf{pk}_1, \ldots, \mathsf{pk}_n\} \subseteq \mathbb{G}_1$ with $n \leq 2^d$, let $\sigma(S)$ be the lexicographic ordering under compressed-G1 byte encoding and $\mathsf{MT}_H$ the Poseidon Merkle tree of depth $d$ with leaf $H(\mathsf{pk})$ and zero padding $H(0)$. The commitment is

$$
\mathsf{Commit}(S, \epsilon, s) \;=\; H\!\bigl( H(\mathsf{MerkleRoot}(\mathsf{MT}_H(\sigma(S))),\, \epsilon),\, s \bigr) \in \mathbb{F}_r,
$$

where $\epsilon \in \mathbb{F}_r$ is the epoch counter and $s \in \mathbb{F}_r$ is a per-epoch salt uniform in $\mathbb{F}_r$. The canonicalization $\sigma$ ensures $\mathsf{Commit}$ is well-defined on unordered sets.

### 5.2 Relations

Two NP relations, both compiled to Rank-1 Constraint System (R1CS) and proved with Groth16 [2], cover every operation of the mode studied here.

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

$R_{\mathsf{Mem}}$ is invoked by the `create_group`, `verify_membership`, and `deactivate_group` contract entry points; $R_{\mathsf{Upd}}$ is invoked by `update_commitment`.

Critically, $C_{\mathsf{new}}$ is a *public input*: it enters the verifier's instance-commitment (IC) linear combination and is part of the statement the proof binds. Postmortem P2 (§6) describes what went wrong when $C_{\mathsf{new}}$ was *not* a public input.

**Authorization semantics.** The relation binds $C_{\mathsf{new}}$ to *some* new root and *some* new salt, but does not constrain the relationship between $\mathsf{root}_{\mathsf{old}}$ and $\mathsf{root}_{\mathsf{new}}$. A valid proof can transition from any old root to any new root. A single current member can therefore replace the entire member set in one operation; that is by design. Constraining the diff would roughly double the R1CS constraint count and expose an on-chain ±1 member-count signal the design is intended to avoid.

**Replay and freshness.** Replay is defended at the *contract* layer, not inside the relation. The `update_commitment` entry point verifies that the public input $C_{\mathsf{old}}$ equals the group's currently stored commitment *and* that $\epsilon_{\mathsf{old}}$ equals the stored epoch; on success it atomically writes $C_{\mathsf{new}}$ and increments the stored epoch. Consequently, once a group advances past epoch $\epsilon$, every proof whose public input mentions $(C_\epsilon, \epsilon)$ fails the entry-point precondition — even if captured verbatim off the wire, and even if held by a former member whose private key was never rotated. A captured proof still carries authorization power *at most* until the next legitimate update wins the race on-chain; under Soroban's sequential execution model, first-write wins and any replay at that point is equivalent to having authorized the same next state the legitimate prover was already submitting. We state this as a property of the deployed bytecode; checking that the entry-point preconditions match this specification was added to the design-review template after P2.

### 5.3 Security model and games

We state each game between a probabilistic polynomial-time adversary $\mathcal{A}$ and a challenger $\mathcal{C}$, with $\lambda = 128$ commensurate with BLS12-381. The underlying assumptions are:

- **(A1)** Groth16 is knowledge-sound in the **algebraic group model** (AGM) of Fuchsbauer, Kiltz, and Loss [17]. We use the AGM rather than the strictly stronger generic group model; AGM is what recent Groth16 analyses target and narrows the idealization to the pairing group only.
- **(A2)** With the public-input hashing modification of Baghery, Kohlweiss, Siim, and Volkhov [18] (building on Lipmaa [13]), Groth16 is **simulation-extractable** (SE) in AGM+ROM. Plain Groth16 is malleable [16], so SE — not plain knowledge-soundness — is the property we need for operation-binding.
- **(A3)** Out-of-circuit evaluations of Poseidon are modelled as a random oracle. This is a heuristic on the sponge construction, not a theorem from standard hardness assumptions; we flag the games that rely on it.
- **(A4)** The Phase-2 trusted setup is honest (at least one contributor discarded their toxic waste). This assumption does *not* currently hold; see §8, open condition Φ, which makes every statement below conditional.

*Soundness* (for each of $R_{\mathsf{Mem}}$ and $R_{\mathsf{Upd}}$). $\mathcal{A}$ is given the verification key and outputs $(\pi, x)$; wins if $\mathsf{Verify}(\mathsf{vk}, x, \pi) = 1$ and no witness $w$ satisfies $R(x; w)$. *Reduction:* under (A1) and (A4), Groth16's AGM extractor produces such a $w$ from any accepting $\mathcal{A}$, contradicting the winning condition.

*Zero-knowledge.* Standard Groth16 simulation-ZK under (A1), (A4) [2, 17].

*Commitment hiding.* $\mathcal{A}$ outputs $S_0, S_1$ with $|S_0| = |S_1| \leq 2^d$; $\mathcal{C}$ samples $b \leftarrow \{0,1\}$, $s \leftarrow \mathbb{F}_r$, returns $C = \mathsf{Commit}(S_b, \epsilon, s)$; $\mathcal{A}$ outputs $b'$ and wins with non-negligible advantage over $1/2$. *Reduction:* under (A3), the outer Poseidon evaluation $H(\cdot, s)$ is a random oracle queried on input $(y, s)$ where $y$ depends on $S_b$ and $s$ is uniform in $\mathbb{F}_r$. Since $\mathcal{A}$ is computationally bounded and $s$ has $\sim 255$ bits of entropy, with overwhelming probability $\mathcal{A}$ never queries the oracle at $(y_b, s)$, so $C$ is uniform and independent of $b$. We are explicit that this does *not* treat Poseidon as a PRF; the argument is indistinguishability from uniform under a uniform hidden salt in ROM, and can be strengthened to a concrete sponge-indistinguishability assumption rather than full ROM if desired.

*Operation-binding.* $\mathcal{A}$ queries an honest prover oracle on chosen triples $(C_i, \epsilon_i, C'_i)$ and receives valid proofs $\pi_i$; then $\mathcal{A}$ outputs $(\pi^*, C, \epsilon, C^{**})$ with $(C, \epsilon) = (C_i, \epsilon_i)$ for some $i$ and $C^{**} \neq C'_i$, such that $\mathsf{Verify}$ accepts. *Reduction:* plain Groth16 knowledge-soundness is **insufficient** here because Groth16 is malleable [16]; the original draft's claim that "IC binding once $C_{\mathsf{new}}$ is a public input" discharges this game was a category error. The correct assumption is (A2): simulation-extractability of Groth16 in AGM+ROM with the public-input-hashing modification [13, 18]. The SE extractor, run on $\mathcal{A}$, produces a witness $w^*$ satisfying $R_{\mathsf{Upd}}(C, \epsilon, C^{**}; w^*)$ even though $\mathcal{A}$ was only given simulated/honest proofs for $C'_i \neq C^{**}$. Since $w^*$ in particular contains $(\mathsf{root}_{\mathsf{new}}, s_{\mathsf{new}})$ with $H(H(\mathsf{root}_{\mathsf{new}}, \epsilon + 1), s_{\mathsf{new}}) = C^{**}$, $\mathcal{A}$ trivially knows a preimage of a commitment it claims to have produced without one. The deployed contract incorporates the public-input-hashing modification; without it, SE of Groth16 is not established in the literature. Onym *failed* this game from v1.0.0 through v1.2.x — see P2.

*Fee-payer unlinkability.* We split this into an idealized and a realistic game; the $1/n$ bound belongs only to the idealized one.

**Ideal (no auxiliary information):** $\mathcal{C}$ samples a group of size $n \geq 2$ and a uniformly chosen prover index $i \leftarrow [n]$; the prover generates $\pi$ and submits it via the relayer. $\mathcal{A}$ sees only the on-chain transaction and outputs a guess $\hat{i}$; advantage is $\Pr[\hat{i} = i] - 1/n$. *Reduction:* the on-chain byte-string is a constant-size 192-byte proof plus public inputs that are functions of group state (not of $i$), wrapped in a transaction whose fee-paying key is the relayer's; zero-knowledge of Groth16 under (A1) closes the argument.

**Realistic (with auxiliary information $\mathsf{aux}$):** $\mathsf{aux}$ denotes the adversary's side-channel observations on member $i$ — relayer-ingress timing, P3-class Nostr traffic patterns (§6.P3), and block-producer submission-time data accessible to the Stellar validator set (§2, §8). $\mathcal{A}$'s advantage is $\Pr[\hat{i} = i \mid \mathsf{aux}] - 1/n$, and the game as written does *not* upper-bound it. The relayer pattern eliminates the on-chain fee-payer signal; it does not eliminate timing. For $n = 2$ (pair chats) the ideal-bound statement is vacuous and the realistic bound is the operative one; no mechanism in Onym drives the realistic bound below $1/2$ for pair chats. We make no claim of unlinkability beyond "on-chain metadata, holding $\mathsf{aux}$ fixed, contributes no additional signal."

Every statement in §5.3 is conditional on (A4). §8 discusses what happens when (A4) is relaxed — the short answer is: everything breaks.

## 6. Three postmortems

Each episode was caught, fixed, and generalized into a design-review checklist item. All three concern bugs that were actually repaired. The unfixed prerequisite — the Phase-2 trusted setup — is not a postmortem of a closed issue and is discussed separately in §8 as open condition Φ.

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

**What went wrong.** Through v1.2.9 every state-changing contract operation shared one circuit whose relation was $R_{\mathsf{Mem}}$ with public inputs $(C, \epsilon)$. The `update_commitment` entry point accepted, *in addition to* the proof and its public inputs, a separate `new_commitment: BytesN<32>` parameter; the contract wrote this parameter to storage as the next epoch's state. The proof authorized knowledge of a member in the *current* state. It said nothing about which next state was being authorized. `new_commitment` was a contract-controlled free parameter.

**Who caught it.** An external code reviewer, contracted specifically for a Groth16-circuit-to-contract-ABI seam pass, asked: *"Where in the math is the argument that this proof is non-transferable to a different `new_commitment`?"* The answer was: nowhere. Groth16's verification equation incorporates public inputs through the IC linear combination [13], and a proof is cryptographically bound exactly to those public inputs and nothing else. A second subtlety surfaced during the fix discussion: even once $C_{\mathsf{new}}$ is made a public input, plain Groth16 is malleable [16], so the right formal property is simulation-extractability [13, 18] — see §5.3, *Operation-binding*. Making $C_{\mathsf{new}}$ an IC input was necessary; it was not sufficient until the public-input-hashing modification of [18] was added.

**Why the preceding four reviews did not catch it.** The system had passed four structured reviews — circuit soundness, contract ABI, primitive choice, and wire-format correctness. Each was well-scoped within itself. The seam that failed was not inside any one review's scope but *between* them: the circuit review verified $R_{\mathsf{Mem}}$ was sound for membership, the ABI review verified `update_commitment` took well-typed arguments, and no pass owned the question of whether the operation's security predicate was expressible in terms of the statement the proof actually bound. The lesson is about review topology — scope coverage rather than scope depth — not about any individual reviewer.

**The fix.** Split the relation: $R_{\mathsf{Mem}}$ unchanged, $R_{\mathsf{Upd}}$ new with $(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}, C_{\mathsf{new}})$ public. $C_{\mathsf{new}}$ now enters the IC linear combination, and public inputs are additionally hashed into the Fiat-Shamir transcript per [18] to obtain SE.

**Lesson.** Formal soundness of a relation $R$ implies nothing about the security of an operation whose security predicate is over variables $R$ does not mention. Our design-review template now requires, per authorized operation: (1) a one-sentence predicate stating what the proof binds; (2) an enumeration of every value the contract reads on that path, labeled "in IC" or "contract-controlled"; (3) a side-by-side read of the relation file and the contract entry-point; (4) an explicit statement of whether knowledge-soundness or simulation-extractability is the intended security level, and why.

### P3 — Transport-layer co-membership leakage

Onym's clients exchange invitations, rekey envelopes, and group ciphertext over Nostr relays [3]. Every Nostr event is signed by a secp256k1 key. Through v1.2.7 each client signed every event with its *long-lived* identity key. The companion transport specification mentioned the implication in a single bullet of its Security Considerations:

> *"Stable Nostr device keys improve usability but increase sender linkability."*

That bullet is technically true and strategically misleading. The actual consequence was *group linkability*: any relay could cluster the user base by co-membership from public relay data — "these $N$ keys all published to hidden-topic $T$ → these $N$ devices are in the same group."

**The fix.** Ephemeral per-event secp256k1 keys, with a receive-side audit switching four places from `event.pubkey` to the inner BLS public key. Topic tags are now derived per-epoch as $H(\mathsf{group\_id} \mathbin\| \epsilon)$ and rotate on every state change. We do not claim that no traffic-analysis-based co-membership inference remains — relay-level timing correlation is an open problem and also feeds the realistic fee-payer unlinkability bound (§5.3) — but the two concrete mechanisms that made co-membership trivially observable from public data are both eliminated.

**Lesson.** A privacy invariant that lives in a *Security Considerations* bullet of a different document is fiction. Identity coupling — a single key serving as both signing key and sender identity — is a recurring antipattern; decoupling the two roles is almost free on day one and a postmortem otherwise.

## 7. Evaluation

Reference implementation is deployed on Stellar testnet at contract `CC6NUUKG25RSFI6D57HISDQ4HRBLXFAUC3GFVZIACHQX3NLRPYTRWWKE`. A testnet deployment is not production: no real users, no economic stakes, no adversarial environment.

**On-chain verification cost.** Groth16 proofs are 192 bytes; public inputs add 32 bytes for $R_{\mathsf{Mem}}$ and 96 bytes for $R_{\mathsf{Upd}}$. Verification invokes three pairings via Stellar's BLS12-381 host functions plus a small multi-scalar multiplication over the IC; total Soroban instruction count is 6–10 million, inside testnet fee budgets.

**What a public-chain observer learns**, conditional on honest Phase-2 setup (§8, Φ), Groth16 SE in AGM+ROM, and the Poseidon ROM heuristic: that a group exists; the number and timestamps of its state changes; its current size tier and any prior tiers it graduated out of; and the relayer paying the fee. The observer does *not* learn the member set, the updater's identity, the exact member count within a tier, or whether the group shrank or rotated within tier. A **Stellar validator** additionally learns the relayer's submission time on a seconds-scale window around block inclusion. We have *not* run a formal differential-privacy analysis of the timing channel, nor quantified information recoverable from tier-transition patterns; both are future work.

## 8. Honest limitations and open conditions

### Φ. Open condition — Phase-2 trusted setup

This is the load-bearing limitation, not a postmortem. Groth16 requires a per-circuit trusted setup [2]. Each of our six verification keys (two relations × three tiers) must be the output of a multi-party computation (MPC) in which at least one honest participant discarded their contribution; otherwise the party who holds the aggregate trapdoor can forge accepting proofs for arbitrary public inputs. Our reference implementation runs the full Powers-of-Tau Phase-1 protocol with multi-party contributions. The Phase-2 circuit-specific ceremony currently runs on a single machine operated by one party. **Until this is remedied, assumption (A4) of §5.3 does not hold, and every security game there — soundness, ZK, commitment hiding (via forged proofs about $C$), operation-binding, and both fee-payer unlinkability variants — is defeated by the setup operator.** The games are not "weakened" by the current Phase-2 state; they are vacuous. Mainnet rollout is gated on a Phase-2 MPC with at least five independent contributors and a scoped external audit of $R_{\mathsf{Upd}}$. Neither has been done. A cryptographic prerequisite that is not yet satisfied does not become satisfied by being documented in a limitations section; we list it first, and in its own subsection, precisely so that no reader of §5.3 treats its guarantees as currently in force.

### Other limitations

**Testnet, not mainnet.** Mainnet is gated on Φ and on an external $R_{\mathsf{Upd}}$ audit.

**No committed production customer.** We have had exploratory conversations with a Matrix-adjacent group and one governance-tooling project. Neither has a signed letter of intent.

**Validator-set observation point.** Stellar's tier-1 validator set is public, known, and small. A validator sees a transaction before the network does, and a relayer that submits to a specific validator exposes submission-time/inclusion-time correlation on a seconds-scale window that ordinary observers cannot replicate. This is the residual semi-trusted party in the architecture. We argue it is qualitatively weaker than an application-server observation point on two grounds: (i) it does not determine *which* transactions exist (the relayer does), and (ii) its observation window is bounded to the ledger-closing interval rather than the indefinite retention of server logs. We do not argue it is cryptographically absent. Applications whose threat model includes a nation-state-scale adversary with validator-set collusion should assume this channel is live and design their external traffic patterns accordingly.

**Group-size leakage via tier transitions.** The tier is public and fixed at group creation. A group that has lived in the Small tier and then is re-created into Medium has monotonically disclosed that it grew past 32 members; that disclosure does not expire. Applications for which such thresholds are themselves sensitive should start at the largest tier they might ever need.

**Fee-payer correlation via the relayer.** The relayer's own account is a metadata observation point. A single relayer is better than each member being their own fee payer, but worse than a decentralized relayer pool.

**All members must be online provers.** Every membership change requires a current member to generate a Groth16 proof on their device. The privacy guarantee depends on the proof being generated with locally held key material; delegating to a server would reintroduce operator trust.

**Hostile-takeover exposure.** A single current member can replace the entire member set in one update. This is the defining property of the mode studied here; applications that cannot tolerate it should use a different policy [15].

**MLS integration is a sketch, not a demonstrated integration.** Earlier drafts claimed Onym "maps an MLS Commit to an `update_commitment` call." That claim overstates what has been built. Our reference clients use a bespoke MLS-lite transport, not a stock MLS implementation; we have *not* integrated with a conforming Delivery-Service interface, handled Welcome messages in a spec-compliant way, nor designed the GroupContext-extension encoding for the refreshed salt. We *sketch* how the mapping would work — an MLS Commit triggers an epoch advance, the resulting leaf set determines $\mathsf{root}_{\mathsf{new}}$, and the salt would travel in an encrypted GroupContext extension — but the state transitions, Welcome handling, and DS-vs-Onym reconciliation are unresolved. A concrete integration is future work; for the DS-privacy half of it, Mafia [23] is the most directly relevant prior work.

## 9. Related work

Table 2 locates Onym within the space of approaches to the metadata-hiding group-membership problem. We place primary comparisons against other ZK-membership-registry constructions on public ledgers, which are the nearest neighbors in construction.

**Table 2.** Approaches to the metadata-hiding group-membership problem.

| Approach | Example(s) | Trust | Application-layer metadata observable by |
|----------|-----------|-------|------------------------|
| Anonymous credentials on a central server | Signal PGS [9] | Signal server (honest-but-curious, binary-faithful) | Not by Signal in HbC; by Signal under active compromise |
| Federated delivery | Matrix, XMPP | Every federated homeserver | Federation peers |
| Transparency logs | CT [8], Key Transparency [11] | Log operator | Log operator |
| Mixnets | Nym, Loopix | Mixnet operators (non-colluding threshold) | Network-layer adversary only |
| Per-application ZK primitive, privacy-pool style | Semaphore [4], Tornado Cash [5], Zcash [6], Railgun [19], AZTEC [20], Nocturne [22] | Chain + SNARK soundness + trusted setup | No application operator; chain validators see submission timing |
| Auditable ZK transactions with controlled disclosure | Azeroth [21] | As above, plus a designated auditor | Auditor sees disclosed subsets |
| Private MLS Delivery Service | Mafia [23] | MLS DS reduced to ordering oracle over ciphertext | Network-layer adversary only |
| Public-ledger anchoring + SNARK group registry (this work) | Onym | Chain + SNARK SE + trusted setup + Poseidon ROM heuristic | No application operator; Stellar validators see submission timing |

Onym's construction is closest in shape to Semaphore [4], Tornado Cash [5], Railgun [19], AZTEC [20], and Nocturne [22]: all six anchor a commitment tree on a public ledger and verify membership (or spend authority) with a Groth16 or Plonk SNARK. Onym differs from each in that its tree commits to an *evolving group-membership set*, rather than a fixed anonymity set of deposit notes or identity commitments, and its authorization predicate is over state *transitions* (old-commitment / new-commitment pairs) rather than single-statement membership. Azeroth [21] shares the public-ledger + ZK pattern but targets auditable transaction semantics with a designated auditor — an explicitly different goal. Mafia [23] addresses the MLS-side problem — Delivery-Service metadata privacy — and complements, rather than competes with, Onym's on-chain registry; the two would plausibly compose in a production stack, which is exactly the integration §8 declines to claim we have done.

## 10. Conclusion

Metadata observability is the unsolved half of online-communication privacy. The best-deployed solution today, Signal's Private Group System, closes it under a trust-the-operator model. Onym explores an adjacent frame — a public anchor, SNARK-gated state changes, no application-layer operator positioned to observe group membership — and is candid that the frame is not trust-free: the chain's validator set is a semi-trusted party, the Phase-2 trusted setup is an open cryptographic condition that currently invalidates every formal guarantee, and several privacy bounds (fee-payer unlinkability under auxiliary information, tier-transition monotonic disclosure) are meaningfully weaker than an idealized framing suggests. The mode studied here is the minimalist extreme: it stores the minimum on-chain metadata the architecture can support, at the cost of placing all authorization-strength responsibility on the social layer.

The contribution of this article is the engineering and security-modelling lessons from building toward it on testnet: count R1CS cost before host-function cost when a primitive straddles the circuit boundary; a relation's soundness theorem says nothing about operations whose security predicate is over variables the relation does not mention, and operation-binding over Groth16 specifically needs simulation-extractability rather than plain knowledge-soundness; privacy invariants that live in Security Considerations bullets of other documents are fiction; and a trusted-setup prerequisite does not become discharged by being mentioned in a later section.

We would prefer that readers take the postmortems and the open condition more seriously than the system. The system is one point in a design space. The failure modes generalize to any system in this neighborhood.

## Acknowledgments

We thank the Stellar Development Foundation for Protocol 22's BLS12-381 host functions and the arkworks contributors for the Rust Groth16 and Poseidon gadgets. We thank the external code reviewer, contracted for a Groth16-circuit-to-contract-ABI seam pass, whose question *"where in the math is the argument that this proof is non-transferable?"* is the reason P2 is a postmortem rather than a zero-day. We also thank the anonymous reviewer whose hard critique drove the §5.3 rewrite (simulation-extractability, AGM, Poseidon-as-ROM) and the reframing of P4 as open condition Φ.

**Use of generative AI.** This manuscript was *drafted with editorial assistance from Claude* (Anthropic, 2026), used for prose generation, technical exposition, and structural editing. The first author directed the technical content, wrote and verified all formal statements (relations, games, reductions), and is responsible for the correctness of the work. Claims about the literature — in particular the AGM / SE / ROM choices in §5.3 — were verified by the first author against the cited sources and are not AI-produced.

## References

[1] R. Barnes et al., "The Messaging Layer Security (MLS) Protocol," IETF RFC 9420, July 2023.

[2] J. Groth, "On the Size of Pairing-Based Non-Interactive Arguments," in *Proc. EUROCRYPT*, 2016, pp. 305–326.

[3] fiatjaf, "NIP-01: Basic Protocol Flow Description," Nostr Implementation Possibilities, 2023.

[4] Privacy & Scaling Explorations, "Semaphore Protocol Specification v4," 2024.

[5] L. Wang, X. Tang, and S. Meiklejohn, "An Empirical Analysis of Privacy in the Tornado Cash Mixer," in *Proc. ACM CCS*, 2022.

[6] D. Hopwood, S. Bowe, T. Hornby, and N. Wilcox, "Zcash Protocol Specification," version 2024.5.0, Electric Coin Company, 2024.

[7] L. Grassi, D. Khovratovich, C. Rechberger, A. Roy, and M. Schofnegger, "Poseidon: A New Hash Function for Zero-Knowledge Proof Systems," in *Proc. USENIX Security*, 2021, pp. 519–535.

[8] B. Laurie, A. Langley, and E. Kasper, "Certificate Transparency," IETF RFC 6962, June 2013.

[9] M. Chase, T. Perrin, and G. Zaverucha, "The Signal Private Group System and Anonymous Credentials Supporting Efficient Verifiable Encryption," in *Proc. ACM CCS*, 2020.

[10] D. Cole, "'We Kill People Based on Metadata,'" *New York Review of Books*, May 10, 2014. Quoting Gen. M. V. Hayden at the Johns Hopkins Foreign Affairs Symposium, Apr. 7, 2014.

[11] M. S. Melara et al., "CONIKS: Bringing Key Transparency to End Users," in *Proc. USENIX Security*, 2015, pp. 383–398.

[12] R. Barbulescu and S. Duquesne, "Updating Key Size Estimations for Pairings," *J. Cryptol.*, vol. 32, no. 4, pp. 1298–1336, 2019.

[13] H. Lipmaa, "Simulation-Extractable SNARKs Revisited," Cryptology ePrint Archive, Report 2019/612, 2019.

[14] Stellar Development Foundation, "CAP-0059: Host functions for BLS12-381," Stellar Core Advancement Proposal, Aug. 2024.

[15] R. Enikeev, "Configurable Group Policies for Onym," design document, Apr. 2026.

[16] G. Fuchsbauer, "Subversion-Zero-Knowledge SNARKs," in *Proc. Public-Key Cryptography (PKC)*, 2018. (Establishes malleability of Groth16 and motivates simulation-extractability for any application requiring non-malleability of public inputs.)

[17] G. Fuchsbauer, E. Kiltz, and J. Loss, "The Algebraic Group Model and its Applications," in *Proc. CRYPTO*, 2018, pp. 33–62.

[18] K. Baghery, M. Kohlweiss, J. Siim, and M. Volkhov, "Another Look at Extraction and Randomization of Groth's zk-SNARK," in *Proc. Financial Cryptography*, 2021. (Weak-SE of a modified Groth16 in AGM+ROM; public-input-hashing modification.)

[19] Railgun Project, "Railgun: Privacy System for Ethereum and EVM Chains," technical whitepaper, 2022.

[20] Z. J. Williamson, "The AZTEC Protocol," technical whitepaper, 2018.

[21] G. Chen et al., "Azeroth: Auditable Zero-Knowledge Transactions in Smart Contracts," 2024. (Auditable ZK payment construction with designated auditor; nearest neighbor to Onym for audit-oriented ZK-on-ledger designs.)

[22] Nocturne Labs, "Nocturne: Private Account Abstraction on Ethereum," technical whitepaper, 2023.

[23] T. Melissaris et al., "Private Delivery Services for Messaging Layer Security" (project codename: *Mafia*), in *Proc. EUROCRYPT*, 2025. (Closest prior work on MLS DS metadata privacy; complementary to Onym's on-chain registry.)
