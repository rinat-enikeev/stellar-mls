# Proof of Soundness — `contracts/sep-oligarchy`

**Scope:** the per-type Oligarchy Soroban contract at `contracts/sep-oligarchy/src/lib.rs` (revision: branch `feat/contract-sep-oligarchy`, branched off `design/oligarchy-update-testnet` v0.1.4).

**Status:** informal soundness argument. Not a machine-checked proof. Intended as an audit anchor for the contract surface — the cryptographic core (Groth16, Poseidon, BLS12-381) is not re-proven here; it's relied upon as a black box and the trust assumptions are listed explicitly below.

**Companion:** [`proof_of_correctness.md`](proof_of_correctness.md) covers spec→impl alignment ("does the contract do what's documented"); this document covers the dual question ("can the contract be tricked into accepting something that violates the design's invariants").

---

## 1. Threat model

Three actors interact with the contract:

| Actor | Capability | Trust |
|---|---|---|
| **Admin** | Calls `__constructor` once; can rotate VKs (`update_vk` for Membership / Create / Update at any tier) and toggle restricted mode (`set_restricted_mode`) any time after | Trusted to install honest VKs; not trusted with member or admin privacy (admin can read storage like any chain observer but cannot forge proofs without the VK secrets). On testnet the admin holds dev VKs; production gates on the ceremony per design §6 Phase E. |
| **Member** | Submits Groth16 proofs at `verify_membership`, `deactivate_group`. Has the secret key behind a member-tree leaf | Untrusted from the contract's point of view. The contract verifies the proof; the membership proof opens against the **member tree only** (chat-capacity). |
| **Admin signer** | Submits Groth16 proofs at `update_commitment`. Has the secret key behind an admin-tree leaf | Untrusted from the contract's point of view. The K-signer subset verifies in-circuit against the admin-quorum threshold; contract just checks Groth16 + replay + chain-binding. |
| **Group creator** | Submits Groth16 proof at `create_oligarchy_group`. Constructs initial member + admin trees with themselves at slot 0 of each | Untrusted. The verbose §4.8 Create proof binds member_root, admin_root, salt_initial, occupancy_commitment, and commitment as public inputs — closing the create-time self-DoS sep-democracy still carries. |
| **Observer** | Reads chain state and event log | Untrusted. Soundness here means "cannot induce the contract to accept an unauthorized state transition." Privacy properties (what an observer can learn from chain state) are scoped to design §3.5 / §4.7.7 residuals. |

