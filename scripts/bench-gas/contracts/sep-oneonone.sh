#!/usr/bin/env bash
# sep-oneonone gas bench driver.
#
# Coverage (V1):
#   deploy, create_group (committed fixture), set_restricted_mode,
#   bump_group_ttl.
#
# Out of scope (V2):
#   verify_membership — needs a fresh membership proof matching the
#   create-fixture's commitment, but we don't have the witness used
#   to generate `oneonone-create-proof.bin`. Tracked as a follow-up.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
LIB="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)/lib.sh"
# shellcheck source=../lib.sh
. "$LIB"

REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../../.." && pwd)"
FIXTURE_DIR="$REPO_ROOT/contracts/plonk-verifier/tests/fixtures"

export BENCH_CURRENT_CONTRACT="sep-oneonone"

# ---- precompute hex encodings (no temp files; CLI takes hex inline) ----
CREATE_PROOF_HEX="$(bin_hex "$FIXTURE_DIR/oneonone-create-proof.bin")"
CREATE_PI_JSON="$(pi_concat_json_array "$FIXTURE_DIR/oneonone-create-pi.bin" 2)"
CREATE_COMMITMENT_HEX="$(read_pi_field_hex "$FIXTURE_DIR/oneonone-create-pi.bin" 0)"
GROUP_ID_HEX="$(printf '42%.0s' $(seq 1 32))"

echo "==> [$BENCH_CURRENT_CONTRACT] deploy"
CID="$(bench_deploy \
    "bench-gas-oneonone" \
    "$BENCH_ARTIFACT_DIR/sep_oneonone_contract.wasm" \
    --admin "$BENCH_DEPLOYER_ADDRESS")"

if [ -z "$CID" ]; then
    echo "    deploy failed — skipping further ops" >&2
    exit 0
fi
echo "    contract: $CID"

echo "==> [$BENCH_CURRENT_CONTRACT] create_group"
bench_invoke "$CID" "create_group" "n/a" "create_group" \
    --caller "$BENCH_DEPLOYER_ADDRESS" \
    --group-id "$GROUP_ID_HEX" \
    --commitment "$CREATE_COMMITMENT_HEX" \
    --proof "$CREATE_PROOF_HEX" \
    --public-inputs "$CREATE_PI_JSON"

echo "==> [$BENCH_CURRENT_CONTRACT] set_restricted_mode(true)"
bench_invoke "$CID" "set_restricted_mode" "n/a" "set_restricted_mode" \
    --restricted true

echo "==> [$BENCH_CURRENT_CONTRACT] bump_group_ttl"
bench_invoke "$CID" "bump_group_ttl" "n/a" "bump_group_ttl" \
    --group-id "$GROUP_ID_HEX"

# Read-only entrypoints (`get_commitment`) are skipped — the CLI
# short-circuits to local simulation regardless of `--send yes`, so
# no tx is submitted and no fee is charged.

echo "==> [$BENCH_CURRENT_CONTRACT] done"
