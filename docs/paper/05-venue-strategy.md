# Venue Comparison and Submission Strategy

**Date:** 2026-04-19. Today is the cutoff used for "is this deadline open?" judgments below.

This document complements the IEEE S&P magazine draft in `03-revised.md`. After researching three tiers of venues — peer-reviewed magazines, top-tier security/crypto conferences, and practitioner/applied venues — we conclude that **the highest expected-value plan is not to pick one venue but to amortize one body of work across a coordinated package** of complementary outlets that have explicit no-conflict rules with each other.

---

## 1. Comparison matrix

Compact view of the venues researched. Columns: word/page budget, peer review style, audience, fit for "deployed ZK system + lessons learned" content, next actionable deadline (from today, 2026-04-19), and the dominant risk that could cause rejection.

### Magazines

| Venue | Length | Review | Audience | Fit | Next deadline | Risk |
|---|---|---|---|---|---|---|
| **IEEE Security & Privacy magazine** | ~5,000 effective words, ≤15 refs | Single-blind, 3 reviewers | Mixed: practitioners + researchers | **High** — long track record of "deployed crypto + lessons" features | Rolling | "Too narrow / too technical for broad S&P readership" |
| **CACM Practice** | ≤10 single-col pages (~6,000 words), ≤40 refs | Editorially curated peer review | Broadest of all venues | **Highest among magazines** — section explicitly wants applied systems with lessons learned; recent ZK/blockchain features published | Rolling (email pitch first) | Editor declines the pitch (low-cost rejection) |
| **ACM Queue** | ~3,500 words, ≤15 refs | Invitation-only, light peer review | Senior software engineers | High in topic, mismatch in length | Rolling (pitch first) | Invitation gating |
| **USENIX ;login: Online** | ~3,000 words | Editorial + light peer review | SREs, ops, security practitioners | Medium — fits postmortem half | Rolling | Length forces split |
| **IEEE Computer / Software / Internet Computing** | ≤4,200–7,200 words | Single-blind | Broad CS / SE / Internet | Lukewarm — not the right reader | Rolling | "Wrong magazine" |
| **IACR Communications in Cryptology (CiC)** | No page limit on accepted | Double-blind | Cryptology researchers | Strong topic fit but **requires research-paper voice**, not magazine | **Apr 27, 2026** (next quarterly cycle); then Jul 27, Oct 26 | Reviewers will want formal threat model and proofs; magazine voice will be re-shaped |

### Top-tier conferences (require expansion to 12–13 page double-column)

| Venue | Length | Review | Audience | Fit | Next deadline | Risk |
|---|---|---|---|---|---|---|
| **PoPETs / PETS** | 12 pages body | Double-blind, journal-style with Revise track | Privacy researchers + builders | **Best big-venue fit** — CFP language ("real privacy applications that run in real systems") almost matches the draft verbatim | **May 31, 2026** (issue 2027.1) | Privacy framing must be central, not a side-effect |
| **NDSS '27 Summer** | 13 pages | Double-blind, two-round + Major Revision | Systems-security researchers | **Most systems-friendly of "big four"** | **May 6, 2026** (~17 days — tight) | Need to argue why blockchain anchor is load-bearing |
| **USENIX Security '27 Cycle 1** | 13 pages | Double-blind, with Major Revision track | Broadest big-four security audience | High — welcomes "Analysis of deployed cryptography and cryptographic protocols" | **~Aug 2026** | Need a research argument beyond engineering |
| **ACM CCS '26 Cycle 2** | 12 pages | Double-blind | Big-four security | Mixed — most novelty-bar-strict for crypto | Apr 29, 2026 (~10 days, **abstract Apr 22**) | "Engineering not research" reject likely without a quantitatively new design tradeoff |
| **IEEE S&P symposium '27** | 13 + 5 pages | Double-blind, **binary accept/reject** | Big-four security | Mixed — heavy novelty culture | ~Jun 2026 | Binary outcome punishes "good system, modest research delta" |
| **Financial Cryptography '27** | 15 + refs | Double-blind | Blockchain + applied crypto | Natural topical home | ~Sep 2026 | LNCS paywall, lower visibility |
| **ESORICS** | 16 + 4 pages | Single-blind (not anonymous) | Broad EU security | Reasonable | Spring '26 closed; **'27 winter (~Jan 2027)** | LNCS paywall |
| **AsiaCCS '27** | 12 pages | Double-blind | Asian security community | **Avoid** — CFP explicitly discourages "primarily blockchain" | Mid-2026 | Topical disqualification risk |

