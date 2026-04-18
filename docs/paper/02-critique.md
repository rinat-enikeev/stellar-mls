# Self-Critique: First Draft of Onym Feature Article

**Reviewer role:** Imagined IEEE S&P magazine reviewer #2 — a security practitioner with cryptographic background and a low tolerance for marketing language. The author has not seen these comments.

**Recommendation:** Major revision.

The submission describes a real, deployed, open-source system in a topic area that S&P readers care about (privacy-preserving group state on a public blockchain), and the two engineering lessons are concrete and transferable. The writing is generally clear. However, the draft has a number of weaknesses that, taken together, would prevent acceptance in its current form. I list them in roughly decreasing order of importance.

---

## Major issues

### M1. The "lessons" are over-claimed and under-evidenced as generalisable.

The abstract and the closing both promise that the two design choices "dominated the system's outcome" and that the lessons "generalise." For lesson 1 (count R1CS cost first) this is plausibly true for any zk-SNARK-anchored system; for lesson 2 (write down what each proof authorises), the draft never engages with the obvious counter-argument that the bug it describes is well-known in the formal-methods and ZK literature as *malleability of public inputs* / *binding of the proof statement to the operation*. The Groth16 community has discussed this since 2017 (e.g., the GKMMM/CDS critiques of the original Groth16 paper, and Tornado Cash's own front-running mitigations). Calling it a generalisable lesson without citing prior work makes the paper read as if the field's collective knowledge starts with the authors' postmortem. **Action:** add a paragraph in Sec. 5 that situates the unbound-`new_commitment` gap within the existing literature on Groth16 malleability and public-input binding, and cite at least Bowe & Gabizon's "On the Malleability of Bitcoin Transactions" analogue for ZK proofs and the now-well-known mitigation pattern (binding the operation's full transition into the public inputs).

### M2. Performance numbers in Sec. 7 are presented without methodology.

The article says "on a 2024-class smartphone, generating a `R_Membership` proof for the small tier with Variant B takes roughly 200 ms." Which device? Which OS? Was the prover running cold or warm? Was the Merkle tree precomputed or built per call? Is the 200 ms wall clock or CPU time? Was multithreading used? The numbers are central to the practitioner appeal of the article, but as written they are not reproducible and not falsifiable. **Action:** add a short methodology paragraph and a bench-config table; or downgrade the numbers to "sub-second on commodity 2024 hardware" and remove the specific milliseconds. The first option is preferable for a magazine known for reproducibility.

### M3. The proving-time claim "300%–400% reduction" is implied but never measured.

The Variant A → Variant B transition is described qualitatively (small-tier "from multiple seconds and forced the prover to swap to disk" to "a fraction of a second") but no actual before/after times are given. This is a missed opportunity: the 14× constraint reduction is the headline number, but readers will want to know whether the constraint reduction translated linearly to wall-clock prover time. Often it does not (memory pressure, witness-generation cost, multi-exponentiation amortisation). **Action:** report measured Variant A and Variant B prover times side-by-side for the same device, or explicitly note that Variant A was never measured in production because it failed to complete on the target hardware.

### M4. Sec. 5 conflates two distinct fixes.

The fix described is presented as "split the relation into `R_Membership` and `R_Update`," but in practice the real change is *binding `C_new` as a public input*. Splitting is one way to achieve that binding (it keeps the read-only paths' circuits small); the alternative (add `C_new` as an optional public input to a single circuit, with conditional constraints) is briefly mentioned in the postmortem as a rejected design but not in the paper. The paper should explain *why* a separate circuit was chosen over a single conditional circuit. The omission makes the fix sound less considered than it actually was. **Action:** add a one-paragraph design-alternatives discussion to Sec. 5.

### M5. The threat model in Sec. 2 is asserted but never tied back to specific defences.

Section 2 lists five security games (soundness, ZK, hiding, epoch integrity, prover privacy) and then says "we have proved them in a companion soundness document." But the body of the paper never refers back to those games when describing the system. A reader cannot determine which design choice closes which game. **Action:** add a short table or in-line markers tying each design element to the game it discharges. At minimum: dual-hash commitment → hiding; Groth16 + tier setup → soundness; 192-byte proof + tier indistinguishability → prover privacy (constrained by tier upper bound); strict epoch monotonicity + `R_Update`'s `C_new` binding → epoch integrity; relayer pattern → fee-payer unlinkability (a *new* sixth game that should be named in Sec. 2 but currently isn't).

### M6. The transport-layer incident (Sec. 6) is interesting but feels bolted-on.

The first five sections build a coherent argument about cryptographic engineering. Then Sec. 6 pivots to a Nostr/secp256k1 transport-layer issue that is structurally different (it's about identity coupling at a transport layer the cryptographic core does not touch). Either the paper should commit to a "lessons from incidents" framing throughout — in which case Sec. 6 belongs alongside Sec. 5 under a unified "Incidents" supersection — or Sec. 6 should be cut and saved for a separate publication. As it stands, the article promises in its title and abstract to discuss "two design choices" and then gives a third lesson in Sec. 6 that is announced as the second of two. **Action:** unify Sec. 5 and Sec. 6 under a single "Two incidents and what they taught us" section, and adjust the abstract and title to match. (Alternative: cut Sec. 6 and keep the article tight; this would be the easier choice but loses material that practitioners will find valuable.)

---

## Moderate issues

### m7. The "spec drift" framing in Sec. 6 lacks engagement with how the field handles this elsewhere.

The IETF and the W3C both have explicit conventions for how a referenced spec's privacy claims must be normatively cross-checked when they affect a referencing spec's invariants (e.g., RFC 6973 on Privacy Considerations). The paper presents the Onym spec / Nostr NIP contradiction as something we discovered ourselves; in fact, RFC 6973 §6.3 essentially mandates the cross-check we failed to perform. **Action:** cite RFC 6973 and frame the failure as a missing application of an existing best practice.

### m8. The unbound-`new_commitment` discovery story leans heavily on the human framing.

"A code-review question changed our reading of it" is a nice narrative beat but does not help a reader know how to find the same class of bug in their own system. The paper would be stronger if it concluded Sec. 5 with a small concrete checklist (3–5 items) that an engineer could apply during a circuit/contract review. The current closing paragraph gestures at "what does this proof authorise" as a "deliverable" but is one level too abstract. **Action:** turn the closing of Sec. 5 into a numbered checklist.

### m9. Comparison with Semaphore is too brief.

Semaphore, Tornado Cash, and Onym all use the same essential construction (Merkle tree of commitments + Groth16 membership proof). The Related Work section in its current form does not differentiate the systems clearly enough for a reader to understand why Onym is interesting. Onym's distinguishing claims are: (a) general group-membership registry rather than single-use payment privacy; (b) tight coupling to MLS group-state transitions; (c) the dual-circuit architecture for state transitions. **Action:** rewrite Sec. 9 to be comparative rather than enumerative — ideally with a small table.

### m10. The 14× reduction headline number is for the small tier.

The reduction is 14× for the small tier, 10.5× for medium, and 8.5× for large. The article repeatedly quotes the 14× number without acknowledging that this is the *most favourable* tier, because SHA-256's contribution is constant and the Merkle path's contribution grows logarithmically. A reader who wants to reproduce Onym at the large tier will see less benefit. **Action:** quote a range ("8.5×–14× depending on tier") in the abstract and conclusion.

### m11. The claim that the contract "stores no committer address" is true but elides an operational subtlety.

The relayer's Stellar account *is* on the transaction envelope and *is* persisted in Stellar's history. The contract's storage doesn't have a committer field, but the Stellar transaction history does have a fee-source field. A naive reader might conclude "no address ever appears anywhere"; the truth is "no address appears in the contract storage; the relayer's address appears in the envelope." **Action:** clarify in Sec. 3 that the relayer's address *does* appear in the Stellar transaction envelope; the privacy guarantee is that the relayer's address is unrelated to the prover's identity.

### m12. The paper has no figure.

Section 3 says "Figure 1 sketches the data flow" but no figure is included. IEEE S&P expects 1–3 figures in a feature article. **Action:** add the data-flow figure or remove the sentence.

### m13. References are uneven.

[5] (Tornado Cash) is a whitepaper of contested provenance; for a magazine-quality submission, prefer the academic literature on Tornado Cash's design (e.g., the SoK on privacy mixers from 2022) or omit. [3] (Nostr) is cited only to a project homepage; an IEEE-style citation would prefer a standards-track or peer-reviewed source. [10] (BLS12-381 blog post) — same concern; cite the underlying Barreto-Lynn-Scott family paper. **Action:** harden the references; replace blog posts with peer-reviewed or standards citations where available, and add a one-sentence justification for any non-standard citation that remains.

---

## Minor issues

### n14. Word count.

Draft is 5,269 words; target is ~5,500. Adding the table for m9, the methodology for m2, the alternatives discussion for m4, the figure caption for m12, and the prior-work paragraph for M1 will likely push it to ~6,000 words. The IEEE S&P magazine generally accepts up to ~6,000 words for a feature article, but this should be confirmed at submission time.

### n15. The phrase "checklist-shaped" in the conclusion is cute but informal.

S&P readers tolerate informal phrasing but not in a closing sentence intended to land. **Action:** rewrite.

### n16. Acknowledgements are vague.

"Anonymous reviewers who pressed on the unbound-`new_commitment` question" is over-coy when the article itself is a postmortem of that exact question. Either name the reviewer (with permission) or remove this acknowledgement entirely.

### n17. "200 ms," "350 ms," "800 ms" — the precision is misleading.

These are presumably median-of-N benchmarks, but the article does not say so. If they are point measurements, they are noise; if they are medians, the variance should be reported. **Action:** report median and 95th percentile, or round to a single significant figure with a "roughly" qualifier and explain why precision is not warranted.

### n18. Footnote-style URLs in the references.

The reference list includes a bare URL for [3] and one for [4]. IEEE Xplore-formatted references want either a standards body identifier or a structured citation; bare URLs should appear in footnotes only.

### n19. The "Variant A was never measured in production" admission needed for M3 may also undercut the headline 14× claim.

If Variant A was not actually run on the target hardware, the claim that Variant B is "14× smaller" should be supported solely by R1CS constraint counts (which are independent of hardware), and the wall-clock claims should be made about Variant B alone. The article should be careful to keep these two threads separate so a reviewer cannot accuse the authors of hand-waving.

### n20. The abstract is one continuous paragraph and ends mid-claim.

The abstract closes with: "...cryptographic engineering in production is dominated by the cost of the wrong primitive in the right place, and 'what does this proof actually authorize?' must be a deliverable of every design review." This is a pair of claims, not a result. Magazine abstracts should end on a result-oriented sentence (what we built, what it costs, what we learned). **Action:** rewrite the last sentence of the abstract to land on a measurement or a deployment status, not a maxim.

---

## Suggested revision order

1. (M6) Decide Sec. 6's fate: unify under a single Incidents section, or cut. **Pick: unify.** This is the structural decision; everything else depends on it.
2. (M5) Add the security-game-to-design-element mapping table at the end of Sec. 2 or beginning of Sec. 3.
3. (M2, M3, n17) Add a benchmark methodology paragraph and a small table; replace point estimates with medians + 95th percentiles where possible.
4. (M4, m8) Rewrite the closing of Sec. 5 with a design-alternatives paragraph and a numbered checklist.
5. (M1, m7) Add prior-work citations to Sec. 5 (Groth16 malleability) and to Sec. 6 (RFC 6973).
6. (m9) Replace Sec. 9's prose with a comparison table.
7. (m11) Clarify the relayer-address-on-envelope subtlety in Sec. 3.
8. (m10, n19) Restate the 14× claim as a range; separate constraint claims from wall-clock claims.
9. (m12) Add Figure 1 (data flow).
10. (n15, n16, n20) Tighten abstract and conclusion; remove or sharpen acknowledgements.
11. (m13, n18) Harden references.

---

## Summary

The paper has the bones of a well-received S&P feature: a real system, deployed and open source; two concrete and unflinching engineering lessons; tractable performance numbers; and a tight focus. The major weaknesses are over-claimed novelty, missing benchmark methodology, a structurally jarring transport-layer section, and weak engagement with prior work on the exact class of vulnerability described in Sec. 5. All are fixable in one revision pass.
