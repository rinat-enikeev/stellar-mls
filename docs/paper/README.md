# Onym IEEE Security & Privacy Magazine Submission

This directory contains an end-to-end pass at producing a submission-ready feature article for IEEE Security & Privacy magazine. It exists as documentation: the venue selection, structural plan, first draft, self-critique, revised draft, and revision-notes file are all preserved so that the editorial provenance is auditable.

## Contents

| File | Purpose |
|------|---------|
| [`00-plan.md`](00-plan.md) | Venue selection (IEEE S&P magazine, feature article, ~5,500 words), thesis, contributions claimed, outline. |
| [`01-draft.md`](01-draft.md) | First draft (5,269 words). Pre-self-critique. |
| [`02-critique.md`](02-critique.md) | Reviewer-style self-critique against the first draft: 6 major, 7 moderate, 7 minor issues, with a suggested revision order. |
| [`03-revised.md`](03-revised.md) | Submission-ready revision (6,438 words including front matter and references). Addresses every critique item. |
| [`04-revision-notes.md`](04-revision-notes.md) | Item-by-item map from each critique point to the change that addresses it in the revised draft. |
| [`05-venue-strategy.md`](05-venue-strategy.md) | Venue comparison matrices and coordinated submission timeline. |
| [`06-hard-critique.md`](06-hard-critique.md) | Adversarial external critique targeting motivation, severity framing, and unresolved dependencies. |
| [`07-rewrite.md`](07-rewrite.md) | From-scratch rewrite addressing `06-hard-critique.md`. Reframes motivation around the metadata-observability problem for online communication; presents the solution both informally ("for non-cryptographers") and formally; expands the narrative to four postmortems, including the single-machine trusted setup as a named current limitation. Supersedes `03-revised.md` as the submission candidate. |

## Submission status

`07-rewrite.md` is the article intended for submission. The remaining steps before actual submission to IEEE S&P magazine are:

1. Re-typeset in the IEEE Computer Society magazine LaTeX or Word template (the file is currently markdown for review-readability).
2. Produce a rendered Figure 1 from the textual caption in Sec. 3.
3. Confirm the magazine's current word-count and reference limits — the revised draft is at the upper end of typical feature length.
4. Author byline, ORCID, biography, and corresponding-author email.
5. Final pre-submission read-through against the IEEE Computer Society style guide.

The technical content does not anticipate further revision. The contract ID, the deployment status, and the gating items for mainnet rollout (Phase-2 MPC trusted-setup ceremony and external audit of the new `R_Update` circuit) are accurate as of 2026-04-19 and would be re-verified at submission time.
