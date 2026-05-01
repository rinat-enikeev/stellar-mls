#!/usr/bin/env bash
# sep-oligarchy gas bench driver.
#
# Coverage (V1):
#   deploy, create_oligarchy_group (committed fixture, tier 0),
#   verify_membership (revert-mode against post-create state),
#   set_restricted_mode, bump_group_ttl.
#
# Bench mechanics:
#   * verify_membership returns Ok(false) on InvalidProof (no revert),
#     so the captured fee equals the real success-path cost — the
#     verifier runs the full pairing check identically in both arms.
#
# Out of scope (V2):
#   update_commitment — first run showed Soroban-CLI runs simulation
#   pre-flight and refuses to submit txs that revert, so revert-mode
#   bench can't capture a fee for ops whose contract path returns
#   Err. Need real verifying proofs from gen-update-proof; tracked
#   alongside the V2 anarchy/democracy/tyranny work.

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

# update_commitment is V2 — see header. Soroban-CLI rejects
# simulation-failing txs before submission, so a "well-formed but
# non-verifying proof" path doesn't capture a fee.

echo "==> [$BENCH_CURRENT_CONTRACT] set_restricted_mode(true)"
bench_invoke "$CID" "set_restricted_mode" "n/a" "set_restricted_mode" \
    --restricted true

echo "==> [$BENCH_CURRENT_CONTRACT] bump_group_ttl"
bench_invoke "$CID" "bump_group_ttl" "n/a" "bump_group_ttl" \
    --group-id "$GROUP_ID_HEX"

echo "==> [$BENCH_CURRENT_CONTRACT] done"
