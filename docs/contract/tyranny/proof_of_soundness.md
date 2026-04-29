# Proof of Soundness — `contracts/sep-tyranny`

**Scope:** the per-type Tyranny Soroban contract at `contracts/sep-tyranny/src/lib.rs`. Mirrors `docs/contract/{democracy,oligarchy,anarchy,oneonone}/proof_of_soundness.md`.

## What "soundness" means here

A contract-level adversary cannot:

- **SI-1:** Forge a `create_group` for a new `group_id` without producing a Groth16 proof verifying under the stored Create VK against `(commitment, epoch=0, admin_pubkey_commitment)`.
- **SI-2:** Forge an `update_commitment` without producing a Groth16 proof verifying under the stored Update VK against `(c_old, epoch_old, c_new, admin_pubkey_commitment)`. In particular: an adversary without the admin's BLS secret key cannot produce a verifying Update proof, because the Update circuit binds the admin's secret-key-derived pubkey hash to the contract-supplied `admin_pubkey_commitment`.
- **SI-3:** Replay an already-burned proof.
- **SI-4:** Mutate `admin_pubkey_commitment` after creation.
- **SI-5:** Cause `verify_membership` to return `true` for state different from what's stored.
- **SI-6:** Trigger an irreversible state change with a leaked / observed proof — the post-#153 attack pattern.
- **SI-7:** Bypass admin auth on `update_vk` / `set_restricted_mode`.
- **SI-8:** Install a stored VK with wrong IC layout or non-subgroup-valid points.

## Reductions

| Property | Reduces to |
|---|---|
| SI-1 | Groth16 zk-SNARK soundness over BLS12-381 + circuit-level soundness of `TyrannyCreateCircuit`. |
| SI-2 | Groth16 soundness + circuit-level soundness of `TyrannyUpdateCircuit`. The latter is the load-bearing security claim for Tyranny: producing a verifying Update proof requires knowledge of the admin's BLS secret key (the circuit binds `Poseidon(pubkey) == admin_pubkey_commitment` and proves discrete-log knowledge of `pubkey`). |
| SI-3 | SHA-256 collision resistance. `proof_hash = sha256(proof.a \|\| proof.b \|\| proof.c)`; `record_proof` post-verify, `check_proof_replay` pre-verify. |
| SI-4 | Code structure. `admin_pubkey_commitment` is stored in `DataKey::AdminCommitment(group_id)`, written exclusively by `create_group`. No other entrypoint touches that slot — `update_commitment` reads it (via `load_admin_commitment`) but never writes; `bump_group_ttl` extends its TTL but doesn't change the value; `update_vk`, `set_restricted_mode`, and the read-only entrypoints don't touch it. The wire payload `UpdatePublicInputs` does not carry `admin_pubkey_commitment` — there is no path for caller-supplied data to reach the storage slot. No `update_admin` entrypoint exists in v0. |
| SI-5 | Groth16 soundness on Membership VK. The contract checks `public_inputs == stored` before invoking the verifier. |
| SI-6 | **By construction.** No `deactivate_group` entrypoint; `verify_membership` is read-only with no mutating sibling that consumes Membership-VK proofs. The leaked-proof attack surface is empty. See `docs/postmortem-deactivate-group-frontrun.md`. |
| SI-7 | Soroban's `require_auth()`. |
| SI-8 | `__constructor` and `update_vk` call `validate_vk_points(&vk)` and check `vk.ic.len()`. |

## Front-running surface (acknowledged residual)

Same shape as the other per-type contracts:

- **`create_group` replay.** A leaked Create proof can be resubmitted by an attacker under a different `caller`, with the same `commitment` + `admin_pubkey_commitment`, against a different `group_id`. Benign griefing: the attacker can only create the group the prover already wanted; the legitimate creator picks a different `group_id` and resubmits. There is no per-creator on-chain privilege to be stolen.
- **`update_commitment` replay.** A leaked Update proof can be resubmitted by an attacker, but the state delta is fully fixed by the proof's public inputs (`c_old`, `epoch_old`, `c_new`) AND the contract-supplied `admin_pubkey_commitment`. The attacker can only push the state to where the prover was already going, against the specific group whose `AdminCommitment(group_id)` matches what the proof was bound to. Equivalent to a benign acceleration of the prover's intended update; gas cost shifts.

Closure of these residuals at the circuit level requires a v2 ceremony (binding `caller` as a public input). Out of scope for v0.

## Tyranny-specific residual: admin-key loss

If the admin's BLS secret key is lost, no Update proof can ever verify against the stored `admin_pubkey_commitment`. The group is unupdatable for the rest of its TTL lifetime; the entry ages out via storage TTL when no `bump_group_ttl` calls keep it alive.

This is structurally similar to Oligarchy's `salt_occ`-loss residual but applied to a smaller set of keys (one, not many). Documented as an accepted operational limitation in §3 of the design doc. Mitigations: backup the admin key, OR plan for a v1 `update_admin` entrypoint that allows the contract admin to rotate `admin_pubkey_commitment` (with an appropriate auth gate — likely the contract admin via `require_auth`, or a quorum of group-side participants via a dedicated rotation circuit).

## What's NOT proved here

- **Circuit-level soundness of TyrannyCreateCircuit / TyrannyUpdateCircuit.** Phase A circuit work; this contract trusts the VKs to encode the constraints.
- **Privacy of admin identity.** The Update circuit hides the admin's BLS pubkey from chain observers (only the Poseidon hash `admin_pubkey_commitment` is on-chain), but the contract-level proof here doesn't formalize the privacy claim. Standard Groth16 ZK property.
- **Soroban-host correctness.**

## Open residuals (acknowledged)

- **Pre-inclusion mempool front-run** on `create_group` and `update_commitment`. Benign griefing for both (state-delta-fixed by proof's public inputs); closure requires `caller` binding in the circuit (v2 ceremony).
- **TTL-aged proof replay.** Same shape as legacy `sep-xxxx`'s acknowledged residual. After `LEDGER_BUMP` (~30 days), the same proof bytes can be re-used. For `create_group`, harmless (different group_id required). For `update_commitment`, the proof is bound to a specific `(c_old, epoch_old, c_new)`, and `wire.epoch_old != current.epoch` will fail post-success — the attacker can replay only against a group whose epoch is still pinned at `epoch_old`, AND only push `current` to `c_new` — which is precisely what the prover wanted. No exploitable surface.
- **Admin-key loss** (Tyranny-specific). See above.
