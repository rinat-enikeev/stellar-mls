#!/bin/sh
#
# Install the AdminUpdate VK on an already-upgraded testnet contract —
# Phase 3 of issue #78.
#
# Per design doc §6.4.4, AdminUpdateCircuit is shape-identical to
# UpdateCircuit (same IC length = 4, same constraint system). No new
# ceremony is needed — we reuse the existing keyset-v2 UpdateCircuit VK
# under a new dispatcher slot (VkKind::AdminUpdate). Tier is ignored for
# AdminUpdate at the contract level (single VK, not per-tier), so we
# pass tier=0 by convention.
#
# Reads the target contract ID and deployer alias from relayer/.env —
# same env contract as install-democracy-vks-testnet.sh:
#   RELAYER_CONTRACT_ID=C...  # public address, already present
#   DEPLOYER=<alias>          # stellar keys alias OR raw S-secret for
#                             # the contract's stored admin
#
# CONTRACT_ID, if explicitly set, takes precedence over RELAYER_CONTRACT_ID.
#
# Usage:
#   ./scripts/install-adminupdate-vk-testnet.sh            # install on testnet
#   NETWORK=futurenet ./scripts/install-adminupdate-vk-testnet.sh
#   DRY_RUN=1 ./scripts/install-adminupdate-vk-testnet.sh  # print command only
#
# Exit codes: 0 on success, non-zero on env/validation/invoke failure.
#
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

NETWORK="${NETWORK:-testnet}"
ENV_FILE="${ENV_FILE:-$REPO_ROOT/relayer/.env}"
DRY_RUN="${DRY_RUN:-0}"

# AdminUpdate reuses the existing keyset-v2 UpdateCircuit VK. Any tier is
# fine (AdminUpdate ignores tier); we use the small VK because it is the
# smallest wire footprint — the three tiers share the same constraint
# system so the pairing checks are identical.
VK_PATH="${VK_PATH:-$REPO_ROOT/keyset-v2/vk-update-small.json}"

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

# Source the env file in a subshell-safe way.
set -a
# shellcheck disable=SC1090
. "$ENV_FILE"
set +a

# Prefer an explicit CONTRACT_ID override; otherwise fall back to
# RELAYER_CONTRACT_ID (same public address, no duplicate var needed).
: "${CONTRACT_ID:=${RELAYER_CONTRACT_ID:-}}"

[ -n "$CONTRACT_ID" ]  || die "neither CONTRACT_ID nor RELAYER_CONTRACT_ID set in $ENV_FILE"
[ -n "${DEPLOYER:-}" ] || die "DEPLOYER not set in $ENV_FILE"

case "$CONTRACT_ID" in
    C*) ;;
    *) die "contract ID does not look like a Stellar contract ID (must start with C): $CONTRACT_ID" ;;
esac

[ -f "$VK_PATH" ] || die "missing VK file: $VK_PATH"

# Confirm the deployer alias (or secret) is resolvable by the Stellar CLI
# before we send any transaction.
if ! stellar keys public-key "$DEPLOYER" >/dev/null 2>&1; then
    die "stellar keys alias '$DEPLOYER' not found. Run 'stellar keys ls' to check."
fi

DEPLOYER_ADDR="$(stellar keys public-key "$DEPLOYER" | tr -d '\n')"

echo "==> AdminUpdate VK install"
echo "    Contract:   $CONTRACT_ID"
echo "    Network:    $NETWORK"
echo "    Deployer:   $DEPLOYER ($DEPLOYER_ADDR)"
echo "    VK:         $VK_PATH (reused from keyset-v2 UpdateCircuit)"
echo

# Preflight: simulate update_vk(kind=AdminUpdate) to surface spec/ABI
# problems early (e.g. contract predates PR #77 and has no AdminUpdate
# variant). As in install-democracy-vks-testnet.sh, this does NOT verify
# admin match — Soroban simulation reports auth requirements rather
# than rejecting the call. The actual admin check happens at real
# submit time and is parsed inside install_adminupdate_vk().
if [ "$DRY_RUN" != "1" ]; then
    echo "==> Preflight: simulate update_vk(kind=AdminUpdate) to catch ABI/spec errors"
    if preflight_err="$(
        stellar contract invoke \
            --id "$CONTRACT_ID" \
            --source-account "$DEPLOYER" \
            --network "$NETWORK" \
            --send no \
            -- update_vk \
            --kind '"AdminUpdate"' \
            --tier 0 \
            --new-vk-file-path "$VK_PATH" 2>&1 >/dev/null
    )"; then
        echo "    ok — contract ABI recognises AdminUpdate"
    else
        # Spec-mismatch: contract predates PR #77.
        if printf '%s' "$preflight_err" | grep -qi 'Unknown case AdminUpdate\|Failed to parse argument .*kind'; then
            printf '%s\n' "$preflight_err" >&2
            die "contract $CONTRACT_ID does not recognise VkKind::AdminUpdate — it predates PR #77. Deploy/upgrade the contract first (see Phase 1 of issue #78)."
        fi
        printf '%s\n' "$preflight_err" >&2
        die "preflight failed (see above). Cannot proceed."
    fi
    echo
fi

install_adminupdate_vk() {
    echo "==> update_vk(kind=AdminUpdate, tier=0)"
    set -- \
        stellar contract invoke \
            --id "$CONTRACT_ID" \
            --source-account "$DEPLOYER" \
            --network "$NETWORK" \
            -- update_vk \
            --kind '"AdminUpdate"' \
            --tier 0 \
            --new-vk-file-path "$VK_PATH"

    if [ "$DRY_RUN" = "1" ]; then
        printf '    (dry-run)'
        for arg in "$@"; do printf ' %s' "$arg"; done
        printf '\n'
        return 0
    fi

    # Translate the generic "Missing signing key for account G…" CLI
    # error into an actionable admin-mismatch message that names the
    # required address — matches the install-democracy-vks script.
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

install_adminupdate_vk

echo
echo "AdminUpdate VK installed. Smoke-test against an Oligarchy group with"
echo "update_admin_commitment + a bogus proof — expect error #11 InvalidProof"
echo "(NOT #7 InvalidVKSetup, which would mean the slot is still empty)."
