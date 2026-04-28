#!/bin/bash
set -euo pipefail

# Deploy the per-type Tyranny Soroban contract to Stellar testnet.
#
# Sibling of scripts/deploy_sep_{anarchy,democracy,oligarchy,oneonone}_testnet.sh,
# scoped to the single-admin contract at contracts/sep-tyranny/. Per
# docs/tyranny-update-testnet-design.md.
#
# Scope:
#   * Build sep_tyranny_contract.wasm
#   * Deploy with admin + 9 VKs (3 Membership + 3 Create + 3 Update,
#     all per-tier)
#   * Surface the deployed contract address
#
# Out of scope (Phase A blocker — Tyranny circuits are NEW vs. keyset-v2):
#   * Generating real fixtures (TyrannyCreateCircuit + TyrannyUpdateCircuit
#     don't exist in any prior keyset). Operator supplies dev VKs via
#     FIXTURE_DIR.
#
# Alias state note: this script registers `--alias $ALIAS` (default
# `sep-tyranny-testnet`) inside CONFIG_DIR. When PERSIST_IDENTITY=1 the
# alias survives between runs in $HOME/.config/stellar; iterating locally
# with the same alias may collide with stellar's prior-deploy bookkeeping.
# Override ALIAS=... or set PERSIST_IDENTITY=0 (default) for ephemeral runs.

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

NETWORK="${NETWORK:-testnet}"
IDENTITY="${IDENTITY:-sep-tyranny-testnet-deployer}"
ALIAS="${ALIAS:-sep-tyranny-testnet}"
KEEP_ARTIFACTS="${KEEP_ARTIFACTS:-0}"
PERSIST_IDENTITY="${PERSIST_IDENTITY:-0}"

if [ "$PERSIST_IDENTITY" = "1" ]; then
    CONFIG_DIR="${CONFIG_DIR:-${STELLAR_CONFIG_DIR:-$HOME/.config/stellar}}"
else
    CONFIG_DIR="${CONFIG_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sep-tyranny-stellar-config.XXXXXX")}"
fi
WORK_DIR="${WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sep-tyranny-testnet.XXXXXX")}"
ARTIFACT_DIR="$WORK_DIR/artifacts"

# FIXTURE_DIR: Tyranny dev VKs. Until Phase A's circuits + fixture
# generator land, the operator supplies this dir.
# Expected layout (9 files):
#   $FIXTURE_DIR/vk-membership-{small,medium,large}.json   (3 IC points)
#   $FIXTURE_DIR/vk-create-{small,medium,large}.json       (4 IC points)
#   $FIXTURE_DIR/vk-update-{small,medium,large}.json       (5 IC points)
FIXTURE_DIR="${FIXTURE_DIR:-$REPO_ROOT/keyset-tyranny-dev}"

# Cleanup matrix: KEEP_ARTIFACTS=1 disables the trap entirely, so the
# ephemeral CONFIG_DIR (which holds the deployer's private key under
# PERSIST_IDENTITY=0) is left on disk for forensics. Operators choosing
# KEEP_ARTIFACTS=1 with PERSIST_IDENTITY=0 should treat
# /tmp/sep-tyranny-stellar-config.* as private-key material.
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

echo "==> Building Tyranny Soroban contract"
mkdir -p "$ARTIFACT_DIR"
stellar contract build \
    --manifest-path "$REPO_ROOT/contracts/sep-tyranny/Cargo.toml" \
    --out-dir "$ARTIFACT_DIR"

WASM_PATH="$ARTIFACT_DIR/sep_tyranny_contract.wasm"
if [ ! -f "$WASM_PATH" ]; then
    echo "expected built wasm at $WASM_PATH" >&2
    exit 1
fi

if [ ! -d "$FIXTURE_DIR" ]; then
    echo "FIXTURE_DIR=$FIXTURE_DIR does not exist; supply Tyranny dev VKs (Phase A blocker)" >&2
    exit 1
fi

# Pre-deploy IC-count validation: membership=3, create=4, update=5.
expect_ic_count() {
    expected="$1"
    file="$2"
    if ! jq -e '.ic | type == "array"' "$file" >/dev/null 2>&1; then
        echo "VK shape error: $file is missing the .ic field or .ic is not an array" >&2
        exit 1
    fi
    actual="$(jq '.ic | length' "$file" 2>/dev/null || echo "")"
    if [ "$actual" != "$expected" ]; then
        echo "VK shape mismatch: $file has $actual IC points, expected $expected" >&2
        echo "  membership VKs need 3 IC points (commitment, epoch + base)" >&2
        echo "  create VKs need 4 IC points (commitment, epoch, admin_pubkey_commitment + base)" >&2
        echo "  update VKs need 5 IC points (c_old, epoch_old, c_new, admin_pubkey_commitment + base)" >&2
        exit 1
    fi
}
for f in vk-membership-small.json vk-membership-medium.json vk-membership-large.json; do
    [ -f "$FIXTURE_DIR/$f" ] || { echo "missing $FIXTURE_DIR/$f" >&2; exit 1; }
    expect_ic_count 3 "$FIXTURE_DIR/$f"
