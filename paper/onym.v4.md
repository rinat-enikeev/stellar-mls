---
title: "Postmortems from a SNARK-Gated Group Registry on Stellar"
author: Rinat Enikeev
venue: experience report (not a systems-construction or security-proofs submission)
date: 2026-04-21
---

# Postmortems from a SNARK-Gated Group Registry on Stellar

## Abstract

Onym is a Stellar contract that anchors each group's membership state as a 32-byte commitment and gates every state change on a Groth16 zero-knowledge SNARK. The alpha deployment is on Stellar testnet and is scoped to the *unanimous-member* authorization mode, where any one current member may authorize any next state. This article is an **experience report**: it documents two authorization and privacy postmortems from the deployment, a demoted R1CS-cost lesson, and the design-review template we derived. It is not a cryptographic-proofs contribution, not a production-readiness claim, and not a general messaging substrate. Two deployment facts are load-bearing: the Phase-2 trusted setup currently has a single contributor, so every formal claim is vacuous against that party; and for $n=2$ pair chats, fee-payer unlinkability is trivially defeated by realistic side information. Both belong here rather than buried.

## 1. What this paper is, and is not

Messaging content is routinely end-to-end encrypted; membership metadata almost never is. Every mainstream architecture we know of has at least one application-layer operator — a server, a federated peer, a Delivery Service [1] — positioned to observe who is in which group. Signal's Private Group System [7] shows this can be closed to an honest-but-curious operator using anonymous credentials.

Onym asks a narrower question: can the *registry* that records a group's current membership state be pushed onto a public ledger with a SNARK-gated authorization rule, so that no application-layer operator sits in the metadata path at all? The answer we report on is "yes, with substantial caveats" — and the caveats are the most useful part of what follows.

We want to set reviewer expectations directly. This is a registry-primitive experience report, not:

- **a messaging substrate paper** — the MLS-side integration is future work and we do not claim to have built it;
- **a cryptographic-proofs contribution** — §2 states what the SNARK binds in the minimum formality the postmortems need; readers who want full BKSV-style simulation-extractability definitions should consult [9, 11, 12] directly;
- **a production-readiness claim** — the deployment is alpha on testnet, and §6 says so plainly;
- **a general-policy paper** — only the unanimous-member mode is analyzed; other policies shipped in v1.7.0 on 2026-04-19 and are not reviewed here.

The contribution is the engineering postmortems (§4) and the design-review template that fell out of them.

## 2. Construction

Let $\mathbb{F}_r$ be the BLS12-381 scalar field and $G_1 \in \mathbb{G}_1$ a generator. Let $H$ denote Poseidon [5] at the standard BLS12-381 parameter set. For a member set $S = \{\mathsf{pk}_1, \ldots, \mathsf{pk}_n\}$ canonicalized under compressed-G1 serialization, let $\mathsf{root}(S)$ be the Poseidon Merkle root with leaves $H(\mathsf{pk})$. The group commitment is

$$
C = H\!\bigl(H(\mathsf{root}(S), \epsilon),\, s\bigr) \in \mathbb{F}_r,
$$

where $\epsilon$ is the epoch counter and $s$ is a per-epoch salt uniform in $\mathbb{F}_r$. The nested structure is not cosmetic: the inner digest $H(\mathsf{root}, \epsilon)$ is what sub-circuits bind to when they need to reference "this group at this epoch" without disclosing $s$, and keeps the salt-disclosure story orthogonal to root-or-epoch disclosure across related proofs.

Two Groth16 relations [2], both proved on BLS12-381:

- $R_{\mathsf{Mem}}(C, \epsilon)$: prover knows a secret key $sk$, a Merkle path, and the salt $s$ opening $C$ to a tree containing $H(sk \cdot G_1)$.
- $R_{\mathsf{Upd}}(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}, C_{\mathsf{new}})$: $R_{\mathsf{Mem}}$ holds at $(C_{\mathsf{old}}, \epsilon_{\mathsf{old}})$, and $C_{\mathsf{new}}$ is an opening-commitment under epoch $\epsilon_{\mathsf{old}}+1$ with a fresh salt $s_{\mathsf{new}}$.

Crucially, $C_{\mathsf{new}}$ is a public input to $R_{\mathsf{Upd}}$ — part of what the proof binds, not a contract-controlled parameter. Getting this wrong was P1 (§4.1).

**What the SNARK binds, informally.** Simulation-extractability (SE) — not plain knowledge-soundness — is the right property here, because Groth16 is malleable [6]: an adversary can reshape an honest proof into a different accepting proof without a witness. Under SE (in the algebraic group model plus random-oracle heuristic for out-of-circuit Poseidon), any accepting $R_{\mathsf{Upd}}$ proof implies the submitter knows a witness linking the old commitment to the new one via a current member's key, even if the submitter has previously observed simulated proofs. The public-input-hashing modification of [9] (building on [11]) is what makes SE available for Groth16; the deployed contract uses it.

