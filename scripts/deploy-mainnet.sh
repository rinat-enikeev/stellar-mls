#!/bin/sh
#
# Deploy the SEP-XXXX contract to Stellar mainnet (or testnet for dry-run).
#
# Prerequisites:
#   - cargo, stellar CLI installed
#   - A funded Stellar identity imported into stellar CLI
#
# Usage:
#   # Mainnet (default)
#   IDENTITY=my-mainnet-deployer ./scripts/deploy-mainnet.sh
#
#   # Testnet dry-run
#   IDENTITY=my-testnet-deployer ./scripts/deploy-mainnet.sh --network testnet
#
#   # Keep generated VK files after deployment
#   IDENTITY=my-deployer ./scripts/deploy-mainnet.sh --keep-artifacts
#
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

# ── Configuration ──────────────────────────────────────────────────
IDENTITY="${IDENTITY:-}"
NETWORK="mainnet"
KEEP_ARTIFACTS=0

# Parse arguments
while [ $# -gt 0 ]; do
    case "$1" in
        --identity)  IDENTITY="$2"; shift 2 ;;
        --network)   NETWORK="$2"; shift 2 ;;
        --keep-artifacts) KEEP_ARTIFACTS=1; shift ;;
        *)           echo "Unknown argument: $1" >&2; exit 1 ;;
    esac
done

if [ -z "$IDENTITY" ]; then
    echo "ERROR: No identity specified." >&2
    echo "  Set IDENTITY=<name> or pass --identity <name>" >&2
    echo "  The identity must already exist in stellar CLI and be funded." >&2
    echo "" >&2
    echo "  To import an existing secret key:" >&2
    echo "    stellar keys import my-deployer --secret-key" >&2
    echo "" >&2
    echo "  To create and fund a testnet identity:" >&2
    echo "    stellar keys generate my-deployer --network testnet" >&2
    echo "    stellar keys fund my-deployer --network testnet" >&2
    exit 1
fi

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/sep-xxxx-deploy.XXXXXX")"
ARTIFACT_DIR="$WORK_DIR/artifacts"
VK_DIR="$WORK_DIR/vks"

cleanup() {
    if [ "$KEEP_ARTIFACTS" != "1" ]; then
        rm -rf "$WORK_DIR"
    fi
}
trap cleanup EXIT INT TERM

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "ERROR: Missing required command: $1" >&2
        exit 1
    fi
}

require_cmd cargo
require_cmd stellar

# ── Step 1: Get deployer address ───────────────────────────────────
echo "==> Resolving deployer identity: $IDENTITY"
DEPLOYER_ADDRESS="$(stellar keys public-key "$IDENTITY" | tr -d '\n')"
if [ -z "$DEPLOYER_ADDRESS" ]; then
    echo "ERROR: Could not resolve public key for identity '$IDENTITY'" >&2
    echo "  Make sure the identity exists: stellar keys list" >&2
    exit 1
fi
echo "    Deployer address: $DEPLOYER_ADDRESS"
echo "    Network: $NETWORK"

# ── Step 2: Build contract WASM ────────────────────────────────────
echo ""
echo "==> Building Soroban contract WASM"
mkdir -p "$ARTIFACT_DIR" "$VK_DIR"
stellar contract build \
    --manifest-path "$REPO_ROOT/contracts/sep-xxxx/Cargo.toml" \
    --out-dir "$ARTIFACT_DIR"

WASM_PATH="$ARTIFACT_DIR/sep_xxxx_contract.wasm"
if [ ! -f "$WASM_PATH" ]; then
    echo "ERROR: Expected WASM at $WASM_PATH" >&2
    exit 1
fi
echo "    WASM built: $(wc -c < "$WASM_PATH" | tr -d ' ') bytes"

# ── Step 3: Generate verification keys ────────────────────────────
echo ""
if [ -n "${KEYSET_DIR:-}" ]; then
    echo "==> Generating verification keys from keyset: $KEYSET_DIR"
    cargo run --quiet \
        --release \
        --manifest-path "$REPO_ROOT/Cargo.toml" \
        --bin generate_mainnet_vks \
        -- \
        --keyset-dir "$KEYSET_DIR" \
        --out-dir "$VK_DIR"
