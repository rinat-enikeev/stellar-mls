# Postmortem: The Unbound `new_commitment` — A Proof That Authorized the Wrong Statement

**Date:** 2026-04-17
**Severity:** CRITICAL
**Duration of vulnerability:** From the initial circuit design (first commit introducing `MembershipCircuit`) through discovery on 2026-04-17 — approximately the full design lifetime of the project.
**Platforms affected:** All platforms and layers — iOS SDK, Android SDK, HTTP relayer, Soroban contract. The gap is in the protocol, not in any single implementation.
**Status:** Fix implemented and deployed to Stellar testnet (2026-04-18, release v1.3.0, contract `CBKKEZU3CEAXZNJ4RSLDSVCXNJFQK4WMAOGF26SUBT2QUKHWQE2PRFSO`). Phases 0–10 of [implementation-plan-update-circuit-binding.md](implementation-plan-update-circuit-binding.md) are landed on `main`. Mainnet rollout gated on Phase 11 (scripted attack-simulation soak) and external audit sign-off. No exploitation observed.
**Incident status:** Pre-mainnet-traffic discovery. The contract had been deployed only to testnet with synthetic groups at the time the gap was found; no user-owned data was ever at risk under mainnet.

---

## Summary

The `update_commitment` path of the Stellar-MLS contract takes a Groth16 proof
*and* a separate `new_commitment: BytesN<32>` parameter. The proof's public
inputs cover only `(C_old, epoch_old)`. Nothing cryptographically binds the
authenticated operation ("I am a current member, I authorize an epoch
transition") to the specific `new_commitment` that gets persisted on-chain. A
relayer, a Stellar mempool front-runner, or any intermediary between prover
and ledger can swap `new_commitment` for an arbitrary 32-byte value and the
proof still verifies.

The gap was discovered during a code review, not through an incident. The
user inspecting the contract asked a question that — as is often the case
with real security gaps — was easier to ask than to answer: *"Where in the
math is the argument that this proof is non-transferable to a different
`new_commitment`?"* The answer turned out to be: *nowhere*.

This postmortem covers how the gap persisted through design, implementation,
and multiple audit passes; why each layer of review missed it; what we are
doing about it; and — more importantly — what class of oversight it
represents and how we intend to stop repeating it.

The fix, tracked in
[update-circuit-binding-design.md](update-circuit-binding-design.md) and
[implementation-plan-update-circuit-binding.md](implementation-plan-update-circuit-binding.md),
introduces a dedicated `UpdateCircuit` with three public inputs
`(C_old, epoch_old, C_new)`, binding `C_new` via an in-circuit equality
constraint to `Poseidon(Poseidon(root_new, epoch_old + 1), salt_new)`. The existing `MembershipCircuit` is untouched for
the three read-style paths (`create_group`, `verify_membership`,
`deactivate_group`) whose statements really are single-epoch assertions.

The fix shipped on 2026-04-17 as twelve commits on `main` (`2fbde01` … `c58c074`)
and was cut as release v1.3.0 (`d2837d2`) on 2026-04-18. The operation-level
soundness argument — Theorem 3b in
[proof-of-soundness.md](proof-of-soundness.md#6b-theorem-3b-update-circuit-operation-level-soundness)
— is now the canonical security statement for `update_commitment`; the prior
Theorem 3 (membership-only) is retained but explicitly scoped to the three
read-style paths where it is sufficient.

---

## Timeline

All dates in 2026 unless noted. Times America/New_York.

| When | What |
|------|------|
| Project inception | `MembershipCircuit` designed with public inputs `(commitment, epoch)`. The design assumes that "proving membership at the current epoch" is sufficient authorization for every ZK-gated operation. |
| Early implementation | `update_commitment` contract entry point added with `new_commitment: BytesN<32>` as a top-level parameter alongside `proof` and `public_inputs`. No comment or RFC questions why the proof's statement does not cover `new_commitment`. |
| Mid-implementation | Replay protection implemented via `proof_hash` storage (addresses a *different* class of attack — replaying the *same* proof twice). The presence of this mitigation creates the false impression that proof handling is defensively complete. |
| Pre-audit | Comment added at `contracts/sep-xxxx/src/lib.rs:455-458` acknowledging that the proof does not bind the *caller* (N-14 identity-binding limitation). Commitment binding is not mentioned; by implication, it is treated as unproblematic. |
| Audit v1 (`docs/audit-report.md`) | Reviewer traces `update_commitment` and flags three unrelated findings. Nothing on statement/operation divergence. |
| Audit v2 (`docs/audit-report-v2.md`) | Second reviewer, focus on replay and canonical-bytes handling. Adds canonical-bytes check on `commitment` at `:802-808`. Does not extend the same check to `new_commitment`. Still no statement/operation finding. |
| Audit v3 / v4 (`docs/audit-report-v3.md`, `docs/audit-4.md`) | Focus on operational and cryptographic correctness within each individual constraint. All reviewers read the circuit and the contract in isolation; no one holds them side-by-side and asks "do these agree on what the proof is authorizing?" |
| Soundness writeup (`docs/proof-of-soundness.md`) | A knowledge-soundness theorem is proved over the relation $R_{\text{Membership}}(C, e; \text{sk}, \text{salt}, \text{auth\_path})$. The theorem is correct. It is also a theorem about the wrong object — the update operation's security property is a predicate over `(C_old, e_old, C_new, e_new)`, which $R_{\text{Membership}}$ does not mention. The soundness doc gives the wrong statement the full force of a proof. |
| 2026-04-16 | User raises the question during a code review: *"What in the math forces the prover's authorization to pertain to this specific `new_commitment`, not any other 32-byte string an attacker might substitute?"* Initial attempt to point at replay protection, canonical-bytes check, and epoch monotonicity as the answer. Each mitigation is examined; none of them cover the gap. |
| 2026-04-16, ~30 min later | Side-by-side reading of `src/circuit/mod.rs:48-74` (public inputs) and `contracts/sep-xxxx/src/lib.rs:459-523` (contract ABI) confirms: `new_commitment` is a contract parameter, not a public input. The gap is real. |
| 2026-04-16 | First fix plan proposed: add `new_commitment` as a public input to `MembershipCircuit`. Internal critique points out (a) the public input must be *constrained* — an unconstrained allocation contributes `IC_i = 0` and binds nothing, (b) three other call sites share the circuit and would be semantically polluted, (c) `Fr::from_bytes` silently reduces mod-r so a canonical check is required on `new_commitment` too. |
| 2026-04-17 | Initial refined fix proposal: separate `UpdateCircuit` with `(C_old, epoch_old, C_new, epoch_new)` as four public inputs. |
| 2026-04-17 | Further review normalises the design to three public inputs `(C_old, epoch_old, C_new)`, deriving `epoch_new = epoch_old + 1` inside the circuit. This is the form the implementation targets. `MembershipCircuit` unchanged for read paths; canonical-bytes check extended to `new_commitment`. |
| 2026-04-17 | Four documents drafted: vulnerability report, design doc, implementation plan, this postmortem. |
| 2026-04-17 | Phases 0–3 landed on `main` (commits `2fbde01`, `d8b4fe7`, `fe96ea9`, `b5be306`): `UpdateCircuit` module scaffolding, R1CS constraints (1)–(3), Rust prover API (`prove_update`, `UpdatePublicInputs`), and C FFI + JNI surface with the 73-byte wire format and `0x02` version byte. |
| 2026-04-17 | Phase 4 contract rewrite (commit `cd81c09`): `update_commitment` retired the standalone `new_commitment: BytesN<32>` and `new_epoch: u64` parameters; the entry point now takes `(group_id, proof, UpdatePublicInputs)` and verifies against a new per-tier `UpdateVK` via a 4-IC-point / 3-scalar MSM. Canonical-bytes enforcement extended to `c_new`. `initialize` widened from three to six VKs; N-14 comment block rewritten to reflect the new binding. |
| 2026-04-17 | Phase 5 relayer update (commit `f421a12`): top-level `newCommitment` JSON field dropped; `add_public_inputs_arg` split into `_membership` and `_update` variants; version-byte and length gates enforce the v2 wire format on ingress. |
| 2026-04-17 | Phases 6–7 SDK work (commits `5786d02`, `0f49e37`): Swift and Kotlin SDKs mirror `UpdatePublicInputs`, expose `proveUpdate`, and drop `newCommitment` from `SEPUpdateCommitmentRequest`. |
| 2026-04-17 | Phase 8 keyset-v2 artifacts (commit `5c4afa1`): six PK/VK pairs published under `keyset-v2/` (three tiers × two circuits). `MembershipVK.ic.len() == 3`, `UpdateVK.ic.len() == 4`. `keyset-v2/metadata.json` records hashes; keyset-v1 retained and flagged deprecated for forensic access. **This keyset is test-only (single-party local setup); a Phase-2 MPC ceremony over the six `(circuit, tier)` pairs remains required before mainnet.** |
| 2026-04-17 | Phase 9 cross-platform vectors (commit `0c729c6`): `docs/cross-platform-test-vectors.json` bumped to `schemaVersion: 2` with update vectors (positive + rebinding/epoch-mismatch/canonical-bytes negatives) consumed by Rust, Swift, Kotlin, and contract tests. |
| 2026-04-17 | Phase 10 normative docs (commit `c58c074`): `docs/proof-of-soundness.md` gained Theorem 3b (update-circuit operation-level soundness) alongside the original Theorem 3 (membership-only, now explicitly scoped to read paths); `docs/sep.md`, `docs/design-doc.md`, and `docs/relay-design-doc.md` updated to the 3-public-input `R_Update` relation and the new `update_commitment` ABI. |
| 2026-04-18 | Release v1.3.0 cut and deployed to Stellar testnet as contract `CBKKEZU3CEAXZNJ4RSLDSVCXNJFQK4WMAOGF26SUBT2QUKHWQE2PRFSO` (commit `d2837d2`). iOS and Android clients ship keyset-v2 proving keys bundled; `generate_mainnet_vks` and `deploy_sep_xxxx_testnet.sh` pass all six VKs to `initialize`. `clients/vendor/bundle/…` and `keyset-v1/` mobile copies removed in `111bd13` to unblock the release build; follow-up patch `edd4687` released as v1.3.1. |
| 2026-04-18 | Workspace test matrix green on the new wire format: `cargo test --workspace`, `stellar contract build`, `swift test` (`swift-mls/`, `clients/ios/StellarChat/`), `./gradlew test` (`kotlin-mls/`, `clients/android/StellarChat/`). Cross-platform vectors round-trip through all four consumers. |
| Pending | Phase 11 — scripted attack simulation on testnet (substitute `c_new`; expect `InvalidProof`; substitute non-canonical bytes; expect `InvalidCommitmentEncoding`; replay proof against different `group_id`; expect `PublicInputsMismatch`; double-submit; expect `ProofReplay`). Must run green for ≥1 week without regression before mainnet. |
| Pending | Phase 2 MPC ceremony for the six `(circuit, tier)` pairs (Phase 1 powers-of-tau reused from keyset-v1). Current testnet keyset is single-party test-only and must be rotated before any mainnet traffic. |
| Pending | External audit pass scoped specifically to the `UpdateCircuit` relation, the rewritten `update_commitment` entry point, and the operation-level soundness theorem. |
| Pending | Phase 12 — mainnet rollout; public post-mortem announcement cross-linking this document and the vulnerability report. |

---

## What happened

In Groth16 over BLS12-381, the verifier checks a pairing equation that
incorporates a linear combination of "IC" (instance-commitment) points
indexed by the circuit's public inputs. The proof is bound, cryptographically
and non-malleably, to exactly those public inputs — and to nothing else. Any
parameter the verifier reads that does *not* appear in that linear
combination is, by construction, outside the scope of what the proof
authorizes.

The Stellar-MLS protocol's `update_commitment` takes three inputs that the
verifier reads:

- `proof: Groth16Proof` — the zero-knowledge proof bytes.
- `public_inputs: MembershipPublicInputs { commitment, epoch }` — the two
  scalars the proof is bound to.
- `new_commitment: BytesN<32>` — the 32-byte value that will be persisted
  as the next epoch's group commitment.

The third input is where the gap lives. It's read by the contract, written
to storage, but never folded into the proof's IC linear combination. The
prover has no in-circuit way to express "I authorize `new_commitment = X`
and not `new_commitment = Y`". The statement being proved — *"the prover
knows sk such that Poseidon(sk) ∈ tree(root) ∧ C_old = Poseidon(Poseidon(root,
epoch_old), salt_old)"* — is true for any number of future commitments, as
long as the *old* state matches.

In operational terms: a prover signs a blank check. Anyone along the path
from prover to ledger can fill in the `new_commitment` field.

The attack surface is broad:

- The relayer receives `{proof, public_inputs, new_commitment}` as separate
  JSON fields over HTTP. Swapping `new_commitment` is one line of code.
- The Stellar mempool is publicly observable. A mempool-watching attacker
  can intercept a pending `update_commitment` transaction, substitute
  `new_commitment`, and rebroadcast at higher fee.
- Any TLS-terminating intermediary with the ability to modify HTTP bodies
  (a compromised CDN, a malicious Cloudflare Worker, a relay proxy in a
  self-hosted stack) has the same capability.

The fix — binding `C_new` into the circuit as a constrained public input —
closes the gap at the protocol level, not at any single intermediary. The
Groth16 IC vector contributes `IC_3 · C_new` to `vk_x`, so a substituted
`C_new'` makes the pairing equation fail.

---

## Impact

**Production impact: none.** The contract has been deployed only to
Stellar *testnet* with synthetic groups, and the mainnet contract has not
yet been deployed with real user data. No user-owned cryptographic
material, no real group memberships, and no real group histories are at
risk from this vulnerability as of the discovery date.

**Hypothetical mainnet impact (had the gap shipped):**

- **Silent group hijack.** A relayer or mempool front-runner substitutes
  `new_commitment` with one whose pre-image they control. The victim sees
  a valid on-chain event matching their epoch expectations. Behind the
  scenes, control of the group has silently transferred to the attacker.
- **Group bricking.** `new_commitment` is substituted with random bytes.
  Every subsequent `update_commitment` fails because no honest prover
  can produce a proof whose `C_old` matches the junk commitment now in
  storage. The group is a soft-brick: still present on-chain, but
  incapable of further evolution. Off-chain, members can continue
  chatting, but the canonicalized on-chain membership record is
  corrupted and un-auditable.
- **History forking.** Two different `update_commitment` requests sharing
  the same proof but different `new_commitment` values are both valid.
  Whichever one lands first wins; the other's view of the group history
  is corrupted.
- **DoS via race.** An attacker races every honest update with their own
  substituted version, fee-bumping to guarantee they land first. Honest
  updates become un-performable because the on-chain state is
  consistently in the attacker's preferred shape.

**Testnet impact:** During our testnet soak, no attacker performed any of
these attacks. The testnet environment does not have the adversarial
population of mainnet, so the absence of exploitation should not be read
as evidence that the gap was hard to exploit.

**Scope of data loss:** none. Because the contract is pre-mainnet, there
is no user data to lose.

---

## Detection

The gap was not detected by any automated tool, test, or audit pass. It was
detected by a single question from the user:

> "Where in the math is the argument that this proof is non-transferable to
> a different `new_commitment`? I see replay protection against the same
> proof being submitted twice. I see canonical-bytes enforcement on the
> stored `commitment`. I see an epoch monotonicity check. But none of those
> mean the proof authorizes *this specific* `new_commitment` rather than
> some other value an attacker might substitute."

The key cognitive move in the question: decomposing the existing mitigations
and asking which specific attack each one prevents. Replay protection
prevents reuse of the *same* `(proof, public_inputs)` tuple; it says nothing
about a modified tuple. Canonical-bytes enforcement prevents non-canonical
Fr encodings from being accepted by `Fr::from_bytes`; it says nothing about
the cryptographic binding of the proof to the stored value. Epoch
monotonicity prevents history rewind; it says nothing about what the new
epoch's commitment actually is.

Once each mitigation was held up to the specific attack it prevents, none
of them covered the attack of *"substitute `new_commitment` while keeping
`(proof, commitment_old, epoch_old)` identical"*. Code inspection of
`src/circuit/mod.rs:48-74` (two public inputs: `commitment`, `epoch`) and
`contracts/sep-xxxx/src/lib.rs:459-523` (three distinct arguments: `proof`,
`public_inputs`, `new_commitment`) confirmed the gap within minutes.

The question was asked during review of an unrelated feature. It was not
provoked by any prior alert, red-team exercise, or audit prompt.

---

## Root cause analysis

Five independent choices, each individually defensible, combined to produce a
critical gap that persisted through multiple review passes.

### 1. Circuit statement covered the wrong relation

`MembershipCircuit` proves the relation:

$$R_{\text{Membership}}(C, e;\ \text{sk}, \text{salt}, \text{auth\_path}) = 1$$

— that is, "the prover knows a secret key whose Poseidon hash is a leaf in
a tree whose canonicalized root, composed with `epoch`, hashes to `C`".

For `create_group`, `verify_membership`, and `deactivate_group`, this is the
right relation. Each of those operations genuinely *is* a single-epoch
membership assertion. The verifier's job is "convince me a qualifying member
is making this call"; nothing else needs binding.

For `update_commitment`, the operation is *not* a single-epoch assertion. It
is a **transition** from state $(C_{old}, e_{old})$ to state $(C_{new},
e_{new})$. The security property the operation must satisfy is a predicate
over all four of those values — specifically, that a qualifying current
member, and only a qualifying current member, may choose the next state.

The circuit was designed for the first interpretation and applied to both.
The four call sites share a verifier, and the shared verifier only knows
how to ask the first question.

### 2. ABI coupled `new_commitment` as a separate function parameter

`contracts/sep-xxxx/src/lib.rs:459-523` accepts `new_commitment: BytesN<32>`
alongside `proof: Groth16Proof` and `public_inputs: MembershipPublicInputs`.

From a type-system perspective, this is uncontroversial: `new_commitment`
is "what will be written" and the others are "what authorizes the write".
But the natural reader assumption — *"if I see a proof and I see
public_inputs, then public_inputs are everything the proof commits to"* —
is silently violated. The ABI lets a reader infer a binding that is not
actually present.

A safer ABI design would have been: `update_commitment(group_id, proof,
public_inputs)` with `new_commitment` embedded in `public_inputs`. The
compiler cannot enforce that the struct fields map to circuit public
inputs, but it would at least have forced every human reader to trace one
less layer.

### 3. Soundness theorem was correct about the wrong statement

`docs/proof-of-soundness.md:257-259` contains a correct knowledge-soundness
proof for the relation $R_{\text{Membership}}$. The theorem is not wrong.
What is wrong is the choice of object.

The soundness property that the `update_commitment` *operation* requires
is:

> For all PPT adversaries producing a tuple $(\pi, \text{pi})$ accepted
> by the `update_commitment` verifier, where
> $\text{pi} = (C_{old}, e_{old}, C_{new})$, there exists an extractor
> producing a witness $(\text{sk}, \text{root}_{old}, \text{salt}_{old},
> \text{auth\_path}, \text{root}_{new}, \text{salt}_{new})$ satisfying
> constraints (1)–(3) of $R_{\text{Update}}$ over that exact $\text{pi}$,
> except with negligible probability.

A ZK proof cannot bind subjective intent; it can only bind a concrete
public statement. The right formal claim is that a proof accepted for
one public tuple does not carry over to a different public tuple
without a fresh witness. That is the cryptographic guarantee; the
operational guarantee ("the right party authorized this specific
`C_new`") follows from the fact that `C_new` is now in the public
statement and the prover must have known the witness at proving time.

Proving knowledge soundness for the two-input relation and calling the
job done is a classic logical error: asserting the existence of a proof
of $A$ implies the existence of a proof of $A \land B$, which is only
true if $B$ is already implied by $A$ — which here it is not.

The soundness doc's own structural guarantee lulled reviewers. "The math
is correct" was, in a trivial sense, true — it's just that the math
pertained to the wrong statement.

### 4. Audit reports focused on constraint-level correctness

Audits v1–v4 (`docs/audit-report.md`, `docs/audit-report-v2.md`,
`docs/audit-report-v3.md`, `docs/audit-4.md`) each did excellent work within
their chosen frame: verifying that each constraint in the circuit was
correctly expressed, that each mitigation in the contract (replay,
canonical bytes, epoch monotonicity) was correctly implemented, that each
datatype's domain separation was well-chosen.

None of the audits held "what does the circuit prove?" and "what does the
operation need?" side-by-side and asked whether they matched.

This is a standard limitation of audits that focus on individual
vulnerabilities rather than on scope alignment. A vulnerability-focused
audit asks: *"given this code, what can go wrong?"* A scope-alignment
audit asks: *"given this operation's security goal, what must the code
be proving?"* The latter was not performed on this system. Had it been,
the first question would have been "what is `update_commitment`
authorizing?", and the first answer would have been "the proof doesn't
say".

### 5. Inline acknowledgment of one binding gap hid a deeper binding gap

At `contracts/sep-xxxx/src/lib.rs:455-458`, a comment notes that the proof
does not bind the *caller identity* — the N-14 finding from the audit
series. This comment is accurate: the prover does not sign the Stellar
transaction envelope, so the proof and the fee-payer are decoupled by
design (this is intentional — it's how the relayer pattern works).

The comment's **presence** created a subtle false-negative. A reader
encountering it naturally infers "this is the single known binding
limitation; all other bindings are present". The actual situation is the
inverse: the caller-identity gap is the *minor* known limitation, and the
commitment-binding gap underneath it is the *major* unknown one.

Comments that name one class of limitation can suppress the question "what
other classes of limitation are there?". In retrospect, a more defensive
comment would have been: *"The proof binds (C_old, epoch_old). It does
NOT bind: caller identity, new_commitment, fee-payer, transaction
envelope. Each of these may or may not be a security issue depending on
the deployment model — enumerate before shipping."*

### 6. The defect class is structurally unique to `update_commitment`

Of the three contract entry points that consume a `Groth16Proof`
(`create_group`, `update_commitment`, `deactivate_group`), only
`update_commitment` persists a state value that is not itself a public
input of the proof. `create_group` (`contracts/sep-xxxx/src/lib.rs:384,
:408`) stores the same `commitment` the proof verifies against — the
equality is enforced at `:384` and the MSM at `:408` uses the same
value, so there is no "new commitment outside the proof" to rebind.
`deactivate_group` (`:568-632`) stores no new commitment at all; the
state transition is a boolean flag flip on the already-verified current
entry, introducing no attacker-controllable bytes.

The structural asymmetry is visible only when the three call sites are
compared side-by-side with one concrete question held fixed: *which
persisted bytes are covered by the proof's public inputs, and which are
not?* None of the prior reviews posed that question explicitly across
all three sites. A future audit methodology that tabulates, per
Groth16-verifying entry point, the set difference
`{persisted bytes} \ {public inputs covered by the proof}` would have
surfaced this instance on the first pass and would surface any future
instance of the same class on its first pass. The long-term prevention
item below proposes encoding this check in CI so the question is asked
by the build, not left to the reviewer's initiative.

---

## Why it was easy to miss

- **Each file reads correctly on its own.** The circuit is a correct
  membership circuit. The contract is a correct storage updater. The
  soundness doc is a correct proof. The gap is only visible when you
  compare the circuit's public inputs against the contract's function
  parameters and notice the set difference.
- **Existing mitigations looked load-bearing.** Replay protection, epoch
  monotonicity, and canonical-bytes enforcement all sound like they
  should cover proof-level concerns. A reader encountering these three
  features could reasonably conclude that "proof security" was a
  well-addressed category.
- **Soundness-doc framing created the wrong mental model.** A theorem
  saying "this proof is sound" strongly implies "this proof secures the
  operation". The distinction between the soundness of a relation and
  the security of an operation is a subtle one; the doc did not foreground
  it.
- **Audits inherited the framing.** Each audit built on the prior, which
  meant each audit inherited the (implicit) framing that proof-level
  soundness was settled. None of them reopened the question.
- **Nobody wrote the attack.** It's common for red-team exercises to
  surface binding gaps by force: "give me a proof from user A and see if
  I can reuse it for user B's action". The Stellar-MLS project had no
  such exercise prior to the discovery.

---

## Lessons learned

### 1. A proof's statement must be an explicit, enumerated contract

Every ZK-gated operation must have a written down, signed-off contract
that lists:

- Every field the proof binds to (appears in public inputs).
- Every field the verifier reads but the proof does not bind to (and why
  that is safe).
- Every field the operation depends on (i.e., affects the security
  property) that neither the proof binds to nor the verifier reads.

If the third list is non-empty, that's a vulnerability candidate — unless
each entry has a documented reason it doesn't need binding (for instance,
the caller's IP address, which cannot affect any on-chain security
property). The unbound `new_commitment` was in the third list; no one
noticed because the list was never written down.

### 2. Soundness theorems must be stated over operations, not circuits

A theorem of the form "the circuit $C$ with relation $R$ is knowledge-sound"
is necessary but not sufficient. The operational theorem must be of the
form "the operation $op$ with security property $P$ is authorized by a
valid proof for $R$ if and only if $P$ holds". The operational theorem is
strictly stronger because it forces the theorem's author to match up the
predicate over which the relation is defined against the predicate the
operation's security property ranges over.

Going forward, every ZK-gated operation in Stellar-MLS gets one
operational soundness theorem, not one relation-level theorem. The
`docs/proof-of-soundness.md` doc will be restructured along these lines.

### 3. Audits need an explicit scope-alignment pass

Audits will gain a new rubric item: *"For each ZK-gated function, list the
public inputs of its circuit and the contract parameters read by its
verifier. If any contract parameter is not either in the public inputs or
documented as deliberately unbound, flag it."*

This check is mechanical and could be partially automated (a linter
walking each `Groth16Proof`-consuming contract function and reporting its
non-public-input `BytesN<32>` and `u64` parameters). It would have caught
this vulnerability on the first run.

### 4. When a design doc disclaims one kind of binding, the next question is "what else?"

The inline N-14 comment at `:455-458` disclaimed caller binding. The
lesson is to interpret such a disclaimer as a cue, not a completion: if
*one* binding is being called out as absent, a structured enumeration of
*all* bindings — present and absent — is warranted. The question "what
else might be unbound?" is the correct response to any "this particular
thing is unbound" comment.

### 5. Existing mitigations can mask absent ones

Replay, canonical-bytes, and monotonicity checks all contribute real
security value. Their combined presence gave the impression that
proof-level handling was defensively complete. In hindsight, none of them
individually — and none of their union — addressed the specific attack of
substituting a binding-adjacent parameter.

Future design reviews will explicitly decompose each mitigation by the
attack it prevents, and will not allow "there are multiple mitigations in
this area" to substitute for "there is a mitigation for each specific
attack in this area".

---

## Work completed

Concrete deliverables landed on `main` between 2026-04-17 and 2026-04-18.
Line-level references are as of release v1.3.0 (`d2837d2`); later commits
may have shifted offsets.

### Circuit and prover (Phases 0–3)

- **New module** `src/circuit/update.rs` defines `UpdateCircuit<F>` with the
  three public inputs `(c_old, epoch_old, c_new)` pinned to allocation order
  and the three R1CS constraints from [update-circuit-binding-design.md §5.2](update-circuit-binding-design.md).
  Constraint (3) is the specific gadget whose absence produced the
  vulnerability:
  `c_new == Poseidon(Poseidon(root_new, epoch_old + 1), salt_new)`,
  with `epoch_old + 1` computed inside the circuit via an `FpVar` linear
  combination so `epoch_new` does not appear in the wire format.
- `MembershipCircuit` is untouched. Its `ic.len() == 3` and its statement
  semantics are unchanged for `create_group`, `verify_membership`, and
  `deactivate_group`.
- **Prover API** `src/prover.rs::prove_update` takes high-level inputs
  (secret key, old/new canonically-ordered member lists, old epoch, both
  salts, tier) and returns a 192-byte compressed Groth16 proof plus a
  73-byte `UpdatePublicInputs` blob. `UpdatePublicInputs` carries
  `VERSION = 0x02` as its leading byte; `SERIALIZED_LEN == 73`
  (1 + 32 + 8 + 32).
- **C FFI / JNI** `src/ffi.rs::sep_prove_update` and
  `src/jni_ffi.rs::Java_…_sepProveUpdate` expose the new path with
  explicit buffer-size validation (`UpdateProofBufferTooSmall`) and
  version-byte rejection (`UpdateProofVersionMismatch`).
  `include/stellar_mls.h` declares
  `STELLAR_MLS_PUBLIC_INPUTS_VERSION == 2` and
  `STELLAR_MLS_UPDATE_PUBLIC_INPUTS_LEN == 73`.

### Contract (Phase 4)

- `contracts/sep-xxxx/src/lib.rs` retires the vulnerable signature. The new
  entry point is `update_commitment(env, group_id, proof, public_inputs:
  UpdatePublicInputs)` — three arguments, not five. The stand-alone
  `new_commitment: BytesN<32>` and `new_epoch: u64` parameters no longer
  exist at the ABI; `git grep -n 'new_epoch: u64' contracts/` returns
  nothing.
- A new `VkKind` enum (`Membership | Update`) and a new `DataKey::UpdateVK(u32)`
  storage slot separate the two verification keys per tier. `initialize`
  widened from three to six VKs; `update_vk(kind, tier, new_vk)` performs
  kind-aware rotation.
- Helper `verify_groth16_proof_update` performs canonical-bytes enforcement
  on `c_old` and `c_new` before MSM, loads `UpdateVK(tier)`, asserts
  `ic.len() == 4`, and computes
  `vk_x = ic[0] + c_old · ic[1] + epoch_old · ic[2] + c_new · ic[3]`.
  Substituting `c_new` in the envelope perturbs `vk_x` and the pairing
  equation fails — closing the gap at the cryptographic layer.
- New error discriminants `InvalidCommitmentEncoding = 15` and
  `UnknownVkKind = 16` cover the canonical-bytes rejection and the
  kind-aware VK routing respectively.
- Replay check ordering preserved: `check_proof_replay` runs before the
  pairing computation, `record_proof` runs after a successful verification
  and before state writes.
- The N-14 comment block was rewritten to make the scope of the
  intentional caller-unbinding explicit and to flag that `new_commitment`
  is **now** cryptographically bound.

### Relayer and SDKs (Phases 5–7)

- `relayer/src/handler.rs` dropped the top-level `newCommitment` request
  field. `add_public_inputs_arg` split into `_membership` and `_update`
  variants so each call site's expectation is explicit at the type level.
  Incoming bodies are rejected with HTTP 400 if the leading byte is not
  `0x02` or if the length is not exactly 73 bytes.
- Swift SDK (`swift-mls/`) and the iOS client (`clients/ios/StellarChat/`)
  gained `UpdatePublicInputs`, a high-level `Prover.proveUpdate(…)`, and
  a version-skew-safe deserialiser. `SEPUpdateCommitmentRequest` no longer
  carries `newCommitment`.
- Kotlin SDK (`kotlin-mls/`) and the Android client
  (`clients/android/StellarChat/`) mirror the Swift surface via JNI.

### Trusted setup, vectors, and docs (Phases 8–10)

- `keyset-v2/` ships six (circuit, tier) PK/VK pairs plus
  `metadata.json` recording hashes and `icLen` per pair
  (`membership: 3`, `update: 4`). Mobile clients bundle the test-only keys
  in `clients/android/StellarChat/app/src/main/assets/keyset-v2/` and the
  corresponding iOS bundle. Keyset-v1 retained for forensic access and
  marked deprecated.
- `docs/cross-platform-test-vectors.json` bumped to `schemaVersion: 2`
  with update-path vectors (positive + `rebinding_attack`,
  `non_canonical_c_new`, `epoch_mismatch` negatives) consumed by the
  Rust verifier, Swift `Prover`, Kotlin `Prover`, and the Soroban
  contract. Every `publicInputs` string is exactly 146 hex chars.
- `docs/proof-of-soundness.md` now contains **Theorem 3b
  (Update-Circuit Operation-Level Soundness)**, formally restating the
  soundness property over `(C_old, e_old, C_new)` and proving it by a
  six-step argument: Groth16 acceptance → witness extraction →
  new-commitment binding via constraint (3) → epoch-increment binding
  via the in-circuit `+1` → canonical-bytes foreclosure → contract-side
  state-freshness cross-check. The prior Theorem 3 is retained and
  explicitly scoped to the three read-style paths where the
  single-epoch statement is sufficient.
- `docs/sep.md`, `docs/design-doc.md`, `docs/relay-design-doc.md`, and
  `docs/real-world-gap-analysis.md` updated to the 3-public-input
  `R_Update` relation and the v2 wire format.

### Testnet rollout (partial Phase 11)

- Release v1.3.0 (`d2837d2`, 2026-04-18) deployed as contract
  `CBKKEZU3CEAXZNJ4RSLDSVCXNJFQK4WMAOGF26SUBT2QUKHWQE2PRFSO` on Stellar
  testnet. `scripts/deploy_sep_xxxx_testnet.sh` and
  `src/bin/generate_contract_testnet_fixtures.rs` were extended to emit
  six VKs and drive the new ABI; the smoke script performs
  `create_group → verify_membership → update_commitment → get_state →
  verify_membership → deactivate_group → get_history` end-to-end against
  the live deployment.

### What is not yet done

- **Phase 11 attack simulation** (`tests/e2e/attack_simulation.rs`) is not
  yet wired in. The four assertions — `InvalidProof` under `c_new`
  substitution, `InvalidCommitmentEncoding` under non-canonical bytes,
  `PublicInputsMismatch` under cross-group replay, `ProofReplay` under
  double-submission — need to be expressed as a reproducible test and
  soaked for ≥1 week against testnet.
- **Phase-2 MPC ceremony.** The current keyset-v2 is a deterministic
  single-party local setup suitable for testnet integration only. The
  six `(circuit, tier)` pairs must be re-contributed through a
  multi-party ceremony before any mainnet traffic.
- **External audit.** The rewritten `update_commitment`, the
  `UpdateCircuit` constraints, and Theorem 3b need an independent
  sign-off before mainnet.
- **Mainnet deployment (Phase 12)** and the public announcement cross-linked
  to this post-mortem are gated on all three of the above.

---

## Prevention

### Immediate (landed)

- ✅ Fix landed per
  [update-circuit-binding-design.md](update-circuit-binding-design.md)
  and
  [implementation-plan-update-circuit-binding.md](implementation-plan-update-circuit-binding.md).
  Phases 0–10 shipped on 2026-04-17; release v1.3.0 on 2026-04-18 deployed
  to testnet (`CBKKEZU3CEAXZNJ4RSLDSVCXNJFQK4WMAOGF26SUBT2QUKHWQE2PRFSO`).
- ✅ Vulnerability report published
  ([vuln-unbound-new-commitment.md](vuln-unbound-new-commitment.md)).
- ✅ Operation-level soundness theorem added
  ([proof-of-soundness.md §6b](proof-of-soundness.md#6b-theorem-3b-update-circuit-operation-level-soundness)).

### Immediate (outstanding)

- Phase-2 MPC ceremony for the six `(circuit, tier)` pairs.
- Testnet-deploy and run the scripted attack simulation end-to-end (Phase 11 in `tests/e2e/attack_simulation.rs`).

### Short-term (weeks to months)

- **Audit every ZK-gated contract entry point for the same class of gap.**
  For each function that consumes a `Groth16Proof`, list every contract
  parameter it reads, cross-reference against the corresponding
  `PublicInputs` struct, and verify that every parameter with
  security-relevant effect is present in the struct. Start with
  `create_group`, `verify_membership`, and `deactivate_group`.
- **Introduce a CI lint.** Write a static check that, for every contract
  function signature `fn X(… proof: Groth16Proof, … public_inputs: P,
  …)`, enumerates the remaining parameters of types `BytesN<32>`, `u64`,
  `Vec<u8>`, and fails CI if any of them is not either explicitly
  whitelisted in a comment or present as a field of `P`. This is a
  conservative check that will produce false positives, but false
  positives are safe.
- **Restructure `docs/proof-of-soundness.md`.** One theorem per
  *operation*, not per circuit. Retire the prior Theorem 3 as "superseded
  by operation-level theorems 3a, 3b, 3c" (one per read path) plus a new
  Theorem 4 for the update operation.
- **Add a "binding contract" preamble to every ZK-gated function.**
  A short doc-comment block listing (bound, read-but-unbound,
  deliberately-unbound) fields for the function, as a reading aid for
  reviewers and auditors.

### Long-term (months to quarters)

- **Formal-methods investment.** The gap would have been caught by a
  formal specification that required the relation's public inputs to
  equal the operation's security-property domain, with a discharge
  obligation per parameter read by the contract. Investigate
  lightweight formal specs (Alloy, Rocq, Lean) for the ZK layer.
- **Red-team every new ZK-gated operation before rollout.** Specifically
  require a "proof transferability" exercise: given a valid proof,
  attempt to reuse it against a modified operation. A successful
  transferability attempt is a blocker.
- **Documented threat model per operation.** Each operation ships with a
  written threat model listing the adversary, the adversary's
  capabilities (network, relayer, mempool, fee-payer, co-member), and
  the specific attack each capability enables or does not enable. This
  is more work than current practice but is the right bar for a protocol
  handling on-chain commitments.
- **Separate "binding audit" from "implementation audit".** Future audit
  engagements will have two deliverables: "does each implementation
  match its spec?" and "does each spec match its operational goal?".
  Conflating the two was a significant factor in this gap persisting
  through four audit passes.

---

## Related documents

- [vuln-unbound-new-commitment.md](vuln-unbound-new-commitment.md) — the
  vulnerability report.
- [update-circuit-binding-design.md](update-circuit-binding-design.md) — the
  design for the fix.
- [implementation-plan-update-circuit-binding.md](implementation-plan-update-circuit-binding.md) —
  the phased execution plan for rollout.
- [proof-of-soundness.md](proof-of-soundness.md) — to be restructured
  (Phase 10 of the implementation plan) with operation-level theorems.
- [audit-critical.md](audit-critical.md) — prior audit findings (this gap
  was not among them; lesson for future audit scoping).
- [postmortem-secure-member-removal.md](postmortem-secure-member-removal.md) —
  style model for this document.

---

## Appendix A — Annotated reproduction

Full attacker walk-through, suitable for training and future audit drills.

### Setup

- A group exists on-chain with state `(C_old, epoch_old = 5)`.
- Alice is a current member with secret key `sk_A` and valid auth path.
- Alice wants to add Bob. She computes `members_new = members_old ∪ {Bob}`,
  canonicalizes, computes `root_new`, picks `salt_new`, derives
  `C_new = Poseidon(Poseidon(root_new, 6), salt_new)`.
- Mallory is a malicious relayer (or mempool watcher; the scenarios are
  isomorphic).

### Alice's proof generation (unchanged between v1 and v2)

Alice invokes her SDK's update path, which calls `prove_update` (in the
fixed system) or `prove_membership` (in the vulnerable system). The proof
generation produces a Groth16 proof whose public inputs cover `C_old` and
`epoch_old = 5`.

### HTTP request from Alice to relayer (v1, vulnerable)

```json
POST /update_commitment HTTP/1.1
Content-Type: application/json

{
  "group_id":       "abcdef...",
  "proof":          "<192 bytes, hex>",
  "public_inputs": { "commitment": "<C_old, hex>", "epoch": 5 },
  "new_commitment": "<C_new, hex>"
}
```

Mallory intercepts this request. She computes her own `C_new_mallory`
(she chose `salt_mallory` and `root_mallory` under her control). She
rewrites the request body:

```json
{
  "group_id":       "abcdef...",
  "proof":          "<same 192 bytes, hex>",
  "public_inputs": { "commitment": "<C_old, hex>", "epoch": 5 },
  "new_commitment": "<C_new_mallory, hex>"
}
```

She forwards to Soroban. The contract's
`verify_groth16_proof(proof, public_inputs)` call succeeds — the proof
is bound only to `C_old` and `epoch = 5`, which are unchanged. The
contract writes `new_commitment = C_new_mallory` to storage.

### Result under v1 (vulnerable)

- On-chain state is now `(C_new_mallory, epoch = 6)`.
- Alice's SDK sees a valid `update_commitment` event at epoch 6 and
  continues. She does not notice that the on-chain commitment is her
  substitution rather than her computation, because the SDK does not
  verify the on-chain commitment against the one it computed locally
  (and even if it did, the next operation Alice attempts would fail,
  but by that point Mallory has already established her position).
- Subsequent updates require a valid proof against `C_new_mallory`.
  Only Mallory, who knows `salt_mallory` and `root_mallory`, can
  produce this. The group has been hijacked.

### Result under v2 (fixed)

The HTTP request shape changes — `new_commitment` is no longer a
top-level field; it lives inside `public_inputs`:

```json
{
  "group_id": "abcdef...",
  "proof":    "<192 bytes, hex>",
  "public_inputs": {
    "version":   2,
    "c_old":     "<C_old, hex>",
    "epoch_old": 5,
    "c_new":     "<C_new, hex>"
  }
}
```

`epoch_new` is not transmitted — the contract derives it as
`current.epoch + 1`, and the circuit binds `C_new` to
`Poseidon(Poseidon(root_new, epoch_old + 1), salt_new)` internally.

If Mallory substitutes `c_new`, the modified `public_inputs` produces a
different `vk_x` in the pairing equation's linear combination (three
public inputs, four IC points):

$$\text{vk\_x} = IC_0 + C_{\text{old}} \cdot IC_1 + e_{\text{old}} \cdot IC_2 + C_{\text{new,Mallory}} \cdot IC_3$$

The pairing check fails. The contract rejects with `InvalidProof`.
Attack neutralized at the cryptographic layer, independent of any
intermediary's trust level.

### What the fix does not neutralize

- Mallory can still refuse to forward the request at all (liveness
  attack, not a confidentiality or integrity attack).
- Mallory can still substitute `group_id` and the contract will reject
  with `GroupNotFound` or proof failure — which is a visible-to-the-user
  failure, not a silent hijack.
- Mallory can still try to replay a past proof; this is already
  prevented by the existing `proof_hash` replay protection.

These are known, documented, and either acceptable (liveness) or
mitigated (replay).

---

## Appendix B — Queries that would have caught the bug

The following would-have-caught queries are included as audit
artifacts — each is a specific check that, had it been run on the
vulnerable codebase, would have surfaced the gap.

### Code-level

1. *"List every contract function that takes both a `Groth16Proof` and a
   `BytesN<32>` parameter. For each, enumerate which `BytesN<32>`
   parameters are named in the associated `PublicInputs` struct."*
   `update_commitment` would have come out as: takes
   `Groth16Proof`, `MembershipPublicInputs { commitment, epoch }`, and a
   stand-alone `new_commitment: BytesN<32>` — and `new_commitment` is
   not in the struct. Flag.
2. *"Diff each contract function's parameter list against its
   corresponding circuit's public-input allocation order."* Would have
   surfaced that `MembershipCircuit` has two public inputs while
   `update_commitment` takes three security-critical values.
3. *"Grep for every field of `BytesN<32>` that is written to persistent
   storage by a contract function. For each, trace whether the written
   value came from `public_inputs` or from a separate parameter."*
   Would have surfaced that `update_commitment` writes a value
   (`new_commitment`) that did not come from `public_inputs`.

### Specification-level

4. *"For each operation, write out the operational security property as a
   predicate over the operation's inputs. For each input in the
   predicate, show where it appears in the circuit's relation."* Would
   have surfaced that `C_new` appears in the security property (the
   operation authorizes a specific transition, not an arbitrary one)
   but not in the relation.
5. *"Write, for each ZK-gated operation, a single sentence of the form
   'Given a valid proof, an adversary can produce what alternative
   operations without producing a new proof?'"* For `update_commitment`,
   the honest answer would have been: *"An adversary can produce any
   `update_commitment` call with any `new_commitment` as long as the
   `(commitment_old, epoch_old)` fields are unchanged."* That sentence,
   written down, is the vulnerability.

### Red-team-level

6. *"Given a proof and public inputs produced by an honest prover,
   enumerate the set of contract function calls that would still
   verify if you modify any other parameter."* A five-minute exercise
   that catches this class of gap directly.
7. *"For each mitigation in the contract (replay, canonical bytes, epoch
   monotonicity), write down the specific attack each one prevents.
   Then list the attacks that are not covered by any of them."* Would
   have surfaced "substitute `new_commitment` while reusing proof" as
   uncovered.

All seven queries are cheap. None of them require formal methods,
machine-checked proofs, or expensive red-team engagement. Running any
one of them against the pre-fix codebase would have surfaced the gap.
None were run.