We do not write the formal games here. The genre of this paper does not require them, the minimal content needed for the postmortems is the paragraph above, and the reader who wants the proofs should go to [9, 11, 12] where they are already written.

**Replay.** The contract entry point checks that the submitted $(C_{\mathsf{old}}, \epsilon_{\mathsf{old}})$ match stored state before writing $C_{\mathsf{new}}$ and incrementing the epoch. This is a correctness property of the Soroban runtime, not a cryptographic one, and we treat it as such. A proof captured off-wire has authorization power at most until the next legitimate update wins the first-write race.

**Fee-payer unlinkability, stated cleanly.** Onym's on-chain footprint is information-theoretically independent of which group member authored the update; all unlinkability loss is attributable to side channels outside the ledger. We claim no more than that, and we claim no less. For $n \geq 3$ this is a useful guarantee under reasonable operational assumptions about the relayer; for $n = 2$ it is trivially defeated by realistic auxiliary observations, which is why Onym is a group substrate and not a 1:1 substrate.

**Deployment honesty.** The deployed verification keys are the output of a single-contributor Phase-2 setup. Against the setup operator — the party holding the aggregate simulation trapdoor — the authorization property is vacuous, because that party can forge an accepting proof for any public input including hostile state rewrites. A multi-contributor Phase-2 MPC is the first production-readiness item. We publish these postmortems before that ceremony has been re-run because the lessons are about the engineering missteps that preceded it, not claims about the current deployment's guarantees.

## 3. In plain language

Picture a town hall keeping one sealed envelope per club. The envelope reveals nothing about members. When a member wants to act on the club's behalf, they hand over a slip proving they belong inside the envelope without anyone opening it. When the club changes, one member writes what the next envelope should say, proves "I belong inside yesterday's envelope and today's envelope is the correct next version of it," and the hall swaps envelopes. Nothing on the hall's wall ever names a member.

The hall is the Stellar blockchain. The envelope is a 32-byte Poseidon commitment. The slip is a 192-byte Groth16 proof. The arithmetic check is three BLS12-381 pairings run by a Soroban smart contract. The member's secret is a BLS12-381 private key on their device.

Three things leak despite the envelope. (i) Someone pays the transaction fee; if members paid their own, their real-world-identified Stellar accounts would be linked to group activity, so Onym submits through a public relayer that pays from its own account. The relayer knows which group, when, and who asked — a metadata observation point we do not close. (ii) Each group is created inside a size tier (≤ 32, ≤ 256, or ≤ 2,048 members) which is public; crossing a tier is a monotonic public disclosure. (iii) Stellar validators see the relayer's submission before the network does, on a seconds-scale window around block close.

## 4. Postmortems

### 4.1 P1 — The theorem that proved the wrong thing

**Severity.** Critical authorization vulnerability: any party in the network path between a prover and the contract could substitute the new commitment written to storage while retaining the prover's still-valid proof. Every privacy and authorization guarantee was void for the operation that changes group state.

**What went wrong.** Through v1.2.9 every state-changing operation shared one circuit with relation $R_{\mathsf{Mem}}$ and public inputs $(C, \epsilon)$. The `update_commitment` entry point accepted a separate `new_commitment: BytesN<32>` parameter that the contract wrote to storage. The proof authorized knowledge of a member in the *current* state; it said nothing about which next state was being authorized. `new_commitment` was a contract-controlled free parameter.

**Who caught it.** An external reviewer, contracted for a Groth16-circuit-to-contract-ABI seam pass, asked: *"Where in the math is the argument that this proof is non-transferable to a different `new_commitment`?"* The answer was: nowhere. A proof binds to its public inputs and nothing else. A second subtlety surfaced during the fix: even once $C_{\mathsf{new}}$ is a public input, plain Groth16 is malleable [6], so what is needed is simulation-extractability, not plain knowledge-soundness. Making $C_{\mathsf{new}}$ an IC input was necessary but not sufficient until the public-input-hashing modification of [9] was added.

**Why four preceding reviews did not catch it.** The system had passed four structured reviews — circuit soundness, contract ABI, primitive choice, wire-format correctness — each well-scoped within itself. The seam that failed was *between* scopes: no pass owned the question of whether the operation's security predicate was expressible in terms of the statement the proof actually bound. The lesson is about review topology, not individual reviewers.

