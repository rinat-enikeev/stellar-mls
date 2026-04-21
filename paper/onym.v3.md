---
title: "Metadata Without Observers: Postmortems from a SNARK-Gated Group Registry on Stellar"
author: Rinat Enikeev
venue: IEEE Security & Privacy magazine (feature article)
status: v3 — addresses 6 post-v2 review concerns (SE game correctness, MPC honesty sentence, pair-chat caveat earlier, §2 validator-set rationale, weakened AI disclosure, R1CS lesson moved out of §5.1)
date: 2026-04-21
---

# Metadata Without Observers: Postmortems from a SNARK-Gated Group Registry on Stellar

## Abstract

End-to-end encryption closed the content half of the online-communication privacy problem. The metadata half — *who is in a group with whom* — still leaks to at least one operator in almost every deployed messaging system. Onym is a testnet system that anchors each group's state on the Stellar blockchain as a 32-byte commitment and gates every state change on a Groth16 zero-knowledge SNARK over BLS12-381. This article is scoped to the *unanimous-member* mode, in which any current member may unilaterally authorize any next state with a single membership proof. No member address or updater identity appears on the ledger; the group's size tier (≤32, ≤256, or ≤2,048 members) is public, and tier transitions monotonically disclose that a group crossed those thresholds. We state the construction formally in the algebraic group model with a random-oracle heuristic for Poseidon, reducing *operation-binding* to simulation-extractability rather than plain knowledge-soundness. We report two authorization/privacy postmortems and a demoted lesson on R1CS cost. Two caveats belong up front rather than buried in the security model: (i) Onym's deployed verification keys are the output of a single-contributor Phase-2 ceremony, and until this is replaced by a multi-contributor MPC every formal guarantee is vacuous against the setup operator; (ii) fee-payer unlinkability is trivially vacuous for $n=2$ pair chats once realistic auxiliary observations are admitted — Onym is a group-messaging substrate, not a 1:1 substrate. Onym is deployed to Stellar testnet; phase is alpha.

---

## 1. The metadata problem

Content-level encryption is now a commodity — Signal's Double Ratchet, iMessage, WhatsApp, Google Messages' Messaging-Layer-Security (MLS) rollout, and Matrix's Olm/Megolm all provide it; none hide metadata from the operator. iCloud backups hold message metadata for every Apple user; WhatsApp provides subscriber records under stored-communications-act orders; Matrix homeservers see every room's membership list by construction; Telegram "secret chats" leave groups, channels, and cloud chats server-readable. Signal is the outlier: grand-jury subpoenas have forced it to disclose only registration time and last-seen date.

Metadata is not a secondary concern. The 2020 EncroChat takedown and the 2021 Sky ECC operation both recovered content from compromised devices, but the *targeting* was metadata-driven, and former NSA director Michael Hayden's 2014 remark — "we kill people based on metadata" — is a statement about operational reality.

Structurally, every messaging architecture we know of has at least one operator positioned to observe the metadata: the server operator in centralized systems, every federated server in federated ones, and the Delivery Service (DS) in MLS deployments [1]. Recent work on private MLS Delivery Services [13] is the closest prior work on that half of the problem.

This paper is not a proposal to replace these systems. Signal's Private Group System [7] shows that strong metadata privacy is compatible with a commercial messenger at scale. The question Onym asks is narrower: **can we build a group-membership registry in which no application-layer operator observes membership, and in which the authorization rules are externally auditable from the public contract bytecode alone?** Signal is the gold standard in the trust-the-operator frame; Onym explores an adjacent design point. We do not advertise it as "trust nobody" — a public-ledger anchor reduces the trusted party to the chain and its validator set, which is materially weaker than an application-server position but not cryptographically absent.

## 2. Why blockchain + SNARKs

Of four candidate approaches — transparency logs, federated delivery, anonymous-credential central servers [7], and public-ledger + ZK — Onym takes the fourth. Transparency logs hide content but not *who writes to which key*; federated delivery distributes trust without eliminating per-server observation; anonymous-credential servers match the metadata-privacy bar under an honest-but-curious server but require auditors to trust that the running binary matches the published protocol. Public-ledger anchoring verifies every state transition from public chain state and deployed bytecode, with no server binary in the trust base. The trust that remains is on the chain's censorship resistance, SNARK soundness, the SNARK's trusted setup, and — at finer granularity — the validator set not colluding with the relayer on submission-time / inclusion-time correlation.

