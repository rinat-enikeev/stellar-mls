#!/usr/bin/env bash
# sep-tyranny gas bench driver.
#
# V1 coverage: deploy, set_restricted_mode.
# V2 (deferred): create_group, verify_membership, update_commitment —
#   each contract uses tier-keyed VKs (VK_CREATE_D{5,8,11},
#   VK_UPDATE_D{5,8,11}) with no committed fixture chain that survives
#   a fresh deploy. Need runtime proof generation OR per-tier
#   committed (proof, PI, group_id, admin_pubkey_commitment) bundles.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
LIB="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)/lib.sh"
# shellcheck source=../lib.sh
. "$LIB"

export BENCH_CURRENT_CONTRACT="sep-tyranny"

echo "==> [$BENCH_CURRENT_CONTRACT] deploy"
CID="$(bench_deploy \
    "bench-gas-tyranny" \
    "$BENCH_ARTIFACT_DIR/sep_tyranny_contract.wasm" \
    --admin "$BENCH_DEPLOYER_ADDRESS")"

if [ -z "$CID" ]; then
    echo "    deploy failed — skipping further ops" >&2
    exit 0
fi
echo "    contract: $CID"

echo "==> [$BENCH_CURRENT_CONTRACT] set_restricted_mode(true)"
bench_invoke "$CID" "set_restricted_mode" "n/a" "set_restricted_mode" \
    --restricted true

echo "==> [$BENCH_CURRENT_CONTRACT] done"
