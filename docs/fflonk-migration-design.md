# PLONK + EF KZG migration — drop our own ceremony

**Date:** 2026-04-29 · revised 2026-04-30
**Status:** Draft (Proposal — pre-implementation, post-spike)
**Author:** Onym contributors
**Version:** 0.2 — locks in the proving-stack spike outcome
**Supersedes:** v0.1 (same file, pre-spike)
**Related:**
- [`postmortem-ceremony-data-loss.md`](postmortem-ceremony-data-loss.md) — motivating incident; the operational argument for this migration
- [`implementation-plan-fflonk-migration.md`](implementation-plan-fflonk-migration.md) — phase-by-phase execution plan; needs a follow-up revision to match v0.2 of this doc
- [`group-governance-types-design.md`](group-governance-types-design.md) — parent design defining `groupType ∈ {Anarchy, OneOnOne, Democracy, Oligarchy, Tyranny}`; this design replaces the proving system underneath all five
- [`anarchy-update-testnet-design.md`](anarchy-update-testnet-design.md), [`democracy-update-testnet-design.md`](democracy-update-testnet-design.md), [`oligarchy-update-testnet-design.md`](oligarchy-update-testnet-design.md), [`oneonone-update-testnet-design.md`](oneonone-update-testnet-design.md) — sibling per-type designs; circuit, VK, and ceremony framing in this design supersedes Phase E ("Ceremony coordination") in each
- `contracts/sep-xxxx/src/lib.rs` — legacy monolithic contract; **decommissioned** by this migration

---

## Spike outcome (v0.2 changes from v0.1)

A pre-implementation spike resolved the open questions §10 flagged in v0.1. The principal corrections:

