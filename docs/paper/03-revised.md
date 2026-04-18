---
title: "Onym: Anchoring Private Group Membership on a Public Ledger — Two Incidents and What They Taught Us"
author: Rinat Enikeev
venue: IEEE Security & Privacy magazine (feature article)
status: Submission-ready revision (post-self-critique)
date: 2026-04-19
word_count_target: 5,800
---

# Onym: Anchoring Private Group Membership on a Public Ledger — Two Incidents and What They Taught Us

## Abstract

Anchoring group membership on a public blockchain is attractive — every participant gets the same ordered view, with no central server to compromise — but the most obvious encoding (a list of member addresses on-chain) destroys exactly the privacy that motivated using a group in the first place. We describe Onym, a deployed open-source system that anchors MLS-compatible group membership on the Stellar blockchain using a Poseidon Merkle commitment and a Groth16 zero-knowledge proof, and we report two incidents that reshaped its design. Replacing in-circuit SHA-256 with a dual-Poseidon commitment binding cut the Groth16 R1CS constraint count by 8.5× to 14× depending on tier and brought sub-second proving onto commodity 2024-class smartphones. Splitting our membership relation into a separate update relation that binds the *new* commitment as a public input closed an operation-binding gap — a known Groth16 malleability pattern — that survived four audit passes and a formal soundness theorem because the proof was correct, but for the wrong predicate. Onym is deployed on Stellar testnet at contract `CBKKEZU3CEAXZNJ4RSLDSVCXNJFQK4WMAOGF26SUBT2QUKHWQE2PRFSO`; mainnet rollout is gated on a Phase-2 trusted-setup MPC ceremony and an external audit of the new update relation.

---

## 1. Introduction

Group-based coordination is foundational to many decentralised applications. End-to-end encrypted messaging groups, anonymous DAO voting rolls, multi-party signing committees, and credential-issuance schemes share one structural requirement: a shared notion of *who is in this group right now*, agreed by every member, hard to forge, and durable across membership changes. Blockchains are a natural anchor for this notion. They provide global ordering, immutability, and permissionless auditability — exactly the guarantees a shared membership state needs.

The Messaging Layer Security (MLS) protocol [1] is the most operationally relevant example. MLS manages group encryption keys with a tree-based ratchet that produces a fresh cryptographic state on every membership change ("Commit"). For two devices to remain in sync about who is in the group and which encryption epoch is current, they must agree on the order and content of those Commits. Today, that ordering is delivered by a trusted Delivery Service. Anchoring the same state on a blockchain would remove that trust assumption — except that the most obvious encoding, `group_id → [member_address_A, member_address_B, ...]`, immediately publishes to the world the social graph that end-to-end encryption was designed to keep private. The benefit and the harm are tightly coupled.

Onym is our attempt to keep the benefit and discard the harm. The system anchors private group membership on the Stellar blockchain using two cryptographic objects: a Poseidon Merkle commitment over the member set, and a Groth16 zero-knowledge proof [2] that the prover holds a secret key whose corresponding public key is a leaf of the committed tree. The contract — a Soroban smart contract running on Stellar Protocol 22 [11], which provides BLS12-381 host functions — stores per group only an opaque 32-byte commitment, an epoch counter, and a tier identifier. No member identity ever touches the ledger. Verification is constant-cost regardless of group size: a 2,048-member group is indistinguishable from a 2-member group from the contract's perspective. The system is open source, deployed on Stellar testnet at the time of writing, and integrated into a reference iOS and Android chat client.

This article is not a survey, and it does not introduce new cryptographic primitives. It is a practitioner's account of building a privacy-preserving group registry on a public blockchain — and an honest discussion of the two incidents that dominated the engineering. The first (Sec. 4) was a circuit-shrinking choice that reduced our Groth16 constraint count by between 8.5× and 14× depending on tier and made proof generation usable on commodity mobile hardware. The second (Sec. 5) was an authorisation-binding gap in which the zero-knowledge proof was sound but pertained to the wrong predicate; the gap survived four audit passes because every review examined the cryptographic core and the smart contract in isolation, and no one held them side-by-side and asked, *what does this proof actually authorise?* A third incident (Sec. 6), discussed because it teaches the same broader lesson at a different layer of the stack, concerns a transport-layer privacy invariant silently downgraded by a sibling specification's "trade-off" framing.

We make four contributions:

1. A practitioner-oriented description of how to anchor MLS-compatible group state on a public blockchain without leaking the member set or the updater's identity (Sec. 3).
2. A constraint-budget analysis that quantifies the cost of in-circuit SHA-256 and motivates a Poseidon-only commitment binding, with measured numbers from a working reference implementation (Sec. 4).
3. An incident report on a "statement vs. operation" mismatch — a Groth16 proof that authorised the wrong write — and the two-circuit refactor that fixed it (Sec. 5). We situate the gap within the existing literature on Groth16 public-input malleability.
4. A second incident report on co-membership leakage caused by stable transport-layer keys, illustrating how a normative privacy invariant can be silently downgraded by a sibling spec's framing (Sec. 6).

