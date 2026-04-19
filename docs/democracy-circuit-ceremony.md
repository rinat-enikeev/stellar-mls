## Preamble

```
Title: Democracy Circuit VK — Ceremony & On-Chain Rollout Plan
Author: @rinat-enikeev
Status: Draft (tracking Phase D of the governance rollout)
Created: 2026-04-19
Extends: docs/group-governance-types-design.md §6.4.2,
         docs/trusted-setup-ceremony-phase1-coordinator-playbook.md,
         docs/trusted-setup-ceremony-phase2-participant-playbook.md
```

Companion to `src/circuit/democracy.rs`. The client-side governance journey
(Anarchy / 1v1 / Democracy / Oligarchy) ships in v1.6 with **client-only**
Democracy enforcement: ballots are collected over the chat transport, tallied
locally, and finalised by broadcasting a standard `update_commitment` backed
by the existing anarchy-equivalent `UpdateCircuit`. This is the quorum-bypass
gap named in the governance design doc §3.2: a malicious single member can
still finalise a removal by bypassing the UI and calling `update_commitment`
directly.

This document is the rollout plan that closes that gap by deploying the
`DemocracyUpdateCircuit` VK and flipping the contract dispatcher to require
it for `group_type = 2`.

---

## 1. Scope & non-goals

In scope:

- Compile `DemocracyUpdateCircuit` for the v1 tier × K_max matrix:
  `(tier=0, K_max=32)` and `(tier=1, K_max=256)`.
- Run a Phase 2 trusted-setup ceremony per compiled pair, reusing the
  existing Phase 1 powers-of-tau transcript already published for the
  membership/update circuits.
- Publish verifying keys to the contract via
  `DataKey::UpdateVKByType(tier, 2)`.
- Flip the `update_commitment` dispatcher so `group_type = 2` routes to the
  new VK (the scaffolding for that dispatcher already exists in the contract
  per `contracts/sep-xxxx/src/lib.rs §6.3`).
- Backfill a migration path for existing Democracy groups created in v1.6:
  they continue to accept Anarchy-shaped updates until the VK lands, then
  flip to require the Democracy VK at the next epoch.

Out of scope for this doc:

- Tier 2 (`K_max` beyond 256) — deferred per §6.4.2.
- A new Phase 1 transcript. Democracy reuses the existing `phase1-final.ptau`.
- Threshold changes (configurable quorum other than ≥50%).

---

## 2. Constraint body: what the ceremony signs over

The ceremony VK is only as meaningful as the constraint system it binds. The
current `src/circuit/democracy.rs` ships as a skeleton — field layout,
witness shape, and public-input order are fixed, but `generate_constraints`
returns `SynthesisError::AssignmentMissing`. **This must land before the
ceremony, not after.** A Phase 2 contribution over a placeholder circuit
produces a VK that cannot be rebound to the real constraint set.

Implementation steps, in order:

1. Port the Merkle-opening and Poseidon-commitment gadgets from
   `src/circuit/update.rs` — they are the same primitives.
