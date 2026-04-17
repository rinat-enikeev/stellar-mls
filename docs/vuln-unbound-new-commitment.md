# Vulnerability Report — Unbound `new_commitment` in `update_commitment`

**Date:** 2026-04-17
**Status:** Open — fix in design (see [`update-circuit-binding-design.md`](update-circuit-binding-design.md))
**Severity:** CRITICAL
**Component:** Soroban contract + Groth16 circuit
**Affected versions:** all pre-fix (keyset-v1 era)
**Reporter:** Internal protocol review
**Discoverer:** Internal protocol review
**CWE mapping:** CWE-345 (Insufficient Verification of Data Authenticity), CWE-347 (Improper Verification of Cryptographic Signature), CWE-294 (Authentication Bypass by Capture-replay — Commitment rebinding variant)
**Related:** [`postmortem-unbound-new-commitment.md`](postmortem-unbound-new-commitment.md), [`implementation-plan-update-circuit-binding.md`](implementation-plan-update-circuit-binding.md)

---

## Summary

The Stellar‑MLS protocol uses a Groth16 zero‑knowledge proof to authorise every group state transition. For the `update_commitment` operation, the on‑chain contract accepts a `new_commitment: BytesN<32>` argument and a Groth16 proof `π` with public inputs `(commitment, epoch)` referring to the **current** state. The proof is verified against the current state only. The `new_commitment` is then written verbatim to storage without ever appearing in the verified statement.

The consequence: any party that forwards the transaction to the chain — the relayer (`relayer/src/handler.rs:229-235`), any MITM on the relayer request, or any observer of the Stellar mempool who can front‑run with the same proof — can replace `new_commitment` with a value of their choosing while keeping `π`, `commitment`, and `epoch` unchanged. The on‑chain verifier still accepts the proof because it is numerically valid for its own declared public inputs; the contract then commits the attacker‑chosen `new_commitment` to storage as the group's next state. The original submitter has no way to detect this before block inclusion and no way to revert it afterwards.

