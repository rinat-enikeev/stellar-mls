#!/usr/bin/env bash
# sep-anarchy gas bench driver (V2).
#
# Coverage:
#   deploy, set_restricted_mode, plus per-tier (d=5/8/11):
#     create_group, verify_membership, update_commitment.
#
# Per-tier ops use freshly-generated PLONK proofs from the
# `gen-membership-proof` and `gen-update-proof` binaries. The baked
# VKs are shape-only (depend on circuit topology, not witness), so
# any (secret_keys, prover_index, salt) tuple at the same depth
# produces a proof that verifies under the on-chain VK.
#
# State chain per group_id:
#   1. create_group writes  (commitment=Cm,  epoch=0).
#   2. verify_membership re-uses the same proof+PI; matches state.
#   3. update_commitment uses an update proof generated at the same
#      depth with epoch_old=0, salt_old=salt_membership, salt_new
#      different. The c_old in update PI matches Cm.
#
# One contract hosts all three tiers (different group_id per tier),
# so the release body shows a single sep-anarchy address row.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
LIB="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)/lib.sh"
# shellcheck source=../lib.sh
. "$LIB"

export BENCH_CURRENT_CONTRACT="sep-anarchy"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/bench-gas-anarchy.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT INT TERM

echo "==> [$BENCH_CURRENT_CONTRACT] deploy"
CID="$(bench_deploy \
    "bench-gas-anarchy" \
    "$BENCH_ARTIFACT_DIR/sep_anarchy_contract.wasm" \
    --admin "$BENCH_DEPLOYER_ADDRESS")"
if [ -z "$CID" ]; then
    echo "    deploy failed — skipping further ops" >&2
    exit 0
fi

run_tier() {
    local tier="$1"
    local depth="$2"
    # Distinct group_id per tier — high byte = 0x40+tier so the bytes
    # decode to canonical Fr regardless of tier.
    local group_id_hex
    group_id_hex="$(printf '%02x%s' $(( 0x40 + tier )) "$(printf '00%.0s' $(seq 1 31))")"

    echo "==> [$BENCH_CURRENT_CONTRACT] tier=$tier (d=$depth) generating fresh proofs"
    bench_gen_membership_proof "$depth" "$WORK/mp-d$depth"
    bench_gen_update_proof     "$depth" "$WORK/up-d$depth"

    local mp_proof_hex mp_pi_json commitment_hex
    mp_proof_hex="$(bench_gen_proof_hex "$WORK/mp-d$depth")"
    mp_pi_json="$(bench_gen_pi_json "$WORK/mp-d$depth")"
    commitment_hex="$(bench_gen_commitment_hex "$WORK/mp-d$depth")"

    local up_proof_hex up_pi_json
    up_proof_hex="$(bench_gen_proof_hex "$WORK/up-d$depth")"
    up_pi_json="$(bench_gen_pi_json "$WORK/up-d$depth")"

    echo "==> [$BENCH_CURRENT_CONTRACT] tier=$tier create_group"
    bench_invoke "$CID" "create_group" "$tier" "create_group" \
        --caller "$BENCH_DEPLOYER_ADDRESS" \
        --group-id "$group_id_hex" \
        --commitment "$commitment_hex" \
        --tier "$tier" \
        --member-count 8 \
        --proof "$mp_proof_hex" \
        --public-inputs "$mp_pi_json"

    echo "==> [$BENCH_CURRENT_CONTRACT] tier=$tier verify_membership"
    bench_invoke "$CID" "verify_membership" "$tier" "verify_membership" \
        --group-id "$group_id_hex" \
        --proof "$mp_proof_hex" \
        --public-inputs "$mp_pi_json"

    echo "==> [$BENCH_CURRENT_CONTRACT] tier=$tier update_commitment"
    bench_invoke "$CID" "update_commitment" "$tier" "update_commitment" \
        --group-id "$group_id_hex" \
        --proof "$up_proof_hex" \
        --public-inputs "$up_pi_json"
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
