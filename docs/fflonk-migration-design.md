# fflonk + EF KZG migration — drop our own ceremony

**Date:** 2026-04-29
**Status:** Draft (Proposal — pre-implementation)
**Author:** Onym contributors
**Version:** 0.1 — initial design
**Supersedes:** (none)
**Related:**
- [`postmortem-ceremony-data-loss.md`](postmortem-ceremony-data-loss.md) — motivating incident; the operational argument for this migration
- [`group-governance-types-design.md`](group-governance-types-design.md) — parent design defining `groupType ∈ {Anarchy, OneOnOne, Democracy, Oligarchy, Tyranny}`; this design replaces the proving system underneath all five
- [`anarchy-update-testnet-design.md`](anarchy-update-testnet-design.md), [`democracy-update-testnet-design.md`](democracy-update-testnet-design.md), [`oligarchy-update-testnet-design.md`](oligarchy-update-testnet-design.md), [`oneonone-update-testnet-design.md`](oneonone-update-testnet-design.md) — sibling per-type designs; circuit, VK, and ceremony framing in this design supersedes Phase E ("Ceremony coordination") in each
- `contracts/sep-xxxx/src/lib.rs` — legacy monolithic contract; **decommissioned** by this migration

---

## 1. Background

The codebase ships a Groth16-based zk-SNARK stack on BLS12-381 with a per-circuit trusted-setup ceremony:

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

This rules out vanilla PLONK (≈9 pairings per verify) as the production verifier shape. fflonk-style proof aggregation collapses verify to **2 pairings** plus extra MSM, which is the realistic target.

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

### 4.1 Proving system: fflonk on BLS12-381

