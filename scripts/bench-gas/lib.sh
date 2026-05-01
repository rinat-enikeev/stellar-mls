#!/usr/bin/env bash
# Shared helpers for the testnet gas benchmark suite.
#
# Encoding contract:
#   `stellar contract invoke` accepts `--<arg>-file-path <PATH>` for any
#   typed arg, where the file contains a JSON value matching the
#   contract's schema:
#     - BytesN<N>          → JSON string of N hex chars × 2: `"<hex>"`
#     - Vec<BytesN<32>>    → JSON array of hex strings:        `["<hex>", ...]`
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

# bin_to_hex_json <input.bin> <output.json>
# Wraps raw bytes as `"<hex>"` (a JSON string). Suitable for BytesN<N>.
bin_to_hex_json() {
    local in="$1"
    local out="$2"
    local hex
    hex="$(xxd -p -c 99999 "$in" | tr -d '\n')"
    printf '"%s"' "$hex" > "$out"
}

# pi_concat_to_json <input.bin> <num_fields> <output.json>
# Splits a flat 32*N byte file into a JSON array of N hex-encoded
# 32-byte chunks. Suitable for Vec<BytesN<32>>.
pi_concat_to_json() {
    local in="$1"
    local n="$2"
    local out="$3"
    local i hex
    : > "$out.tmp"
    {
        printf '['
        for (( i=0; i<n; i++ )); do
            hex="$(dd if="$in" bs=32 skip="$i" count=1 2>/dev/null | xxd -p -c 99999 | tr -d '\n')"
            if (( i > 0 )); then printf ','; fi
            printf '"%s"' "$hex"
        done
        printf ']'
    } > "$out"
}

# hex_to_json <hex_string> <output.json>
# Wraps a hex string (no 0x prefix) as a JSON BytesN value.
hex_to_json() {
    local hex="$1"
    local out="$2"
    printf '"%s"' "$hex" > "$out"
}

# read_pi_field_hex <pi.bin> <field_index>
# Echoes the hex of the i-th 32-byte chunk in a flat PI file.
read_pi_field_hex() {
    local pi="$1"
    local i="$2"
    dd if="$pi" bs=32 skip="$i" count=1 2>/dev/null | xxd -p -c 99999 | tr -d '\n'
}

# be32_zero_json <output.json>
# JSON for 32 zero bytes (= big-endian repr of u64 zero, used for epoch=0).
be32_zero_json() {
    local out="$1"
    printf '"%s"' "$(printf '%064d' 0)" > "$out"
}

# ---------- invocation + fee capture ----------

# capture_tx_hash <stderr_logfile>
# Greps the stellar CLI's stderr for the transaction hash. Returns the
# hex hash on stdout, exits non-zero if not found.
capture_tx_hash() {
    local err="$1"
    # CLI v26 prints `ℹ Transaction hash is <hex>` (note the leading
    # info glyph). We tolerate either the glyph form or a bare line.
    grep -oE '[0-9a-f]{64}' "$err" | head -1
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

# emit_row <contract> <op> <tier> <hash> [extra_json]
# Append a JSONL row to $BENCH_JSONL with fee + cost data for the tx.
emit_row() {
    local contract="$1"
    local op="$2"
    local tier="$3"
    local hash="$4"
    local extra="${5:-{\}}"

    if [ -z "$hash" ]; then
        # Fee capture failed (often: tx wasn't submitted, e.g. read-only).
        # Emit a row with null fee fields so the renderer can flag it.
        jq -n \
            --arg contract "$contract" \
            --arg op "$op" \
            --arg tier "$tier" \
            --argjson extra "$extra" \
            '{contract: $contract, op: $op, tier: $tier, fee_stroops: null, hash: null} + $extra' \
            >> "$BENCH_JSONL"
        return 0
    fi

    local raw
    raw="$(fetch_fee_full "$hash" || echo '{}')"
    jq -n \
        --arg contract "$contract" \
        --arg op "$op" \
        --arg tier "$tier" \
        --arg hash "$hash" \
        --argjson raw "$raw" \
        --argjson extra "$extra" \
        '{contract: $contract, op: $op, tier: $tier, hash: $hash,
          fee_stroops: ($raw.totals.fee_charged // $raw.fee_charged // null),
          inclusion_fee: ($raw.totals.inclusion_fee // null),
          resource_fee: ($raw.totals.resource_fee // null),
          refundable_fee_refund: ($raw.totals.refundable_fee_refund // null),
          raw: $raw} + $extra' \
        >> "$BENCH_JSONL"
}

# ---------- deploy + invoke wrappers ----------

# bench_deploy <contract_alias> <wasm> <constructor_args...>
# Deploys, captures the contract id (stdout) and the fee for the
# deployment tx (stderr → tx hash → fetch fee). Echoes the contract
# id on stdout for the caller to bind to a variable. Stderr is sunk
# to $err synchronously (no `tee >()` race) and replayed for the
# operator on completion.
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