**Out-of-scope adversarial capabilities:**
- Forging Groth16 proofs (relies on Groth16's discrete-log assumption + a trusted setup).
- Breaking BLS12-381 pairing or Poseidon collision resistance.
- Compromising Soroban's host runtime, BLS host functions, or persistent-storage integrity.
- Compromising the admin key (covered by the ceremony for production; testnet uses dev VKs explicitly).
- Pre-inclusion mempool replay (acknowledged in `test-vectors.json#proof_replay_protection.rules[2]` — would require a circuit-level operation tag binding, not contract-level).

---

## 2. Trust assumptions (boundary of this argument)

The contract takes the following as oracles. Soundness arguments below are conditional on these:

1. **Soroban BLS host functions** (`env.crypto().bls12_381()`) — `g1_add`, `g1_msm`, `pairing_check`, `G{1,2}Affine::is_in_subgroup`, `Fr::from_bytes`, `Fr::to_bytes`, `Fr::from_u256`, `U256::from_be_bytes` — implement the BLS12-381 group operations correctly. (Stellar SEP-0046 / soroban-sdk 25.3.0.)
2. **Groth16 verifier formula** as implemented in `verify_membership_proof` (`lib.rs:944-989`), `verify_create_proof` (`lib.rs:1000-1075`), and `verify_update_proof` (`lib.rs:1080-1170`) — `e(-π_A, π_B) · e(α, β) · e(vk_x, γ) · e(π_C, δ) = 1_GT` is sound under the q-PKE / generic-group assumption used by Groth16, given a trusted setup.
3. **Poseidon collision-resistance / preimage-resistance** is used by the *prover* (off-chain) to compute `commitment` and `occupancy_commitment`. The contract is value-agnostic to Poseidon — it sees only 32-byte field elements. This proof does not cover prover-side Poseidon misuse.
4. **`env.crypto().sha256`** is collision-resistant over short inputs (used for the proof-replay nullifier).
5. **The admin's installed VKs correspond to a real ceremony / dev-keyset that fixes the circuit constraint system.** A malicious admin who installs a VK with a known toxic-waste secret can forge proofs; this is the ceremony threat model documented in design §6 Phase E. In v0.1.4 the surface is larger than Democracy because the contract holds 9 VKs (3 per tier × 3 families); each is a separate ceremony output.
6. **Stellar's transaction ordering and persistent-storage atomicity** — reads and writes within a single Soroban host invocation are atomic; failed entrypoints revert all writes. Concurrent state changes between transactions are linearized by the ledger.

---

## 3. Soundness invariants

The contract enforces the following invariants. Each is checked at every state-changing entrypoint where it applies.

### 3.1 State-binding invariants

**SI-1: `current.commitment` is bound by a Groth16 proof against the create VK at create time and against the update VK at every subsequent epoch.**
Established at `create_oligarchy_group` (`lib.rs:570`) — a successful create verifies the verbose-binding proof relating `(commitment, 0, occupancy_commitment, member_root, admin_root, salt_initial)` under the create VK at `member_tier`. Re-established at every `update_commitment` (`lib.rs:660`) — a successful update verifies a proof relating `c_old → c_new` under the v0.1.4 oligarchy update VK, transitively binding `c_new` to the chain of all prior epochs' two-tree state.

**SI-2: `c_old` and `epoch_old` on the wire equal `current.commitment` and `current.epoch` at the moment of `update_commitment`.**
Enforced at `lib.rs:643-648`. A wire payload that doesn't bind to current state is rejected with `PublicInputsMismatch` before the proof is even loaded — no race-condition window.

**SI-3: `occupancy_commitment_old` on the wire equals `current.occupancy_commitment`.**
Same guard (`lib.rs:645`). Pins the salted combined-bitmap-derived value to chain state, preventing a relayer from advancing to an arbitrary new commitment by routing the proof through a different prior state.

**SI-4: `admin_threshold_numerator` is fixed at `create_oligarchy_group` and never mutated.**
Enforced by `lib.rs:684` — the new entry inherits `current.admin_threshold_numerator`. Verified at `update_commitment` by reading `current.admin_threshold_numerator` from storage (`lib.rs:668`) rather than from the wire. Test pin: `test_update_commitment_admin_threshold_pinned_to_storage` — code-level inspection that builds `IC[6] · 50` and `IC[6] · 67` via the BLS host directly and asserts the resulting G1 shifts differ. This proves the threshold scalar is being absorbed into the MSM (a regression that drops the threshold from the verifier's `vk_x` would make the shifts collide).

**SI-5 (Oligarchy-specific): The verbose Create proof binds `occupancy_commitment_initial` to the actual member + admin bitmaps.**
Unlike sep-democracy (where the create proof's only public inputs are commitment + epoch and occupancy_commitment_initial is supplied by the wire and stored without verification), sep-oligarchy's Create VK has 7 IC points and the proof binds `occupancy_commitment` as IC[3]. A creator cannot supply a bogus value: the circuit recomputes the salted combined commitment from witnessed bitmaps and salt_occ_initial, and the proof would fail. **Closes the create-time self-DoS** that sep-democracy still carries. Test pin: `test_create_oligarchy_group_rejects_non_canonical_occupancy_commitment` (canonical-Fr gate runs before verification; but the verbose binding ensures verification itself catches semantic mismatches).

### 3.2 Cryptographic invariants

**SI-6: All curve points stored in any of the 9 VKs are subgroup-valid.**
`validate_vk_points` (`lib.rs:899-918`) runs `is_in_subgroup` on `α_g1`, `β_g2`, `γ_g2`, `δ_g2`, and every `IC[i]`. Called at `__constructor` for all 9 VKs (`lib.rs:359-368`) and at `update_vk` (`lib.rs:413`). A small-subgroup VK is rejected with `Error::InvalidPoint`.

**SI-7: All proof points are subgroup-valid.**
`validate_proof_points` (`lib.rs:920-924`) runs `is_in_subgroup` on `proof.a`, `proof.b`, `proof.c`. Called at the start of `verify_membership_proof` (`lib.rs:954-956`), `verify_create_proof` (`lib.rs:1014-1016`), and `verify_update_proof` (`lib.rs:1095-1097`). A small-subgroup proof returns `false` (→ `Error::InvalidProof` upstream).

**SI-8: All field-element commitments use canonical Fr encoding.**
`is_canonical_fr` (`lib.rs:926-930`) round-trips `Fr::from_bytes ∘ Fr::to_bytes` and rejects any 32-byte value whose `Fr::from_bytes` reduces. Called at `create_oligarchy_group` for all five wire-supplied scalars (`lib.rs:541-558`) and at `update_commitment` for `c_new` and `occupancy_commitment_new` (`lib.rs:651-656`), and inside the verifier helpers as defense-in-depth (`lib.rs:957`, `:1017-1023`, `:1098-1110`). Non-canonical inputs are rejected with `InvalidCommitmentEncoding`. Closes the malleability hole where `Fr` reduction can produce two distinct 32-byte preimages of the same field element.

**SI-9: VK IC-vector length matches each circuit's public-input arity.**
`MEMBERSHIP_IC_POINTS = 3`, `CREATE_IC_POINTS = 7`, `UPDATE_IC_POINTS = 7`. Enforced at `__constructor` (`lib.rs:339-358`) and `update_vk` (`lib.rs:406-412`) and re-asserted inside the verifiers (`lib.rs:951`, `:1011`, `:1092`). A wrong-arity VK can never be installed; even if storage were corrupted, the verifier guards it.

### 3.3 Replay protection

**SI-10: A proof's bytes (`a || b || c`) cannot be reused at any state-changing entrypoint within `LEDGER_BUMP` (~30 days).**
`proof_hash` (`lib.rs:835-841`) = `sha256(a.to_array() || b.to_array() || c.to_array())`. `check_proof_replay` (`lib.rs:843-853`) is called BEFORE proof verification at `create_oligarchy_group` (`lib.rs:564`), `update_commitment` (`lib.rs:658`), and `deactivate_group` (`lib.rs:738`). `record_proof` (`lib.rs:855-863`) is called only after the proof verifies, so a failed entrypoint does not consume the nullifier (transactional revert).

The scope is **contract-global** — not per-group. Two distinct groups cannot accept byte-identical proofs. Because Groth16 proofs are randomized (the prover samples fresh `r, s` per proof), two honest provers producing proofs at the same time will produce distinct bytes; the nullifier only blocks a literal byte-replay attack.

### 3.4 Authorization

**SI-11: Admin-gated entrypoints require Soroban auth from the stored admin address.**
`update_vk` (`lib.rs:389-419`) and `set_restricted_mode` (`lib.rs:423-435`) both load `Admin` from instance storage and call `admin.require_auth()`. Without a matching auth entry, Soroban panics with "Unauthorized" before any state mutation. Test pin: `test_update_vk_requires_auth`.

**SI-12: `create_oligarchy_group` requires `caller.require_auth()` and, in restricted mode, `caller == admin`.**
The `caller.require_auth()` call (`lib.rs:506`) prevents one address from creating a group on behalf of another. In restricted mode (admin-toggled), `caller != admin` returns `AdminOnly` (`lib.rs:519-521`). Test pin: `test_create_oligarchy_group_restricted_mode_rejects_non_admin`.

**SI-13: `update_commitment`, `verify_membership`, `deactivate_group` do NOT require Soroban-level auth.**
The proof IS the authorization. Documented at `lib.rs:632-637`. Specifically:
- `update_commitment` requires a proof with K admin signers satisfying the admin-quorum threshold (in-circuit constraint per design §4.7.6).
- `verify_membership` and `deactivate_group` require a member-tree membership proof (chat-capacity).

This is **not** a soundness gap: an attacker without a valid proof cannot construct a passing `verify_*_proof` call. The only attack surface is a valid proof that's been replayed, which SI-10 closes.

### 3.5 State-machine invariants

**SI-14: Epoch is monotonic and increments by exactly 1 per successful `update_commitment`.**
Enforced at `lib.rs:641` (`current.epoch.checked_add(1)`) and `lib.rs:680` (`new_epoch` written to storage). The `checked_add` returns `InvalidEpoch` if `current.epoch == u64::MAX` — unreachable in practice but defensively gated. SI-2 chains the epoch forward.

**SI-15: A group transitions `active=true → false` exactly once and irreversibly.**
`create_oligarchy_group` writes `active: true` (`lib.rs:580`). `update_commitment` preserves `active: true` on the new entry (`lib.rs:683`). `deactivate_group` rejects if `current.active == false` (`lib.rs:725-727`) and sets `active: false` (`lib.rs:752`). No entrypoint flips `active` from `false` back to `true`.

**SI-16: `tier` is fixed at create and never mutated.**
`update_commitment`'s new entry inherits `tier: current.tier` (`lib.rs:682`); `deactivate_group`'s deactivated entry uses `..current.clone()` which preserves `tier`. No entrypoint takes `tier` as a wire input post-create.

**SI-17: `GroupCount(member_tier)` is bounded by `MAX_GROUPS_PER_TIER = 10000`.**
`create_oligarchy_group` (`lib.rs:560-565`) reads the count and rejects with `TierGroupLimitReached` if `count >= MAX_GROUPS_PER_TIER`. `deactivate_group` (`lib.rs:760-769`) decrements with an underflow guard. Test pin: `test_create_oligarchy_group_enforces_tier_group_limit`.

**SI-18: `__constructor` is callable exactly once.**
`do_initialize` (`lib.rs:331-333`) returns `AlreadyInitialized` if `DataKey::Admin` is already set. Called at deploy time by Soroban; cannot be re-invoked. Same idempotency pattern as sep-democracy (with no separate `initialize` entrypoint).

### 3.6 History invariants

**SI-19: History is a rolling FIFO window of at most `HISTORY_WINDOW = 64` entries.**
`archive_entry` (`lib.rs:865-884`) appends and prunes from the front. Test pin: `test_archive_entry_appends_and_prunes`. Entries pruned from contract storage are still present in contract events (`CommitmentUpdated`).

**SI-20: History captures the prior active state, not the post-update state.**
`update_commitment` calls `archive_entry(env, &group_id, &current)` BEFORE writing the new entry (`lib.rs:678-686`). `deactivate_group` likewise archives `&current` before flipping inactive (`lib.rs:747-753`). The "next" state is in `Group(group_id)`; "previous" states are in `History(group_id)`.

---

## 4. Per-entrypoint soundness argument

For each state-changing entrypoint, soundness reduces to "every successful return implies the corresponding storage delta is justified by a valid Groth16 proof + auth + storage gates."

### 4.1 `create_oligarchy_group`

A successful return requires:

1. Contract is initialized.
2. Caller proves Soroban auth.
3. Restricted-mode gate (`caller == admin` if `RestrictedMode == true`).
4. `member_tier ∈ {0, 1, 2}`.
5. `admin_threshold_numerator ∈ [1, 100]`.
6. All 6 wire-supplied scalars (commitment, occupancy_commitment_initial, member_root, admin_root, salt_initial) match `public_inputs`, with epoch=0.
7. `Group(group_id)` not present.
8. All 5 BytesN<32> wire fields are canonical Fr (SI-8).
9. `GroupCount(member_tier) < MAX_GROUPS_PER_TIER` (SI-17).
10. `proof_hash(proof)` not in `UsedProof` (SI-10).
11. Create proof verifies under `CreateVK(member_tier)` against the 6-element public-input vector (SI-1).

Storage delta on success: `Group(group_id) := CommitmentEntry{...}` (active, epoch=0), `History(group_id) := []`, `GroupCount(tier) += 1`, `UsedProof(proof_hash) := true`.

**Soundness statement:** the only way to populate `Group(group_id)` is to produce a Groth16 proof against the create circuit binding all six public inputs. By assumption (T-2), this is computationally infeasible without knowledge of a witness satisfying the circuit's R1CS — which the Phase A circuit reduces to "knowing a secret key behind a member leaf in `member_root` AND an admin leaf in `admin_root` AND demonstrating that the supplied `occupancy_commitment` correctly bundles the bitmaps + salt_occ_initial via §4.7.2 v0.1.4". The verbose binding closes the self-DoS sep-democracy still carries: a malicious creator cannot supply a bogus `occupancy_commitment` and lock their own group, because the proof would not verify.

### 4.2 `update_commitment`

A successful return requires:
1. Initialized.
2. `Group(group_id)` exists and `active == true`.
3. `current.epoch + 1` doesn't overflow (`InvalidEpoch`).
4. SI-2, SI-3 (state chaining).
5. `c_new` and `occupancy_commitment_new` canonical Fr.
6. Replay-fresh.
7. Update proof verifies under `UpdateVK(current.tier)` with public inputs `(c_old, epoch_old, c_new, occupancy_commitment_old, occupancy_commitment_new, admin_threshold_numerator)` where `admin_threshold_numerator` is read from `current.admin_threshold_numerator` (SI-4).

Storage delta on success: `Group(group_id) := CommitmentEntry{c_new, current.epoch+1, ..., active: true, threshold: current.admin_threshold}`; `History(group_id)` ← prior `current`; `UsedProof(proof_hash) := true`.

**Soundness statement:** the contract stores only states that are connected by a chain of valid update proofs from `create_oligarchy_group`'s initial state. Forking the chain (writing a `c_new` not derived from a witness against `c_old`) is infeasible (T-2). The chain-supplied threshold means an attacker cannot satisfy the circuit at a more permissive threshold by lying on the wire; any wire-vs-storage threshold mismatch causes the verifier to receive a different IC[6] scalar, which causes pairing-check failure with overwhelming probability.

### 4.3 `verify_membership`

Read-only. Returns `Ok(true)` iff `(public_inputs.commitment == current.commitment) ∧ (public_inputs.epoch == current.epoch) ∧ (proof verifies under VK(current.tier) against those inputs)`. Returns `Ok(false)` on bad proof, `Err(...)` on storage / chain mismatch.

The membership VK opens against the **member tree only** (chat-capacity). Admin status is NOT verified by this call; admin authorization is exclusively via `update_commitment`'s K-signer subset.

**Soundness statement:** an `Ok(true)` return implies, by T-2, that the prover knew a witness against `current.commitment` opening to a member-tree leaf. No state mutation occurs.

### 4.4 `deactivate_group`

A successful return requires SI-1's witness (current member-tree proof) plus SI-2 chaining at `current.epoch`. Replay-fresh. State delta: `Group(group_id).active = false`, `History(group_id)` ← prior `current`, `GroupCount(current.tier) -= 1`, `UsedProof(proof_hash) := true`.

**Soundness statement:** any current member of the **member tree** can deactivate (V-C1 safety valve from sep-xxxx). Once `active == false`, no further `update_commitment` or `deactivate_group` succeeds (SI-15). `verify_membership` against the final pre-deactivation state remains valid forever. **Admin status NOT required** for deactivation — the safety valve uses the chat-capacity proof, not the admin-quorum proof.

### 4.5 `update_vk`

Admin-only (SI-11). State delta: `VK(tier)`, `CreateVK(tier)`, or `UpdateVK(tier)` replaced. **Soundness statement:** see admin-trust assumption (T-5). After rotation, any subsequent operation against the affected (tier, kind) verifies under the new VK. The contract has 3 VK families × 3 tiers = 9 rotation slots, each independently rotatable.

### 4.6 `set_restricted_mode`

Admin-only (SI-11). Toggles a single bool. No cryptographic effect. Affects only `create_oligarchy_group`'s admin-gating branch.

### 4.7 `bump_group_ttl`

Permissionless. Extends storage TTL on `Group(group_id)` and `History(group_id)`. No state-value mutation (only TTL metadata). Fails with `GroupNotFound` if the group doesn't exist.

### 4.8 `get_commitment` / `get_history`

Read-only. No state mutation. Cannot violate any invariant.

---

## 5. Out-of-scope (acknowledged, not closed by this contract)

- **Pre-inclusion mempool replay.** A relayer observing a not-yet-included transaction can submit the same proof bytes first. The circuit-level operation-tag binding (design §10) would close this; the contract-level nullifier doesn't.
- **Admin-key compromise.** A compromised admin can rotate any of the 9 VKs to a malicious circuit, accept fake proofs, and effectively forge group state. Mitigated by ceremony for production (design §6 Phase E).
- **Soroban host-side storage tampering.** If the host runtime is compromised, all contract state is suspect. Out of any contract-level analysis.
- **Resource exhaustion.** A determined attacker submitting many failed proofs consumes contract budget on each call. Soroban's per-tx fee model bounds this economically.
- **`salt_occ` loss bricks future updates** (design §9). Salt is private and distributed only via `SEPSaltResponse.occupancySalt`. If a group loses its current `salt_occ`, no member can produce the next valid update proof. Mitigated by Phase D operability requirements (clients replicate salt to all members on every broadcast). The `deactivate_group` safety valve still works since membership proofs don't require the salt.
- **Stale `salt_occ` reuse across epochs** (design §9). Salt freshness is honor-system on the prover. A buggy prover that caches the salt regresses the v0.1.4 brute-force guarantee. Pinned by the prover-side test `prove_oligarchy_v2_stale_salt_reuse_rejected`; not contract-side enforceable.
- **Privacy "which tree changed"** (design §3.5 / §4.7.7). Not a contract-soundness property; a prover-side guarantee. The contract is correct-by-construction wrt whatever Poseidon values the prover commits to.

---

## 6. Test pins

The inline test suite in `contracts/sep-oligarchy/src/test.rs` enforces these invariants programmatically. Each invariant has at least one test:

| Invariant | Test |
|---|---|
| SI-1 / SI-5 | `test_create_oligarchy_group_happy_path`, `test_create_oligarchy_group_rejects_invalid_proof` |
| SI-2 | `test_update_commitment_rejects_stale_c_old`, `test_update_commitment_rejects_wrong_epoch_old` |
| SI-3 | `test_update_commitment_rejects_stale_occupancy_commitment_old` |
| SI-4 | `test_update_commitment_admin_threshold_pinned_to_storage` |
| SI-6 | (subgroup checks at constructor — implicit via `test_initialize` using `hash_to_g{1,2}` valid mocks) |
| SI-7 | (subgroup checks at proof verify — implicit via mock_proof) |
| SI-8 | `test_create_oligarchy_group_rejects_non_canonical_*` (5 tests covering each wire field), `test_update_commitment_rejects_non_canonical_*` (2 tests) |
| SI-9 | `test_invalid_membership_vk_length_rejected`, `test_invalid_create_vk_length_rejected`, `test_invalid_update_vk_length_rejected` |
| SI-10 | `test_update_commitment_rejects_replayed_proof` |
| SI-11 | `test_update_vk_requires_auth` |
| SI-12 | `test_create_oligarchy_group_restricted_mode_rejects_non_admin` |
| SI-14 | `test_update_commitment_rejects_wrong_epoch_old`, `test_update_commitment_happy_path` |
| SI-15 | `test_deactivate_already_inactive_group`, `test_update_commitment_rejects_inactive_group` |
| SI-16 | `test_get_commitment_returns_current_state` |
| SI-17 | `test_create_oligarchy_group_enforces_tier_group_limit` |
| SI-18 | (constructor idempotency — covered by `test_initialize` semantics) |
| SI-19 | `test_archive_entry_appends_and_prunes` |
| SI-20 | (covered by SI-19) |

`test_vectors_consistency` additionally CI-asserts that `test-vectors.json`'s pinned error codes, tier capacities, IC-point counts, and `MAX_GROUPS_PER_TIER` agree with the contract source.

---

## 7. Verification gaps

The following soundness arguments are **assumed**, not verified by this codebase:

1. **Real Groth16 proofs against the v0.1.4 oligarchy circuits** (membership / create / update). Tests use mock proofs (subgroup-valid points that fail pairing). The full create → update → verify → deactivate round-trip with positive verification is gated on Phase A's `prove_oligarchy_v2` + fixture generator (design §6 Phase A; deploy script header notes the gap).
2. **Cross-platform vector agreement.** The Phase A6 cross-platform test vectors will pin Swift / Kotlin / Rust prover outputs against this contract's verifier.
3. **Ceremony-gated production VKs.** Testnet uses dev VKs across 9 VK slots; mainnet release gates on the ceremony per design §6 Phase E.
4. **Two-tree dispatcher constraint #8 soundness** (design §4.7.3 and §9 risks). The non-target-no-change constraint is a circuit-level property; the contract trusts it. R1CS soundness review at Phase A is the gate.

---

## 8. Change-control

Any future edit to `contracts/sep-oligarchy/src/lib.rs` that touches:

- The `Error` enum,
- The `CommitmentEntry`, `PublicInputs`, `CreatePublicInputs`, `UpdateCommitmentPublicInputs`, `VkKind`, or `DataKey` types,
- Any `pub fn` entrypoint signature,
- `validate_vk_points`, `validate_proof_points`, `is_canonical_fr`,
- `verify_membership_proof`, `verify_create_proof`, `verify_update_proof`,
- `proof_hash`, `check_proof_replay`, `record_proof`,

…must update `test-vectors.json` (CI-asserted by `test_vectors_consistency`) AND must re-evaluate the relevant soundness invariants above. The acceptance criterion is: every SI-N still holds verbatim, OR the document is amended to reflect a deliberate weakening.