The attack primitive is a **silent group hijack**: if the attacker chooses `new_commitment' = Poseidon(Poseidon(root_attacker, epoch+1), salt_attacker)` for a Merkle tree `root_attacker` they control, every subsequent membership proof must be produced against the attacker's tree. The legitimate members of the group are silently, cryptographically, and atomically evicted at the next epoch. The same primitive also gives a cheaper destructive variant — **bricking** — where the attacker picks any well‑formed‑looking `new_commitment'` whose preimage nobody knows; the group can never advance past that epoch because no member can produce a valid membership proof for a tree they do not know.

This is not an implementation mistake. The circuit, the contract, the prover SDK, and the relayer are all internally consistent and locally correct. The defect is at the protocol level: the statement being proved does not cover the operation being authorised. The Groth16 soundness proof in [`proof-of-soundness.md`](proof-of-soundness.md) (line 257–259) is a correct proof of the wrong theorem — it establishes knowledge soundness for `(C_old, e_old)` when the security property the `update_commitment` operation actually needs is soundness over `(C_old, e_old, C_new)`.

---

## Severity rationale

Using a CVSS‑style decomposition (informal — this project does not use CVSS formally):

| Dimension | Rating | Rationale |
|---|---|---|
| Attack vector | Network | Exploitable by any party in the request path or the Stellar mempool |
| Attack complexity | Low | Swap one 32‑byte field in a JSON payload or transaction envelope; no cryptographic work required |
| Privileges required | None | Attacker needs no group membership, no keys, no Stellar authorisation beyond the ability to submit a transaction |
| User interaction | None | Silent; the victim sees a successful update |
| Scope | Changed | An attack on the relay path changes security boundaries on‑chain (group ownership) |
| Confidentiality impact | Low | Commitments are opaque; attacker does not learn member identities from this bug alone |
| Integrity impact | **High** | The attacker can overwrite group state; integrity of the group's future is compromised |
| Availability impact | **High** | Bricking variant renders the group permanently unusable |
| Exploitability | **High** | No race is required if the attacker controls the relayer; a race is required if the attacker is a mempool observer, but Stellar's 5‑second ledger close gives comfortable windows |

Severity: **CRITICAL**. This is the highest severity the project uses. The bug bypasses the central authorisation primitive of the protocol (the Groth16 proof) for the single write operation that advances group membership.

---

## Affected components

| Layer | File | Lines | Function / Symbol | Role in the defect |
|---|---|---|---|---|
| Circuit | `src/circuit/mod.rs` | 1–9, 48–74, 117–248 | `MembershipCircuit` | Defines the statement the proof commits to. `new_commitment` is **absent** from public inputs and witnesses. |
| Circuit doc | `src/circuit/mod.rs` | 1–4 | Header comment | Explicitly documents "Public inputs: commitment, epoch" — confirming the gap by design. |
| Prover | `src/prover/mod.rs` | 26–43, 46–52, 71–116 | `ProverInput`, `PublicInputs`, `prove` | Prover API exposes only `(commitment, epoch)` as public inputs; no knowledge of the next state is ever pushed into the proof. |
| FFI | `src/ffi.rs` | 424 | `sep_prove` | Returns `(proof, PublicInputs)` with the 2‑field `PublicInputs`. |
| JNI | `src/jni_ffi.rs` | 360 | `sepProve` | Mirror of the FFI — same 2‑field surface. |
| SDK (Swift) | `swift-mls/Sources/SwiftMLS/Types.swift` | 31–39, 110–123 | `PublicInputs`, `SEPUpdateCommitmentRequest` | `newCommitment` is a top‑level request field **outside** `publicInputs`; clients serialise and transmit it unbound from the proof. |
| SDK (Kotlin) | `kotlin-mls/` | mirror | mirror | Mirror of Swift SDK. Same shape. |
| Relayer | `relayer/src/handler.rs` | 229–235 | `"update_commitment"` branch | Forwards `newCommitment` as an unsigned JSON field, then passes it to the Stellar CLI as `--new-commitment`. |
| Relayer | `relayer/src/handler.rs` | 346–375 | `add_public_inputs_arg` | Serialises only `(commitment, epoch)` — confirms that `new_commitment` is outside the "public inputs" universe on the wire. |
| Contract | `contracts/sep-xxxx/src/lib.rs` | 143–150 | `PublicInputs` struct | 2‑field struct; no slot for `new_commitment`. |
| Contract | `contracts/sep-xxxx/src/lib.rs` | 459–523 | `update_commitment` | Accepts `new_commitment` as a separate function argument; verifies proof against `public_inputs` only; writes `new_commitment` to storage unverified. |
| Contract | `contracts/sep-xxxx/src/lib.rs` | 780–823 | `verify_groth16_proof` | Builds the MSM over `ic1, ic2` with scalars `commitment, epoch` only (see `:812-814`); has no mechanism to bind `new_commitment`. |
| Contract | `contracts/sep-xxxx/src/lib.rs` | 231–267 | `initialize` | Requires `ic.len() == 3` — pinning the circuit to **exactly** 2 public inputs forever without redeploy/rotate. |
| Contract | `contracts/sep-xxxx/src/lib.rs` | 455–458 | N‑14 inline comment | Acknowledges a related but different gap (caller binding); does not mention commitment binding. Misleads reviewers into thinking binding concerns have been catalogued. |
| Soundness proof | `docs/proof-of-soundness.md` | 257–259 | Theorem 3 | Proves knowledge soundness for statement `(C, e)` — the wrong statement for `update_commitment`. |
| Threat model doc | `docs/sep.md` | 532–549 | Relayer trust model | Conflates updater **privacy** ("which member") with update **integrity** ("what is written"). |

---

## Root cause

### 1. The statement is a proper subset of the operation's authorisation requirements

A ZK‑gated state transition of the form `(S, op) → S'` requires the proof to commit to every input the authorisation depends on. For `update_commitment`, that set is `(C_old, e_old, C_new)`. The circuit commits only to `(C_old, e_old)`. The remaining authorisation input `C_new` is therefore **unauthenticated** — it flows through the protocol as attacker‑mutable data.

Concretely, the circuit's header comment (`src/circuit/mod.rs:1-4`) reads:

