#!/bin/sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

NETWORK="${NETWORK:-testnet}"
IDENTITY="${IDENTITY:-sep-xxxx-testnet-deployer}"
ALIAS="${ALIAS:-sep-xxxx-testnet}"
KEEP_ARTIFACTS="${KEEP_ARTIFACTS:-0}"

CONFIG_DIR="${CONFIG_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sep-xxxx-stellar-config.XXXXXX")}"
WORK_DIR="${WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sep-xxxx-testnet.XXXXXX")}"
ARTIFACT_DIR="$WORK_DIR/artifacts"
FIXTURE_DIR="$WORK_DIR/fixtures"

cleanup() {
    if [ "$KEEP_ARTIFACTS" != "1" ]; then
        rm -rf "$WORK_DIR" "$CONFIG_DIR"
    fi
}
trap cleanup EXIT INT TERM

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "missing required command: $1" >&2
        exit 1
    fi
}

require_cmd cargo
require_cmd stellar

echo "==> Building Soroban contract"
mkdir -p "$ARTIFACT_DIR" "$FIXTURE_DIR"
stellar contract build \
    --manifest-path "$REPO_ROOT/contracts/sep-xxxx/Cargo.toml" \
    --out-dir "$ARTIFACT_DIR"

WASM_PATH="$ARTIFACT_DIR/sep_xxxx_contract.wasm"
if [ ! -f "$WASM_PATH" ]; then
    echo "expected built wasm at $WASM_PATH" >&2
    exit 1
fi

echo "==> Generating contract-ready Groth16 fixtures"
cargo run --quiet \
    --release \
    --manifest-path "$REPO_ROOT/Cargo.toml" \
    --bin generate_contract_testnet_fixtures \
    -- \
    --out-dir "$FIXTURE_DIR"

GROUP_ID="$(tr -d '\n' < "$FIXTURE_DIR/group-id.hex")"
COMMITMENT_0="$(tr -d '\n' < "$FIXTURE_DIR/commitment-epoch-0.hex")"
COMMITMENT_1="$(tr -d '\n' < "$FIXTURE_DIR/commitment-epoch-1.hex")"
TIER="$(tr -d '\n' < "$FIXTURE_DIR/tier.txt")"

echo "==> Creating deployer identity in isolated Stellar config"
stellar keys generate "$IDENTITY" \
    --config-dir "$CONFIG_DIR" \
    --network "$NETWORK" \
    --overwrite >/dev/null

DEPLOYER_ADDRESS="$(stellar keys public-key "$IDENTITY" --config-dir "$CONFIG_DIR" | tr -d '\n')"

echo "==> Funding deployer via Friendbot-backed testnet funding"
stellar keys fund "$IDENTITY" \
    --config-dir "$CONFIG_DIR" \
    --network "$NETWORK" >/dev/null

echo "==> Deploying contract to $NETWORK"
CONTRACT_ID="$(
    stellar contract deploy \
        --config-dir "$CONFIG_DIR" \
        --network "$NETWORK" \
        --source-account "$IDENTITY" \
        --alias "$ALIAS" \
        --wasm "$WASM_PATH" \
        | tr -d '\n'
)"

if [ -z "$CONTRACT_ID" ]; then
    echo "failed to capture deployed contract id" >&2
    exit 1
fi

echo "==> Initializing contract with test verification keys"
stellar contract invoke \
    --config-dir "$CONFIG_DIR" \
    --network "$NETWORK" \
    --id "$CONTRACT_ID" \
    --source-account "$IDENTITY" \
    -- initialize \
    --admin "$DEPLOYER_ADDRESS" \
    --vk-small-file-path "$FIXTURE_DIR/vk-small.json" \
    --vk-medium-file-path "$FIXTURE_DIR/vk-medium.json" \
    --vk-large-file-path "$FIXTURE_DIR/vk-large.json" >/dev/null

echo "==> create_group"
stellar contract invoke \
    --config-dir "$CONFIG_DIR" \
    --network "$NETWORK" \
    --id "$CONTRACT_ID" \
    --source-account "$IDENTITY" \
    -- create_group \
    --group-id "$GROUP_ID" \
    --commitment "$COMMITMENT_0" \
    --tier "$TIER" \
    --proof-file-path "$FIXTURE_DIR/proof-epoch-0.json" \
    --public-inputs-file-path "$FIXTURE_DIR/public-inputs-epoch-0.json" >/dev/null

