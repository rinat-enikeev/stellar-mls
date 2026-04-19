# Hard Critique of 07-rewrite.md

**Reviewer stance:** Adversarial but constructive. The rewrite addressed every structural complaint from 06-hard-critique.md: the motivation now leads with metadata as a real-world problem, the vulnerability is called a vulnerability, the trusted setup is foregrounded, the alternative-solutions space is explored, and the writing is tighter. This critique targets what remains weak after those fixes, and raises new issues the rewrite introduced.

**Recommendation:** Conditional accept with minor-to-moderate revision. The paper is now defensible as an IEEE S&P feature article. The remaining issues are fixable without a structural rewrite.

---

## Part I: Motivation and framing — much improved, still has gaps

### C1. The EncroChat/Sky ECC examples prove the value of metadata, not the need for Onym.

The introduction now opens with metadata-driven law-enforcement operations (EncroChat, Sky ECC) and the Hayden quote. These are effective at establishing that metadata matters. They do *not* establish that Onym's specific architecture — public-ledger anchoring with SNARKs — is a warranted response. EncroChat and Sky ECC were compromised through device infiltration and server compromise, not through a Delivery Service metadata leak. The implicit syllogism is: metadata is dangerous → therefore we need a system that hides metadata from *every* operator → therefore blockchain + SNARK. But the middle step is doing enormous load-bearing work and is not argued. A reader could equally conclude: metadata is dangerous → therefore use Signal, which already hides it from its own operator with anonymous credentials and has a decade of operational track record.

**What would fix this:** Either find an example where *the operator itself* was the metadata threat (a state actor compelling a DS operator to disclose group membership, a federated homeserver logging co-membership for intelligence), or explicitly concede that Onym's threat model is prospective, not retrospective — "no deployed DS has been publicly compromised for group-membership metadata *yet*, and we want an architecture that makes the question moot."

### C2. The four-candidate framing in Sec. 2 is better but still tilts the scale.

The rewrite now honestly lists transparency logs, federated delivery, anonymous-credential servers, and public-ledger anchoring as four candidates. This is a real improvement over the original. However, the treatment of candidate 3 (Signal PGS) ends with: "It is not externally auditable by reading the spec plus the bytecode at a public address." This frames auditability as the decisive differentiator, but the paper never defends *why* auditability-by-reading-a-public-address is the right yardstick. Signal's clients are open-source; the server protocol is published; the anonymous-credential scheme has been formally analysed (Chase, Perrin, Zaverucha, CCS 2020). The auditability gap between Signal PGS and Onym is real but narrow, and presenting it as though it obviously justifies the complexity of approach 4 is a rhetorical move, not an argument.

**What would fix this:** Quantify the auditability gap. What *specific* verification can a third party perform on Onym's public chain state that they cannot perform on Signal's published protocol? If the answer is "verify that the server executed the protocol faithfully without trusting the server binary," say so explicitly and acknowledge that this is a meaningful but niche requirement.

### C3. The BLS12-381 vs. BN254 justification is now present but overstated.

The rewrite says BN254's "effective security dropped to an estimated 100–103 bits after improved number-field-sieve attacks [15], below the 128-bit line we were targeting." Then it says BLS12-381 is "at 120 bits of estimated pairing security." Both curves are below the 128-bit target. Presenting 120 bits as "closer to the target" when 100–103 is "below the line" is applying two different standards to the same threshold. Either 120 bits is acceptable and the 128-bit target is a soft goal, or 120 bits is also below the line and the choice of BLS12-381 is a lesser-of-two-evils, not a clean win.

**What would fix this:** Be precise about the security target. If 120 bits is acceptable, state the actual threshold and justify it. If 128 bits is the hard requirement, acknowledge that BLS12-381 also falls short and explain why the shortfall is tolerable.

---

## Part II: Technical and structural issues

### C4. The commitment-hiding game is weaker than it looks.

The commitment-hiding game (Sec. 4.3) requires the adversary to distinguish $\mathsf{Commit}(S_0, \epsilon, s)$ from $\mathsf{Commit}(S_1, \epsilon, s)$ when $|S_0| = |S_1|$. The equal-size constraint is necessary because the tier is public. But the game is discharged "by the one-wayness of Poseidon with high-entropy salt and by $s$ never appearing on-chain." One-wayness is not the same as hiding. A commitment scheme requires computational hiding (indistinguishability), which is a stronger property than one-wayness. The paper should either (a) prove hiding from the pseudorandomness of Poseidon (which is a standard assumption in the Poseidon literature), or (b) cite an existing result that establishes this reduction. As stated, the security argument has a gap between the claimed property and the assumption used to discharge it.

### C5. $R_{\mathsf{Upd}}$ does not constrain the new member set.

$R_{\mathsf{Upd}}$'s second conjunct is:

$$H(H(\mathsf{root}_{\mathsf{new}}, \epsilon_{\mathsf{old}} + 1), s_{\mathsf{new}}) = C_{\mathsf{new}}.$$

This binds $C_{\mathsf{new}}$ to *some* new root and *some* new salt, but it does not constrain the *relationship* between $\mathsf{root}_{\mathsf{old}}$ and $\mathsf{root}_{\mathsf{new}}$. A valid proof can transition from any old root to any new root. The paper acknowledges this implicitly (any current member can "compute what the club's next envelope should say"), but the security implications are never stated. Specifically:

- A single malicious member can replace the entire member set in one operation, ejecting all other members.
- There is no in-circuit enforcement of "add one member" or "remove one member" semantics.
- The contract has no way to distinguish a legitimate membership change from a hostile takeover.

This is presumably a design choice (the alternative — constraining the diff — would be much more expensive in R1CS), but it is a significant one that should be discussed explicitly. An IEEE S&P reviewer will notice that the system's authorization model is: "any current member can set arbitrary next state." That is a much weaker guarantee than most group-membership systems provide.