**Fix and lesson.** Split the relation — $R_{\mathsf{Mem}}$ unchanged, $R_{\mathsf{Upd}}$ new with $(C_{\mathsf{old}}, \epsilon_{\mathsf{old}}, C_{\mathsf{new}})$ public, public inputs hashed into the Fiat-Shamir transcript per [9] to obtain SE. The deployed contract was re-read by the same external reviewer after the fix. Formal soundness of a relation says nothing about the security of an operation whose security predicate is over variables the relation does not mention. Our design-review template now requires, per authorized operation: (1) a one-sentence predicate stating what the proof binds; (2) an enumeration of every value the contract reads on that path, labeled *in IC* or *contract-controlled*; (3) a side-by-side read of the relation file and the contract entry-point; (4) an explicit statement of whether knowledge-soundness or simulation-extractability is the intended level.

### 4.2 P2 — Transport-layer co-membership leakage

Onym's clients exchange invitations and ciphertext over Nostr relays. Each Nostr event is signed by a secp256k1 key. Through v1.2.7 each client signed every event with its *long-lived* identity key. A companion transport spec mentioned the implication in one bullet of its Security Considerations:

> *"Stable Nostr device keys improve usability but increase sender linkability."*

Technically true, strategically misleading: the actual consequence was *group linkability* — any relay could cluster the user base by co-membership from public relay data, "these N keys all published to hidden-topic T → these N devices are in the same group."

**Fix.** Ephemeral per-event secp256k1 keys; topic tags now derived per-epoch as $H(\mathsf{group\_id} \,\|\, \epsilon)$ and rotated on every state change; a receive-side audit moved four code paths from `event.pubkey` to the inner BLS public key. We do not claim that no traffic-analysis-based co-membership inference remains — relay-level timing correlation is an open residual channel and also feeds the fee-payer side-channel story in §2.

**Lesson.** A privacy invariant that lives in a *Security Considerations* bullet of a different document is fiction. Identity coupling — one key serving as both signing key and sender identity — is a recurring antipattern; decoupling is almost free on day one and a postmortem otherwise.

### 4.3 A demoted lesson: host-function hashes as a constraint-budget landmine

An earlier version of Onym composed Poseidon inside the circuit with SHA-256 outside, chosen because Soroban exposes SHA-256 as a native host function. Because the circuit must reproduce the outer hash to bind its inner Poseidon output to the commitment the contract observes, the outer primitive's R1CS cost is paid in full at proving time: one SHA-256 compression requires approximately 25,000 R1CS constraints in the standard arkworks gadget, which dominated the small-tier circuit by an order of magnitude. Replacing the outer SHA-256 with two Poseidon calls (approximately 600 constraints in total) reduced R1CS cost by 8–14× across tiers.

The durable takeaway — *R1CS cost dominates host-function cost for any primitive that straddles the circuit boundary* — is what the design-review template retains. The specific SHA-256-versus-Poseidon trade-off that produced the mistake is now moot on Stellar: Protocol 25's CAP-0075 (January 2026) adds native Poseidon and Poseidon2 host functions, collapsing the host-side asymmetry that made SHA-256 the attractive default.

## 5. Evaluation

Testnet deployment in alpha phase. Groth16 proofs are 192 bytes; public inputs add 32 bytes for $R_{\mathsf{Mem}}$ and 96 bytes for $R_{\mathsf{Upd}}$. On-chain verification invokes three BLS12-381 pairings plus a small multi-scalar multiplication; Soroban instruction count is 6–10M per update, inside testnet fee budgets. We do not report production group counts or update volumes; the system is not in production.

What an observer learns, assuming an eventual honest Phase-2 MPC: that a group exists, how many state changes have occurred and when, its current size tier and any prior tier it graduated out of, and the relayer's identity. Not learned: the member set, the updater's identity, the exact count within tier, or whether the group shrank or rotated within tier. We have not performed a formal differential-privacy analysis of the timing channel nor quantified tier-transition information leakage; methodology and measurements are future work.

## 6. Limitations

- **Single-contributor Phase-2 setup.** Restated here because it is the gating limitation for every authorization claim. Until this is redone as a multi-contributor MPC, the deployment's guarantees are vacuous against the setup operator.
- **Unanimous-member mode only.** A single current member can replace the entire set. That is the defining property of the mode; applications that cannot tolerate it need a different policy.
- **Pair chats.** Not a supported use case, per the clean statement in §2.
- **Validator-set timing channel.** Seconds-scale observation window around block close; qualitatively weaker than an application server (no ability to decide which transactions exist, bounded retention), but not cryptographically absent.
- **MLS integration is a sketch, not a demonstration.** We do not claim to have integrated with a conforming MLS Delivery Service; Mafia [13] is the directly relevant prior work on the DS-metadata side of the problem and the plausible composition partner.
- **Key compromise in unanimous mode.** A stolen member key authorizes arbitrary state transitions including expelling all other members. Recovery is a policy-layer question and is not solved here.
- **Post-quantum.** BLS12-381 + Groth16 is not PQ-secure. Migration to a PQ-curve or transparent-setup system (e.g., PLONK/FRI-based) is a deferred design question.