### Practitioner / talk / standards venues

| Venue | Format | Review | Audience | Fit | Next deadline | Notes |
|---|---|---|---|---|---|---|
| **RWC 2027 (IACR Real World Crypto)** | Talk + optional 10pp paper | Steering-committee curation | ~600 industrial cryptographers | **Highest-leverage applied venue** — exact match | ~Sep–Oct 2026 | Non-anonymous; no proceedings; archival via eprint |
| **HotPETs 2026** | 10-min talk + 1-page abstract | Curated | Privacy researchers + activists | High; loves engineering postmortems | **May 11, 2026** (~22 days) | Travel stipend; co-located with PETS in Calgary |
| **ZKProof 9** | Community proposal / SoK | Community review | Applied-ZK researchers, standards people | Very high — Poseidon Merkle + circuit-shrinking + malleability postmortem are core ZKProof material | Late 2026 / early 2027 | ZKProof 8 already closed |
| **ACM AFT 2026** | Up to 20 pages, archival | Double-blind | Blockchain research | Strong — natural home | **Abstract May 20, paper May 27, 2026** | LIPIcs proceedings, open-access |
| **ACM DLT (journal)** | No hard limit | Peer-reviewed | DLT researchers + practitioners | **Explicitly welcomes "real-world deployment and evaluation"** | Rolling; one SI Feb 28, 2026 closed | Now fully open access since Jan 2026 |
| **IRTF CFRG Internet-Draft** | xml2rfc, no length limit | None (rolling drafts); RG adoption is the gate | Standards engineers | Very high for the cryptographic-construction story | Rolling; **IETF 126 Madrid Jul 2026** is the natural presentation slot | Adopt-by-CFRG is its own multi-year currency |
| **eprint.iacr.org** | PDF | None (scope check) | Cryptographers worldwide | Universal — preprint-of-record for crypto | Rolling | Required for RWC pitch credibility |
| **arXiv cs.CR** | PDF | None (endorsement) | Broader CS audience | Universal — preprint-of-record for systems | Rolling | Useful for non-crypto discoverability |
| **zkSummit 15** | Curated talk | Editorial | ZK builders + founders | High social/network value | TBA (post-zkSummit 14 in May 2026) | Recruiting and ecosystem visibility |
| **SBC 2027** | Talk only | Light review | Top blockchain research | High | Mar 2027 (SBC 2026 closed Mar 13) | Stanford CBR / IC3 audience |
| **IEEE ICBC 2027** | 4–8 pages or demo | Double-blind | IEEE ComSoc blockchain | Good for demo track | Jan 2027 (ICBC 2026 closed) | Solid second-tier blockchain venue |

---

## 2. The placement decision: package, not pick

The traditional "pick one venue and submit" strategy is wrong for this paper for two reasons.

**First, the paper is multi-modal.** The 6,000-word draft contains (a) a deployed-system narrative, (b) two engineering postmortems with measurable lessons, (c) measurement data on iOS and Android, (d) a cryptographic-construction description, and (e) an open standards artefact (an SEP). No single venue exercises all five. RWC and Queue want the postmortem and deployment voice; PETS and AFT want the measurement and threat-model rigour; CFRG wants the cryptographic construction in standards form; CACM Practice wants the broad-audience lessons-learned framing. Splitting the modes across venues *that explicitly permit overlap* converts what is one paper into four credit-carrying artefacts — at low marginal cost, because the content already exists.

**Second, the relevant venues mostly do not conflict.** RWC, ZKProof, HotPETs, zkSummit, SBC, CFRG drafts, and eprint/arXiv preprints all explicitly accept content that has been published, presented, or is under submission elsewhere. AFT, PoPETs, NDSS, USENIX, and CACM forbid simultaneous submission to *other archival peer-reviewed venues* — a constraint that admits exactly one "main" archival paper at a time, but does not block any of the talk/preprint/standards tracks running in parallel.