We chose Stellar over Ethereum because Stellar Protocol 22 [10] introduced BLS12-381 host functions, reducing Groth16 verification to three pairings and a small multi-scalar multiplication at negligible fee cost. BN254's effective security dropped to an estimated 100–103 bits after improved number-field-sieve attacks [8]; BLS12-381 is at roughly 120 bits of pairing security, which is our threshold. Stellar's tier-1 validator set is public and numbers on the order of tens of organizations; we treat it as a **semi-trusted party** distinct from ordinary observers.

The validator-set residual trust is qualitatively weaker than the trust placed in an application server in the Signal-style anonymous-credential frame, and the distinction matters for how a reader should interpret the trust-base column in §9 Table 1. A validator does not decide *which* transactions exist — the set of valid transactions is determined by the contract bytecode and every validator verifies against the same state — it only decides *ordering and inclusion timing* within one ledger-closing interval (a seconds-scale window). Its observation window is bounded to that interval rather than the indefinite server-log retention an application server enjoys, and its ability to deny or slow an individual transaction is bounded by Stellar Consensus Protocol's safety requirements against the rest of the tier-1 set. Application servers in the anonymous-credential frame, by contrast, are positioned to observe every query to the credential system *and* to silently refuse service to specific users without any cross-check from a second independent party. We therefore treat validator-set observation as a *residual* channel that we do not claim to close, rather than as the primary channel the design is aimed at; §8 returns to the operational implications.

## 3. Scope of this article

The deployed Onym contract supports several authorization policies for state changes. This article is scoped to the unanimous-member mode: a group where any current member, alone, may authorize any next state. Other policies are out of scope; readers should not generalize the privacy properties reported here to them. All postmortems in §6 concern v1.0.0–v1.6.7, which supported only the unanimous-member mode; other policies shipped in v1.7.0 on 2026-04-19.

**Onym is a group-messaging substrate, not a 1:1-messaging substrate.** The fee-payer unlinkability guarantee in §5.3 is of the form "$1/n$-anonymity within a group of size $n$," and once realistic auxiliary observations (relayer-ingress timing, transport-layer patterns, validator submission windows) are admitted, Onym drives no mechanism below $1/2$ for $n=2$. Readers imagining Onym as a substrate for pair chats should stop — the design does not serve that use case well, and the abstract's second caveat is a scope statement, not a security-game footnote.

## 4. In plain language

Picture a town hall that must keep a membership register for many private clubs, and picture the town's constitution prohibiting the hall from knowing who is in any club. Instead of writing names, the hall posts one sealed envelope per club. The envelope is a single short line of text that reveals nothing about members. A member who walks up to the desk can prove they belong inside the envelope without anyone opening it — they hand over a small slip of paper, the desk staff perform a fixed arithmetic check, and the staff return "yes" or "no." No staff member ever learns who the member is.

When the club's membership changes, one current member computes what the club's *next* envelope should say, writes both old and new envelopes on the slip, proves "I belong inside yesterday's envelope, and today's envelope is the correct next version of it," and hands the slip over. The hall swaps envelopes. Nothing on the hall's wall ever named a member. Any current member can perform that swap alone, and the hall has no way to tell which member did so.

The real architecture follows that picture literally:

- The *hall* is the Stellar blockchain. Anyone can read every envelope, which is just a 32-byte number per group.
- The *envelope* is a cryptographic commitment: an opaque hash-tree of the member set, bound to an epoch counter and a random salt.
- The *slip of paper* is a 192-byte Groth16 proof.
- The *arithmetic check* is three pairings on BLS12-381.
- The *member's secret* is a BLS12-381 private key held on their device.
- The *clerk behind the desk* is a public Soroban smart contract.

