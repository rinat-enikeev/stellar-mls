# Protocol Soundness Analysis — PR #77 Governance

Audit scope: smart contract (`contracts/sep-xxxx/src/lib.rs`),
ZK circuit (`src/circuit/democracy.rs`), deployment scripts, and
client-side protocol handling (iOS + Android).

---

## 1  Smart-contract layer

### 1.1  Sound design (confirmed)

| Property | Mechanism | Lines |
|----------|-----------|-------|
| Commitment canonical encoding | Fr round-trip check before storage | 960-968, 826-829, 1090-1094, 1209-1213 |
| Epoch monotonicity | `checked_add(1)` with `InvalidEpoch` error | 952, 1054, 1205-1207 |
| Public-input binding | On-chain state asserted == caller values *before* Groth16 verify | 954-958, 1056-1067, 1189-1203 |
| Group-type immutability | `group_type` set at creation, carried forward on update | 997 |
| Routing enforcement | `update_commitment` rejects non-Anarchy; Democracy/Oligarchy have separate entry points | 945-949, 1040-1049 |
| VK family separation | `DataKey::VK`, `UpdateVK`, `UpdateVKByType`, `AdminUpdateVK` stored under distinct keys with per-kind IC-length validation | 463-471, 545-547 |
| Duplicate group-ID prevention | `group_exists` covers both V1 and V2 keys | 710, 819 |
| Cross-function proof replay | Global `UsedProof(hash)` key shared across all proof types | 423, 1657-1667 |
| Lazy V1→V2 migration | `load_group_v2` falls back to V1 with safe defaults | 1451-1477 |

### 1.2  Vulnerabilities found

#### V-C1 — `deactivate_group` bypasses governance quorum (medium-high)

`deactivate_group` (line 1318) accepts any valid membership proof
regardless of `group_type`.  A single member of a **Democracy** group
(which otherwise requires ≥50 % quorum for updates) can unilaterally
destroy the group.  Same for **Oligarchy** — a non-admin member can
deactivate.

The existing test at line 3951 documents this as an intentional "safety
valve."  **Recommendation:** add an explicit doc-comment on
`deactivate_group` stating the design rationale, so auditors don't flag
it repeatedly.  *(Fixed in this commit.)*

#### V-C2 — `UsedProof` TTL expiry allows theoretical proof replay (~30 days)

`record_proof` stores proof hashes in persistent storage with TTL =
`LEDGER_BUMP` (518 400 ledgers ≈ 30 days).  After eviction the same
proof bytes could be re-submitted.

**Mitigation already present:** the proof must still pass the Groth16
verifier against *current* on-chain state (`c_old`, `epoch_old`).
Because epochs increment monotonically, a replayed proof would need the
group to return to the exact same `(commitment, epoch)` tuple — which
cannot happen under normal operation.  The residual risk is negligible
but the design intent (permanent nullification) is not fully achieved.

**Recommendation:** consider bumping `UsedProof` TTLs inside
`bump_group_ttl`, or document this as an accepted limitation.

#### V-C3 — `create_oligarchy_group` does not validate `member_count` bounds (medium)

Unlike `create_group_v2` which validates `member_count` for 1v1 (== 2)
and Democracy (≥ 2, ≤ tier capacity), `create_oligarchy_group` stores
`member_count` verbatim.  A caller can pass `u32::MAX`.

*(Fixed in this commit — bounds check added.)*

#### V-C4 — Democracy delta `m_old + 1` can wrap at `u32::MAX` (low)

Line 1075: `m_new == m_old + 1` wraps when `m_old == u32::MAX`.
Mitigated by tier-capacity bounds (max 2048), but the arithmetic is not
inherently safe.

*(Fixed in this commit — rewritten with `saturating_add`.)*

#### V-C5 — `tier_capacity` returns 0 for undefined tiers (informational)

The `_ => 0` default arm at line 54 silently returns 0 capacity for
out-of-range tiers.  All current callers validate tier first, so this
is benign.  A future caller that skips validation would get a 0-capacity
tier that passes bounds checks for any nonzero `member_count`.

---

## 2  ZK circuit layer (`DemocracyUpdateCircuit`)

### 2.1  Sound design (confirmed)

| Property | Mechanism | Lines |
|----------|-----------|-------|
| Epoch progression | `epoch_new = epoch_old + 1` computed in-circuit (not a free public input) | 603 |
| Quorum floor | `2 * K >= member_count_old` + `active_bits[0] = 1` | 464, 471 |
| Signer uniqueness | Strict ascending leaf-index ordering for active slots | 405-436 |
| Delta mutual exclusion | `is_insert * is_remove = 0` + exhaustive covering by count constraints | 549-584 |
| Root binding via commitment | `c = Poseidon(Poseidon(root, epoch), salt)` — collision-resistant | 589-616 |
| Shared Merkle siblings | Same `delta_path` for old/new root is correct for single-leaf-change model | 530-535 |
| Range checks | `enforce_fits_in_bits` via canonical bit decomposition + upper-bit-zero enforcement | 628-634 |
| Field arithmetic | All values fit in u32/u64, far below BN254 scalar field modulus | Throughout |

### 2.2  Vulnerabilities found

#### V-Z1 — `member_count_old` is not bound to the tree (medium)