else
    echo "ERROR: KEYSET_DIR is required for production deployment." >&2
    echo "  Generate a keyset first: ./scripts/generate-keyset.sh" >&2
    echo "  Then: KEYSET_DIR=keyset-v1 IDENTITY=... ./scripts/deploy-mainnet.sh" >&2
    echo "" >&2
    echo "  For testing only: KEYSET_DIR=test VK_SEED=42 ..." >&2
    if [ -n "${VK_SEED:-}" ]; then
        echo "" >&2
        echo "  VK_SEED=$VK_SEED detected — proceeding with seed-based generation (testing only)."
        cargo run --quiet \
            --release \
            --manifest-path "$REPO_ROOT/Cargo.toml" \
            --bin generate_mainnet_vks \
            -- \
            --seed "$VK_SEED" \
            --out-dir "$VK_DIR"
    else
        exit 1
    fi
fi

for f in vk-small.json vk-medium.json vk-large.json; do
    if [ ! -f "$VK_DIR/$f" ]; then
        echo "ERROR: Expected VK file at $VK_DIR/$f" >&2
        exit 1
    fi
done

# ── Step 4: Deploy contract ────────────────────────────────────────
echo ""
echo "==> Deploying contract to $NETWORK"
CONTRACT_ID="$(
    stellar contract deploy \
        --network "$NETWORK" \
        --source-account "$IDENTITY" \
        --wasm "$WASM_PATH" \
        | tr -d '\n'
)"

if [ -z "$CONTRACT_ID" ]; then
    echo "ERROR: Failed to capture contract ID from deployment" >&2
    exit 1
fi
echo "    Contract ID: $CONTRACT_ID"

# ── Step 5: Initialize contract with VKs ───────────────────────────
echo ""
echo "==> Initializing contract with verification keys"
stellar contract invoke \
    --network "$NETWORK" \
    --id "$CONTRACT_ID" \
    --source-account "$IDENTITY" \
    -- initialize \
    --admin "$DEPLOYER_ADDRESS" \
    --vk-small-file-path "$VK_DIR/vk-small.json" \
    --vk-medium-file-path "$VK_DIR/vk-medium.json" \
    --vk-large-file-path "$VK_DIR/vk-large.json" >/dev/null

echo "    Initialized successfully"

# ── Step 6: Verify contract is live ────────────────────────────────
echo ""
echo "==> Verifying contract is live (expecting GroupNotFound for dummy ID)"
VERIFY_OUTPUT="$(
    stellar contract invoke \
        --network "$NETWORK" \
        --id "$CONTRACT_ID" \
        --source-account "$IDENTITY" \
        --send no \
        -- get_state \
        --group-id "0000000000000000000000000000000000000000000000000000000000000001" 2>&1 || true
)"
if printf '%s' "$VERIFY_OUTPUT" | grep -qiE "GroupNotFound|Error\(5\)"; then
    echo "    Contract is live and responding correctly"
else
    echo "    WARNING: Unexpected response from get_state: $VERIFY_OUTPUT" >&2
    echo "    The contract may still be functional — check manually." >&2
fi

# ── Summary ────────────────────────────────────────────────────────
echo ""
echo "════════════════════════════════════════════════════════════════"
echo "  Deployment complete!"
echo "════════════════════════════════════════════════════════════════"
echo ""
echo "  Contract ID:      $CONTRACT_ID"
echo "  Admin address:    $DEPLOYER_ADDRESS"
echo "  Network:          $NETWORK"
echo ""
echo "  ── Mobile App Configuration ──"
echo ""
echo "  iOS / Android Settings → Stellar Contract:"
echo "    Endpoint URL:   https://soroban.stellar.org"
echo "    Contract ID:    $CONTRACT_ID"
echo ""
echo "  ── Relayer Configuration ──"
echo ""
echo "  Set these in relayer/.env:"
echo "    RELAYER_CONTRACT_ID=$CONTRACT_ID"
echo "    RELAYER_RPC_URL=https://soroban.stellar.org"
echo "    RELAYER_NETWORK=$NETWORK"
echo ""

if [ "$KEEP_ARTIFACTS" = "1" ]; then
    echo "  Artifacts preserved at: $WORK_DIR"
    trap - EXIT INT TERM
fi
