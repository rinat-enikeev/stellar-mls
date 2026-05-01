#!/usr/bin/env bash
# sep-oligarchy gas bench driver.
#
# Coverage (V1):
#   deploy, create_oligarchy_group (committed fixture, tier 0),
#   verify_membership (revert-mode against post-create state),
#   update_commitment (revert-mode against post-create state),
#   set_restricted_mode, bump_group_ttl.
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

# ---- precompute hex encodings ----
# create PI: 6 fields × 32 = 192 bytes. PI = [commitment, be32(0),
#     occ, admin_pubkey_commitment, group_id_fr, ?]; only PI[0..3]
#     are validated against wire args.
CREATE_PROOF_HEX="$(bin_hex "$FIXTURE_DIR/oligarchy-create-proof.bin")"
CREATE_PI_JSON="$(pi_concat_json_array "$FIXTURE_DIR/oligarchy-create-pi.bin" 6)"
CREATE_COMMITMENT_HEX="$(read_pi_field_hex "$FIXTURE_DIR/oligarchy-create-pi.bin" 0)"
CREATE_OCC_HEX="$(read_pi_field_hex          "$FIXTURE_DIR/oligarchy-create-pi.bin" 2)"

GROUP_ID_HEX="$(printf '07%.0s' $(seq 1 32))"

# verify-membership PI (revert-mode): [state.commitment, be32(0)]
VERIFY_PI_JSON="[\"${CREATE_COMMITMENT_HEX}\",\"${ZERO32_HEX}\"]"

# update_commitment PI (revert-mode): [c_old, be32(0), c_new, occ_old,
#     occ_new, be32(threshold=1)]. c_new + occ_new are arbitrary
#     canonical Fr (32-byte scalars, last byte ≠ 0).
ONE32_HEX="${ZERO32_HEX:0:62}01"
TWO32_HEX="${ZERO32_HEX:0:62}02"
THRESHOLD_HEX="${ZERO32_HEX:0:62}01"
UPDATE_PI_JSON="[\"${CREATE_COMMITMENT_HEX}\",\"${ZERO32_HEX}\",\"${ONE32_HEX}\",\"${CREATE_OCC_HEX}\",\"${TWO32_HEX}\",\"${THRESHOLD_HEX}\"]"

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

echo "==> [$BENCH_CURRENT_CONTRACT] create_oligarchy_group(tier=0)"
bench_invoke "$CID" "create_oligarchy_group" "0" "create_oligarchy_group" \
    --caller "$BENCH_DEPLOYER_ADDRESS" \
    --group-id "$GROUP_ID_HEX" \
    --commitment "$CREATE_COMMITMENT_HEX" \
    --member-tier 0 \
    --admin-threshold-numerator 1 \
    --occupancy-commitment-initial "$CREATE_OCC_HEX" \
    --proof "$CREATE_PROOF_HEX" \
    --public-inputs "$CREATE_PI_JSON"

echo "==> [$BENCH_CURRENT_CONTRACT] verify_membership(tier=0, revert-mode)"
bench_invoke "$CID" "verify_membership" "0" "verify_membership" \
    --group-id "$GROUP_ID_HEX" \
    --proof "$CREATE_PROOF_HEX" \
    --public-inputs "$VERIFY_PI_JSON"

echo "==> [$BENCH_CURRENT_CONTRACT] update_commitment(revert-mode)"
bench_invoke "$CID" "update_commitment" "0" "update_commitment" \
    --group-id "$GROUP_ID_HEX" \
    --proof "$CREATE_PROOF_HEX" \
    --public-inputs "$UPDATE_PI_JSON"

echo "==> [$BENCH_CURRENT_CONTRACT] set_restricted_mode(true)"
bench_invoke "$CID" "set_restricted_mode" "n/a" "set_restricted_mode" \
    --restricted true

echo "==> [$BENCH_CURRENT_CONTRACT] bump_group_ttl"
bench_invoke "$CID" "bump_group_ttl" "n/a" "bump_group_ttl" \
    --group-id "$GROUP_ID_HEX"

echo "==> [$BENCH_CURRENT_CONTRACT] done"
