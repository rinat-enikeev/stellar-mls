# Postmortem: front-running `deactivate_group`

**Status:** Decided 2026-04-27 · removal pending across two follow-up PRs
**Decision:** Drop the `deactivate_group` entrypoint from `sep-democracy`, `sep-oligarchy`, `sep-anarchy`. Retain in `sep-xxxx` (legacy) until that contract is decommissioned wholesale.
**Discovered:** Skill-eval audit run, `claude-opus-4-7` + vendored `stellar-dev` skill, fixture `04_audit_sep_democracy`. Captured at `evals/reports/stellar-dev_2026-04-27_17-39-27.md`.

## TL;DR

`deactivate_group` accepts a single Groth16 membership proof and irreversibly freezes the group. Two unfixable-at-the-contract-level replay vectors exist:

1. **Pre-inclusion mempool front-run.** `verify_membership` is read-only and does not consume the proof's nullifier. Any observer of an honest member's verify call can resubmit the same proof bytes as `deactivate_group` and win block ordering. Honest member's tx then fails with `ProofReplay`; the group is permanently dead.
2. **Nullifier-expiry replay.** `UsedProof` entries TTL out after `LEDGER_BUMP` (~30 days). A group that sits at the same `(commitment, epoch)` longer than that can be deactivated by replaying *any* earlier membership proof — including the create-time proof if no `update_commitment` has run.

