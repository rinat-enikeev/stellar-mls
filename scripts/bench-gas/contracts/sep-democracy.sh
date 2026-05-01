#!/usr/bin/env bash
# sep-democracy gas bench driver (V2).
#
# Coverage:
#   deploy, set_restricted_mode, plus per-tier (d=5/8/11):
#     create_group (uses MEMBERSHIP_VK — gen-membership-proof works),
#     verify_membership (same fresh proof + state).
#
# Out of scope (V3):
#   update_commitment — uses VK_DEMO_UPDATE_D{5,8,11}, a democracy-
#   specific update circuit. `gen-update-proof` produces proofs for
#   the generic update circuit (used by sep-anarchy), not for the
#   democracy variant. Needs a `gen-democracy-update-proof` binary.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
LIB="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)/lib.sh"
# shellcheck source=../lib.sh
. "$LIB"

export BENCH_CURRENT_CONTRACT="sep-democracy"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/bench-gas-democracy.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT INT TERM

# A canonical occupancy commitment value (any canonical Fr works —
# the contract only checks `is_canonical_fr` on this; it's stored,
# not re-verified by the create proof).
OCCUPANCY_HEX="${ZERO32_HEX:0:62}01"

echo "==> [$BENCH_CURRENT_CONTRACT] deploy"
CID="$(bench_deploy \
    "bench-gas-democracy" \
    "$BENCH_ARTIFACT_DIR/sep_democracy_contract.wasm" \
    --admin "$BENCH_DEPLOYER_ADDRESS")"
if [ -z "$CID" ]; then
    echo "    deploy failed — skipping further ops" >&2
    exit 0
fi

run_tier() {
    local tier="$1"
    local depth="$2"
    local group_id_hex
    group_id_hex="$(printf '%02x%s' $(( 0x60 + tier )) "$(printf '00%.0s' $(seq 1 31))")"

    echo "==> [$BENCH_CURRENT_CONTRACT] tier=$tier (d=$depth) generating fresh proof"
    bench_gen_membership_proof "$depth" "$WORK/mp-d$depth"

    local mp_proof_hex mp_pi_json commitment_hex
    mp_proof_hex="$(bench_gen_proof_hex "$WORK/mp-d$depth")"
    mp_pi_json="$(bench_gen_pi_json "$WORK/mp-d$depth")"
    commitment_hex="$(bench_gen_commitment_hex "$WORK/mp-d$depth")"

    echo "==> [$BENCH_CURRENT_CONTRACT] tier=$tier create_group"
    bench_invoke "$CID" "create_group" "$tier" "create_group" \
        --caller "$BENCH_DEPLOYER_ADDRESS" \
        --group-id "$group_id_hex" \
        --commitment "$commitment_hex" \
        --tier "$tier" \
        --threshold-numerator 60 \
        --occupancy-commitment-initial "$OCCUPANCY_HEX" \
        --proof "$mp_proof_hex" \
        --public-inputs "$mp_pi_json"

    echo "==> [$BENCH_CURRENT_CONTRACT] tier=$tier verify_membership"
    bench_invoke "$CID" "verify_membership" "$tier" "verify_membership" \
        --group-id "$group_id_hex" \
        --proof "$mp_proof_hex" \
        --public-inputs "$mp_pi_json"
}

run_tier 0 5
if [ "${BENCH_FULL_TIERS:-1}" = "1" ]; then
    run_tier 1 8
    run_tier 2 11
fi

echo "==> [$BENCH_CURRENT_CONTRACT] set_restricted_mode(true)"
bench_invoke "$CID" "set_restricted_mode" "n/a" "set_restricted_mode" \
    --restricted true

echo "==> [$BENCH_CURRENT_CONTRACT] done"
