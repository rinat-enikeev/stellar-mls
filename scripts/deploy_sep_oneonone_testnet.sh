#!/bin/bash
set -euo pipefail

# Deploy the per-type 1v1 Soroban contract to Stellar testnet.
#
# Sibling of scripts/deploy_sep_{anarchy,democracy,oligarchy}_testnet.sh,
# scoped to the immutable two-party contract at contracts/sep-oneonone/.
# Per docs/oneonone-update-testnet-design.md.
#
# Shebang note: bash (not /bin/sh) so we can rely on `set -o pipefail`.
#
# Alias state note: this script registers `--alias $ALIAS` (default
# `sep-oneonone-testnet`) inside CONFIG_DIR. When PERSIST_IDENTITY=1
# the alias survives between runs in $HOME/.config/stellar.
#
# Scope:
#   * Build sep_oneonone_contract.wasm
#   * Deploy with admin + 1 Membership VK + 1 Create VK (2 VKs total
#     — smaller than sep-anarchy's 6, sep-democracy's 6, sep-oligarchy's 9)
#   * Surface the deployed contract address
#
# Out of scope:
#   * Post-deploy smoke (create_group → verify_membership). Requires the
#     OneOnOneCreateCircuit fixture generator and a real proving key —
#     see docs/oneonone-update-testnet-design.md §10 Phase A.

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

NETWORK="${NETWORK:-testnet}"
IDENTITY="${IDENTITY:-sep-oneonone-testnet-deployer}"
ALIAS="${ALIAS:-sep-oneonone-testnet}"
KEEP_ARTIFACTS="${KEEP_ARTIFACTS:-0}"
PERSIST_IDENTITY="${PERSIST_IDENTITY:-0}"

if [ "$PERSIST_IDENTITY" = "1" ]; then
    CONFIG_DIR="${CONFIG_DIR:-${STELLAR_CONFIG_DIR:-$HOME/.config/stellar}}"
else
    CONFIG_DIR="${CONFIG_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sep-oneonone-stellar-config.XXXXXX")}"
fi
WORK_DIR="${WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sep-oneonone-testnet.XXXXXX")}"
ARTIFACT_DIR="$WORK_DIR/artifacts"

# FIXTURE_DIR: pre-generated 1v1 dev VKs. Until Phase A's fixture
# generator lands, the operator supplies this dir.
# Expected layout (2 files):
#   $FIXTURE_DIR/vk-membership.json   (3 IC points)
#   $FIXTURE_DIR/vk-create.json       (3 IC points — OneOnOneCreateCircuit)
FIXTURE_DIR="${FIXTURE_DIR:-$REPO_ROOT/keyset-oneonone-dev}"