**What would fix this:** Add a paragraph to Sec. 4.2 or Sec. 7 that explicitly states the authorization semantics: any member can set any next state, and the social/application-layer protocol (not the circuit) is responsible for ensuring benign transitions. Discuss the tradeoff.

### C6. The fee-payer unlinkability game's $1/n$ bound is vacuous for small groups.

The fee-payer unlinkability game says the adversary wins if they can identify the updater with probability non-negligibly greater than $1/n$. For a 3-member group, the baseline is $1/3$. The relayer pattern hides the fee payer, but does not address *timing* correlation: if Alice always updates within 30 seconds of going online (observable via Nostr presence or IP metadata), the adversary's advantage can far exceed $1/n$ without breaking the relayer abstraction. The game as stated does not capture side-channel advantages, and for small groups, the formal guarantee is already weak. The paper should either strengthen the game to account for timing (hard) or explicitly flag that the $1/n$ bound is a best-case number that degrades with auxiliary information.

### C7. The postmortem on P3 (co-membership leakage) undersells the remaining attack surface.

The fix for P3 is ephemeral per-event secp256k1 keys. This addresses static key linkability. But the paper notes that events also carry "a stable hidden topic tag." If the hidden topic tag is per-group and remains stable across events, an observer who can identify the tag (by construction, correlation, or relay-side logging) can still cluster co-membership. The paper says the fix addressed the key linkability and then moved on, but the topic-tag linkability is mentioned in passing and never resolved. Is the hidden topic tag rotated? Is it encrypted to the group? If it is stable and observable by the relay, the co-membership leak is reduced but not eliminated.

**What would fix this:** Clarify whether the hidden topic tag was also addressed. If yes, describe the fix. If no, add it to the Honest Limitations section.

### C8. The "companion document" for full reductions is still uncitable.

Section 4.3 says: "A companion document contains the full reductions." This companion document is not cited, not linked, and not accessible to the reader. For a venue that values verifiable claims, this is a significant weakness. The paper asserts five security games and discharges each in one sentence. A reviewer has no way to verify the reductions.

**What would fix this:** Either (a) include the full reductions in an appendix (if the venue allows supplementary material), (b) post the companion document to a public archive (IACR ePrint, arXiv) and cite it with a stable URL, or (c) explicitly acknowledge that the reductions are not independently verifiable from the published article and that this limits the strength of the formal claims.

---

## Part III: Presentation and completeness

### C9. Table 3 underrepresents Signal's privacy properties.

Table 3's "Metadata-observable by" column says Signal PGS is observable "by Signal, under compromise." This is misleading. Signal PGS uses anonymous credentials specifically so that Signal's *own servers* cannot observe group membership. The correct statement is that Signal's servers do not learn group membership in the honest-but-curious model; under active compromise (server-side code modification), they could. The table's phrasing collapses these two very different threat models into a single cell and makes Signal look worse than it is.

### C10. The abstract is now too long.

At approximately 250 words, the abstract is at the outer edge of IEEE S&P feature-article norms (typically 150–200 words). The abstract's strength is its honesty — it names the vulnerability, the trusted-setup gap, and the testnet status — but it could be tightened. The parenthetical list of all four postmortems reads as a table of contents rather than an abstract. Consider summarizing the postmortems as "four postmortems documenting a constraint-cost crisis, a critical authorization vulnerability, a transport-layer co-membership leak, and an unresolved trusted-setup dependency" without the full description of each.

### C11. Reference [12] is double-assigned.

Reference [12] is cited in Sec. 1 for the Hayden "we kill people based on metadata" quote and in Sec. 5 P1 for "the arkworks gadget [12]." These are two different sources sharing the same reference number. One of them needs to be renumbered.

### C12. The word count target is 5,500 but the paper is likely over.

The frontmatter says `word_count_target: 5,500`. The actual text appears to exceed this. The evaluation section (Sec. 6) could be tightened — the methodology description, while welcome, is more detailed than a feature article requires. The related-work table (Table 3) could be compressed by dropping the "Deployment maturity" column, which adds breadth but not depth.

### C13. No acknowledgement of the limitation that Onym requires all members to be online provers.

Every membership change requires a current member to generate a Groth16 proof on their device. This means:

- A member whose phone is off or out of battery cannot authorize a group change.
- There is no "admin adds member while member is offline" flow unless the admin holds the new member's key material.
- The system assumes always-on mobile proving capability.

This is a significant usability constraint relative to traditional group-membership systems where a server can process changes on behalf of offline members. It should be listed in Sec. 7 (Honest Limitations).

---

## Summary

The rewrite is substantially stronger than the previous draft. The motivation now engages with the real world. The vulnerability is called a vulnerability. The trusted setup is foregrounded rather than buried. The formal definitions are clean. The postmortem structure — four episodes under a unifying thesis about local-vs-system correctness — is compelling and well-suited to IEEE S&P's readership.

The remaining issues fall into three categories:
1. **Precision gaps in the security arguments** (C4, C5, C6, C8): the formal claims are asserted at a level of detail that invites scrutiny but does not survive it. The hiding game uses the wrong assumption; $R_{\mathsf{Upd}}$ does not constrain state transitions; the unlinkability game ignores timing; the reductions are not accessible.
2. **Incomplete treatment of residual attack surface** (C7, C13): the co-membership fix may be partial; the always-on-prover requirement is unstated.
3. **Presentation polish** (C1, C2, C3, C9, C10, C11, C12): the motivation is much improved but still has rhetorical tilts that a careful reviewer will flag.

None of these are fatal. A revision that addresses C4, C5, C7, and C8 would produce a strong article.
