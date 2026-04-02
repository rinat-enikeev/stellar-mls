# Mainnet Deployment Guide

Deploy the SEP-XXXX private group membership contract to Stellar mainnet, run a fee-paying relayer, and connect the iOS/Android chat apps — all from a single funded account.

## Prerequisites

- **Rust 1.75+** with `cargo`
- **`stellar` CLI** ([install](https://developers.stellar.org/docs/tools/developer-tools/cli/install-cli))
- **A funded Stellar mainnet account** (minimum 20 XLM, recommended 50 XLM for deployment + initialization)
- **A second funded account** for the relayer (recommended 100 XLM for ongoing fee payments)

## Cost Estimates

| Operation | Approximate Cost |
|-----------|-----------------|
| Contract deployment (WASM upload) | ~10 XLM |
| Contract initialization (3 VKs) | ~5 XLM |
| Each `create_group` | ~0.01 XLM |
| Each `update_commitment` | ~0.005 XLM |
| `verify_membership` (read-only) | Free |
| **Deployer minimum** | **~20 XLM** |
| **Relayer recommended** | **~100 XLM** |

---

## Step 1: Deploy the Contract

### 1.1 Import your Stellar account

If you already have a funded mainnet account with a secret key (S...):

```bash
# Import into stellar CLI
stellar keys import my-deployer --secret-key
# Paste your S... key when prompted
```

Or for testnet dry-run:

```bash
stellar keys generate my-deployer --network testnet
stellar keys fund my-deployer --network testnet
```

### 1.2 Run the deployment script

```bash
# Mainnet deployment
IDENTITY=my-deployer ./scripts/deploy-mainnet.sh

# Testnet dry-run (recommended first)
IDENTITY=my-deployer ./scripts/deploy-mainnet.sh --network testnet
```

The script will:
1. Build the Soroban contract WASM
2. Generate verification keys (deterministic seed 42, matching mobile app defaults)
3. Deploy the contract
4. Initialize with verification keys for all three tiers (small/medium/large)
5. Verify the contract is live
6. Print the contract ID and configuration instructions

### 1.3 Save the contract ID

The script outputs something like:

```
════════════════════════════════════════════════════════════════
  Deployment complete!
════════════════════════════════════════════════════════════════

  Contract ID:      CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
  Admin address:    GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
  Network:          mainnet
```

Save the **Contract ID** — you'll need it for the relayer and app configuration.

### 1.4 Script options

| Option | Description |
|--------|-------------|
| `--identity <name>` | Stellar CLI identity to use (alternative to `IDENTITY` env var) |
| `--network <name>` | `mainnet` (default) or `testnet` |
| `--keep-artifacts` | Preserve generated VK files and WASM after deployment |

---

## Step 2: Deploy the Relayer

The relayer is an HTTP service that receives contract invocation requests from mobile apps, signs them with its own Stellar account, and submits to the network. This way chat participants don't need funded Stellar accounts.

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
# Your relayer account's secret key (REQUIRED)
RELAYER_SECRET_KEY=SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX

# Contract ID from Step 1 (REQUIRED)
RELAYER_CONTRACT_ID=CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX

# RPC endpoint
RELAYER_RPC_URL=https://soroban.stellar.org

# Network
RELAYER_NETWORK=mainnet

# Listen address
RELAYER_BIND=0.0.0.0:8080

# Optional: comma-separated bearer tokens for auth
# If empty, no authentication is required
RELAYER_AUTH_TOKENS=my-secret-token-1,my-secret-token-2

# Rate limit per IP
RELAYER_RATE_LIMIT=30
```

### 2.3 Run the relayer

```bash
cd relayer

# Development
sh ./run.sh --release

# Or build and run
cargo build --release
./target/release/sep-xxxx-relayer
```

You should see:

```
Relayer address: GXXXXXXX...
Contract ID:    CXXXXXXX...
Network:        mainnet
Auth required:  true
Rate limit:     30 req/min per IP
Listening on 0.0.0.0:8080
```

### 2.4 Run with Docker

```bash
cd relayer
docker build -t sep-relayer .
docker run -d \
  --env-file .env \
  -p 8080:8080 \
  --name sep-relayer \
  sep-relayer
```

### 2.5 Verify the relayer

```bash
# Should return an error (group doesn't exist) — confirms the relayer is working
curl -s -X POST http://localhost:8080 \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer my-secret-token-1" \
  -d '{
    "contractID": "CXXXX...",
    "function": "get_state",
    "payload": {
      "groupID": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    }
  }'
```

Expected: an error response mentioning "GroupNotFound" — this means the relayer successfully reached the contract.

### 2.6 Production deployment

For production, deploy the relayer behind HTTPS:

- Use a reverse proxy (nginx, Caddy) with TLS termination
- Point your domain (e.g., `https://relayer.example.com`) to the relayer
- The mobile apps support TLS certificate pinning for additional security

### 2.7 Relayer security

The relayer validates every request:

| Check | Description |
|-------|-------------|
| Contract ID whitelist | Only the configured contract ID is allowed |
| Function whitelist | Only SEP-XXXX functions (`create_group`, `update_commitment`, `verify_membership`, `deactivate_group`, `get_state`, `get_history`) |
| Proof size | Must decode to exactly 384 bytes |
| Payload size | Rejected if > 8 KB |
| Rate limiting | Per IP address (configurable) |
| Bearer auth | Optional, recommended for production |

---

## Step 3: Configure Mobile Apps

### iOS

1. Open **StellarChat** → **Settings**
2. In the **Stellar Contract** section:
   - **Endpoint URL**: `https://soroban.stellar.org` (or your relayer URL if not using the relayer section)
   - **Contract ID**: paste the contract ID from Step 1
3. If using the relayer, in the **Relayer** section:
   - **Relayer URL**: `https://relayer.example.com` (or `http://localhost:8080` for local testing)
   - **Auth Token**: your bearer token (if configured)
4. Tap **Save**

### Android

1. Open **StellarChat** → **Settings**
2. In the **Stellar Contract** section:
   - **Endpoint URL**: `https://soroban.stellar.org`
   - **Contract ID**: paste the contract ID from Step 1
3. If using the relayer:
   - **Relayer URL**: `https://relayer.example.com`
   - **Auth Token**: your bearer token
4. Tap **Save**

### How it works

When you create a group in the app:
1. The app generates a ZK proof of membership using the local proving key (seed 42)
2. The app sends the proof + group data to the relayer as JSON
3. The relayer wraps the request in a Stellar transaction, signs with its own account, and submits
4. The contract verifies the proof on-chain and stores the group's commitment
5. The app shows "Verified on-chain" status

The relayer's Stellar account pays the transaction fee. Chat participants never need their own funded Stellar accounts.

---

## Step 4: End-to-End Verification

### 4.1 Create a group

1. Open the app on one device
2. Tap **+** → **Create Group**
3. Enter a name → **Create**
4. If contract + relayer are configured, the app automatically publishes the group on-chain
5. Check the group list — it should show epoch and member count

### 4.2 Verify on-chain

In the app, the group should display a "Verified" badge indicating its commitment matches the on-chain state.

You can also verify manually:

```bash
# Replace GROUP_ID_HEX with the group's ID (visible in app debug info)
stellar contract invoke \
  --network mainnet \
  --id CXXXX... \
  --source-account my-deployer \
  --send no \
  -- get_state \
  --group-id GROUP_ID_HEX
```

### 4.3 Cross-platform test

1. Create a group on iOS
2. Share the invite code with an Android device
3. Join on Android
4. Both devices should show the same group with matching epoch and member count
5. Send messages — they should flow bidirectionally via Nostr relays

---

## Troubleshooting

| Error | Cause | Fix |
|-------|-------|-----|
| `InvalidProof` | VK/proving key mismatch | Ensure the contract was deployed with seed-42 VKs (use `deploy-mainnet.sh`) |
| `NotInitialized` | Contract deployed but not initialized | Re-run the deployment script |
| `GroupAlreadyExists` | Duplicate group creation | Normal if retrying — the group already exists on-chain |
| `AdminOnly` | Restricted mode enabled | The admin can disable it: `set_restricted_mode(false)` |
| Relayer returns 401 | Invalid bearer token | Check `RELAYER_AUTH_TOKENS` in `.env` |
| Relayer returns 429 | Rate limited | Wait 60 seconds or increase `RELAYER_RATE_LIMIT` |
| Relayer returns 400 | Payload validation failed | Check the error message — usually wrong contract ID or malformed proof |
| Relayer returns 502 | CLI invocation failed | Check that `stellar` CLI is installed and the relayer account is funded |
| App shows "Not configured" | Missing contract settings | Enter endpoint URL and contract ID in Settings |
| App shows verification failure | Epoch mismatch | Local state diverged from on-chain — try re-verifying |

---

## Security Notes

### Account separation

- **Deployer account** — becomes the contract admin. Store its secret key securely offline after deployment. Only needed for contract upgrades or enabling restricted mode.
- **Relayer account** — used for ongoing operations. Keep it funded. If compromised, an attacker can only create/modify groups (requires valid ZK proofs) and spend the relayer's XLM.

### Testing vs production VKs

This deployment uses **testing verification keys** (deterministic seed 42). This means:
- Proofs are cryptographically valid and the system works correctly
- The trusted setup was performed by a single machine (not an MPC ceremony)
- Acceptable for personal use, demos, and development
- For production with adversarial threat models, run a multi-party ceremony (see `docs/phase-2.md`)

### Privacy model

- **On-chain**: The contract stores only opaque 32-byte commitments — never member lists, keys, or identities. The relayer's address is visible as the transaction signer (not the group member's).
- **Nostr**: All messages are AES-256-GCM encrypted. Relays see ciphertext and topic tags only.
- **Relayer**: Sees the JSON payloads (proofs, commitments) but not member identities. For maximum privacy, access the relayer over Tor/VPN.

---

## Appendix: VK Seed Compatibility

Both mobile apps generate proving keys at runtime using `generateTestingProvingKey(tier, seed=42)`. The deployment script's VK generator (`generate_mainnet_vks`) uses the same seed 42 for all three tiers:

| Tier | Depth | Max Members | Seed |
|------|-------|-------------|------|
| Small | 5 | 32 | 42 |
| Medium | 8 | 256 | 42 |
| Large | 11 | 2,048 | 42 |

This is critical: the verification key deployed on-chain must correspond to the same `(depth, seed)` as the proving key used by the apps. If you change the seed in the apps, you must redeploy the contract with matching VKs.

Note: The testnet deployment script (`deploy_sep_xxxx_testnet.sh`) uses seeds 1001/1002/1003 for VK generation — those VKs are **incompatible** with the mobile apps' default proving keys. Always use `deploy-mainnet.sh` for app-compatible deployments.