```
Public inputs: commitment, epoch
Witness: secret_key, poseidon_root, salt, merkle_path, leaf_index

Constraints:
1. leaf = Poseidon(secret_key) — key ownership via preimage knowledge
2. MerklePoseidonOpen(leaf, path, index, root) == poseidon_root
3. Poseidon(Poseidon(poseidon_root, epoch), salt) == commitment
```

This specification, and the arkworks `ConstraintSynthesizer` implementation at `src/circuit/mod.rs:117-248` that realises it, is self‑consistent: the proof demonstrates that the prover knows a secret key whose Poseidon leaf sits in a tree whose root, combined with the epoch and salt, yields the bound commitment. The statement is **exactly** "membership at `(C, e)`." It is not "a state transition from `(C, e)` to `(C', e+1)`."

### 2. The contract function signature treats `new_commitment` as ordinary data

`contracts/sep-xxxx/src/lib.rs:459-466`:

```rust
pub fn update_commitment(
    env: Env,
    group_id: BytesN<32>,
    new_commitment: BytesN<32>,
    new_epoch: u64,
    proof: Groth16Proof,
    public_inputs: PublicInputs,
) -> Result<(), Error> {
```

`new_commitment` is the third argument. It is a 32‑byte opaque blob from the contract's perspective. The only downstream use of `new_commitment` is to write it into the next `CommitmentEntry`:

```rust
let new_entry = CommitmentEntry {
    commitment: new_commitment.clone(),
    epoch: new_epoch,
    timestamp,
    tier: current.tier,
    active: true,
};
env.storage().persistent().set(&DataKey::Group(group_id.clone()), &new_entry);
```

No code path ever compares `new_commitment` against anything in the proof or the `public_inputs` struct. Nor could it — the proof exposes no claim about `new_commitment`.

### 3. The verifier call is parameterised only by the current state

`contracts/sep-xxxx/src/lib.rs:489-494`:

```rust
let vk = Self::load_vk(&env, current.tier)?;
if !verify_groth16_proof(&env, &vk, &proof, &current.commitment, current.epoch) {
    return Err(Error::InvalidProof);
}
```

And inside `verify_groth16_proof` at `:812-815`:

```rust
let msm_points: Vec<G1Affine> = vec![env, ic1, ic2];
let msm_scalars: Vec<Fr> = vec![env, commitment_fr, epoch_fr];
let msm_result = bls.g1_msm(msm_points, msm_scalars);
let vk_x = bls.g1_add(&ic0, &msm_result);
```

The verifier computes `vk_x = IC[0] + commitment · IC[1] + epoch · IC[2]`. There is no third IC point, no third scalar, no third term. By Groth16's structure, this `vk_x` point encodes the statement the proof is being checked against. Swapping `new_commitment` in the outer function does not perturb `vk_x` in any way, so the pairing check outcome is entirely independent of `new_commitment`.

### 4. The surrounding mitigations are orthogonal to commitment binding

The contract has three mitigations that *could* plausibly be mistaken for binding protection:

1. **Epoch monotonicity.** `:479-481` enforces `new_epoch == current.epoch + 1`. This prevents forks and replays *across* epochs, not rebinding *within* an epoch.
2. **Public‑inputs ↔ current‑state consistency.** `:483-486` enforces `public_inputs.commitment == current.commitment && public_inputs.epoch == current.epoch`. This prevents the attacker from re‑submitting a proof with outdated public inputs. It does not bind `new_commitment` — `new_commitment` is not even in the struct.
3. **Proof‑hash replay.** `:489, :726-749` prevents exact‑bytes resubmission of the same proof. This does not bind `new_commitment` either: the attacker does not need to re‑use a proof later — they need to *submit it now with different `new_commitment`*.

Each mitigation is correct for its stated purpose. None addresses the gap.

### 5. The one gap that was noted in the source code was a different gap

`contracts/sep-xxxx/src/lib.rs:453-458`:

```
N-14: Design note: This function uses proof-based authorization only
(no caller.require_auth()). Any Stellar account can call it — only a
valid Groth16 membership proof is required. This is intentional: the
protocol is proof-based, not identity-based. The proof replay mechanism
(C-2) prevents exact re-submission. For environments where address
binding is desired, an optional caller: Address parameter can be added.
```

