---
title: "Venue selection and publishing checklist for onym.v4"
author: Rinat Enikeev
date: 2026-04-21
scope: paper/onym.v4.md
---

# Venue selection and publishing checklist

Companion to `paper/onym.v4.md`. The paper declares itself an **experience report** — postmortems and a design-review template from an alpha SNARK-gated group registry on Stellar testnet. That genre decision is load-bearing for venue fit: proofs-track venues will reject the framing, and pure-systems venues will ask for evaluation data the paper does not have. The venue shortlist below filters on genre first.

## 1. What this paper is, for venue matching

- **Genre.** Practitioner / experience report with a modest formal-scaffolding appendix (informal SE framing in §2), two postmortems (§4.1, §4.2), one demoted engineering lesson (§4.3), and a design-review template as the durable takeaway.
- **Length.** 154 lines of Markdown, approx. 3,500 words. Will expand ~15–25% under most venue templates (IEEE / ACM double-column adds ~10% vs. single-column Markdown).
- **Evaluation content.** Instruction-count range (6–10M), proof-size / public-input sizes, constraint-cost reduction (8–14×). **No measurement table, no production data.** §5 explicitly declines.
- **Claims on trust base.** Alpha, testnet, single-contributor Phase-2, unanimous-member mode only. Every claim is scoped against those.
- **Artifact posture.** Deployed Soroban contract exists on testnet; no contract address currently in the paper (open gap — see §3).
- **AI disclosure.** Author used Claude as literature tutor and §2 / §4 framing interlocutor; disclosed in Acknowledgments. Each venue's AI-use policy must be re-checked at submission time because those policies have changed repeatedly across 2024–2026.

## 2. Venues, ranked

Ranking is by genre fit, then reviewer pool competence on the technical substrate (Groth16, BLS12-381, MLS, smart-contract metadata), then turnaround.

### Tier A — strong genre fit

**A1. IEEE Security & Privacy Magazine** (bimonthly, IEEE Computer Society)
- *Fit.* Explicitly welcomes experience articles and field reports. Audience is security practitioners + researchers. Postmortems-and-lessons is a recurring article shape in this magazine.
- *Length.* Typically 4,000–6,000 words. v4 is slightly short and can expand on §4.3 and §7 without padding.
- *Review model.* Editorial + external review; not double-blind. Turnaround usually 2–4 months to first decision.
- *Why it is the top choice.* The paper's durable outputs — review-topology lesson, identity-coupling antipattern, R1CS-cost-dominates-host-function rule — transfer to practitioners working outside Stellar. Magazine-form editing will also improve §3 prose.
- *Watch-outs.* Magazine articles are citable but not considered a "conference/journal publication" for some academic metrics; confirm this is acceptable for author's purposes before committing.

**A2. ACM Queue / CACM Practice section**
- *Fit.* ACM Queue is ACM's practitioner venue; strongest-edited practitioner articles migrate to CACM's Practice section (co-published). "Postmortems from a deployed SNARK registry" is directly in the content space Queue covers.
- *Length.* 3,000–6,000 words.
- *Review model.* Editorial review by the Queue editorial board; not double-blind.
- *Why it could beat A1.* Wider non-security-specialist reach; stronger copy editing; CACM Practice cross-publication amplifies reach into the broader CS community. Weaker reviewer depth on the Groth16/SE specifics than A1.
- *Watch-outs.* Editorial process can be slower and more iterative than IEEE S&P Magazine.

**A3. USENIX ;login: successor / "Deployable Security" angle**
- *Context.* USENIX ;login: ceased publication in 2021. The practitioner-article role has partially migrated to USENIX blog posts and to SREcon proceedings, neither of which is citable as a paper in the traditional sense.
- *Fit.* Low as a paper venue; listed here only to rule out.

### Tier B — research conferences with experience / deployed-systems paths

**B1. ACSAC — Annual Computer Security Applications Conference**
- *Fit.* Explicitly accepts deployed-systems and experience papers; the "applications" framing in the name is honored by the PC. Accept rate historically 20–24%.
- *Length.* ~12 pages in ACM sigconf. v4 would need substantial expansion to fill 12 pages *honestly* — likely by growing §2 formal scaffolding toward SE definitions and adding a measurement section with tier × operation cells.
- *Watch-outs.* Expansion pressure may force the paper across the genre line it carefully declared in §1. If expansion would mean "add formal games the paper explicitly declines to write," do not submit here.

**B2. DSN Industry Track — Dependable Systems and Networks**
- *Fit.* Industry track explicitly accepts experience reports and field studies. Security-overlap is real but DSN's core is dependability — the fit is "systems built and what broke," which matches the paper shape.
- *Length.* Shorter than main track, 6–8 pages.
- *Watch-outs.* Reviewer pool is thinner on ZK / SNARK substrate; the SE paragraph in §2 may land as "trust me" rather than as a defended claim.

**B3. Financial Cryptography (FC) — Workshop on Trusted Smart Contracts (WTSC)**
- *Fit.* FC's associated workshops include WTSC which is the natural home for Stellar/Soroban contract-deployment lessons. The P1 story about operation-binding vs. knowledge-soundness is directly on-topic.
- *Length.* Workshop paper, ~12 pages.
- *Watch-outs.* Workshops are less archival than main-conference proceedings; counts for some author purposes, not others.

