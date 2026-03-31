# Phase 3: Soroban Contract — Explained Simply

## What does the contract do?

The Soroban contract is the on-chain piece of the privacy system. It stores group commitments and verifies Groth16 proofs — but it never sees member identities, secret keys, or the member list.

Think of it as a locked bulletin board:
- Anyone can **post** a new group (with a proof they belong to it)
- Anyone can **update** a group (with a proof they belonged to it before the update)
- Anyone can **check** if someone's proof is valid
- But the board itself never learns **who** is in any group

---

## How does it work?

### The life of a group

```
1. CREATE          2. UPDATE           3. UPDATE           4. DEACTIVATE
   epoch 0            epoch 1             epoch 2             (frozen)
   ┌────────┐         ┌────────┐          ┌────────┐          ┌────────┐
   │commit_0│────────▶ │commit_1│────────▶  │commit_2│────────▶ │commit_2│
   │tier: S │  proof   │tier: S │  proof    │tier: S │  proof   │active:F│
   │active:T│  binds   │active:T│  binds    │active:T│  binds   │        │
   └────────┘  to 0    └────────┘  to 1     └────────┘  to 2    └────────┘
```

Every transition requires a **zero-knowledge proof** that the submitter is a member. The proof binds to the **current** state — you can't update a group unless you can prove you belong to it right now.

### What's stored on-chain

For each group, the contract stores:

| Field | Size | Description |
|-------|------|-------------|
| `commitment` | 32 bytes | Poseidon binding (opaque to the contract) |
| `epoch` | 8 bytes | Counter, starts at 0, increments by 1 |
| `timestamp` | 8 bytes | Ledger time of last update |
| `tier` | 4 bytes | Circuit size (0/1/2), immutable after creation |
| `active` | 1 byte | Whether updates are still accepted |

Plus a rolling history window of the last 64 entries, and contract events for full audit trails.

### What's NOT stored on-chain

- Member list
- Secret keys
- Salt
- Who submitted the transaction
- How many members the group has

---

## The six contract functions

### `initialize(admin, vk_small, vk_medium, vk_large)`

Called once by the deployer. Stores the Groth16 verification keys for all three tiers.

Each VK contains:
- `alpha_g1` (96 bytes) — from the trusted setup ceremony
- `beta_g2`, `gamma_g2`, `delta_g2` (192 bytes each)
- `ic` — 3 input commitment points in G1 (for the 2 public inputs: commitment and epoch)

These are the same keys produced by Phase 2. They never change.

### `create_group(group_id, commitment, tier, proof)`

Creates a new group at epoch 0.

1. Checks the group doesn't already exist
2. Verifies the Groth16 proof against `(commitment, epoch=0)`
3. Stores the initial state
4. Emits a `GroupCreated` event

The proof proves: "I know a secret key whose hash is a leaf in the Merkle tree that produced this commitment at epoch 0."

### `update_commitment(group_id, new_commitment, proof)`

Advances the group to the next epoch.

1. Loads the current state `(commitment, epoch)`
2. Verifies the proof against the **current** state (not the new one!)
3. Archives the current state to history
4. Stores the new commitment at `epoch + 1`
5. Emits a `CommitmentUpdated` event

**Why verify against the current state?** This ensures the updater is a member *before* the change. Otherwise anyone could submit a new commitment without proving membership.

### `verify_membership(group_id, proof) → bool`

Read-only. Checks if the proof is valid for the group's current state.

Doesn't modify anything — useful for other contracts or off-chain queries to verify someone is a group member without learning who they are.

### `deactivate_group(group_id, proof)`

Freezes the group permanently. Requires a membership proof (only a member can deactivate).

After deactivation:
- `verify_membership` still works ✓
- `get_state` still works ✓
- `update_commitment` is rejected ✗

### `get_state(group_id) → CommitmentEntry`

Returns the current group state.

### `get_history(group_id) → Vec<CommitmentEntry>`

Returns up to 64 most recent historical entries. Full history is available via contract events.

---

## How Groth16 verification works on Soroban

This is the core of the contract. It verifies a zero-knowledge proof using only BLS12-381 curve operations — no Poseidon, no Merkle trees, no SHA-256 needed on-chain.

### The equation

The Groth16 verification equation:

```
e(π_A, π_B) = e(α, β) · e(vk_x, γ) · e(π_C, δ)
```

Where:
- `π_A`, `π_B`, `π_C` are the proof (submitted by the prover)
- `α`, `β`, `γ`, `δ` are from the verification key (stored at init)
- `vk_x` is computed from the public inputs

### The three steps

**Step 1: Compute vk_x**

The contract combines the verification key's input commitment points with the public inputs:

```
vk_x = IC[0] + commitment · IC[1] + epoch · IC[2]
```

This uses one G1 multi-scalar multiplication (MSM) and one G1 addition. The `commitment` is the on-chain stored value; `epoch` is the stored epoch. The contract controls these — the caller can't substitute different values.

**Step 2: Negate π_A**

To use the efficient multi-pairing check (which checks if a product of pairings equals 1), we need `-π_A`. This is done by scalar multiplication:

```
-A = (r - 1) · A
```

where `r` is the BLS12-381 scalar field order. Since `r · A = O` (identity), `(r-1) · A = -A`.

**Step 3: Multi-pairing check**

The equation is rewritten as:

```
e(-π_A, π_B) · e(α, β) · e(vk_x, γ) · e(π_C, δ) = 1
```

This is a single call to the `bls12_381_multi_pairing_check` host function with 4 pairs of curve points.