The comment explicitly catalogues one kind of binding as a known limitation: *address/caller binding*. It does not mention commitment binding. A reviewer reading this comment would reasonably infer that the designers had considered what the proof binds and documented the one thing it does not bind. The unlisted gap on commitment binding therefore hides in plain sight behind an adjacent, named gap.

---

## Reproduction

The steps below describe the attack against a well‑formed relayer request. An end‑to‑end testnet reproduction is part of the verification plan in [`implementation-plan-update-circuit-binding.md`](implementation-plan-update-circuit-binding.md) (Phase 11).

### Prerequisites

- A Soroban contract deployed with `SepXxxxContract` from `contracts/sep-xxxx/src/lib.rs` and `initialize`d with a keyset‑v1 VK.
- A group `G` in tier `T` at state `(C_0, e=0)`. The victim `V` is a member.
- The attacker `A` is a party in the message path. They may be:
  - the relayer operator, or
  - an active MITM on the TCP connection `V → relayer`, or
  - a Stellar mempool observer who sees `V`'s transaction before ledger close.
- `A` has no membership in `G` and no BLS keys tied to `G`.

### Step 1 — Victim prepares a legitimate update

`V` constructs the new roster (say, admitting a new member). `V` then:

1. Builds the new Poseidon Merkle tree and computes `root_V`.
2. Samples a fresh `salt_V`.
3. Computes `C_new_V = Poseidon(Poseidon(root_V, 1), salt_V)` using `compute_poseidon_commitment` from `src/commitment/mod.rs`.
4. Generates a Groth16 proof `π` via `prove(&pk, &ProverInput { members: old_members, secret_key: V_sk, epoch: 0, salt: salt_V_old, depth })` — i.e. proves *membership at the current state* `(C_0, e=0)`. The prover API (`src/prover/mod.rs:71-116`) has no parameter that would encode `C_new_V` or `root_V` or the new `salt_V`.
5. Assembles a relayer payload:

```json
{
  "groupID":         "<hex of G>",
  "newCommitment":   "<base64 of C_new_V>",
  "newEpoch":        1,
  "proof":           "<base64 of π>",
  "publicInputs":    { "commitment": "<b64 C_0>", "epoch": 0 }
}
```

6. POSTs the payload to the relayer.

### Step 2 — Attacker rebinds

`A` intercepts or crafts the same payload. They substitute `"newCommitment"` with `C_new_A`, chosen to be either:

- **Hijack variant:** `C_new_A = Poseidon(Poseidon(root_A, 1), salt_A)`, where `root_A` is the Merkle root of a tree whose leaves are `{Poseidon(sk_A), Poseidon(sk_A'), …}` — secret keys only `A` knows.
- **Bricking variant:** `C_new_A = H(anything)` — any 32‑byte value whose preimage is unknown (e.g. a SHA‑256 of random data, coerced into the Poseidon image by truncation — the contract does not check preimage knowledge).

The modified payload keeps `proof`, `publicInputs`, and `newEpoch` untouched:

```json
{
  "groupID":         "<hex of G>",
  "newCommitment":   "<base64 of C_new_A>",     // attacker-chosen
  "newEpoch":        1,
  "proof":           "<base64 of π>",           // unchanged
  "publicInputs":    { "commitment": "<b64 C_0>", "epoch": 0 }
}
```

### Step 3 — On‑chain verification and commit

The relayer forwards the payload to Stellar. The contract enters `update_commitment` at `contracts/sep-xxxx/src/lib.rs:459`. Each check:

- `:467 require_initialized` — pass.
- `:469-473 storage().get(&DataKey::Group(group_id))` — returns current entry `(C_0, 0)`.
- `:475-477 active` — pass.
- `:479-481 new_epoch == current.epoch + 1` — `1 == 0 + 1` — pass.
- `:483-486 public_inputs.commitment == current.commitment && public_inputs.epoch == current.epoch` — `C_0 == C_0 && 0 == 0` — pass. (Attacker did not touch `public_inputs`.)
- `:489 check_proof_replay` — `π` is fresh — pass.
- `:491-494 verify_groth16_proof(&vk, &π, &C_0, 0)` — computes `vk_x = IC[0] + C_0 · IC[1] + 0 · IC[2]`; pairing check holds because `π` was generated honestly against `(C_0, 0)`. **Pass.**
- `:496 record_proof` — records `SHA256(π_a || π_b || π_c)`.
- `:500-508 archive_entry + new_entry.commitment = new_commitment.clone()` — **writes `C_new_A` to storage**.
- `:514-520 publish CommitmentUpdated` — event carries `commitment: C_new_A`.