The system is one engineering artefact, but we believe both lessons generalise: the first to any zk-SNARK-anchored system that is deciding between hash families inside and outside the circuit, and the second to any cryptographically authorised state machine in which more than one operation shares a single proof relation.

## 2. Background, threat model, and security goals

**On-chain membership today.** Most existing on-chain group registries store membership lists in plaintext or as a hash of a known address set. Both encodings reveal the group's social graph either directly or via an offline dictionary attack — the universe of plausible members is rarely large enough for a hash to hide it. Even when the membership list itself is hidden, every state-changing transaction is signed by a Stellar account, so an observer who watches `update_commitment` calls over time correlates signing addresses with group activity and recovers a partial graph through traffic analysis.

**MLS as the motivating use case.** RFC 9420 [1] specifies a tree-based key agreement that produces a cryptographic group state on every membership change. The state must be ordered, consistent across devices, and resistant to forks. A blockchain is a near-perfect anchor for this state because it provides a global, signed total order at low cost. Onym makes that anchor compatible with MLS's privacy properties — the on-chain record must not identify members, must not identify the updater, and must not let an observer measure even whether the group grew or shrank in a given epoch.

**Threat model.** We assume a powerful but standard adversary:

- They observe the entire Stellar ledger, all contract events, and all relay traffic carrying invitations and ciphertexts.
- They may submit arbitrary transactions, including replays and substitutions of values they observe.
- They may corrupt a subset of group members and learn their long-term keys.
- They may operate or compromise the relayer that submits transactions on behalf of group members.
- They are computationally bounded (probabilistic polynomial time, security parameter λ = 128 commensurate with BLS12-381).

We do **not** defend against network-layer traffic analysis (IP correlation, mixnet attacks on relays), side channels in client implementations, denial-of-service against the Stellar network, or compromise of the application's encryption layer once a member's device is fully owned. These are standard scoping decisions for a system at this layer.

**Security goals as games.** From any view of the ledger, the adversary should learn no more than: that a group with a particular opaque identifier exists; that it has changed state some number of times; the timestamps of those changes; and an upper bound on group size from the circuit tier (32, 256, or 2,048). We formalise six security games and tie each to a specific design element. Table 1 summarises the mapping.

**Table 1.** Security games and the design elements that discharge them.

| Game | Adversary wins if… | Discharged by |
|------|--------------------|---------------|
| Soundness | Produces an accepting proof without knowing any member secret. | Groth16 over BLS12-381; per-tier trusted setup (Sec. 3). |
| Zero-knowledge | Distinguishes a real proof from a simulated one. | Groth16 ZK property; 192-byte constant-size proofs (Sec. 3). |
| Commitment hiding | Recovers the Merkle root from the on-chain commitment. | Dual-hash construction with fresh per-epoch salt (Sec. 3). |
| Epoch integrity | Reverts, skips, or forks the epoch sequence. | Strict in-contract monotonicity + `R_Update`'s in-circuit `epoch_old + 1` (Sec. 5). |
| Prover privacy | Determines which member produced a given proof. | Constant-size proof + tier-uniform IC vector (Sec. 3, 7). |
| Fee-payer unlinkability | Correlates a Stellar account with group activity. | Relayer pattern (Sec. 3, 8). |

The full formal proofs of these games are derived in a companion soundness document, summarised in Sec. 7. The point of Table 1 is operational: when a reviewer asks "what closes game X?", the answer is a specific design element they can read in the code.

## 3. System overview

Figure 1 sketches the data flow. Each component is described briefly below.

> **Figure 1.** Onym data flow. A current group member generates a Groth16 membership proof on-device (using a Poseidon Merkle opening and a witness salt obtained through the encrypted application channel), submits the signed Soroban operation to a public relayer over an anonymous transport, and the relayer wraps the operation in a Stellar transaction signed by the relayer's own account. The on-chain contract verifies three pairings, compares the public-input commitment to its stored value, and persists the next epoch. Member identity keys, salt, and proof witnesses never leave the member's device.

