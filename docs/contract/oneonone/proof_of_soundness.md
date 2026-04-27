# Proof of Soundness — `contracts/sep-oneonone`

**Scope:** the per-type 1v1 Soroban contract at `contracts/sep-oneonone/src/lib.rs`. Mirrors `docs/contract/{democracy,oligarchy,anarchy}/proof_of_soundness.md` but scoped to the immutable-by-design 1v1 surface.

## What "soundness" means here

A contract-level adversary controlling network state, mempool ordering, and the bytes of any user-supplied input cannot:

- **SI-1:** Forge a `create_group` for a new `group_id` without producing a Groth16 proof that verifies under the stored Create VK against `(commitment, epoch=0)`.
- **SI-2:** Replay an already-burned proof at `create_group`.
- **SI-3:** Mutate a stored `CommitmentEntry`. (No mutation path exists.)
- **SI-4:** Cause `verify_membership` to return `true` for a group state different from what's stored, or for a forged proof.
- **SI-5:** Trigger an irreversible state change with a leaked / observed proof — the post-#153 attack pattern.
- **SI-6:** Bypass admin auth on `update_vk` or `set_restricted_mode`.
- **SI-7:** Install a stored VK with non-3-IC layout or non-subgroup-valid points.

## Reductions to standard assumptions

| Property | Reduces to |
|---|---|
| SI-1 | Groth16 zk-SNARK soundness over BLS12-381. The Create VK was generated for the `OneOnOneCreateCircuit`; producing a verifying proof without a satisfying witness implies breaking Groth16. The `is_canonical_fr` check on `commitment` rules out non-canonical encodings that some Groth16 implementations would accept under malleable Fr representations. |
| SI-2 | SHA-256 collision resistance. `proof_hash = sha256(proof.a \|\| proof.b \|\| proof.c)` over the 384-byte uncompressed proof; replay protection rejects byte-identical proof inputs. (Pre-inclusion mempool replay by an unrelated attacker is benign griefing — see §front-running below.) |
| SI-3 | Code structure. There is no entrypoint that writes to `DataKey::Group(id)` after `create_group`'s initial set. `__constructor`, `update_vk`, `set_restricted_mode`, `bump_group_ttl`, `verify_membership`, `get_commitment` — none of them touch `Group(id)`. The only path is `create_group` for a fresh `group_id`. |
| SI-4 | Groth16 soundness on the Membership VK. Same reduction as SI-1. The contract additionally checks `public_inputs == stored` before invoking the verifier, so a verifier success implies the public inputs the proof was bound to also match the stored state. |
| SI-5 | **By construction, not by reduction.** There is no state-mutating entrypoint that consumes a Membership-VK-keyed proof. The leaked-proof attack surface is therefore empty. See §"Post-#153 closure" below. |
| SI-6 | Soroban's `require_auth()`. Both `update_vk` and `set_restricted_mode` load the stored admin address from instance storage and call `admin.require_auth()`; without a valid signature from the admin's keypair, the host aborts before the entrypoint body runs. |
| SI-7 | `__constructor` and `update_vk` both call `validate_vk_points(&vk)` (subgroup checks) and reject `vk.ic.len() != expected_ic_count`. A malformed VK is rejected before being persisted. |

## Post-#153 closure

The mempool front-running attack documented in `docs/postmortem-deactivate-group-frontrun.md` exploited two structural features:

1. `verify_membership` is read-only and intentionally does not consume the proof's nullifier (`record_proof` is not called).
2. `deactivate_group` accepted the same `(commitment, epoch)` public-input shape as `verify_membership`, against the same Membership VK.

An attacker observing an honest member's `verify_membership` call could replay the leaked proof bytes as `deactivate_group` and irreversibly freeze the group.

In `sep-oneonone`, this attack is closed structurally:

- **Feature 1 still holds:** `verify_membership` is read-only.
- **Feature 2 is gone:** there is no `deactivate_group` entrypoint, and there is no other state-mutating entrypoint that consumes a Membership-VK proof.

The only state-mutating proof-consuming entrypoint is `create_group`, which:

- Verifies under the **Create** VK (different `alpha`, `beta`, `gamma`, `delta` from the Membership VK; a Membership-VK proof would fail the Create-VK pairing check).
- Requires a fresh `group_id` (existing `Group(id)` triggers `GroupAlreadyExists`).
- The state delta is fully fixed by the Groth16 public inputs (`commitment`, `epoch=0`); an attacker replaying a leaked Create-VK proof can only create the group the prover already wanted, under a different `caller`.

The worst-case adversarial outcome is identical to the post-#153 framing for the other three per-type contracts on `create_group`: benign griefing, prover picks a different `group_id` and resubmits.

## Front-running surface (acknowledged residual)

`create_group` is replayable by any observer of a prover's transaction:

- `caller` is **not** bound into the Create-circuit public inputs; only `(commitment, epoch=0)` is. A `caller.require_auth()` gate is enforced, but the gate authorizes the attacker as the new caller — it does not require the attacker to be the original prover.
- An attacker who observes the prover's transaction can construct a copy with their own `caller` address, submit it, and win block ordering. Result: the attacker's `create_group` succeeds (the proof verifies, the commitment is stored), and the prover's subsequent submission fails with `GroupAlreadyExists` or `ProofReplay`.

The state delta is identical regardless of who submits: same `commitment`, same `group_id` if the attacker forces the prover's exact one or a different `group_id` if the attacker picks. There is no per-creator on-chain privilege to steal — the contract does not store `caller` anywhere persistent. This is an accepted residual, parallel to all sibling per-type contracts.

A circuit-level closure (binding `caller` as a Create-circuit public input) would close the residual but requires a v2 ceremony. Out of scope for v0.

## What's NOT proved here

- **Circuit-level soundness of `OneOnOneCreateCircuit`** (the "exactly 2 non-zero leaves at founding" invariant). That property must hold at the circuit level for SI-1 to be meaningful for 1v1's actual semantics. Proved separately in the circuit's own audit / soundness analysis. The contract trusts the Create VK to encode the constraint.
- **Privacy of the witness** (zero-knowledge). Standard Groth16 ZK property; out of scope for the contract.
- **Soroban-host correctness.** The contract relies on `require_auth`, `bls12_381` host functions, and storage TTLs behaving per spec.
- **Operational soundness of the `MAX_GROUPS = 10_000` ceiling.** This is a known capacity follow-up; production deployments should plan for fresh contract instances rather than re-introducing a decrement path. See design doc §8.

## Open residuals (acknowledged, not closed)

- **Pre-inclusion mempool front-run on `create_group`.** Closed only by circuit-level `caller` binding (v2 ceremony work). Documented as accepted residual; the attack outcome is benign griefing.
- **TTL-aged proof replay.** Same shape as the residual documented in `contracts/sep-xxxx/src/lib.rs:108-128` (the `UsedProof` TTL is `LEDGER_BUMP` ~30 days). After expiry the same proof bytes can be re-used at another `create_group` for a different `group_id` — but the only effect is to consume `GroupCount` for a group whose 2-leaf tree the legitimate prover has presumably already sealed off-chain. Not exploitable for state-stealing; documented as a quirk of the global-nullifier scope.
