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

---

# Re-review (commits `9009b43 v4 camera`, `dd87b39 v4 cleanuo`)

Delta against the review above. 12 lines changed in `paper/onym.v4.md`: §4.3 rewritten, the "A note on reviewer expectations" section removed in full. Paper is now 149 lines (was 160).

## What the delta does right

- **"A note on reviewer expectations" removed.** The first review described that section as "unusual but effective"; on re-read it is the correct cut. A camera-ready experience report should not carry a standing dialogue with an earlier reviewer round in its body — it reads as self-referential drafting commentary, and §1 already does the genre-declaration work that section was doing. Removal is a net gain in self-containment.
- **§4.3 prose cleanup.** The self-referential parenthetical *"(Demoted from P1 in earlier drafts; kept for the design-review template.)"* is gone, the heading is softened to *"A demoted lesson"*, and the durable lesson is broken out into its own closing paragraph cleanly separated from the Protocol 25 obsoleting fact. This matches the shape of §4.1 and §4.2 — the postmortem section now has a consistent rhythm across all three entries. One very minor prose note: the first sentence *"An earlier version of Onym composed Poseidon inside the circuit with SHA-256 outside, chosen because Soroban exposes SHA-256 as a native host function"* has *"chosen"* agreeing in number with neither *"an earlier version"* nor *"Poseidon"* cleanly; the v3-era phrasing "SHA-256 was chosen because Soroban has a native host function for it" was grammatically tighter. Pre-camera-ready polish, not a content issue.

## Open-gap re-evaluation

Re-checked each of the three gaps from the prior review against the current file.

**Gap 1 — Acknowledgments human/AI blur.** *Still open, unchanged.* The acknowledgments line *"…the anonymous reviewer whose hard critique shaped the framing of v3 and v4"* still reads as human-peer-reviewer to an unprimed reader, and the *Use of generative AI* paragraph still separately commits that Claude was the interlocutor for the §2 and §4 framing choices — the same framing choices the "anonymous reviewer" was just thanked for. The cleanup pass did not touch this line. One-sentence fix: fold the two sentences together, or make the acknowledgments line specific ("an adversarial-review pass conducted via Claude on prior drafts").

**Gap 2 — No artifact identifier in §5.** *Still open, unchanged.* §5 still opens with "Testnet deployment in alpha phase" with no contract address, repository URL, or build-tag reference. The §4 postmortems cite specific version ranges (*v1.0.0–v1.2.9*, *v1.2.7*, *v1.7.0 on 2026-04-19*); a reader has no artifact to map those to. One line in §5 — contract address plus tag range per postmortem — would close this without reshaping the paper. Unchanged by the camera pass.

**Gap 3 — Validator-trust argument regression.** *Correction needed to my prior review.* On re-read of §6 the bullet reads: *"Seconds-scale observation window around block close; qualitatively weaker than an application server (no ability to decide which transactions exist, bounded retention), but not cryptographically absent."* Two of the three argument clauses from v3 (no decision over which transactions exist; bounded observation window) are in that parenthetical; only the SCP-safety-under-Byzantine-threshold clause is missing. I undercharacterized the bullet as "conclusion but not argument" in the prior review — the parenthetical does carry the argument in compressed form. Remaining delta is a one-clause hedge about SCP's safety threshold, not a whole-paragraph restoration. Demotes this from a gap to a polish item.

## Net assessment of the delta

Both cuts — §4.3 and the reviewer-expectations section — are the correct camera-ready moves. The paper is now structurally self-contained and the §4 postmortem rhythm is consistent. Gap 1 (acknowledgments blur) and gap 2 (no contract address) remain the two substantive items open and are unchanged by this pass. Gap 3 downgrades on re-examination. Approving the delta; the two remaining substantive items are one-line fixes either author can apply in a final polish pass before submission.

---

# Re-review (commits `9b59ad2`, `58471a9`) — and venue question

Two more commits since the last pass. 154 lines now (was 149). Net change to `paper/onym.v4.md`: one acknowledgments-line rewording — *"shaped the framing of v3 and v4"* → *"shaped the paper's framing"*.

## Delta

