#!/usr/bin/env bash
# sep-oneonone gas bench driver.
#
# Coverage (V1):
#   deploy, create_group (committed fixture), set_restricted_mode,
#   bump_group_ttl, get_commitment.
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

WORK="$(mktemp -d "${TMPDIR:-/tmp}/bench-gas-oneonone.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT INT TERM

# ---- encode fixtures into CLI-ready JSON files ----
# Constructor: (admin: Address) — no fixture args.
#
# create_group(caller, group_id, commitment, proof: BytesN<1601>,
#              public_inputs: Vec<BytesN<32>>)
# PI layout (2 fields × 32 bytes): [commitment, be32(epoch=0)]
# Total file size: 64 bytes.
bin_to_hex_json "$FIXTURE_DIR/oneonone-create-proof.bin" "$WORK/create-proof.json"
pi_concat_to_json "$FIXTURE_DIR/oneonone-create-pi.bin" 2 "$WORK/create-pi.json"

CREATE_COMMITMENT_HEX="$(read_pi_field_hex "$FIXTURE_DIR/oneonone-create-pi.bin" 0)"
hex_to_json "$CREATE_COMMITMENT_HEX" "$WORK/create-commitment.json"

GROUP_ID_HEX="$(printf '42%.0s' $(seq 1 32))"
hex_to_json "$GROUP_ID_HEX" "$WORK/group-id.json"

# ---- deploy ----
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

# ---- create_group ----
echo "==> [$BENCH_CURRENT_CONTRACT] create_group"
bench_invoke "$CID" "create_group" "n/a" "create_group" \
    --caller "$BENCH_DEPLOYER_ADDRESS" \
    --group-id-file-path "$WORK/group-id.json" \
    --commitment-file-path "$WORK/create-commitment.json" \
    --proof-file-path "$WORK/create-proof.json" \
    --public-inputs-file-path "$WORK/create-pi.json"

# ---- set_restricted_mode (admin op, on then off) ----
echo "==> [$BENCH_CURRENT_CONTRACT] set_restricted_mode(true)"
bench_invoke "$CID" "set_restricted_mode" "n/a" "set_restricted_mode" \
    --restricted true

# ---- bump_group_ttl ----
echo "==> [$BENCH_CURRENT_CONTRACT] bump_group_ttl"
bench_invoke "$CID" "bump_group_ttl" "n/a" "bump_group_ttl" \
    --group-id-file-path "$WORK/group-id.json"

# ---- get_commitment (read; submitted to capture the on-chain fee) ----
echo "==> [$BENCH_CURRENT_CONTRACT] get_commitment"
bench_invoke "$CID" "get_commitment" "n/a" "get_commitment" \
    --group-id-file-path "$WORK/group-id.json"

echo "==> [$BENCH_CURRENT_CONTRACT] done"