### Tier C — specialist / long-tail options

**C1. ZKProof community workshop** — The ZKProof effort publishes community drafts and standardization-track documents. Not a conventional proceedings venue; useful for the design-review template piece to influence the wider SNARK-deployer community.

**C2. Real World Crypto (RWC)** — *Talk* venue, not a paper venue; submissions are short abstracts and accepted talks are filmed and archived. If the paper is accepted elsewhere, RWC is the highest-leverage talk venue for this audience.

**C3. IACR ePrint** — Preprint / archival, not a venue. Cross-post once a venue is chosen and the venue's preprint policy permits.

**C4. arXiv (cs.CR)** — Same role as C3 for a broader CS audience. Prefer IACR ePrint for this specific paper; arXiv is secondary.

### Venues to avoid

- **IEEE S&P (Oakland), ACM CCS, USENIX Security, NDSS, EUROCRYPT, CRYPTO, ASIACRYPT, TCC.** The paper explicitly declines to be a proofs or systems-construction contribution. These venues would either desk-reject for genre or expand-reject after review for missing the formal content the paper declares out of scope. Do not submit.
- **Journal of Cryptology, IEEE TIFS, ACM TOPS, TDSC.** Journal review cycles are long (6–18 months) and these journals want either a full formal treatment or a rigorous measurement study. v4 is neither. Do not submit.

## 3. Pre-submission work on the paper itself

These are closed before any venue is chosen; they are not venue-dependent.

### 3.1 Must-fix (flagged in `onym.v4.review.md`, still open)

- [ ] **Artifact identifier in §5.** Add deployed Soroban contract address, repository URL, and build-hash / tag mapping for each cited version range (v1.0.0–v1.2.9 for the pre-P1 line; v1.2.7 for the pre-P2 line; v1.7.0 on 2026-04-19 for the multi-policy landing). A reader must be able to map postmortems to bytecode.
- [ ] **Acknowledgments human / AI attribution.** Fold the "anonymous reviewer whose hard critique shaped the paper's framing" line into the *Use of generative AI* paragraph, or retitle it to "an adversarial-review pass conducted via Claude on prior drafts." Currently the two sentences describe the same artifact and a reader will infer a human peer reviewer where there was none.

### 3.2 Should-fix (prose / accuracy polish)

- [ ] **§2 Poseidon arity.** Declare $H: \mathbb{F}_r^\ast \to \mathbb{F}_r$ (variadic) explicitly since the construction uses $H$ at arity 1 and arity 2–3.
- [ ] **§2 AGM + ROM cross-reference.** Move the §1 pointer "readers who want full BKSV-style simulation-extractability definitions should consult [9, 11, 12] directly" to the end of the *What the SNARK binds, informally* paragraph in §2 so the AGM + ROM mention and the reference pointer are adjacent.
- [ ] **§4.3 first sentence.** "An earlier version of Onym composed Poseidon inside the circuit with SHA-256 outside, chosen because…" — *chosen* has no clean antecedent. Rewrite to the v3-era form: "SHA-256 was chosen for the outer hash because Soroban exposes it as a native host function."
- [ ] **§5 measurement honesty.** Either produce a small tier × operation table for the 6–10M instruction range and the 8–14× R1CS reduction, or add one sentence explicitly labeling these as order-of-magnitude estimates rather than measurements.
- [ ] **Reference [13] Mafia attribution.** *"T. Melissaris et al., 'Private Delivery Services for Messaging Layer Security' (Mafia), EUROCRYPT 2025."* Double-check against the proceedings; earlier drafts had a speculative attribution. Verify author list, exact title, and proceedings year before any submission.
- [ ] **Reference completeness.** Journal venues may want a broader related-work table. §7 declines this deliberately; if the chosen venue pushes back, the minimal expansion is a one-paragraph comparison with Semaphore, Signal PGS, and Mafia only — do not re-insert the Railgun / AZTEC / Nocturne / Azeroth gesture that §7 rejects.

### 3.3 Nice-to-have (only if venue length permits)

- [ ] **§4.3 Protocol 25 CAP citation.** Currently says "Protocol 25's CAP-0075 (January 2026) adds native Poseidon and Poseidon2 host functions." Add a reference entry pointing at the CAP document.
- [ ] **Abstract temporal anchor.** Add "results in this article reflect pre-MPC deployment state" to the abstract — makes the Phase-2 condition unmissable even for a reader who stops at the abstract.

## 4. Venue-independent publishing checklist

### 4.1 Formatting & submission mechanics

- [ ] Select the venue's template (IEEE magazine, ACM sigconf, USENIX, Springer LNCS). Convert `onym.v4.md` from Markdown to LaTeX. Math already uses LaTeX-compatible syntax; prose does not.
- [ ] Verify page / word limit for the selected venue; expand or cut as needed *without* crossing the genre line.
- [ ] Author block: ORCID, affiliation, contact email. Author is independent / solo on this paper; declare accordingly.
- [ ] Copyright / license: check the venue's policy. Some require exclusive rights; some permit CC-BY. The paper is currently unlicensed in the repository — decide on a pre-submission license (suggest CC-BY 4.0) and add a LICENSE header.