Two subtleties matter. First, if a member pays the Stellar transaction fee from their own account, that account's public key appears in the transaction envelope and re-links a real-world identity to group activity. Onym members therefore submit their proof to a public relayer; the relayer wraps the proof in a Stellar transaction signed by its own account. The relayer cannot forge a proof or change which group is updated, but it can withhold submissions — we treat it as a liveness, not a safety, dependency. Second, each group is created inside one of three fixed *size tiers* — ≤32, ≤256, or ≤2,048 members — and the tier is public. If a group outgrows its tier it must be re-created at a larger one, and that re-creation is observable on-chain. The abstraction does *not* hide the fact that a group grew past 32 or past 256 members; it hides only the exact count within a tier. Tier transitions are a *monotonic* disclosure: once a group crosses a threshold, the crossing is public forever.

What an outside observer of the public chain learns is: a group exists; its envelope has been swapped *k* times at timestamps *t₁, …, tₖ*; the group's current size tier and any prior tier it graduated out of; and the relayer paying the fee. The observer does not learn who is in the group, who initiated any swap, the exact count within a tier, or whether the group shrank or rotated within its tier. A Stellar validator additionally learns the relayer's submission time before block inclusion; §8 discusses why that extra observation window is qualitatively weaker than an application-server observation point.

## 5. Formal construction

Let $\mathbb{F}_r$ be the BLS12-381 scalar field and $\mathbb{G}_1, \mathbb{G}_2, \mathbb{G}_T$ the pairing groups with generator $G_1 \in \mathbb{G}_1$. Let $H: \mathbb{F}_r^* \to \mathbb{F}_r$ denote Poseidon [5] at the standard BLS12-381 parameter set. Throughout the security analysis, out-of-circuit evaluations of $H$ are modelled as a random oracle; in-circuit evaluations are modelled concretely, inside Groth16's knowledge-soundness argument.

### 5.1 Commitment

For a member set $S = \{\mathsf{pk}_1, \ldots, \mathsf{pk}_n\} \subseteq \mathbb{G}_1$ with $n \leq 2^d$, let $\sigma(S)$ be the lexicographic ordering under compressed-G1 byte encoding and $\mathsf{MT}_H$ the Poseidon Merkle tree of depth $d$ with leaf $H(\mathsf{pk})$ and zero padding $H(0)$. The commitment is

$$
\mathsf{Commit}(S, \epsilon, s) \;=\; H\!\bigl( H(\mathsf{MerkleRoot}(\mathsf{MT}_H(\sigma(S))),\, \epsilon),\, s \bigr) \in \mathbb{F}_r,
$$

where $\epsilon \in \mathbb{F}_r$ is the epoch counter and $s \in \mathbb{F}_r$ is a per-epoch salt uniform in $\mathbb{F}_r$. The canonicalization $\sigma$ ensures $\mathsf{Commit}$ is well-defined on unordered sets. The choice to compose Poseidon twice rather than layer Poseidon inside the circuit with a different host-function hash outside it was itself a constraint-budget lesson; we record it as §6.3 after the postmortems rather than strand a multi-paragraph aside inside the formal construction.

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

$R_{\mathsf{Mem}}$ is invoked by `create_group`, `verify_membership`, and `deactivate_group`; $R_{\mathsf{Upd}}$ is invoked by `update_commitment`. Crucially, $C_{\mathsf{new}}$ is a *public input* and therefore enters the verifier's instance-commitment (IC) linear combination — part of the statement the proof binds. Postmortem P1 (§6) describes what went wrong when it was not.

**Authorization semantics.** The relation binds $C_{\mathsf{new}}$ to some new root and some new salt, but does not constrain the relationship between $\mathsf{root}_{\mathsf{old}}$ and $\mathsf{root}_{\mathsf{new}}$. A single current member can therefore replace the entire member set in one operation; that is by design. Constraining the diff would roughly double the R1CS constraint count and expose an on-chain ±1 member-count signal the design is intended to avoid.

**Replay and freshness.** Replay is defended at the *contract* layer, not inside the relation. The `update_commitment` entry point verifies that the public input $C_{\mathsf{old}}$ equals the group's currently stored commitment *and* that $\epsilon_{\mathsf{old}}$ equals the stored epoch; on success it atomically writes $C_{\mathsf{new}}$ and increments the stored epoch. Once a group advances past epoch $\epsilon$, every proof with public input mentioning $(C_\epsilon, \epsilon)$ fails the entry-point precondition — even if captured off the wire and held by a former member whose key was never rotated. A captured proof still carries authorization power *at most* until the next legitimate update wins the race on-chain; under Soroban's sequential execution, first-write wins, and any replay at that point is equivalent to authorizing the same next state the legitimate prover was already submitting.

