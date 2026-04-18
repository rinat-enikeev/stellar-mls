# Paper Plan

## Venue

**IEEE Security & Privacy magazine** — peer-reviewed bimonthly magazine of the IEEE Computer Society. Aimed at security and privacy practitioners and researchers; values system descriptions, case studies, and lessons-from-deployment over narrow technical novelty. Articles must be accessible to a broad audience, written in a "practical and original" voice, and report on real systems or experimental validation.

## Article type and target length

- **Feature article**, ~5,500 words.
- ~150-word abstract.
- 12–15 references (IEEE numeric style).
- 3–5 figures/tables permitted; we use 3 tables (constraint budgets, threat-model summary, comparison with related systems) and one figure (system diagram).

## Working title

> **Onym: Private Group Membership on a Public Ledger — Two Design Choices That Made or Broke the System**

## Thesis

Anchoring group membership state on a public blockchain is feasible *and* privacy-preserving when the on-chain object is a zero-knowledge commitment plus a Groth16 membership proof. But the engineering surface around the cryptographic core is where systems fail: in our deployment two non-obvious design choices (a Poseidon-only commitment binding, and a separate update circuit binding the *new* commitment as a public input) made the difference between an unusably slow, subtly insecure prototype and a system that runs on commodity mobile hardware and provably authorizes the right operation.

## Contributions claimed

1. A practitioner-oriented account of building a privacy-preserving group registry on Stellar, including architecture and measured costs.
2. A constraint-budget analysis showing a 14× circuit-size reduction by replacing in-circuit SHA-256 with a dual-Poseidon outer binding (Variant B), and the on-chain trade-offs it forces.
3. An incident report on a "statement vs. operation" mismatch — a Groth16 proof that was sound but pertained to the wrong predicate — and the two-circuit refactor that fixed it.
4. A second incident report on transport-layer co-membership leakage caused by stable Nostr device keys, illustrating how a normative privacy property can be silently downgraded by a sibling spec's "trade-off" framing.
5. A summary of residual leakages and the relayer-based fee-decoupling mitigation.

## Outline

1. **Introduction** (~600 words) — motivation, problem statement, contributions.
2. **Background and threat model** (~700 words) — MLS, blockchain anchoring, what an on-chain observer learns by default, security goals.
3. **System overview** (~800 words) — dual-hash commitment, Groth16 over BLS12-381, two circuits, on-chain ABI, fee decoupling.
4. **Design choice #1: in-circuit SHA-256 vs. Poseidon-only binding** (~700 words) — measured constraint counts, prover-time impact on mobile, on-chain consequences.
5. **Design choice #2: separating the membership circuit from the update circuit** (~900 words) — the unbound `new_commitment` gap, why every audit pass missed it, and the fix.
6. **Lessons from a transport-layer incident** (~500 words) — stable Nostr keys, spec drift between SEP and NIP, the ephemeral-author fix.
7. **Performance and deployment** (~400 words) — proof size, verification cost on Soroban, end-to-end latency on iOS/Android.
8. **Limitations and open problems** (~300 words) — fee-payer correlation, group-size leakage from tiers, post-compromise forward secrecy.
9. **Related work** (~300 words) — Semaphore, Tornado Cash, MLS, Zcash, Aztec, Signal Sealed Sender.
10. **Conclusion** (~200 words).

## Tone and conventions

- Practitioner-and-researcher voice: concrete, named, avoids jargon without explanation, but does not flinch from cryptographic precision when needed.
- Use "we" throughout; the system is named *Onym*, the underlying spec is *SEP-XXXX*.
- All claims about constraint counts, proof sizes, and gas figures are measured from the open-source reference implementation and cited by repository path.
- Postmortems are described in operational terms (timeline, trigger, fix, generalisable lesson) — not as recriminations.