The plan below uses these rules to maximise expected acceptance count and reach without breaking any venue's submission policy.

---

## 3. Recommended coordinated submission package

The package is organised around **one archival main paper** at any given time, **one preprint of record**, **one or two talk/standards artefacts in flight**, and **one magazine version** for broad audience reach. Specific slots and dates below.

### Phase 0 — This week (Apr 19–26, 2026)

1. **Post the eprint and the arXiv preprint.** Convert `03-revised.md` to a formal-paper LaTeX template (USENIX-style is fine; IEEE Computer Society 2-column also acceptable), expand to ~12 pages by adding a proper "Threat model and security games" section (you already have the table; expand each game to a proof-sketch paragraph) and a "Measurement methodology" appendix that supports Table 3. Submit to:
   - `eprint.iacr.org` — the canonical preprint venue for cryptographers; required for credibility at RWC and ZKProof.
   - `arxiv.org/cs.CR` — broader CS systems discoverability; useful for MLS/Nostr/Stellar communities.

   *Why this week:* every other deadline in this plan benefits from being able to point at a stable PDF.

2. **Email Terence Kelly (CACM Practice section chair) with a one-page outline.** This is a near-zero-cost pitch; if rejected, no work lost; if accepted, you get editorial guidance for free. CACM Practice fits the 6,000-word draft natively, has no APC, and reaches by far the broadest audience. Recommended over IEEE S&P magazine as the *primary* magazine target *because* CACM accepts the existing length without cuts.

### Phase 1 — May 2026 (one tight target + two rolling tracks)

3. **Submit to HotPETs 2026 (deadline May 11, 2026, ~22 days).** Lowest-cost peer-reviewed talk slot in the package. Requires only a 1-page abstract + 2-paragraph fit statement. Co-located with PoPETs in Calgary, audience overlap with the privacy community you most want to reach with the postmortem half of the story. Use the malleability gap as the hook ("a Groth16 proof that authorised the wrong write — and survived four audits").

4. **Submit to AFT 2026 (abstract May 20, paper May 27, ~38 days).** This is the recommended **archival** main-paper target for this cycle. Reasons:
   - Topical fit is natural (Stellar = blockchain, ZK group registry = AFT core).
   - 20-page LIPIcs format gives room to do the security analysis properly.
   - Notification mid-July, which lands well before RWC submission window.
   - LIPIcs is open access; a citable, archival result by mid-2026.
   - Avoids the binary-accept/reject of S&P symposium and the novelty-bar pressure of CCS.

5. **Submit to PoPETs 2027.1 (deadline May 31, 2026, ~42 days) — *or* defer to PoPETs 2027.2 (Aug 31).** PoPETs is the highest-fit big-venue option but cannot run in parallel with AFT (both archival peer review). Decision rule:
   - If the paper expansion is on track for AFT by ~May 22, submit to **AFT** (better topical fit + more paper-writing time).
   - If AFT slips, drop AFT and pivot to **PoPETs 2027.1** by May 31 (similarly archival, also open access, journal-style Revise track is more forgiving for a paper that needs polish).

   The two are in the same archival slot and you submit to *one* of them, not both.

6. **File a CFRG Internet-Draft (rolling).** Two short drafts:
   - `draft-<author>-cfrg-zk-group-membership-binding` covering the Poseidon-Merkle commitment and Groth16 binding (the construction).
   - `draft-<author>-cfrg-zk-public-input-binding-considerations` covering the malleability postmortem as informational guidance.

   Aim to present both at **IETF 126 Madrid (Jul 27 – Aug 1, 2026)** during the CFRG session. Drafts can be filed any time before the IETF cutoff; the gating step is requesting agenda time on the CFRG mailing list ~3 weeks before the meeting.

### Phase 2 — Jul–Sep 2026 (decisions and follow-on submissions)