### 5.3 Security model and games

Games run between a probabilistic polynomial-time adversary $\mathcal{A}$ and challenger $\mathcal{C}$, with $\lambda = 128$. The analysis rests on three assumptions: **(A1)** Groth16 is knowledge-sound in the **algebraic group model** (AGM) [12] — strictly weaker than the generic group model, and what recent Groth16 analyses target; **(A2)** with the public-input-hashing modification of [9] (building on Lipmaa [11]), Groth16 is **simulation-extractable** (SE) in AGM+ROM — plain Groth16 is malleable [6], so SE, not plain knowledge-soundness, is the property needed for operation-binding; **(A3)** out-of-circuit Poseidon is modelled as a random oracle — a heuristic on the sponge construction. (A1) and (A2) presume an honest Groth16 per-circuit setup, which is a standing prerequisite for every Groth16 deployment and not Onym-specific.

**Deployment-state honesty.** Onym's deployed verification keys are the output of a single-contributor Phase-2 ceremony; until this is replaced by a multi-contributor MPC, every game in this section is vacuous against the setup operator, who holds the aggregate simulation trapdoor and can produce accepting proofs for arbitrary public inputs. This is stated in the abstract and repeated here because it is load-bearing: the games below describe what *will* hold once Phase-2 is rerun with ≥ 3 independent contributors and at least one discards their contribution honestly, and they describe nothing else today.

*Soundness* (each of $R_{\mathsf{Mem}}, R_{\mathsf{Upd}}$). $\mathcal{A}$ outputs $(\pi, x)$; wins if Verify accepts and no witness satisfies $R$. Under (A1), the Groth16 AGM extractor produces such a witness from any accepting $\mathcal{A}$.

*Zero-knowledge.* Standard Groth16 simulation-ZK in the AGM [2, 12].

*Commitment hiding.* $\mathcal{A}$ outputs $S_0, S_1$ with $|S_i| \leq 2^d$; $\mathcal{C}$ samples $b, s$ and returns $C = \mathsf{Commit}(S_b, \epsilon, s)$. Under (A3), the outer Poseidon call is a random oracle and $s$ has $\sim 255$ bits of entropy; a computationally bounded $\mathcal{A}$ queries the oracle at $(y_b, s)$ only with negligible probability, so $C$ is uniform and independent of $b$. This does *not* treat Poseidon as a PRF; the argument is indistinguishability from uniform under a uniform hidden salt in ROM.

*Operation-binding.* $\mathcal{A}$ is given access to a **simulator oracle** $\mathsf{Sim}$ that, on any adversarially-chosen instance $x_i = (C_i, \epsilon_i, C'_i) \in \mathbb{F}_r^3$, returns an accepting proof $\pi_i$ for $R_{\mathsf{Upd}}$ *without* taking a witness — $\mathsf{Sim}$ can do so because it holds the simulation trapdoor produced by the honest Phase-2 setup. After polynomially many such queries, $\mathcal{A}$ outputs $(\pi^*, x^*)$ with $x^* = (C, \epsilon, C^{**})$ such that Verify accepts and $x^* \neq x_i$ for every queried $i$ (note: $x^* \neq x_i$ is satisfied whenever $C^{**}$ differs from the $C'_i$ of the matching $(C_i, \epsilon_i)$ query — the operational case we care about in P1). $\mathcal{A}$ wins if Verify accepts. Plain knowledge-soundness does **not** discharge this game — the adversary's view is polluted by simulated proofs that were produced without witnesses, and Groth16 is malleable [6] so $\mathcal{A}$ might mangle a $\pi_i$ into an accepting $\pi^*$ without ever knowing a witness. Under (A2), the SE extractor produces a witness $w^*$ satisfying $R_{\mathsf{Upd}}(x^*; w^*)$ despite the simulated-proof history; since $w^*$ contains $(\mathsf{root}_{\mathsf{new}}, s_{\mathsf{new}})$ opening $C^{**}$, $\mathcal{A}$ knows a preimage of the new commitment it authorized. The deployed contract incorporates [9]'s public-input-hashing modification; without it, SE of Groth16 is not established. Onym *failed* this game from v1.0.0 through v1.2.x — see P1.

