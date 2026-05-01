#!/usr/bin/env bash
# Shared helpers for the testnet gas benchmark suite.
#
# Encoding contract (corrected after the v1 run on testnet showed the
# CLI receiving JSON-wrapped hex as raw bytes):
#   * `--<arg>-file-path <PATH>` reads the file as **raw bytes** and
#     binds them as the arg value. Useful only for binary-shaped types
#     when you have a binary file to feed in.
#   * For `BytesN<N>` we pass `--<arg> <hex>` inline (CLI parses hex
#     into the typed BytesN). Inline length is fine for proofs at
#     1601 bytes (~3KB hex on a Linux ARG_MAX of 128KB+).
#   * For `Vec<BytesN<32>>` we pass `--<arg> '<json_array>'` inline
#     (CLI parses JSON array of hex strings into the typed Vec).
#
# Fee capture: `stellar contract invoke` (default --send=yes) prints
# the tx hash to stderr in the line `ℹ Transaction hash is <hex>`. We
# capture stderr, grep the hash, then `stellar tx fetch fee --hash`
# to get the resource + inclusion + refund breakdown.
#
# All output rows go to a JSONL sink (BENCH_JSONL env var); the
# renderer reads them post-run.

set -euo pipefail

# ---------- low-level encoders ----------
# All encoders return hex (or JSON arrays of hex) on stdout — callers
# splice the value into the CLI invocation directly. No more temp
# files holding JSON-wrapped values; the v1 run proved the CLI's
# --<arg>-file-path treats the file as raw bytes regardless of
# extension, so JSON wrappers leaked through to the contract as
# literal `"<hex>"` byte strings.

# bin_hex <input.bin>
# Echoes raw bytes hex-encoded (no `0x`, no quotes, no newline).
# Suitable for `--<arg> $(bin_hex …)` where the arg is BytesN<N>.
bin_hex() {
    xxd -p -c 99999 "$1" | tr -d '\n'
}

# pi_concat_json_array <input.bin> <num_fields>
# Splits a flat 32*N byte file into a JSON array of N hex strings.
# Suitable for `--<arg> "$(pi_concat_json_array … )"` where the arg is
# Vec<BytesN<32>>.
pi_concat_json_array() {
    local in="$1"
    local n="$2"
    local i hex
    printf '['
    for (( i=0; i<n; i++ )); do
        hex="$(dd if="$in" bs=32 skip="$i" count=1 2>/dev/null | xxd -p -c 99999 | tr -d '\n')"
        if (( i > 0 )); then printf ','; fi
        printf '"%s"' "$hex"
    done
    printf ']'
}

# read_pi_field_hex <pi.bin> <field_index>
# Echoes the hex of the i-th 32-byte chunk in a flat PI file.
read_pi_field_hex() {
    local pi="$1"
    local i="$2"
    dd if="$pi" bs=32 skip="$i" count=1 2>/dev/null | xxd -p -c 99999 | tr -d '\n'
}

# Constants used across drivers.
ZERO32_HEX="$(printf '%064d' 0)"

# ---------- V2 proof generators ----------
# Wrappers around the `gen-membership-proof` / `gen-update-proof`
# binaries (built under `--features gen-proof-tool` by setup.sh).
# Each call writes `proof.bin`, `proof.hex`, `commitment.hex`,
# `public_inputs.json`, etc. into a fresh out-dir; callers consume
# the artifacts via `bench_gen_*_load`.
#
# Witness defaults are baked in below — they're shape-only fixtures
# (the VK is shape-dependent, not witness-dependent), so the same
# (secret_keys, prover_index) works at every depth.

# 8 canonical secret keys; tree pads beyond that. prover_index=3.
BENCH_GEN_SECRET_KEYS='0x01,0x02,0x03,0x04,0x05,0x06,0x07,0x08'
BENCH_GEN_PROVER_INDEX='3'
BENCH_GEN_SALT_OLD='0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee'
BENCH_GEN_SALT_NEW='0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff'

# bench_gen_membership_proof <depth> <out_dir>
# Generates a membership proof at the given depth (5/8/11) at epoch 0
# with the canonical witness. Writes proof.bin / commitment.hex /
# public_inputs.json into out_dir.
bench_gen_membership_proof() {
    local depth="$1"
    local out_dir="$2"
    mkdir -p "$out_dir"
    "${BENCH_REPO_ROOT}/target/release/gen-membership-proof" \
        --depth "$depth" \
        --epoch 0 \
        --salt "$BENCH_GEN_SALT_OLD" \
        --secret-keys "$BENCH_GEN_SECRET_KEYS" \
        --prover-index "$BENCH_GEN_PROVER_INDEX" \
        --out-dir "$out_dir" >&2
}

