#!/bin/bash
set -euo pipefail

# Deploy the per-type Oligarchy Soroban contract to Stellar testnet.
#
# Sibling of scripts/deploy_sep_democracy_testnet.sh, scoped to the
# Oligarchy-only contract at contracts/sep-oligarchy/. Per
# docs/contract/oligarchy/impl_plan.md and design v0.1.4 §4.6
# (contract redeploy + VK install).
#
# Shebang note: bash (not /bin/sh) so we can rely on `set -o pipefail`
# without dash-incompatibility worries. The deploy pipeline at the
# bottom captures `stellar contract deploy | tr -d '\n'` and would
# silently absorb `stellar`'s exit code under plain POSIX sh.
#
# Alias state note: this script registers `--alias $ALIAS` (default
# `sep-oligarchy-testnet`) inside CONFIG_DIR. When PERSIST_IDENTITY=1
# the alias survives between runs in $HOME/.config/stellar; iterating
# locally with the same alias may collide with stellar's prior-deploy
# bookkeeping. Override ALIAS or set PERSIST_IDENTITY=0 (default) for
# ephemeral runs.
#
# Scope of this script (PR landing the contract):
#   * Build sep_oligarchy_contract.wasm
#   * Deploy with admin + per-tier membership VKs + per-tier
#     create VKs + per-tier v0.1.4 oligarchy update VKs (9 VKs total)
#   * Surface the deployed contract address
#
# Out of scope (depends on design §6 Phase A — oligarchy_v2 circuit
# + prover landing first):
#   * Generating real Groth16 v0.1.4 fixtures (proofs against the
#     v0.1.4 two-tree occupancy-commitment circuit). The v1 fixture
#     generator at `Cargo.toml#[[bin]] generate_contract_testnet_fixtures`
#     produces Anarchy-shaped proofs that don't match the 7-IC-point
#     Oligarchy create / update VKs. A new generator (or a flag on the
#     existing one) is needed before this script can exercise the full
#     create_oligarchy_group → update_commitment round-trip. Today the
#     script smoke-tests by deploying only.
#   * VK files at $FIXTURE_DIR. Until Phase A4 produces the v0.1.4 VK
#     bytes, this script accepts FIXTURE_DIR as an env var pointing
#     at pre-generated v0.1.4 dev VKs (see design §4.6 dev-VK install
#     pattern).

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

NETWORK="${NETWORK:-testnet}"
IDENTITY="${IDENTITY:-sep-oligarchy-testnet-deployer}"
ALIAS="${ALIAS:-sep-oligarchy-testnet}"
KEEP_ARTIFACTS="${KEEP_ARTIFACTS:-0}"

# PERSIST_IDENTITY=1 reuses the user's global stellar keystore at
# ~/.config/stellar so the contract can receive admin-gated calls
# (update_vk for VK rotation per design §4.6) after this script exits.
# Default mode is ephemeral self-contained smoke deploy.
PERSIST_IDENTITY="${PERSIST_IDENTITY:-0}"

if [ "$PERSIST_IDENTITY" = "1" ]; then
    CONFIG_DIR="${CONFIG_DIR:-${STELLAR_CONFIG_DIR:-$HOME/.config/stellar}}"
else
    CONFIG_DIR="${CONFIG_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sep-oligarchy-stellar-config.XXXXXX")}"
fi
WORK_DIR="${WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sep-oligarchy-testnet.XXXXXX")}"
ARTIFACT_DIR="$WORK_DIR/artifacts"

# FIXTURE_DIR: location of pre-generated v0.1.4 oligarchy VK files.
# Until Phase A4's fixture generator lands, the operator supplies
# this dir.
# Expected layout (9 files):
#   $FIXTURE_DIR/vk-membership-small.json   (3 IC points)
#   $FIXTURE_DIR/vk-membership-medium.json
#   $FIXTURE_DIR/vk-membership-large.json
#   $FIXTURE_DIR/vk-create-small.json       (7 IC points — v0.1.4 §4.8)
#   $FIXTURE_DIR/vk-create-medium.json
#   $FIXTURE_DIR/vk-create-large.json
#   $FIXTURE_DIR/vk-update-small.json       (7 IC points — v0.1.4 update)
#   $FIXTURE_DIR/vk-update-medium.json
#   $FIXTURE_DIR/vk-update-large.json
FIXTURE_DIR="${FIXTURE_DIR:-$REPO_ROOT/keyset-oligarchy-dev}"

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

echo "==> Building Oligarchy Soroban contract"
mkdir -p "$ARTIFACT_DIR"
stellar contract build \
    --manifest-path "$REPO_ROOT/contracts/sep-oligarchy/Cargo.toml" \
    --out-dir "$ARTIFACT_DIR"

WASM_PATH="$ARTIFACT_DIR/sep_oligarchy_contract.wasm"
if [ ! -f "$WASM_PATH" ]; then
    echo "expected built wasm at $WASM_PATH" >&2
    exit 1
fi