*Fee-payer unlinkability.* **Ideal (no aux):** $\mathcal{C}$ samples a group of size $n \geq 2$ and a uniform prover index $i$; prover submits via the relayer. $\mathcal{A}$ sees only the on-chain transaction; advantage is $\Pr[\hat{i} = i] - 1/n$. The on-chain bytes are a constant-size proof plus public inputs independent of $i$, in a transaction paid by the relayer; ZK closes the argument. **Realistic (with aux):** let $\mathsf{aux}$ denote side-channel observations on member $i$ — relayer-ingress timing, P2-class Nostr patterns, validator-set submission timing. $\mathcal{A}$'s advantage is $\Pr[\hat{i} = i \mid \mathsf{aux}] - 1/n$; the game does *not* bound it. For $n = 2$ the ideal bound is vacuous and no Onym mechanism drives the realistic bound below $1/2$. We claim no more than "on-chain metadata, holding $\mathsf{aux}$ fixed, contributes no additional signal."

## 6. Postmortems

Two security postmortems follow, and one demoted performance lesson (§6.3) that earlier drafts listed as a third postmortem. The demotion reflects that it was a constraint-budget mistake, not a security bug, and listing it alongside authorization failures overstated its severity; §6.3 keeps the lesson because the design-review takeaway is worth recording, but separates it from the authorization failures.

### 6.1 P1 — The theorem that proved the wrong thing

**Severity first.** A critical authorization vulnerability: any party in the network path between a prover and the contract could substitute the new commitment written to storage, retaining the prover's still-valid proof. Every privacy and authorization guarantee was void for the operation that changes group state.

**What went wrong.** Through v1.2.9 every state-changing operation shared one circuit with relation $R_{\mathsf{Mem}}$ and public inputs $(C, \epsilon)$. The `update_commitment` entry point accepted, in addition to the proof and its public inputs, a separate `new_commitment: BytesN<32>` parameter that the contract wrote to storage. The proof authorized knowledge of a member in the *current* state; it said nothing about which next state was being authorized. `new_commitment` was a contract-controlled free parameter.

**Who caught it.** An external reviewer, contracted for a Groth16-circuit-to-contract-ABI seam pass, asked: *"Where in the math is the argument that this proof is non-transferable to a different `new_commitment`?"* The answer was: nowhere. Groth16 binds a proof to its public inputs via the IC linear combination [11] and to nothing else. A second subtlety surfaced during the fix: even once $C_{\mathsf{new}}$ is a public input, plain Groth16 is malleable [6], so the required property is simulation-extractability [9, 11]. Making $C_{\mathsf{new}}$ an IC input was necessary; it was not sufficient until the public-input-hashing modification of [9] was added.

**Why the preceding four reviews did not catch it.** The system had passed four structured reviews — circuit soundness, contract ABI, primitive choice, wire-format correctness — each well-scoped within itself. The seam that failed was *between* scopes: no pass owned the question of whether the operation's security predicate was expressible in terms of the statement the proof actually bound. The lesson is about review topology, not any individual reviewer.

**The fix and the lesson.** Split the relation: $R_{\mathsf{Mem}}$ unchanged, $R_{\mathsf{Upd}}$ new with $(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}, C_{\mathsf{new}})$ public; public inputs hashed into the Fiat-Shamir transcript per [9] to obtain SE. Formal soundness of a relation $R$ implies nothing about the security of an operation whose security predicate is over variables $R$ does not mention. Our design-review template now requires, per authorized operation: (1) a one-sentence predicate stating what the proof binds; (2) an enumeration of every value the contract reads on that path, labeled *in IC* or *contract-controlled*; (3) a side-by-side read of the relation file and the contract entry-point; (4) an explicit statement of whether knowledge-soundness or simulation-extractability is the intended security level.

### 6.2 P2 — Transport-layer co-membership leakage

Onym's clients exchange invitations, rekey envelopes, and group ciphertext over Nostr relays. Every Nostr event is signed by a secp256k1 key. Through v1.2.7 each client signed every event with its *long-lived* identity key. The companion transport specification mentioned the implication in a single bullet of its Security Considerations:

