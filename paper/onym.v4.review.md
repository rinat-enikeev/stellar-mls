---
title: "Review of onym.v4.md"
author: "@gramyzer (on instruction from @rinat-enikeev, PR #89 comment 4286446241)"
date: 2026-04-21
scope: paper/onym.v4.md
---

# Review of `paper/onym.v4.md`

Read against v3 and against the four gaps the v3 honesty review flagged. 160 lines vs. v3's 231; the compression is genre-driven rather than content-loss.

## What v4 gets materially right

- **Title no longer overclaims.** *"Metadata Without Observers"* is gone; the paper now reads *"Postmortems from a SNARK-Gated Group Registry on Stellar."* Honesty-review gap (1) closed.
- **Genre framing arrives in the abstract and frontmatter.** `venue: experience report (not a systems-construction or security-proofs submission)`; first paragraph of the abstract names the genre; §1 is titled "What this paper is, and is not" and enumerates four things this paper is *not*. Honesty-review gap (3) closed and then some.
- **Phase-2 single-contributor setup and $n=2$ vacuity are both in the abstract**, framed explicitly as "load-bearing." §2's *Deployment honesty* paragraph restates the Phase-2 fact where a reader will actually see it, and §6 limitations repeats it as the first bullet. This is the single most consequential caveat and v4 does not bury it.
- **Formal scaffolding is right-sized to the genre.** §2 gives the minimum needed for the postmortems to make sense — an informal SE paragraph citing [9, 11, 12], a clean fee-payer-unlinkability paragraph separating on-chain from side-channel contributions, and a one-paragraph replay argument. §2 explicitly declines to write the games formally and points the reader to [9, 11, 12]. Consistent with an experience report.
- **"A note on reviewer expectations" (§after 8)** is unusual but effective. It directly addresses the hard-critique feedback loop from earlier drafts: names which two asks were adopted (title retraction, registry-primitive commitment, Phase-2 status promoted) and which asks are out-of-scope for this genre. This is healthier than leaving the same critique to re-surface each round.
- **MLS integration claim is withdrawn cleanly in §6** ("MLS integration is a sketch, not a demonstration"). Matches v3's honesty on this point.
- **R1CS lesson in §4.3 is demoted with the demotion owned**, and the paragraph closes with the durable lesson cleanly separated from the obsoleting Protocol 25 fact. This is the right shape for an engineering postmortem.
- **Evaluation (§5) owns the measurement gap** — "We do not report production group counts or update volumes; the system is not in production." The 6–10M instruction-count figure is presented as a range, not a measurement table. Honest about what the paper is not.
- **Acknowledgments/AI paragraph is tighter and more direct.** The AI-use paragraph names specific roles ("tutor on the literature," "interlocutor for the framing choices in §2 and §4") and commits to author accountability in plain terms.

## What is still open

Three gaps remain. All are one-to-three-sentence fixes, none blocking.

### 1. Acknowledgments still blur human / AI attribution

The honesty review flagged this in v3 and v4 does not fully address it. The acknowledgments line reads:

> "…the anonymous reviewer whose hard critique shaped the framing of v3 and v4."

A reader will interpret "anonymous reviewer" as a human peer reviewer. The subsequent *Use of generative AI* paragraph commits that Claude was the interlocutor for "the framing choices in §2 and §4" — which is exactly what the "anonymous reviewer whose hard critique shaped the framing" did. Either fold the two sentences together ("an adversarial-review pass run on prior drafts via Claude"), or make the acknowledgments line specific enough that a reader does not read "anonymous human reviewer" into it. The rest of §Acknowledgments is careful on attribution; this one line is still the fuzzy part.

### 2. No artifact identifier for the deployed contract

v4 no longer makes the "externally auditable from deployed bytecode alone" claim that v3 made (v3's §2 asserted it explicitly; v4 has softened this to "no application-layer operator sits in the metadata path at all"), so the specific gap the honesty review flagged is narrower now. But §5 still says "Testnet deployment in alpha phase" without a contract address, repository URL, or build hash. For an experience report whose postmortems reference a specific contract version history (v1.0.0–v1.2.9, v1.7.0 on 2026-04-19), a reader has no way to look at the artifact the postmortems are about. One line in §5 citing the contract address and the tag range for each postmortem would make the reported-on system concrete without changing the paper's shape.

### 3. §3's third leak ("validators see submission before the network does, on a seconds-scale window around block close") is good, but §6's *Validator-set timing channel* bullet is now the only place §3's point is reconciled with the trust-base framing. v3 had a dedicated §2 paragraph arguing *why* this residual channel is qualitatively weaker than an application-server observation point (no decision over which transactions exist; bounded observation window; SCP safety constraint against the rest of tier-1). That argument is the reason a reader should accept that validator-set observation is a residual channel rather than a full operator. v4's §6 bullet restates the conclusion but not the argument. Either restore one sentence of the argument in §6 or in §3, or accept that the experience-report genre does not require the trust-base argument to be self-contained. The current state is a small regression on a load-bearing point the v3 review actually adopted.

## Secondary notes

- **§2 construction, $H$'s arity.** v3 typed Poseidon as $H: \mathbb{F}_r^* \to \mathbb{F}_r$ (variadic). v4 only says "$H$ denote[s] Poseidon" without specifying arity, yet the construction writes $H(\mathsf{root}(S), \epsilon)$ (2-to-1) and $H(\mathsf{pk})$ (1-to-1). Cosmetic; one half-sentence fixes it.
- **§2 "*What the SNARK binds, informally*"** is the correct genre move, but the phrase "(in the algebraic group model plus random-oracle heuristic for out-of-circuit Poseidon)" is inline in a prose paragraph and a reader less familiar with AGM / ROM may not track it. §1 already said "readers who want full BKSV-style simulation-extractability definitions should consult [9, 11, 12] directly" — that pointer could move one paragraph down next to the AGM+ROM mention so a reader encounters both at the same point.
- **§4.1 is the strongest writing in the paper.** "A proof binds to its public inputs and nothing else. A second subtlety surfaced during the fix: even once $C_{\mathsf{new}}$ is a public input, plain Groth16 is malleable [6], so what is needed is simulation-extractability, not plain knowledge-soundness." This is the load-bearing postmortem sentence and it carries its weight cleanly.
- **§4.2 P2's "identity coupling is a recurring antipattern" lesson** is generalizable and well-stated. This is a takeaway a reader working on unrelated transport-layer systems can actually use.

## Summary

v4 is a material improvement over v3 on the four axes the honesty review flagged: title no longer overclaims (adopted), genre declared in abstract (adopted), Phase-2 promoted (already done in v3, retained), auditability-claim/artifact-id (softened the claim; artifact id still missing but less load-bearing now). The "reviewer-expectations" §is an unusual but honest addition. Three gaps remain — the acknowledgments-line human/AI blur, the missing contract address in §5, and the regression on the validator-trust argument — none of which require re-doing work. v4 reads as a paper that knows what it is and what it is not; that is the most important property for an experience report to have.