7. **AFT or PoPETs decision arrives.** If accepted, the paper is archival and citable; cite it in everything downstream. If rejected with usable reviewer comments, revise and submit to the *other* of {AFT, PoPETs} for the next cycle (PoPETs 2027.2 deadline Aug 31, 2026).

8. **Submit RWC 2027 contributed talk (deadline ~Sep–Oct 2026).** With an eprint preprint and (likely) an accepted AFT or PoPETs paper to point at, the RWC pitch becomes: *"Talk on a deployed Stellar-anchored ZK group registry; archival paper available at AFT/PoPETs; both engineering postmortems detailed in eprint preprint."* This is exactly the talk RWC selects for. Non-conflicting with all archival submissions.

9. **Submit USENIX Security '27 Cycle 1 (deadline ~Aug 2026)** *only if* AFT and PoPETs both rejected. USENIX is the highest-prestige stretch target but the longest queue (decision into late 2026) and the highest novelty bar; use it as a fallback rather than a parallel submission.

### Phase 3 — Q4 2026 (consolidation and breadth)

10. **Submit the magazine version.** By now you have:
    - An archival paper (AFT or PoPETs).
    - A preprint (eprint + arXiv).
    - A standards artefact in CFRG.
    - An RWC talk in process.

    Submit the 6,000-word magazine version to **CACM Practice** (preferred) or **IEEE S&P magazine** (`03-revised.md` is already drafted). The peer-review-style magazine accepts work that summarises archival research; this is the broad-audience reach component.

11. **Submit to ACM DLT journal.** A revised, longer version (the journal explicitly welcomes deployment + evaluation papers, has no hard length limit, and is now fully open access). This is the journal-of-record version that an industry adopter or a regulator would cite.

12. **Submit to ZKProof 9 (CFP late 2026 / early 2027) as an SoK.** "SoK: Engineering Lessons from a Deployed Groth16 Anonymous Group Registry." Includes the Variant A→B trick, the malleability gap, and standards recommendations. ZKProof's role is community guidance; this becomes the canonical reference document for ZK practitioners building anything similar.

---

## 4. Concrete actions to increase acceptance probability

Independent of venue, the following improvements measurably increase acceptance odds for any of the targets above. They are ranked by leverage:

1. **Add a measured Variant A wall-clock number — even if it's "did not complete in N seconds with N% probability."** Reviewers consistently push back on missing baselines; an honest "we tried, it failed" is far better than "we don't report A because B replaced it." This is the single most-cited weakness in the self-critique (M3).

2. **Write the security-games section as proof sketches, not table entries.** Table 1 in the magazine version maps games to design elements but elides the proof. For any conference target, the table needs to expand into one paragraph per game (≈6 paragraphs, ~1,000 words). This is the difference between "engineering paper" (rejected) and "engineering paper with security analysis" (accepted) at PoPETs and NDSS.

3. **Make the artefact submission first-class.** The repo is already open source. Package it as an artefact submission with a `REPRODUCE.md` (one shell script that builds a circuit, generates a proof, verifies it on testnet, and prints the constraint count). USENIX/PoPETs/NDSS all have artefact-evaluation tracks that reward Reproduced/Functional badges, and a Distinguished Artefact is a real option for Onym given the open-source breadth.

4. **Argue why the blockchain anchor is load-bearing, not decorative.** This is the single most common reviewer objection at NDSS and PoPETs for blockchain-flavoured papers. Add a paragraph to the introduction comparing Onym to a Trusted-Delivery-Service approach: what does the blockchain provide that a trusted server doesn't? (Public auditability of group-state ordering; tamper-evident commitment history; permissionless anchoring.) Frame it so a reviewer can copy the answer into their review.

5. **Cite the Groth16 malleability literature explicitly.** The revised draft cites Lipmaa [15] for simulation extractability; reviewers will additionally expect citations to (a) Bowe & Gabizon's "On the Malleability of Pairing-based Proofs," (b) Tornado Cash's withdrawal-address-as-public-input pattern (cite Wang/Tang/Meiklejohn), (c) the Snark-Friendly hash function literature (Poseidon, Rescue, GMiMC). Doing this preempts the "you didn't engage with prior work" reject reason.