The contract returns `Ok(())`. The group is now at `(C_new_A, 1)`.

### Step 4 — Group is captured (hijack variant)

At epoch 1, every `verify_membership`, `update_commitment`, and `deactivate_group` call must produce a Groth16 proof against `current.commitment = C_new_A`. The only witnesses that can produce such a proof are `root_A` and `salt_A` — known only to `A`. `V` and every other legitimate member of the original tree have no valid opening into `root_A`. Their membership in `G` is silently revoked and cannot be restored.

The victim's client will observe the on‑chain `CommitmentUpdated` event and, if it naïvely trusts the event, re‑derive the group traffic key under the new salt. If the client's key‑derivation flow consumes the new salt from an off‑chain channel (e.g. Nostr) separately, the victim will be unable to decrypt subsequent messages. Either way, the group is gone.

### Step 5 — Group is destroyed (bricking variant)

The same state write happens. At epoch 1, no valid `update_commitment` or `verify_membership` proof exists because nobody knows the preimage of `C_new_A`. The group is stuck — it cannot advance, it cannot be verified, and the contract offers no recovery path (`deactivate_group` also requires a valid membership proof — see `:593-594`).

### Attacker cost

- Compute: negligible. One Poseidon‑double‑hash for the hijack variant, nothing for the bricking variant.
- On‑chain: one `update_commitment` transaction fee.
- Trust: none. The attacker does not need to be trusted by anyone.
- Detectability: before block inclusion, **zero**. After inclusion, the fact that a state change happened is visible, but the attack is indistinguishable from a legitimate update by a member of the group until the victim tries to produce the next proof and fails.

---

## Impact scenarios

### Scenario A — Silent hijack by a dishonest relayer operator

The relayer operator deploys the stock `relayer` binary. A user's client submits an honest `update_commitment` request. The operator's modified relayer intercepts every `update_commitment` payload, substitutes `newCommitment` with a commitment of the operator's own tree, forwards to the chain, and returns a success response to the user. The user observes the `CommitmentUpdated` event, sees that `newCommitment` on chain does not match what they sent, but by then the group has advanced and there is no rollback. In the first epoch after the attack, no legitimate member can produce a proof; the operator runs the group.

This is undetectable by ordinary relayer‑auditing procedures because the malicious behaviour is data‑substitution, not code‑signature change — a public git fork of the relayer could implement it. Nor is it prevented by relayer sandboxing or request signing at the transport layer, because the JSON payload is not pre‑signed by the user.

### Scenario B — Mempool front‑runner

The relayer is honest. `A` runs a watcher on the Stellar mempool that looks for `update_commitment` transactions from any relayer. `A` decodes the inner `new_commitment`, `proof`, `public_inputs` arguments. `A` crafts a competing transaction:

- `group_id`: copied
- `new_commitment`: substituted with `C_new_A`
- `new_epoch`: copied
- `proof`: copied
- `public_inputs`: copied

`A` submits their transaction with a higher fee. If Stellar orders transactions by fee (Soroban currently does not do per‑account front‑running protection; the first one to land by fee / validator preference wins), `A`'s transaction lands first. The victim's honest transaction then fails — not because of `A`'s mutation but because `public_inputs.epoch == current.epoch` no longer holds (current epoch is now `e+1`).

### Scenario C — MITM on a plaintext relayer connection

If the relayer is reached over plaintext HTTP (or over TLS with a compromised endpoint), any on‑path attacker can modify the JSON body in transit. This degenerates to Scenario A from the user's perspective.

### Scenario D — Bricking by any party in the request path