# bench_gen_update_proof <depth> <out_dir>
# Generates an update proof at the given depth: c_old comes from the
# epoch=0 / salt_old witness (matches `bench_gen_membership_proof`'s
# commitment), c_new comes from the epoch=1 / salt_new witness.
# Re-using salt_old here keeps c_old aligned with the post-create
# state set by the membership-proof's create_group call.
bench_gen_update_proof() {
    local depth="$1"
    local out_dir="$2"
    mkdir -p "$out_dir"
    "${BENCH_REPO_ROOT}/target/release/gen-update-proof" \
        --depth "$depth" \
        --epoch-old 0 \
        --salt-old "$BENCH_GEN_SALT_OLD" \
        --salt-new "$BENCH_GEN_SALT_NEW" \
        --secret-keys "$BENCH_GEN_SECRET_KEYS" \
        --prover-index "$BENCH_GEN_PROVER_INDEX" \
        --out-dir "$out_dir" >&2
}

# Helpers to read out-dir artifacts back as shell-safe strings.
bench_gen_proof_hex() { cat "$1/proof.hex"; }
bench_gen_pi_json()   { cat "$1/public_inputs.json"; }
bench_gen_commitment_hex() { cat "$1/commitment.hex"; }

# ---------- invocation + fee capture ----------

# capture_tx_hashes <stderr_logfile>
# Echoes every successfully-submitted transaction hash the stellar
# CLI logged, one per line, in submission order. Anchored on the
# stellar.expert URL line, which v26 prints only after the network
# accepts the tx — so simulation-failed ops that never hit chain are
# correctly skipped.
#
# `stellar contract deploy` submits two txs (upload_contract_wasm +
# create_contract); a normal invoke submits one. Callers pick by
# index.
#
# v26 sample lines this matches (one per accepted tx):
#   🔗 https://stellar.expert/explorer/testnet/tx/<64-hex>
capture_tx_hashes() {
    local err="$1"
    grep -oE 'stellar\.expert/explorer/[a-z]+/tx/[0-9a-f]{64}' "$err" \
        | grep -oE '[0-9a-f]{64}'
}

# capture_tx_hash <stderr_logfile>
# Convenience: first hash from `capture_tx_hashes`. Empty if none.
capture_tx_hash() {
    capture_tx_hashes "$1" | head -1
}

# fetch_fee_stroops <hash>
# Echoes the total fee_charged in stroops, parsed from
# `stellar tx fetch fee --output json`.
fetch_fee_stroops() {
    local hash="$1"
    local rpc_args=()
    if [ -n "${BENCH_NETWORK:-}" ]; then
        rpc_args=(--network "$BENCH_NETWORK")
    fi
    if [ -n "${BENCH_CONFIG_DIR:-}" ]; then
        rpc_args=(--config-dir "$BENCH_CONFIG_DIR" "${rpc_args[@]}")
    fi
    stellar tx fetch fee --hash "$hash" --output json "${rpc_args[@]}" \
        | jq -r '.totals.fee_charged // .fee_charged // empty'
}

# fetch_fee_full <hash>
# Echoes JSON with both fee_charged (net) + resource breakdown when
# available. Used by the JSONL emitter.
fetch_fee_full() {
    local hash="$1"
    local rpc_args=()
    if [ -n "${BENCH_NETWORK:-}" ]; then
        rpc_args=(--network "$BENCH_NETWORK")
    fi
    if [ -n "${BENCH_CONFIG_DIR:-}" ]; then
        rpc_args=(--config-dir "$BENCH_CONFIG_DIR" "${rpc_args[@]}")
    fi
    stellar tx fetch fee --hash "$hash" --output json "${rpc_args[@]}"
}

# emit_contract_address <contract> <address>
# One row per deployed contract — the renderer pulls these into the
# stellar.expert link table at the top of the release body.
emit_contract_address() {
    local contract="$1"
    local address="$2"
    if [ -z "$address" ]; then
        return 0
    fi
    jq -nc \
        --arg row_type "contract" \
        --arg contract "$contract" \
        --arg address "$address" \
        '{row_type: $row_type, contract: $contract, address: $address}' \
        >> "$BENCH_JSONL"
}