## 7. Related work

Closest in construction is Semaphore [4]: both anchor a commitment tree on a public ledger and verify membership with a Groth16 SNARK. Onym differs in that the tree commits to an *evolving group-membership set* rather than a fixed anonymity set, and the authorization predicate is over state *transitions* rather than single-statement membership. Signal PGS [7] is the gold standard for metadata-private group membership under a trust-the-operator assumption; Onym occupies an adjacent design point (no application-layer operator, public-chain validator set as residual trust). Mafia [13] addresses the MLS-DS half of the metadata problem and would plausibly compose with Onym in a production stack.

We do not include a broader ZK-pool lineage table (Railgun, AZTEC, Nocturne, Azeroth, etc.) because none of those systems is a close comparable for a group-membership registry with transition authorization, and gesturing at them at the genre level does not help a reader who already knows the space.

## 8. Conclusion

The contribution is two postmortems and a design-review template, not a production system or a cryptographic proofs contribution. The engineering takeaways generalize: a relation's soundness theorem says nothing about operations whose security predicate is over variables the relation does not mention, and operation-binding over Groth16 needs simulation-extractability rather than plain knowledge-soundness; privacy invariants that live in *Security Considerations* bullets of other documents are fiction; R1CS cost dominates host-function cost for primitives that straddle the circuit boundary. Readers should take the postmortems more seriously than the system.

## Acknowledgments

We thank the Stellar Development Foundation for the ZK primitives landed in the chain core (CAP-0059 BLS12-381 in Protocol 22, CAP-0073/74/75 in Protocol 25 X-Ray) that made this work practical; the arkworks contributors for the Rust Groth16 and Poseidon gadgets; the external code reviewer whose P1-catching question this paper takes its name from; and the anonymous reviewer whose hard critique shaped the paper's framing.

**Use of generative AI.** The author is a builder, not a cryptographer. Onym was designed, implemented, and deployed before any of §2's language existed on paper. Turning the working system into writing that a cryptographer would read as sound required consulting Claude (Anthropic, 2026) extensively — as a tutor on the literature (AGM, simulation-extractability, the public-input-hashing modification, Poseidon's ROM framing) and as an interlocutor for the framing choices in §2 and §4. For each framing, the author read the sources Claude surfaced and adopted it only after the sources checked out. This article is a builder's postmortems and review lessons, not a protocol proposal; the postmortems are the author's work, the author is accountable for every technical statement and any remaining errors, and the formal scaffolding around the postmortems is in the paper at all because AI made it accessible to a builder.

## References

[1] R. Barnes et al., "The Messaging Layer Security (MLS) Protocol," IETF RFC 9420, 2023.

[2] J. Groth, "On the Size of Pairing-Based Non-Interactive Arguments," *EUROCRYPT*, 2016.

[3] D. Hopwood et al., "Zcash Protocol Specification," v2024.5.0, Electric Coin Company, 2024.

[4] Privacy & Scaling Explorations, "Semaphore Protocol Specification v4," 2024.

[5] L. Grassi et al., "Poseidon: A New Hash Function for Zero-Knowledge Proof Systems," *USENIX Security*, 2021.

[6] G. Fuchsbauer, "Subversion-Zero-Knowledge SNARKs," *PKC*, 2018.

[7] M. Chase, T. Perrin, G. Zaverucha, "The Signal Private Group System and Anonymous Credentials Supporting Efficient Verifiable Encryption," *ACM CCS*, 2020.

[8] R. Barbulescu, S. Duquesne, "Updating Key Size Estimations for Pairings," *J. Cryptol.* 32(4), 2019.

[9] K. Baghery, M. Kohlweiss, J. Siim, M. Volkhov, "Another Look at Extraction and Randomization of Groth's zk-SNARK," *Financial Cryptography*, 2021.

[10] Stellar Development Foundation, "CAP-0059: Host functions for BLS12-381," 2024.

[11] H. Lipmaa, "Simulation-Extractable SNARKs Revisited," IACR ePrint 2019/612.

[12] G. Fuchsbauer, E. Kiltz, J. Loss, "The Algebraic Group Model and its Applications," *CRYPTO*, 2018.

[13] T. Melissaris et al., "Private Delivery Services for Messaging Layer Security" (*Mafia*), *EUROCRYPT*, 2025.