if [ ! -d "$FIXTURE_DIR" ]; then
    echo "FIXTURE_DIR=$FIXTURE_DIR does not exist; supply v0.1.4 oligarchy dev VKs (design §4.6)" >&2
    exit 1
fi
# Pre-deploy IC-count validation: membership=3, create=7, update=7.
# A v1-shaped (4-IC-point) update VK would slip through to the
# contract's validate_vk_points and surface as an opaque
# Error(Contract, #9). Surface the mismatch with the actual filename
# instead.
expect_ic_count() {
    expected="$1"
    file="$2"
    # Guard: jq's `null | length` prints `null`, masking a structurally
    # invalid VK (no `.ic` key, or `.ic` not an array) as a mere count
    # mismatch. Reject those up-front with a distinct diagnostic.
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
        echo "  create VKs need 7 IC points (commitment, epoch, occupancy_commitment, member_root, admin_root, salt_initial + base)" >&2
        echo "  update VKs need 7 IC points (5 wire scalars + admin_threshold + base)" >&2
        echo "  v1-shaped files (4 IC points) will be rejected by the contract" >&2
        exit 1
    fi
}
for f in vk-membership-small.json vk-membership-medium.json vk-membership-large.json; do
    if [ ! -f "$FIXTURE_DIR/$f" ]; then
        echo "missing $FIXTURE_DIR/$f" >&2
        echo "  Phase A4's fixture generator hasn't shipped yet. Until then, supply the v0.1.4 dev VKs manually" >&2
        echo "  per design §4.6 (contract redeploy + VK install) and §10.1 Q1/Q2 (Phase-A blockers)." >&2
        exit 1
    fi
    expect_ic_count 3 "$FIXTURE_DIR/$f"
done
for f in vk-create-small.json vk-create-medium.json vk-create-large.json; do
    if [ ! -f "$FIXTURE_DIR/$f" ]; then
        echo "missing $FIXTURE_DIR/$f" >&2
        echo "  Phase A4's fixture generator hasn't shipped yet." >&2
        exit 1
    fi
    expect_ic_count 7 "$FIXTURE_DIR/$f"
done
for f in vk-update-small.json vk-update-medium.json vk-update-large.json; do
    if [ ! -f "$FIXTURE_DIR/$f" ]; then
        echo "missing $FIXTURE_DIR/$f" >&2
        echo "  Phase A4's fixture generator hasn't shipped yet." >&2
        exit 1
    fi
    expect_ic_count 7 "$FIXTURE_DIR/$f"
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
        # Persistent identity: most likely already funded, but if Friendbot
        # is throttled and the key is also under-funded, the deploy below
        # will fail with a confusing 'insufficient balance' or auth error.
        # Operators who hit that should retry after a Friendbot cooldown.
        echo "    fund attempted but Friendbot returned non-zero (possible throttle); deploy may fail on balance" >&2
        echo "    (likely cause: $IDENTITY is already funded under PERSIST_IDENTITY=1; if deploy fails on balance, retry after Friendbot cooldown)" >&2
    else
        stellar keys fund "$IDENTITY" --config-dir "$CONFIG_DIR" --network "$NETWORK" >/dev/null
    fi
fi

echo "==> Deploying contract to $NETWORK with constructor args (atomic deploy+init)"
# Constructor arg names below (--admin, --vk-small-file-path, …) come
# from `SepOligarchyContract::__constructor` parameter names in
# contracts/sep-oligarchy/src/lib.rs. If that signature changes, this
# block needs to track. The flag-style mapping is:
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
        --vk-small-file-path "$FIXTURE_DIR/vk-membership-small.json" \
        --vk-medium-file-path "$FIXTURE_DIR/vk-membership-medium.json" \
        --vk-large-file-path "$FIXTURE_DIR/vk-membership-large.json" \
        --create-vk-small-file-path "$FIXTURE_DIR/vk-create-small.json" \
        --create-vk-medium-file-path "$FIXTURE_DIR/vk-create-medium.json" \
        --create-vk-large-file-path "$FIXTURE_DIR/vk-create-large.json" \
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
echo "Oligarchy contract deployed."
echo "Contract ID: $CONTRACT_ID"
echo "Stellar config dir: $CONFIG_DIR"
echo "Temporary artifacts dir: $WORK_DIR"
echo
# TODO(phase-A4): post-deploy smoke (create_oligarchy_group → update_commitment → verify_membership → deactivate_group); blocked on prove_oligarchy_v2 + fixture generator (design §6 Phase A4)
echo "Next: smoke-test create_oligarchy_group → update_commitment → verify_membership → deactivate_group"
echo "      requires Phase A's prove_oligarchy_v2 + a v0.1.4 fixture generator (design §6 Phase A)."
echo "      Bake those, then drop in the post-deploy smoke-test block from"
echo "      scripts/deploy_sep_xxxx_testnet.sh as a follow-up."

if [ "$KEEP_ARTIFACTS" = "1" ]; then
    trap - EXIT INT TERM
fi
