# Phase 2: Trusted Setup Ceremony — Explained Simply

## What problem are we solving?

In Phase 1 we built a circuit that proves group membership without revealing who you are. But Groth16 (the proof system) has a catch: before anyone can create or verify proofs, someone must run a **trusted setup** that produces two keys:

- **Proving key** — used by members to create proofs
- **Verification key** — used by the smart contract to check proofs

The setup generates secret randomness called **toxic waste**. If any single person knows the full toxic waste, they can forge fake proofs — pretending to be a group member when they are not.

The solution: run the setup as a **multi-party ceremony** where many independent people each add their own randomness. The toxic waste is only dangerous if **every single participant** conspires. If even one participant honestly destroys their contribution, the system is secure.

---

## The building blocks

### 1. Structured Reference String (SRS)

The SRS is a collection of elliptic curve points that encode the toxic waste without revealing it. Think of it as a locked box: the randomness is sealed inside curve points, and nobody can extract the raw numbers.

The SRS contains five series of points:

| What | Notation | Purpose |
|------|----------|---------|
| Powers of tau in G1 | τ^0·G1, τ^1·G1, τ^2·G1, ... | Main setup material |
| Powers of tau in G2 | τ^0·G2, τ^1·G2, ... | Cross-checking G1 series |
| Alpha-shifted powers | α·τ^0·G1, α·τ^1·G1, ... | Encodes the α parameter |
| Beta-shifted powers | β·τ^0·G1, β·τ^1·G1, ... | Encodes the β parameter |
| Beta in G2 | β·G2 | Cross-checking β |

Here τ (tau), α (alpha), and β (beta) are the secret scalars — the toxic waste. G1 and G2 are generator points on the BLS12-381 curve.

### 2. Elliptic curve pairings

A pairing is a special function `e(P, Q)` that takes a G1 point and a G2 point and produces a value in a target group GT. The key property:

```
e(a·G1, b·G2) = e(G1, G2)^(a·b)
```

This means we can check relationships between secret exponents without knowing the exponents themselves. For example, to verify that two points encode the same secret τ:

```
e(τ·G1, G2) == e(G1, τ·G2)
```

Both sides equal `e(G1, G2)^τ`, so they match if and only if the same τ was used. This is how we verify contributions without seeing the secrets.

### 3. Schnorr-like proofs of knowledge

