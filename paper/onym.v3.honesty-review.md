---
title: "Honesty review of onym.v3.md"
author: "@releaseng (on instruction from @rinat-enikeev, PR #89 comment 4286420419)"
date: 2026-04-21
scope: paper/onym.v3.md
---

# Honesty review of `paper/onym.v3.md`

Assessed against the paper's own claims and self-disclosures. Not a technical review; purely an honesty pass — what is admitted, what is softened, what is still oversold.

## Where v3 is materially honest

These are the hardest things to admit, and the paper admits them where a reader will actually see them:

- **Single-contributor Phase-2 setup is in the abstract.** §5.3 "Deployment-state honesty" states outright that every game in the section is vacuous against the setup operator today, and that the games describe what *will* hold once Phase-2 is rerun — "and describe nothing else today." The abstract repeats it. This is the single most load-bearing caveat and v3 does not bury it.
- **Pair-chat ($n=2$) vacuity is promoted to the abstract and §3.** Readers are told not to use Onym as a 1:1 substrate before they read §5.3.
- **Validator-set residual trust is declared, not elided.** §2, §8, and the Table 1 row all say validators see submission timing; the trust-base column is honest.
- **MLS integration claim is explicitly withdrawn.** §8 writes out that "Earlier drafts claimed… That overstates what has been built."
- **§6.3 demotion is owned as a severity correction.** Not silent.
- **Poseidon-as-ROM, not PRF.** §5.3 includes the sentence "This does *not* treat Poseidon as a PRF."
- **Author's own background is disclosed.** §10: "The author is a builder, not a cryptographer. Onym was designed, implemented, and deployed before any of §5.3 existed on paper."
- **Inline §9 names are flagged as unpinned.** The trailing §10 paragraph tells readers not to use them as a lineage argument.

On the axes a hostile reviewer would probe first — trusted setup, pair-chats, validator trust, MLS, author credentials, and use of AI — v3 is honest, and the improvement over v2 is real.

## Where honesty is still incomplete

Four gaps remain, all one-or-two-sentence fixes:

### 1. Title vs. admitted observers

*"Metadata Without Observers"* is contradicted by the paper's own body. The paper documents three classes of observers: Stellar validators (§2, §8), the relayer (§4), and — until Phase-2 is rerun — the setup operator (§5.3). The subtitle ("Postmortems from a SNARK-Gated Group Registry on Stellar") is accurate about genre; the main title is a marketing phrase the substance does not support. A truthful retitle — e.g. *"Metadata Without an Application-Layer Observer"* — would align title with §2's actual claim.

### 2. Acknowledgments blur human/AI attribution

§10 is careful about AI disclosure: it lists the exact §5.3 framing choices Claude was interlocutor on (SE, AGM, Poseidon-as-RO, Phase-2 framing, R1CS separation). The preceding acknowledgments paragraph then credits the §5.3 rewrite to "the anonymous reviewer whose hard critique drove the §5.3 rewrite" — phrasing a reader will interpret as a human peer reviewer. The list of framing choices in §10 matches the §5.3 rewrite exactly. Either the acknowledgments line should fold into the AI-use paragraph, or it should describe the artifact truthfully (e.g., "an adversarial-review pass run on the v2 draft"). Today this is the one place v3's attribution is fuzzier than the rest of §10.

### 3. Genre framing arrives late

§10 declares that the article "is a builder's postmortems and design-review lessons, not a protocol proposal." The abstract tells a different story: "We state the construction formally in the algebraic group model…" A first-time reader forms a protocol-construction expectation from the abstract and only learns the genre disclaimer on line 201. One sentence in the abstract — "this is a postmortems-and-lessons article, not a construction contribution" — would set expectations honestly up front.

### 4. §2's auditability claim has no artifact identifier

§2 asserts that "the authorization rules are externally auditable from the public contract bytecode alone." No deployed Soroban contract address, repository URL, or build hash appears anywhere in v3 for a reader to actually perform that audit. An auditability claim without the artifact identifier is a claim without a witness. Earlier drafts named the contract (`CC6N…WWKE`); v3 dropped it. Either restore it or soften the §2 claim.

## Secondary points

- **§7 numbers are ranges, not measurements.** "6–10 million Soroban instructions" and "8–14× R1CS reduction" (§6.3) are quoted without a measurement table for the three tier sizes. Honest path: either produce a small table tying each range to a tier × operation cell, or label them as order-of-magnitude estimates.
- **"phase is alpha" is stated twice (abstract, §7)**, which is honest; the abstract could additionally say "results in this article reflect pre-MPC state" to make the temporal condition of §5.3 unmissable.

## Summary

The author is being honest about the things that matter most — trusted setup, pair chats, validator trust, MLS over-claim, their own non-cryptographer background, and the role of AI in writing §5.3. The remaining honesty gaps are (a) the title, (b) one acknowledgments sentence that blurs human/AI attribution, (c) the abstract's genre framing, and (d) the missing on-chain artifact id that §2 implicitly promises. None require re-doing work; all are small textual fixes.
