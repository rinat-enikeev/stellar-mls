# Revision Notes — How the Self-Critique Was Addressed

Each critique item from `02-critique.md` is listed below with a one-paragraph note on how the revised draft (`03-revised.md`) addresses it. Items are tagged with the critique IDs (M = Major, m = Moderate, n = Minor).

## Major

- **M1 — Over-claimed novelty / no engagement with prior Groth16 malleability work.** Sec. 5 now contains a paragraph that names the gap as a known Groth16 *public-input malleability* / *operation-binding* pattern, points out that Tornado Cash and Semaphore handle the analogous problem by binding the contract-controlled value as a public input (i.e., the same fix we eventually adopted), and adds a citation to Lipmaa's simulation-extractable SNARK survey [15]. The framing of the lesson is now "this class of bug is well-known; what is worth reporting is that it survived a reasonable review process, and *why*."

- **M2 — Performance numbers without methodology.** Sec. 7 now opens with a "Benchmark methodology" paragraph that states devices (Pixel 8 Pro on Android 14, iPhone 15 Pro on iOS 17), iteration count and warmup discipline (median of 50, 10 warmup discarded), what is included in the timed region (witness gen, MSM, serialisation), what is precomputed (salt, Merkle openings), and where Soroban verification numbers come from (testnet simulation traces). All point estimates are replaced with a structured table.

- **M3 — Missing before/after wall-clock comparison.** Sec. 7 explicitly addresses this by stating that Variant A wall-clock numbers are not reported because Variant A failed to complete reliably on the iPhone 15 Pro under multi-app memory pressure, and that we do not consider unreliable measurements honest enough to publish. The cross-variant comparison is therefore made on R1CS constraint count alone (a hardware-independent measure). This also addresses **n19**.

- **M4 — Sec. 5 conflated two distinct fixes (binding vs. splitting).** Sec. 5 now contains a "Design alternatives we considered" paragraph that explicitly considers and rejects the single-circuit + boolean-gated public-input variant, with two concrete reasons (boolean gating is a footgun; read-only call sites would carry a witness for an operation they don't perform). The trade-off (six setup ceremonies instead of three) is also disclosed.

- **M5 — Threat model not tied to defences.** A new Table 1 in Sec. 2 maps each of six security games (the original five plus the previously-implicit fee-payer unlinkability game) to the specific design element that discharges it. Subsequent sections refer back to the games where appropriate.

- **M6 — Transport-layer incident felt bolted-on.** The article's title and abstract now commit to a "Two incidents and what they taught us" framing. Sec. 6 has been re-titled "A third incident: transport-layer co-membership leakage" and includes an explicit closing paragraph that ties Sec. 5 and Sec. 6 to the same root failure mode ("a security invariant claimed in one document was not actually enforced by the construction in another"). The incident is no longer announced as the second of two.

## Moderate

- **m7 — No engagement with RFC 6973.** Sec. 6 now cites RFC 6973 §6.3 [13] explicitly and frames the spec drift as a missing application of an existing IETF best practice rather than a discovery.

- **m8 — Sec. 5 closing too abstract.** The closing of Sec. 5 has been replaced with a five-item numbered checklist (write the operational predicate; enumerate every value the contract reads and its IC status; read the relation file and the contract entry point side-by-side; reject related mitigations as binding answers; check that your soundness theorem mentions the right relation).

- **m9 — Related work too brief.** Sec. 9 has been rewritten around a comparison table (Table 4) that contrasts Onym with Tornado Cash, Semaphore, and Zcash on five axes (anchoring chain, proof system, membership scope, operation-binding mechanism, mobile prover). Aztec is discussed in prose as the closest non-Groth16 deployed system, with the reason we did not adopt PLONK.

- **m10 — 14× headline number is for the small tier only.** The abstract now reads "8.5× to 14× depending on tier." Sec. 4 explains why the reduction shrinks at higher tiers (constant SHA-256 contribution vs. logarithmic Merkle contribution) and tells a reader implementing at the large tier to expect 8.5×, not 14×. The conclusion uses the range form.

- **m11 — Relayer-address-on-envelope subtlety.** Sec. 3's "Fee decoupling" paragraph now contains an explicit sentence: "the relayer's Stellar account *does* appear on the transaction envelope and *is* persisted in Stellar's history. The privacy guarantee is not that no address ever appears — it is that the address that appears is the relayer's, which is unrelated to the prover's identity."

- **m12 — Missing figure.** Figure 1 has been added in Sec. 3 with a descriptive caption. (The figure is described in text rather than embedded as an image; the camera-ready submission would replace this with an actual diagram drawn in PowerPoint or TikZ as IEEE Computer Society templates require.)

- **m13 — References uneven.** [3] now cites the canonical NIP-01 specification rather than a project homepage; [5] now cites a peer-reviewed CCS analysis of Tornado Cash rather than the contested whitepaper; [10] now cites the original Barreto-Lynn-Scott family construction paper rather than a blog post; the bare-URL entries are downgraded to `[Online]. Available:` form per IEEE Xplore style.

## Minor

- **n14 — Word count.** Revised draft is 6,438 words including all front-matter, table contents, and references. The body (Sec. 1–10) is approximately 5,900 words, within typical IEEE S&P magazine feature-article length.

- **n15 — "Checklist-shaped" too informal.** Replaced. The conclusion now closes on a sentence that names the two practices in operational terms.

- **n16 — Acknowledgements vague.** The "anonymous reviewers" bullet has been removed; only the substantive technical acknowledgements remain.

- **n17 — Precision of millisecond figures.** Table 3 now reports median *and* 95th percentile for every measurement, on two distinct devices, with the methodology paragraph stating the iteration count and discipline.

- **n18 — URL-only references.** Bare URLs are now formatted as `[Online]. Available: <URL>` with a structured prefix per IEEE style.

- **n19 — See M3 above.** The constraint-count claim and the wall-clock claim are now kept rigorously separate.

- **n20 — Abstract closes on a maxim, not a result.** The abstract's final sentence now states the deployment status, the contract ID, and the rollout gate (Phase-2 MPC + external audit), making the abstract land on a verifiable engineering result rather than an aphorism.

## Items not addressed and why

None of the critique items are deferred. Two are partially addressed in ways worth flagging:

- **m12 (figure):** The article describes Figure 1 in a textual caption; producing the actual rendered diagram is left for the camera-ready submission step, when an IEEE Computer Society LaTeX or Word template is available. The text is structured so that swapping in a real image does not require any prose changes.
- **n14 (word count):** The revised draft is at the upper end of typical magazine length. A camera-ready pass would likely tighten 200–400 words from Sec. 7 and Sec. 8 by collapsing the rolling-history-window paragraph and the post-compromise-forward-secrecy paragraph into a single sentence each, leaving the technical core (Sec. 4–6) intact.