# emit_row <contract> <op> <tier> <hash> [extra_json]
# Append a JSONL row to $BENCH_JSONL with fee + cost data for the tx.
emit_row() {
    local contract="$1"
    local op="$2"
    local tier="$3"
    local hash="$4"
    local extra="${5:-{\}}"

    if [ -z "$hash" ]; then
        # Fee capture failed (the tx wasn't submitted — most often the
        # CLI rejected it at simulation time, or it's a read-only
        # entrypoint that short-circuits to local sim). Emit a row
        # with null fee fields so the renderer can flag it.
        jq -nc \
            --arg row_type "op" \
            --arg contract "$contract" \
            --arg op "$op" \
            --arg tier "$tier" \
            --argjson extra "$extra" \
            '{row_type: $row_type, contract: $contract, op: $op, tier: $tier, fee_stroops: null, hash: null} + $extra' \
            >> "$BENCH_JSONL"
        return 0
    fi

    local raw
    raw="$(fetch_fee_full "$hash" || echo '{}')"
    # `stellar tx fetch fee --output json` returns
    #   { "proposed": {fee, resource_fee, inclusion_fee},
    #     "charged":  {fee, resource_fee, inclusion_fee,
    #                  non_refundable_resource_fee, refundable_resource_fee} }
    # `charged.fee` is the net amount the source account paid; that's
    # what we surface as the headline `fee_stroops`. `proposed` is what
    # the simulator pre-allocated — useful diagnostic, kept under
    # `proposed_fee` for the renderer.
    jq -nc \
        --arg row_type "op" \
        --arg contract "$contract" \
        --arg op "$op" \
        --arg tier "$tier" \
        --arg hash "$hash" \
        --argjson raw "$raw" \
        --argjson extra "$extra" \
        '{row_type: $row_type, contract: $contract, op: $op, tier: $tier, hash: $hash,
          fee_stroops: $raw.charged.fee,
          inclusion_fee: $raw.charged.inclusion_fee,
          resource_fee: $raw.charged.resource_fee,
          non_refundable_resource_fee: $raw.charged.non_refundable_resource_fee,
          refundable_resource_fee: $raw.charged.refundable_resource_fee,
          proposed_fee: $raw.proposed.fee,
          raw: $raw} + $extra' \
        >> "$BENCH_JSONL"
}

# ---------- deploy + invoke wrappers ----------

# bench_deploy <contract_alias> <wasm> <constructor_args...>
# Deploys and echoes the contract id on stdout. `stellar contract
# deploy` v26 batches upload + create into a single transaction
# (one tx hash, one stellar.expert URL printed) — the second 🔗
# line in the output is the lab.stellar.org **contract** URL, not
# a tx URL. So we emit a single `deploy` row with the captured
# fee.
bench_deploy() {
    local alias="$1"
    local wasm="$2"
    shift 2

    local err
    err="$(mktemp)"

    local cid
    cid="$(stellar contract deploy \
        --config-dir "$BENCH_CONFIG_DIR" \
        --network "$BENCH_NETWORK" \
        --source-account "$BENCH_DEPLOYER" \
        --alias "$alias" \
        --wasm "$wasm" \
        -- "$@" 2> "$err" | tr -d '\n')" || cid=""

    cat "$err" >&2

    local hash
    hash="$(capture_tx_hash "$err" || true)"
    rm -f "$err"

    emit_row "$BENCH_CURRENT_CONTRACT" "deploy" "n/a" "$hash"
    emit_contract_address "$BENCH_CURRENT_CONTRACT" "$cid"
    printf '%s' "$cid"
}

# bench_invoke <contract_id> <op> <tier> <fn> <fn_args...>
# Submits the call, captures tx hash, fetches fee, emits a JSONL row.
# Uses --send=yes so we get a real fee_charged even on revert paths.
# A non-zero CLI exit (e.g. revert) is intentional for revert-mode
# benches and does not abort the run.
bench_invoke() {
    local cid="$1"
    local op="$2"
    local tier="$3"
    local fn="$4"
    shift 4

    local err
    err="$(mktemp)"

    stellar contract invoke \
        --config-dir "$BENCH_CONFIG_DIR" \
        --network "$BENCH_NETWORK" \
        --id "$cid" \
        --source-account "$BENCH_DEPLOYER" \
        --send yes \
        -- "$fn" "$@" \
        > /dev/null 2> "$err" || true

    cat "$err" >&2

    local hash
    hash="$(capture_tx_hash "$err" || true)"
    rm -f "$err"

    emit_row "$BENCH_CURRENT_CONTRACT" "$op" "$tier" "$hash"
}