done
for f in vk-create-small.json vk-create-medium.json vk-create-large.json; do
    [ -f "$FIXTURE_DIR/$f" ] || { echo "missing $FIXTURE_DIR/$f" >&2; exit 1; }
    expect_ic_count 4 "$FIXTURE_DIR/$f"
done
for f in vk-update-small.json vk-update-medium.json vk-update-large.json; do
    [ -f "$FIXTURE_DIR/$f" ] || { echo "missing $FIXTURE_DIR/$f" >&2; exit 1; }
    expect_ic_count 5 "$FIXTURE_DIR/$f"
done

if [ "$PERSIST_IDENTITY" = "1" ] && stellar keys public-key "$IDENTITY" --config-dir "$CONFIG_DIR" >/dev/null 2>&1; then
    echo "==> Reusing existing persistent identity: $IDENTITY"
else
    echo "==> Creating deployer identity in Stellar config ($CONFIG_DIR)"
    stellar keys generate "$IDENTITY" \
        --config-dir "$CONFIG_DIR" \
        --network "$NETWORK" \
        --overwrite >/dev/null
fi

DEPLOYER_ADDRESS="$(stellar keys public-key "$IDENTITY" --config-dir "$CONFIG_DIR" | tr -d '\n')"
[ -n "$DEPLOYER_ADDRESS" ] || { echo "failed to resolve deployer public key" >&2; exit 1; }

echo "==> Funding deployer via Friendbot"
if ! stellar keys fund "$IDENTITY" --config-dir "$CONFIG_DIR" --network "$NETWORK" >/dev/null 2>&1; then
    if [ "$PERSIST_IDENTITY" = "1" ]; then
        echo "    fund attempted but Friendbot returned non-zero; deploy may fail on balance"
    else
        stellar keys fund "$IDENTITY" --config-dir "$CONFIG_DIR" --network "$NETWORK" >/dev/null
    fi
fi

echo "==> Deploying contract to $NETWORK"
CONTRACT_ID="$(
    stellar contract deploy \
        --config-dir "$CONFIG_DIR" \
        --network "$NETWORK" \
        --source-account "$IDENTITY" \
        --alias "$ALIAS" \
        --wasm "$WASM_PATH" \
        -- \
        --admin "$DEPLOYER_ADDRESS" \
        --vk-membership-small-file-path "$FIXTURE_DIR/vk-membership-small.json" \
        --vk-membership-medium-file-path "$FIXTURE_DIR/vk-membership-medium.json" \
        --vk-membership-large-file-path "$FIXTURE_DIR/vk-membership-large.json" \
        --vk-create-small-file-path "$FIXTURE_DIR/vk-create-small.json" \
        --vk-create-medium-file-path "$FIXTURE_DIR/vk-create-medium.json" \
        --vk-create-large-file-path "$FIXTURE_DIR/vk-create-large.json" \
        --vk-update-small-file-path "$FIXTURE_DIR/vk-update-small.json" \
        --vk-update-medium-file-path "$FIXTURE_DIR/vk-update-medium.json" \
        --vk-update-large-file-path "$FIXTURE_DIR/vk-update-large.json" \
        | tr -d '\n'
)"

[ -n "$CONTRACT_ID" ] || { echo "failed to capture deployed contract id" >&2; exit 1; }

echo
echo "Tyranny contract deployed."
echo "Contract ID: $CONTRACT_ID"
echo "Stellar config dir: $CONFIG_DIR"
echo "Temporary artifacts dir: $WORK_DIR"
echo

# Liveness smoke: bump_group_ttl on a never-existed group MUST return
# Error(Contract, #5) (GroupNotFound). The entrypoint is permissionless
# (no auth setup needed), reachable without TyrannyCreate/Update
# fixtures, and proves the contract is responsive to invocations
# before Phase A real-fixture work lands. SKIP_SMOKE=1 to skip.
if [ "${SKIP_SMOKE:-0}" != "1" ]; then
    echo "==> Liveness smoke: bump_group_ttl against unknown group (expects #5 GroupNotFound)"
    SMOKE_OUT="$(stellar contract invoke \
        --config-dir "$CONFIG_DIR" \
        --network "$NETWORK" \
        --source-account "$IDENTITY" \
        --id "$CONTRACT_ID" \
        -- \
        bump_group_ttl \
        --group-id 0000000000000000000000000000000000000000000000000000000000000000 \
        2>&1 || true)"
    if echo "$SMOKE_OUT" | grep -q 'Error(Contract, #5)'; then
        echo "    smoke OK — contract responded with GroupNotFound"
    else
        echo "    smoke unexpected output (continuing — Phase A real-fixture smoke covers full path):" >&2
        echo "$SMOKE_OUT" | head -10 >&2
    fi
fi

# TODO(phase-a): full post-deploy smoke (create_group → update_commitment →
# verify_membership). Requires TyrannyCreateCircuit + TyrannyUpdateCircuit
# fixture generator and dev proving keys.

if [ "$KEEP_ARTIFACTS" = "1" ]; then
    trap - EXIT INT TERM
fi