**Selection:** [fflonk](https://eprint.iacr.org/2021/1167) — a PLONK variant that aggregates the proof openings into 2 pairings via Kate-Zaverucha-Goldberg batched openings.

| Property | Groth16 (current) | fflonk (proposed) |
|---|---|---|
| Setup type | Per-circuit (phase 1 + phase 2) | **Universal** (phase 1 only) |
| SRS source | Self-run ceremony, 7 contributors | EF KZG, ≈140k contributors |
| Soroban verify | 1 MSM + 1 `pairing_check`(4 pairs) | 1 MSM + 1 `pairing_check`(2 pairs) |
| Field-op overhead | minimal | ≈10 hash ops + linearization arithmetic |
| Proof size, wire | 192 B | ≈700 B |
| Proof size, on-chain | 384 B | ≈900 B |
| Prover memory | small | small (n=2048 dominates) |
| Adding a new circuit | New ceremony required | Recompile only |

The verifier's pairing count drops from 4 to 2; the additional MSM and field arithmetic for linearization push net Soroban gas to roughly 1.3–1.7× current. Within the §3.1 budget.

**Implementation crate (preferred):** [`jellyfish-plonk`](https://github.com/EspressoSystems/jellyfish) — Espresso Systems' production-grade BLS12-381 PLONK in Rust. Maintained, audited, mainnet-deployed. Supports custom gates, Plookup, and turbo-PLONK style.

**Implementation crate (fallback):** `arkworks-rs/plonk` — older but interoperable with the rest of the arkworks stack we already use. Use this if the jellyfish API turns out to be a poor fit for the on-mobile prover targets.

**fflonk specifically:** Espresso's jellyfish supports the Kate batched-opening optimization that gives fflonk its 2-pairing verify. If the Rust ecosystem doesn't have a turn-key fflonk crate by the time we implement Phase B, vanilla PLONK is the fallback at the cost of ~2× Soroban verify gas vs. the fflonk target. **This is an open question — see §10.**

### 4.2 SRS: Ethereum Foundation KZG

**Source:** EF's 2023 KZG ceremony for EIP-4844 proto-danksharding, finalized 2023-11-14, with ≈141k contributors. Ceremony output: powers-of-tau on BLS12-381, 4096 G1 + 65 G2 elements. Public, signed, downloadable from `https://ceremony.ethereum.org`.

**Why this works:**
- BLS12-381 — same curve we use.
- 4096 G1 — covers our largest circuit (≤2048 PLONK rows under custom gates) with ~2× headroom.
- 65 G2 — fflonk verifier needs ~2 G2 elements. Plenty.
- ≈141k contributors — soundness reduces to "at least one of 141k was honest." Strictly stronger trust assumption than our 7-contributor ceremony.
- Public artifact — anyone can independently re-derive the SRS from the ceremony transcript. We don't host the SRS; we ship its hash and let the prover/verifier load it from a deterministic byte sequence at build time.

**Distribution:** SRS bytes (≈200 KB G1 + 6 KB G2) compiled into the prover crate as a `static` byte array. Mobile and server prover binaries embed the same SRS. Soroban verifier embeds only the verifier-key portion (a few G2 points + the Lagrange-form selector commitments per circuit, ≈1–2 KB per circuit).

### 4.3 Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                                                                 │
│   EF KZG SRS (BLS12-381, ≈200 KB)                               │
│   ─ embedded as `static` in `src/prover/srs.rs`                 │
│   ─ verified at build time against the published transcript     │
│                                                                 │
│           ┌──────────────────────────┐                          │
│           │ src/circuit/{mod,update, │                          │
│           │ democracy,oligarchy,…}.rs│                          │
│           │ — PLONK custom gates     │                          │
│           └────────────┬─────────────┘                          │
│                        │                                        │
│                        ▼                                        │
│           ┌──────────────────────────┐                          │
│           │ src/prover/preprocess()  │  produces                │
│           │ — per circuit, one-shot  │  (proving_key, vk)       │
│           └────────────┬─────────────┘                          │
│                        │                                        │
│       ┌────────────────┴───────────────┐                        │
│       │                                │                        │
│       ▼                                ▼                        │
│  proving_key.bin                  verifier_key.json             │
│  (mobile + server prover)         (embedded into each gov       │
│                                    contract at build time)      │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

**No coordinator. No participant CLI. No browser verifier. No retention rules. No phase-2.** A new circuit is added by writing the gadget and recompiling — the SRS is universal, the verifier-key is deterministic preprocessing, and the new VK is committed alongside the contract source.

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

This simplification is enabled by the universal SRS: if the verifier never changes between circuits, why pay storage for it? The answer in Groth16 was "to allow VK rotation as a kill switch." Under fflonk + universal SRS, deploying a new contract version achieves the same thing more cleanly, and groups continue using whichever contract version they were created against.

### 4.5 Mobile clients

**iOS:** `clients/ios/StellarChat/StellarChat/Resources/keyset-v2/` (12 files, ~5 MB) → replaced by:
- `Resources/srs.bin` — universal SRS (≈200 KB, shared across all circuits)
- `Resources/circuits/*.pk.bin` — per-circuit proving keys (smaller than current Groth16 pks because no α/β/δ encoded; preprocessed selectors only)
- VKs are in-contract bytecode, not bundled with the client

**Android:** `clients/android/StellarChat/app/src/main/assets/keyset-v2/` → analogous restructure under `assets/srs/` and `assets/circuits/`.

Net mobile bundle size delta: roughly **flat or smaller** (gain back current per-tier pk overhead; pay for universal SRS once).

**FFI surface (`src/ffi.rs` + `src/jni_ffi.rs`):**
- Drop `sep_generate_testing_proving_key`, `sep_generate_testing_update_proving_key`, `sep_generate_testing_democracy_update_proving_key` and their JNI wrappers — these existed because Groth16 needed per-circuit setup. fflonk needs none of this.
- Keep `sep_generate_membership_proof`, `sep_generate_update_proof`, `sep_generate_democracy_update_proof` etc. but their internal implementation calls `jellyfish::prove(&srs, &pk, &witness)` instead of `Groth16::<Bls12_381>::prove`.
- Proof serialization (`sep_proof_to_contract_format`) format changes from 384 B uncompressed Groth16 to ≈900 B uncompressed fflonk; client and contract are both updated together.

## 5. Alternatives Considered

| Alternative | Why not |
|---|---|
| **Stay on Groth16, just fix the prune + relay_loop bugs.** | Closes today's instances, leaves the architecture that produces them. Still need 9–11 phase-2 ceremonies for the gov contracts. Postmortem §"alternatives" rejects this. |
| **Stay on Groth16, run our own ceremony correctly this time.** | Same operational surface; same default-config and silent-error risk profile; doesn't address phase-2 multiplication for new circuits. |
| **Vanilla PLONK on BLS12-381 with EF KZG.** | Same SRS story, same operational benefit, but ~2.5× current verify gas vs. ~1.5× for fflonk. Acceptable fallback if fflonk's Rust+BLS12-381 ecosystem turns out not to support what we need. |
| **Halo2-IPA (transparent, no setup).** | Eliminates ceremony entirely but uses Pasta curves — Soroban has no Pasta host functions, so verify cost balloons by ~50–200×. Blocked by §3.2. |
| **Plonky3 / STARK.** | Eliminates ceremony, but small-field arithmetic + FRI commitments mean Soroban verify needs hundreds of KB of proof and field-op-heavy verify implemented in WASM. ~500–2000× current gas. Blocked by §3.2. |
| **Halo2-KZG (BLS12-381).** | Functionally equivalent to PLONK; same SRS story; comparable verify cost. Acceptable but no advantage over jellyfish-PLONK + fflonk. |
| **Aztec Ignition SRS instead of EF KZG.** | Both work. Aztec gives 2^21-degree headroom (vs. 4096 for EF), but our circuits don't need it. EF is the larger contributor pool and the more recent ceremony, so easier to argue trust. Aztec is the documented secondary source. |
| **Run our own KZG ceremony (small participant pool, but on universal SRS).** | Eliminates phase-2 friction but reintroduces the whole operational surface this design exists to remove. No. |

## 6. Implementation Plan — six phases

### Phase A — receipt salvage *(parallel, ≤1 week)*

Independent of the proving-system migration; recovers public auditability of the existing testnet ceremony record.

- A.1. Outreach to the 7 participant pubkeys with their per-round missing-receipt hash list (from `ceremony-backup/20260429T163349Z/MISSING.txt`). Ask for re-upload of `state.txt` and `receipt.txt` they still have locally.
- A.2. Re-publish recovered blobs to Blossom under PR #161's pinned-pubkey rule. Hashes match → `rounds.{state_txt_hash, receipt_hash}` rows are restored without coordinator changes.
- A.3. Document final recovery state in `docs/postmortem-ceremony-data-loss.md` "Blast radius" → "final".

Not blocking on B/C/D/E; not gated by F.

### Phase B — proving-system swap *(2–3 weeks)*

- B.1. Add `jellyfish-plonk` as workspace dep; remove `ark-groth16`, `ark-snark`. Keep `ark-bls12-381`, `ark-ff`, `ark-ec`, `ark-serialize` (jellyfish reuses them).
- B.2. Embed EF KZG SRS as `static` bytes in `src/prover/srs.rs`. Build-time check: SHA256 of the embedded blob matches the published EF transcript hash. Refuse to compile on mismatch.
- B.3. Port circuits:
  - `src/circuit/mod.rs` (788 LOC) → PLONK custom gates with Plookup tables for Poseidon constants.
  - `src/circuit/update.rs` (440 LOC) → same approach.
  - `src/circuit/democracy.rs` (851 LOC) → same approach. Quorum-binding logic stays identical; only the gate format changes.
  - Future per-type circuits (oligarchy, tyranny, oneonone) inherit the same skeleton.
- B.4. Replace `src/prover/mod.rs` (1,030 LOC) Groth16 prove/verify calls with jellyfish equivalents.
- B.5. Add `cargo test -p src` integration tests proving end-to-end for each circuit against EF SRS.
- B.6. Cross-platform test vector generation (`docs/cross-platform-test-vectors.json`) regenerated for the new proof format.

Exit criterion: `cargo test --workspace` green; round-trip prove → verify for all current circuits succeeds against the embedded SRS.

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

Exit criterion: each gov contract builds, has a passing testnet deployment with a real fflonk proof, and gas measurements within budget.

### Phase D — mobile rebundle *(1 week, parallel with C)*

- D.1. Replace `clients/ios/StellarChat/StellarChat/Resources/keyset-v2/` 12-file bundle with `Resources/srs.bin` + `Resources/circuits/*.pk.bin`.
- D.2. Same for `clients/android/StellarChat/app/src/main/assets/keyset-v2/`.
- D.3. Update FFI:
  - `src/ffi.rs:405-517` — drop `sep_generate_testing_*_proving_key` family.
  - `src/ffi.rs:345-408` — replace `sep_proof_to_contract_format` with the new fflonk encoding.
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
- **Phase C:** per gov contract, a Soroban-vm test that consumes a real fflonk proof produced by the Rust prover and asserts `verify_membership/update returns true`. Cost-budget assertions: `env.budget()` reports stay under 2× the pre-migration baseline.
- **Phase D:** on-device prove → testnet verify round trip on iOS (real iPhone, both A-series and M-series) and Android (low-end + mid-tier devices). Memory and time measurements logged for the rollout doc.
- **Phase E:** post-decommission smoke test — testnet group create, member add, member remove, group update — all running entirely on the new contracts, no coordinator service, no `keyset-v2` bundles.

## 8. Rollout (cross-phase coordination)

- **Branch hygiene:** Phase A is independent (no code changes; participant outreach + Blossom uploads only). Phase B can land on `main` behind a feature flag (`PROVING_SYSTEM=fflonk`) without breaking existing flows; default stays Groth16 until Phase C ships. Phase C is the cutover. Phases D/E/F are sequential cleanup.
- **Testnet first.** Each gov contract deploys to testnet under the new verifier and runs a 2-week soak before mainnet consideration. Mainnet itself is downstream of [`mainnet-deployment.md`](mainnet-deployment.md) and is **not** included in this design's scope.
- **Mobile flag day.** Old clients with Groth16 proving keys cannot generate proofs accepted by the new contracts and vice versa. Phase D coordinates app-store submission with Phase C contract deployment so users updating their app land on a contract version that accepts their proofs.
- **Frozen sep-xxxx.** Between Phase C cutover and Phase F decommission, `sep-xxxx` continues to verify Groth16 proofs from old clients. No new groups are created against it. `keyset-v2/` keeps working until that contract is taken down.

## 9. Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| **Rust fflonk implementation maturity.** No turn-key fflonk crate on BLS12-381 may exist; we end up with vanilla PLONK and ≈2.5× verify gas. | Medium | Prototype Phase B verifier on a stub circuit before committing to the migration. Vanilla PLONK is the documented fallback; verify cost is acceptable but not ideal. |
| **EF KZG SRS coverage edge case.** A future circuit needs n>2048 PLONK rows. | Low (current circuits well below 2048; PLONK custom gates compress vs. R1CS) | Switch SRS source to Aztec Ignition (n=2^21) without changing any other code. SRS source is one `static` blob. |
| **Soroban verify gas exceeds 2× budget.** | Low–medium | Phase C.3 fails the rollout. We fall back to optimizing the linearization step (precomputing more of it as static constants in the contract) before considering wider proving-system changes. |
| **Mobile prover memory regression.** | Low (n=2048 well within budget) | Memory measurement is part of Phase D's exit criterion. If exceeded, increase Plookup table reuse or split a circuit into sub-circuits. |
| **Migration takes longer than budgeted; testnet has two proving systems live for a quarter.** | Medium | Both systems ship behind the same client API surface; the proof type is wire-format-distinguishable so a contract can accept either during a transition window if absolutely necessary. We avoid this if at all possible. |
| **Audit gap: PLONK gadgets re-implemented from scratch are easy to get wrong.** | Medium | Use jellyfish's audited gadget library wherever possible. Each circuit gets an independent review. Cross-platform test vectors are the second line of defense. |

## 10. Open Questions

- **fflonk-specific Rust crate** — is `arkworks-fflonk` or `jellyfish::fflonk` production-ready on BLS12-381 today, or do we ship vanilla PLONK and revisit fflonk's 2-pairing optimization in a follow-up? Phase B.1 spike resolves this.
- **VK-as-bytecode vs. VK-as-storage** — we propose embedding VKs in contract bytecode (§4.4). This sacrifices the `update_vk` admin kill switch in exchange for simpler storage. Is the per-deploy version-rotation pattern enough? *(Tentative answer: yes — `sep-democracy` etc. already version their contracts; this aligns.)*
- **Phase A participant outreach scope** — do we attempt to recover all 22 sidecars, or accept the partial loss and move on? *(Tentative answer: best-effort outreach for two weeks, then close out.)*
- **Aztec Ignition vs. EF KZG** as primary SRS — do we hardcode EF and document Aztec as fallback, or vice versa? *(Tentative answer: EF — larger contributor pool, more recent ceremony, simpler trust argument.)*
- **Browser verifier replacement** — `crates/ceremony-wasm` was used by the ceremony page. With the page gone, do we ship any browser-side verifier, or is "verify on the server, use TLS" sufficient for the dropped-ceremony freeze announcement page? *(Tentative answer: drop entirely. Anyone who wants to re-verify can run the Rust prover/verifier locally.)*

## 11. Follow-Up Work

- **Per-circuit benchmark harness.** Once Phase B lands, codify the prover-time and verifier-gas measurements in a CI job. The migration thesis depends on a specific cost envelope; we should keep that envelope honest.
- **Circuit-level postmortem hygiene.** When the gov-type circuits ship (oligarchy v0.1.5, tyranny initial), each gets a circuit-specific test-vector file regenerated against the new proving system.
- **Move `keyset-democracy-dev` and `keyset-v1` out of the repo root.** Both directories become historical artifacts after Phase F. Archive to `docs/historical/` with a README pointing at the postmortem.
- **Soroban gas budget tracking.** Add a workspace doc tracking per-entrypoint gas budgets; each gov contract claims a budget; CI fails on regression. The per-circuit verifier is the single largest cost in each contract — worth tracking explicitly.