### 4.2 Double-blind handling (Tier B venues only; Tier A magazines are not double-blind)

- [ ] Remove author name from title block and PDF metadata.
- [ ] Replace the first-person *"We thank the Stellar Development Foundation…"* and *"the author is a builder, not a cryptographer"* phrasings with neutralized forms for the review draft. Restore in camera-ready.
- [ ] Replace explicit "@rinat-enikeev / PR #89" traces in any supplementary material.
- [ ] Replace the deployed contract address (once added per §3.1) with a placeholder for review; disclose in camera-ready.
- [ ] Rename repository references; e.g., do not cite the `rinat-enikeev/stellar-mls` GitHub URL in the submission — use a neutralized mirror or a DOI-anchored archive.

### 4.3 AI-use disclosure

- [ ] Check the chosen venue's AI-use policy at *the time of submission*, not at the time of writing. Policies at ACM, IEEE, USENIX, and IACR have all been revised at least once since 2024.
- [ ] The v4 Acknowledgments *Use of generative AI* paragraph is compatible with every current policy I am aware of as of 2026-04-21 (names the model, names the roles, retains author accountability). Re-verify per venue.
- [ ] If §3.1 is closed by folding acknowledgments into the AI paragraph, re-read that paragraph for truthfulness before submission.

### 4.4 Disclosure timeline for P1 and P2

- [ ] **P1 (operation-binding).** The vulnerability was fixed before the paper is published. Confirm: was the fix deployed on testnet *before* any mainnet-eligible exposure? If no party was relying on the vulnerable version for authorization at the time the paper becomes public, no coordinated-disclosure timeline is required. Document this in a one-line submission-time note, not in the paper.
- [ ] **P2 (Nostr long-lived-key co-membership).** Same question: was the fix deployed before any third-party deployment was exposed? Likely yes given the testnet-only posture.
- [ ] If either answer is "no," a coordinated-disclosure window (typically 90 days) should precede publication.

### 4.5 Artifact evaluation (if the venue offers it)

- [ ] If the venue has an artifact track (ACSAC does; DSN does; IEEE S&P Magazine does not), decide whether to submit the deployed contract + circuit repository. The reproducibility bar is satisfiable for circuit compilation and proof generation; the "deployed on testnet" claim is verifiable by re-running against the cited contract address.
- [ ] If submitting: prepare a minimal reproduction README — build the circuit, generate a proof, verify the proof against the deployed contract. Estimate: half a day of work assuming the build is already documented.

### 4.6 Cover letter (Tier A magazines want one; Tier B conferences sometimes)

- [ ] One paragraph: what the paper is (experience report), why this venue, what the two postmortems contribute to the audience, what the paper is explicitly not (proofs contribution, production-readiness claim, messaging substrate paper). Mirror §1 framing.
- [ ] If Tier A: name the author's background honestly — "builder, not a cryptographer" matches §Acknowledgments and is appropriate in a magazine cover letter.

### 4.7 Preprint posting

- [ ] Decide pre- or post-acceptance posting. Most Tier A / Tier B venues permit preprint posting before submission; check the specific policy. IACR ePrint is the natural home for a SNARK-related paper.
- [ ] If posting: include a note on the ePrint landing page indicating intended-venue status (e.g., "under review at IEEE S&P Magazine").

## 5. Suggested submission path

1. **Close §3.1 gaps** (artifact identifier, acknowledgments AI/human attribution). 1–2 hours.
2. **Close §3.2 prose items.** 1–2 hours.
3. **Verify reference [13] against proceedings.** 30 minutes.
4. **Pick template.** Recommend IEEE S&P Magazine first submission.
5. **Convert to LaTeX, expand §5 or §4.3 to fit target length.** 1 day.
6. **Post preprint to IACR ePrint** with "under review" note.
7. **Submit to IEEE S&P Magazine.**
8. **If rejected,** resubmit to ACM Queue (A2) with minimal reshaping, then ACSAC (B1) as third choice — but only if the paper can expand honestly without crossing into formal-proofs territory.

## 6. Do-not-do list

- Do not expand §2 into full SE proofs to meet a formal-venue reviewer's ask. The paper declined that scope deliberately and the decline is honest; re-expanding would invalidate the §1 "what this paper is not" frame.
- Do not add a measurement table by back-filling numbers. If the 6–10M range is an estimate, label it as such; fake precision is worse than labeled imprecision.
- Do not remove the Phase-2 single-contributor caveat from the abstract under copy-editing pressure. It is the load-bearing caveat that makes every formal claim in the paper honest.
- Do not quietly soften the $n = 2$ pair-chat statement. It is what separates Onym from a 1:1 substrate and the paper's integrity depends on being direct about it.
- Do not submit to EUROCRYPT / CRYPTO / IEEE S&P Oakland / USENIX Security / CCS / NDSS. Wrong genre, certain rejection, wastes 4–6 months per venue in queue.
