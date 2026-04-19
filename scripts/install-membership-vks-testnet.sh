#!/bin/sh
#
# Re-install the ceremony Membership + Update VKs (keyset-v2) on a testnet
# contract that was deployed with deterministic fixture VKs by
# scripts/deploy_sep_xxxx_testnet.sh.
#
# Background: deploy_sep_xxxx_testnet.sh initializes the contract from the
# output of `generate_contract_testnet_fixtures`, which seeds VK generation
# with hardcoded RNG seeds (1001/1002/1003/2001/2002/2003). Those VKs do
# NOT match the OsRng ceremony proving keys that clients bundle from
# keyset-v2/. Result: every client-generated proof fails verification
# with error #7 InvalidProof. This script swaps the 6 fixture VKs on the
# live contract for the real ceremony VKs so client proofs verify.
#
# Reads the target contract ID and deployer alias from relayer/.env:
#   RELAYER_CONTRACT_ID=C...  # public contract address
#   DEPLOYER=<alias>          # stellar keys alias for the stored admin
#
# CONTRACT_ID, if explicitly set, takes precedence over RELAYER_CONTRACT_ID.
#
# Usage:
#   ./scripts/install-membership-vks-testnet.sh            # install on testnet
#   NETWORK=futurenet ./scripts/install-membership-vks-testnet.sh
#   DRY_RUN=1 ./scripts/install-membership-vks-testnet.sh  # print commands only
#
# Exit codes: 0 on success, non-zero on env/validation/invoke failure.
#
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

NETWORK="${NETWORK:-testnet}"
ENV_FILE="${ENV_FILE:-$REPO_ROOT/relayer/.env}"
DRY_RUN="${DRY_RUN:-0}"

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

# Reject mainnet — keyset-v2 here is the OsRng single-trusted-party keyset,
# acceptable for testnet but not for mainnet (mainnet requires MPC-ceremony
# VKs installed via scripts/deploy-mainnet.sh).
case "$NETWORK" in
    mainnet|public) die "this script targets testnet keyset-v2 VKs — refusing to target $NETWORK" ;;
esac

VK_DIR="$REPO_ROOT/keyset-v2"
VK_M_SMALL="$VK_DIR/vk-small.json"
VK_M_MEDIUM="$VK_DIR/vk-medium.json"
VK_M_LARGE="$VK_DIR/vk-large.json"
VK_U_SMALL="$VK_DIR/vk-update-small.json"
VK_U_MEDIUM="$VK_DIR/vk-update-medium.json"
VK_U_LARGE="$VK_DIR/vk-update-large.json"

require_cmd stellar

[ -f "$ENV_FILE" ] || die "env file not found: $ENV_FILE"

set -a
# shellcheck disable=SC1090
. "$ENV_FILE"
set +a

: "${CONTRACT_ID:=${RELAYER_CONTRACT_ID:-}}"

[ -n "$CONTRACT_ID" ]  || die "neither CONTRACT_ID nor RELAYER_CONTRACT_ID set in $ENV_FILE"
[ -n "${DEPLOYER:-}" ] || die "DEPLOYER not set in $ENV_FILE"

case "$CONTRACT_ID" in
    C*) ;;
    *) die "contract ID does not look like a Stellar contract ID (must start with C): $CONTRACT_ID" ;;
esac

for f in "$VK_M_SMALL" "$VK_M_MEDIUM" "$VK_M_LARGE" \
         "$VK_U_SMALL" "$VK_U_MEDIUM" "$VK_U_LARGE"; do
    [ -f "$f" ] || die "missing VK file: $f (ensure keyset-v2/ is populated)"
done

if ! stellar keys public-key "$DEPLOYER" >/dev/null 2>&1; then
    die "stellar keys alias '$DEPLOYER' not found. Run 'stellar keys ls' to check."
fi

DEPLOYER_ADDR="$(stellar keys public-key "$DEPLOYER" | tr -d '\n')"

echo "==> Membership + Update VK install (ceremony keyset-v2 over fixture VKs)"
echo "    Contract:     $CONTRACT_ID"
echo "    Network:      $NETWORK"
echo "    Deployer:     $DEPLOYER ($DEPLOYER_ADDR)"
echo "    VK dir:       $VK_DIR"
echo

install_vk() {
    kind_json="$1"
    kind_label="$2"
    tier="$3"
    vk_path="$4"

    echo "==> update_vk(kind=$kind_label, tier=$tier)"
    set -- \
        stellar contract invoke \
            --id "$CONTRACT_ID" \
            --source-account "$DEPLOYER" \
            --network "$NETWORK" \
            -- update_vk \
            --kind "$kind_json" \
            --tier "$tier" \
            --new-vk-file-path "$vk_path"

    if [ "$DRY_RUN" = "1" ]; then
        printf '    (dry-run)'
        for arg in "$@"; do printf ' %s' "$arg"; done
        printf '\n'
        return 0
    fi

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

install_vk '"Membership"' "Membership" 0 "$VK_M_SMALL"
install_vk '"Membership"' "Membership" 1 "$VK_M_MEDIUM"
install_vk '"Membership"' "Membership" 2 "$VK_M_LARGE"
install_vk '"Update"'     "Update"     0 "$VK_U_SMALL"
install_vk '"Update"'     "Update"     1 "$VK_U_MEDIUM"
install_vk '"Update"'     "Update"     2 "$VK_U_LARGE"

echo
echo "All 6 Membership + Update VKs rotated to keyset-v2 ceremony keys."
echo "Smoke-test: create_group_v2 from the iOS/Android client should now"
echo "succeed (was failing with #7 InvalidProof against fixture VKs)."