2. Add the strict-ascending index check (constraint #3). The cheapest
   implementation is `leaf_idx_{i+1} - leaf_idx_i - 1 ≥ 0` via the existing
   non-negative range-check gadget.
3. Add the threshold check `2·K ≥ member_count_old` (constraint #4). `K` is
   not a field element; it is encoded as the prefix length of the `signers`
   vector. The circuit must either zero-pad to `K_max` and expose a public
   `k` input, or use a selector vector. v1 uses the zero-pad approach to
   keep the public-input schedule fixed.
4. Add the single-leaf-delta gadget (constraint #6) using
   `DemocracyDelta` as a tagged union. Each variant expands to a distinct
   sub-circuit selected by a boolean trio; unused branches are constrained
   to zero.
5. Wire up the commitment binders for `c_old` and `c_new`.
6. Add property tests under `src/circuit/democracy.rs` mirroring the cases
   listed in `docs/group-governance-types-design.md §7` (Democracy).

Only after step 6 passes should Phase 2 begin.

---

## 3. Phase 2 ceremony — per-pair workflow

The existing playbook (`docs/trusted-setup-ceremony-phase2-participant-playbook.md`)
covers the operational shape. What differs for the Democracy circuits is
**what gets signed** and **where the artifacts land**. For each of the two
tier×K_max pairs:

```
phase1-final.ptau
  └─> ceremony-tool compile democracy.r1cs  (per tier, per K_max)
      └─> phase2-init-<tier>-<kmax>.mpc
          └─> rotate through ≥ 5 participants
              └─> phase2-final-<tier>-<kmax>.mpc
                  └─> ceremony-tool export-vk
                      └─> vk-democracy-<tier>-<kmax>.json  (bytes-for-bytes stable)
```

Coordinator checklist:

- Participants cover ≥ 3 independent jurisdictions and ≥ 2 independent
  hardware lineages. The membership ceremony's participant set is a
  reasonable starting pool but **must not be reused wholesale** — each
  ceremony needs a fresh quorum to preserve the "only one honest
  participant" soundness argument.
- Each participant publishes the SHA-256 of their contribution transcript
  in the coordinator thread within 24 hours of their turn.
- The final transcript is re-verified by `ceremony-tool verify` by at least
  two independent verifiers before VK export.

### 3.1 VK domain separation — non-negotiable

Two independent Phase 2 ceremonies are run (one per `(tier, K_max)` pair).
The two VKs **must not** be byte-equal and **must not** share any Phase 2
contribution material, even though they descend from the same
`phase1-final.ptau`. Groth16 soundness for circuit X binds proofs to
circuit X's R1CS via `IC[0..=n]`; if the tier-0 and tier-1 VKs were
accidentally generated from a single transcript (e.g. an operator
copy-paste), a tier-0 proof could verify against the tier-1 slot and
vice versa, collapsing the whole tier gating.

Domain separation is enforced at three layers — the ceremony only
controls layer 1:

1. **R1CS shape.** The constraint counts differ between tiers:
   tier 0 (depth 5, K_max 32) and tier 1 (depth 8, K_max 256) produce
   materially different R1CS matrices. A correctly-run ceremony that
   compiles the circuit for each pair separately cannot produce
   byte-identical VKs — the α·β·γ·δ pairings hash over different
   R1CS transcripts.
2. **Contract storage key.** The contract stores each VK under
   `DataKey::UpdateVKByType(tier, 2)`, so tier-0 and tier-1 slots are
   distinct ledger entries. Even if two VKs *were* byte-equal, a
   proof submitted with `tier = 0` is still dispatched against the
   tier-0 slot only.
3. **Public-input schedule.** Democracy proofs carry `member_count_old`
   and `member_count_new` in the public-input vector, and the contract
   rejects `member_count_new > 2^depth` before verification. A tier-0
   VK verifying a tier-1-sized proof would still require the caller to
   lie about `member_count`; the contract-side range check catches this.

Operator verification steps (before announcing the VKs):

```
sha256sum keyset-democracy/tier0-k32/verifying_key.bin
sha256sum keyset-democracy/tier1-k256/verifying_key.bin
# The two digests MUST differ. If they match, one of the ceremony runs
# reused transcripts and the entire ceremony MUST be re-run.

ceremony-tool inspect-vk keyset-democracy/tier0-k32/verifying_key.bin \
  | grep 'num_inputs\|num_constraints'
ceremony-tool inspect-vk keyset-democracy/tier1-k256/verifying_key.bin \
  | grep 'num_inputs\|num_constraints'
# num_inputs must be 5 for both (same public-input schedule).
# num_constraints must differ materially (tier 0 ≪ tier 1).
```

Cross-kind separation (Membership VK vs Update VK vs UpdateByType vs
AdminUpdate) is enforced by the `VkKind` dispatcher in the contract; no
ceremony-side action is needed, but the coordinator SHOULD still publish
the four digests (Membership small/medium/large, UpdateByType tier0/tier1,
AdminUpdate) in a single artefact bundle so clients can verify the full
set at once.

---

## 4. On-chain rollout

Contract changes required:

1. Add `DataKey::UpdateVKByType(tier, 2)` writes to `set_vk` (or the
   equivalent admin path) so the deployed contract can accept the new VKs.
2. In the `update_commitment` dispatcher (`lib.rs §6.3`, currently guarded
   with a `Democracy: unimplemented` fallthrough), wire the case for
   `current.group_type == 2`:
   - Load the tier-appropriate VK via `UpdateVKByType(tier, 2)`.
   - Verify `member_count_old == stored.member_count` (source-of-truth
     binding, §6.4.2).
   - Verify `member_count_new ∈ {m-1, m, m+1}`.
   - Verify the Groth16 proof with the 5-element public-input vector
     `(c_old, epoch_old, c_new, member_count_old, member_count_new)`.
   - On success, persist `member_count_new` into the V2 entry.
3. Add the `DemocracyInputMissing = 21`, `MemberCountMismatch = 24`,
   `MemberCountOutOfRange = 25` error paths defined at lib.rs:125, 385, 386.
4. Bump the contract build and re-deploy with a migration that is a **no-op
   for existing groups** — Democracy groups stay on the legacy dispatcher
   until their first post-rollout `update_commitment`, which is the first
   call that observes the new VK.

Rollout sequencing:

- T0: VKs published to testnet. Clients learn about the feature via an
  `is_democracy_enforced` view function.
- T0 + 7 days: testnet soak, invariant tests, red-team window.
- T1: VKs published to mainnet. Existing Democracy groups continue to
  function; their next Commit after T1 is rejected unless it carries a
  Democracy proof. A client-side banner is shown in the 7 days before T1
  prompting users to migrate pending proposals.

---

## 5. Client-side changes (post-VK)

- `ProofGenerator.swift` / `ProofGenerator.kt` gain
  `generateDemocracyUpdateProof(...)` entry points, backed by the same
  native prover code-path as the existing membership proofs.
- `GroupListViewModel.finalizeBallot` on both platforms stops calling
  `removeMember(...)` (which relies on Anarchy-shaped updates) and instead
  packages the collected `VOTE_CAST` signatures into the circuit witness.
- The `ballotTally` expiry filter (Phase C) continues to be enforced
  client-side for UX; on-chain enforcement is by circuit constraint #4
  evaluated over the signer set captured at proof generation.

The wire format for `VOTE_CAST::v1::<ballotID>::<yes|no>` gains a signature
field in v2: `VOTE_CAST::v2::<ballotID>::<yes|no>::<blsSig>`. The signature
is collected by the coordinator and feeds `DemocracySigner.secret_key`
openings at proof time. Legacy `v1` casts continue to be recognised by the
tally UI but are not usable as circuit witnesses.

---

## 6. Decommissioning the client-only path

Once the VK is live on mainnet, the client-only finalise path
(`GroupListViewModel.finalizeBallot` → `removeMember`) is a foot-gun: a
local build could still use it and trigger an on-chain rejection by the new
dispatcher. Remove the fallback in the release that immediately follows T1.

Timeline of removal milestones:

| Milestone | Behaviour |
|-----------|-----------|
| v1.6.x (today) | Client-only ballot UI. Anarchy-shaped finalise. |
| v1.7.0 (T0)    | Democracy VK on testnet. Clients opt-in via `--enable-democracy-vk`. |
| v1.7.1 (T1)    | Mainnet rollout. Dispatcher requires Democracy proofs. |
| v1.8.0         | Client-only fallback removed. `finalizeBallot` always produces a `DemocracyUpdateProof`. |

---

## 7. Risks & open questions

- **Circuit size.** Tier 1 × K_max 256 is ≈ 614k Poseidon constraints.
  Proving time on a mid-range phone (A15/Snapdragon 8 Gen 2) is an open
  measurement. If prover latency exceeds ~60 s, consider splitting the
  quorum proof into a two-round protocol (aggregator produces a
  short proof over pre-aggregated openings).
- **Ceremony participant overlap.** Reusing the membership ceremony's
  participant pool wholesale weakens the soundness of both ceremonies. At
  least two new participants should join the Democracy ceremony.
- **Key-rotation pattern in `DemocracyDelta`.** The `Replace` variant
  covers mid-group key rotation. Whether rotation alone should require a
  quorum (vs. the rotating member unilaterally) is a governance policy
  question currently unresolved — §6.4.2 bounds the delta magnitude but
  doesn't constrain the *kind* of change. Phase D ships the permissive
  interpretation; a tightening policy would be a v1.8 change.
