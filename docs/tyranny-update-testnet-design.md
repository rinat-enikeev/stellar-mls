# Per-type Soroban contract design: `sep-tyranny` (single-admin governance)

Status: v0 design doc · Targets per-type extraction parallel to `sep-democracy`, `sep-oligarchy`, `sep-anarchy`, `sep-oneonone`.

## 1. What this is, in one paragraph

A Soroban contract for chat groups governed by a **single admin**. The admin's BLS pubkey is committed at creation time (Poseidon-hashed) and pinned for the group's lifetime. Only proofs that demonstrate knowledge of the admin's secret key can advance the group's membership commitment. Members can verify their own membership read-only; non-admins cannot mutate state. The admin's on-chain address is **not** exposed by `update_commitment` — the cryptographic binding hides their identity from chain observers, the same way Democracy's quorum hides individual voter identity.

## 2. Why a separate contract (vs. Oligarchy K=1)

Tyranny is functionally a 1-of-1 Oligarchy. Two real reasons it gets its own contract instead of "configure Oligarchy with admin_threshold=1 and an admin tree of size 1":

- **No admin tree.** Oligarchy commits to a tree of admin pubkeys (variable-cardinality), with a per-group `admin_root` and rotation machinery. Tyranny commits to a single admin pubkey hash. The on-chain footprint is one Fr scalar, not a Merkle root with associated tree-update circuits.
- **No threshold parameter.** Oligarchy's `admin_threshold_numerator` is a configurable quorum knob over the admin tree. With a 1-element admin tree it can only ever be `1` (or `0`, which is invalid). Storing a constant is noise; same logic that drove the `tier`/`member_count` removals from `sep-oneonone`.

Net result: a smaller circuit family, smaller VK shapes, smaller storage entry, smaller wire payload, smaller test surface. The cost is one more per-type crate to maintain — paid once, parallel to the other four.

## 3. Post-postmortem-#153 reality

Ships **without `deactivate_group`** from the start (per [`docs/postmortem-deactivate-group-frontrun.md`](postmortem-deactivate-group-frontrun.md)). Any Tyranny group can be wound down by the admin via `update_commitment` with an agreed sentinel `c_new` (clients interpret as "wound down"); the group's on-chain entry then ages out via TTL when the admin stops issuing updates.

There is no `update_admin` entrypoint in v0 — the admin is fixed at creation. If the admin loses their BLS secret key, the group is unupdatable; the entry ages out. This is the same operational shape as Oligarchy's `salt_occ` loss case, but applied to a smaller set of keys (one, not many). Documented as an acknowledged operational limitation; admin rotation can land in v1 if needed.

## 4. What the contract is NOT