**Member identity keys are BLS12-381 G1 scalars.** Each member holds a private key `sk` in the BLS12-381 scalar field and exposes a public key `pk = sk · G1`. The choice is forced by the cost of verifying key ownership inside the circuit: emulating Curve25519 (the Ed25519 base field) inside a BLS12-381-native R1CS circuit costs roughly 500,000 constraints for a single scalar multiplication — far too expensive for a smartphone prover. BLS12-381 native keys reduce key ownership to a single native-field operation. We preserve the link to a member's Stellar Ed25519 address through an off-chain attestation: the member signs their BLS12-381 public key with their Ed25519 key and shares the attestation with other members through the encrypted channel. The attestation is verified at registration time, never inside the ZK circuit, and never appears on-chain.

**Commitment construction.** The member set is sorted by compressed G1 byte order (membership is a *set*; the commitment must be order-independent), placed as leaves in a binary Poseidon Merkle tree of fixed depth `d`, and the tree root is bound to the epoch counter and a fresh 32-byte salt to produce the on-chain commitment. The salt is generated by the Commit initiator, distributed to current members through the encrypted channel, and never published. We deliberately separate two hash families: Poseidon for everything inside the circuit (≈300 R1CS constraints per evaluation), and — in the original design — SHA-256 for the outer binding so the contract can recompute the commitment using a native Soroban host function. Section 4 explains why we abandoned SHA-256 for the outer binding.

**Membership proof.** Any current member can produce a Groth16 proof that they hold a secret key whose Poseidon image is a leaf of the committed tree:

> "I know `sk` such that `Poseidon(sk)` is a leaf in the Poseidon Merkle tree whose root, combined with the on-chain epoch and the (witness) salt, produces the on-chain commitment for `(group_id, epoch)`."

The proof is 192 bytes (three compressed BLS12-381 group elements: π_A in G1, π_B in G2, π_C in G1) regardless of group size, and verification on the contract uses exactly three pairings — a fixed-cost operation directly accelerated by Stellar Protocol 22's host functions.

**On-chain ABI.** The Soroban contract exposes five state-changing operations (`initialize`, `create_group`, `update_commitment`, `verify_membership`, `deactivate_group`) and two read-only queries (`get_state`, `get_history`). The contract stores no committer address; the ZK proof authorises every state change. Operational extensions (admin-controlled VK rotation, restricted-create mode, TTL bumps) sit outside the interoperable interface but are documented as part of the deployment.

**Fee decoupling.** A ZK proof hides which member generated it, but a transaction signed by that member's Stellar account re-links a Stellar identity to group activity. We sidestep this with a relayer pattern: a member submits a signed `InvokeHostFunction` operation through any anonymous channel to a public relayer, which wraps the operation in a transaction using its own account as the fee source. To avoid a common misreading: the relayer's Stellar account *does* appear on the transaction envelope and *is* persisted in Stellar's history. The privacy guarantee is not that no address ever appears — it is that the address that appears is the relayer's, which is unrelated to the prover's identity. The relayer sees only an opaque proof and an opaque group ID. It cannot determine which member is acting. Implementations that skip the relayer must document the residual correlation as a known privacy limitation.

**Where MLS fits.** Each MLS Commit produces a new group state. Onym maps a Commit to an `update_commitment` call: the new Poseidon root is computed over the post-Commit member set, a fresh salt is sampled, the new commitment is computed, and a proof is generated against the new state and submitted via the relayer. Salt distribution piggybacks on MLS's encrypted GroupContext extension. MLS continues to handle key derivation and ratcheting; Onym contributes only the privacy-preserving anchor. A second consumer is anonymous DAO voting, where the same membership proof becomes a one-of-many ballot signature.

## 4. Design choice #1: in-circuit SHA-256 vs. Poseidon-only binding

The original design (which we will call Variant A) bound the commitment with SHA-256:

```
commitment = SHA-256(poseidon_root || epoch || salt)
```

The choice was deliberate. SHA-256 is available as a Soroban host function (`env.crypto().sha256()`), making in-contract verification gas-efficient and giving anyone — auditors, indexers, third-party developers — a one-line way to recompute the commitment from witness values. The bridge between cryptographic primitives and the runtime felt clean.

The bridge cost more than we expected.

In-circuit SHA-256 is brutally expensive in R1CS. Even for a fixed-length 72-byte preimage — which avoids variable-length bookkeeping — a single SHA-256 compression call adds roughly 25,000 constraints. Table 2 shows the constraint budgets. SHA-256 alone dominated the small-tier circuit (32 members) by a factor of more than 8× over every other constraint combined. The implication for the prover was direct: on a 2024-class smartphone with 6 GB of RAM, generating a Variant A proof for the small tier was slow enough that we never measured it cleanly in production — the prover failed to complete reliably under multi-app memory pressure. The medium and large tiers were effectively unusable on mobile.

**Table 2.** Measured R1CS constraint counts for the reference circuit, by tier and commitment binding.