cleanup() {
    if [ "$KEEP_ARTIFACTS" != "1" ]; then
        rm -rf "$WORK_DIR"
        if [ "$PERSIST_IDENTITY" != "1" ]; then
            rm -rf "$CONFIG_DIR"
        fi
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
require_cmd jq

echo "==> Building 1v1 Soroban contract"
mkdir -p "$ARTIFACT_DIR"
stellar contract build \
    --manifest-path "$REPO_ROOT/contracts/sep-oneonone/Cargo.toml" \
    --out-dir "$ARTIFACT_DIR"

WASM_PATH="$ARTIFACT_DIR/sep_oneonone_contract.wasm"
if [ ! -f "$WASM_PATH" ]; then
    echo "expected built wasm at $WASM_PATH" >&2
    exit 1
fi

if [ ! -d "$FIXTURE_DIR" ]; then
    echo "FIXTURE_DIR=$FIXTURE_DIR does not exist; supply 1v1 dev VKs" >&2
    exit 1
fi

# Pre-deploy IC-count validation: both VKs are 3-IC. Surface malformed
# fixtures with the offending filename rather than as an opaque
# Error(Contract, #9) at constructor time.
expect_ic_count() {
    expected="$1"
    file="$2"
    if ! jq -e '.ic | type == "array"' "$file" >/dev/null 2>&1; then
        echo "VK shape error: $file is missing the .ic field or .ic is not an array" >&2
        exit 1
    fi
    actual="$(jq '.ic | length' "$file" 2>/dev/null || echo "")"
    if [ -z "$actual" ]; then
        echo "  unable to parse IC array from $file" >&2
        exit 1
    fi
    if [ "$actual" != "$expected" ]; then
        echo "VK shape mismatch: $file has $actual IC points, expected $expected" >&2
        echo "  1v1 VKs (Membership + Create) both need 3 IC points (commitment, epoch + base)" >&2
        exit 1
    fi
}
for f in vk-membership.json vk-create.json; do
    if [ ! -f "$FIXTURE_DIR/$f" ]; then
        echo "missing $FIXTURE_DIR/$f" >&2
        exit 1
    fi
    expect_ic_count 3 "$FIXTURE_DIR/$f"
done

if [ "$PERSIST_IDENTITY" = "1" ] && stellar keys public-key "$IDENTITY" --config-dir "$CONFIG_DIR" >/dev/null 2>&1; then
    echo "==> Reusing existing persistent identity: $IDENTITY (config-dir: $CONFIG_DIR)"
else
    echo "==> Creating deployer identity in Stellar config ($CONFIG_DIR)"
    stellar keys generate "$IDENTITY" \
        --config-dir "$CONFIG_DIR" \
        --network "$NETWORK" \
        --overwrite >/dev/null
fi

DEPLOYER_ADDRESS="$(stellar keys public-key "$IDENTITY" --config-dir "$CONFIG_DIR" | tr -d '\n')"
if [ -z "$DEPLOYER_ADDRESS" ]; then
    echo "failed to resolve deployer public key for identity '$IDENTITY' in $CONFIG_DIR" >&2
    exit 1
fi

echo "==> Funding deployer via Friendbot-backed testnet funding"
if ! stellar keys fund "$IDENTITY" --config-dir "$CONFIG_DIR" --network "$NETWORK" >/dev/null 2>&1; then
    if [ "$PERSIST_IDENTITY" = "1" ]; then
        echo "    fund attempted but Friendbot returned non-zero (possible throttle); deploy may fail on balance"
    else
        stellar keys fund "$IDENTITY" --config-dir "$CONFIG_DIR" --network "$NETWORK" >/dev/null
    fi
fi

echo "==> Deploying contract to $NETWORK with constructor args (atomic deploy+init)"
# Constructor parameter mapping comes from `SepOneOnOneContract::__constructor`.
# Two VKs: vk_membership + vk_create (both 3-IC).
CONTRACT_ID="$(
    stellar contract deploy \
        --config-dir "$CONFIG_DIR" \
        --network "$NETWORK" \
        --source-account "$IDENTITY" \
        --alias "$ALIAS" \
        --wasm "$WASM_PATH" \
        -- \
        --admin "$DEPLOYER_ADDRESS" \
        --vk-membership-file-path "$FIXTURE_DIR/vk-membership.json" \
        --vk-create-file-path "$FIXTURE_DIR/vk-create.json" \
        | tr -d '\n'
)"

if [ -z "$CONTRACT_ID" ]; then
    echo "failed to capture deployed contract id" >&2
    exit 1
fi

echo
echo "1v1 contract deployed."
echo "Contract ID: $CONTRACT_ID"
echo "Stellar config dir: $CONFIG_DIR"
echo "Temporary artifacts dir: $WORK_DIR"
echo
# TODO(phase-a): post-deploy smoke (create_group → verify_membership).
# Requires the OneOnOneCreateCircuit fixture generator and dev proving
# key — see docs/oneonone-update-testnet-design.md §10. There is no
# update_commitment / deactivate_group to exercise (1v1 by design).

if [ "$KEEP_ARTIFACTS" = "1" ]; then
    trap - EXIT INT TERM
fi
