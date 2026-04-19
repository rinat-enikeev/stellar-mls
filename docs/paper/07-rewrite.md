---
title: "Metadata Without Observers: A Postmortem-Driven Case Study of Private Group Membership on a Public Ledger"
author: Rinat Enikeev
venue: IEEE Security & Privacy magazine (feature article)
status: Rewrite from scratch, addressing 06-hard-critique.md
date: 2026-04-19
word_count_target: 5,500
---

# Metadata Without Observers: A Postmortem-Driven Case Study of Private Group Membership on a Public Ledger

## Abstract

End-to-end encryption closed the content half of the online-communication privacy problem. The metadata half — *who is in a group with whom, when they joined, how the group grew* — is still leaking, and almost every deployed messaging system today leaks it to at least one operator. We describe Onym, a testnet system that tries to close the metadata gap for the specific case of group membership by anchoring each group's state on the Stellar blockchain as a 32-byte commitment and gating every state change on a Groth16 zero-knowledge proof over BLS12-381. No member address, no updater identity, and no group-size signal beyond a coarse tier ever appears on the ledger. The article has two parts. The first introduces the solution twice: once in plain language for readers who have not touched a SNARK, and once formally for readers who want the relations, the IC binding, and the game-based security statements. The second part is four postmortems, each an episode in which the system's correctness drifted off the rails — a constraint-cost crisis that almost killed mobile proving; a critical authorization vulnerability in which a formally sound theorem described the wrong predicate and four audit passes missed the gap; a transport-layer co-membership leak caused by stable signing keys; and a single-machine trusted-setup shortcut that currently caps the system's security at one-party trust. Onym is not yet a mainnet system and does not yet have a production customer. This is a case study in what it took to build toward those properties, and in the specific ways the work kept going wrong.

---

## 1. The metadata problem for online communication

The last decade made content-level encryption a commodity. Signal's Double Ratchet, Apple's iMessage, WhatsApp, Google Messages' RCS-with-MLS rollout, and Matrix's Olm/Megolm all provide some form of end-to-end content encryption. None of them hide metadata from the operator. iCloud backups hold iMessage content for the subset of Apple users who do not enable Advanced Data Protection, and hold message metadata for *every* user. WhatsApp's transparency report for 2021 disclosed that the service provides "basic subscriber records" and, under stored-communications-act orders, message metadata routinely; content it does not have. Matrix homeservers see every room's membership list by construction. Telegram "secret chats" leave everything else — groups, channels, cloud chats — with server-readable metadata. Signal is the outlier: repeated grand-jury subpoenas (2016 Virginia, 2020 California, 2021 Santa Clara) have forced Signal to disclose the only metadata Signal holds about a user — registration time and last-seen date — and its transparency reports substantiate that list is exhaustive. That is a deliberate and unusual engineering achievement. It is not the industry norm.

Metadata is not a secondary concern. Two of the largest law-enforcement operations of the last five years — the 2020 EncroChat takedown (roughly 60,000 users across Europe) and the 2021 Sky ECC operation (roughly 170,000 users) — recovered content from compromised devices, but the *targeting* that led to each takedown was metadata-driven: which devices were talking to which, at what rate, in what clusters. A former NSA General Counsel's well-known remark — "we kill people based on metadata" [12] — is a statement about operational reality, not a rhetorical flourish. For many threat models, who-talks-to-whom is more damaging than what was said.

The structural cause is that every messaging architecture we know of has at least one operator who is structurally positioned to observe the metadata. In centralised systems it is the server operator. In federated systems (XMPP, Matrix, email) every federated server learns the metadata of its tenants. In Messaging Layer Security (MLS) deployments the Delivery Service is the ordering authority and therefore the metadata authority — it sees each Commit, each Welcome, and which device-pseudonyms are present in each group. MLS's specification [1] is explicit that the Delivery Service is trusted for ordering; most deployments (Google Messages, Cisco Webex, Wire) reasonably fold that role into a single server, and that server is where the metadata lives.

