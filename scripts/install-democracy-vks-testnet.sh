#!/bin/sh
#
# Install the dev Democracy VKs (UpdateByType(2) for tiers 0 and 1) on an
# already-upgraded testnet contract — Phase 2 of issue #78.
#
# Reads the target contract ID and deployer alias from relayer/.env. Expects:
#   RELAYER_CONTRACT_ID=C...  # already present — same address as CONTRACT_ID
#                             # (Stellar contract IDs are public, not secret)
#   DEPLOYER=<alias>          # stellar keys alias holding the stored admin secret
#
# CONTRACT_ID, if explicitly set, takes precedence over RELAYER_CONTRACT_ID.
#
# The script invokes `update_vk` twice against the in-repo VK JSONs committed
# at keyset-democracy-dev/vk-democracy-DEV-*.json.
#
# Usage:
#   ./scripts/install-democracy-vks-testnet.sh            # install on testnet
#   NETWORK=futurenet ./scripts/install-democracy-vks-testnet.sh
#   DRY_RUN=1 ./scripts/install-democracy-vks-testnet.sh  # print commands only
#
# Exit codes: 0 on success, non-zero on env/validation/invoke failure.
#
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

NETWORK="${NETWORK:-testnet}"
ENV_FILE="${ENV_FILE:-$REPO_ROOT/relayer/.env}"
DRY_RUN="${DRY_RUN:-0}"

# V-D1: Reject mainnet — this script uses dev VKs whose proving keys
# are publicly reproducible.  Production VK installation requires a
# dedicated mainnet script with ceremony-verified keys.
case "$NETWORK" in
    mainnet|public) die "this script installs DEV verification keys — refusing to target $NETWORK" ;;
esac

VK_DIR="$REPO_ROOT/keyset-democracy-dev"
VK_TIER_0="$VK_DIR/vk-democracy-DEV-0-32.json"
VK_TIER_1="$VK_DIR/vk-democracy-DEV-1-256.json"

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "missing required command: $1" >&2
        exit 1
    fi
}

die() {
    echo "error: $1" >&2
    exit 1
}

require_cmd stellar

[ -f "$ENV_FILE" ] || die "env file not found: $ENV_FILE"

# Source the env file — this executes in the current shell, so a
# compromised .env can inject arbitrary commands.  Acceptable here
# because the operator controls the file (V-D2).
set -a
# shellcheck disable=SC1090
. "$ENV_FILE"
set +a

# Prefer an explicit CONTRACT_ID override; otherwise fall back to the
# RELAYER_CONTRACT_ID already stored in relayer/.env — they are the same
# public contract address, so requiring the operator to duplicate the value
# just invites drift.
: "${CONTRACT_ID:=${RELAYER_CONTRACT_ID:-}}"

[ -n "$CONTRACT_ID" ]  || die "neither CONTRACT_ID nor RELAYER_CONTRACT_ID set in $ENV_FILE"
[ -n "${DEPLOYER:-}" ] || die "DEPLOYER not set in $ENV_FILE"

case "$CONTRACT_ID" in
    C*) ;;
    *) die "contract ID does not look like a Stellar contract ID (must start with C): $CONTRACT_ID" ;;
esac

[ -f "$VK_TIER_0" ] || die "missing VK file: $VK_TIER_0 (run ./scripts/generate-democracy-vk-dev.sh)"
[ -f "$VK_TIER_1" ] || die "missing VK file: $VK_TIER_1 (run ./scripts/generate-democracy-vk-dev.sh)"

# Confirm the deployer alias is resolvable by the Stellar CLI before we send
# any transaction; otherwise stellar's error message points at --source-account
# which is less useful than saying "alias missing".
if ! stellar keys public-key "$DEPLOYER" >/dev/null 2>&1; then
    die "stellar keys alias '$DEPLOYER' not found. Run 'stellar keys ls' to check."
fi