| Tier | Members | Tree depth `d` | Variant A (SHA-256, est.) | Variant B (Poseidon-only, measured) | Reduction |
|------|---------|----------------|---------------------------|-------------------------------------|-----------|
| Small | 32 | 5 | ~26,740 | **1,910** | 14.0× |
| Medium | 256 | 8 | ~27,640 | **2,630** | 10.5× |
| Large | 2,048 | 11 | ~28,540 | **3,350** | 8.5× |

We replaced SHA-256 in the binding with a two-call Poseidon construction (Variant B):

```
commitment = Poseidon(Poseidon(poseidon_root, epoch), salt)
```

Two Poseidon evaluations cost roughly 600 R1CS constraints. The small tier collapsed from ~26,740 constraints to 1,910 — a 14× reduction; medium and large tiers dropped 10.5× and 8.5× respectively. The reduction shrinks at higher tiers because SHA-256's contribution is a constant 25,000 while the Merkle path's contribution grows logarithmically with depth, so the SHA-256-shaped term becomes proportionally smaller as the tree deepens. A reader implementing Onym at the large tier should expect the 8.5× number, not the headline 14×.

The change has consequences on the ledger side. The contract can no longer recompute the commitment with a native host function — Soroban does not yet provide a Poseidon precompile. We mitigate this by treating the commitment as an opaque value validated solely through the ZK proof: the contract checks that `public_inputs.commitment` matches the stored 32-byte value, and the Groth16 verifier guarantees the rest. There is no in-contract path to verify the commitment from witness values, but there does not need to be: the verifier circuit performs that check.

A second consequence is encoding subtlety. Variant B reduces the 32-byte salt modulo the BLS12-381 scalar field order `r ≈ 2^255`. About half of uniformly random salts are reduced, so two distinct byte strings can map to the same field element. This drops effective salt entropy from 256 bits to roughly 255. Cryptographically negligible — but worth documenting, because the alternative (rejection-sample salts in the canonical range) costs a non-trivial amount of off-circuit code to get right and is easy to forget when porting to a new client.

The deeper lesson is one we now treat as a checklist item: **any time a cryptographic primitive sits both inside and outside an R1CS circuit, count its constraint cost first and let the answer drive the rest of the design.** We had assumed the SHA-256 host function on Soroban would dominate the choice. The R1CS cost dominated by an order of magnitude in the other direction. The "bridge" we paid 25,000 constraints to build was a bridge to a host function that turned out to be optional, because the proof itself was the bridge.

## 5. Design choice #2: separating the membership relation from the update relation

The other incident is the one we got wrong, and how we found out.

Until release v1.3.0, all four state-changing operations on the Onym contract — `create_group`, `update_commitment`, `verify_membership`, and `deactivate_group` — used the same Groth16 circuit. The circuit's relation, which we will call $R_\text{Membership}$, was:

> "There exists `sk, root, salt, merkle_path, leaf_index` such that `Poseidon(sk)` opens `merkle_path` to `root`, and `Poseidon(Poseidon(root, epoch), salt) == commitment`."

Two public inputs (`commitment`, `epoch`); the rest is witness. The circuit is sound: an adversary who does not hold a member key cannot produce an accepting proof. We had a formal soundness theorem to that effect. A code-review question changed our reading of it.

> *"Where in the math is the argument that this proof is non-transferable to a different `new_commitment`?"*

The answer was: nowhere.

`update_commitment` took, in addition to the proof and the public inputs, a separate `new_commitment: BytesN<32>` parameter. The contract verified the proof against `(commitment, epoch)` — the *current* state — and then wrote `new_commitment` to storage as the next epoch's state. The proof bound the prover to the current epoch, but said nothing about which next state they were authorising. Anyone in the path between prover and ledger — a relayer, a Stellar mempool front-runner, an intermediary HTTP proxy — could substitute an arbitrary 32-byte value for `new_commitment` and the proof would still verify. The prover had signed a blank check.

This is a known pattern. In Groth16, the verification equation incorporates a linear combination of so-called *IC* (instance-commitment) points indexed by the circuit's public inputs. The proof is bound, cryptographically and non-malleably, to exactly those public inputs and to nothing else [15]. Any value the contract reads that is not a public input is, by construction, outside what the proof authorises. The Tornado Cash and Semaphore [4] designs handle the analogous problem (binding a withdrawal address into the proof) by making the contract-controlled value a public input of the relation — exactly the pattern we eventually adopted. The class of vulnerability has been discussed in the ZK-applications literature since at least 2019 under labels including "public-input malleability" and "operation-binding gap." Onym's variant is not novel; what we believe is worth reporting is that it survived a reasonable review process, and *why*.

**Why every audit pass missed it.** The system had been through four security audits at the time of discovery. None had flagged it. We think there are three structural reasons:

1. **Reviewers read the cryptographic core and the contract in isolation.** The circuit's R1CS file lives in `src/circuit/mod.rs`. The contract's ABI lives in `contracts/sep-xxxx/src/lib.rs`. The two files, side by side, make the gap visible in under a minute. Read separately, each looks correct. Audit findings tend to be local; this gap was a property of the *interface* between two correct components.

2. **The presence of related mitigations created false reassurance.** We had implemented anti-replay (a SHA-256 of accepted proofs is recorded and re-submission is rejected), canonical-bytes enforcement on the public-input commitment (rejecting non-canonical Fr encodings), and strict epoch monotonicity. Each addresses a *different* attack class. Their visible presence made it feel like proof handling was defensively complete. It wasn't.

3. **Our soundness theorem was a theorem about the wrong object.** The formal argument proved knowledge soundness over $R_\text{Membership}(C, e; sk, salt, \text{auth\_path})$. The argument is correct. It is also irrelevant to the question of whether the *update operation* is sound, because the update operation's security predicate is `(C_old, e_old, C_new, e_new)` — variables that $R_\text{Membership}$ does not mention. We had given the wrong statement the full force of a proof.

**Design alternatives we considered.** Two fixes were on the table. The first was to add `C_new` as a fourth public input to the existing `R_Membership` circuit, gated by an in-circuit boolean that disabled the constraint for the read-only call sites. This is the minimal-diff fix: a single circuit, three call sites continue to use a degenerate witness for `C_new`, and the fourth call site (`update_commitment`) supplies a real value. We rejected it for two reasons: (a) an unconstrained boolean public input is a footgun (the circuit must constrain the boolean to {0,1}, and any reviewer reading the constraint set must trust that the gating is implemented correctly); (b) the read-only call sites would semantically carry a witness for an operation they do not perform, which makes the operation-binding question harder, not easier, to answer at audit time.

The second option — splitting the relation into $R_\text{Membership}$ (unchanged) and a new $R_\text{Update}$ — duplicates the membership-opening constraints across two circuits but keeps each circuit's authorisation predicate small and direct. A reviewer reading $R_\text{Update}$ sees "this proof binds (C_old, epoch_old, C_new) and nothing else"; a reviewer reading $R_\text{Membership}$ sees "this proof binds (C, epoch) and nothing else." The duplication cost is six trusted-setup ceremonies instead of three (two circuits per tier, three tiers), which is real but bounded. We chose this option because it makes the operation-binding question answerable from the relation file alone.

The new $R_\text{Update}$ has three public inputs `(C_old, epoch_old, C_new)` and three constraints:

1. The prover is a member of the old committed state: `Poseidon(sk)` opens to `root_old`, and `Poseidon(Poseidon(root_old, epoch_old), salt_old) == C_old`.
2. `C_new` is a canonical commitment over the next epoch: `Poseidon(Poseidon(root_new, epoch_old + 1), salt_new) == C_new`. The `+ 1` is performed inside the circuit; the contract never accepts an attacker-chosen new epoch.
3. Implicitly, the verifier rejects any envelope in which `C_new` has been substituted, because `C_new` is now part of the IC linear combination.

The contract additionally compares its stored commitment and epoch to `(C_old, epoch_old)` from the public inputs and rejects mismatches before any pairing operation, providing a fast-fail state-binding check. Each of the three circuit tiers now ships two verification keys (one for $R_\text{Membership}$, one for $R_\text{Update}$), and the contract's `initialize` takes six VKs instead of three. Wire formats change: `update_commitment` now takes a single 73-byte `UpdatePublicInputs` blob with a `0x02` version byte; the standalone `new_commitment` parameter is gone. Cross-platform test vectors for the new format are checked into the repository and consumed by all four client implementations (Rust, Swift, Kotlin, contract).

The fix shipped over twelve commits on `main` and was cut as release v1.3.0 (commit `d2837d2`) on 2026-04-18. The contract was redeployed to Stellar testnet at the new contract ID. No mainnet traffic was at risk; the gap was discovered before mainnet rollout.

**A checklist for the same class of bug.** We have written the lesson into our design review template:

1. For every cryptographically authorised operation, write a one-page document that states, in operational terms, the predicate the proof discharges. If the document is shorter than the contract's ABI for the same operation, the proof is binding less than the operation does.
2. Enumerate, by name, every value the contract reads on that path. For each, state whether it is part of the proof's IC vector. Any value that is *not* is an attacker-controlled free parameter.
3. Read the relation file and the contract entry point side by side, in the same review session. If the names of the public inputs don't match the names of the contract parameters one-for-one (or by an obvious, documented bijection), there is a binding question to answer.
4. Reject "we have replay protection" or "we have a canonical-bytes check" as answers to the binding question. Those mitigations address other attack classes.
5. If your soundness theorem mentions a relation $R$, check that the operation in question is a predicate over the variables $R$ binds. If it isn't, the theorem is about the wrong object.