- **Not a v1 sep-xxxx migration.** Clean-slate. No `group_type` field in storage, no `member_count`, no `active: bool` (no deactivate path), no occupancy commitment.
- **Not Anarchy with `caller.require_auth()`.** That would leak the admin's Stellar address on every `update_commitment` — a privacy regression vs. the cryptographic binding.
- **Not Oligarchy with K=1.** See §2.
- **No `deactivate_group`** (postmortem #153).
- **No `update_admin`** in v0. Admin is pinned at create.
- **No occupancy commitment.** Tyranny does not hide member counts; same scope as Anarchy's "value-agnostic to count" model. If the admin's update history reveals counts, that's an accepted residual.
- **No `OneOnOneImmutable` / `Invalid1v1Tier` / `InvalidThreshold` / `MissingAdminRoot` / `InvalidInitialMembership` error variants.** All inapplicable; numbering kept aligned with siblings (gaps are intentional; full list in §7).

## 5. Entrypoints

Eight user-callable entrypoints + the constructor. Same surface as `sep-anarchy` minus `deactivate_group`, plus a Create-VK distinct from Membership-VK.

| Entrypoint | Auth | Purpose |
|---|---|---|
| `__constructor(env, admin, vk_membership_{small,medium,large}, vk_create_{small,medium,large}, vk_update_{small,medium,large})` | `admin.require_auth()` | One-time. 9 VKs total (3 families × 3 tiers). |
| `update_vk(env, kind: VkKind, tier: u32, new_vk)` | admin | VK rotation; `kind ∈ {Membership, Create, Update}`. |
| `set_restricted_mode(env, restricted)` | admin | Toggles whether non-admin callers may invoke `create_group`. |
| `bump_group_ttl(env, group_id)` | none | Permissionless TTL bump. |
| `create_group(env, caller, group_id, commitment, tier, admin_pubkey_commitment, proof, public_inputs)` | `caller.require_auth()` + restricted-mode gate | Verifies a Create-circuit proof against `(commitment, epoch=0, admin_pubkey_commitment)`. Persists `Group(group_id)` (CommitmentEntry) + `AdminCommitment(group_id)`. |
| `update_commitment(env, group_id, proof, public_inputs)` | none (proof IS auth) | Verifies an Update-circuit proof against `(c_old, epoch_old, c_new, admin_pubkey_commitment)`, where `admin_pubkey_commitment` is contract-supplied from `AdminCommitment(group_id)`. |
| `verify_membership(env, group_id, proof, public_inputs) → bool` | none | Read-only. Verifies a Membership-circuit proof. |
| `get_commitment(env, group_id) → CommitmentEntry` | none | Read-only state lookup. |
| `get_history(env, group_id, max_entries) → Vec<CommitmentEntry>` | none | Read-only history (rolling window). |

### 5.1 Three VKs

| VK | IC | Public inputs | Used at |
|---|---|---|---|
| Membership | 3 | `(commitment, epoch)` | `verify_membership` |
| Create | 4 | `(commitment, epoch=0, admin_pubkey_commitment)` | `create_group` |
| Update | 5 | `(c_old, epoch_old, c_new, admin_pubkey_commitment)` — `admin_pubkey_commitment` contract-supplied | `update_commitment` |

The Update circuit's witness includes the admin's BLS secret key; the circuit constrains `Poseidon(admin_pubkey) == admin_pubkey_commitment` and proves the prover knows the secret key behind that pubkey. The same circuit constrains the new member tree to differ from the old by **≤1 leaf** (single-leaf-delta).

Why ≤1: parity with the rest of the per-type family (Anarchy, Democracy, Oligarchy all enforce single-leaf-delta in their update circuits). Single-leaf-delta keeps the Update circuit small (one Merkle path verification, not N), keeps audit surface tractable, and matches the natural "one user joined / one user left / one key rotated" cadence of admin-driven membership changes. **Batching is a client-level concern**: the admin issues N sequential `update_commitment` calls for an N-leaf delta, advancing epoch by exactly 1 each time. Each on-chain Groth16 verify costs ~$0.05 at testnet rates, so a 50-member purge is ~$2.50 of gas — within tolerance for an admin-driven workflow. If a future product requires atomic batch-updates, that's a v1 circuit change (e.g., a `TyrannyBatchUpdateCircuit` with bounded `k`), not a v0 contract surface concern.

### 5.2 Front-running surface

`UsedProof(proof_hash)` is the contract-level mitigation for **post-inclusion** replay — a proof's bytes can be consumed at most once across the contract's nullifier scope.

The remaining residual is **pre-inclusion mempool front-running**:

- **`create_group`**. An attacker observing the prover's pending transaction can submit the same proof bytes (under their own `caller`) before the prover's tx lands. The attacker's `create_group` succeeds; `record_proof` consumes the nullifier; the prover's tx then fails (`GroupAlreadyExists` if the attacker used the same `group_id`, or `ProofReplay` if the attacker swapped `group_id` while keeping the same proof). To ship the group, the legitimate creator **regenerates a fresh proof** (different witness sampling) and submits with a different `group_id`. State delta is identical regardless of who submits — same `commitment`, same `admin_pubkey_commitment`. No per-creator on-chain privilege to be stolen.
- **`update_commitment`**. Same shape: the attacker can front-run by submitting the prover's leaked proof first, but the state delta is fully fixed by the proof's public inputs (`c_old`, `epoch_old`, `c_new`) AND the contract-supplied `admin_pubkey_commitment`. The attacker can only push the state to where the prover was already going.

Distinct from the post-#153 `deactivate_group` pattern (which was uniquely dangerous because the state delta was *irreversible* and triggerable by anyone observing a read-only `verify_membership` call). For Tyranny, the worst-case adversarial outcome is "prover regenerates and resubmits."

Closure of the pre-inclusion residual at the circuit level requires binding `caller` as a public input — v2 ceremony work, out of scope for v0.

## 6. Storage layout

```rust
pub struct CommitmentEntry {
    pub commitment: BytesN<32>,
    pub epoch: u64,
    pub timestamp: u64,
    pub tier: u32,
}

pub enum VkKind {
    Membership,
    Create,
    Update,
}

pub enum DataKey {
    Admin,                                       // Address (instance)
    RestrictedMode,                              // bool (instance)
    VK(u32),                                     // Membership VK per tier (persistent)
    CreateVK(u32),                               // Create VK per tier (persistent)
    UpdateVK(u32),                               // Update VK per tier (persistent)
    Group(BytesN<32>),                           // CommitmentEntry (persistent)
    AdminCommitment(BytesN<32>),                 // BytesN<32> per group (persistent)
    History(BytesN<32>),                         // Vec<CommitmentEntry> (persistent)
    UsedProof(BytesN<32>),                       // () (persistent, TTL bounded)
    GroupCount(u32),                             // u32 (instance, per tier)
}
```

`admin_pubkey_commitment` lives in its own per-group storage slot (`DataKey::AdminCommitment(group_id)`) rather than as a field on `CommitmentEntry`. Two reasons:

- **Avoids history duplication.** The admin commitment is invariant for a group's lifetime. Storing it in `CommitmentEntry` would mean every entry pushed to `History(group_id)` carries the same 32-byte value — at `HISTORY_WINDOW = 64` snapshots, that's ~2 KB of duplication per group, ~20 MB across the `MAX_GROUPS_PER_TIER × tiers = 30,000`-group ceiling. Not catastrophic but pointless.
- **Cleaner separation of concerns.** `CommitmentEntry` carries everything that varies per epoch (`commitment`, `epoch`, `timestamp`); `AdminCommitment` carries the fixed-at-create anchor. Each storage key has a single, auditable invariant.

`update_commitment` reads both `Group(group_id)` (for `c_old` / `epoch_old`) and `AdminCommitment(group_id)` (for the 4th public input). One extra ledger access per update; negligible vs. the Groth16 verify cost.

Fields removed from sibling templates:

| Removed | Reason |
|---|---|
| `group_type` | Single-type contract; address IS the discriminator |
| `active: bool` | No deactivate path |
| `member_count` | Tyranny doesn't hide counts; the field would be informational-only and drop |
| `occupancy_commitment` | No member-count hiding |
| `threshold_numerator` | No quorum |

## 7. Errors

```
1   NotInitialized
2   AlreadyInitialized
4   GroupAlreadyExists
5   GroupNotFound
7   InvalidProof
8   InvalidTier
9   InvalidVkLength
10  PublicInputsMismatch
11  InvalidEpoch
12  ProofReplay
13  TierGroupLimitReached
14  AdminOnly
15  InvalidCommitmentEncoding
26  InvalidPoint
```

14 reachable variants. Gaps in numbering (3, 6, 16-25, 27-30) are reserved by sibling contracts for variants Tyranny doesn't need (`Reserved3`, `GroupInactive`, `OneOnOneImmutable`, `MissingAdminRoot`, `Invalid1v1Tier`, `MemberCountMismatch`, `InvalidThreshold`, `GroupTypeMismatch`, `InvalidInitialMembership`, `GroupStillActive`, etc.). Numbering alignment is intentional; cross-contract error-code lookups stay consistent.

## 8. Capacity ceiling

`MAX_GROUPS_PER_TIER = 10_000`. `GroupCount(tier)` is monotonic increment-only (no decrement path since no `deactivate_group`). Same one-way-ratchet shape as the other post-#153 contracts; flagged as a known production-scale follow-up; testnet-fine.

## 9. Wire format

### `create_group`

```rust
caller: Address
group_id: BytesN<32>
commitment: BytesN<32>             // canonical Fr
tier: u32                          // ≤ 2
admin_pubkey_commitment: BytesN<32> // canonical Fr; pinned at create
proof: Groth16Proof
public_inputs: PublicInputsCreate  // { commitment, epoch=0, admin_pubkey_commitment }
```

The Create-circuit verifier opens 3 public inputs: positions 1-3 are `commitment`, `epoch=0`, `admin_pubkey_commitment` (in that order, IC[1..=3]; IC[0] base).

### `update_commitment`

```rust
group_id: BytesN<32>
proof: Groth16Proof
public_inputs: UpdatePublicInputs  // { c_old, epoch_old, c_new }  // 3 wire scalars
```

The Update-circuit verifier opens 4 public inputs: positions 1-3 are `c_old`, `epoch_old`, `c_new` (from wire); position 4 is `admin_pubkey_commitment` (read from `AdminCommitment(group_id)` storage). 5 IC points total (base + 4 inputs).

The contract's epoch invariant: `current.epoch == public_inputs.epoch_old`, post-update `current.epoch = epoch_old + 1` (overflow → `InvalidEpoch`).

### `verify_membership`

```rust
group_id: BytesN<32>
proof: Groth16Proof
public_inputs: MembershipPublicInputs  // { commitment, epoch }
```

3 IC points (standard membership shape).

## 10. Implementation plan (phases)

| Phase | Status |
|---|---|
| **Phase A (circuits)** — `TyrannyCreateCircuit` (3 public inputs / 4 IC, witness includes initial member tree + admin BLS pubkey + secret key — proves `Poseidon(pubkey) == admin_pubkey_commitment`) and `TyrannyUpdateCircuit` (4 public inputs / 5 IC, witness includes admin secret key + member trees before/after — proves admin secret key matches the committed pubkey hash and new tree differs from old by ≤1 leaf). Membership is the standard 3-IC circuit (2 public inputs / 3 IC). **Phase A blockers exist.** No prior keyset has these circuits. PR-Y ships with mock-VK fixture wiring; the deploy script accepts `FIXTURE_DIR` of dev VKs. | Pending |
| **Phase B (FFI)** — `generateTyrannyCreateProof`, `generateTyrannyUpdateProof` proof-generation paths in the Rust core. Bounded; ≤ 2 days once Phase A circuits are defined. | Pending |
| **Phase C (contract)** — this design + PR-Y. | Owned by PR-Y |
| **Phase D (clients)** — `swift-mls` SDK, iOS, Android. Out of scope for design + contract PRs. | Pending |
| **Phase E (ceremony)** — separate ceremony required (Tyranny VKs are net new vs. keyset-v2). Resolution after Phase A. | Pending |

## 11. Test plan

Inline tests parallel to `sep-anarchy`'s pattern. **~37 tests** total (smaller than Anarchy's 36 because no `deactivate_group` paths, but offset by the additional `update_commitment` admin-binding test and the additional `get_history` coverage; PR-Y will pin the exact count via `test_vectors_consistency`).