6. **Pre-register the threat model.** State, in one sentence each, the six security games and what an adversary controls in each. Reviewers reject papers whose threat model emerges in the analysis; reviewers accept papers whose threat model is defined upfront and whose analysis discharges the named games one by one.

7. **For the RWC talk: rehearse the malleability postmortem as 25 minutes.** RWC selection weights "story quality" and "speaker quality" almost as heavily as topic. The malleability gap is a great talk because it has a discoverable beat ("a code-review question we couldn't answer") and a fixable resolution. Practice the talk before submitting; RWC accepts based on the abstract + speaker + impact.

8. **For CFRG: keep the two drafts short and informational.** CFRG is suspicious of drafts that try to standardise a complete construction on first contact. Position both drafts as "Informational" status; aim for "discuss at IETF 126" rather than "adopt by IETF 126." Adoption is a multi-year process; the goal at this stage is RG familiarity and feedback.

9. **Address the m6 critique by aligning structure across all variants.** The magazine version unified Sec. 5 and Sec. 6 under "two incidents." The conference version should keep that structure: one section per incident, parallel format ("trigger / mechanism / fix / generalisable lesson"). Reviewers reward parallel structure; they punish "and-here's-another-thing."

10. **Time the CFRG draft to land before the AFT submission.** A draft posted on the IETF datatracker becomes a citable artefact (an `I-D` reference) the moment it is up. AFT reviewers familiar with the CFRG ecosystem will read "draft-<you>-cfrg-…" as evidence of standards engagement, which raises the perceived impact of the work.

---

## 5. The decision tree

```
                Today (2026-04-19)
                       |
            +----------+----------+
            |                     |
    Phase 0 (this week)     Phase 1 (May)
            |                     |
   eprint + arXiv +         HotPETs (May 11)
   email CACM Practice      AFT or PoPETs (May 27/31)
                            CFRG drafts filed
                                  |
                          Phase 2 (Jul–Sep)
                                  |
                  +---------------+---------------+
                  |                               |
        AFT/PoPETs ACCEPTED              AFT/PoPETs REJECTED
                  |                               |
        Submit RWC + magazine           Pivot to USENIX Sec '27
        + DLT journal +                 OR PoPETs next cycle
        ZKProof 9 SoK                   + reuse all comments
                  |                               |
                  +---------------+---------------+
                                  |
                          Phase 3 (Q4 2026)
                                  |
                       Magazine + DLT + ZKProof 9
                       SoK; RWC talk delivered
```

---

## 6. Venues to skip and why

- **AsiaCCS '27** — explicit "not primarily blockchain" in the CFP; unnecessary review risk.
- **IEEE Network / IoT Magazine** — wrong audience, math-discouraged, expensive APC.
- **ESORICS '26 spring** — deadline two days out, infeasible.
- **CESC** — no 2026 edition, effectively replaced by SBC.
- **DINPS** — likely defunct (no 2026 edition).
- **TPMPC 2026** — already closed; topical mismatch (MPC, not ZK) anyway.

---

## 7. Why the existing IEEE S&P magazine draft is still the right starting point

Nothing in this plan recommends discarding the work in `03-revised.md`. The magazine voice, the two-incident structure, the security-games table, and the comparison with Tornado Cash / Semaphore / Zcash are all directly reusable across every target above:

- **AFT / PoPETs paper:** keeps the structure, expands each section with formal threat model, full security-games proof sketches, and a measurement methodology appendix.
- **CACM Practice:** uses the exact 6,000-word draft with a CACM-style intro reframe.
- **IEEE S&P magazine:** uses `03-revised.md` as-is.
- **RWC talk:** the talk version is "Sec. 4 + Sec. 5 + 8 minutes of measurement results."
- **ZKProof SoK:** the SoK is "Sec. 4 + Sec. 5 + a normative recommendations section."
- **CFRG drafts:** the two drafts are the formalisation of Sec. 4 and Sec. 5 respectively.

The 6,000-word magazine draft is, in effect, the spine of the package. The three-tier research above identifies which limbs to grow from it.
