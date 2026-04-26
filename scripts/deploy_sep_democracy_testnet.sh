#!/bin/sh
set -eu

# Deploy the per-type Democracy Soroban contract to Stellar testnet.
#
# Sibling of scripts/deploy_sep_xxxx_testnet.sh, scoped to the
# Democracy-only contract at contracts/sep-democracy/. Per
# docs/contract/democracy/impl_plan.md §E and design v0.5.1 §4.6
# (contract redeploy + VK install).
#
# Scope of this script (PR landing the contract):
#   * Build sep_democracy_contract.wasm
#   * Deploy with admin + per-tier membership VKs + per-tier
#     v2 democracy update VKs
#   * Surface the deployed contract address
#
# Out of scope (depends on design §6 Phase A — democracy_v2 circuit
# + prover landing first):
#   * Generating real Groth16 v2 fixtures (proofs against the v2
#     occupancy-commitment circuit). The v1 fixture generator at
#     `Cargo.toml#[[bin]] generate_contract_testnet_fixtures` produces
#     Anarchy-shaped proofs that don't match the 7-IC-point Democracy
#     update VK. A new generator (or a flag on the existing one) is
#     needed before this script can exercise the full
#     create_group → update_commitment → verify_membership → deactivate
#     cycle. Today the script smoke-tests by deploying only.
#   * VK files at $FIXTURE_DIR. Until Phase A produces the v2 VK
#     bytes, this script accepts FIXTURE_DIR as an env var pointing
#     at pre-generated v2 dev VKs (see design §4.6 dev-VK install
#     pattern).

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

NETWORK="${NETWORK:-testnet}"
IDENTITY="${IDENTITY:-sep-democracy-testnet-deployer}"
ALIAS="${ALIAS:-sep-democracy-testnet}"
KEEP_ARTIFACTS="${KEEP_ARTIFACTS:-0}"

# PERSIST_IDENTITY=1 reuses the user's global stellar keystore at
# ~/.config/stellar so the contract can receive admin-gated calls
# (update_vk for VK rotation per design §4.6) after this script exits.
# Default mode is ephemeral self-contained smoke deploy.
PERSIST_IDENTITY="${PERSIST_IDENTITY:-0}"

if [ "$PERSIST_IDENTITY" = "1" ]; then
    CONFIG_DIR="${CONFIG_DIR:-${STELLAR_CONFIG_DIR:-$HOME/.config/stellar}}"
else
    CONFIG_DIR="${CONFIG_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sep-democracy-stellar-config.XXXXXX")}"
fi
WORK_DIR="${WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sep-democracy-testnet.XXXXXX")}"
ARTIFACT_DIR="$WORK_DIR/artifacts"

# FIXTURE_DIR: location of pre-generated v2 democracy VK files. Until
# Phase A's fixture generator lands, the operator supplies this dir.
# Expected layout:
#   $FIXTURE_DIR/vk-membership-small.json   (3 IC points)
#   $FIXTURE_DIR/vk-membership-medium.json
#   $FIXTURE_DIR/vk-membership-large.json
#   $FIXTURE_DIR/vk-update-small.json       (7 IC points — v2 democracy)
#   $FIXTURE_DIR/vk-update-medium.json
#   $FIXTURE_DIR/vk-update-large.json
FIXTURE_DIR="${FIXTURE_DIR:-$REPO_ROOT/keyset-democracy-dev}"

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

echo "==> Building Democracy Soroban contract"
mkdir -p "$ARTIFACT_DIR"
stellar contract build \
    --manifest-path "$REPO_ROOT/contracts/sep-democracy/Cargo.toml" \
    --out-dir "$ARTIFACT_DIR"

WASM_PATH="$ARTIFACT_DIR/sep_democracy_contract.wasm"
if [ ! -f "$WASM_PATH" ]; then
    echo "expected built wasm at $WASM_PATH" >&2
    exit 1
fi

if [ ! -d "$FIXTURE_DIR" ]; then
    echo "FIXTURE_DIR=$FIXTURE_DIR does not exist; supply v2 democracy dev VKs (design §4.6)" >&2
    exit 1
fi
for f in vk-membership-small.json vk-membership-medium.json vk-membership-large.json \
         vk-update-small.json vk-update-medium.json vk-update-large.json; do
    if [ ! -f "$FIXTURE_DIR/$f" ]; then
        echo "missing $FIXTURE_DIR/$f" >&2
        echo "  Phase A's fixture generator hasn't shipped yet. Until then, supply the v2 dev VKs manually" >&2
        echo "  per design §4.6 (contract redeploy + VK install) and §10.1 Q1/Q2 (Phase-A blockers)." >&2
        exit 1
    fi
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

echo "==> Funding deployer via Friendbot-backed testnet funding"
if ! stellar keys fund "$IDENTITY" --config-dir "$CONFIG_DIR" --network "$NETWORK" >/dev/null 2>&1; then
    if [ "$PERSIST_IDENTITY" = "1" ]; then
        echo "    (fund skipped — $IDENTITY likely already funded; continuing)"
    else
        stellar keys fund "$IDENTITY" --config-dir "$CONFIG_DIR" --network "$NETWORK" >/dev/null
    fi
fi

echo "==> Deploying contract to $NETWORK with constructor args (atomic deploy+init)"
CONTRACT_ID="$(
    stellar contract deploy \
        --config-dir "$CONFIG_DIR" \
        --network "$NETWORK" \
        --source-account "$IDENTITY" \
        --alias "$ALIAS" \
        --wasm "$WASM_PATH" \
        -- \
        --admin "$DEPLOYER_ADDRESS" \
        --vk-small-file-path "$FIXTURE_DIR/vk-membership-small.json" \
        --vk-medium-file-path "$FIXTURE_DIR/vk-membership-medium.json" \
        --vk-large-file-path "$FIXTURE_DIR/vk-membership-large.json" \
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
echo "Democracy contract deployed."
echo "Contract ID: $CONTRACT_ID"
echo "Stellar config dir: $CONFIG_DIR"
echo "Temporary artifacts dir: $WORK_DIR"
echo
echo "Next: smoke-test create_group → update_commitment → verify_membership → deactivate_group"
echo "      requires Phase A's prove_democracy_v2 + a v2 fixture generator (design §6 Phase A)."
echo "      Bake those, then drop in the post-deploy smoke-test block from"
echo "      scripts/deploy_sep_xxxx_testnet.sh as a follow-up."

if [ "$KEEP_ARTIFACTS" = "1" ]; then
    trap - EXIT INT TERM
fi
