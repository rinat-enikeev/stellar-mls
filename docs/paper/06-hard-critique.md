# Hard Critique of the Onym IEEE S&P Submission

**Reviewer stance:** Adversarial but constructive. The prior self-critique (02-critique.md) caught many presentation-level issues and the revised draft (03-revised.md) addressed them competently. This critique targets deeper structural and intellectual problems, with particular emphasis on the motivation, which remains the weakest part of the paper.

**Recommendation:** Reject as submitted. The engineering content in Sec. 4-5 is genuinely valuable, but the paper does not earn the reader's trust that the system it describes *should exist*. A strong revision that honestly confronts the motivation gap could produce an accept.

---

## Part I: Motivation — the load-bearing weakness

### C1. The paper never makes the case that blockchain-anchored group membership solves a real problem.

This is the central weakness and everything else flows from it.

The introduction asserts: "Blockchains are a natural anchor for [group membership]. They provide global ordering, immutability, and permissionless auditability — exactly the guarantees a shared membership state needs." This is not an argument; it is a restatement of blockchain properties dressed as a motivation. The paper never answers the question a skeptical IEEE S&P reader will immediately ask: **who is harmed by the trusted Delivery Service that MLS already has, and what concrete threat does Onym mitigate that the existing MLS architecture does not?**

MLS's Delivery Service is not a theoretical construct — it is the operational reality of every deployed MLS implementation (Google Messages, Cisco Webex, Wire, etc.). Those systems work. Their Delivery Service provides ordering. The paper's entire premise is that replacing this with a blockchain is desirable, but:

- No real-world incident is cited where a Delivery Service was compromised and membership state was forged or leaked.
- No threat scenario is described where a blockchain anchor would have prevented harm that actually occurred.
- No user population is identified that has expressed a need for this.
- No deployment partner, pilot, or letter of intent is mentioned.

The paper is a solution in search of a problem. The motivation section reads as if the author started from "I want to build ZK proofs on Stellar" and worked backwards to "therefore MLS needs blockchain anchoring." That may be unfair to the actual development history, but the text as written provides no evidence to the contrary.

**What would fix this:** Name a concrete, real-world scenario where the trusted Delivery Service is the weak link. Dissident coordination in a state-surveillance context? Multi-jurisdictional groups where no single DS operator is trusted by all parties? If the answer is "no one has this problem yet, but they will," say that explicitly and defend the claim — but also accept that the paper then belongs in a workshop or vision track, not as a deployed-system feature article.

### C2. The MLS coupling is asserted, not demonstrated.

The paper claims tight coupling to MLS (Sec. 3: "Each MLS Commit produces a new group state. Onym maps a Commit to an `update_commitment` call"). But:

- No MLS library integration is shown or cited. The paper says there is a "reference iOS and Android chat client" but never describes how MLS Commits actually flow through Onym in practice.
- The `SEPRekeyEnvelope` mechanism (Sec. 8) is described as "not yet integrated with a stock MLS library." So the MLS integration is incomplete *by the paper's own admission*.
- The paper never addresses the fundamental tension: MLS is designed around a tree-based ratchet with efficient key updates. Onym requires regenerating a Merkle tree and producing a ZK proof on every membership change. What is the latency cost of adding Onym to an MLS Commit? What happens if two members issue concurrent Commits (a standard MLS scenario)? How does Onym handle MLS's proposal/commit distinction?

The MLS motivation is doing double duty as both the problem statement and the credibility anchor ("we're building on a real IETF standard"), but the actual integration is shallow enough to raise the question of whether MLS is a genuine design driver or a prestige citation.

### C3. The alternative-solutions space is entirely unexplored.

The paper compares Onym to Tornado Cash, Semaphore, and Zcash (Table 4) — all blockchain+ZK systems. It never considers whether the problem (if it exists; see C1) could be solved without a blockchain at all:

- **Transparency logs** (Certificate Transparency-style) provide append-only, publicly auditable ordering without blockchain overhead.
- **Federated consensus** among a small set of DS operators (the way email works) could provide ordering without a single trust point.
- **Hash chains with gossiped checkpoints** provide tamper-evidence without blockchain gas costs.
- **Key Transparency** (Google's, or the IETF draft) is explicitly designed for the problem of verifiable membership/key directories and is further along in standardization than Onym.

By comparing only to other blockchain+ZK systems, the paper avoids the harder question: is the blockchain+ZK approach *the right tool* for this job, or is it a maximally complex solution to a problem that admits simpler answers?

### C4. "Deployed on testnet" is not "deployed."

The abstract says "deployed." The body says "deployed on Stellar testnet." These are not the same thing. A testnet deployment has:

- No real users
- No real economic stakes
- No adversarial environment
- No real transaction costs
- No uptime requirements

The paper is honest enough to say mainnet is gated on Phase-2 MPC + audit, but the abstract's "deployed" framing is misleading. A system that has never processed a real transaction under real conditions is a prototype, not a deployment. IEEE S&P readers who have shipped production security systems will notice this immediately.

### C5. The demand-side evidence is zero.

The paper mentions four potential use cases in the introduction: "End-to-end encrypted messaging groups, anonymous DAO voting rolls, multi-party signing committees, and credential-issuance schemes." For each:

- **Messaging groups:** No messaging app has expressed interest. Signal, WhatsApp, and iMessage all use centralized or federated delivery. Wire and Matrix have their own ordering. Who is the customer?
- **DAO voting:** DAOs that need anonymous voting already use Semaphore or custom Merkle-tree schemes on Ethereum/L2s, where the DeFi ecosystem lives. Why would they move to Stellar?
- **Signing committees:** Multi-party signing (MPC/threshold) schemes have their own group management. Where is the gap Onym fills?
- **Credential issuance:** This is a hand-wave with no elaboration anywhere in the paper.

A single paragraph describing a real user, a real partner, or even a real conversation with a potential adopter would materially strengthen the paper. Its absence is conspicuous.

---

## Part II: Structural and intellectual issues

### C6. The "two incidents" framing is dishonest about severity.

The paper frames the SHA-256-to-Poseidon switch (Sec. 4) and the unbound-`new_commitment` gap (Sec. 5) as peer incidents — "two design choices that dominated the system's outcome." They are not peers:

- Sec. 4 is an optimization. The system was slow but correct.
- Sec. 5 is a **critical security vulnerability**. The system's authorization model was broken. Anyone in the network path could substitute arbitrary state transitions. The entire privacy guarantee was void.

Calling a critical vulnerability a "design choice" that sits alongside a performance optimization is a framing that minimizes the severity. An IEEE S&P reader will notice that the paper avoids the word "vulnerability" entirely. The closest it comes is "authorisation-binding gap." This is euphemistic for a system that claims to be a security primitive.

The honest framing would be: "We shipped a system with a critical authorization bypass that survived four audits. Here is what went wrong and what we learned." That framing is actually *more* compelling for IEEE S&P — the magazine values honest postmortems.

### C7. "Four audit passes missed it" demands more detail or should be cut.

The paper claims four security audits failed to catch the unbound-`new_commitment` gap, then offers three structural reasons why. But:

- Who conducted these audits? Internal reviews? External firms? Formal verification teams?
- What was their scope? If the scope was "review the circuit" and not "review the circuit-contract interface," then it is unsurprising they missed an interface bug, and the lesson is about scoping, not about the subtlety of the bug.
- Were the auditors given the same "soundness theorem" that proved the wrong thing? If so, the lesson is about the danger of formal proofs as social proof — auditors see a theorem and stop looking.

Without this context, "four audit passes" is either an indictment of the audit process (which the paper should own) or a misleading claim that inflates the difficulty of finding the bug (which undermines the paper's credibility).

### C8. The formal soundness claim is undermined by the paper's own narrative.

The paper says: "We had a formal soundness theorem to that effect. A code-review question changed our reading of it." This is a devastating admission. The paper had a formal proof that was correct but irrelevant. The natural reader question is: **why should I trust the current formal claims?**

The revised system now has $R_\text{Update}$ with its own soundness argument. But the paper never addresses why the new argument is more trustworthy than the old one. The same team, the same methodology, the same review process produced the first (wrong) theorem. What has changed? If the answer is "we now know to check that the theorem's relation matches the operation," that is a process improvement, not a mathematical guarantee.

The paper should either (a) submit the new soundness argument to an independent formal verification and report the result, or (b) explicitly acknowledge that the reader has no reason to trust the new theorem more than the old one, other than the team's increased awareness.

### C9. The trusted setup is a critical dependency that is buried and unresolved.

Groth16 requires a trusted setup. The paper acknowledges (Sec. 8) that the Phase-2 ceremony has not been done and that "the current implementation derives the final Groth16 keys on a single machine, which sacrifices the 1-of-N trust property." This means:

- The person who ran the setup can forge arbitrary proofs.
- Every security game in Table 1 is currently moot.
- The system's security guarantees are literally zero until the ceremony is completed.

This is not a "limitation" to be mentioned in Sec. 8 alongside group-size leakage and fee-payer correlation. It is a fundamental prerequisite that the paper should foreground. A system whose security reduces to "trust the developer who ran the setup" is not a zero-knowledge system — it is a single-party trust system with extra steps.

### C10. Stellar is never justified as the right chain.

The paper says Onym uses Stellar because Protocol 22 provides BLS12-381 host functions. But:

- Ethereum has BN254 precompiles (EIP-196/197) that have been used for Groth16 verification since 2019 (Tornado Cash, Semaphore, etc.).
- Ethereum L2s (Aztec, zkSync, Scroll) have even cheaper verification.
- Stellar's smart contract platform (Soroban) is young, has a small developer ecosystem, and lacks the battle-testing of Ethereum's EVM.

The paper does not address why BLS12-381 is preferred over BN254 (which has more tooling, more audited circuits, and more deployment history), nor why Stellar is preferred over Ethereum or an L2. The choice of Stellar reads as an ecosystem affiliation rather than a technical decision.

---

## Part III: Presentation issues the prior critique missed

### C11. The abstract buries the lede.

The abstract leads with the construction ("Poseidon Merkle commitment and a Groth16 zero-knowledge proof"), then describes the optimization, then the vulnerability. An IEEE S&P reader scanning abstracts wants to know: what is the system, what problem does it solve, and what did you learn? The "what problem does it solve" part is a subordinate clause ("anchoring MLS-compatible group membership on the Stellar blockchain") that a reader could easily miss.

### C12. The paper has no evaluation of the privacy properties.

The paper claims privacy (member identity hidden, updater identity hidden, group size hidden up to tier). But there is no evaluation:

- No formal privacy analysis (differential privacy, simulation-based, game-based beyond the assertions in Table 1).
- No empirical privacy analysis (e.g., what can an observer learn from timing patterns, transaction frequency, tier transitions?).
- No comparison of privacy guarantees with competing approaches.

The "security games" in Table 1 are *asserted* but the proofs are in an uncited "companion soundness document" that the reader cannot access. This is not acceptable for a venue that values verifiable claims.

### C13. The writing is too long for the content.

At 6,438 words, the paper is at the upper end of IEEE S&P feature length. Much of this is spent on implementation details (ABI, wire formats, client platforms) that belong in documentation, not in a research article. The paper would be stronger at 5,000 words with the implementation details cut and the motivation section expanded.

---

## Summary

The paper's engineering content — the constraint-budget analysis (Sec. 4) and the operation-binding incident (Sec. 5) — is genuinely useful to the ZK-applications community. The writing is clear and the incident reports are honest (with the framing caveats noted in C6). But the paper fails to establish that the system it describes addresses a real need (C1-C5), euphemizes a critical vulnerability (C6), leaves its own formal claims in a trust vacuum (C8), and buries a fundamental security prerequisite (C9).

The strongest version of this paper would:
1. Lead with an honest problem statement ("no one has asked for this yet, but here is why we believe they will, and here are the engineering lessons we learned building it anyway").
2. Call the Sec. 5 incident what it is: a critical authorization vulnerability, not a "design choice."
3. Foreground the trusted-setup dependency as the system's current security ceiling.
4. Cut 1,000 words of implementation detail and use the space for motivation and privacy evaluation.
