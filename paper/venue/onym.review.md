---
title: "Review of paper/venue/ artifacts: sound, honest, submit-ready?"
author: "@gramyzer (on instruction from @rinat-enikeev, PR #89 comment 4286635697)"
date: 2026-04-21
scope: paper/venue/onym.md, paper/venue/onym.tex, paper/venue/onym.pdf
---

# Review of `paper/venue/` artifacts

Three artifacts under review: `onym.md` (157 lines, ~3,500 words), `onym.tex`
(LaTeX conversion, 255 lines, single-column article class), `onym.pdf`
(234 KB). Read against `onym.v4.md`, `onym.v4.review.md`, and
`onym.v4.venue-checklist.md`. The venue cut is a late polish pass on v4, not a
rewrite.

The three-part question — *sound, honest, good enough to submit?* — is answered
inline below, then summarized.

## 1. Is it sound?

**Yes, for the genre it declares.** Soundness here means two things: (a) the
formal scaffolding in §2 does not overstate what the SNARK binds, and (b) the
postmortems in §4 diagnose the right cause for the right bug.

- §2 states simulation-extractability (SE) informally, cites Groth16
  malleability [6], names the public-input-hashing modification of [9] that
  makes SE available, and is explicit about the AGM + ROM heuristic base. This
  is the minimum formality the postmortems need and the paper does not pretend
  to more.
- §4.1's P1 diagnosis — "a relation's soundness theorem says nothing about
  operations whose security predicate is over variables the relation does not
  mention" — is technically load-bearing and stated cleanly. The fix story
  (split the relation, make $C_{\mathsf{new}}$ a public input, then add
  public-input-hashing to promote KS to SE) matches the deployed contract
  code's trajectory.
- §4.2's P2 diagnosis is routine transport-layer hygiene stated as a
  generalizable antipattern. Correct.
- §4.3's demoted R1CS-cost lesson is the right shape: a specific mistake made
  moot by CAP-0075, with a durable takeaway (R1CS cost dominates host-function
  cost across the circuit boundary) retained.

**Notable delta from `onym.v4.md` that affects soundness.** The venue cut
replaces *"public inputs hashed into the Fiat-Shamir transcript per [9]"* with
*"public inputs hashed per the modification of [9]."* This is a correction, not
a polish: Groth16 is a non-interactive pairing-based SNARK with a structured
reference string; it does not have a Fiat-Shamir transcript. The v4 phrasing
would have been a technical tell to a crypto reviewer. The venue cut is more
accurate.

**Soundness gaps I did not find.** No claim in §2 or §4 appears to overstate.
The "fee-payer unlinkability" paragraph is carefully worded as an
information-theoretic statement about the on-chain footprint with all
side-channel loss attributed elsewhere — this is honest phrasing, not a
sleight-of-hand.

## 2. Is it honest?

**Yes, materially, on every caveat that matters for the genre.** The five
load-bearing honesty axes a hostile reviewer would probe first are all
surfaced where a reader will actually see them:

- **Trusted setup.** Single-contributor Phase-2 is in the abstract, restated in
  §2's *Deployment honesty* paragraph, and is the first bullet of §6. §5
  gates every post-deployment observation on "once an honest multi-contributor
  Phase-2 MPC has been run (a deployment prerequisite, currently unmet)" —
  tighter than v4's *"assuming an eventual honest Phase-2 MPC"*, which could
  have been misread as an assumption satisfied today.
- **Pair chats ($n=2$).** In the abstract, in §2, and in §6. Not buried.
- **Validator-set residual trust.** §3 and §6; the parenthetical argument
  (*"no ability to decide which transactions exist, bounded retention"*)
  carries the trust-base reasoning in compressed form.
- **MLS integration.** §6 states "sketch, not a demonstration" — withdrawn
  from any construction claim.
- **Author background and AI use.** The *Use of generative AI* paragraph is
  specific about what Claude did (literature tutor; framing interlocutor for
  §2 and §4; three named framing changes), preserves author accountability,
  and is separated from the human-contributor list.

**Notable honesty delta from `onym.v4.md`.** The "anonymous reviewer whose hard
critique shaped the paper's framing" line that prior reviews flagged twice
(Gap 1 in `onym.v4.review.md`) is **gone**. It has been replaced with *"the
external code reviewer whose P1-catching question is the reason P1 is a
postmortem rather than a zero-day."* That is concrete, attributes a specific
act to a specific (un-named but real) person, and no longer reads as a
human-peer-reviewer stand-in for Claude. **Gap 1 is closed.**

