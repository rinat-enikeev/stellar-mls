# Steps to Publish — IEEE Security & Privacy Magazine

Target venue: **IEEE Security & Privacy** magazine, feature article.
Submission system: ScholarOne Manuscripts (`https://mc.manuscriptcentral.com/cs-ieee`).

---

## Pre-submission preparation

- [x] Select venue and confirm fit (IEEE S&P magazine, feature article)
- [x] Draft initial plan with thesis, contributions, and outline (`docs/paper/00-plan.md`)
- [x] Write first draft (`docs/paper/01-draft.md`)
- [x] Self-critique against venue expectations (`docs/paper/02-critique.md`)
- [x] Revise draft addressing all critique items (`docs/paper/03-revised.md`)
- [x] Research alternative and complementary venues (`docs/paper/05-venue-strategy.md`)
- [x] Conduct adversarial external critique (`docs/paper/06-hard-critique.md`)
- [x] From-scratch rewrite addressing adversarial critique (`docs/paper/07-rewrite.md`)
- [x] Second hard critique pass (`docs/paper/08-hard-critique.md`)
- [x] Revision addressing second critique (`docs/paper/08-rewrite.md`)
- [x] Scope article to Anarchy governance type after PR #77 schema merge (`docs/paper/09-rewrite.md`)
- [x] Produce final draft (`final-draft.md`)

## Formatting and compliance

- [ ] Trim references to **15 or fewer** (current draft has 19; IEEE S&P limit is 15)
- [ ] Trim body text to **~5,000 effective words** (current draft is ~5,800; accounting for tables/figures the effective count may exceed the limit)
- [ ] Ensure **maximum 4 figures/tables** (current draft has 4 tables — Table 1, 2, 3, 4 — which is at the limit; verify no additional figures are needed)
- [ ] Convert all markdown tables and math to IEEE CS magazine LaTeX format using the `IEEEcsmag` class
- [ ] Verify all display equations are numbered consecutively and flush left
- [ ] Expand all acronyms at first use in both abstract and body (MLS, SNARK, R1CS, ABI, IC, MPC, etc.)
- [ ] Use American English throughout with serial commas
- [ ] Run the **IEEE LaTeX Analyzer** on the final `.tex` file before submission

## Replace projected data with empirical measurements

- [ ] Run on-device benchmarks for Table 3 (Pixel 8 Pro + iPhone 15 Pro, median of 50 runs, 10 warmup discarded)
- [ ] Replace "projected" prover-time figures with measured data
- [ ] Remove the "SOURCE DISCLOSURE" and "Note" disclaimers from the evaluation section

## Author information

- [ ] Prepare author biography (single paragraph, 5-6 sentences: role, institution, degrees, research interests, IEEE membership status)
- [ ] Register or confirm ORCID identifier
- [ ] Prepare author photo (minimum 300 dpi)
- [ ] Designate corresponding-author email

## Trusted-setup prerequisite (P4 — gates mainnet, not submission)

- [ ] Run Phase-2 MPC ceremony with at least 5 independent contributors
- [ ] Each contributor publicly destroys their contribution secret
- [ ] Scoped external audit of $R_{\mathsf{Upd}}^{\mathsf{Anarchy}}$
- [ ] Post companion ePrint document with full security-game reductions

## Pre-submission final checks

- [ ] Final read-through against the IEEE Computer Society style guide
- [ ] Produce rendered figures in original editable format (PDF preferred, minimum 300 dpi)
- [ ] Verify contract ID and deployment status are current at time of submission
- [ ] Verify testnet is live and accessible for any reviewer who checks

## Submission

- [ ] Upload manuscript to ScholarOne Manuscripts (`https://mc.manuscriptcentral.com/cs-ieee`)
- [ ] Complete IEEE Computer Society clearance via ScholarOne clearance system
- [ ] Confirm submission received and assign manuscript number

## Parallel tracks (from venue strategy, non-conflicting)

- [ ] Post preprint to `eprint.iacr.org` and `arxiv.org/cs.CR`
- [ ] Email CACM Practice section chair with one-page pitch
- [ ] Submit 1-page abstract to HotPETs 2026 (deadline May 11, 2026)
- [ ] Submit to AFT 2026 or PoPETs 2027.1 (deadlines May 27/31, 2026)
- [ ] File two CFRG Internet-Drafts (rolling; target IETF 126 Madrid, Jul 2026)
- [ ] Submit RWC 2027 contributed talk (deadline ~Sep-Oct 2026)