## 6. A third incident: transport-layer co-membership leakage

The cryptographic core was not the only place where we had to learn the same lesson. A second incident, six weeks before the unbound-`new_commitment` discovery, taught it on the transport layer; we report it here because the structural failure mode is the same — a cryptographic invariant silently downgraded by spec drift.

Onym's clients deliver invitations, rekey envelopes, and encrypted group ciphertext over Nostr [3], a permissionless event-publishing relay system. Every Nostr event is signed by a secp256k1 key. We had used each client's *long-lived* identity key to sign every group message and every per-recipient envelope. The companion specification that defined Onym's transport layer mentioned the implication in a single bullet of its Security Considerations:

> *"Stable Nostr device keys improve usability but increase sender linkability."*

That sentence is technically true and strategically misleading. "Sender linkability" sounds like a profile-building concern affecting an individual. The actual consequence, given that group ciphertext also carries a stable hidden topic tag, was *group linkability*: any relay (or anyone with a relay's event log) could partition the user base into co-membership clusters from public data alone — *"these N keys all published to hidden-topic T → these N devices are in the same group"*. The SEP, which the system's security analysis flowed from, claimed precisely the opposite as a load-bearing privacy property: *"an observer cannot determine who is in the same group."* The two normative documents contradicted each other for the entire lifetime of the transport layer.

This kind of cross-document inconsistency is exactly what RFC 6973 §6.3 [13] is meant to catch: a referencing specification's privacy claims must be normatively reconciled with every referenced specification. We had not done the reconciliation.

The fix was small. The Rust FFI already accepted arbitrary 32-byte secrets, so a per-event ephemeral secp256k1 key on each platform was a five-line factory and a `defaultNil` parameter on every send call site. We then had to audit the receive side, where `event.pubkey` had been used as a sender identifier in four places (display logic, salt-request rate limiting, removal-notice display name lookup, and a database column). Those uses had to switch to the *inner* BLS public key — which had always been the real authentication anchor. The outer Nostr key was never security-critical; it had been used as a stable identifier because it happened to be stable, not because anything required it.

We pulled two lessons. First, **a privacy invariant is either normative or it is fiction**. Calling a violation a "trade-off" makes the spec's contributors feel honest while leaving downstream readers with no signal that the system breaks one of its own claims. Bullets in Security Considerations are not enough; we now require any privacy property in any normative document to be either (a) cryptographically enforced, with a citation to the construction that enforces it, or (b) explicitly listed in a "Known privacy limitations" section of every consumer's documentation.

Second, **identity coupling is a recurring antipattern**. Whenever a single key serves both as a signing key and as a sender identity, future-you will be tempted to leave it stable for reasons that have nothing to do with security, and "stable enough" will quietly become the system's privacy ceiling. Decoupling those two roles — ephemeral per-event signing key, stable inner cryptographic identity — costs almost nothing if you do it on day one and a postmortem if you don't.

Both Sec. 5 and Sec. 6 are, at root, the same failure mode: a security invariant claimed in one document was not actually enforced by the construction in another. The first manifested in the cryptographic core, the second in the transport layer. The fix in both cases was small once the gap was named. The cost of *naming* the gap is the entire content of this section.

## 7. Performance and deployment

Onym is deployed on Stellar testnet at contract ID `CBKKEZU3CEAXZNJ4RSLDSVCXNJFQK4WMAOGF26SUBT2QUKHWQE2PRFSO` and is integrated into reference iOS (SwiftUI) and Android (Jetpack Compose) clients. The Rust cryptographic core, two SDKs, the relayer, the contract, and a self-hosted relay-and-storage stack are open source. Mainnet rollout is gated on a Phase-2 MPC trusted-setup ceremony for the six `(circuit, tier)` Groth16 verification keys and an external audit pass scoped specifically to the new $R_\text{Update}$ circuit.

**Benchmark methodology.** All prover-time numbers below are measured on a Pixel 8 Pro (Android 14, Tensor G3, 12 GB RAM) and an iPhone 15 Pro (iOS 17, A17 Pro, 8 GB RAM). Each measurement is the median of 50 runs with 10 warmup runs discarded; we also report the 95th-percentile wall-clock time. Witness generation, multi-scalar multiplication, and proof serialisation are all included; salt material and Merkle openings are precomputed off-path (the production client maintains them incrementally as Commits arrive). Variant A wall-clock numbers are not reported because Variant A failed to complete reliably on the iPhone 15 Pro under multi-app memory pressure, and we do not consider unreliable measurements honest enough to publish; for cross-variant comparison we use the R1CS constraint count alone. Verification-cost numbers are taken from Soroban testnet simulation traces.

**Table 3.** End-to-end proof generation latency for Variant B, median (95th percentile), milliseconds.

| Tier | Members | Tree depth | Pixel 8 Pro `R_Membership` | Pixel 8 Pro `R_Update` | iPhone 15 Pro `R_Membership` | iPhone 15 Pro `R_Update` |
|------|---------|------------|----------------------------|------------------------|------------------------------|-------------------------|
| Small | 32 | 5 | 180 (240) | 235 (310) | 165 (220) | 215 (290) |
| Medium | 256 | 8 | 320 (410) | 415 (530) | 290 (380) | 380 (490) |
| Large | 2,048 | 11 | 760 (920) | 985 (1,200) | 690 (840) | 895 (1,090) |

**Proof size and verification cost.** A Groth16 proof is 192 bytes (the three compressed BLS12-381 group elements). Public inputs add 64 bytes for `R_Update` (two 32-byte field elements, with the epoch encoded as one of them). On-chain verification uses three pairings via the Soroban BLS12-381 host functions, plus a small multi-scalar multiplication for the IC linear combination. Total Soroban instruction count is in the 6–10 million range — well within fee budgets for a chat application's per-Commit cost.

**On-chain footprint.** The contract stores per group a 32-byte commitment, a `u64` epoch, a `u32` tier, a `bool` active flag, and an opaque rolling-history window of the last 64 entries. The full audit trail is recoverable from contract events, which Horizon indexers retain without persistent contract storage rent. Deactivation reclaims persistent storage for the rolling window.

**Constant cost as a structural property.** A 2,048-member group costs the same to verify as a 2-member group. This is not an optimisation; it is the property that makes the system usable. Any consumer that has to deal with group-size-dependent gas would lose the ability to make uniform UX promises across tiers.

## 8. Limitations and open problems

**Fee-payer correlation without a relayer.** If a member submits the transaction from their own Stellar account, the fee payer address re-links a Stellar identity to group activity. The relayer pattern in Sec. 3 closes this gap — but a single relayer is a metadata-aggregation vantage point. Future protocol versions of Soroban that support native fee sponsorship would let us decouple invoker and fee payer in a single transaction, removing the relayer's privileged position.

**Group-size leakage from tiers.** The circuit tier reveals an upper bound on group size (32, 256, or 2,048). For most applications this is a very weak signal. For applications where the existence of a 2,000-member group is itself sensitive — e.g., a dissident network — applications should pad to a higher tier than strictly necessary.

**Post-compromise forward secrecy.** As established in our fourth audit, member removal *cannot* be made cryptographically forward-secret using only symmetric key rotation; the removed member already holds the long-lived `groupSecret`. We have implemented an asymmetric per-recipient rekey mechanism (a `SEPRekeyEnvelope` delivered through the per-device inbox, encrypted to each remaining member's BLS public key) that does provide post-removal confidentiality. The mechanism is compatible with MLS's tree-based ratchet but adds an out-of-band step that we have not yet integrated with a stock MLS library.

**Trusted setup.** Each circuit tier requires an independent Powers-of-Tau MPC ceremony for its Groth16 setup. Our reference implementation runs the full Phase-1 protocol (sequential contributions, pairing-based verification, transcript verification) but currently derives the final Groth16 keys on a single machine, which sacrifices the 1-of-N trust property for the derived keys. A production Phase-2 ceremony is a remaining requirement and is the current critical-path item for mainnet rollout.

## 9. Related work

Anonymous credentials and ring signatures are the classical primitives for "I am one of N without revealing which one." Three deployed systems are closest to Onym in construction; Table 4 summarises the comparison.

**Table 4.** Comparison with deployed zero-knowledge group-membership systems.

| System | Anchoring chain | Proof system | Membership scope | Operation-binding | Mobile prover |
|--------|-----------------|--------------|------------------|-------------------|---------------|
| Tornado Cash [5] | Ethereum | Groth16 | Per-deposit anonymity set | Withdrawal address as public input | Browser only |
| Semaphore [4] | Ethereum | Groth16 | Per-application identity tree | External nullifier as public input | Browser, recent mobile support |
| Zcash [6] | Zcash (own L1) | Halo 2 (post-Sapling) | Per-shielded-pool note set | Transaction-data hash as public input | Mobile via lightwallet |
| **Onym (this work)** | **Stellar (Soroban)** | **Groth16 over BLS12-381** | **Per-group fixed-tier tree** | **`R_Update` binds `C_new` as public input** | **iOS / Android, sub-second sub-large tier** |

The structural similarity is unmistakable — all four systems anchor a Merkle tree of commitments on a public ledger and use a SNARK to prove membership without revealing which leaf. Onym differs primarily in scope (a general group-membership registry rather than a single-application primitive), in its tight coupling to MLS group-state transitions, and in the dual-circuit architecture for state transitions that we describe in Sec. 5. Aztec [7] uses PLONK on Ethereum for confidential transactions and is the closest non-Groth16 deployed system; we did not adopt PLONK because Stellar Protocol 22 provides BLS12-381 pairing host functions that directly accelerate Groth16 verification but does not yet provide the polynomial-commitment host functions PLONK would require. Signal's Sealed Sender [8] addresses a related decoupling problem (sender unlinkability) at the messaging layer rather than the registry layer.

## 10. Conclusion

We described Onym, a deployed system that anchors private group membership on the Stellar blockchain using a Poseidon Merkle commitment and a Groth16 zero-knowledge proof. Two design choices — and one transport-layer incident with the same shape — dominated the system's outcome. An 8.5×–14× R1CS constraint reduction from replacing in-circuit SHA-256 with a dual-Poseidon commitment binding made the prover usable on commodity 2024-class smartphones; sub-second proving in the small and medium tiers and sub-1.2-second proving in the large tier are now consistent measurements across both reference platforms. A two-circuit refactor that bound the new commitment as a public input of an $R_\text{Update}$ circuit closed an authorisation-binding gap — a known Groth16 public-input malleability pattern — that a sound theorem and four audit passes had failed to catch, because the proof was correct but for the wrong predicate. A transport-layer co-membership leak from stable Nostr signing keys taught the same lesson at a different layer.

The contract is deployed on Stellar testnet; mainnet rollout is gated on a Phase-2 MPC ceremony and an external audit of the new update circuit. The two cryptographic-engineering practices we extract — count R1CS cost before host-function cost when choosing primitives that straddle the circuit boundary, and write down what each authorising proof actually authorises — are operational checklist items that would each have saved us a postmortem.

## Acknowledgements

We thank the Stellar Development Foundation for Protocol 22's BLS12-381 host functions, without which this system would not be feasible at constant cost, and the arkworks contributors for the Rust Groth16 and Poseidon implementations.

## References

[1] R. Barnes et al., "The Messaging Layer Security (MLS) Protocol," IETF RFC 9420, July 2023.

[2] J. Groth, "On the Size of Pairing-Based Non-Interactive Arguments," in *Proc. EUROCRYPT*, 2016, pp. 305–326.

[3] fiatjaf, "NIP-01: Basic Protocol Flow Description," Nostr Implementation Possibilities, 2023. [Online]. Available: https://github.com/nostr-protocol/nips/blob/master/01.md

[4] Privacy & Scaling Explorations, "Semaphore Protocol Specification v4," 2024. [Online]. Available: https://docs.semaphore.pse.dev/

[5] L. Wang, X. Tang, and S. Meiklejohn, "An Empirical Analysis of Privacy in the Tornado Cash Mixer," in *Proc. ACM CCS*, 2022.

[6] D. Hopwood, S. Bowe, T. Hornby, and N. Wilcox, "Zcash Protocol Specification," version 2024.5.0, Electric Coin Company, 2024.

[7] J. Williamson, "The Aztec Protocol," Aztec Network technical report, 2022.

[8] J. Lund, "Sealed Sender for Signal," Signal blog, October 2018. [Online]. Available: https://signal.org/blog/sealed-sender/

[9] L. Grassi, D. Khovratovich, C. Rechberger, A. Roy, and M. Schofnegger, "Poseidon: A New Hash Function for Zero-Knowledge Proof Systems," in *Proc. USENIX Security*, 2021, pp. 519–535.

[10] P. S. L. M. Barreto, B. Lynn, and M. Scott, "Constructing Elliptic Curves with Prescribed Embedding Degrees," in *Proc. SCN*, 2002, pp. 257–267.

[11] Stellar Development Foundation, "CAP-0046-12: BLS12-381 Host Functions," Stellar Core Advancement Proposal, 2024.

[12] arkworks contributors, "arkworks: An Ecosystem for Developing and Programming with zkSNARKs," 2024. [Online]. Available: https://arkworks.rs

[13] A. Cooper et al., "Privacy Considerations for Internet Protocols," IETF RFC 6973, July 2013.

[14] D. J. Bernstein, N. Duif, T. Lange, P. Schwabe, and B. Yang, "High-Speed High-Security Signatures (Ed25519)," *J. Cryptographic Engineering*, vol. 2, no. 2, pp. 77–89, 2012.

[15] H. Lipmaa, "Simulation-Extractable SNARKs Revisited," Cryptology ePrint Archive, Report 2019/612, 2019. [Online]. Available: https://eprint.iacr.org/2019/612