All three of the above scenarios can be executed in "bricking" mode with no planning and no infrastructure: the attacker picks any `newCommitment` they like, not necessarily one whose preimage they know, and the target group becomes permanently unusable at the next epoch. This is a cheap denial‑of‑service primitive against any group that performs an update through a compromised or hostile path.

### Scenario E — Chained hijack followed by ransomware

An attacker who executes Scenario A can advertise recovery in exchange for payment: since `A` is the only party who can produce valid proofs for the hijacked group, `A` can demand ransom in exchange for signing an update that returns control. The victim has no recourse at the protocol level. Out of scope for this report, but worth naming as a downstream impact class.

---

## Scope of attacker capabilities

The attack's minimum required capabilities:

| Capability | Who has it |
|---|---|
| Observe one `update_commitment` payload in flight (relayer request body, Stellar transaction envelope, or mempool entry) | Relayer operator, MITM on plaintext TCP, any Stellar node operator with mempool visibility, any party who can submit their own transaction in the same ledger window |
| Submit a Stellar transaction | Anyone with a funded account (testnet or mainnet) |
| Compute one Poseidon hash and one Merkle tree root (hijack variant only) | Anyone with the `stellar-mls` Rust core or any Poseidon‑BLS12‑381 implementation |

The attack does **not** require:

- Membership in the target group.
- Possession of any BLS private key.
- Execution of any circuit.
- Stellar account authorisation tied to the group (`update_commitment` is intentionally caller‑agnostic — see N‑14).

---

## Why existing mitigations do not close the gap

### Epoch monotonicity (`:479-481`)

Purpose: prevents an attacker from re‑committing old state or from writing a non‑sequential epoch. Effect on this attack: **none**. The attacker writes `epoch=1` when the current epoch is `0`. Monotonicity is respected.

### Public‑inputs ↔ current‑state consistency (`:483-486`)

Purpose: prevents using a proof from a previous epoch against the current state. Effect on this attack: **none**. The attacker does not modify `public_inputs`; they only modify `new_commitment`.

### Proof‑hash replay protection (`:489, :726-749`)

Purpose: prevents exact resubmission of a previously‑accepted proof. Effect on this attack: **none, and possibly negative**. The attacker submits the victim's proof **first**, with a mutated `new_commitment`. The victim's subsequent submission then fails the proof‑hash replay check, not because it was replayed in any attack sense but because it is now byte‑identical to the attacker's prior use. The victim sees the replay‑rejection error and has no easy way to learn that a rebinding occurred.

A related subtlety: Groth16 proofs are re‑randomisable. `(a, b, c)` can be transformed to `(a', b', c')` that verifies the same statement. So `A` could re‑randomise the victim's proof before submitting to avoid the proof‑hash collision — then the victim's own submission would not be rejected as a replay but as a state mismatch (the epoch has already advanced past `public_inputs.epoch`). Either way, `A` wins.

### Canonical‑bytes check on public inputs (`:802-808`)

Purpose: prevents a proof from being accepted against a non‑canonical field‑element encoding of `commitment`. Effect on this attack: **none**. The attacker does not violate canonicity. Observation: **the same check is not applied to `new_commitment` before storage**. This is a secondary bug — a non‑canonical `new_commitment` can be stored, and the next epoch's `public_inputs.commitment == current.commitment` check will then fail against any canonical re‑encoding, bricking the group. This is tracked as a secondary fix in the design doc.

### Group activity flag (`:475-477`)

Purpose: rejects updates to deactivated groups. Effect on this attack: **none**. The attacker targets an active group.

### Per‑tier `MAX_GROUPS_PER_TIER` (`:395-403`)

Purpose: prevents storage exhaustion via `create_group` spam. Effect on this attack: **none** — does not apply to `update_commitment`.

### Relayer‑side auth / rate limits

Out of scope for a cryptographic guarantee. Even a perfectly rate‑limited relayer is ineffectual against Scenario B (mempool front‑running) and against Scenario A (the relayer itself is the attacker).

---

## Related finding — the soundness proof covers the wrong statement

[`proof-of-soundness.md`](proof-of-soundness.md) at lines 257–259 reads:

> By the Groth16 knowledge soundness property (Assumption 3.3, Groth 2016, Theorem 1), for any PPT prover P\* that produces an accepting proof π for statement `(C, e)`, there exists a PPT extractor E that outputs a witness w = (sk, root, s, path, idx) such that: R((C, e), w) = 1 with overwhelming probability.

This is a correct application of Groth16 soundness. However, it establishes soundness only for the statement `(C, e)`. The `update_commitment` operation's security relies on a property over the statement `(C_old, e_old, C_new)`: "no PPT adversary can produce a proof that accepts at the verifier for the same `(C_old, e_old)` paired with two different `C_new` values." No theorem of that form exists in the current soundness document. The gap is not that the Groth16 instance is misused — it is that the theorem stops one step short of the operation's security property.

This is a general lesson for ZK‑gated systems: **soundness theorems must be stated over the operation, not over the circuit**. See the postmortem for the implication for future audits.

---

## Fix outline

The fix is described in full in [`update-circuit-binding-design.md`](update-circuit-binding-design.md). Summary:

1. Introduce a separate **`UpdateCircuit`** (leaving `MembershipCircuit` unchanged for `create_group` / `verify_membership` / `deactivate_group`).
2. Public inputs: `(commitment, epoch, new_commitment)`. Witnesses: existing set plus `new_poseidon_root`, `new_salt`.
3. New constraint: `Poseidon(Poseidon(new_poseidon_root, epoch+1), new_salt) == new_commitment`. The equality constraint is what makes `new_commitment` load‑bearing — an unconstrained public input would have degenerate IC contribution and would not bind.
4. Contract: new `UpdatePublicInputs` type; `update_commitment` drops the standalone `new_commitment` and `new_epoch` parameters (both derivable from state + public inputs); `verify_groth16_proof_update` uses 4‑point IC + 3‑scalar MSM; canonical‑bytes check on `public_inputs.new_commitment` before storage.
5. Relayer: drop the top‑level `newCommitment` request field; serialise it inside `publicInputs`.
6. SDKs: mirror the new type; version byte at the head of serialisation to fail loudly on stale clients.
7. Trusted setup: run Phase 2 MPC for both circuits; publish keyset‑v2.

The fix is non‑negotiable for mainnet. The secondary fix (canonical check on `new_commitment`) is also required regardless, as a silent‑reduction bricking variant exists even under the corrected binding.

Out of scope for this fix (documented decisions):

- **Nullifier.** Skipped. Under epoch monotonicity + proof‑hash replay, a nullifier imposes "≤ 1 update per member per epoch" governance but adds no additional soundness in this protocol.
- **Caller / tx binding.** Skipped. The protocol is intentionally identity‑free on chain; copying `(proof, public_inputs)` verbatim yields the exact state transition the victim wanted, so that is not a soundness issue.
- **"Updater remains a member of the new tree."** Skipped. The current scheme already allows any member to author an update that removes everyone else (self‑take‑over by a legitimate member is a pre‑existing governance property), so tying the proof to new‑tree membership does not close an additional vulnerability.

---

## Disclosure timeline

| When | What |
|------|------|
| 2026-04-17 | Internal review raises a suspicion that "the math contains no argument that the proof is non-transferable" and that the relayer might be able to substitute the commitment. |
| 2026-04-17 | Code inspection confirms the gap across circuit, prover, relayer, and contract. No mainnet traffic is affected — the project is in alpha. |
| 2026-04-17 | Vulnerability report drafted (this document). Design doc for the fix drafted. Implementation plan drafted. Postmortem drafted. |
| 2026-04-17 → TBD | Implementation per [`implementation-plan-update-circuit-binding.md`](implementation-plan-update-circuit-binding.md). |
| TBD | Testnet soak; attack‑simulation test confirms rejection; mainnet cut. |

Public disclosure: deferred until the fix lands on mainnet. Internal status tracked in this file.

---

## References

### Source