- **Gap 1 (acknowledgments human/AI blur) — touched but not resolved.** The line now reads *"…the anonymous reviewer whose hard critique shaped the paper's framing."* Dropping "v3 and v4" removes a small self-referential artifact but does not address the substantive issue: an unprimed reader still parses "anonymous reviewer" as a human peer reviewer, and the *Use of generative AI* paragraph still separately credits Claude as the interlocutor for the §2 and §4 framing. The two sentences still describe the same work under different labels. Still a one-sentence fix (fold or specify), still open.
- **Gap 2 (no artifact identifier in §5) — unchanged.** Still no contract address, repository URL, or build-tag mapping for the `v1.0.0–v1.2.9` / `v1.2.7` / `v1.7.0` postmortem versions.
- **Gap 3 (validator-trust argument) — as before, polish not gap.**
- No other body changes.

## Is it good enough?

For the genre it declares — an experience report, postmortems plus a review template, not a proofs or production-evaluation contribution — **yes**, with two one-line polish items open (gap 1 and gap 2). The paper now knows what it is, says so up front, re-says it in the limitations, and the §4 postmortems carry the contribution on their own weight. The writing is tight (154 lines), §4.1 is load-bearing and clean, and the AI-use paragraph is the most direct version across v2–v4. The remaining open items do not require re-doing work and should not gate submission; they are a final polish pass.

## Venue

The paper's genre self-declaration — *"experience report (not a systems-construction or security-proofs submission)"* — rules out the standard crypto venues (Crypto, Eurocrypt, CCS, S&P, USENIX Security) where proofs/production measurements are load-bearing. Correctly so; submitting there would invite exactly the round of reviewer asks v4 §1 is pre-empting, and v4 does not have the content to answer them.

The right venues, in priority order:

1. **Real World Crypto (RWC) — primary recommendation, as a short talk.** RWC is practitioner-facing, explicitly welcomes deployment postmortems and engineering lessons about cryptographic systems in the field, and has no proofs requirement. The P1 postmortem — a relation's public-input structure not matching the operation's security predicate, and the SE-vs-KS distinction that followed — is exactly the shape of content RWC audiences come for. RWC 2027 submissions typically open late summer / early fall 2026; an extended-abstract + talk submission fits naturally. A one-slide version of the design-review template is the right level of detail.
2. **HotPETs (workshop co-located with PoPETS) — secondary.** HotPETs takes short position / experience papers in the privacy-enhancing-technologies space; the co-membership-leakage P2 postmortem and the fee-payer-unlinkability framing in §2 fit the venue's scope. HotPETs is non-archival, which matches the genre and keeps the full archival venue open for a later systems paper.
3. **USENIX `;login:` (online magazine) — for the written artifact.** Long-form engineering postmortems by practitioners are exactly what `;login:` publishes. The current paper reads as a `;login:` article almost unchanged; one editorial pass to soften inline formalism into explanatory prose is all the genre adjustment needed.
4. **IACR ePrint as preprint, in parallel with any of the above.** Low-cost archival step that makes the postmortems citable without committing the paper to a proofs-venue review round.

Not a fit — in addition to the top-tier crypto/security venues above — are Financial Cryptography full track (still proofs-leaning), NDSS (systems-security construction expected), and IEEE S&P Magazine (broader audience; the SNARK-deployment specifics would need heavy translation, weakening the contribution).

One caveat for any of these paths: Gap 2 (missing contract address / tag range in §5) matters more for an external venue than for an internal review. A reader at RWC or HotPETs will want to look at the artifact the postmortems are about; one line in §5 closes this. Gap 1 (acknowledgments blur) is a professional-norms item — AI-use transparency is increasingly expected at RWC and at `;login:` — and the fix is one sentence. Both should be done before submission, neither requires new technical work.

**Bottom line.** Good enough for RWC 2027 as a short talk and for `;login:` as a written article, after the two one-line polish items (artifact id in §5; fold acknowledgments line into AI-use paragraph or make it specific about Claude). Not good enough — and correctly not shaped for — the top-tier crypto/security full-paper venues, which v4 §1 already says it is not targeting.