**One honesty item remains open.** §5's observer-learns paragraph is now
correctly gated on the unmet Phase-2 MPC precondition, but the 6–10M
instruction-count range and the 8–14× R1CS reduction in §4.3 are still quoted
without a "these are estimates, not measurements" label and without a
tier × operation table. The venue-checklist flagged this as a should-fix and
it is not done. Currently honest by omission (§5 already says "we do not
report production group counts or update volumes") but a one-sentence
"order-of-magnitude estimates" caveat would close the ambiguity. Not blocking
for submission; worth doing.

## 3. Is it good enough to submit?

**For the genre it declares — experience report, practitioner postmortems,
design-review template — yes, to Tier A practitioner venues (RWC talk, USENIX
;login:-successor, IEEE S&P Magazine, ACM Queue), after one blocker and a
short polish pass.** Not good enough, and correctly not shaped, for the
proofs-track or main-systems venues (EUROCRYPT, CRYPTO, S&P Oakland, USENIX
Security, CCS, NDSS); the paper says so itself in §1 and the venue-checklist
§2 restates the ruling correctly.

### 3.1 The one remaining blocker

**Gap 2 from `onym.v4.review.md` is still open.** §5 opens *"Testnet deployment
in alpha phase"* and never names the deployed contract address, repository
URL, or build-tag mapping. The §4 postmortems cite precise version ranges —
**v1.0.0–v1.2.9** for the pre-P1 line, **v1.2.7** for the pre-P2 line, **v1.7.0
on 2026-04-19** for the multi-policy landing — and a reader at RWC, HotPETs,
or any artifact-evaluation venue has no way to map those ranges to the actual
artifact the postmortems are about. One line in §5 closes this:

> Contract: `C<address>` on Stellar testnet. Source: `<repo URL>`. Postmortem
> tag ranges: P1 pre-fix at tags `v1.0.0`–`v1.2.9`; P2 pre-fix at tag
> `v1.2.7`; multi-policy landing at tag `v1.7.0` (2026-04-19).

The venue-checklist §3.1 lists this as the must-fix item that has not moved
across v4 → venue. This is the single submission-blocker.

### 3.2 Polish items (non-blocking)

- **Orphan references in `onym.tex`.** `\bibitem{zcash}` (ref [3], Hopwood et
  al. *Zcash Protocol Specification*) and `\bibitem{barbulescu2019}` (ref [8],
  Barbulescu–Duquesne, *Updating Key Size Estimations for Pairings*) are in
  the bibliography but never cited in the body of the LaTeX source. Either
  cite them where they are load-bearing (Zcash's Sapling circuit is a plausible
  §7-related-work mention for the commitment-tree design; Barbulescu–Duquesne
  backs a security-level claim that §2 does not actually make) or delete them.
  Most camera-ready pipelines reject unused `\bibitem` entries silently but a
  strict reviewer will notice.
- **CAP-0075 / Protocol 25 reference.** §4.3 and the acknowledgments both
  cite CAP-0075 as an obsoleting fact for the §4.3 postmortem; the reference
  list has no entry for it. Venue-checklist §3.3 flagged this as a
  nice-to-have. Given that §4.3 depends on CAP-0075 being a real shipped fact,
  promote it from nice-to-have to should-fix.
- **§2 Poseidon arity declaration.** §2 writes $H$ at arity 1 ($H(\mathsf{pk})$)
  and arity 2–3 ($H(\mathsf{root}, \epsilon)$, $H(\cdot, s)$) but only states
  "$H$ denote[s] Poseidon." A half-sentence "$H$ is used at arities 1–3 at the
  standard parameter set for each" closes the ambiguity.
- **§4.3 sentence-one grammar.** *"An earlier version of Onym composed
  Poseidon inside the circuit with SHA-256 outside, chosen because Soroban
  exposes SHA-256 as a native host function"* — *chosen* has no clean
  antecedent (it modifies neither *"an earlier version"* nor *"Poseidon"*).
  V3's phrasing *"SHA-256 was chosen for the outer hash because Soroban
  exposes it as a native host function"* is tighter.
- **§5 measurement-honesty label.** One sentence: *"Instruction-count and
  constraint-reduction ranges in this section are order-of-magnitude estimates
  from circuit compilation and testnet runs, not a measurement study."* Closes
  the honesty ambiguity flagged in §2 of this review.
- **Reference [13] Mafia.** *"T. Melissaris et al., 'Private Delivery Services
  for Messaging Layer Security' (Mafia), EUROCRYPT 2025."* The
  venue-checklist flagged this as needing verification against the
  proceedings. Verify before submission; an incorrect citation is a reviewer
  tell.
- **§1 cross-reference drift.** §1 now says *"full simulation-extractability
  definitions should consult [9, 11, 12] directly"* (BKSV-style removed — good)
  but the pointer location is still §1 rather than adjacent to the AGM+ROM
  mention in §2's *What the SNARK binds, informally* paragraph. Venue-checklist
  §3.2 flagged the move; still not done. Cosmetic.

### 3.3 What venue

The `onym.v4.venue-checklist.md` ranking stands: Tier A is IEEE S&P Magazine
(A1) first, ACM Queue / CACM Practice (A2) second. The `onym.v4.review.md`
alternate ranking — RWC short talk as primary, `;login:`-successor as the
written-artifact home, HotPETs as workshop fallback, IACR ePrint as
preprint — is equally defensible and depends on whether the author's
priority is reviewer depth on the SNARK substrate (RWC / ;login:) or reach into
general security-practitioner audiences (IEEE S&P Magazine / ACM Queue).
Either path is a genre-fit match and each venue is compatible with the paper's
declared scope.

The "venues to avoid" list in venue-checklist §2 is correct and complete;
submitting to EUROCRYPT / CRYPTO / S&P Oakland / USENIX Security / CCS / NDSS
would waste a 4–6 month queue slot per venue on a genre mismatch that §1
already anticipates.

## Bottom line

- **Sound?** Yes, for the genre. The venue cut corrects the one technical
  error in v4 (Fiat-Shamir phrasing) and tightens the Phase-2 MPC language
  into an unmet-precondition gate.
- **Honest?** Yes, materially. Gap 1 from `onym.v4.review.md` (ack / AI
  blur) is closed. The measurement-estimate labeling is a soft honesty item
  worth closing but not a deception.
- **Good enough to submit?** Yes *after* adding the contract address /
  repository URL / tag-range line to §5 (Gap 2, unchanged from v4 → venue).
  Not good enough without it — an RWC / HotPETs / artifact-evaluation reviewer
  will ask exactly this and the venue-checklist and two prior reviews agree
  on the point.

One line in §5, the `\bibitem` cleanup in the TeX source, and the CAP-0075
reference entry are the submission-readying delta. Nothing here requires new
technical work or re-opening design decisions. The paper now knows what it is,
says so up front, and carries the postmortems on their own weight — which is
the most important property for an experience report to have.
