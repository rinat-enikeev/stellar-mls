# Proof of Soundness — `contracts/sep-democracy`

> **Amendment (postmortem #153 phase 1):** Soundness invariants tied to `deactivate_group` describe a removed entrypoint. The mempool-front-run residual that this document originally acknowledged for `deactivate_group` is no longer reachable: there is no entrypoint that consumes a membership-VK-keyed proof to mutate live state. See `docs/postmortem-deactivate-group-frontrun.md` for rationale and the architectural argument for removal vs. v2-circuit work.

**Scope:** the per-type Democracy Soroban contract at `contracts/sep-democracy/src/lib.rs` (revision: branch `feat/contract-sep-democracy`, commit `37fedab` and later).

**Status:** informal soundness argument. Not a machine-checked proof. Intended as an audit anchor for the contract surface — the cryptographic core (Groth16, Poseidon, BLS12-381) is not re-proven here; it's relied upon as a black box and the trust assumptions are listed explicitly below.

**Companion:** [`proof_of_correctness.md`](proof_of_correctness.md) covers spec→impl alignment ("does the contract do what's documented"); this document covers the dual question ("can the contract be tricked into accepting something that violates the design's invariants").

---

## 1. Threat model

Three actors interact with the contract:

| Actor | Capability | Trust |
|---|---|---|
| **Admin** | Calls `__constructor` once; can rotate VKs (`update_vk`) and toggle restricted mode (`set_restricted_mode`) any time after | Trusted to install honest VKs; not trusted with member privacy (admin can read storage like any chain observer but cannot forge proofs without the VK secrets). On testnet the admin holds dev VKs; production gates on the ceremony per design §6 Phase E. |
| **Member** | Submits Groth16 proofs at `create_group`, `update_commitment`, `verify_membership`, `deactivate_group`. Has the secret key behind a member leaf | Untrusted from the contract's point of view. The contract verifies the proof; the proof is the authorization. |
| **Observer** | Reads chain state and event log | Untrusted. Soundness here means "cannot induce the contract to accept an unauthorized state transition." Privacy properties (what an observer can learn from chain state) are scoped to the design doc's §3.4 / §4.7.7 residuals, not this document. |

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
2. **Groth16 verifier formula** as implemented in `verify_membership_proof` (`lib.rs:927-972`) and `verify_update_proof` (`lib.rs:986-1060`) — `e(-π_A, π_B) · e(α, β) · e(vk_x, γ) · e(π_C, δ) = 1_GT` is sound under the q-PKE / generic-group assumption used by Groth16, given a trusted setup.
3. **Poseidon collision-resistance / preimage-resistance** is used by the *prover* (off-chain) to compute `commitment` and `occupancy_commitment`. The contract is value-agnostic to Poseidon — it sees only 32-byte field elements. This proof does not cover prover-side Poseidon misuse.
4. **`env.crypto().sha256`** is collision-resistant over short inputs (used for the proof-replay nullifier).
5. **The admin's installed VKs correspond to a real ceremony / dev-keyset that fixes the circuit constraint system.** A malicious admin who installs a VK with a known toxic-waste secret can forge proofs; this is the ceremony threat model documented in design §6 Phase E.
6. **Stellar's transaction ordering and persistent-storage atomicity** — reads and writes within a single Soroban host invocation are atomic; failed entrypoints revert all writes. Concurrent state changes between transactions are linearized by the ledger.

---

## 3. Soundness invariants

The contract enforces the following invariants. Each is checked at every state-changing entrypoint where it applies.

### 3.1 State-binding invariants

**SI-1: `current.commitment` is bound by a Groth16 proof against the membership VK at `current.tier`.**
Established at `create_group` (`lib.rs:509`) and re-established at every `update_commitment` (`lib.rs:592`) — a successful update verifies a proof relating `c_old → c_new` under the v2 democracy update VK, transitively binding `c_new` to the chain of all prior epochs' member sets.

**SI-2: `c_old` and `epoch_old` on the wire equal `current.commitment` and `current.epoch` at the moment of `update_commitment`.**
Enforced at `lib.rs:575-580`. A wire payload that doesn't bind to current state is rejected with `PublicInputsMismatch` before the proof is even loaded — no race-condition window.

**SI-3: `occupancy_commitment_old` on the wire equals `current.occupancy_commitment`.**
Same guard (`lib.rs:577`). Pins the bitmap-derived value to chain state, preventing a relayer from advancing the bitmap to an arbitrary new commitment by routing the proof through a different prior state.

**SI-4: `threshold_numerator` is fixed at `create_group` and never mutated.**
Enforced by `lib.rs:620` — the new entry inherits `current.threshold_numerator`. Verified at `update_commitment` by reading `current.threshold_numerator` from storage (`lib.rs:601`) rather than from the wire. Test pin: `test_update_commitment_threshold_supplied_from_storage`, `test_update_commitment_threshold_mismatch_rejected_v2`.

### 3.2 Cryptographic invariants

**SI-5: All curve points stored in a VK are subgroup-valid.**
`validate_vk_points` (`lib.rs:879-898`) runs `is_in_subgroup` on `α_g1`, `β_g2`, `γ_g2`, `δ_g2`, and every `IC[i]`. Called at `__constructor` (`lib.rs:343-348`) and `update_vk` (`lib.rs:406`). A small-subgroup VK is rejected with `Error::InvalidPoint`.

**SI-6: All proof points are subgroup-valid.**
`validate_proof_points` (`lib.rs:900-904`) runs `is_in_subgroup` on `proof.a`, `proof.b`, `proof.c`. Called at the start of `verify_membership_proof` (`lib.rs:937-939`) and `verify_update_proof` (`lib.rs:1000-1002`). A small-subgroup proof returns `false` (→ `Error::InvalidProof` upstream).

**SI-7: All field-element commitments use canonical Fr encoding.**
`is_canonical_fr` (`lib.rs:906-910`) round-trips `Fr::from_bytes ∘ Fr::to_bytes` and rejects any 32-byte input whose value ≥ Fr modulus, since `Fr::from_bytes` then reduces and the round-trip differs from the input (i.e., the wire bytes are NOT the canonical representation of the resulting Fr element). Called at `create_group` for `commitment` and `occupancy_commitment_initial` (`lib.rs:490-495`), at `update_commitment` for `c_new` and `occupancy_commitment_new` (`lib.rs:582-587`), and inside the verifier helpers as defense-in-depth (`lib.rs:940-942`, `:1003-1014`). A non-canonical value is rejected with `InvalidCommitmentEncoding`. The canonicalization closes the malleability hole where `Fr` reduction can produce two distinct 32-byte preimages of the same field element.

**SI-8: VK IC-vector length matches the circuit's public-input arity.**
`MEMBERSHIP_IC_POINTS = 3` (base + commitment + epoch); `UPDATE_IC_POINTS = 7` (base + 5 wire scalars + threshold). Enforced at `__constructor` (`lib.rs:328-341`) and `update_vk` (`lib.rs:399-405`) and re-asserted inside the verifiers (`lib.rs:934-936`, `:997-999`). A wrong-arity VK can never be installed; even if storage were corrupted, the verifier guards it.

### 3.3 Replay protection

**SI-9: A proof's bytes (`a || b || c`) cannot be reused at any state-changing entrypoint within `LEDGER_BUMP` (~30 days).**
`proof_hash` (`lib.rs:823-829`) = `sha256(a.to_array() || b.to_array() || c.to_array())`. `check_proof_replay` (`lib.rs:831-841`) is called BEFORE proof verification at `create_group` (`lib.rs:506`), `update_commitment` (`lib.rs:589`), and `deactivate_group` (`lib.rs:680`). `record_proof` (`lib.rs:843-851`) is called only after the proof verifies, so a failed entrypoint does not consume the nullifier (the storage write reverts on the verify-failure branch via Soroban's transactional semantics).

The scope is **contract-global** — not per-group. Two distinct groups cannot accept byte-identical proofs. Because Groth16 proofs are randomized (the prover samples fresh `r, s` per proof), two honest provers producing proofs at the same time will produce distinct bytes; the nullifier only blocks a literal byte-replay attack.

**SI-9 caveat:** The replay window is bounded by `UsedProof`'s TTL (`LEDGER_BUMP` ≈ 30 days). After expiry, the same proof bytes can be re-used. For long-lived groups this is mitigated by `bump_group_ttl` and by clients periodically advancing state. Pre-inclusion mempool replay (an attacker observing a transaction in the mempool and front-running with the same proof bytes) is **not** closed by this nullifier — it requires circuit-level operation-tag binding (design §10 / `test-vectors.json#proof_replay_protection.rules[2]`).

### 3.4 Authorization

**SI-10: Admin-gated entrypoints require Soroban auth from the stored admin address.**
`update_vk` (`lib.rs:382-412`) and `set_restricted_mode` (`lib.rs:416-428`) both load `Admin` from instance storage and call `admin.require_auth()`. Without a matching auth entry, Soroban panics with "Unauthorized" before any state mutation. Test pin: `test_update_vk_requires_auth`.

**SI-11: `create_group` requires `caller.require_auth()` and, in restricted mode, `caller == admin`.**
The `caller.require_auth()` call (`lib.rs:460`) prevents one address from creating a group on behalf of another (an attacker can't pass an arbitrary `caller` value because Soroban's auth framework verifies that `caller` actually authorized the invocation). In restricted mode (admin-toggled), `caller != admin` returns `AdminOnly` (`lib.rs:473-475`). Test pin: `test_create_group_restricted_mode_rejects_non_admin`.

**SI-12: `update_commitment`, `verify_membership`, `deactivate_group` do NOT require Soroban-level auth.**
The proof IS the authorization — the prover demonstrated knowledge of a member-leaf secret key. Any address may submit the call (relayer model). Documented at `lib.rs:555-559`. This is **not** a soundness gap: an attacker without a valid proof cannot construct a passing `verify_*_proof` call. The only attack surface is a valid proof that's been replayed, which SI-9 closes.

### 3.5 State-machine invariants

**SI-13: Epoch is monotonic and increments by exactly 1 per successful `update_commitment`.**
Enforced at `lib.rs:573` (`current.epoch.checked_add(1)`) and `lib.rs:613` (`new_epoch` written to storage). The `checked_add` returns `InvalidEpoch` if `current.epoch == u64::MAX` — unreachable in practice but defensively gated. SI-2 chains the epoch forward (any wire `epoch_old` mismatching `current.epoch` rejects).

**SI-14: A group transitions `active=true → false` exactly once and irreversibly.**
`create_group` writes `active: true` (`lib.rs:521`). `update_commitment` preserves `active: true` on the new entry (`lib.rs:616`). `deactivate_group` rejects if `current.active == false` (`lib.rs:672-674`) and sets `active: false` (`lib.rs:695`). No entrypoint flips `active` from `false` back to `true`.

**SI-15: `tier` is fixed at create and never mutated.**
`update_commitment`'s new entry inherits `tier: current.tier` (`lib.rs:615`); `deactivate_group`'s deactivated entry uses `..current.clone()` which preserves `tier`. No entrypoint takes `tier` as a wire input post-create.

**SI-16: `GroupCount(tier)` is bounded by `MAX_GROUPS_PER_TIER = 10000`.**
`create_group` (`lib.rs:497-504`) reads the count and rejects with `TierGroupLimitReached` if `count >= MAX_GROUPS_PER_TIER`. `deactivate_group` (`lib.rs:704-713`) decrements with an underflow guard (`if count > 0`). Test pin: `test_create_group_enforces_tier_group_limit`.

**SI-17: `__constructor` is callable exactly once.**
`do_initialize` (`lib.rs:323-325`) returns `AlreadyInitialized` if `DataKey::Admin` is already set. Called at deploy time by Soroban; cannot be re-invoked. The redundant `initialize` post-deploy entrypoint was removed in PR #148 review (chunk 2 #11).

### 3.6 History invariants

**SI-18: History is a rolling FIFO window of at most `HISTORY_WINDOW = 64` entries.**
`archive_entry` (`lib.rs:853-872`) appends and prunes from the front. Test pin: `test_archive_entry_appends_and_prunes`. Entries pruned from contract storage are partially recoverable from `CommitmentUpdated` events: the membership-commitment chain (`commitment, epoch, timestamp`) is preserved past the window, but the `occupancy_commitment` chain past `HISTORY_WINDOW` is NOT recoverable from events alone — `CommitmentUpdated`'s payload omits `occupancy_commitment`. Reconstructing the full occupancy chain past the window would require either retaining a longer history window or amending the event payload.

**SI-19: History captures the prior active state, not the post-update state.**
`update_commitment` calls `archive_entry(env, &group_id, &current)` BEFORE writing the new entry (`lib.rs:609-624`). `deactivate_group` likewise archives `&current` before flipping inactive (`lib.rs:693-701`). The "next" state is in `Group(group_id)`; "previous" states are in `History(group_id)`.

---

## 4. Per-entrypoint soundness argument

For each state-changing entrypoint, soundness reduces to "every successful return implies the corresponding storage delta is justified by a valid Groth16 proof + auth + storage gates."

### 4.1 `create_group`

A successful return requires (in source order):

1. Contract is initialized (SI-17 inverse).
2. Caller proves Soroban auth.
3. Restricted-mode gate (`caller == admin` if `RestrictedMode == true`).
4. `tier ∈ {0, 1, 2}`.
5. `threshold_numerator ∈ [1, 100]`.
6. `public_inputs.commitment == commitment ∧ public_inputs.epoch == 0`.
7. `Group(group_id)` not present.
8. `commitment` and `occupancy_commitment_initial` are canonical Fr (SI-7).
9. `GroupCount(tier) < MAX_GROUPS_PER_TIER` (SI-16).
10. `proof_hash(proof)` not in `UsedProof` (SI-9).
11. Membership proof verifies under `VK(tier)` against `(commitment, 0)` public inputs.

Storage delta on success: `Group(group_id) := CommitmentEntry{...}` (active, epoch=0, supplied threshold/tier/commitments), `History(group_id) := []`, `GroupCount(tier) += 1`, `UsedProof(proof_hash) := true`.

**Soundness statement:** the only way for a non-admin (in restricted mode) or for any caller (in unrestricted mode) to populate `Group(group_id)` is to produce a Groth16 proof against the membership circuit, against a `commitment` that is canonical-Fr-valid, with `epoch=0`. By assumption (T-2), this is computationally infeasible without knowledge of a witness satisfying the circuit's R1CS — which the Phase A circuit reduces to "knowing a secret key behind a member leaf in the Merkle tree rooted at `commitment`." A successful create therefore implies the existence of such a witness.

### 4.2 `update_commitment`

A successful return requires:

1. Initialized.
2. `Group(group_id)` exists and `active == true`.
3. `current.epoch + 1` doesn't overflow (`InvalidEpoch`).
4. SI-2, SI-3 (state chaining).
5. `c_new` and `occupancy_commitment_new` canonical Fr.
6. Replay-fresh.
7. Update proof verifies under `UpdateVK(current.tier)` with public inputs `(c_old, epoch_old, c_new, occupancy_commitment_old, occupancy_commitment_new, threshold_numerator)` where `threshold_numerator` is read from `current.threshold_numerator` (SI-4).

Storage delta on success: `Group(group_id) := CommitmentEntry{c_new, current.epoch+1, ..., active: true, threshold: current.threshold}`; `History(group_id)` ← prior `current`; `UsedProof(proof_hash) := true`.

**Soundness statement:** the contract stores only states that are connected by a chain of valid update proofs from `create_group`'s initial state. Forking the chain (writing a `c_new` not derived from a witness against `c_old`) is infeasible (T-2). The chain-supplied threshold means an attacker cannot satisfy the circuit at a more permissive threshold by lying on the wire; any wire-vs-storage threshold mismatch causes the verifier to receive a different IC[6] scalar, which causes pairing-check failure with overwhelming probability (T-1, T-2).

### 4.3 `verify_membership`

Read-only. Returns `Ok(true)` iff `(public_inputs.commitment == current.commitment) ∧ (public_inputs.epoch == current.epoch) ∧ (proof verifies under VK(current.tier) against those inputs)`. Returns `Ok(false)` on bad proof, `Err(...)` on storage / chain mismatch.

**Soundness statement:** an `Ok(true)` return implies, by T-2, that the prover knew a witness against `current.commitment`. No state mutation occurs.

### 4.4 `deactivate_group`

A successful return requires SI-1's witness (current member proof) plus SI-2 chaining at `current.epoch`. Replay-fresh. State delta: `Group(group_id).active = false`, `History(group_id)` ← prior `current`, `GroupCount(current.tier) -= 1`, `UsedProof(proof_hash) := true`.

**Soundness statement:** any current member can deactivate (V-C1 safety valve from `sep-xxxx`). Once `active == false`, no further `update_commitment` or `deactivate_group` succeeds (SI-14). `verify_membership` against the final pre-deactivation state remains valid forever.

### 4.5 `update_vk`

Admin-only (SI-10). State delta: `VK(tier)` or `UpdateVK(tier)` replaced. **Soundness statement:** see admin-trust assumption (T-5). A new VK rotates trust to a new ceremony output; the contract has no way to verify the new VK is "honest" — that's the ceremony's job. The contract only verifies (a) IC arity matches the circuit's expected public-input count and (b) all curve points are subgroup-valid. After rotation, any subsequent `create_group` / `update_commitment` / `verify_membership` / `deactivate_group` against the affected tier verifies under the new VK. Existing groups continue to work (they re-verify against the new VK at their next call).

### 4.6 `set_restricted_mode`

Admin-only (SI-10). Toggles a single bool. No cryptographic effect. Affects only `create_group`'s admin-gating branch.

### 4.7 `bump_group_ttl`

Permissionless. Extends storage TTL on `Group(group_id)` and `History(group_id)`. No state-value mutation (only TTL metadata). Fails with `GroupNotFound` if the group doesn't exist.

### 4.8 `get_commitment` / `get_history`

Read-only. No state mutation. Cannot violate any invariant.

---

## 5. Out-of-scope (acknowledged, not closed by this contract)

- **Off-chain bitmap brute-force attack.** A chain observer who knows the public-priors initial state (per design §4.8 / §3.4) can enumerate single-leaf-delta candidates per epoch and recover the bitmap evolution. The combined `occupancy_commitment` is salt-less. Closing this requires per-epoch salting (parallel to `c_new`'s salt) — a follow-up to Phase A. **Not a contract-soundness gap; a privacy gap.** This contract correctly enforces the verifier; the verifier's privacy claim is a separate analysis.
- **Pre-inclusion mempool replay.** A relayer observing a not-yet-included transaction can submit the same proof bytes first. The circuit-level operation-tag binding (design §10) would close this; the contract-level nullifier doesn't.
- **Admin-key compromise.** A compromised admin can rotate VKs to a malicious circuit, accept fake proofs, and effectively forge group state. Mitigated by ceremony for production (design §6 Phase E); on testnet, the admin is dev-keyed and the testnet contract is explicitly experimental.
- **Soroban host-side storage tampering.** If the host runtime is compromised, all contract state is suspect. Out of any contract-level analysis.
- **Resource exhaustion.** A determined attacker submitting many failed proofs consumes contract budget on each call (subgroup checks, partial pairing computation up to the point of failure). Soroban's per-tx fee model bounds this economically.

---

## 6. Test pins

The inline test suite in `contracts/sep-democracy/src/test.rs` enforces these invariants programmatically. Each invariant has at least one test:

| Invariant | Test |
|---|---|
| SI-1 | `test_create_group_rejects_invalid_proof`, `test_create_group_happy_path` (mock proof reaches verifier and fails as expected) |
| SI-2 | `test_update_commitment_rejects_stale_c_old`, `test_update_commitment_rejects_wrong_epoch_old` |
| SI-3 | `test_update_commitment_rejects_stale_occupancy_commitment_old` |
| SI-4 | `test_update_commitment_threshold_supplied_from_storage`, `test_update_commitment_threshold_mismatch_rejected_v2` |
| SI-5 | (subgroup checks at constructor — tested implicitly via `test_initialize` using `hash_to_g{1,2}` valid mocks) |
| SI-6 | (subgroup checks at proof verify — implicit via mock_proof, which constructs valid points; an invalid-point proof would surface `false` from validate_proof_points) |
| SI-7 | `test_create_group_rejects_non_canonical_commitment`, `test_create_group_rejects_non_canonical_occupancy_commitment`, `test_update_commitment_rejects_non_canonical_c_new`, `test_update_commitment_rejects_non_canonical_occupancy_commitment_new` |
| SI-8 | `test_invalid_membership_vk_length_rejected`, `test_invalid_update_vk_length_rejected` |
| SI-9 | `test_update_commitment_rejects_replayed_proof` |
| SI-10 | `test_update_vk_requires_auth` |
| SI-11 | `test_create_group_restricted_mode_rejects_non_admin` |
| SI-13 | `test_update_commitment_rejects_wrong_epoch_old`, `test_update_commitment_happy_path` (epoch_old must match) |
| SI-14 | `test_deactivate_already_inactive_group`, `test_update_commitment_rejects_inactive_group` |
| SI-15 | (implicit — no entrypoint takes tier as wire input post-create; covered by `test_get_commitment_returns_current_state`) |
| SI-16 | `test_create_group_enforces_tier_group_limit` |
| SI-17 | (constructor idempotency — covered by `test_initialize` semantics; calling the contract pre-init returns `NotInitialized`, post-init returns `AlreadyInitialized`) |
| SI-18 | `test_archive_entry_appends_and_prunes` |
| SI-19 | (covered by SI-18; the test inserts entries and asserts FIFO order) |

`test_vectors_consistency` additionally CI-asserts that `test-vectors.json`'s pinned error codes, tier capacities, and IC-point counts agree with the contract source — catching any drift introduced by future edits.

---

## 7. Verification gaps

The following soundness arguments are **assumed**, not verified by this codebase:

1. **Real Groth16 proofs against the v2 democracy circuit.** Tests use mock proofs (subgroup-valid points that fail pairing). The full `create_group → update_commitment → verify_membership → deactivate_group` round-trip with positive verification is gated on Phase A's `prove_democracy_v2` + fixture generator (design §6 Phase A; deploy script header notes the gap). When Phase A lands, the deploy script's smoke-test block will exercise this end-to-end on testnet.
2. **Cross-platform vector agreement.** The Phase A6 cross-platform test vectors (`docs/cross-platform-test-vectors.json` extension) will pin Swift / Kotlin / Rust prover outputs against this contract's verifier. Until then, prover-vs-contract drift is a possibility.
3. **Ceremony-gated production VKs.** Testnet uses dev VKs; mainnet release gates on the ceremony per design §6 Phase E. Until the ceremony runs, the admin-trust assumption is "only deploy on testnet."

---

## 8. Change-control

Any future edit to `contracts/sep-democracy/src/lib.rs` that touches:

- The `Error` enum,
- The `CommitmentEntry`, `PublicInputs`, `UpdateCommitmentPublicInputs`, `VkKind`, or `DataKey` types,
- Any `pub fn` entrypoint signature,
- `validate_vk_points`, `validate_proof_points`, `is_canonical_fr`,
- `verify_membership_proof`, `verify_update_proof`,
- `proof_hash`, `check_proof_replay`, `record_proof`,

…must update `test-vectors.json` (CI-asserted by `test_vectors_consistency`) AND must re-evaluate the relevant soundness invariants above. The acceptance criterion is: every SI-N still holds verbatim, OR the document is amended to reflect a deliberate weakening.