> *"Stable Nostr device keys improve usability but increase sender linkability."*

That bullet is technically true and strategically misleading. The actual consequence was *group linkability*: any relay could cluster the user base by co-membership from public relay data — "these $N$ keys all published to hidden-topic $T$ → these $N$ devices are in the same group."

**The fix.** Ephemeral per-event secp256k1 keys, with a receive-side audit switching four places from `event.pubkey` to the inner BLS public key. Topic tags are now derived per-epoch as $H(\mathsf{group\_id} \mathbin\| \epsilon)$ and rotate on every state change. We do not claim that no traffic-analysis-based co-membership inference remains — relay-level timing correlation is an open problem and also feeds the realistic fee-payer unlinkability bound (§5.3) — but the two concrete mechanisms that made co-membership trivially observable from public data are both eliminated.

**Lesson.** A privacy invariant that lives in a *Security Considerations* bullet of a different document is fiction. Identity coupling — a single key serving as both signing key and sender identity — is a recurring antipattern; decoupling the two roles is almost free on day one and a postmortem otherwise.

### 6.3 Demoted: host-function hashes as a constraint-budget landmine

*(Demoted from P1 in earlier drafts. Not a security postmortem; kept for its design-review value.)*

An earlier version of Onym composed Poseidon inside the circuit with SHA-256 outside it. SHA-256 was chosen because Soroban has a native host function for it, so the on-chain cost of computing the outer commitment at verification time was trivially cheap; the trouble is that the circuit *also* has to recompute the outer hash in order to bind its inner Poseidon output to the same commitment the contract observes, and one SHA-256 compression costs roughly 25,000 R1CS constraints in the standard arkworks gadget. On the small tier (32 members) the circuit was dominated by the outer hash by more than an order of magnitude; the relation's member-set logic was a rounding error next to it. Replacing the outer SHA-256 with two Poseidon calls ($\sim 600$ R1CS constraints total) reduces R1CS cost by $8$–$14\times$ across tiers and is the choice recorded in the §5.1 construction.

**Lesson.** When a primitive straddles the circuit boundary — computed once by the contract on-chain, re-computed inside the circuit as a constraint on the witness — *R1CS cost dominates host-function cost* for any primitive the chain will verify many times. The review template we deploy now requires, per cryptographic primitive: (1) whether it crosses the circuit boundary; (2) its R1CS cost under the intended gadget; (3) its host-function cost; (4) a one-sentence justification of which side the trade-off should favor. The v1.0 Onym circuit would have passed a review that checked (3) but not (2).

This lesson is on its way to obsolescence: Stellar Protocol 25 (*X-Ray*, CAP-0075, Jan 2026) adds native Poseidon/Poseidon2 host functions, which collapses the SHA-256-vs-Poseidon host-side trade-off that produced this mistake in the first place. The review-template lesson survives the obsolescence; the specific choice between SHA-256 and Poseidon on the host side does not.

## 7. Evaluation

Reference implementation is deployed on Stellar testnet; phase is alpha.

**On-chain verification cost.** Groth16 proofs are 192 bytes; public inputs add 32 bytes for $R_{\mathsf{Mem}}$ and 96 bytes for $R_{\mathsf{Upd}}$. Verification invokes three pairings via Stellar's BLS12-381 host functions plus a small multi-scalar multiplication; total Soroban instruction count is 6–10 million, inside testnet fee budgets.

**What a public-chain observer learns**, conditional on the §5.3 assumptions: that a group exists; the number and timestamps of its state changes; its current size tier and any tier it graduated out of; and the relayer paying the fee. The observer does *not* learn the member set, the updater's identity, the exact count within a tier, or whether the group shrank or rotated within tier. A Stellar validator additionally learns the relayer's submission time on a seconds-scale window around block inclusion. We have not run a formal differential-privacy analysis of the timing channel nor quantified information recoverable from tier-transition patterns; both are future work.

## 8. Honest limitations

