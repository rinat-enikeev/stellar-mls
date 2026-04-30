# Mainnet Deployment Guide

Deploy the five per-type Onym private group membership contracts (`sep-anarchy`, `sep-democracy`, `sep-oligarchy`, `sep-oneonone`, `sep-tyranny`) to Stellar mainnet, run a fee-paying relayer, and connect the iOS/Android chat apps — all from a single funded account.

This guide reflects the post-migration architecture: fflonk on BLS12-381 consuming Ethereum Foundation's 2023 KZG SRS, with verifier keys compiled into contract bytecode. There is no project-run trusted-setup ceremony, no keyset directory to generate, no on-chain VK rotation, and no `sep-xxxx` monolithic contract. See [`fflonk-migration-design.md`](fflonk-migration-design.md), [`postmortem-ceremony-data-loss.md`](postmortem-ceremony-data-loss.md), and [`group-governance-types-design.md`](group-governance-types-design.md) for the architectural background.

## Prerequisites

- **Rust 1.75+** with `cargo`
- **`stellar` CLI** ([install](https://developers.stellar.org/docs/tools/developer-tools/cli/install-cli))
- **A funded Stellar mainnet account** (recommended 80 XLM for the full five-contract deployment, less if deploying a subset of governance types)
- **A second funded account** for the relayer (recommended 100 XLM for ongoing fee payments)

## Architecture overview

Five contracts, one per governance type. Each contract:

- Embeds its verifier key(s) as compile-time `&[u8]` constants. The VK is the deterministic preprocessing of `(circuit, EF_KZG_SRS)`; reproducible bit-for-bit on any clean build of the same workspace revision.
- Calls `bls12_381::pairing_check` over **2 pairs** per proof verification (fflonk-aggregated openings) plus one G1-MSM (~20 scalars) and SHA-256-based Fiat-Shamir transcript.
- Has no `update_vk` admin entrypoint and no `DataKey::VK(_)` storage. VK rotation is by deploying a new contract version, not by amending an existing one.

Constructors take only `admin`. `restricted_mode` is instance-stored and toggled post-deployment via `set_restricted_mode(bool)` (defaults to `false`). There is no per-tier VK initialization step.

| Contract | Created via | Mutates state? | Tiers | Notes |
|---|---|---|---|---|
| `sep-anarchy` | `create_group` | `update_commitment` (any single member) | small/medium/large | No quorum, no admin, no voting |
| `sep-democracy` | `create_group` | `update_commitment` (in-circuit quorum check) | small/medium/large | Threshold-bound updates |
| `sep-oligarchy` | `create_oligarchy_group` (verbose) | `update_commitment` (admin-tree co-signing) | small/medium/large | Admin tree + member tree |
| `sep-oneonone` | `create_group` | (immutable post-creation) | 1v1 only | No `update_commitment` entrypoint |
| `sep-tyranny` | `create_group` | `update_commitment` (admin-only) | small/medium/large | Single-admin transitions |

You do not need to deploy all five contracts. Each is self-contained; deploy only the governance types you ship to users.

## Cost Estimates

| Operation | Approximate Cost |
|---|---|
| Contract deployment per type (WASM upload + constructor) | ~12 XLM |
| Each `create_group` / `create_oligarchy_group` | ~0.013 XLM |
| Each `update_commitment` | ~0.007 XLM |
| `verify_membership` (read-only) | Free |
| `get_commitment` / `get_history` (read-only) | Free |
| **Deployer recommended, full suite (5 contracts)** | **~80 XLM** (5 × ~12 XLM deployment + ~20 XLM buffer for retries / initial smoke tests) |
| **Deployer recommended, single contract** | **~20 XLM** (~12 XLM deployment + buffer) |
| **Relayer recommended** | **~100 XLM** |

<!-- TODO: verify post-Phase-C — fflonk-vs-Groth16 ratio is provisional until gas benchmarks land -->
The fflonk verifier costs roughly 1.3–1.7× the legacy Groth16 verifier on Soroban (2-pair `pairing_check` + extra MSM scalars vs. Groth16's 4-pair `pairing_check`). Update-circuit costs scale similarly. Pre-deployment gas measurements are captured per contract under each crate's `tests/gas_benchmark.rs`.

---

## Step 1: Deploy the Contract Suite

### 1.1 Import your Stellar account

If you already have a funded mainnet account with a secret key (S...):

```bash
stellar keys import my-deployer --secret-key
# Paste your S... key when prompted
```

Or for testnet dry-run:

```bash
stellar keys generate my-deployer --network testnet
stellar keys fund my-deployer --network testnet
```

### 1.2 Run the deployment script

The deployment script no longer reads or generates a `keyset/` directory. Verifier keys are compiled into the contract WASM at build time; the build is deterministic, so the same workspace revision produces the same WASM bytes (and therefore the same contract behavior) on any machine.

Deploy a single governance type:

```bash
# Mainnet, just sep-anarchy
IDENTITY=my-deployer ./scripts/deploy-mainnet.sh --type anarchy

# Testnet dry-run (recommended before mainnet)
IDENTITY=my-deployer ./scripts/deploy-mainnet.sh --type anarchy --network testnet
```

Deploy the full suite:

```bash
IDENTITY=my-deployer ./scripts/deploy-mainnet.sh --all
```

The script will, for each type:

1. Build the Soroban contract WASM (deterministic; verifier-key bytes embedded via `build.rs`). The build itself fails if the embedded SRS hash doesn't match the EF KZG ceremony's pinned hash — see Appendix. The deploy script does not re-verify the SRS at deploy time; build-time integrity is the load-bearing check.
2. Deploy the contract.
3. Invoke `__constructor(admin)` with the configured admin address.
4. Verify the contract is live by calling a read-only entrypoint.
5. Print the contract ID and append it to `deploy/contracts.json` for the relayer + mobile-app config steps.

### 1.3 Save the contract IDs

The script outputs one block per deployed contract:

```
════════════════════════════════════════════════════════════════
  sep-anarchy deployed
════════════════════════════════════════════════════════════════

  Contract ID:      CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
  Admin address:    GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
  Network:          mainnet
  WASM hash:        <sha256>
  VK fingerprint:   <sha256 of compiled-in vk bytes>
```

Save the **Contract ID** for each type — you'll need them for the relayer and app configuration. The `VK fingerprint` is logged so you can prove on-chain VK provenance against `docs/cross-platform-test-vectors.json`.

### 1.4 Script options

<!-- TODO: verify post-Phase-C — script flag set is provisional until deploy-mainnet.sh is rewritten in implementation PR -->

| Option | Description |
|---|---|
| `--identity <name>` | Stellar CLI identity to use (alternative to `IDENTITY` env var) |
| `--network <name>` | `mainnet` (default) or `testnet` |
| `--type <name>` | Deploy a single governance type: `anarchy`, `democracy`, `oligarchy`, `oneonone`, `tyranny` |
| `--all` | Deploy all five types (uses `--type` for each, in order) |
| `--admin <G...>` | Override the admin address (default: the deployer identity) |
| `--keep-artifacts` | Preserve generated WASM artifacts after deployment |

### 1.5 What is no longer required

The pre-migration deployment had a "Generate a keyset" sub-step and `KEYSET_DIR` / `VK_SEED` environment variables. **None of that exists post-migration.** If you find references to `scripts/generate-keyset.sh`, `keyset-v1/`, `keyset-v2/`, `seed=42`, or `install-*-vks-*.sh` in older notes, treat them as historical context only — they refer to the legacy Groth16 ceremony surface that was decommissioned per [`fflonk-migration-design.md`](fflonk-migration-design.md) Phase E.

---

## Step 2: Deploy the Relayer

The relayer is an HTTP service that receives contract invocation requests from mobile apps, signs them with its own Stellar account, and submits to the network. Chat participants don't need funded Stellar accounts.

A single relayer can route to all five per-type contracts. The relayer's contract-ID whitelist becomes a list rather than a single value.

### 2.1 Fund a relayer account

Create a separate Stellar account for the relayer. Do **not** reuse the deployer/admin account.

```bash
# For mainnet: fund via an exchange or another account
# For testnet:
stellar keys generate my-relayer --network testnet
stellar keys fund my-relayer --network testnet
```

### 2.2 Configure

```bash
cd relayer
cp .env.example .env
```

Edit `.env`:

```bash
# Relayer account secret key (REQUIRED)
RELAYER_SECRET_KEY=SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX

# Comma-separated list of allowed contract IDs (REQUIRED).
# One entry per deployed governance type. The relayer rejects any
# request whose contractID is not in this list.
RELAYER_CONTRACT_IDS=CAAA...anarchy,CBBB...democracy,CCCC...oligarchy,CDDD...oneonone,CEEE...tyranny

# RPC endpoint
RELAYER_RPC_URL=https://soroban.stellar.org

# Network
RELAYER_NETWORK=mainnet

# Listen address
RELAYER_BIND=0.0.0.0:8080

# Optional: comma-separated bearer tokens
RELAYER_AUTH_TOKENS=my-secret-token-1,my-secret-token-2

# Rate limit per IP
RELAYER_RATE_LIMIT=30
```

### 2.3 Run the relayer

```bash
cd relayer
sh ./run.sh --release

# Or build and run
cargo build --release
./target/release/onym-relayer
```

Expected startup log (only the contracts in `RELAYER_CONTRACT_IDS` appear; partial deployments show fewer entries):

```
Relayer address: GXXXXXXX...
Allowed contracts:
  sep-anarchy:   CAAA...
  sep-democracy: CBBB...
  sep-oligarchy: CCCC...
  sep-oneonone:  CDDD...
  sep-tyranny:   CEEE...
Network:         mainnet
Auth required:   true
Rate limit:      30 req/min per IP
Listening on 0.0.0.0:8080
```

### 2.4 Run with Docker

```bash
cd relayer
docker build -t onym-relayer .
docker run -d \
  --env-file .env \
  -p 8080:8080 \
  --name onym-relayer \
  onym-relayer
```

### 2.5 Verify the relayer

```bash
# Use any of the deployed contract IDs. A get_commitment for a
# nonexistent group returns "GroupNotFound" — confirms the relayer
# reached the contract.
curl -s -X POST http://localhost:8080 \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer my-secret-token-1" \
  -d '{
    "contractID": "CAAA...",
    "function": "get_commitment",
    "payload": {
      "groupID": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    }
  }'
```

Expected: `GroupNotFound`. Repeat per deployed contract ID to verify each is reachable.

### 2.6 Production deployment

Deploy the relayer behind HTTPS:

- Use a reverse proxy (nginx, Caddy) with TLS termination.
- Point your domain (e.g., `https://relayer.example.com`) to the relayer.
- The mobile apps support TLS certificate pinning for additional security.

### 2.7 Relayer security

The relayer validates every request:

| Check | Description |
|---|---|
| Contract ID whitelist | The `contractID` must appear in `RELAYER_CONTRACT_IDS` |
| Function whitelist (per contract type) | <!-- TODO: verify post-Phase-C — exact per-type function set is provisional until contracts land --> sep-anarchy / sep-democracy / sep-tyranny: `create_group`, `update_commitment`, `verify_membership`, `get_commitment`, `get_history`. sep-oligarchy: `create_oligarchy_group` instead of `create_group`. sep-oneonone: no `update_commitment`. |
| Proof size | fflonk wire format: `0x01` version prefix + ≈900 bytes uncompressed. The relayer rejects any payload whose `proof` field doesn't decode to a valid `PlonkProof` of the expected size for that contract+function. |
| Payload size | Rejected if > 16 KB |
| Rate limiting | Per IP address (configurable) |
| Bearer auth | Optional, recommended for production |

The proof-size check upgraded from Groth16's fixed 384 bytes to fflonk's ~900-byte versioned format. The version-prefix byte makes it possible to add a future proof-system swap without breaking the wire format check — the relayer verifies the prefix corresponds to a known proving system before counting bytes.

---

## Step 3: Configure Mobile Apps

The mobile apps no longer ship per-tier proving keys generated from a project-run ceremony. They ship a single universal SRS bundle (≈206 KB, identical bytes across iOS/Android/server) plus per-circuit preprocessed prover keys (≈300 KB each). All proving keys are bundled at build time, not deploy time.

### iOS

1. Open **StellarChat** → **Settings**
2. In the **Stellar Contracts** section:
   - **Endpoint URL**: `https://soroban.stellar.org` (or your relayer URL)
   - **Anarchy contract ID**: paste the contract ID from Step 1.3
   - **Democracy contract ID**: same
   - **Oligarchy contract ID**: same
   - **OneOnOne contract ID**: same
   - **Tyranny contract ID**: same
   (Leave blank any types you didn't deploy.)
3. If using the relayer, in the **Relayer** section:
   - **Relayer URL**: `https://relayer.example.com`
   - **Auth Token**: your bearer token (if configured)
4. Tap **Save**

### Android

1. Open **StellarChat** → **Settings**
2. In the **Stellar Contracts** section, configure the same five contract IDs as iOS.
3. If using the relayer:
   - **Relayer URL**: `https://relayer.example.com`
   - **Auth Token**: your bearer token
4. Tap **Save**

### How it works

When you create a group in the app:

1. The user picks a governance type (Anarchy / OneOnOne / Democracy / Oligarchy / Tyranny). The app routes the create call to that type's contract ID.
2. The app generates a fflonk membership proof using the bundled prover key for the chosen circuit + tier and the bundled universal SRS (no per-circuit setup at runtime — the prover key is the deterministic preprocessing of `(circuit, srs)`, baked at build time).
3. The app sends the proof + group data to the relayer as JSON.
4. The relayer wraps the request in a Stellar transaction, signs with its own account, and submits.
5. The contract verifies the proof against its compiled-in VK; on success, stores the group's commitment.
6. The app shows "Verified on-chain" status.

Subsequent state transitions use the contract's update path (which differs by type — see "Architecture overview" above).

The relayer's Stellar account pays the transaction fee. Chat participants never need their own funded Stellar accounts.

---

## Step 4: End-to-End Verification

### 4.1 Create a group

1. Open the app on one device.
2. Tap **+** → **Create Group**.
3. Pick a governance type (e.g., "Anarchy") and enter a name → **Create**.
4. The app generates a fflonk proof, sends it to the relayer, and waits for the contract acceptance.
5. Check the group list — it should show epoch 0 and the configured tier.

Repeat with each governance type you've deployed.

### 4.2 Verify on-chain

In the app, the group should display a "Verified" badge indicating its commitment matches the on-chain state at the corresponding contract.

You can also verify manually (replace `CAAA...` with the contract ID for that group's type):

```bash
stellar contract invoke \
  --network mainnet \
  --id CAAA... \
  --source-account my-deployer \
  --send no \
  -- get_commitment \
  --group-id GROUP_ID_HEX
```

### 4.3 Cross-platform test

1. Create a group on iOS (any type).
2. Share the invite code with an Android device.
3. Join on Android.
4. Both devices should show the same group with matching commitment and epoch.
5. Trigger a state change appropriate for the group type:
   - **Anarchy**: any member updates commitment → both devices reflect epoch+1.
   - **Democracy**: members vote, threshold met, quorum-bound update lands.
   - **Oligarchy**: admin co-signs an update.
   - **OneOnOne**: no update path; verify both endpoints see the same commitment.
   - **Tyranny**: admin updates commitment unilaterally.
6. Send messages — they should flow bidirectionally via Nostr relays.

### 4.4 Cross-platform test vectors

`docs/cross-platform-test-vectors.json` pins (public-inputs, proof, VK fingerprint) tuples for each circuit. The proofs are generated from fixed synthetic test witnesses checked into the test harness — they are not real user secrets. To assert a fresh build agrees with the canonical reference:

```bash
cargo run --bin verify-test-vectors --release
swift run swiftmls-verify-vectors docs/cross-platform-test-vectors.json
./gradlew :kotlin-mls:run --args="verify-vectors docs/cross-platform-test-vectors.json"
```

All three platforms must report all entries verifying. A platform reporting a verification failure indicates a build divergence — most likely a stale SRS bundle or out-of-tree circuit edit.

---

## Troubleshooting

| Error | Cause | Fix |
|---|---|---|
| `cargo build` fails with "SRS hash mismatch — refusing to build" | The embedded SRS bundle (`src/prover/srs/ef-kzg-2023.bin`) doesn't match the expected EF KZG hash | Re-fetch the SRS from `https://ceremony.ethereum.org` and verify its hash. Do not edit the bundled file by hand. |
| `InvalidProof` from the contract | Client and contract were built from divergent revisions | Confirm both built from the same workspace revision; the deterministic VK fingerprint in the deploy log should match the VK fingerprint the client sees in `docs/cross-platform-test-vectors.json`. |
| `NotInitialized` | Constructor was not called or admin was not set | Re-run `deploy-mainnet.sh --type <name>` for that contract; it invokes the constructor as part of the script. |
| `GroupAlreadyExists` | Duplicate group creation | Normal if retrying — the group already exists on-chain |
| `AdminOnly` | Restricted mode is enabled and the caller is not admin | Admin can disable: `set_restricted_mode(false)` |
| Relayer returns 401 | Invalid bearer token | Check `RELAYER_AUTH_TOKENS` in `.env` |
| Relayer returns 429 | Rate limited | Wait or increase `RELAYER_RATE_LIMIT` |
| Relayer returns 400 with "unknown contract" | Contract ID not in `RELAYER_CONTRACT_IDS` | Add the contract ID to the relayer config |
| Relayer returns 400 with "proof size mismatch" | Proof is not a valid versioned `PlonkProof` | Confirm the client and relayer agree on proof version (current: `0x01`); mismatch usually indicates a stale client build |
| Relayer returns 502 | RPC invocation failed | Check that the relayer account is funded and the Soroban RPC endpoint is reachable |
| App shows "Not configured" for a governance type | The contract ID for that type is empty in Settings | Enter the contract ID in Settings, or accept that this governance type is unavailable |
| App shows verification failure | Local state diverged from on-chain | Re-verify; if persistent, the local cache may be stale — clear and re-fetch |

---

## Security Notes

### Account separation

- **Deployer account** — becomes the contract admin. Store its secret key securely offline after deployment. Only needed for `set_restricted_mode` toggles or future contract upgrades. There is no `update_vk` admin action — VKs are bytecode-baked and rotate by deploying a new contract version.
- **Relayer account** — used for ongoing operations. Keep it funded. If compromised, an attacker can only create/modify groups (still requires valid fflonk proofs from chat participants) and spend the relayer's XLM.

### Proving-system trust model

- **Universal SRS:** the project does not run its own trusted-setup ceremony. The fflonk SRS is the public output of Ethereum Foundation's 2023 KZG ceremony (≈141k contributors, finalised 2023-11-14). Soundness reduces to "at least one of ≈141k EF ceremony participants was honest and erased their contribution scalar" — a strictly stronger trust assumption than any small-N project-run ceremony.
- **Verifier key provenance:** each contract's compile-time VK is the deterministic preprocessing of `(circuit, EF_KZG_SRS)`. Two clean builds of the same workspace revision produce byte-identical WASM and byte-identical VK bytes. The deploy script logs the VK fingerprint; auditors can reproduce by checking out the same revision and rebuilding.
- **No on-chain VK updates:** the `update_vk` admin entrypoint that existed in the legacy `sep-xxxx` contract was removed in [`fflonk-migration-design.md`](fflonk-migration-design.md) Phase C. Rotating a VK requires deploying a new contract version; existing groups continue to use the contract version they were created against until they migrate.
- **Prover-key lifecycle:** the mobile apps ship the universal SRS plus per-circuit preprocessed prover keys at build time. Both are reproducible from the public EF KZG transcript; no per-tier randomness, no per-deployment seed, no `KEYSET_VERSION` to bump on rotation. App release cadence and contract release cadence are independent.

### Privacy model

- **On-chain:** each contract stores opaque 32-byte commitments, history slots, and used-proof nullifiers. Member identities, keys, lists, and counts are not on-chain. The relayer's address appears as the transaction signer (not the group member's).
- **Nostr:** messages are AES-256-GCM encrypted; relays see ciphertext and topic tags only.
- **Relayer:** sees the JSON payloads (proofs, commitments) but not member identities. For maximum privacy, access the relayer over Tor/VPN.
- **Governance-type leakage:** the choice of contract address reveals the group's governance type. If hiding the governance type is in your threat model, route all contracts through a single front-door (e.g., a dispatcher contract that branches on payload) — out of scope for this guide.

---

## Appendix: Universal SRS provenance

The fflonk verifier consumes a universal SRS — a sequence of powers of τ on BLS12-381, of size 4096 G1 + 65 G2 elements (≈206 KB in arkworks-compressed encoding; uncompressed is ≈396 KB). This SRS is the public output of **Ethereum Foundation's 2023 KZG ceremony** for EIP-4844, finalised 2023-11-14 with ≈141k contributors.

| Property | Value |
|---|---|
| Curve | BLS12-381 |
| G1 elements | 4,096 (covers PLONK row counts up to 2,048 with comfortable headroom) |
| G2 elements | 65 (sufficient for the fflonk verifier's 2-pair check) |
| On-disk size | ≈206 KB (compressed encoding) |
| Source | `https://ceremony.ethereum.org/api/v1/transcript` |
| Embedded path | `src/prover/srs/ef-kzg-2023.bin` |
| Hash file | `src/prover/srs/expected-hash.in` |
| Build-time check | `build.rs` SHA-256-asserts the embedded bytes against the hash file; mismatch fails the build with "SRS hash mismatch — refusing to build" |

To independently verify the SRS:

```bash
shasum -a 256 src/prover/srs/ef-kzg-2023.bin
# Compare against the value in src/prover/srs/expected-hash.in
# and against the hash published by EF at ceremony.ethereum.org.
```

The same bytes are embedded into the mobile prover bundles (iOS `Resources/srs.bin`, Android `assets/srs/srs.bin`) and into each per-type contract's `build.rs`-driven VK preprocessing pipeline. If you ever see three different SHA-256 hashes for these three locations, treat it as a build-system integrity incident.

If a future circuit needs more than 4,096 G1 elements, an alternative BLS12-381 SRS must be identified and pinned (e.g., a future expansion of the EF KZG transcript). Note that **Aztec Ignition is on BN254**, not BLS12-381, so it is *not* a drop-in fallback for this stack — switching to a BN254 source would require swapping the curve everywhere in the prover/verifier pipeline, not just the SRS bytes. Switching SRS sources within BLS12-381 is a one-line change in `src/prover/srs.rs` plus a new hash pin; the rest of the prover/verifier stack is SRS-source-agnostic so long as the curve is fixed.