When a participant updates the SRS, they need to prove they actually know their update factor (and didn't just submit garbage). They do this with a Schnorr-like proof:

1. Pick a random nonce `s`
2. Publish `(s·G1, δ·s·G1)` where δ is the update factor

A verifier can check this using pairings to confirm the contributor knows δ, without learning what δ is.

---

## How the ceremony works

### Step 1: Initialization

The first participant generates random (τ, α, β) and computes the entire SRS from scratch. They publish the SRS and a proof of knowledge, then **destroy** their random values.

```
Participant 1:
  τ₁, α₁, β₁ ← random
  SRS₁ = compute_srs(τ₁, α₁, β₁)
  proof₁ = prove_knowledge(τ₁, α₁, β₁)
  publish(SRS₁, proof₁)
  destroy(τ₁, α₁, β₁)
```

### Step 2: Sequential contributions

Each subsequent participant takes the current SRS, multiplies in their own randomness, and publishes the result:

```
Participant j (for j = 2, 3, ..., N):
  δ_τ, δ_α, δ_β ← random
  SRS_j = update_srs(SRS_{j-1}, δ_τ, δ_α, δ_β)
  proof_j = prove_knowledge(δ_τ, δ_α, δ_β)
  publish(SRS_j, proof_j)
  destroy(δ_τ, δ_α, δ_β)
```

The update rule is simple. For the tau powers in G1:

```
new[i] = δ_τ^i · old[i]
```

After all N participants, the accumulated tau is `τ = τ₁ · δ_τ₂ · δ_τ₃ · ... · δ_τ_N`. No single participant knows this product — they only know their own factor.

### Step 3: Verification

After each contribution, anyone can verify it was done correctly using six pairing checks:

1. **Tau ratio**: proves the contributor updated τ correctly
2. **Alpha ratio**: proves the contributor updated α correctly
3. **Beta ratio**: proves the contributor updated β correctly
4. **Tau cross-consistency**: G1 and G2 tau series use the same τ
5. **Alpha cross-consistency**: alpha series uses the same τ as the main series
6. **Beta cross-consistency**: beta series uses the same τ as the main series

If any check fails, the contribution is rejected.

### Step 4: SRS consistency check

After all contributions, the final SRS is verified for internal consistency. This checks that the power series actually form valid sequences:

```
e(τ^i · G1, τ · G2) == e(τ^{i+1} · G1, G2)
```

This equation holds for consecutive powers because both sides equal `e(G1, G2)^{τ^{i+1}}`.

### Step 5: Key derivation

**In a production ceremony**, the Groth16 keys are derived via Phase 2 of Groth16 MPC: the QAP is evaluated directly on the SRS curve points, so no single machine ever learns the accumulated scalars. This preserves the 1-of-N trust guarantee from the ceremony all the way through to the final keys.

**In the reference implementation**, the simulation tracks the accumulated ceremony scalars (τ, α, β) and hashes them (domain-separated SHA-256) into a ChaCha20 seed for arkworks' standard key generation. This means the machine running `derive_keys` knows the full toxic waste. The ceremony's 1-of-N trust guarantee does NOT carry through to the derived keys — it is a simulation of the ceremony protocol, not a production-secure setup. See the "Reference implementation vs production" section below for details.

### Step 6: Transcript publication

The ceremony produces a transcript — a chain of records, one per contribution, along with the SRS snapshot at each step:

```
Record j:
  index:      j
  proof:      Schnorr-like proof of knowledge
  srs_hash:   SHA-256(SRS after contribution j)
  srs_snapshot: the full SRS after contribution j
```

Anyone can fully verify the transcript by:
1. Checking the initial SRS + proof via `verify_initial_contribution`
2. Replaying each subsequent contribution proof against the before/after SRS pairs
3. Verifying each SRS hash matches the recorded hash
4. Verifying the final SRS passes full internal consistency checks

---

## Why is it secure?

### 1-of-N trust model

The toxic waste is the product of all participants' random values. To reconstruct it, an attacker needs **every** factor:

```
τ = τ₁ · δ_τ₂ · δ_τ₃ · ... · δ_τ_N
```

If even one participant honestly generates randomness and destroys it, the attacker is missing a factor. Without the full product, they cannot forge proofs.

This is an extremely strong guarantee. In a ceremony with 100 participants across different organizations, countries, and hardware, the probability that all 100 collude is negligible.

### Pairing checks prevent corruption

A malicious participant cannot corrupt the SRS without being detected. Every contribution is verified via pairing equations that check mathematical consistency. For example:

- Replacing `τ^2·G1` with a random point breaks `e(τ·G1, τ·G2) == e(τ^2·G1, G2)`
- Using a different τ for G1 and G2 breaks `e(τ·G1, G2) == e(G1, τ·G2)`

There is no way to produce a corrupted SRS that passes all six pairing checks. The verification is cryptographically complete.

### Proof of knowledge prevents replay

The Schnorr-like proof ensures each contributor actually generated fresh randomness. Without it, a participant could simply republish the previous SRS unchanged (a no-op contribution that doesn't add any security).

### Key derivation trust model

In a production Phase 2 MPC, keys are derived from SRS curve points via QAP evaluation — no single machine learns the toxic waste. The 1-of-N trust guarantee extends to the final keys.

In the reference implementation's simulation, one machine sees the accumulated scalars during key derivation. This is an inherent limitation of simulating MPC on a single machine. The ceremony protocol itself (contributions, pairing-based verification, transcript) is fully implemented; only the final key derivation step relies on a trusted coordinator.

---

## Circuit identifiers

Each circuit tier gets a unique identifier:

```
circuit_id = SHA-256("SEP-XXXX" || tier_id || tree_depth || poseidon_params_hash)
```

This binds the ceremony to a specific circuit definition. If the circuit changes (different Poseidon parameters, different tree depth), the ID changes, and a new ceremony is required.

| Tier | tier_id | tree_depth | circuit_id |
|------|---------|------------|------------|
| Small | 0 | 5 | SHA-256("SEP-XXXX" \|\| 0 \|\| 5 \|\| params_hash) |
| Medium | 1 | 8 | SHA-256("SEP-XXXX" \|\| 1 \|\| 8 \|\| params_hash) |
| Large | 2 | 11 | SHA-256("SEP-XXXX" \|\| 2 \|\| 11 \|\| params_hash) |

---

## What the code does

The `ceremony` module implements the full pipeline:

| Function | What it does |
|----------|-------------|
| `initialize()` | First participant: generate (τ, α, β), compute SRS, produce proof |
| `contribute()` | Subsequent participant: generate (δ_τ, δ_α, δ_β), update SRS, produce proof |
| `verify_contribution()` | Check a contribution via 6 pairing equations; rejects identity points and degenerate SRS |
| `verify_initial_contribution()` | Verify the first SRS + proof against the curve generators |
| `verify_consistency()` | Check ALL SRS elements for internal consistency (full O(n) pairing check) |
| `hash_srs()` | SHA-256 of all SRS curve points (with length prefixes) |
| `derive_keys()` | Groth16 key generation from accumulated ceremony scalars (simulation only) |
| `compute_circuit_id()` | Compute unique circuit identifier for a tier |
| `required_degree()` | Compute SRS degree from circuit constraint count |
| `run_ceremony()` | Orchestrate: initialize → contribute × N → verify → derive keys |
| `verify_transcript()` | Full replay: verify initial contribution, all subsequent proofs, hash chain, and final consistency |

### Test coverage (19 tests)

| Test | What it verifies |
|------|-----------------|
| Initialize and verify consistency | SRS dimensions correct, passes full consistency check |
| Single contribution verifies | One contribution passes all pairing checks |
| Chain of contributions | 3 sequential contributions all verify |
| Contribution changes SRS | Every SRS element changes after a contribution |
| Tampered SRS rejected | Replacing a curve point fails verification |
| Wrong proof rejected | Proof for different update factors is detected |
| Zero proof rejected | Identity (point-at-infinity) proof elements are rejected |
| Degenerate SRS rejected | SRS with zero key elements is rejected |
| SRS hash deterministic | Same SRS always produces same hash |
| SRS hash changes | Hash changes after contribution |
| Key derivation deterministic | Same accumulated scalars always produce same keys |
| Different scalars, different keys | Different ceremony inputs produce different keys |
| Ceremony keys work for proving | Full roundtrip: ceremony → derive keys → create proof → verify proof |
| Circuit ID deterministic | Same tier always produces same ID |
| Circuit ID differs per tier | Each tier has a unique ID |
| Run ceremony (small tier) | Full 3-participant ceremony → prove → verify |
| Verify transcript (full replay) | Full transcript replay passes; tampered final SRS fails |
| Tampered intermediate rejected | Tampered intermediate SRS snapshot fails transcript verification |
| Required degree | SRS degree is sufficient for each tier's constraint count |

---

## Reference implementation vs production

The reference implementation simulates the ceremony on a single machine. In production:

| Aspect | Reference implementation | Production ceremony |
|--------|------------------------|-------------------|
| Participants | Simulated sequentially | Independent machines, geographically distributed |
| Toxic waste | Known to the simulator machine | Never known to any single party |
| Key derivation | ChaCha20 seeded from accumulated scalars (the machine running it knows the toxic waste) | QAP evaluation on SRS curve points (Phase 2 MPC — no machine knows the toxic waste) |
| 1-of-N guarantee | Applies to SRS only; does NOT extend to derived keys | Extends through to final keys |
| Minimum participants | 1 (for testing) | 10+ (for security) |
| Verification | All pairing checks implemented, full consistency checks on all elements | Same checks, run by independent auditors |
| Transcript | In-memory with full SRS snapshots | Published publicly for anyone to verify |

The ceremony protocol (contributions, pairing-based verification, transcript replay) is identical in both cases. The critical difference is key derivation: the reference implementation's key derivation step breaks the 1-of-N trust guarantee because one machine sees the accumulated scalars. A production deployment MUST replace this with Phase 2 Groth16 MPC.
