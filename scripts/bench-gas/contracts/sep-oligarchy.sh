#!/usr/bin/env bash
# sep-oligarchy gas bench driver.
#
# Coverage (V1):
#   deploy, create_oligarchy_group (committed fixture, tier 0),
#   verify_membership (revert-mode against post-create state),
#   update_commitment (revert-mode against post-create state),
#   set_restricted_mode, bump_group_ttl, get_commitment.
#
# Bench mechanics:
#   * verify_membership returns Ok(false) on InvalidProof (no revert),
#     so the captured fee equals the real success-path cost — the
#     verifier runs the full pairing check identically in both arms.
#   * update_commitment errors out on InvalidProof, so the tx reverts
#     after the verifier. The fee misses the post-verify storage
#     writes (history archive + new entry + TTL bumps), which PR #206
#     measured at ~75K CPU / ~75KB mem (≈1% of total).

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
LIB="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)/lib.sh"
# shellcheck source=../lib.sh
. "$LIB"

REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../../.." && pwd)"
FIXTURE_DIR="$REPO_ROOT/contracts/plonk-verifier/tests/fixtures"

export BENCH_CURRENT_CONTRACT="sep-oligarchy"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/bench-gas-oligarchy.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT INT TERM

# ---- encode fixtures ----
# create PI: 6 fields × 32 = 192 bytes. PI = [commitment, be32(0),
#     occ, admin_pubkey_commitment, group_id_fr, ?]; only PI[0..3]
#     are validated against wire args.
bin_to_hex_json "$FIXTURE_DIR/oligarchy-create-proof.bin"  "$WORK/create-proof.json"
pi_concat_to_json "$FIXTURE_DIR/oligarchy-create-pi.bin" 6 "$WORK/create-pi.json"

CREATE_COMMITMENT_HEX="$(read_pi_field_hex "$FIXTURE_DIR/oligarchy-create-pi.bin" 0)"
CREATE_OCC_HEX="$(read_pi_field_hex         "$FIXTURE_DIR/oligarchy-create-pi.bin" 2)"
hex_to_json "$CREATE_COMMITMENT_HEX" "$WORK/create-commitment.json"
hex_to_json "$CREATE_OCC_HEX"        "$WORK/create-occ.json"

GROUP_ID_HEX="$(printf '07%.0s' $(seq 1 32))"
hex_to_json "$GROUP_ID_HEX" "$WORK/group-id.json"

# verify-membership PI (revert-mode): [state.commitment, be32(0)]
ZERO32_HEX="$(printf '00%.0s' $(seq 1 32))"
{
    printf '['
    printf '"%s",' "$CREATE_COMMITMENT_HEX"
    printf '"%s"'  "$ZERO32_HEX"
    printf ']'
} > "$WORK/verify-pi.json"

# update_commitment PI (revert-mode): [c_old, be32(0), c_new, occ_old,
#     occ_new, be32(threshold=1)]. c_new + occ_new are arbitrary
#     canonical Fr (we use 0...01 and 0...02).
ONE32_HEX="$(printf '%062d01' 0)"   # 32 bytes, last byte = 0x01
TWO32_HEX="$(printf '%062d02' 0)"
THRESHOLD_HEX="$(printf '%062d01' 0)"   # be32(1)
{
    printf '['
    printf '"%s",' "$CREATE_COMMITMENT_HEX"
    printf '"%s",' "$ZERO32_HEX"
    printf '"%s",' "$ONE32_HEX"
    printf '"%s",' "$CREATE_OCC_HEX"
    printf '"%s",' "$TWO32_HEX"
    printf '"%s"'  "$THRESHOLD_HEX"
    printf ']'
} > "$WORK/update-pi.json"

# ---- deploy ----
echo "==> [$BENCH_CURRENT_CONTRACT] deploy"
CID="$(bench_deploy \
    "bench-gas-oligarchy" \
    "$BENCH_ARTIFACT_DIR/sep_oligarchy_contract.wasm" \
    --admin "$BENCH_DEPLOYER_ADDRESS")"

if [ -z "$CID" ]; then
    echo "    deploy failed — skipping further ops" >&2
    exit 0
fi
echo "    contract: $CID"

# ---- create_oligarchy_group (tier 0 from canonical fixture) ----
echo "==> [$BENCH_CURRENT_CONTRACT] create_oligarchy_group(tier=0)"
bench_invoke "$CID" "create_oligarchy_group" "0" "create_oligarchy_group" \
    --caller "$BENCH_DEPLOYER_ADDRESS" \
    --group-id-file-path "$WORK/group-id.json" \
    --commitment-file-path "$WORK/create-commitment.json" \
    --member-tier 0 \
    --admin-threshold-numerator 1 \
    --occupancy-commitment-initial-file-path "$WORK/create-occ.json" \
    --proof-file-path "$WORK/create-proof.json" \
    --public-inputs-file-path "$WORK/create-pi.json"

# ---- verify_membership (revert-mode; state matches, proof is well-
#      formed but doesn't verify; verifier runs full pairing) ----
echo "==> [$BENCH_CURRENT_CONTRACT] verify_membership(tier=0, revert-mode)"
bench_invoke "$CID" "verify_membership" "0" "verify_membership" \
    --group-id-file-path "$WORK/group-id.json" \
    --proof-file-path "$WORK/create-proof.json" \
    --public-inputs-file-path "$WORK/verify-pi.json"

# ---- update_commitment (revert-mode) ----
echo "==> [$BENCH_CURRENT_CONTRACT] update_commitment(revert-mode)"
bench_invoke "$CID" "update_commitment" "0" "update_commitment" \
    --group-id-file-path "$WORK/group-id.json" \
    --proof-file-path "$WORK/create-proof.json" \
    --public-inputs-file-path "$WORK/update-pi.json"

# ---- admin / utility ops ----
echo "==> [$BENCH_CURRENT_CONTRACT] set_restricted_mode(true)"
bench_invoke "$CID" "set_restricted_mode" "n/a" "set_restricted_mode" \
    --restricted true

echo "==> [$BENCH_CURRENT_CONTRACT] bump_group_ttl"
bench_invoke "$CID" "bump_group_ttl" "n/a" "bump_group_ttl" \
    --group-id-file-path "$WORK/group-id.json"

echo "==> [$BENCH_CURRENT_CONTRACT] get_commitment"
bench_invoke "$CID" "get_commitment" "n/a" "get_commitment" \
    --group-id-file-path "$WORK/group-id.json"

echo "==> [$BENCH_CURRENT_CONTRACT] done"