Neither vector can be closed by contract logic alone. The legacy `sep-xxxx` source already documents both as "known residual exposure" (Re-audit Finding #4, `contracts/sep-xxxx/src/lib.rs:1828-1857` and the high-level summary at `:108-128`). The proposed real fix is a circuit rotation: bind an operation tag, `group_id`, and `caller` as Membership-circuit public inputs so a VERIFY proof is no longer a valid DEACTIVATE proof and proofs can't be stolen across addresses.

The new per-type contracts (`sep-democracy`, `sep-oligarchy`, `sep-anarchy`) carried the `deactivate_group` shape forward without inheriting the warning blocks. **We are choosing to drop the entrypoint instead of doing the circuit work.** Rationale below.

## Why this wasn't found earlier

The bug is not new. It is documented in the legacy contract as a known residual. What is new:

- The per-type extraction (`sep-democracy` v0.5, `sep-oligarchy` v0.1.4, `sep-anarchy` v0) carried over the entrypoint *without* the warning blocks. A reviewer looking at the new contracts in isolation would not see the existing acknowledgment.
- The `stellar-dev` skill audit re-flagged the issue from first principles, confirming it survives in the per-type code. This is the eval harness paying for itself: the audit fixture surfaced a structural issue against fresh contract source, which prompted re-grep of the legacy code, which surfaced the long-standing acknowledgment.

## The bug, in detail

### Three entrypoints take a Groth16 proof

| Entrypoint | `caller.require_auth()` | Public inputs | State change | Front-run impact |
|---|:-:|---|---|---|
| `create_group` | ✓ | `(commitment, epoch=0)` | Create new group | State delta fixed by inputs. Attacker can only create the group the prover already wanted. Benign griefing (gas shifted). |
| `update_commitment` | ✗ | 5-tuple incl. `c_new` | Advance commitment | State delta fixed by inputs. Attacker can only push to where the prover was already going. Benign. |
| **`deactivate_group`** | ✗ | `(commitment, epoch)` — **identical to `verify_membership`** | Permanently freeze group | **Real attack.** Irreversible. Public inputs match a read-only entrypoint that does not burn nullifiers. |

The proof-shape collision between `verify_membership` and `deactivate_group` is the root cause. They use the *same* Membership VK and the *same* `(commitment, epoch)` public inputs. `verify_membership` is intentionally read-only and intentionally does not call `record_proof` (consuming the nullifier on every membership check would defeat the purpose of having a query API). That design choice — correct in isolation — turns every honest membership check into a free supply of attack-grade `deactivate_group` proofs.

### Why contract-level mitigations don't work

Considered and rejected:

- **Make `verify_membership` consume the nullifier.** Breaks the read-only invariant. Every UI "am I in this group?" check becomes a state-write, blowing up gas and TTL pressure. Removes the entrypoint's purpose.
- **Per-group nullifier sets.** Unbounded growth or rent problems. Doesn't close pre-inclusion replay anyway.
- **Keep nullifiers forever.** Same growth problem; reads degrade over time.
- **Bind `caller` into the contract-level proof_hash.** Doesn't help — the proof itself is what's being stolen, and the attacker submits under their own address.

The only real fix is at the circuit level: rotate Membership to a new VK that binds an operation tag (`MEMBERSHIP_VERIFY` ≠ `MEMBERSHIP_DEACTIVATE`), `group_id`, and `caller`. That's a Phase A change — new circuit, new ceremony, new fixtures, new client wiring. Significant work for a primitive whose value is already in question (next section).

## The architectural argument

`deactivate_group` is a unilateral kill switch in contracts whose entire premise is governance:

- `sep-democracy`: state transitions require quorum (`threshold_numerator` enforced in-circuit). `deactivate_group` requires **one** member's proof.
- `sep-oligarchy`: admin-tree co-signing for state transitions. `deactivate_group` requires **one** member's proof from the membership tree, no admin involvement.
- `sep-anarchy`: no quorum (Anarchy's whole point) — but here the entrypoint is also the *only* irreversible primitive in the contract. The "permissionless leave" framing fits Anarchy; the "permanent freeze for everyone else" framing does not.

The "V-C1 safety valve from sep-xxxx" comment is legacy carried forward without re-evaluation. In sep-xxxx the safety valve was meaningful: a OneOnOne or Anarchy group needed a way for a single party to dissolve the relationship. In Democracy and Oligarchy, the same primitive **is exactly the wrong shape** — it bypasses the governance model the rest of the contract enforces.

If we needed deactivation that respects governance, the right design is `update_commitment` with a sentinel `c_new` that participating clients interpret as "deactivated." That's a client-level convention, not a contract-level entrypoint, and it inherits the in-circuit threshold check for free.

## Decision

**Drop `deactivate_group` from `sep-democracy`, `sep-oligarchy`, `sep-anarchy`.**

Replacements available without any contract change:

- **Group abandonment by inactivity.** Storage TTL eventually archives the group entry. Clients should treat "group hasn't updated in N days" as a UX signal independent of an explicit `active` flag.
- **Admin VK rotation as suppression.** `update_vk` rotates the tier's verifier to a junk one, making all subsequent proofs fail verification. Reversible (rotate back), admin-gated (uses `require_auth`), already part of the contract surface. This is a strictly stronger primitive than `deactivate_group` ever was.
- **Quorum-driven sentinel update.** `update_commitment` with a `c_new` of `0x0000…` (or other agreed sentinel) interpreted by clients as "wound down." Inherits the in-circuit threshold check; cannot be front-run to a state the prover didn't consent to (per the analysis above, `update_commitment` front-running is benign).

## Alternatives weighed

| Option | Cost | Closes the bug? | Why not chosen |
|---|---|:-:|---|
| **MembershipCircuit v2 (op-tag + caller binding)** | High — new circuit, new ceremony, new fixtures, regenerated VKs across all tiers, coordinated client wiring | Yes | Pays for a primitive we already think is architecturally wrong. We'd close the front-run only to delete the entrypoint in the next quarter when the unilateral-kill problem resurfaces in product review. |
| **Per-group monotonic `proof_nonce` (closes nullifier-expiry replay)** | Medium — new public input, partial circuit change | Closes #2 only; #1 still open without op-tag work | Same objection: paying circuit cost to keep an entrypoint we don't want. |
| **Keep + document the residual** | Zero | No | sep-xxxx already does this. The result is the bug we're now removing. |
| **Drop the entrypoint** ✓ | Low — contract changes + client surface removal | N/A — surface eliminated | Chosen. |

## Blast radius

### Contracts (3 active + 1 legacy)

| Contract | `deactivate_group` location | Action |
|---|---|---|
| `sep-democracy` | `src/lib.rs:664-720` | Remove. ~5 tests in `src/test.rs` removed. Update `test-vectors.json`. |
| `sep-oligarchy` | `src/lib.rs:830-…` | Remove. ~5 tests removed. Update `test-vectors.json`. |
| `sep-anarchy` | `src/lib.rs:672-…` | Remove. Tests + vectors. |
| `sep-xxxx` (legacy) | `src/lib.rs:1858-…` | **Keep as-is.** Contract is on its retirement path; no new feature work, including bug-fixes that change the surface. The Re-audit Finding #4 documentation already in the source is the operator's notice. |

Each contract drop also removes:
- `Self::archive_entry` calls specific to deactivation.
- The `GroupCount` decrement on deactivation (verify no other call site relies on this — review during PR-1).
- The `GroupInactive` error variant *may* still be reachable via `update_commitment` on an already-deactivated group; check before pruning the variant.

### SDK

- `swift-mls/Sources/SwiftMLS/ContractClient.swift` — public `deactivateGroup(_:)` API. Remove.
- `SEPDeactivateGroupRequest` request/response types — remove.

### Clients

- **iOS**: `clients/ios/StellarChat/StellarChat/StellarChatApp.swift` (`deactivateGroupOnChain` UI flow), `Models/OnChainService.swift` (`deactivateGroup` service method), group settings UI button + confirmation dialog.
- **Android**: `clients/android/StellarChat/app/src/main/java/chat/onym/android/viewmodel/GroupListViewModel.kt` (`deactivateGroupOnChain`), `onchain/OnChainService.kt`, `onchain/SEPContractClient.kt`, `onchain/ContractTypes.kt` (request/response types). Group settings UI + confirmation dialog.

### Documentation

- `docs/democracy-update-testnet-design.md`, `docs/oligarchy-update-testnet-design.md`, `docs/anarchy-update-testnet-design.md` — entrypoint listings need updating.
- `docs/contract/{democracy,oligarchy,anarchy}/impl_plan.md` — entrypoint table.
- `docs/contract/{democracy,oligarchy,anarchy}/proof_of_correctness.md` — spec→impl mapping for the dropped entrypoint.

## Migration phases

| Phase | Scope | PR size | Blocks |
|---|---|---|---|
| **0 (this PR)** | This postmortem only. No code change. | 1 file, ~250 lines | — |
| **1** | Contract-side removal: `sep-democracy` + `sep-oligarchy` + `sep-anarchy`. Tests + `test-vectors.json` + design docs updated. | 3 contracts × ~150 LOC + tests + 3 design-doc patches. | Phase 0 merged. None of these contracts are wired to client production traffic yet — testnet groups still flow through `sep-xxxx`. |
| **2** | SDK + client removal: `swift-mls` API, iOS UI, Android UI. SDK version bump. | ~200 LOC across 7 files + UI dialog deletions. | Phase 1 merged + clients pin to the new contracts (separate work; tracked elsewhere). |
| **legacy** | `sep-xxxx` keeps `deactivate_group` and the existing residual-exposure documentation. Decommissioned wholesale when its testnet contract is retired. | n/a | Independent of phases 1-2. |

## What this isn't

- **Not a postmortem of a production incident.** No group has been attacked. The flaw was discovered before the new per-type contracts were wired into production traffic. The "postmortem" framing here is for the decision-rationale; nothing operational broke.
- **Not retroactive on the legacy contract.** `sep-xxxx` keeps the entrypoint; its retirement is the only remediation there. Operators are reminded: the existing source-level documentation at `contracts/sep-xxxx/src/lib.rs:108-128` and `:1828-1857` describes mitigations (periodic no-op `update_commitment` calls).
- **Not a circuit change.** No new VK, no ceremony, no fixture regen. We are removing a contract entrypoint and the client surface that calls it, not re-keying anything.
- **Not a permanent close on the underlying problem.** If a future product requirement needs a member-driven deactivation primitive, the right path is the v2-circuit work (op-tag + `caller` binding + per-group `proof_nonce`). Re-opening that conversation should start from this doc, not from the existing legacy comments alone.

## How this was found, in detail

1. PR #152 vendored the [stellar/stellar-dev-skill](https://github.com/stellar/stellar-dev-skill) bundle into `.claude/skills/stellar-dev/` and built an A/B eval harness at `evals/`.
2. Fixture `evals/skills/stellar-dev/fixtures/04_audit_sep_democracy.py` was added — it pipes `contracts/sep-democracy/src/lib.rs` through Claude with the skill's `security.md` / `common-pitfalls.md` / `contracts-soroban.md` references in context, and asks for a six-section structured audit.
3. Run: `python evals/eval.py --skill stellar-dev --fixture 04_audit_sep_democracy --model claude-opus-4-7 --save`.
4. The "Authorization & access control" section of the audit flagged `deactivate_group` front-running as one of the top three highest-impact fixes. Cross-checking against the legacy `sep-xxxx` source confirmed the issue is long-known but the awareness did not cross over to the per-type contracts.
5. The keyword scorers in fixture 04 are too coarse to numerically distinguish with-skill vs without-skill on this audit (both modes hit 8/8). The qualitative content of the audit is the deliverable, not the score gap. The eval-harness README already calls this out as an expected mode of use; this incident validates it.

## References

- Audit run: `evals/reports/stellar-dev_2026-04-27_17-39-27.md`
- Per-type contract sources: `contracts/sep-democracy/src/lib.rs:664-720`, `contracts/sep-oligarchy/src/lib.rs:830-…`, `contracts/sep-anarchy/src/lib.rs:672-…`
- Legacy acknowledgment of both replay vectors: `contracts/sep-xxxx/src/lib.rs:108-128` (high-level) and `:1828-1857` (`deactivate_group` doc-comment, Re-audit Finding #4)
- Companion design docs (need updating in Phase 1): `docs/democracy-update-testnet-design.md`, `docs/oligarchy-update-testnet-design.md`, `docs/anarchy-update-testnet-design.md`