- `src/circuit/mod.rs:1-9` — circuit statement header comment.
- `src/circuit/mod.rs:48-74` — `MembershipCircuit` struct, public inputs and witnesses.
- `src/circuit/mod.rs:117-248` — constraint generation.
- `src/circuit/mod.rs:271-285` — reusable `poseidon_hash_two_gadget`.
- `src/circuit/mod.rs:598-612` — `test_public_input_count`.
- `src/prover/mod.rs:26-43` — `ProverInput`.
- `src/prover/mod.rs:46-52` — `PublicInputs`.
- `src/prover/mod.rs:71-116` — `prove`.
- `src/ffi.rs:424` — FFI boundary.
- `src/jni_ffi.rs:360` — JNI boundary.
- `swift-mls/Sources/SwiftMLS/Types.swift:31-39` — Swift `PublicInputs` mirror.
- `swift-mls/Sources/SwiftMLS/Types.swift:110-123` — `SEPUpdateCommitmentRequest` with top-level `newCommitment`.
- `relayer/src/handler.rs:229-235` — `update_commitment` branch.
- `relayer/src/handler.rs:346-375` — `add_public_inputs_arg`.
- `contracts/sep-xxxx/src/lib.rs:143-150` — contract `PublicInputs`.
- `contracts/sep-xxxx/src/lib.rs:231-267` — `initialize` with 3-IC-point VK check.
- `contracts/sep-xxxx/src/lib.rs:395-403` — tier group cap (unrelated mitigation).
- `contracts/sep-xxxx/src/lib.rs:407-408` — `create_group` verifier call (unaffected by this fix).
- `contracts/sep-xxxx/src/lib.rs:453-458` — N-14 comment (caller binding, not commitment binding).
- `contracts/sep-xxxx/src/lib.rs:459-523` — `update_commitment` — primary defect site.
- `contracts/sep-xxxx/src/lib.rs:479-481` — epoch monotonicity.
- `contracts/sep-xxxx/src/lib.rs:483-486` — public-inputs / current-state consistency.
- `contracts/sep-xxxx/src/lib.rs:489, :726-749` — proof-hash replay.
- `contracts/sep-xxxx/src/lib.rs:491-494` — verifier call.
- `contracts/sep-xxxx/src/lib.rs:550` — `verify_membership` verifier call (read-only — unaffected by this fix).
- `contracts/sep-xxxx/src/lib.rs:593-594` — `deactivate_group` verifier call.
- `contracts/sep-xxxx/src/lib.rs:780-823` — `verify_groth16_proof`.
- `contracts/sep-xxxx/src/lib.rs:802-808` — canonical-bytes check (applied to `commitment` only).
- `contracts/sep-xxxx/src/lib.rs:812-815` — MSM over 2 public inputs.

### Docs

- [`proof-of-soundness.md`](proof-of-soundness.md) — Theorem 3 at lines 257–259 (wrong statement).
- [`sep.md`](sep.md) — §"Relayer trust model" at lines 532–549 (conflates privacy and integrity).
- [`design-doc.md`](design-doc.md) — architecture overview.
- [`audit-critical.md`](audit-critical.md) — prior critical findings (C‑1 … C‑9); this report extends the taxonomy with a new critical.
- [`audit-report.md`](audit-report.md) — prior full audit.
- [`audit-report-v2.md`](audit-report-v2.md), [`audit-report-v3.md`](audit-report-v3.md), [`audit-4.md`](audit-4.md) — successive audit iterations; none caught this finding.
- [`real-world-gap-analysis.md`](real-world-gap-analysis.md) — to be updated with this finding.

### Companions in this report set

- [`update-circuit-binding-design.md`](update-circuit-binding-design.md) — design of the fix.
- [`implementation-plan-update-circuit-binding.md`](implementation-plan-update-circuit-binding.md) — phased implementation.
- [`postmortem-unbound-new-commitment.md`](postmortem-unbound-new-commitment.md) — root‑cause analysis and preventative lessons.

### External

- Jens Groth. *On the Size of Pairing-based Non-interactive Arguments*. EUROCRYPT 2016. §3 (the Groth16 construction), §4 (knowledge soundness).
- RFC 9420 (MLS). Referenced here only for the naming convention "Stellar‑MLS"; the MLS protocol itself does not specify on‑chain commitments and this defect is not an MLS defect.
- arkworks Groth16 documentation (`ark-groth16` crate) — IC vector construction; public-input allocation order semantics.