- **Proving crate is `jf-plonk` from `EspressoSystems/jellyfish` (git-only), not `jellyfish-plonk` (which doesn't exist on crates.io).** Pulled via Cargo `git` dependency, pinned to release tag.
- **jf-plonk's TurboPlonk verifier already does 2 pairings**, via Fiat-Shamir-combined KZG openings at evaluation points ζ and ζ·g. Verified by inspecting `plonk/src/proof_system/verifier.rs:201-256`: a single `E::multi_pairing([total_w, total_c], [beta_h, h])` call. No fflonk aggregation layer to write — we get fflonk-equivalent verify cost out of the box. The doc's "fflonk" framing has been generalised to "PLONK + KZG batched openings."
- **EF KZG SRS bytes are ≈400 KB after extraction** (n=4096 set), not ≈206 KB. The full EF transcript is 253 MB (four PoT sets up to n=2^15); we extract only the n=4096 set we need. A small build-time tool deserialises the transcript and writes the embedded blob.
- **`UnivariateUniversalParams` (jf-pcs) has no public from-bytes constructor.** ~200-300 LoC of deserialisation glue is on us. This is the only crypto-adjacent code we need to write from scratch.
- **The repo is a single crate, not a Cargo workspace.** Edits to `[dependencies]` go in the root `Cargo.toml`. Soroban contracts each have their own crate root and get separate dep edits in Phase C.

The strategic choice (universal SRS via EF KZG, no project-run ceremony, deterministic per-circuit preprocessing, VK-as-bytecode) is unchanged. The implementation surface is concrete now: pick `jf-plonk`, write the SRS extractor, port circuits to jf-plonk's gate API.

---

## 1. Background

The codebase ships a Groth16-based zk-SNARK stack on BLS12-381 with a per-circuit trusted-setup ceremony. The repo is a **single Rust crate** (root `Cargo.toml` is the package manifest, not a workspace); Soroban contracts and the ceremony coordinator each have their own crate roots and don't share dependency declarations with the prover.

- **Proving system:** `ark-groth16 = "0.4"` plus the arkworks supporting crates (`Cargo.toml:13-40`). Three circuits — `MembershipCircuit` (`src/circuit/mod.rs`), `UpdateCircuit` (`src/circuit/update.rs`), `DemocracyUpdateCircuit` (`src/circuit/democracy.rs`).
- **Setup:** Two-phase MPC. Phase 1 produces an SRS over powers-of-tau on BLS12-381 (`src/ceremony/mod.rs`, ≈1,200 LOC). Phase 2 turns each phase-1 SRS into per-circuit prover/verifier keys (`src/ceremony/phase2.rs`, 213 LOC; `ceremony-coordinator/src/handlers/phase2.rs`, 347 LOC; routes registered in `ceremony-coordinator/src/main.rs:117-130`). Phase 2 has not run.
- **Ceremony surface:** `ceremony-coordinator/` HTTP service (≈3 kLOC), `tools/ceremony/` participant CLI, `crates/ceremony-wasm/` browser verifier, `deploy/ceremony/` static frontend, `deploy/blossom/` retention-pinned blob store, three `scripts/install-*-vks-*.sh` deploy helpers.
- **Verifier:** Each gov-type contract (`sep-anarchy`, `sep-democracy`, `sep-oligarchy`, `sep-oneonone`, `sep-tyranny`) embeds the Groth16 verify path, calls `bls12_381::pairing_check` over 4 pairs per proof, and stores per-tier verifying keys under `DataKey::VK(tier)` / `DataKey::UpdateVK(tier)` storage. The legacy `sep-xxxx` contract holds the same logic plus dispatch for all governance types.
- **Mobile clients:** iOS (`swift-mls/`, `clients/ios/`) and Android (`kotlin-mls/`, `clients/android/`) bundle the proving system through an FFI surface in `src/ffi.rs` (1,591 LOC) and `src/jni_ffi.rs` (519 LOC). Each platform ships 12 keyset files (6 proving keys + 6 verifying keys) under `Resources/keyset-v2/` (iOS) and `assets/keyset-v2/` (Android).

The Groth16 setup is sound. The operational surface around it is what this design replaces.

## 2. Problem

Three distinct pressures converge on the same answer.

### 2.1 The ceremony is fragile and we can't afford to keep maintaining it

The 2026-04-29 postmortem ([`postmortem-ceremony-data-loss.md`](postmortem-ceremony-data-loss.md)) documents two independent silent-data-loss bugs in the ceremony stack — a Blossom default-config LRU prune destroying 22 of 30 phase-1 sidecars, and a coordinator Nostr publisher silently dropping 12 of 15 round-commit attestations after the first websocket idle drop. Both bugs were latent for ten days before anyone noticed.

The ceremony surface is large — coordinator service, custom signed-receipt format, custom relay-publishing, custom storage retention rules, custom WASM browser verifier, custom participant CLI, custom transcript frontend. Each layer is a potential default-config or silent-error trap. We hit two; we have no reason to believe we found all of them.

### 2.2 Per-circuit phase-2 doesn't compose with five separate gov contracts

Groth16's circuit-specific setup means **every new circuit needs a new ceremony**. Today the inventory is:
- 3 membership-tier circuits (small/medium/large)
- 3 update-tier circuits (same tiers)
- 1 democracy-specific update circuit (with quorum binding)
- 1 oligarchy-specific update circuit pending (admin co-signing)
- 1 anarchy-specific (reuses generic update)
- 1 tyranny-specific update pending
- 1 oneonone variant

That's 9–11 circuits across the five governance types, each currently scheduled to consume its own phase-2 contribution chain. With the operational risk profile demonstrated in the postmortem, running 9–11 ceremonies sequentially is not realistic.

### 2.3 We're already on the right curve

Soroban's only zk-friendly host functions are **BLS12-381** (`g1_msm`, `g2_msm`, `pairing_check`, etc.). Any other curve must implement field arithmetic in user-space WASM, which is 100–1000× more expensive per verify. **Ethereum Foundation's 2023 KZG ceremony produced a public, audited, ≈140k-contributor powers-of-tau SRS on BLS12-381.** The curve we're already committed to is the curve a globally-audited universal SRS already exists for, on the same field.

## 3. Constraints

### 3.1 Soroban verify gas budget

The verifier must run within ledger fees acceptable for client UX. Current Groth16 verify: 1 G1-MSM (≤3 scalars) + 1 `pairing_check` over 4 pairs. The new verifier must stay within ~2× this envelope; ~1.5× is the target.

This rules out vanilla PLONK (≈9 pairings per verify) as the production verifier shape. KZG-batched-opening aggregation collapses verify to **2 pairings** plus extra MSM, which is the realistic target. jf-plonk's TurboPlonk implementation already does this aggregation (see §4.1), so no separate aggregation layer is required.

### 3.2 Curve & host compatibility

BLS12-381 is non-negotiable. This eliminates BN254-only schemes (most of the gnark/snarkjs PLONK implementations), Pasta-curve schemes (Halo2-IPA), and small-field STARKs (Plonky3 on Goldilocks/BabyBear). The proving system must be a KZG-PCS PLONK family on BLS12-381.

### 3.3 SRS coverage

EF's 2023 KZG ceremony provides 4096 G1 elements + 65 G2 elements. Our circuit row counts under PLONK custom gates (with arithmetic + Poseidon + lookup gadgets) all fit in n=2048 — roughly 2× headroom. If a future circuit ever needs more, Aztec Ignition (BLS12-381, n=2^21) is the secondary source.

### 3.4 Mobile prover envelope

iOS prover memory budget is ≤256 MB resident; Android low-end devices ≤128 MB. PLONK prover memory is dominated by O(n) FFTs over BLS12-381 scalar field elements — for n=2048, that's ≈64 KB of scalars plus polynomial workspaces. Well within budget. Prover time on a 2022-era phone for n=2048 is sub-second.

### 3.5 Five-contract scope, no v2 keyset

This migration rewrites the verifier in each of `sep-anarchy`, `sep-democracy`, `sep-oligarchy`, `sep-oneonone`, `sep-tyranny`. `sep-xxxx` is **dropped wholesale** — not migrated. Existing testnet groups under `keyset-v2` continue to work on `sep-xxxx` until decommission; new groups created post-migration land on the per-type contracts with the new verifier.

### 3.6 No production user impact

`keyset-v2` is the active testnet keyset. Mainnet has not deployed. The migration ships before any production user depends on the legacy proving system.

## 4. Design

### 4.1 Proving system: TurboPlonk via jf-plonk on BLS12-381

**Selection:** [`jf-plonk`](https://github.com/EspressoSystems/jellyfish/tree/main/plonk) (the `plonk` crate inside Espresso Systems' `jellyfish` workspace). Production-grade Rust TurboPlonk implementation, BLS12-381 native, arkworks-based. Pulled via Cargo `git` dependency:

```toml
jf-plonk = { git = "https://github.com/EspressoSystems/jellyfish", tag = "0.8.0", default-features = false, features = ["std"] }
jf-pcs   = { git = "https://github.com/EspressoSystems/jellyfish", tag = "0.8.0", default-features = false, features = ["std"] }
jf-relation = { git = "https://github.com/EspressoSystems/jellyfish", tag = "0.8.0", default-features = false, features = ["std"] }
```

**Why this gets us fflonk-equivalent performance for free.** jf-plonk's `Proof` carries two single (not vector) opening fields:

```rust
pub struct Proof<E: Pairing> {
    pub wires_poly_comms: Vec<Commitment<E>>,
    pub prod_perm_poly_comm: Commitment<E>,
    pub split_quot_poly_comms: Vec<Commitment<E>>,
    pub opening_proof: Commitment<E>,         // ← single aggregated KZG opening at ζ
    pub shifted_opening_proof: Commitment<E>, // ← single aggregated KZG opening at ζ·g
    pub poly_evals: ProofEvaluations<E::ScalarField>,
    pub plookup_proof: Option<PlookupProof<E>>,
}
```

The verifier (`plonk/src/proof_system/verifier.rs:201-256`) Fiat-Shamir-combines those two openings with a random scalar `u`, then performs **one `multi_pairing` call on 2 G1×G2 pairs**:

```rust
let result = E::multi_pairing(
    [total_w, total_c],
    [verifier_param.beta_h, verifier_param.h],
).0.is_one();
```

That's exactly the fflonk shape: `e(W, [τ]₂) = e(C, [1]₂)` after batching. No additional aggregation layer to implement on our side.

**Comparison to Groth16:**

| Property | Groth16 (current) | jf-plonk (proposed) |
|---|---|---|
| Setup type | Per-circuit (phase 1 + phase 2) | **Universal** (phase 1 only) |
| SRS source | Self-run ceremony, 7 contributors | EF KZG, ≈140k contributors |
| Soroban verify | 1 MSM + 1 `pairing_check`(4 pairs) | 1 MSM + 1 `pairing_check`(2 pairs) |
| Field-op overhead | minimal | ≈15 SHA-256 ops + linearization arithmetic |
| Proof size, wire | 192 B | ≈700 B (TurboPlonk + KZG) |
| Proof size, on-chain | 384 B | ≈900 B uncompressed |
| Prover memory | small | small (n≤2048 dominates) |
| Adding a new circuit | New ceremony required | Recompile only |
| Custom gates / lookups | None | TurboPlonk + Plookup (Poseidon round constants compress ≈2× vs. R1CS) |

The verifier's pairing count drops from 4 to 2; the additional MSM scalars and SHA-256 transcript ops push net Soroban gas to roughly 1.3–1.7× current. Within the §3.1 budget.

**Why not a different crate.** `dusk-plonk` is on crates.io but uses its own forked BLS12-381 (`dusk-bls12_381`), breaking arkworks consistency, and ships vanilla PLONK without TurboPlonk's custom-gate compression. `arkworks-rs/plonk` doesn't exist as a maintained published crate. Halo2-KZG on BLS12-381 would be functionally equivalent to jf-plonk but has no advantage over it for our use. jf-plonk is the right pick on the merits.

**The one piece of code we own.** jf-pcs's `UnivariateUniversalParams<Bls12_381>` struct has no public from-bytes constructor — only `gen_srs_for_testing()` (test-only) and `trim()` (downsize an existing SRS). To consume EF KZG bytes we write an extractor (see §4.2). Estimated ≈200-300 LoC; it's the only crypto-adjacent code we add from scratch.

### 4.2 SRS: Ethereum Foundation KZG

**Source.** EF's 2023 KZG ceremony for EIP-4844 proto-danksharding, finalised 2023-11-14, with ≈141k contributors. The published transcript is **253 MB** (per `seq.ceremony.ethereum.org`'s `Content-Length`) and contains four distinct PoT sets at sizes 2^12, 2^13, 2^14, 2^15 G1 elements — each with 65 G2 elements — plus participant signatures and identity attestations.

**What we extract.** Just the smallest set: **n=4096 G1 + 65 G2 elements on BLS12-381**, dropping signatures and identity bytes. After conversion to arkworks-uncompressed encoding:

- 4096 × 96 B (uncompressed G1) = 393,216 B
- 65 × 192 B (uncompressed G2) = 12,480 B
- ≈ **400 KB embedded blob**

(2× the v0.1 estimate of ≈206 KB; still small. Compressed encoding is ≈206 KB but uncompressed is the format jf-pcs consumes natively.)

**Why this works:**
- BLS12-381 — same curve Soroban gives us native `pairing_check` host functions for.
- 4096 G1 — covers our largest circuit (≤2048 PLONK rows under custom gates + Plookup) with ≈2× headroom.
- 65 G2 — TurboPlonk's verifier needs 2 G2 elements (`[1]₂`, `[τ]₂`). Plenty.
- ≈141k contributors — soundness reduces to "at least one of 141k was honest." Strictly stronger than any 7-contributor ceremony we could realistically run ourselves.
- Public artifact — anyone can independently re-derive the SRS from the EF transcript. We don't host it; we ship its SHA-256 hash and embed the bytes.

**Extraction pipeline.** A one-shot build script handles the 253 MB → 400 KB reduction:

1. `scripts/build/fetch-ef-kzg.sh` — `curl` the transcript from `seq.ceremony.ethereum.org`.
2. `scripts/build/extract-ef-kzg.py` — parse the JSON transcript, validate against the published participant signatures, extract the n=4096 G1 + 65 G2 set, write to `src/prover/srs/ef-kzg-2023.bin` in arkworks-uncompressed encoding.
3. `build.rs` — SHA-256-asserts the embedded bytes against `src/prover/srs/expected-hash.in`. Mismatch fails the build with `"SRS hash mismatch — refusing to build"`.

The hash file is committed and reviewed; the binary blob is committed and the hash file independently pins what the binary should be. A file-replace attack on the blob would require also touching the hash file in the same commit (visible in code review).

**The custom deserialiser.** jf-pcs's `UnivariateUniversalParams<Bls12_381>` has no public from-bytes constructor. We provide one:

```rust
// src/prover/srs.rs
pub fn load_ef_kzg_srs() -> UnivariateUniversalParams<Bls12_381> {
    let bytes = include_bytes!("srs/ef-kzg-2023.bin");
    // Parse 4096 G1 affine points (96 B each, big-endian uncompressed BLS12-381)
    // followed by 65 G2 affine points (192 B each).
    // Construct UnivariateUniversalParams { powers_of_g, h, beta_h, powers_of_h }.
    deserialize_ef_kzg(bytes).expect("embedded SRS bytes invalid — build.rs hash check should have caught this")
}
```

Estimated ≈200-300 LoC including a SHA-256 self-test and round-trip test against jf-pcs's own `gen_srs_for_testing()` output (just to confirm our parser produces a struct shape jf-pcs accepts). This is the only crypto-adjacent code we write from scratch.

**Distribution.** SRS bytes compiled into the prover crate as a `static` byte array. Mobile and server prover binaries embed the same bytes (same SHA-256). Soroban verifiers embed only the verifier-key portion (a few G2 points + the Lagrange-form selector commitments per circuit, ≈1–2 KB per circuit), produced by deterministic per-circuit preprocessing — see §4.4.

### 4.3 Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                                                                 │
│   EF KZG SRS (BLS12-381, n=4096 G1 + 65 G2, ≈400 KB)            │
│   ─ extracted from EF's 2023 KZG transcript (253 MB → 400 KB)   │
│   ─ embedded as `include_bytes!` in `src/prover/srs.rs`         │
│   ─ build.rs SHA-256-asserts against expected-hash.in           │
│                                                                 │
│           ┌──────────────────────────┐                          │
│           │ src/circuit/{mod,update, │                          │
│           │ democracy,oligarchy,…}.rs│                          │
│           │ — jf-relation TurboPlonk │                          │
│           │   custom-gate API        │                          │
│           └────────────┬─────────────┘                          │
│                        │                                        │
│                        ▼                                        │
│           ┌──────────────────────────┐                          │
│           │ jf_plonk::PlonkKzgSnark  │  produces                │
│           │ ::preprocess(&srs, &cs)  │  (ProvingKey, VerifyingKey)
│           │ — deterministic; one     │                          │
│           │   call per circuit       │                          │
│           └────────────┬─────────────┘                          │
│                        │                                        │
│       ┌────────────────┴───────────────┐                        │
│       │                                │                        │
│       ▼                                ▼                        │
│  proving_key.bin                  verifier_key.bin              │
│  (mobile + server prover bundle)  (compiled into each gov       │
│                                    contract via build.rs)       │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

**No coordinator. No participant CLI. No browser verifier. No retention rules. No phase-2.** A new circuit is added by writing the gadget and recompiling — the SRS is universal, the verifier key is deterministic preprocessing, and the new VK lands alongside the contract source.

### 4.4 Soroban verifier shape

Per-contract `verify_<proof_type>` becomes:

```rust
pub fn verify_membership(env: &Env, proof: PlonkProof, public_inputs: &[BytesN<32>]) -> bool {
    let vk = Self::membership_vk(env);
    // 1. Reconstruct Fiat-Shamir challenges via Soroban SHA256 host fn
    let challenges = compute_challenges(env, &proof, public_inputs);
    // 2. Build linearization commitment via G1 add + scalar muls
    let lin_commit = linearize(env, &vk, &proof, &challenges);
    // 3. Compute opening MSM
    let opening_lhs = env.crypto().bls12_381().g1_msm(/* ~20 scalars */);
    // 4. Final pairing check on 2 pairs
    env.crypto().bls12_381().pairing_check(/* [(P1, Q1), (P2, Q2)] */)
}
```

Verifier-key storage is per-contract `static` constants (compiled in), not `DataKey::VK(tier)` storage. This is a significant simplification from the current sep-xxxx model — the VK is part of the contract bytecode, rotated by deploying a new contract version, not by an `update_vk` admin entrypoint.

This simplification is enabled by the universal SRS: if the verifier never changes between circuits, why pay storage for it? The answer in Groth16 was "to allow VK rotation as a kill switch." Under jf-plonk + universal SRS, deploying a new contract version achieves the same thing more cleanly, and groups continue using whichever contract version they were created against.

### 4.5 Mobile clients

**iOS:** `clients/ios/StellarChat/StellarChat/Resources/keyset-v2/` (12 files, ~5 MB) → replaced by:
- `Resources/srs.bin` — universal SRS (≈400 KB uncompressed, shared across all circuits)
- `Resources/circuits/*.pk.bin` — per-circuit proving keys (preprocessed selector commitments + Lagrange-form polynomials; one per circuit-tier combination)
- VKs are in-contract bytecode, not bundled with the client

**Android:** `clients/android/StellarChat/app/src/main/assets/keyset-v2/` → analogous restructure under `assets/srs/` and `assets/circuits/`.

Net mobile bundle size delta: roughly **flat or slightly larger** (≈400 KB SRS + per-circuit pks land at similar total size to the current 12 Groth16 pk files; exact delta tracked in Phase D).

**FFI surface (`src/ffi.rs` + `src/jni_ffi.rs`):**
- Drop `sep_generate_testing_proving_key`, `sep_generate_testing_update_proving_key`, `sep_generate_testing_democracy_update_proving_key` and their JNI wrappers — these existed because Groth16 needed per-circuit setup. PLONK-with-universal-SRS needs none of this.
- Keep `sep_generate_membership_proof`, `sep_generate_update_proof`, `sep_generate_democracy_update_proof` etc. but their internal implementation calls `jf_plonk::PlonkKzgSnark::prove(...)` instead of `Groth16::<Bls12_381>::prove`.
- Proof serialisation (`sep_proof_to_contract_format`) format changes from 384 B uncompressed Groth16 to ≈900 B uncompressed TurboPlonk; client and contract are both updated together. The wire format gains a 1-byte version prefix (`0x01` for the initial PLONK shape) so a future swap can be wire-distinguishable.

## 5. Alternatives Considered

| Alternative | Why not |
|---|---|
| **Stay on Groth16, just fix the prune + relay_loop bugs.** | Closes today's instances, leaves the architecture that produces them. Still need 9–11 phase-2 ceremonies for the gov contracts. Postmortem §"alternatives" rejects this. |
| **Stay on Groth16, run our own ceremony correctly this time.** | Same operational surface; same default-config and silent-error risk profile; doesn't address phase-2 multiplication for new circuits. |
| **dusk-plonk on BLS12-381 (crates.io).** | Vanilla PLONK with no TurboPlonk custom-gate compression and no aggregated openings ⇒ ≈9-pairing verify, ~2.5× current verify gas. Also ships its own forked BLS12-381 (`dusk-bls12_381`), breaking arkworks consistency for the surrounding circuit and ceremony code. Crates.io publication is the only thing it has over jf-plonk; not enough to outweigh the cost. |
| **Hand-rolled PLONK on `ark-poly-commit`.** | Most flexible but most work — implementing the IOP from scratch is a multi-month project with first-rate audit risk. jf-plonk already exists, is BLS12-381-native, and the spike confirmed its verifier shape matches what we need. |
| **Halo2-IPA (transparent, no setup).** | Eliminates ceremony entirely but uses Pasta curves — Soroban has no Pasta host functions, so verify cost balloons by ~50–200×. Blocked by §3.2. |
| **Plonky3 / STARK.** | Eliminates ceremony, but small-field arithmetic + FRI commitments mean Soroban verify needs hundreds of KB of proof and field-op-heavy verify implemented in WASM. ~500–2000× current gas. Blocked by §3.2. |
| **Halo2-KZG (BLS12-381).** | Functionally equivalent to jf-plonk; same SRS story; comparable verify cost. No advantage; jf-plonk has a tighter API and more direct test surface for what we need. |
| **Aztec Ignition SRS instead of EF KZG.** | Both work. Aztec gives 2^21-degree headroom (vs. 4096 for EF's smallest set), but our circuits don't need it. EF is the larger contributor pool and the more recent ceremony, so easier to argue trust. Aztec is the documented secondary source if a future circuit ever exceeds 4096. |
| **Run our own KZG ceremony (small participant pool, but on universal SRS).** | Eliminates phase-2 friction but reintroduces the whole operational surface this design exists to remove. No. |

## 6. Implementation Plan — six phases

### Phase A — receipt salvage *(parallel, ≤1 week)*

Independent of the proving-system migration; recovers public auditability of the existing testnet ceremony record.

- A.1. Outreach to the 7 participant pubkeys with their per-round missing-receipt hash list (from `ceremony-backup/20260429T163349Z/MISSING.txt`). Ask for re-upload of `state.txt` and `receipt.txt` they still have locally.
- A.2. Re-publish recovered blobs to Blossom under PR #161's pinned-pubkey rule. Hashes match → `rounds.{state_txt_hash, receipt_hash}` rows are restored without coordinator changes.
- A.3. Document final recovery state in `docs/postmortem-ceremony-data-loss.md` "Blast radius" → "final".

Not blocking on B/C/D/E; not gated by F.

### Phase B — proving-system swap *(2–3 weeks)*

The repo's root `Cargo.toml` is a single-crate manifest, not a workspace; B.1 edits go there directly. Soroban contracts under `contracts/sep-*/` each have their own crate root and are touched in Phase C.

- B.1. Add `jf-plonk`, `jf-pcs`, `jf-relation` as git deps in the root `Cargo.toml` (pinned to release tag, not `main`). Keep `ark-groth16`/`ark-snark` for now behind a `feature = "groth16"` flag so the existing flows continue to work; remove them in B.4 once the swap is wired. `ark-bls12-381`, `ark-ff`, `ark-ec`, `ark-serialize` stay (jf-plonk reuses them).
- B.2. Implement the EF KZG SRS pipeline:
  - `scripts/build/fetch-ef-kzg.sh` — pulls the 253 MB transcript.
  - `scripts/build/extract-ef-kzg.py` — parses transcript JSON, validates participant signatures, extracts the n=4096 G1 + 65 G2 set, writes ≈400 KB to `src/prover/srs/ef-kzg-2023.bin`.
  - `src/prover/srs.rs` — ≈200-300 LoC `load_ef_kzg_srs() -> UnivariateUniversalParams<Bls12_381>` deserialiser. `include_bytes!` the blob at compile time.
  - `build.rs` — SHA-256 assertion against `src/prover/srs/expected-hash.in`. Mismatch fails the build.
- B.3. Port circuits to jf-relation's TurboPlonk gate API (`Circuit::synthesize(&self, builder: &mut PlonkCircuit<Fr>)`):
  - `src/circuit/mod.rs` (788 LOC) → membership circuit.
  - `src/circuit/update.rs` (440 LOC) → update circuit.
  - `src/circuit/democracy.rs` (851 LOC) → democracy-update circuit. Quorum-binding logic stays identical; only the gate format changes.
  - Use Plookup tables for Poseidon round constants — typically halves PLONK row count vs. naive multiplicative encoding.
  - Future per-type circuits (oligarchy, tyranny, oneonone) inherit the same skeleton.
- B.4. Replace `src/prover/mod.rs` (1,030 LOC) Groth16 prove/verify calls with `jf_plonk::PlonkKzgSnark::{preprocess, prove, verify}`. Drop `ark-groth16`, `ark-snark` from `Cargo.toml`.
- B.5. Cross-platform test vectors (`docs/cross-platform-test-vectors.json`) regenerated for the new proof format. Pin verifies, not byte-equality (PLONK is randomised in the prover; only verify is deterministic).
- B.6. Integration tests proving end-to-end for each circuit against the embedded SRS: round-trip prove → verify, tampered-proof rejection, wrong-VK rejection, public-input rebinding rejection.

Exit criterion: `cargo test` green (root crate); round-trip prove → verify for all current circuits succeeds against the embedded SRS; SRS-hash assertion fires on tampering.

### Phase C — Soroban verifier rewrite *(2 weeks, parallel with B after B.1)*

- C.1. Port the verifier in each gov contract:
  - `contracts/sep-anarchy/src/lib.rs` (975 LOC)
  - `contracts/sep-democracy/src/lib.rs` (993 LOC)
  - `contracts/sep-oligarchy/src/lib.rs` (1,257 LOC)
  - `contracts/sep-oneonone/src/lib.rs` (595 LOC)
  - `contracts/sep-tyranny/src/lib.rs` (1,063 LOC)
  Each contract gains a `verify_*` path calling the new verifier shape (§4.4) and embeds its VK as `static` constants.
- C.2. Drop `DataKey::VK(tier)` and `DataKey::UpdateVK(tier)` storage usage from each contract; remove the `update_vk` admin entrypoint (the deploy-new-contract-version pattern replaces it).
- C.3. Bench Soroban verify gas on testnet against the existing Groth16 path; fail Phase C if gas exceeds 2× current.
- C.4. Remove `contracts/sep-xxxx/` from the workspace. The crate is moved out of the build, code retained in git history; existing testnet deployments continue to operate on the old WASM until decommissioned.

Exit criterion: each gov contract builds, has a passing testnet deployment with a real TurboPlonk proof, and gas measurements within budget.

### Phase D — mobile rebundle *(1 week, parallel with C)*

- D.1. Replace `clients/ios/StellarChat/StellarChat/Resources/keyset-v2/` 12-file bundle with `Resources/srs.bin` + `Resources/circuits/*.pk.bin`.
- D.2. Same for `clients/android/StellarChat/app/src/main/assets/keyset-v2/`.
- D.3. Update FFI:
  - `src/ffi.rs:405-517` — drop `sep_generate_testing_*_proving_key` family.
  - `src/ffi.rs:345-408` — replace `sep_proof_to_contract_format` with the new TurboPlonk encoding (1-byte version prefix `0x01` + ≈900 B uncompressed).
  - `src/jni_ffi.rs:342-505` — drop the same JNI surface.
  - `swift-mls/Sources/SwiftMLS/{ProofGenerator,RustBridge}.swift` — drop the obsolete entrypoints; keep proof-generation API stable.
  - `kotlin-mls/src/main/java/com/stellarmls/mls/{SEPProofGenerator,RustBridge}.kt` — same.
- D.4. iOS/Android integration tests: prove on device against a known group, submit to the new contract on testnet, verify acceptance.

Exit criterion: clean build of both platforms with the new bundle; a real proof generated on device verifies on testnet.

### Phase E — coordinator decommission *(1 week, after B+C+D ship)*

- E.1. Delete:
  - `ceremony-coordinator/` (3 kLOC HTTP service)
  - `tools/ceremony/` (participant CLI)
  - `crates/ceremony-wasm/` (browser verifier)
  - `deploy/ceremony/` (frontend)
  - `deploy/blossom/` (kept as-is; PR #161's rules apply)
  - `scripts/install-membership-vks-testnet.sh`, `scripts/install-democracy-vks-testnet.sh`, `scripts/install-adminupdate-vk-testnet.sh`, `scripts/generate-keyset.sh`, `scripts/generate-democracy-vk-dev.sh`, `scripts/verify-ceremony-tool.sh`, `scripts/update-ceremony-downloads.py`.
  - `src/ceremony/` (1.4 kLOC)
- E.2. Drop the `phase2_rounds` table and the entire phase-2 schema layer (already unused).
- E.3. Remove `ceremony-coordinator` and related routes from `docker-compose.yml`. Remove `nostr-relay` from compose **only if** no other service depends on it (relayer + pn-relay both use it — keep the relay; just remove the coordinator).
- E.4. DNS: remove `ceremony.onym.chat` record; redirect the old URL to a static page pointing at the postmortem and the new design.
- E.5. Take down `https://ceremony.onym.chat` content; archive `ceremony-backup/20260429T163349Z/` to cold storage as the historical record.

Exit criterion: `cargo build --workspace` green with the listed paths absent; testnet still operational on the new contracts.

### Phase F — sep-xxxx decommission and ledger close *(1 week, last)*

- F.1. Migrate any remaining testnet groups created against `sep-xxxx` (none expected for production users; testnet-only).
- F.2. Drop `contracts/sep-xxxx/` from git (history retained). Drop related `scripts/deploy_sep_xxxx_testnet.sh`.
- F.3. Update `docs/group-governance-types-design.md` to remove the "implemented in sep-xxxx" hedges; the per-type contracts are authoritative.
- F.4. Update postmortem status to "Complete."
- F.5. Tag a release; publish a freeze announcement on the ceremony-replacement static page.

Exit criterion: workspace contains only the 5 gov-type contracts; no references to sep-xxxx in `Cargo.toml`, deploy scripts, or design docs except in postmortem context.

## 7. Test Plan

- **Phase B:** existing arkworks Groth16 tests are deleted; new jellyfish PLONK tests cover (a) prove-then-verify round trips per circuit, (b) tampered-proof rejection per circuit, (c) wrong-VK rejection, (d) public-input rebinding rejection. SRS-hash check at build time is the highest-leverage test — it makes "we accidentally shipped the wrong SRS" structurally impossible.
- **Phase C:** per gov contract, a Soroban-vm test that consumes a real TurboPlonk proof produced by the Rust prover and asserts `verify_membership/update returns true`. Cost-budget assertions: `env.budget()` reports stay under 2× the pre-migration baseline.
- **Phase D:** on-device prove → testnet verify round trip on iOS (real iPhone, both A-series and M-series) and Android (low-end + mid-tier devices). Memory and time measurements logged for the rollout doc.
- **Phase E:** post-decommission smoke test — testnet group create, member add, member remove, group update — all running entirely on the new contracts, no coordinator service, no `keyset-v2` bundles.

## 8. Rollout (cross-phase coordination)

- **Branch hygiene:** Phase A is independent (no code changes; participant outreach + Blossom uploads only). Phase B can land on `main` behind a Cargo `feature = "plonk"` flag without breaking existing flows; default stays Groth16 until Phase C ships. Phase C is the cutover. Phases D/E/F are sequential cleanup.
- **Testnet first.** Each gov contract deploys to testnet under the new verifier and runs a 2-week soak before mainnet consideration. Mainnet itself is downstream of [`mainnet-deployment.md`](mainnet-deployment.md) and is **not** included in this design's scope.
- **Mobile flag day.** Old clients with Groth16 proving keys cannot generate proofs accepted by the new contracts and vice versa. Phase D coordinates app-store submission with Phase C contract deployment so users updating their app land on a contract version that accepts their proofs.
- **Frozen sep-xxxx.** Between Phase C cutover and Phase F decommission, `sep-xxxx` continues to verify Groth16 proofs from old clients. No new groups are created against it. `keyset-v2/` keeps working until that contract is taken down.

## 9. Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| **EF KZG SRS deserialisation glue.** ~200-300 LoC of byte-parsing that has to produce a struct jf-pcs accepts. A bug here corrupts every proof silently. | Medium | Round-trip test our deserialised SRS against jf-pcs's `gen_srs_for_testing()` output (shape + serialise-then-deserialise). Run the prover end-to-end before the Soroban verifier work in Phase C lands. SHA-256 hash assertion at build time prevents shipping the wrong bytes. |
| **EF KZG SRS coverage edge case.** A future circuit needs n>2048 PLONK rows. | Low (current circuits well below 2048; TurboPlonk + Plookup compress ≈2× vs. R1CS) | Switch SRS source to Aztec Ignition (n=2^21) by re-running the extractor against a different transcript and updating the hash file. SRS source is one `static` blob plus its hash. |
| **Soroban verify gas exceeds 2× budget.** | Low–medium | Phase C.3 fails the rollout. Mitigations include precomputing more linearisation constants as contract `static` data and trimming the Fiat-Shamir transcript ops. The 2-pair `pairing_check` shape itself is fixed by the protocol — we won't get below it, but it's already what we want. |
| **jf-plonk git dep stability.** Pulling crypto code from a git tag rather than crates.io is unusual; an upstream force-push or branch rename could break the build. | Low | Pin to a specific tag (not branch). Vendor the crate into `vendor/jf-plonk/` if upstream behaviour becomes a concern. |
| **Mobile prover memory regression.** | Low (n≤2048 well within budget) | Memory measurement is part of Phase D's exit criterion. If exceeded, increase Plookup table reuse or split a circuit into sub-circuits. |
| **Migration takes longer than budgeted; testnet has two proving systems live for a quarter.** | Medium | Both systems ship behind the same client API surface; the proof type is wire-format-distinguishable (1-byte version prefix) so a contract can accept either during a transition window if absolutely necessary. We avoid this if at all possible. |
| **Audit gap: TurboPlonk circuit gadgets re-implemented from R1CS are easy to get wrong.** | Medium | Use jf-relation's audited Poseidon and Merkle gadgets wherever possible. Each circuit gets an independent review. Cross-platform test vectors + R1CS-vs-TurboPlonk equivalence tests (prove the same statement under both proving systems and confirm both verify) are the second line of defence. |

## 10. Open Questions

- **~~fflonk-specific Rust crate~~ — RESOLVED in v0.2 spike.** jf-plonk's TurboPlonk verifier already does 2 pairings via Fiat-Shamir-combined KZG openings (`plonk/src/proof_system/verifier.rs:201-256` shows a single `multi_pairing` on 2 G1×G2 pairs). No fflonk aggregation layer to write. See "Spike outcome" preamble.
- **~~VK-as-bytecode vs. VK-as-storage~~ — RESOLVED.** Embedded as bytecode constants per §4.4. Per-deploy version-rotation aligns with how the per-type contracts already version themselves.
- **~~Aztec Ignition vs. EF KZG~~ — RESOLVED.** EF KZG is primary (larger contributor pool, more recent ceremony, simpler trust argument). Aztec is documented secondary source if a future circuit ever exceeds n=4096.
- **Phase A participant outreach scope** — do we attempt to recover all 22 sidecars, or accept the partial loss and move on? *(Tentative answer: best-effort outreach for two weeks, then close out.)*
- **jf-plonk vendoring** — pin to a tagged release in Cargo, or vendor the crate into `vendor/jf-plonk/`? *(Tentative answer: tag-pinned dep first; vendor only if the upstream surface ever destabilises.)*
- **Browser verifier replacement** — `crates/ceremony-wasm` was used by the ceremony page. With the page gone, do we ship any browser-side verifier, or is "verify on the server, use TLS" sufficient for the dropped-ceremony freeze announcement page? *(Tentative answer: drop entirely. Anyone who wants to re-verify can run the Rust prover/verifier locally.)*
- **Plookup-table layout for Poseidon** — one shared lookup table across all circuits, or per-circuit tables? Affects bundle size and prover memory. *(Tentative answer: shared table; resolved in B.3 implementation.)*

## 11. Follow-Up Work

- **Per-circuit benchmark harness.** Once Phase B lands, codify the prover-time and verifier-gas measurements in a CI job. The migration thesis depends on a specific cost envelope; we should keep that envelope honest.
- **Circuit-level postmortem hygiene.** When the gov-type circuits ship (oligarchy v0.1.5, tyranny initial), each gets a circuit-specific test-vector file regenerated against the new proving system.
- **Move `keyset-democracy-dev` and `keyset-v1` out of the repo root.** Both directories become historical artifacts after Phase F. Archive to `docs/historical/` with a README pointing at the postmortem.
- **Soroban gas budget tracking.** Add a workspace doc tracking per-entrypoint gas budgets; each gov contract claims a budget; CI fails on regression. The per-circuit verifier is the single largest cost in each contract — worth tracking explicitly.