- **Validator-set observation point.** Stellar's tier-1 validator set is public and small. A validator sees a transaction before the network does, and a relayer submitting to a specific validator exposes submission/inclusion-time correlation on a seconds-scale window ordinary observers cannot replicate. This is the residual semi-trusted party — qualitatively weaker than an application server (it does not decide *which* transactions exist; its observation window is bounded to one ledger-closing interval rather than indefinite server-log retention), but not cryptographically absent. Applications facing a nation-state-scale adversary with validator-set collusion should assume this channel is live.
- **Group-size leakage via tier transitions.** The tier is public and fixed at group creation. A group re-created from Small into Medium has monotonically disclosed that it grew past 32 members, and the disclosure does not expire. Applications for which such thresholds are themselves sensitive should start at the largest tier they might ever need.
- **Fee-payer correlation via the relayer.** The relayer's own account is a metadata observation point; a single relayer is better than each member being their own fee payer but worse than a decentralized pool.
- **All members must be online provers.** Delegating Groth16 proving to a server would reintroduce operator trust.
- **Hostile-takeover exposure.** A single current member can replace the entire member set. This is the defining property of the mode studied here; applications that cannot tolerate it should use a different policy.
- **MLS integration is a sketch, not a demonstrated integration.** Earlier drafts claimed Onym "maps an MLS Commit to an `update_commitment` call." That overstates what has been built. Our reference clients use a bespoke MLS-lite transport, not a stock MLS implementation; we have not integrated with a conforming Delivery-Service interface, nor designed the GroupContext-extension encoding for the refreshed salt. We sketch the mapping — an MLS Commit triggers an epoch advance, the new leaf set determines $\mathsf{root}_{\mathsf{new}}$, and the salt would travel in an encrypted GroupContext extension — but concrete integration is future work. Mafia [13] is the directly relevant prior work for the DS-privacy half of it.

## 9. Related work

**Table 1.** Approaches to the metadata-hiding group-membership problem.

| Approach | Example(s) | Trust | Application-layer metadata observable by |
|----------|-----------|-------|------------------------|
| Anonymous credentials on central server | Signal PGS [7] | Server (honest-but-curious, binary-faithful) | Not by server in HbC; by server under active compromise |
| Federated delivery | Matrix, XMPP | Every federated homeserver | Federation peers |
| Transparency logs | CT, Key Transparency | Log operator | Log operator |
| Mixnets | Nym, Loopix | Non-colluding mixnet threshold | Network-layer adversary only |
| ZK-on-ledger registry / pool (varied goals) | Semaphore [4], Zcash [3], Railgun, AZTEC, Nocturne, Azeroth | Chain + SNARK soundness + trusted setup (+ auditor, for Azeroth) | No application operator; chain validators see submission timing; auditor for Azeroth |
| Private MLS Delivery Service | Mafia [13] | MLS DS reduced to ordering oracle over ciphertext | Network-layer adversary only |
| Public-ledger anchoring + SNARK group registry (this work) | Onym | Chain + SNARK SE + trusted setup + Poseidon ROM heuristic | No application operator; Stellar validators see submission timing |

Onym's construction is closest in shape to Semaphore [4]: both anchor a commitment tree on a public ledger and verify membership with a Groth16 SNARK. It differs in that its tree commits to an *evolving group-membership set* rather than a fixed anonymity set, and its authorization predicate is over state *transitions* rather than single-statement membership. Railgun, AZTEC, and Nocturne share the ZK-pool shape but target payment privacy; Azeroth additionally supports a designated auditor. Mafia [13] addresses the MLS-side problem and complements Onym's on-chain registry; the two would plausibly compose in a production stack.

## 10. Conclusion

Onym explores an adjacent frame to Signal PGS — a public anchor, SNARK-gated state changes, no application-layer operator positioned to observe group membership — and is candid that the frame is not trust-free: the chain's validator set is semi-trusted, and privacy bounds on fee-payer unlinkability and tier transitions are weaker than an idealized framing suggests.

The contribution is the engineering and security-modelling lessons: count R1CS cost before host-function cost when a primitive straddles the circuit boundary; a relation's soundness theorem says nothing about operations whose security predicate is over variables the relation does not mention, and operation-binding over Groth16 needs simulation-extractability, not plain knowledge-soundness; privacy invariants in *Security Considerations* bullets of other documents are fiction. Readers should take the postmortems more seriously than the system; the failure modes generalize to any system in this neighborhood.