- Initialization (4): happy path + 3 IC-arity rejections (Membership=3, Create=4, Update=5 — all are total IC count, public-input count is one less in each case).
- `create_group` (11): happy path, invalid tier, duplicate id, non-canonical commitment, non-canonical admin_pubkey_commitment, invalid proof, restricted-mode admin-only + admin-can-create (positive), group-count cap, replay protection, public-inputs mismatch.
- `update_commitment` (7): happy path, stale c_old, wrong epoch_old, non-canonical c_new, replayed proof, unknown group, **`admin_pubkey_commitment` invariance** (set up a group, attempt update, assert the admin commitment is unchanged whether or not the proof verifies). Note: a literal `epoch-overflow` (`u64::MAX → checked_add`) test is structurally unreachable at testnet scale and would require injecting the boundary state via `as_contract`; covered transitively by the `wire.epoch_old != current.epoch` rejection on the `u64::MAX` boundary case rather than as a dedicated `InvalidEpoch`-from-overflow test.
- `verify_membership` (4): happy path, wrong commitment, wrong epoch, unknown group.
- Admin entrypoints (6): `update_vk_requires_auth`, `set_restricted_mode_requires_auth`, rotates Membership / Create / Update, invalid tier.
- Queries (4): `get_commitment` {happy, unknown}, `get_history` {happy, unknown}, `bump_group_ttl` {happy, unknown}. (Counted as 6 calls grouped into 4 entrypoint coverage rows; PR-Y will surface the actual `#[test] fn` count.)
- ABI pin (1): `test_vectors_consistency`.

