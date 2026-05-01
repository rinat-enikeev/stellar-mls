#!/usr/bin/env bash
# sep-democracy gas bench driver.
#
# V1 coverage: deploy, set_restricted_mode.
# V2 (deferred): create_group + verify_membership via fresh
#   gen-membership-proof; update_commitment via committed
#   democracy-update-proof-d{5,8,11}.bin in revert mode.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
LIB="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)/lib.sh"
# shellcheck source=../lib.sh
. "$LIB"

export BENCH_CURRENT_CONTRACT="sep-democracy"

echo "==> [$BENCH_CURRENT_CONTRACT] deploy"
CID="$(bench_deploy \
    "bench-gas-democracy" \
    "$BENCH_ARTIFACT_DIR/sep_democracy_contract.wasm" \
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
