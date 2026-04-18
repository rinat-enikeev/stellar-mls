# Keyset Generation Guide

This document describes how to produce a complete `keysets/keyset-v<N>/` directory containing the verification keys (VKs) and proving keys (PKs) for every Groth16 circuit this project ships:

- **Membership circuit (`R_Membership`)** — `{small, medium, large}` tiers (Merkle depths 5 / 8 / 11)
- **Update circuit (`R_Update`)** — `{small, medium, large}` tiers (#59 fix; keyset v2 onwards)

→ 3 membership + 3 update = **6 Groth16 circuits per keyset**.

Two production modes are supported:

1. **Single trusted party** — one operator runs the whole setup with `OsRng`. Fast, reproducible locally, acceptable for testnet and dev. Security depends on **one person not being compromised and correctly destroying toxic waste**. Do not use for mainnet.
2. **Multi-party ceremony (MPC)** — Phase 1 (Powers-of-Tau) + Phase 2 (circuit-specific zkey) with `N ≥ 10` independent participants. Required for mainnet. Security holds as long as **at least one honest participant** exists and destroys their contribution randomness.

Both modes produce the same on-disk layout at the end, so the rest of the release pipeline (`scripts/deploy-mainnet.sh`, mobile asset install, `docs/breaking-changes-release-process.md`) is identical.

---

## 1. Target on-disk layout

Whichever mode you pick, the final `keyset-v<N>/` must contain exactly this layout (v2+ includes UpdateCircuit files):

```text
keyset-v<N>/
├── small/
│   ├── proving_key.bin                 # Membership PK (arkworks canonical)
│   ├── verifying_key.bin               # Membership VK (arkworks canonical)
│   ├── update_proving_key.bin          # UpdateCircuit PK (v2+)
│   └── update_verifying_key.bin        # UpdateCircuit VK (v2+)
├── medium/                             # depth=8 — same 4 files
├── large/                              # depth=11 — same 4 files
├── vk-small.json                       # Contract-ready membership VK
├── vk-medium.json
├── vk-large.json
├── vk-update-small.json                # Contract-ready UpdateCircuit VK (v2+)
├── vk-update-medium.json
├── vk-update-large.json
└── metadata.json                       # {version, sha256 of every .bin, tier info, circuit ids}
```

The `.bin` files are used by `swift-mls` / `kotlin-mls` clients (shipped as mobile assets). The `vk-*.json` files are consumed by the Soroban deploy script (`scripts/deploy-mainnet.sh`) which loads them into instance storage under `DataKey::VK(tier)` and `DataKey::UpdateVK(tier)`.

Tier constants (must match `src/lib.rs` `TIERS`):

| Tier | Merkle depth | Max members | Circuit constraints (approx) |
|---|---|---|---|
| `small` | 5 | 32 | ~27,500 |
| `medium` | 8 | 256 | ~28,400 |
| `large` | 11 | 2,048 | ~29,300 |

---

## 2. Single trusted party (testnet / dev only)

### Prerequisites

- Rust toolchain (stable)
- Clean machine — ideally an air‑gapped laptop whose disk is wiped after the run
- ≥ 8 GB RAM (large tier proving key is the constraint)

### Run

```bash
./scripts/generate-keyset.sh --version 2 --out-dir keyset-v2
```

This calls `cargo run --release --bin generate_keyset -- --out-dir keyset-v2 --version 2`, which:

1. Draws randomness from `OsRng` (single source — this is the trust assumption)
2. Runs Groth16 `generate_random_parameters` for all 6 circuits
3. Writes the `.bin` files, extracts the contract-ready JSON VKs, emits `metadata.json` with SHA‑256 hashes

### Destroy toxic waste

The "toxic waste" for a single-party setup is the internal τ, α, β, γ, δ scalars used during parameter generation. Arkworks holds them only in process memory and drops them when the binary exits. To minimise risk:

- [ ] Run on an air‑gapped machine
- [ ] Reboot (or `shutdown -h now`) immediately after the run — clears RAM
- [ ] If run on a VM: discard the VM and its swap volume afterwards
- [ ] Do not enable swap / paging (`swapoff -a` before running on Linux)
- [ ] `sha256sum` every `.bin` and publish the hashes in the release notes so downstream parties can verify they are using the shipped keyset and not a silent substitution

### What you must publish

- `keyset-v<N>.tar.zst` (or similar) of the whole directory
- SHA‑256 of the tarball, signed with your release key
- `metadata.json` contents in the release notes

That's it for mode 1. Proceed to §5 for install.

---

## 3. Multi-party ceremony (mainnet-grade)

The MPC has two separable phases. Treat them as two independent ceremonies — Phase 1 produces a *universal* SRS (Powers-of-Tau) that any circuit up to the chosen size can reuse; Phase 2 is *circuit-specific* (one transcript per circuit) and converts the universal SRS into proving/verifying keys.

```
Phase 1 (Powers-of-Tau)               Phase 2 (circuit-specific zkey)
┌───────────────────────┐             ┌─────────────────────────────────┐
│ coordinator-init      │             │ membership-small  (6 ceremonies │
│     ↓                 │             │ membership-medium   run in      │
│ participant 1 …       │  →  SRS  →  │ membership-large    parallel    │
│     ↓                 │             │ update-small        once phase  │
│ participant N         │             │ update-medium       1 is done)  │
│     ↓                 │             │ update-large                    │
│ final Phase 1 SRS     │             └─────────────────────────────────┘
└───────────────────────┘
```

### 3.1 Roles

| Role | Count | Responsibilities |
|---|---|---|
| **Coordinator** | 1 | Runs `init`, verifies every contribution, maintains archive, publishes trail |
| **Participant** | ≥ 10 per phase | Receives latest state, contributes their entropy, returns state + `contribution_id` |
| **Verifier** | Anyone | Runs the public verification after the ceremony is published |

The coordinator does **not** need to be trusted with the keyset's security (as long as ≥ 1 participant is honest). The coordinator is trusted only for logistics (not stealing entropy, not reordering, not publishing a wrong final state).

### 3.2 Phase 1 — Powers-of-Tau (SRS)

Infrastructure: `src/bin/ceremony_tool.rs`, wrapped by `tools/ceremony/run.sh`.

Existing playbooks (authoritative — follow them for the exact commands):

- `docs/trusted-setup-ceremony-phase1-coordinator-playbook.md` — coordinator script
- `docs/trusted-setup-ceremony-phase1-participant-runbook.md` — participant script
- `docs/trusted-setup-ceremony-phase1-start.md` — how to kick off publicly
- `docs/trusted-setup-ceremony-call-for-participants.md` — template for recruiting

Outline of operations (one run *per tier* — the SRS size is tier‑dependent):

Coordinator, once per tier:
```bash
bash tools/ceremony/run.sh init \
    --tier <small|medium|large> \
    --out-dir ceremony/<tier>/round-000 \
    --participant coordinator-init
bash tools/ceremony/run.sh verify-state --state-dir ceremony/<tier>/round-000
```

Each participant k:
```bash
# receives ceremony/<tier>/round-<k-1>/
bash tools/ceremony/run.sh contribute \
    --state-dir ceremony/<tier>/round-<k-1> \
    --out-dir ceremony/<tier>/round-<k> \
    --participant <handle>
# sends round-<k>/ + contribution_id back to coordinator
```

Coordinator after each round:
```bash
bash tools/ceremony/run.sh verify-contribution \
    --before-state-dir ceremony/<tier>/round-<k-1> \
    --after-state-dir ceremony/<tier>/round-<k>
bash tools/ceremony/run.sh verify-state --state-dir ceremony/<tier>/round-<k>
```

Finalise (per tier):
```bash
bash tools/ceremony/run.sh phase2-summary \
    --state-dir ceremony/<tier>/round-N \
    --out-file ceremony/<tier>/phase2-summary.txt
```

`phase2-summary.txt` is the public handoff to Phase 2.

### 3.3 Phase 2 — per-circuit zkey

Per-tier Phase 2 must be run **once per circuit** — so for keyset v2+ that is **6 Phase-2 ceremonies in total** (3 membership × 3 update). They may run in parallel and may have overlapping but not necessarily identical participant sets.

Phase 2 is performed with `snarkjs` against the Phase 1 SRS. The interop layer is `src/ceremony/phase2.rs`:

- `phase2::export_srs(...)` — writes the final Phase 1 state in a format snarkjs can read
- `phase2::import_phase2_keys(...)` — reads snarkjs `proving_key.bin` + `verifying_key.bin` back into arkworks types

The `snarkjs` pipeline (from `docs/phase2-mpc-integration.md`):

```bash
# One-time per circuit — coordinator
snarkjs groth16 setup circuit.r1cs phase1.ptau phase2_0000.zkey

# Each participant adds entropy
snarkjs zkey contribute phase2_0000.zkey phase2_0001.zkey --name="Participant 1"
snarkjs zkey contribute phase2_0001.zkey phase2_0002.zkey --name="Participant 2"
...

# Apply a public beacon (e.g. a recent Bitcoin block hash) to eliminate
# the "last contributor deletes correlated entropy" attack
snarkjs zkey beacon phase2_final.zkey phase2_beacon.zkey <beacon_hash> 10

# Export artefacts
snarkjs zkey export verificationkey phase2_beacon.zkey verification_key.json
snarkjs zkey verify circuit.r1cs phase1.ptau phase2_beacon.zkey
```

Participant playbook (what each human runs): `docs/trusted-setup-ceremony-phase2-participant-playbook.md`.

Output per circuit (6 times in total for v2+):

```text
phase2/<circuit-name>/
├── proving_key.bin       # produced by snarkjs → arkworks conversion
├── verifying_key.bin
├── verification_key.json # snarkjs native; kept for public audit
├── transcript/           # every intermediate zkey + participant signatures
└── beacon.txt            # the public randomness beacon (block hash + depth)
```

Circuit-name convention:

```
membership-small   membership-medium   membership-large
update-small       update-medium       update-large
```

### 3.4 Assemble the final keyset directory

After all 6 Phase-2 ceremonies are complete and every transcript has been verified:

```bash
# Start from an empty dir
rm -rf keyset-v2 && mkdir keyset-v2

for tier in small medium large; do
    mkdir -p keyset-v2/$tier
    cp phase2/membership-$tier/proving_key.bin   keyset-v2/$tier/proving_key.bin
    cp phase2/membership-$tier/verifying_key.bin keyset-v2/$tier/verifying_key.bin
    cp phase2/update-$tier/proving_key.bin       keyset-v2/$tier/update_proving_key.bin
    cp phase2/update-$tier/verifying_key.bin     keyset-v2/$tier/update_verifying_key.bin
done

# Produce contract-ready VK JSONs from the .bin files
cargo run --release --bin generate_mainnet_vks -- \
    --keyset-dir keyset-v2 \
    --out-dir keyset-v2

# Emit metadata.json with SHA-256 over every .bin and the v2 label
cargo run --release --bin generate_keyset -- \
    --metadata-only \
    --out-dir keyset-v2 \
    --version 2
```

Verify the structural layout matches §1 exactly, then seal and publish.

### 3.5 What must be published for public verification

- Every Phase 1 `round-*` directory for every tier (tarballed)
- Every Phase 2 transcript directory for every circuit
- The beacon value used in each Phase 2 and a public citation (block height, timestamp)
- `metadata.json` with SHA‑256 of each `.bin`
- A signed release manifest
- At least one independent third-party reproduction of the verification steps, posted publicly

---

## 4. Independent verification (anyone can run)

After publication, a third party runs:

```bash
# Phase 1
for round in ceremony/<tier>/round-*; do
    bash tools/ceremony/run.sh verify-state --state-dir "$round"
done

# Phase 2 (per circuit)
snarkjs zkey verify circuit.r1cs phase1.ptau phase2_beacon.zkey

# Keyset hashes match
sha256sum -c keyset-v<N>/metadata.json.sha256sums
```

A keyset that any independent verifier cannot reproduce from the published transcripts **must not be used for mainnet**.

---

## 5. Install the keyset into the clients

Regardless of mode, after `keyset-v<N>/` is sealed:

```bash
# Android — copy the 6 PKs as mobile assets
mkdir -p clients/android/StellarChat/app/src/main/assets/keyset-v<N>
for tier in small medium large; do
    cp keyset-v<N>/$tier/proving_key.bin \
       clients/android/StellarChat/app/src/main/assets/keyset-v<N>/$tier.bin
    cp keyset-v<N>/$tier/update_proving_key.bin \
       clients/android/StellarChat/app/src/main/assets/keyset-v<N>/update-$tier.bin
done

# iOS — copy the same 6 PKs into the bundled Resources folder
mkdir -p clients/ios/StellarChat/StellarChat/Resources/keyset-v<N>
for tier in small medium large; do
    cp keyset-v<N>/$tier/proving_key.bin \
       clients/ios/StellarChat/StellarChat/Resources/keyset-v<N>/$tier.bin
    cp keyset-v<N>/$tier/update_proving_key.bin \
       clients/ios/StellarChat/StellarChat/Resources/keyset-v<N>/update-$tier.bin
done

# Update the bundled resources folder path in clients/ios/StellarChat/project.yml
# (the `sources:` entry that currently points at keyset-v1)

# Deploy the contract with the VK JSONs
IDENTITY=my-deployer VK_DIR=keyset-v<N> ./scripts/deploy-mainnet.sh
```

Then follow `docs/breaking-changes-release-process.md` for the rest of the app release.

---

## 6. Security checklist (both modes)

- [ ] Every `.bin` has a published SHA‑256 hash
- [ ] Every VK JSON has been structurally checked (correct IC-point count: membership=3, update=4)
- [ ] No `--seed N` flag was used anywhere — `generate_mainnet_vks --seed` is test-only and prints a warning; mainnet uses `--keyset-dir`
- [ ] The keyset version is a monotonically increasing integer; never reuse a version number
- [ ] Toxic waste destruction procedure was executed and attested to (single-party) **or** ≥ 1 participant attests publicly they destroyed their entropy (MPC)
- [ ] The deployed contract reads VKs from `DataKey::VK(tier)` and `DataKey::UpdateVK(tier)` — confirm both are populated by running `verify_membership` and `update_commitment` against a freshly-created group before any clients are told about the new contract ID

## 7. What a keyset generation does *not* include

- Contract deployment (`scripts/deploy-mainnet.sh` reads this keyset)
- Client version bumps or app releases (`docs/breaking-changes-release-process.md`)
- Relayer configuration
- RPC endpoint selection

Keyset generation produces *artefacts*; the release checklist consumes them.
