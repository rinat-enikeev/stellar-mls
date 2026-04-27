#!/bin/bash
set -euo pipefail

# Deploy the per-type Anarchy Soroban contract to Stellar testnet.
#
# Sibling of scripts/deploy_sep_democracy_testnet.sh and
# deploy_sep_oligarchy_testnet.sh. Per docs/contract/anarchy/impl_plan.md
# and design v0.1 §4.6 (contract redeploy + VK install).
#
# Shebang note: bash (not /bin/sh) so we can rely on `set -o pipefail`
# without dash-incompatibility worries.
#
# Alias state note: this script registers `--alias $ALIAS` (default
# `sep-anarchy-testnet`) inside CONFIG_DIR. When PERSIST_IDENTITY=1
# the alias survives between runs in $HOME/.config/stellar; iterating
# locally with the same alias may collide with stellar's prior-deploy
# bookkeeping. Override ALIAS or set PERSIST_IDENTITY=0 (default) for
# ephemeral runs.
#
# Scope:
#   * Build sep_anarchy_contract.wasm
#   * Deploy with admin + per-tier membership VKs + per-tier
#     Anarchy update VKs (6 VKs total — same as sep-democracy,
#     smaller than sep-oligarchy's 9)
#   * Surface the deployed contract address
#
# Anarchy reuses the existing keyset-v2 VKs that the monolithic
# sep-xxxx already verifies against. No new circuit, fixture, or
# prover work required (Phase A is zero-LOC for Anarchy per design
# §6 Phase A).

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

NETWORK="${NETWORK:-testnet}"
IDENTITY="${IDENTITY:-sep-anarchy-testnet-deployer}"
ALIAS="${ALIAS:-sep-anarchy-testnet}"
KEEP_ARTIFACTS="${KEEP_ARTIFACTS:-0}"
PERSIST_IDENTITY="${PERSIST_IDENTITY:-0}"

if [ "$PERSIST_IDENTITY" = "1" ]; then
    CONFIG_DIR="${CONFIG_DIR:-${STELLAR_CONFIG_DIR:-$HOME/.config/stellar}}"
else
    CONFIG_DIR="${CONFIG_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sep-anarchy-stellar-config.XXXXXX")}"
fi
WORK_DIR="${WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sep-anarchy-testnet.XXXXXX")}"
ARTIFACT_DIR="$WORK_DIR/artifacts"

# FIXTURE_DIR: existing keyset-v2 VKs. Anarchy reuses the v2 keyset
# directly — no new fixture generation required for pure extraction.
# Expected layout (6 files):
#   $FIXTURE_DIR/vk-small.json         (3 IC points — membership)
#   $FIXTURE_DIR/vk-medium.json
#   $FIXTURE_DIR/vk-large.json
#   $FIXTURE_DIR/vk-update-small.json  (4 IC points — Anarchy update)
#   $FIXTURE_DIR/vk-update-medium.json
#   $FIXTURE_DIR/vk-update-large.json
FIXTURE_DIR="${FIXTURE_DIR:-$REPO_ROOT/keyset-v2}"

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

echo "==> Building Anarchy Soroban contract"
mkdir -p "$ARTIFACT_DIR"
stellar contract build \
    --manifest-path "$REPO_ROOT/contracts/sep-anarchy/Cargo.toml" \
    --out-dir "$ARTIFACT_DIR"

WASM_PATH="$ARTIFACT_DIR/sep_anarchy_contract.wasm"
if [ ! -f "$WASM_PATH" ]; then
    echo "expected built wasm at $WASM_PATH" >&2
    exit 1
fi

if [ ! -d "$FIXTURE_DIR" ]; then
    echo "FIXTURE_DIR=$FIXTURE_DIR does not exist; supply v2 keyset Anarchy VKs" >&2
    exit 1
fi

# Pre-deploy IC-count validation: membership=3, update=4. Surface
# malformed VKs with the offending filename rather than as an opaque
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
        echo "  unable to parse IC array from $file (jq returned empty)" >&2
        exit 1
    fi
    if [ "$actual" != "$expected" ]; then
        echo "VK shape mismatch: $file has $actual IC points, expected $expected" >&2
        echo "  membership VKs need 3 IC points (commitment, epoch + base)" >&2
        echo "  update VKs need 4 IC points (c_old, epoch_old, c_new + base) — Anarchy-specific (smaller than Democracy/Oligarchy's 7-IC)" >&2
        exit 1
    fi
}
for f in vk-small.json vk-medium.json vk-large.json; do
    if [ ! -f "$FIXTURE_DIR/$f" ]; then
        echo "missing $FIXTURE_DIR/$f" >&2
        echo "  Anarchy reuses keyset-v2; verify the v2 ceremony VKs are checked in." >&2
        exit 1
    fi
    expect_ic_count 3 "$FIXTURE_DIR/$f"
done
for f in vk-update-small.json vk-update-medium.json vk-update-large.json; do
    if [ ! -f "$FIXTURE_DIR/$f" ]; then
        echo "missing $FIXTURE_DIR/$f" >&2
        exit 1
    fi
    expect_ic_count 4 "$FIXTURE_DIR/$f"
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
        echo "    (likely cause: $IDENTITY is already funded under PERSIST_IDENTITY=1; if deploy fails on balance, retry after Friendbot cooldown)"
    else
        stellar keys fund "$IDENTITY" --config-dir "$CONFIG_DIR" --network "$NETWORK" >/dev/null
    fi
fi

echo "==> Deploying contract to $NETWORK with constructor args (atomic deploy+init)"
# Constructor arg names below come from `SepAnarchyContract::__constructor`
# parameter names in contracts/sep-anarchy/src/lib.rs. If that signature
# changes, this block needs to track. The flag-style mapping is:
#   `param_name: T`  →  `--param-name <value>`
#   `*_file_path`    →  reads the file off disk into the contract type
CONTRACT_ID="$(
    stellar contract deploy \
        --config-dir "$CONFIG_DIR" \
        --network "$NETWORK" \
        --source-account "$IDENTITY" \
        --alias "$ALIAS" \
        --wasm "$WASM_PATH" \
        -- \
        --admin "$DEPLOYER_ADDRESS" \
        --vk-small-file-path "$FIXTURE_DIR/vk-small.json" \
        --vk-medium-file-path "$FIXTURE_DIR/vk-medium.json" \
        --vk-large-file-path "$FIXTURE_DIR/vk-large.json" \
        --update-vk-small-file-path "$FIXTURE_DIR/vk-update-small.json" \
        --update-vk-medium-file-path "$FIXTURE_DIR/vk-update-medium.json" \
        --update-vk-large-file-path "$FIXTURE_DIR/vk-update-large.json" \
        | tr -d '\n'
)"

if [ -z "$CONTRACT_ID" ]; then
    echo "failed to capture deployed contract id" >&2
    exit 1
fi

echo
echo "Anarchy contract deployed."
echo "Contract ID: $CONTRACT_ID"
echo "Stellar config dir: $CONFIG_DIR"
echo "Temporary artifacts dir: $WORK_DIR"
echo
# TODO(phase-d): post-deploy smoke (create_group → update_commitment →
# verify_membership → deactivate_group); not blocked on Phase A circuit
# work since Anarchy reuses the existing keyset-v2 circuits and v1
# proof-generation paths in src/prover. Adding the smoke-test block
# is bounded by Phase D client routing.

if [ "$KEEP_ARTIFACTS" = "1" ]; then
    trap - EXIT INT TERM
fi