echo "==> verify_membership at epoch 0"
VERIFY_0="$(
    stellar contract invoke \
        --config-dir "$CONFIG_DIR" \
        --network "$NETWORK" \
        --id "$CONTRACT_ID" \
        --source-account "$IDENTITY" \
        --send no \
        -- verify_membership \
        --group-id "$GROUP_ID" \
        --proof-file-path "$FIXTURE_DIR/proof-epoch-0.json" \
        --public-inputs-file-path "$FIXTURE_DIR/public-inputs-epoch-0.json"
)"
VERIFY_0_COMPACT="$(printf '%s' "$VERIFY_0" | tr -d '[:space:]')"
printf '%s' "$VERIFY_0_COMPACT" | grep -F 'true'

echo "==> update_commitment to epoch 1"
stellar contract invoke \
    --config-dir "$CONFIG_DIR" \
    --network "$NETWORK" \
    --id "$CONTRACT_ID" \
    --source-account "$IDENTITY" \
    -- update_commitment \
    --group-id "$GROUP_ID" \
    --new-commitment "$COMMITMENT_1" \
    --new-epoch 1 \
    --proof-file-path "$FIXTURE_DIR/proof-epoch-0.json" \
    --public-inputs-file-path "$FIXTURE_DIR/public-inputs-epoch-0.json" >/dev/null

echo "==> get_state after update"
STATE_AFTER_UPDATE="$(
    stellar contract invoke \
        --config-dir "$CONFIG_DIR" \
        --network "$NETWORK" \
        --id "$CONTRACT_ID" \
        --source-account "$IDENTITY" \
        --send no \
        -- get_state \
        --group-id "$GROUP_ID"
)"
STATE_AFTER_UPDATE_COMPACT="$(printf '%s' "$STATE_AFTER_UPDATE" | tr -d '[:space:]')"
printf '%s' "$STATE_AFTER_UPDATE_COMPACT" | grep -F "\"epoch\":1"
printf '%s' "$STATE_AFTER_UPDATE_COMPACT" | grep -F "\"commitment\":\"$COMMITMENT_1\""
printf '%s' "$STATE_AFTER_UPDATE_COMPACT" | grep -F "\"active\":true"

echo "==> verify_membership at epoch 1"
VERIFY_1="$(
    stellar contract invoke \
        --config-dir "$CONFIG_DIR" \
        --network "$NETWORK" \
        --id "$CONTRACT_ID" \
        --source-account "$IDENTITY" \
        --send no \
        -- verify_membership \
        --group-id "$GROUP_ID" \
        --proof-file-path "$FIXTURE_DIR/proof-epoch-1.json" \
        --public-inputs-file-path "$FIXTURE_DIR/public-inputs-epoch-1.json"
)"
VERIFY_1_COMPACT="$(printf '%s' "$VERIFY_1" | tr -d '[:space:]')"
printf '%s' "$VERIFY_1_COMPACT" | grep -F 'true'

echo "==> deactivate_group"
stellar contract invoke \
    --config-dir "$CONFIG_DIR" \
    --network "$NETWORK" \
    --id "$CONTRACT_ID" \
    --source-account "$IDENTITY" \
    -- deactivate_group \
    --group-id "$GROUP_ID" \
    --proof-file-path "$FIXTURE_DIR/proof-epoch-1.json" \
    --public-inputs-file-path "$FIXTURE_DIR/public-inputs-epoch-1.json" >/dev/null

echo "==> get_state after deactivation"
STATE_AFTER_DEACTIVATE="$(
    stellar contract invoke \
        --config-dir "$CONFIG_DIR" \
        --network "$NETWORK" \
        --id "$CONTRACT_ID" \
        --source-account "$IDENTITY" \
        --send no \
        -- get_state \
        --group-id "$GROUP_ID"
)"
STATE_AFTER_DEACTIVATE_COMPACT="$(printf '%s' "$STATE_AFTER_DEACTIVATE" | tr -d '[:space:]')"
printf '%s' "$STATE_AFTER_DEACTIVATE_COMPACT" | grep -F "\"active\":false"

echo "==> get_history"
HISTORY="$(
    stellar contract invoke \
        --config-dir "$CONFIG_DIR" \
        --network "$NETWORK" \
        --id "$CONTRACT_ID" \
        --source-account "$IDENTITY" \
        --send no \
        -- get_history \
        --group-id "$GROUP_ID" \
        --max-entries 64
)"
HISTORY_COMPACT="$(printf '%s' "$HISTORY" | tr -d '[:space:]')"
printf '%s' "$HISTORY_COMPACT" | grep -F "\"commitment\":\"$COMMITMENT_0\""
printf '%s' "$HISTORY_COMPACT" | grep -F "\"epoch\":0"

echo
echo "Contract deployed and integration-tested successfully."
echo "Contract ID: $CONTRACT_ID"
echo "Stellar config dir: $CONFIG_DIR"
echo "Temporary artifacts dir: $WORK_DIR"

if [ "$KEEP_ARTIFACTS" = "1" ]; then
    trap - EXIT INT TERM
fi