Mock-proof strategy parallels Anarchy: `valid_g{1,2}` via `hash_to_g{1,2}`; happy-path tests assert `InvalidProof` (verifier reached but pairing fails on mock).

## 12. Sequencing

Two PRs back-to-back ("in one go" per the requesting message):

1. **PR-X** (this doc) — `docs/tyranny-update-testnet-design.md`. Decision artifact only.
2. **PR-Y** (stacked) — `contracts/sep-tyranny/` from-scratch crate: `Cargo.toml`, `Cargo.lock`, `src/lib.rs`, `src/test.rs`, `test-vectors.json`, `test_snapshots/test/*.json`, `scripts/deploy_sep_tyranny_testnet.sh`, plus `docs/contract/tyranny/{impl_plan, proof_of_correctness, proof_of_soundness}.md`.

Reviewer can read PR-X for design intent and PR-Y for contract surface independently; PR-Y depends on PR-X's intent but compiles and tests cleanly on its own.

## 13. Out of scope

- **Phase A circuit work.** This design specifies what the Tyranny circuits need to prove; the actual circuit code lives separately (probably in the existing `sep-xxxx` circuit repo or a Tyranny-specific module). PR-Y ships without real fixtures; deploy script accepts `FIXTURE_DIR` of dev VKs.
- **Admin rotation (`update_admin`).** v0 admin is fixed at create. v1 follow-up if needed.
- **Member-count hiding.** Out of scope for v0 — Tyranny is "value-agnostic to count" like Anarchy.
- **Multi-contract routing on the client side.** App-level concern.
- **Migration from Oligarchy K=1.** No live Oligarchy K=1 groups; clean-slate per-type contract.

## 14. References

- `docs/anarchy-update-testnet-design.md` — closest sibling for the no-occupancy / no-threshold scope
- `docs/oligarchy-update-testnet-design.md` — closest sibling for the admin-bound update circuit
- `docs/oneonone-update-testnet-design.md` — precedent for Create-VK distinct from Membership-VK
- `docs/postmortem-deactivate-group-frontrun.md` — rationale for shipping without `deactivate_group`
- `contracts/sep-anarchy/src/lib.rs` — closest sibling impl (mirror create + update + verify + bump_ttl shape)
- `contracts/sep-anarchy/test-vectors.json` — test-vectors-first JSON template