### Host functions used

| Host function | Call count | Purpose |
|---------------|-----------|---------|
| `bls12_381_g1_msm` | 1 | Compute commitment·IC[1] + epoch·IC[2] |
| `bls12_381_g1_add` | 1 | Add IC[0] to MSM result |
| `bls12_381_g1_mul` | 1 | Negate π_A |
| `bls12_381_multi_pairing_check` | 1 | Final verification (4 pairings) |

Total: **4 host function calls** per verification. Estimated cost: ~8–12M Soroban instructions — well within transaction limits.

### Why this is constant-time

The verification cost is the same regardless of group size. A 2,048-member group costs exactly the same to verify as a 2-member group. The proof is always 384 bytes (uncompressed). The public inputs are always 2 field elements. The pairing check is always 4 pairs.

---

## Point serialization format

The contract uses **uncompressed** BLS12-381 points:

| Type | Size | Layout |
|------|------|--------|
| G1 | 96 bytes | x (48 bytes) \|\| y (48 bytes), big-endian |
| G2 | 192 bytes | x₀ (48) \|\| x₁ (48) \|\| y₀ (48) \|\| y₁ (48), big-endian |
| Fr scalar | 32 bytes | Big-endian (stored as U256) |

The SEP specifies 192-byte **compressed** proofs (48 + 96 + 48). Clients must decompress to the uncompressed format before submitting to the contract. This avoids decompression costs on-chain.

---

## Storage layout

```
Instance storage (contract-level):
  Admin → Address (set once at init)

Persistent storage (per-tier):
  VK(0) → VerificationKeyData (Small tier)
  VK(1) → VerificationKeyData (Medium tier)
  VK(2) → VerificationKeyData (Large tier)

Persistent storage (per-group):
  Group(group_id) → CommitmentEntry (current state)
  History(group_id) → Vec<CommitmentEntry> (rolling window, max 64)
```

Storage entries have TTLs extended automatically on writes (~30-day bumps).

---

## Events

Every state change emits a contract event. These form a complete, append-only audit trail — even after the rolling history window prunes old entries.

| Event | Topics | Data |
|-------|--------|------|
| GroupCreated | `["GroupCreated", group_id]` | `(commitment, epoch, tier, timestamp)` |
| CommitmentUpdated | `["CommitmentUpdated", group_id]` | `(new_commitment, new_epoch, timestamp)` |
| GroupDeactivated | `["GroupDeactivated", group_id]` | `(final_epoch, timestamp)` |

No member identity appears in any event.

---

## Error codes

| Code | Name | When |
|------|------|------|
| 1 | NotInitialized | Contract hasn't been initialized |
| 2 | AlreadyInitialized | `initialize` called twice |
| 3 | Unauthorized | Caller is not the admin |
| 4 | GroupAlreadyExists | `create_group` with existing group_id |
| 5 | GroupNotFound | Group doesn't exist |
| 6 | GroupInactive | Group has been deactivated |
| 7 | InvalidProof | Groth16 proof failed verification |
| 8 | InvalidTier | Tier must be 0, 1, or 2 |
| 9 | InvalidVkLength | VK must have exactly 3 IC points |

---

## Security properties

### What the contract guarantees

1. **Proof binding**: Every create/update/deactivate requires a valid Groth16 proof. No one can modify a group without proving membership.

2. **Epoch monotonicity**: Epochs increase strictly by 1. No replays (reusing old proofs), no forks (conflicting updates at the same epoch).

3. **Tier immutability**: A group's tier (and thus its verification key) is set at creation and cannot change.

4. **No identity leakage**: The contract stores only opaque commitments and epochs. The proof reveals nothing about the prover.

### What the contract does NOT guarantee

1. **Fee-payer privacy**: The Stellar account paying the transaction fee is visible. Use the relayer pattern (see relay-design-doc.md) for anonymous submissions.

2. **Group-ID opacity**: If `group_id` is derived carelessly, it might reveal the creator. Use `SHA-256(app_namespace || creator_pubkey || nonce)`.

3. **Timing privacy**: The timestamp of each update is public. Activity patterns (how often a group updates) are observable.

---

## Deployment checklist

1. **Run trusted setup ceremony** (Phase 2) for each tier
2. **Export verification keys** in uncompressed BLS12-381 format
3. **Deploy contract** to Soroban (requires Protocol 22)
4. **Call `initialize`** with admin address and all 3 VKs
5. **Deploy relayer** (optional, recommended for privacy)
6. **Distribute client SDK** with proving keys to group members

---

## Example flow: creating a group

```
Client (off-chain)                          Contract (on-chain)
─────────────────                          ──────────────────

1. Collect member keys [sk₁, sk₂, sk₃]
2. Compute leaf hashes: Poseidon(skᵢ)
3. Sort leaves lexicographically
4. Build Poseidon Merkle tree → root
5. Generate random salt
6. Compute commitment:
   Poseidon(Poseidon(root, 0), salt)
7. Generate Groth16 proof for epoch 0
8. Decompress proof to 384 bytes

9. Submit create_group(                     10. Load VK for tier
     group_id,                              11. Compute vk_x from commitment
     commitment,                            12. Negate π_A
     tier=0,                                13. Multi-pairing check
     proof                                  14. Store CommitmentEntry
   )                                        15. Emit GroupCreated event
                                            16. Return Ok(())
```

The contract never sees steps 1–6. It only sees the commitment (an opaque 32-byte blob) and the proof (curve points). It cannot determine how many members are in the group, who they are, or which member submitted the transaction.
