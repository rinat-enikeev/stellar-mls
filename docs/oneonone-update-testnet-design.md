# Per-type Soroban contract design: `sep-oneonone` (1v1)

Status: v0 design doc · Targets per-type extraction parallel to `sep-democracy`, `sep-oligarchy`, `sep-anarchy`.

## 1. What this is, in one paragraph

A Soroban contract that records the immutable Poseidon commitment of a **two-party** group at creation time and exposes membership verification + TTL bumping. Membership is fixed at creation; there is no on-chain mutation path. Either party "leaving" the group is an off-chain event — the on-chain entry simply ages out via storage TTL. The smallest of the four per-type contracts; no quorum, no admin tree, no update circuit, no deactivation.

## 2. Why a separate contract

Other governance types are extracted into per-type contracts (`sep-democracy`, `sep-oligarchy`, `sep-anarchy`) so the contract address acts as the type discriminator and each crate carries only the surface its type needs. The same logic applies to 1v1, even though 1v1's contract is the smallest:

- **Address as discriminator.** Clients route 1v1 traffic to a fixed contract address. No `group_type` field on storage; no per-type dispatch in `update_commitment` (which doesn't exist for 1v1 anyway).
- **Tier-0 enforcement at contract level.** 1v1 is hardcoded to tier 0 (Small, capacity 32). A single Membership VK and a single Create VK are stored — not a 3-element tier array. `tier` is not a parameter on `create_group`; the contract emits `InvalidTier` only as a defense-in-depth guard if a stored VK ever gets queried under tier ≠ 0 (shouldn't happen, but cheap to keep).
- **No update path.** `update_commitment` is not an entrypoint. A 1v1 group's commitment is sealed at creation. This is stronger than a runtime guard returning `OneOnOneImmutable` — the function literally does not exist.

## 3. Post-postmortem-#153 reality

Postmortem #153 removed `deactivate_group` from `sep-democracy`, `sep-oligarchy`, and `sep-anarchy` due to an unfixable-at-the-contract-level mempool front-running vulnerability (verifying a membership proof leaked attack-grade material that could be replayed as `deactivate_group`). The same vulnerability applies to 1v1 in the legacy `sep-xxxx` contract, and the same fix applies here: **`sep-oneonone` ships without `deactivate_group` from the start**.

The legacy `docs/group-governance-types-design.md` listed `deactivate_group` and `bump_group_ttl` as the only allowed lifecycle events for 1v1. With `deactivate_group` removed, that leaves `bump_group_ttl`. The honest framing: a 1v1 group exists on-chain until its storage entry ages out (TTL ~30 days without a bump). Clients that no longer care about a 1v1 conversation simply stop bumping; the entry archives itself. There is no on-chain "leave" event.

This is fine for 1v1's product semantics — there is no public group state to "freeze" in a meaningful way; either party leaving is observable off-chain by the other party (or via stale chat) and is a pure client-side concern.

## 4. What the contract is NOT

To save a reader from looking for things that don't exist:

- **Not a v1 sep-xxxx migration.** Clean-slate. No `group_type` field, no `member_count` field, no `active: bool`, no `History`, no `archive_entry` helper. None of those make sense for an immutable-at-creation two-party group, so they aren't here.
- **No `update_commitment`.** Out of design intent, not a runtime check.
- **No `deactivate_group`.** Postmortem #153.
- **No admin tree, no quorum threshold, no occupancy commitment.** Those are Oligarchy / Democracy concepts.
- **No tier parameter on `create_group`.** Tier is fixed at 0; the parameter would only exist to reject every value other than 0, which is what the legacy contract did.
- **No `OneOnOneImmutable` error variant.** It existed in `sep-xxxx` to reject `update_commitment` calls on 1v1 groups; with no `update_commitment` entrypoint, the error is unreachable.
- **No `Invalid1v1Tier` error variant.** Same reason — no tier parameter to reject.

## 5. Entrypoints

Six user-callable entrypoints + the constructor.

| Entrypoint | Auth | Purpose |
|---|---|---|
| `__constructor(env, admin, vk_membership, vk_create)` | `admin.require_auth()` | One-time init. Pins admin + the two VKs (Membership + Create). |
| `update_vk(env, kind: VkKind, new_vk)` | `admin.require_auth()` | VK rotation (`Membership` or `Create`). No `tier` parameter — tier is fixed at 0. |
| `set_restricted_mode(env, restricted)` | `admin.require_auth()` | Toggles whether non-admin addresses may call `create_group`. Default `false`. |
| `create_group(env, caller, group_id, commitment, proof, public_inputs)` | `caller.require_auth()` + restricted-mode gate | Verifies a Create-circuit proof (which binds the 2-leaf invariant in-circuit) against `(commitment, epoch=0)`. Stores the entry, increments `GroupCount`. **`caller` is NOT bound into the proof**: it is a pure auth gate (must sign the transaction) plus the address-of-record for the optional restricted-mode admin check. The proof binds only `(commitment, epoch=0)`; see §5.2 for what this means for front-running. |
| `verify_membership(env, group_id, proof, public_inputs) → bool` | none | Read-only. Verifies a Membership-circuit proof against the stored commitment. Returns `Ok(false)` on invalid proof, not an error. |
| `bump_group_ttl(env, group_id)` | none | Permissionless TTL bump. The only ongoing lifecycle event for a 1v1 group. |
| `get_commitment(env, group_id) → CommitmentEntry` | none | Read-only state lookup. |

That's it. No `get_history` (history is always empty). No `update_commitment`. No `deactivate_group`.

### 5.1 Why two VKs and not one

The Create proof must bind an extra invariant the Membership proof doesn't: **the founding member tree contains exactly 2 non-zero leaves**. That's a circuit-level constraint encoded in `OneOnOneCreateCircuit`'s public inputs / witness shape. The Membership circuit (used by `verify_membership`) is the standard 3-IC `(commitment, epoch)` shape — same VK shape as the other per-type contracts use for their membership-only proofs. Two distinct VKs because they verify two distinct circuits.

This parallels `sep-oligarchy`, which also has a Create VK distinct from its Membership VK (Oligarchy's Create binds the verbose §4.8 tuple). The pattern is: when a contract needs to enforce a creation-time invariant beyond "you know a leaf," that invariant gets its own circuit and its own VK.

### 5.2 Front-running surface

A leaked or observed Create-circuit proof can be replayed by an attacker submitting `create_group` under their own address — `caller` is not bound into the proof, only `(commitment, epoch=0)` is. This is **benign griefing**: the attacker can only create the group with the *same* commitment the prover authored, the legitimate creator's subsequent submission fails with `GroupAlreadyExists` or `ProofReplay`, and there is no on-chain privilege to be stolen (no admin role per group, the `caller` is not stored). The legitimate creator picks a different `group_id` and resubmits.

This is the same shape as `create_group` front-running on the other three per-type contracts. The post-#153 reasoning (deactivate_group was uniquely dangerous because the state delta was *irreversible* and triggerable by anyone observing a read-only `verify_membership` call) does not apply to creates: the state delta is fully fixed by the proof's public inputs, and the worst case is "attacker forces the prover to re-pick a group_id." Documented for completeness; no contract-level mitigation needed beyond what's already there (replay protection via `UsedProof`).

## 6. Storage layout

```rust
pub struct CommitmentEntry {
    pub commitment: BytesN<32>,   // Poseidon commitment, canonical Fr
    pub epoch: u64,               // always 0 for 1v1 (immutable; no updates ever)
    pub timestamp: u64,           // ledger timestamp at create
}

pub enum VkKind {
    Membership,
    Create,
}

pub enum DataKey {
    Admin,                                   // Address (instance)
    RestrictedMode,                          // bool (instance)
    MembershipVK,                            // VerificationKeyData (persistent) — single VK, no per-tier array
    CreateVK,                                // VerificationKeyData (persistent) — single VK
    Group(BytesN<32>),                       // CommitmentEntry (persistent)
    UsedProof(BytesN<32>),                   // () (persistent, TTL ~30 days) — replay protection
    GroupCount,                              // u32 (instance) — single counter, no per-tier array
}
```

Differences from the per-type-template (Anarchy/Democracy/Oligarchy):

| Field | Removed in 1v1 | Reason |
|---|---|---|
| `active: bool` on entry | yes | No `deactivate_group`; no code path can set it false. Removing the field is honest about the immutable-by-design property. |
| `member_count` on entry | yes | Always 2 by definition. Storing a constant is noise. |
| `tier: u32` on entry | yes | Always 0 by definition. Same rationale as `member_count` — storing a constant on every group is noise. Clients reading `get_commitment` from a `sep-oneonone` contract know the tier from the contract address. |
| `History(group_id)` | yes | Empty by definition (no updates). |
| `GroupDeactivated` event | yes | Postmortem #153. |
| `CommitmentUpdated` event | yes | No updates. |
| `VK(tier)` per-tier array | replaced with single `MembershipVK` | Tier fixed at 0. |
| `UpdateVK(tier)` array | yes | No update circuit. |
| `GroupCount(tier)` per-tier array | replaced with single `GroupCount` | Tier fixed at 0. |

`UsedProof` is retained because `create_group` still needs proof-replay protection: same Groth16 proof bytes resubmitted as a different `(group_id, commitment)` pair must be rejected. The legacy `sep-xxxx` contract documents this as the global-nullifier scope and we mirror it.

## 7. Errors

A focused set; no inherited dead variants.

| Code | Variant | Trigger |
|---|---|---|
| 1 | NotInitialized | Entrypoint called before `__constructor` |
| 2 | AlreadyInitialized | Constructor called twice |
| 4 | GroupAlreadyExists | `create_group` with an existing `group_id` |
| 5 | GroupNotFound | `verify_membership` / `get_commitment` / `bump_group_ttl` on unknown group |
| 7 | InvalidProof | Groth16 verify failed |
| 9 | InvalidVkLength | Constructor / `update_vk` got a VK with wrong IC count |
| 10 | PublicInputsMismatch | Caller-supplied public inputs don't match expected (commitment, epoch=0) |
| 12 | ProofReplay | Proof bytes already burned in `UsedProof` |
| 13 | GroupCountLimitReached | `create_group` would exceed `MAX_GROUPS` |
| 14 | AdminOnly | Restricted mode set, non-admin caller tried `create_group` |
| 15 | InvalidCommitmentEncoding | Commitment is not canonical Fr |
| 26 | InvalidPoint | VK or proof point fails BLS12-381 small-subgroup check |

Numbers are aligned with the existing per-type contracts where the trigger overlaps (`7 = InvalidProof` everywhere, `26 = InvalidPoint` everywhere, etc.). **Gaps in the numbering (3, 6, 8, 11, 16-25) are intentional**: those codes are reserved by sibling contracts for variants that don't apply to 1v1 (`InvalidTier`, `GroupInactive`, `InvalidEpoch`, `InvalidThreshold`, `OneOnOneImmutable`, `Invalid1v1Tier`, etc.). Holding the alignment makes cross-contract error-code lookups consistent and avoids future renumbering churn if 1v1 ever needs to add an overlap-shaped variant.

## 8. Capacity ceiling

`MAX_GROUPS = 10_000`. `GroupCount` is a one-way ratchet (no decrement path since no `deactivate_group`). At testnet scale, 10k is non-issue. **At production scale, 10k 1v1 groups per contract is restrictive** — two users having multiple short conversations would burn through the budget. This is a known follow-up; the right answer is probably "deploy a fresh contract instance and route by deployment address" rather than re-introduce decrementing. Out of scope for this design. Documented in `test-vectors.json` `GroupCount` note for visibility.

## 9. Wire format

### `create_group`

```rust
caller: Address
group_id: BytesN<32>
commitment: BytesN<32>             // canonical Fr
proof: Groth16Proof
public_inputs: PublicInputs        // { commitment: BytesN<32>, epoch: u64 } — epoch must be 0
```

The Create-circuit verifier opens `public_inputs.commitment` against `IC[1]` and `public_inputs.epoch` against `IC[2]`. The 2-leaf invariant is enforced inside the circuit, not at the contract level — the contract sees only the standard Membership-shape public inputs.

### `verify_membership`

```rust
group_id: BytesN<32>
proof: Groth16Proof
public_inputs: PublicInputs        // { commitment, epoch }
```

`public_inputs.commitment` and `public_inputs.epoch` must equal the stored entry. `epoch` is always 0.

## 10. Implementation plan (phases)

Mirroring the established pattern from `docs/contract/{democracy,oligarchy,anarchy}/impl_plan.md`:

- **Phase A (Create circuit + VK).** Per `docs/group-governance-types-design.md §6.4.1`, `OneOnOneCreateCircuit` enforces "exactly 2 non-zero leaves at founding." Whether this circuit already exists or needs new fixture work is the only Phase-A blocker. (To resolve during PR-Y impl.)
- **Phase B (FFI / Rust bridge).** New `generateOneOnOneCreateProof` proof-generation path, parallel to the existing `generateMembershipProof`. Bounded; ≤ 1 day of work.
- **Phase C (contract).** This document. New crate `contracts/sep-oneonone/` (~400 LOC `lib.rs`, smaller than `sep-anarchy`'s 700 LOC since no update path).
- **Phase D (clients).** `swift-mls` SDK gains `createOneOnOneGroup(...)` + `verifyOneOnOneMembership(...)`; iOS + Android wire the new contract address. **Not part of this design's PR or its follow-up contract PR** — clients move when they're ready, after Phase A circuits land.
- **Phase E (ceremony).** Reuses keyset-v2 if `OneOnOneCreateCircuit` is already in that ceremony; otherwise a separate ceremony is required. Resolution during Phase A.

## 11. Test plan

Inline contract tests parallel to the other per-type contracts. **26 tests** total (smaller than Anarchy's 36 because no `update_commitment` paths and no `deactivate_group` paths to cover):

- **Initialization (3)**: happy path + 2 IC-arity rejections (membership=3, create=≥3 — exact count pinned in PR-Y).
- **`create_group` (10)**: happy path, duplicate group_id, non-canonical commitment, invalid proof (mock), restricted-mode admin-only, restricted-mode non-admin-rejected, group-count cap, replay protection, public-input mismatch on commitment, public-input mismatch on epoch (must be 0).
- **`verify_membership` (5)**: happy path, wrong commitment, wrong epoch, GroupNotFound, mock-proof returns Ok(false).
- **`update_vk` (4)**: requires_auth, rotates membership, rotates create, invalid VK length.
- **Queries (3)**: get_commitment {happy, GroupNotFound}, bump_group_ttl.
- **ABI pin (1)**: `test_vectors_consistency`.

Mock-proof strategy parallels Anarchy: `valid_g1` / `valid_g2` via `hash_to_g{1,2}`; happy-path tests assert `InvalidProof` (verifier reached but mock proof can't pass pairing).

## 12. Sequencing

Two PRs, parallel to the established workflow:

1. **PR-X** (this doc) — `docs/oneonone-update-testnet-design.md`. Decision artifact only.
2. **PR-Y** — `contracts/sep-oneonone/` from-scratch crate: `Cargo.toml`, `Cargo.lock`, `src/lib.rs`, `src/test.rs`, `test-vectors.json`, `test_snapshots/test/*.json`, `scripts/deploy_sep_oneonone_testnet.sh`, plus `docs/contract/oneonone/{impl_plan, proof_of_correctness, proof_of_soundness}.md`.

PR-Y depends on Phase A's `OneOnOneCreateCircuit` VK being available (or being treated identically to Anarchy's "reuse keyset-v2"). If the Create circuit is not yet in any keyset, PR-Y can ship the contract with mock-VK fixture wiring and the deploy script can take a `FIXTURE_DIR` for dev VKs — same pattern as the other per-type deploy scripts.

## 13. Out of scope

- **Phase A circuit work** (`OneOnOneCreateCircuit` definition + ceremony). Owned separately; this design assumes the circuit either already exists in keyset-v2 or will be added there.
- **Client wiring** (`swift-mls` / iOS / Android). Phase D follow-up after the contract lands.
- **Multi-contract routing** (clients deciding which per-type address to invoke based on `groupType`). App-level concern; the contract is type-blind.
- **Capacity-relaxation** (the `MAX_GROUPS = 10_000` ceiling). Production scaling problem; testnet ceiling is fine.
- **OneOnOneImmutable / Invalid1v1Tier error compatibility** with the legacy `sep-xxxx` ABI. Clean-slate per-type contract; legacy clients still talk to legacy `sep-xxxx`.

## 14. References

- `docs/group-governance-types-design.md §6.4.1` — `OneOnOneCreateCircuit` requirements (2-leaf invariant)
- `docs/anarchy-update-testnet-design.md` — closest sibling design; structural template
- `docs/oligarchy-update-testnet-design.md` — Create-VK-distinct-from-Membership-VK precedent
- `docs/postmortem-deactivate-group-frontrun.md` — rationale for shipping without `deactivate_group`
- `contracts/sep-anarchy/src/lib.rs` — closest sibling impl; mirror create + verify + bump_ttl shape, drop update path
- `contracts/sep-anarchy/test-vectors.json` — test-vectors-first JSON template