`member_count_old` is a public input provided by the contract, not
derived from the Merkle tree.  The circuit trusts whatever value the
caller provides.  A buggy contract supplying the wrong count would
bypass quorum.

**Mitigation:** the contract binds `member_count_old` to its stored
`member_count` at line 1065 (`MemberCountMismatch` error).  This is a
single-point-of-failure trust boundary.

**Recommendation:** consider binding `member_count` into the
commitment (`c = Poseidon(Poseidon(root, epoch, member_count), salt)`)
so the circuit can independently verify it.  This would require a
commitment-scheme migration.

#### V-Z2 — No test coverage for Insert and Remove delta operations (medium)

All circuit tests use `build_replace_scenario` (Replace delta only).
Insert (new member) and Remove (evict member) are untested.

**Recommendation:** add Insert and Remove test scenarios before the
Phase 2 ceremony.

#### V-Z3 — No explicit domain separation between leaf and internal node hashes (low)

Leaf hashing uses `poseidon_hash_one(sk)` (1 input) and internal nodes
use `poseidon_hash_two(left, right)` (2 inputs).  The Poseidon sponge
provides *implicit* domain separation (different absorption count), but
an explicit domain tag would add defence-in-depth against
second-preimage attacks.

#### V-Z4 — Quorum is ≥50 % not strict majority for even counts (informational)

`2*K >= member_count_old` means a 2-member Democracy group requires only
1 signer for updates.  This matches the spec ("at least half") but may
surprise operators expecting strict majority.

---

## 3  Deployment scripts

### 3.1  Vulnerabilities found

#### V-D1 — No mainnet guard on install scripts (medium)

Both `install-democracy-vks-testnet.sh` and
`install-adminupdate-vk-testnet.sh` accept `NETWORK=mainnet` despite
using dev VKs.  Running
`NETWORK=mainnet ./scripts/install-democracy-vks-testnet.sh` would
install dev verification keys (for which anyone can regenerate the
proving key) onto a production contract.

*(Fixed in this commit — both scripts now reject mainnet/public.)*

#### V-D2 — Misleading "subshell-safe" comment (low)

Both install scripts say "Source the env file in a subshell-safe way"
but actually source directly into the current shell via `. "$ENV_FILE"`.
A compromised `.env` file would execute arbitrary commands.

*(Fixed in this commit — comment corrected.)*

#### V-D3 — No VK integrity verification at install time (low)

Neither install script verifies a hash/checksum of the VK file before
submitting it to the contract.  The ceremony doc describes verification
steps, but they are not enforced by tooling.

**Recommendation:** print SHA-256 of VK files and require operator
confirmation before submission.

---

## 4  Client-side protocol

### 4.1  Vulnerabilities found

#### V-P1 — Both platforms always use V1 `create_group` (high — not yet exploitable)

Both iOS and Android `publishGroupCreation` call the V1 `create_group`
entrypoint, which creates an Anarchy group with `member_count = 0`
regardless of the UI-selected governance type.  The V2 contract
entrypoints (`create_group_v2`, `create_oligarchy_group`) are defined
in the SDK types but never invoked.

**Impact:** until the client dispatch is wired to V2, **all groups are
Anarchy on-chain** even if labelled otherwise in the UI.

**Note:** this is likely phase-gated (waiting for ceremony-produced VKs).
The contract-side enforcement is present; the client integration is the
remaining gap.

#### V-P2 — Oligarchy admin state is purely client-side (high — ceremony-gated)

Admin promotions/demotions are broadcast as plaintext system messages
over the group transport.  The `adminRoot` Poseidon commitment is never
submitted on-chain because `create_oligarchy_group` is never called
(see V-P1).  Until the AdminUpdate circuit ceremony lands, oligarchy
governance is enforcement-free.

#### V-P3 — Invite-code governance type not verified against on-chain state (medium)

When joining a group, the governance type is taken from the invite code
without cross-checking `get_state_v2`.  A malicious invite could claim
Democracy while the on-chain group is Anarchy.

#### V-P4 — Democracy ballot finalization lacks quorum re-verification (medium)

`finalizeBallot()` broadcasts a marker and immediately executes the
removal without re-checking that quorum is still met.  Any member can
broadcast the finalization marker.

---

## 5  Summary matrix

| ID | Layer | Severity | Status |
|----|-------|----------|--------|
| V-C1 | Contract | Medium-High | Documented (design decision) |
| V-C2 | Contract | Medium | Accepted (epoch-binding mitigates) |
| V-C3 | Contract | Medium | **Fixed** |
| V-C4 | Contract | Low | **Fixed** |
| V-C5 | Contract | Informational | Noted |
| V-Z1 | Circuit | Medium | Recommendation logged |
| V-Z2 | Circuit | Medium | Recommendation logged |
| V-Z3 | Circuit | Low | Noted |
| V-Z4 | Circuit | Informational | By design |
| V-D1 | Scripts | Medium | **Fixed** |
| V-D2 | Scripts | Low | **Fixed** |
| V-D3 | Scripts | Low | Recommendation logged |
| V-P1 | Client | High | Phase-gated (pending ceremony) |
| V-P2 | Client | High | Phase-gated (pending ceremony) |
| V-P3 | Client | Medium | Recommendation logged |
| V-P4 | Client | Medium | Recommendation logged |