DEPLOYER_ADDR="$(stellar keys public-key "$DEPLOYER" | tr -d '\n')"

echo "==> Democracy VK install"
echo "    Contract:   $CONTRACT_ID"
echo "    Network:    $NETWORK"
echo "    Deployer:   $DEPLOYER ($DEPLOYER_ADDR)"
echo "    VK tier 0:  $VK_TIER_0"
echo "    VK tier 1:  $VK_TIER_1"
echo

# Preflight: simulate update_vk(tier=0) to surface spec/ABI problems early
# (e.g. contract predates PR #77 and has no UpdateByType variant). This
# does NOT verify admin match — Soroban simulation *reports* the auth
# requirements rather than rejecting the call, so a simulation that
# returns `null` only proves "the tx would be valid if the right keys
# sign", not "this source IS the admin". The actual admin check happens
# at real submit time and is handled inside install_vk().
if [ "$DRY_RUN" != "1" ]; then
    echo "==> Preflight: simulate update_vk(tier=0) to catch ABI/spec errors"
    if preflight_err="$(
        stellar contract invoke \
            --id "$CONTRACT_ID" \
            --source-account "$DEPLOYER" \
            --network "$NETWORK" \
            --send no \
            -- update_vk \
            --kind '{"UpdateByType":2}' \
            --tier 0 \
            --new-vk-file-path "$VK_TIER_0" 2>&1 >/dev/null
    )"; then
        echo "    ok — contract ABI recognises UpdateByType"
    else
        # Spec-mismatch: contract predates PR #77.
        if printf '%s' "$preflight_err" | grep -qi 'Unknown case UpdateByType\|Failed to parse argument .*kind'; then
            printf '%s\n' "$preflight_err" >&2
            die "contract $CONTRACT_ID does not recognise VkKind::UpdateByType — it predates PR #77. Deploy/upgrade the contract first (see Phase 1 of issue #78)."
        fi
        printf '%s\n' "$preflight_err" >&2
        die "preflight failed (see above). Cannot proceed."
    fi
    echo
fi

install_vk() {
    tier="$1"
    vk_path="$2"

    echo "==> update_vk(kind=UpdateByType(2), tier=$tier)"
    set -- \
        stellar contract invoke \
            --id "$CONTRACT_ID" \
            --source-account "$DEPLOYER" \
            --network "$NETWORK" \
            -- update_vk \
            --kind '{"UpdateByType":2}' \
            --tier "$tier" \
            --new-vk-file-path "$vk_path"

    if [ "$DRY_RUN" = "1" ]; then
        printf '    (dry-run)'
        for arg in "$@"; do printf ' %s' "$arg"; done
        printf '\n'
        return 0
    fi

    # Capture stderr so we can translate the generic "Missing signing key
    # for account G…" error into an actionable "admin mismatch" message
    # that names the required address.
    if invoke_err="$("$@" 2>&1 >/dev/null)"; then
        return 0
    fi
    printf '%s\n' "$invoke_err" >&2
    required_admin="$(printf '%s' "$invoke_err" | sed -n 's/.*Missing signing key for account \(G[A-Z0-9]*\).*/\1/p' | head -n1)"
    if [ -n "$required_admin" ]; then
        cat >&2 <<EOF

admin mismatch: --source-account $DEPLOYER ($DEPLOYER_ADDR) is NOT the contract's stored admin.
The contract at $CONTRACT_ID requires signature from $required_admin.
Re-run with DEPLOYER= set to the stellar keys alias (or raw S-secret) for that address.
EOF
        exit 1
    fi
    exit 1
}

install_vk 0 "$VK_TIER_0"
install_vk 1 "$VK_TIER_1"

echo
echo "Both Democracy VKs installed. Smoke-test with a bogus-proof"
echo "update_commitment on a Democracy group — expect error #11 InvalidProof"
echo "(NOT #7 InvalidVKSetup, which would mean the slot is still empty)."
