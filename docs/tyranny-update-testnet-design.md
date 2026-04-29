# Per-type Soroban contract design: `sep-tyranny` (single-admin governance)

Status: v0 design doc · Targets per-type extraction parallel to `sep-democracy`, `sep-oligarchy`, `sep-anarchy`, `sep-oneonone`.

## 1. What this is, in one paragraph

A Soroban contract for chat groups governed by a **single admin**. The admin's BLS pubkey is committed at creation time (`Poseidon(admin_pubkey, group_id_fr)`) and pinned for the group's lifetime. Only proofs that demonstrate knowledge of the admin's secret key can advance the group's membership commitment. Members can verify their own membership read-only; non-admins cannot mutate state. The admin's on-chain address is **not** exposed by `update_commitment` — the cryptographic binding hides their identity from chain observers, the same way Democracy's quorum hides individual voter identity. **Per-group binding via `group_id_fr`** ensures that the same admin in two different groups produces uncorrelated on-chain commitment values: an observer cannot link "this group's admin == that group's admin" without breaking Poseidon.

## 2. Why a separate contract (vs. Oligarchy K=1)

Tyranny is functionally a 1-of-1 Oligarchy. Two real reasons it gets its own contract instead of "configure Oligarchy with admin_threshold=1 and an admin tree of size 1":

- **No admin tree.** Oligarchy commits to a tree of admin pubkeys (variable-cardinality), with a per-group `admin_root` and rotation machinery. Tyranny commits to a single admin pubkey hash. The on-chain footprint is one Fr scalar, not a Merkle root with associated tree-update circuits.
- **No threshold parameter.** Oligarchy's `admin_threshold_numerator` is a configurable quorum knob over the admin tree. With a 1-element admin tree it can only ever be `1` (or `0`, which is invalid). Storing a constant is noise; same logic that drove the `tier`/`member_count` removals from `sep-oneonone`.

Net result: a smaller circuit family, smaller VK shapes, smaller storage entry, smaller wire payload, smaller test surface. The cost is one more per-type crate to maintain — paid once, parallel to the other four.

## 3. Post-postmortem-#153 reality

Ships **without `deactivate_group`** from the start (per [`docs/postmortem-deactivate-group-frontrun.md`](postmortem-deactivate-group-frontrun.md)). Any Tyranny group can be wound down by the admin via `update_commitment` with an agreed sentinel `c_new` (clients interpret as "wound down"); the group's on-chain entry then ages out via TTL when the admin stops issuing updates.

There is no `update_admin` entrypoint in v0 — the admin is fixed at creation. If the admin loses their BLS secret key, the group is unupdatable; the entry ages out. This is the same operational shape as Oligarchy's `salt_occ` loss case, but applied to a smaller set of keys (one, not many). Documented as an acknowledged operational limitation; admin rotation can land in v1 if needed.

### 3.1 Privacy properties (privacy-first messenger context)