This paper is not a proposal that the industry replace these deployed systems. Signal's Private Group System [11], which uses anonymous credentials so that Signal servers cannot see group membership, is an existence proof that strong metadata privacy is compatible with a commercial messenger at scale. The question Onym asks is narrower: **can we build a group-membership registry where no operator — not even the anchor operator — is structurally positioned to observe the metadata, and where the privacy model is externally auditable from the public spec and the published bytecode?** Signal is the gold standard within a trust-the-operator frame; Onym explores the trust-nobody frame. Both have audiences. The engineering lessons in this paper are what it cost us to get the second frame to testnet.

## 2. A candidate solution, and why blockchain + SNARKs

There are four honest candidate approaches to a metadata-hiding group-membership registry. Each has a real weakness.

1. **Transparency logs** in the Certificate Transparency / Key Transparency [13] style give every observer the same append-only view and are mathematically elegant. They hide content well. They do not hide who is writing to which key, which means they do not hide co-membership in a group-registry setting. Key Transparency is complementary to our work, not a substitute.
2. **Federated delivery** (Matrix's federated homeservers, XMPP, email) distributes trust. It does not eliminate the per-server metadata observation. "Distribute trust" is weaker than "require no operator."
3. **Anonymous-credential-backed central servers** (Signal's Private Group System [11]) are deployed, scalable, and give excellent metadata privacy under an honest-but-curious server model. The trust assumption is specific: the Signal server must execute the oblivious protocol faithfully, and users must trust the server binary to match the spec. This is reasonable and works for many threat models. It is not externally auditable by reading the spec plus the bytecode at a public address.
4. **Public-ledger anchoring plus zero-knowledge membership proofs** commits to group state on a public chain and verifies membership with a SNARK. Every observer reads the same chain; no operator is positioned to observe more. The trust assumptions that remain are on the chain's censorship resistance, on the SNARK's soundness, and on the SNARK's trusted setup when one is required.

The fourth approach is the one Onym takes. The reasons to take it are narrow: we want an architecture in which a technical reader can verify the metadata claims from the public chain state, the deployed verifier bytecode, and the open-source client, without any privileged information. The reasons to *not* take it — in particular, the trusted-setup requirement, the block-producer timing side channel, and the fee-payer correlation — are discussed in Sec. 5 and Sec. 7, where each is the topic of a postmortem or a named limitation. This paper treats approach 4 as one candidate worth reporting on, not as the uniquely correct answer.

Within approach 4, we chose Stellar over Ethereum because Stellar Protocol 22 [14] introduced BLS12-381 host functions as a CAP-0046-12 feature in 2024, which reduced Groth16 verification to three pairings and a small multi-scalar multiplication at fee cost in the $10^{-5}$ XLM range. Ethereum has more ZK-tooling momentum (BN254 pre-compiles, several rollup-level verifiers, mature dev stacks), but BN254's effective security dropped to an estimated 100–103 bits after improved number-field-sieve attacks [15], below the 128-bit line we were targeting. BLS12-381 at 120 bits of estimated pairing security is closer to the target. The choice is defensible, and the cost — a younger smart-contract platform and a smaller ZK-dev ecosystem — is real.

## 3. Onym, in plain language

Picture a town hall that must keep a membership register for many private clubs, and picture the town's constitution prohibits the hall from knowing who is in any club. Instead of writing names, the hall posts one sealed envelope per club. The envelope is a single line of text, so short that it reveals nothing. A member of the club who walks up to the desk can prove they belong inside the envelope without anyone opening it — they hand over a small slip of paper, the desk staff perform a fixed arithmetic check against the slip and the envelope, and the staff return "yes" or "no." No staff member ever learns who the member is.

When the club's membership changes — someone is added, removed, or rotated — one current member computes what the club's *next* envelope should say, writes both the old envelope and the new envelope on the slip, proves "I belong inside yesterday's envelope, and today's envelope is the correct next version of it," and hands the slip over. The hall swaps envelopes. Nothing on the hall's wall ever named a member.

The real architecture follows that picture literally:

- The *hall* is the Stellar blockchain. Anyone in the world can read every envelope, which is just a 32-byte number per group.
- The *envelope* is a cryptographic commitment: an opaque hash-tree of the member set, bound to an epoch counter and a random salt.
- The *slip of paper* is a 192-byte Groth16 proof.
- The *arithmetic check* is three pairings on BLS12-381 — a bounded, fee-predictable on-chain operation.
- The *member's secret* is a BLS12-381 private key held on their phone. It never leaves the phone and never appears on the ledger.
- The *clerk behind the desk* is the public Soroban smart contract `CBKKEZU3CEAXZNJ4RSLDSVCXNJFQK4WMAOGF26SUBT2QUKHWQE2PRFSO`.

One subtlety matters even for the plain-language picture. If a member pays the Stellar transaction fee from their own Stellar account, that account's public key appears in the transaction envelope and re-links a real-world identity to group activity. To prevent this, Onym members submit their proof to a public relayer; the relayer wraps the proof in a Stellar transaction signed by its own account. The relayer cannot forge a proof or change which group is being updated, but it can withhold submissions. We treat it as a liveness, not a safety, dependency.

What an outside observer of the public chain therefore learns, in the honest version, is: a club exists (distinguished by its opaque identifier); its envelope has been swapped $k$ times at timestamps $t_1, \ldots, t_k$; the club is in a size tier with an upper bound of 32, 256, or 2,048 members, whichever circuit was used; the relayer paying the fee is a particular public Stellar account. The observer does not learn who is in the club, who initiated any swap, or whether the club grew or shrank.

## 4. Onym, formally

Let $\mathbb{F}_r$ be the BLS12-381 scalar field and $\mathbb{G}_1, \mathbb{G}_2, \mathbb{G}_T$ the pairing groups with generator $G_1 \in \mathbb{G}_1$. Let $H: \mathbb{F}_r^* \to \mathbb{F}_r$ denote Poseidon [9] instantiated at the standard BLS12-381 parameter set.

### 4.1 Commitment

For a member set $S = \{\mathsf{pk}_1, \ldots, \mathsf{pk}_n\} \subseteq \mathbb{G}_1$ with $n \leq 2^d$, let $\sigma(S)$ be the lexicographic ordering of $S$ under compressed-G1 byte encoding and let $\mathsf{MT}_H$ denote the Poseidon Merkle tree of depth $d$ with leaf $H(\mathsf{pk})$ and a zero padding leaf $H(0)$. The *commitment* is

$$
\mathsf{Commit}(S, \epsilon, s) \;=\; H\!\bigl( H(\mathsf{MerkleRoot}(\mathsf{MT}_H(\sigma(S))),\, \epsilon),\, s \bigr) \in \mathbb{F}_r,
$$

where $\epsilon \in \mathbb{F}_r$ is the epoch counter and $s \in \mathbb{F}_r$ is a per-epoch salt uniformly distributed in $\mathbb{F}_r$. The canonicalisation $\sigma$ ensures $\mathsf{Commit}$ is well-defined on unordered sets.

### 4.2 Relations

Two NP relations are compiled to R1CS and proved with Groth16 [2].

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

Critically, $C_{\mathsf{new}}$ is a *public input* of $R_{\mathsf{Upd}}$: it enters the verifier's IC (instance-commitment) linear combination and is thus bound to the proof by the Groth16 public-input non-malleability property [2, 16]. Section 5 postmortem P2 describes what went wrong when $C_{\mathsf{new}}$ was *not* a public input.

### 4.3 Security games

We state each property as a game between a PPT adversary $\mathcal{A}$ and a challenger $\mathcal{C}$. All games are over security parameter $\lambda = 128$ commensurate with BLS12-381.

*Soundness* (for each of $R_{\mathsf{Mem}}$ and $R_{\mathsf{Upd}}$). $\mathcal{A}$ outputs $(\pi, x)$; $\mathcal{A}$ wins if the verifier accepts and there is no witness $w$ such that $R(x; w)$ holds. Discharged by Groth16 computational knowledge-soundness [2] under the generic-group assumption over BLS12-381, conditional on the trusted setup (Sec. 5 P4).

*Zero-knowledge.* Standard Groth16 ZK [2]; the simulator holds the trapdoor and produces proofs indistinguishable from honest ones.

*Commitment hiding.* $\mathcal{A}$ outputs two member sets $S_0, S_1$ with $|S_0| = |S_1| \leq 2^d$; $\mathcal{C}$ samples $b \leftarrow \{0,1\}$ and $s \leftarrow \mathbb{F}_r$ uniformly, computes $C = \mathsf{Commit}(S_b, \epsilon, s)$, returns $C$; $\mathcal{A}$ outputs $b'$ and wins if $b = b'$. Discharged by the one-wayness of Poseidon with high-entropy salt and by $s$ never appearing on-chain.

*Operation-binding (for $R_{\mathsf{Upd}}$).* $\mathcal{A}$ observes an honestly generated proof $\pi$ over public tuple $(C, \epsilon, C')$ and produces $(\pi^*, C^{**})$ with $C^{**} \neq C'$ such that the verifier accepts $(\pi^*, C, \epsilon, C^{**})$. $\mathcal{A}$ wins if the contract then accepts $C^{**}$ as the next state. Discharged by Groth16 IC binding once $C_{\mathsf{new}}$ is a public input. This is the game that Onym *failed* from v1.0.0 through v1.2.x — see P2.

*Fee-payer unlinkability.* $\mathcal{A}$ observes the Stellar ledger and outputs a member identity $\mathsf{pk}$ and a transaction $T$; $\mathcal{A}$ wins if $\mathsf{pk}$ is the member who generated the proof in $T$ with probability non-negligibly greater than $1/n$ for group size $n$. Discharged by the relayer pattern and by constant-size proofs; an observer sees only the relayer's account.

A companion document contains the full reductions. Their one-line forms are the Table 1 mapping. The point of the formalism here is that the operation-binding game — the one that closes the authorization vulnerability of Sec. 5 — requires $C_{\mathsf{new}}$ to be a public input, and no amount of soundness over $R_{\mathsf{Mem}}$ implies it.

## 5. Four postmortems

We report four episodes in which the correctness of the system drifted off the rails. Each one was caught, fixed, and generalised into a design-review checklist item. We think they generalise because they share a single shape: a *local* component (a circuit, a relation, a key, a setup) was correct by the lights of its local specification and was wrong for the larger property the system actually needed. Local correctness is cheap to verify; system correctness is not.

### P1 — The constraint-cost crisis

**What went wrong.** The original commitment binding composed Poseidon inside the circuit with SHA-256 outside it:

$$
\mathsf{Commit}_A(S, \epsilon, s) = \text{SHA-256}(\mathsf{MerkleRoot}(\mathsf{MT}_H(\sigma(S))) \mathbin\| \epsilon \mathbin\| s).
$$

SHA-256 was chosen because Soroban has a native SHA-256 host function: a contract can recompute the commitment from witness values at negligible fee cost. The bridge between on-chain and off-chain was clean at the spec level. At the R1CS level it was a disaster. A single SHA-256 compression on a fixed 72-byte preimage costs roughly 25,000 R1CS constraints in the arkworks gadget [12] we used. The small-tier circuit (32 members) was dominated by SHA-256 by more than an order of magnitude over every other constraint combined. On a 2024-class smartphone with 6 GB of RAM, proving under multi-app memory pressure failed to complete reliably. The system was slow-but-correct, and the "slow" component had stopped being a performance issue and started being a functional one: it did not finish on the target hardware.

**The fix.** Replace the outer SHA-256 with a two-call Poseidon composition:

$$
\mathsf{Commit}_B(S, \epsilon, s) = H(H(\mathsf{MerkleRoot}, \epsilon), s).
$$

Two Poseidon evaluations cost roughly 600 R1CS constraints in total. Table 1 lists the measured constraint counts and reductions. The contract can no longer recompute the commitment with a host function — we treat it as an opaque value and rely on the proof.

**Table 1.** Measured R1CS constraint counts, by tier, for the two commitment variants.

| Tier | Members | Tree depth $d$ | $\mathsf{Commit}_A$ (SHA-256 outer) | $\mathsf{Commit}_B$ (Poseidon outer) | Reduction |
|------|---------|----------------|-------------------------------------|--------------------------------------|-----------|
| Small | 32 | 5 | ~26,740 | **1,910** | 14.0× |
| Medium | 256 | 8 | ~27,640 | **2,630** | 10.5× |
| Large | 2,048 | 11 | ~28,540 | **3,350** | 8.5× |

The reduction shrinks at higher tiers because SHA-256's contribution is a constant 25,000 while the Merkle path grows logarithmically. Practitioners rebuilding this system at the large tier should expect the 8.5× figure, not the headline 14×.

**The generalised lesson.** Any cryptographic primitive that sits on both sides of an R1CS boundary must have its in-circuit cost counted *first*. A primitive with a cheap host function but an expensive R1CS footprint will be a constraint-budget landmine. We had assumed the on-chain fee cost of SHA-256 would dominate the design; the R1CS cost dominated it by an order of magnitude in the other direction.

### P2 — The theorem that proved the wrong thing (a critical authorization vulnerability)

**Severity first.** This was a critical authorization vulnerability, not a design tradeoff. Any party in the network path between a prover and the contract could substitute the new commitment written to storage, retaining the prover's still-valid proof. Every privacy and authorization guarantee in Sec. 4.3 was void for the operation that changes group state. We did not call this a vulnerability in earlier drafts of this article. We were wrong not to.

**What went wrong.** Through release v1.2.9 all four state-changing contract operations shared one Groth16 circuit whose relation was $R_{\mathsf{Mem}}$ with public inputs $(C, \epsilon)$. The `update_commitment` entry point accepted, *in addition to* the proof and its public inputs, a separate `new_commitment: BytesN<32>` parameter; the contract wrote this parameter to storage as the next epoch's state. The proof authorised knowledge of a member in the *current* state. It said nothing about which next state was being authorised. `new_commitment` was a contract-controlled free parameter.

A code-review question exposed the gap:

> *"Where in the math is the argument that this proof is non-transferable to a different `new_commitment`?"*

The answer was: nowhere. The Groth16 verification equation incorporates the public inputs through the IC (instance-commitment) linear combination [16], and the proof is cryptographically bound exactly to those public inputs and to nothing else. Since `new_commitment` was not a public input, a relayer, a mempool observer, or any intermediate HTTP proxy could replace `new_commitment` with an arbitrary 32-byte value, keep the proof and the public inputs, and the contract would accept.

**How the theorem deceived.** We had a formal soundness argument. The argument was a correct proof of knowledge-soundness for $R_{\mathsf{Mem}}$. The argument was also *irrelevant to the update operation's security*, because the update operation's security predicate is over the variables $(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}, C_{\mathsf{new}})$, and $R_{\mathsf{Mem}}$ does not mention $C_{\mathsf{new}}$. The theorem proved exactly what it claimed. It was silent about what we actually needed. In retrospect the theorem was functioning as social proof: auditors saw a completed proof in the repository, registered that the cryptography had been "proved correct," and stopped looking at the binding question. A formal theorem is a scalpel, not a shield.

**Why four audit passes missed it.** The system had been through four audit passes at the time of discovery. We name them with scope, because the hard critique that preceded this article demanded that honesty.

1. *Internal review, scope: circuit soundness.* Read `src/circuit/mod.rs`. Verified the Merkle opening gadget, the Poseidon parameterization, and the constraint layout. Concluded: $R_{\mathsf{Mem}}$ is sound.
2. *External review, scope: contract ABI conformance.* Read `contracts/sep-xxxx/src/lib.rs` and the SEP document. Verified entry-point signatures, access control, and the canonical-bytes guard on public inputs. Did not re-derive what each proof binds.
3. *Internal review, scope: primitive choice.* Reviewed the Poseidon parameter set, the BLS12-381 subgroup checks, and the Ed25519 attestation format. Did not examine call-site semantics.
4. *External review, scope: wire-format correctness.* Verified the fixed-width encoding of proofs and public inputs across client platforms, test vectors, and the contract ABI. Did not examine the circuit-to-contract interface as a binding question.

No pass had "for each cryptographically authorized operation, state what the proof binds and what it does not bind" as a line item. Two of the passes were internal; two were paid external contractors. None had the relation file and the contract entry-point side-by-side in their written deliverables. We do not think the individual reviewers were at fault. The scoping was.

**The fix.** We split the relation into two: $R_{\mathsf{Mem}}$ (unchanged, used by the three read-only-ish operations `create_group`, `verify_membership`, `deactivate_group`) and $R_{\mathsf{Upd}}$ (new, used only by `update_commitment`). $R_{\mathsf{Upd}}$'s public inputs are $(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}, C_{\mathsf{new}})$; $C_{\mathsf{new}}$ now enters the IC linear combination and is bound to the proof. The circuit additionally enforces $\epsilon_{\mathsf{new}} = \epsilon_{\mathsf{old}} + 1$ in-circuit, so the contract cannot accept an adversarially chosen next epoch. Each of the three tiers now ships two verification keys instead of one. The wire format for `update_commitment` changed: a single 73-byte `UpdatePublicInputs` blob with a `0x02` version byte replaces the separate `new_commitment` parameter. The fix shipped as release v1.3.0 on 2026-04-18. No mainnet traffic was at risk because no mainnet traffic existed.

**The alternative we rejected.** One minimal-diff fix would have added `new_commitment` as a fourth public input to $R_{\mathsf{Mem}}$ with an in-circuit boolean gating that disabled its constraint for the three non-update call sites. We rejected it because a gating boolean is a footgun (the circuit must constrain it to $\{0,1\}$ correctly, and any reviewer has to trust that the gating logic does what it claims), and because an operation's authorization predicate should be readable directly off the relation file, not reconstructed by mentally threading an enable flag through three call sites.

**The generalised lesson.** Formal soundness of a relation $R$ implies nothing about the security of an operation whose security predicate is over variables $R$ does not mention. Our design review template now requires, for every cryptographically authorised operation: (1) a one-sentence predicate stating what the proof binds; (2) an enumeration of every value the contract reads on that path, labelled "in IC" or "contract-controlled"; (3) a side-by-side read of the relation file and the contract entry-point in the same review session. "We have replay protection" and "we have canonical-bytes enforcement" are not answers to the binding question.

### P3 — Transport-layer co-membership leakage

Onym's clients exchange invitations, rekey envelopes, and group ciphertext over Nostr relays [3]. Every Nostr event is signed by a secp256k1 key. Through release v1.2.7 each client signed every event with its *long-lived* identity key. The companion transport specification mentioned the implication in a single bullet of its Security Considerations:

> *"Stable Nostr device keys improve usability but increase sender linkability."*

That bullet is technically true and strategically misleading. "Sender linkability" sounds like a profile-building concern affecting one user at a time. The actual consequence, given that Onym's group ciphertext also carries a stable hidden topic tag, was *group linkability*: any relay (or anyone with access to a relay's event log) could cluster the user base by co-membership from public relay data alone — "these $N$ keys all published to hidden-topic $T$ → these $N$ devices are in the same group." The SEP's own privacy properties claimed precisely the opposite. The two normative documents contradicted each other for the entire lifetime of the transport layer.

This is the cross-document inconsistency RFC 6973 §6.3 [14] was written to catch: a referencing specification's privacy claims must be reconciled with every referenced specification. We had not done the reconciliation. The fix was small — ephemeral per-event secp256k1 keys, a five-line key-factory change — but it required auditing every receive-side use of `event.pubkey` as a sender identifier (four places: display logic, rate-limiting, removal notices, a database column). Those uses had to switch to the inner BLS public key, which had always been the actual authentication anchor. The outer Nostr key was never security-critical; it had been used as a stable identifier because it happened to be stable, not because anything required it.

**The generalised lesson.** A privacy invariant that lives in a *Security Considerations* bullet of a different document is fiction. We now require every privacy property in any normative document Onym references to be either cryptographically enforced (with a citation) or explicitly restated in the Known Privacy Limitations section of every consumer's documentation. Identity coupling — a single key serving as both signing key and sender identity — is a recurring antipattern; decoupling the two roles is almost free on day one and a postmortem otherwise.

### P4 — The single-machine trusted setup

The preceding three postmortems are about bugs that were fixed. This one is a bug that is *not yet fixed* and that caps the system's current security ceiling. We mention it here because hiding it further in the paper, as a limitation in a later section, would reproduce the Sec. 6 failure mode — putting a security invariant in a bullet instead of in the narrative.

Groth16 requires a per-circuit trusted setup [2]. For each of our six verification keys (two relations × three tiers) the setup must be the output of a multi-party computation (MPC) in which at least one honest participant discarded their contribution; otherwise the party who holds the aggregate trapdoor can forge accepting proofs for arbitrary public inputs without knowing any witness. Our reference implementation runs the full Powers-of-Tau Phase-1 protocol with multi-party contributions and pairing-based transcript verification. But the Phase-2 circuit-specific ceremony — which converts Phase-1 into per-circuit verification keys — currently runs on a single machine operated by one party. That party holds the aggregate Phase-2 secret. Anyone with access to that secret can forge an accepting proof for any `update_commitment` call.

Until the Phase-2 ceremony is run as a multi-party computation, every security game in Sec. 4.3 is conditional on trusting the setup operator. The system is not, by its own formal statements, zero-trust. It is one-party trust with better engineering than a lookup table. The mainnet rollout criterion — which at the time of writing is outstanding — is a Phase-2 MPC with at least five independent contributors, each of whom publicly destroys their contribution secret, and a scoped external audit of the $R_{\mathsf{Upd}}$ relation. Neither has been done. We would rather this paper be the one that says so clearly than the one that rediscovers it in a later postmortem.

**The generalised lesson.** A cryptographic prerequisite that is not yet satisfied does not become satisfied by being documented in a limitations section. If the prerequisite caps every security game the system claims, it belongs in the system's abstract and in every mention of its security model, with the correct honest tense — we are building toward it, we have not yet delivered it.

## 6. Evaluation

Onym's reference implementation is deployed on Stellar testnet at contract ID `CBKKEZU3CEAXZNJ4RSLDSVCXNJFQK4WMAOGF26SUBT2QUKHWQE2PRFSO`. A testnet deployment is not a production deployment: no real users, no economic stakes, no adversarial environment, no sustained uptime requirement. We report prover-time and on-chain cost numbers for the engineering lessons they support, not as a claim of operational readiness.

**Prover time.** All numbers are median of 50 runs with 10 warmup runs discarded, on a Pixel 8 Pro (Android 14, Tensor G3, 12 GB RAM) and an iPhone 15 Pro (iOS 17, A17 Pro, 8 GB RAM). Witness generation, multi-scalar multiplication, and proof serialisation are all included. Salt material and Merkle openings are precomputed off the critical path — the production client maintains them incrementally as Commits arrive.

**Table 2.** End-to-end proof generation latency, median (95th percentile), in milliseconds.

| Tier | Members | Depth | Pixel 8 Pro $R_{\mathsf{Mem}}$ | Pixel 8 Pro $R_{\mathsf{Upd}}$ | iPhone 15 Pro $R_{\mathsf{Mem}}$ | iPhone 15 Pro $R_{\mathsf{Upd}}$ |
|------|---------|-------|--------------------------------|--------------------------------|----------------------------------|---------------------------------|
| Small | 32 | 5 | 180 (240) | 235 (310) | 165 (220) | 215 (290) |
| Medium | 256 | 8 | 320 (410) | 415 (530) | 290 (380) | 380 (490) |
| Large | 2,048 | 11 | 760 (920) | 985 (1,200) | 690 (840) | 895 (1,090) |

The small and medium tiers are sub-second on both platforms. The large tier is sub-1.2 s. The $\mathsf{Commit}_A$ variant is not reported because it did not complete reliably on the iPhone 15 Pro under multi-app memory pressure; we do not treat unreliable measurements as honest data.

**On-chain verification cost.** Groth16 proofs are 192 bytes (three compressed BLS12-381 group elements). Public inputs add 32 bytes for $R_{\mathsf{Mem}}$ and 96 bytes for $R_{\mathsf{Upd}}$. Verification invokes three pairings via Stellar's BLS12-381 host functions plus a small multi-scalar multiplication over the IC; total Soroban instruction count is 6–10 million, well inside testnet fee budgets.

**Privacy evaluation.** We report what we can and mark what we cannot. A public-ledger observer learns: that a group with a specific opaque identifier exists; the number and timestamps of its state changes; an upper bound on group size from the tier; the relayer's Stellar account. An observer does not learn, conditional on intact trusted setup (P4) and Groth16 soundness: the member set; the updater's identity; whether the group grew or shrank. We have *not* run a formal differential-privacy analysis of the timing channel, nor quantified the information an observer recovers from tier-transition patterns across a group's lifetime. Both are open.

## 7. Honest limitations

In addition to the open P4:

**Testnet, not mainnet.** The system has processed no real transaction. Any claim that relies on "deployed and working" is claiming engineering-readiness, not operational-readiness. Mainnet is gated on P4's MPC and on the external $R_{\mathsf{Upd}}$ audit.

**No committed production customer.** We have had exploratory conversations with a Matrix-adjacent group and with one governance-tooling project about using Onym as a metadata-hiding state anchor. Neither has a signed letter of intent. The article's contribution is engineering lessons, not market validation, and we do not claim the latter.

**Group-size leakage via tier.** The choice of tier is public and leaks an upper bound on group size. Applications in which the existence of a 2,000-member group is itself sensitive should pad to a higher tier. This is a cost of the constant-verification-cost design.

**Fee-payer correlation via the relayer.** The relayer's own account is a metadata observation point. A single, well-behaved relayer is better than each member being their own fee payer, but it is worse than a fully decentralised relayer pool or a native Soroban fee-sponsorship primitive. We consider this a liveness-of-privacy issue, mitigable with relayer rotation, and a reason to revisit the design when Soroban's fee-sponsorship proposals mature.

**MLS integration is partial.** Onym maps an MLS Commit to an `update_commitment` call and distributes the fresh salt over MLS's encrypted GroupContext extension. We have not yet integrated Onym with a stock MLS implementation's Delivery Service interface; our reference clients use a bespoke MLS-lite implementation. A stock integration is in progress and is the subject of a separate engineering effort.

## 8. Related work and alternatives

Table 3 locates Onym within the space of approaches to the metadata-hiding group-membership problem. The point of the table is *not* to argue Onym is the best approach. It is to document what each approach trades.

**Table 3.** Approaches to the metadata-hiding group-membership problem.

| Approach | Example(s) | Trust | Metadata-observable by | Deployment maturity |
|----------|-----------|-------|------------------------|---------------------|
| Anonymous credentials on a central server | Signal PGS [11] | Signal server (honest-but-curious, binary-faithful) | Signal, under compromise | Production, hundreds of millions of users |
| Federated delivery | Matrix, XMPP | Every federated homeserver | Federation peers | Production, millions |
| Transparency logs | CT [10], Key Transparency [13] | Log operator | Log operator | Production (CT), early (KT) |
| Mixnets | Nym, Loopix | Mixnet operators (non-colluding threshold) | Network-layer adversary only | Early production |
| Public-ledger anchoring + SNARK (this work) | Onym | Chain + SNARK soundness + trusted setup | No operator in the honest-setup case | Testnet |
| Per-application ZK primitive | Semaphore [4], Tornado Cash [5], Zcash [6] | As above | No operator | Production (Zcash), sanctioned (Tornado), production (Semaphore) |

Onym is most directly comparable to Semaphore and Tornado Cash in construction — all three anchor a Merkle tree of commitments on a public ledger and verify membership with a Groth16 SNARK — and distinguishes itself by being a general group-registry primitive coupled to MLS Commit semantics rather than a single-application identity tree. Aztec [7] uses PLONK on Ethereum for confidential transactions and is the closest non-Groth16 deployed system; we did not adopt PLONK because Stellar Protocol 22 provides BLS12-381 pairing host functions that directly accelerate Groth16 verification but does not yet provide the polynomial-commitment host functions PLONK would require. Signal's Sealed Sender [8] addresses a related decoupling problem (sender unlinkability) at the messaging layer rather than the registry layer.

## 9. Conclusion

Metadata observability is the unsolved half of online-communication privacy. The best-deployed solution today, Signal's Private Group System, closes it under a trust-the-operator model and has earned that trust through a decade of public engineering. Onym explores the trust-nobody frame — a public anchor, SNARK-gated state changes, no operator positioned to observe group membership. The contribution of this article is not that the frame is the right one for every threat model; it is not. It is the engineering lessons from building toward it on testnet: count R1CS cost before host-function cost when a primitive straddles the circuit boundary; a relation's soundness theorem says nothing about operations whose security predicate is over variables the relation does not mention; privacy invariants that live in Security Considerations bullets of other documents are fiction; and a trusted-setup prerequisite does not become discharged by being mentioned in a later section.

We would prefer that readers take the postmortems more seriously than the system. The system is one more point in a design space. The failure modes — a theorem that proved the wrong thing, a constraint that made the correct design unusable, a stable key that broke a privacy claim the rest of the system depended on, and a single-machine shortcut that caps the security ceiling — generalise to any system in this neighbourhood.

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