## Acknowledgments

We thank the Stellar Development Foundation for the multi-year arc of ZK primitives they landed in the chain core: CAP-0059's BLS12-381 host functions in Protocol 22 (Dec 2024) unblocked on-chain Groth16 verification at practical fee cost and is what made this work possible at all; Protocol 25 *X-Ray* (Jan 2026) added BN254 multi-scalar multiplication (CAP-0073), BN254 elliptic-curve and pairing operations (CAP-0074), and native Poseidon / Poseidon2 permutation primitives (CAP-0075), which together dissolve the host-vs-circuit hash trade-off behind the §6.3 lesson and widen the curve and primitive design space for successors. We also thank the arkworks contributors for the Rust Groth16 and Poseidon gadgets, the external code reviewer — contracted for a Groth16-circuit-to-contract-ABI seam pass — whose question *"where in the math is the argument that this proof is non-transferable?"* is the reason P1 is a postmortem rather than a zero-day, and the anonymous reviewer whose hard critique drove the §5.3 rewrite (simulation-extractability, AGM, Poseidon-as-ROM).

**Use of generative AI.** This manuscript was *drafted with editorial assistance from Claude* (Anthropic, 2026), used for prose generation, technical exposition, and structural editing. The first author directed the technical content, wrote and verified the formal statements (relations, games, reductions), and is responsible for the correctness of the work. Claims tied to numbered references — in particular the AGM / SE / ROM choices in §5.3, which are attached to specific cited sources — were verified against those sources. The uncited inline names in §9 (Railgun, AZTEC, Nocturne, Azeroth) are retained at the genre level: they identify widely-known ZK-pool systems in shape but are not pinned to specific bibliography entries here, and no technical claim in this article rests on any one of them. Any reader re-purposing §9 for a lineage argument should attribute those systems to their authoritative specifications directly rather than inherit our inline naming.

## References

[1] R. Barnes et al., "The Messaging Layer Security (MLS) Protocol," IETF RFC 9420, July 2023.

[2] J. Groth, "On the Size of Pairing-Based Non-Interactive Arguments," in *Proc. EUROCRYPT*, 2016, pp. 305–326.

[3] D. Hopwood, S. Bowe, T. Hornby, and N. Wilcox, "Zcash Protocol Specification," version 2024.5.0, Electric Coin Company, 2024.

[4] Privacy & Scaling Explorations, "Semaphore Protocol Specification v4," 2024.

[5] L. Grassi, D. Khovratovich, C. Rechberger, A. Roy, and M. Schofnegger, "Poseidon: A New Hash Function for Zero-Knowledge Proof Systems," in *Proc. USENIX Security*, 2021, pp. 519–535.

[6] G. Fuchsbauer, "Subversion-Zero-Knowledge SNARKs," in *Proc. PKC*, 2018. (Establishes malleability of Groth16 and motivates simulation-extractability.)

[7] M. Chase, T. Perrin, and G. Zaverucha, "The Signal Private Group System and Anonymous Credentials Supporting Efficient Verifiable Encryption," in *Proc. ACM CCS*, 2020.

[8] R. Barbulescu and S. Duquesne, "Updating Key Size Estimations for Pairings," *J. Cryptol.*, vol. 32, no. 4, pp. 1298–1336, 2019.

[9] K. Baghery, M. Kohlweiss, J. Siim, and M. Volkhov, "Another Look at Extraction and Randomization of Groth's zk-SNARK," in *Proc. Financial Cryptography*, 2021. (Weak simulation-extractability of modified Groth16 in AGM+ROM; public-input-hashing modification.)

[10] Stellar Development Foundation, "CAP-0059: Host functions for BLS12-381," Stellar Core Advancement Proposal, Aug. 2024.

[11] H. Lipmaa, "Simulation-Extractable SNARKs Revisited," Cryptology ePrint Archive, Report 2019/612, 2019.

[12] G. Fuchsbauer, E. Kiltz, and J. Loss, "The Algebraic Group Model and its Applications," in *Proc. CRYPTO*, 2018, pp. 33–62.

[13] T. Melissaris et al., "Private Delivery Services for Messaging Layer Security" (project codename *Mafia*), in *Proc. EUROCRYPT*, 2025.