| What's hidden cryptographically | How |
|---|---|
| Admin's BLS pubkey | Only `Poseidon(admin_pubkey, group_id_fr)` lands on-chain. The pubkey itself is the witness; chain observers see only the 32-byte hash. |
| Admin's BLS secret key | Witness, never on-chain. The Update circuit's "knowledge of `admin_pubkey`'s secret" is enforced by the circuit, not by any on-chain artifact. |
| Member identities | Standard Poseidon-committed member tree (parallel to Democracy / Oligarchy). |
| **Cross-group admin linkability** | **Closed by per-group binding.** `admin_pubkey_commitment` for the same admin in two groups differs because `group_id_fr` differs (it's derived deterministically from the group_id bytes by the contract). Observer can't correlate two AdminCommitment values across groups without knowing `admin_pubkey`. |

| Acknowledged residuals (NOT closed) | Why / Mitigation |
|---|---|
| Update cadence | Inherent to public ledger. Each `update_commitment` advances epoch with a fresh timestamp; an observer sees per-group activity rhythm regardless of cryptography. Same residual as the other per-type contracts. |
| Stellar tx-submitter address | `update_commitment` has no `caller.require_auth()` (the proof IS the auth), and the proof itself is zero-knowledge. **But** Stellar requires *some* account to sign + pay fees for the transaction. If the admin submits from their own wallet, their Stellar address appears in the tx envelope. **Mitigation: clients MUST route through `SEPRelayerTransport`** ([`docs/sep.md` §5 "Transaction Submission and Fee Decoupling"](sep.md)) — the relayer signs and submits, the relayer's address appears, not the admin's. This is a client-integration responsibility, not a contract-level guarantee. The same applies to `create_group` (which DOES `caller.require_auth()` — admin's signing wallet leaks unless the relayer creates on their behalf). |
| Group existence + tier | The contract address itself is observable. Anyone querying the contract can enumerate created groups via the `Group(group_id)` keys. |

The privacy claim of v0 is: **same admin operating multiple Tyranny groups produces uncorrelated on-chain artifacts** — assuming the admin uses a relayer for transaction submission (closes the address-leak residual) and doesn't reveal the admin pubkey through other channels. The cryptographic core of that claim lives in the per-group binding documented in §5.1.

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
| `create_group(env, caller, group_id, commitment, tier, admin_pubkey_commitment, proof, public_inputs)` | `caller.require_auth()` + restricted-mode gate | Verifies a Create-circuit proof against `(commitment, epoch=0, admin_pubkey_commitment, group_id_fr)`. Persists `Group(group_id)` (CommitmentEntry) + `AdminCommitment(group_id)`. |
| `update_commitment(env, group_id, proof, public_inputs)` | none (proof IS auth) | Verifies an Update-circuit proof against `(c_old, epoch_old, c_new, admin_pubkey_commitment, group_id_fr)`, where `admin_pubkey_commitment` is contract-supplied from `AdminCommitment(group_id)` and `group_id_fr` is contract-derived from the `group_id` bytes. |
| `verify_membership(env, group_id, proof, public_inputs) → bool` | none | Read-only. Verifies a Membership-circuit proof. |
| `get_commitment(env, group_id) → CommitmentEntry` | none | Read-only state lookup. |
| `get_history(env, group_id, max_entries) → Vec<CommitmentEntry>` | none | Read-only history (rolling window). |

### 5.1 Three VKs

| VK | IC | Public inputs | Used at |
|---|---|---|---|
| Membership | 3 | `(commitment, epoch)` | `verify_membership` |
| Create | 5 | `(commitment, epoch=0, admin_pubkey_commitment, group_id_fr)` | `create_group` |
| Update | 6 | `(c_old, epoch_old, c_new, admin_pubkey_commitment, group_id_fr)` — `admin_pubkey_commitment` contract-supplied from storage; `group_id_fr` contract-derived from `group_id` bytes | `update_commitment` |

The Update circuit's witness includes the admin's BLS secret key; the circuit constrains:

```
Poseidon(admin_pubkey, group_id_fr) == admin_pubkey_commitment
```

and proves the prover knows the secret key behind that pubkey. **`group_id_fr` is the per-group salt that closes cross-group commitment linkability** (see §3.1): `group_id_fr = Fr::from_u256(U256::from_be_bytes(group_id))`, derived deterministically by the contract from the 32 group_id bytes. Same group_id always yields the same Fr; different group_ids yield different Frs with overwhelming probability **assuming `group_id` is uniformly random**.

Implementation note for clients: `group_id` is reduced mod the BLS12-381 scalar field order `r` (which is ~2^254). Two `group_id`s differing only in the top ~2 bits will collide on `group_id_fr`. **Clients MUST generate `group_id` from a CSPRNG** (e.g., 32 bytes of `os.urandom`) rather than from low-entropy or attacker-influenced sources — collisions on the high bits weaken the per-group binding's privacy property. This is enforced by client convention, not by the contract; if a future product needs structured group_ids, the protocol gains a domain-tag-prefixed Poseidon hash at the contract level instead of `Fr::from_u256` directly.

The same circuit constrains the new member tree to differ from the old by **≤1 leaf** (single-leaf-delta).

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

`update_commitment` reads both `Group(group_id)` (for `c_old` / `epoch_old`) and `AdminCommitment(group_id)` (for the 4th public input). The 5th public input (`group_id_fr`) is computed at runtime from the `group_id` parameter — no extra storage. One extra ledger access per update vs. a no-binding shape; negligible vs. the Groth16 verify cost.

**The stored value at `AdminCommitment(group_id)` is per-group-unique by construction**: it's `Poseidon(admin_pubkey, group_id_fr)` for whatever `group_id` the slot is keyed under. Same admin operating two Tyranny groups → two different `group_id` → two different `group_id_fr` → two uncorrelated `AdminCommitment` values on-chain. An observer reading `AdminCommitment(A)` and `AdminCommitment(B)` cannot correlate "same admin behind both" without knowing `admin_pubkey` (which is the witness, never on-chain). This is the privacy property §3.1 claims.

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

The Create-circuit verifier opens 4 public inputs: positions 1-4 are `commitment`, `epoch=0`, `admin_pubkey_commitment`, `group_id_fr` (in that order, IC[1..=4]; IC[0] base). 5 IC points total. `group_id_fr` is contract-derived from the entrypoint's `group_id` parameter (`Fr::from_u256(U256::from_be_bytes(group_id))`) — not on the wire.

### `update_commitment`

```rust
group_id: BytesN<32>
proof: Groth16Proof
public_inputs: UpdatePublicInputs  // { c_old, epoch_old, c_new }  // 3 wire scalars
```

The Update-circuit verifier opens 5 public inputs: positions 1-3 are `c_old`, `epoch_old`, `c_new` (from wire); position 4 is `admin_pubkey_commitment` (read from `AdminCommitment(group_id)` storage); position 5 is `group_id_fr` (contract-derived from `group_id` bytes). 6 IC points total (base + 5 inputs).

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
| **Phase A (circuits)** — `TyrannyCreateCircuit` (4 public inputs / 5 IC, witness includes initial member tree + admin BLS pubkey + secret key — proves `Poseidon(admin_pubkey, group_id_fr) == admin_pubkey_commitment`) and `TyrannyUpdateCircuit` (5 public inputs / 6 IC, witness includes admin secret key + member trees before/after — proves the same per-group binding AND new tree differs from old by ≤1 leaf). Membership is the standard 3-IC circuit (2 public inputs / 3 IC). **`group_id_fr` is the per-group salt** that closes cross-group admin-linkability (see §3.1). **Phase A blockers exist.** No prior keyset has these circuits. PR-Y ships with mock-VK fixture wiring; the deploy script accepts `FIXTURE_DIR` of dev VKs. | Pending |
| **Phase B (FFI)** — `generateTyrannyCreateProof`, `generateTyrannyUpdateProof` proof-generation paths in the Rust core. Bounded; ≤ 2 days once Phase A circuits are defined. | Pending |
| **Phase C (contract)** — this design + PR-Y. | Owned by PR-Y |
| **Phase D (clients)** — `swift-mls` SDK, iOS, Android. Out of scope for design + contract PRs. | Pending |
| **Phase E (ceremony)** — separate ceremony required (Tyranny VKs are net new vs. keyset-v2). Resolution after Phase A. | Pending |

## 11. Test plan

Inline tests parallel to `sep-anarchy`'s pattern. **~41 tests** total (comparable to Anarchy's 36 — net positive from the additional admin-binding tests, the per-group `group_id_fr` derivation, and `get_history` slice-coverage; PR-Y pins the exact count via `test_vectors_consistency`).

- **Initialization (4)**: happy path + 3 IC-arity rejections. Membership = 3 IC (2 public inputs + base), Create = 5 IC (4 public inputs + base), Update = 6 IC (5 public inputs + base). The IC-arity rejection tests feed VKs one IC short of the expected count and assert `InvalidVkLength`.
- **`create_group` (11)**: happy path, invalid tier, duplicate id, non-canonical commitment, non-canonical admin_pubkey_commitment, invalid proof, restricted-mode admin-only + admin-can-create (positive), group-count cap, replay protection, public-inputs mismatch.
- **`update_commitment` (8)**: happy path, stale c_old, wrong epoch_old, non-canonical c_new, replayed proof, unknown group, **`admin_pubkey_commitment` invariance** (set up a group, attempt update, assert the admin commitment is unchanged whether or not the proof verifies), **epoch-overflow** (inject `current.epoch = u64::MAX` via `as_contract`, attempt update with matching `epoch_old`; assert `InvalidEpoch (11)`. Pins the `checked_add`-precedes-PIM ordering at the boundary; the path is unreachable at testnet scale by natural advancement but the ordering matters for forward-compat).
- **`verify_membership` (4)**: happy path, wrong commitment, wrong epoch, unknown group.
- **Admin entrypoints (6)**: `update_vk_requires_auth`, `set_restricted_mode_requires_auth`, rotates Membership / Create / Update, invalid tier.
- **Queries (7)**: `get_commitment` {happy, unknown}, `bump_group_ttl` {happy, unknown}, `get_history` {most-recent-suffix slicing happy, full-when-max-exceeds, unknown}. The `get_history` suffix-slicing coverage closes the off-by-one risk on `start = history.len() - cap`.
- **ABI pin (1)**: `test_vectors_consistency` — loads `test-vectors.json` and asserts error codes / IC counts (3/5/6) / `MAX_GROUPS_PER_TIER` / `tier_capacity` / total test count match the contract byte-for-byte.

Optional follow-up (not blocking v0): a **negative-auth pin** for `update_commitment` — call without `mock_all_auths` and assert the entrypoint reaches the verifier rather than panicking on `caller.require_auth()`. This locks the privacy invariant ("update_commitment does NOT require caller auth — proof IS the authorization") against future regressions. Same shape as Anarchy's analogous pattern; PR-Y will land it if testutils ergonomics permit.

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
