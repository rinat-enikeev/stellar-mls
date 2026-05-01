#!/usr/bin/env bash
# Top-level driver: provisions identity + builds, then runs each
# per-contract bench. Emits the JSONL row stream to $BENCH_JSONL
# (default: scripts/bench-gas/results.jsonl).
set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"

BENCH_JSONL="${BENCH_JSONL:-$SCRIPT_DIR/results.jsonl}"
BENCH_BODY="${BENCH_BODY:-$SCRIPT_DIR/results.md}"
export BENCH_JSONL

: > "$BENCH_JSONL"

# shellcheck source=setup.sh
. "$SCRIPT_DIR/setup.sh"

if [ "$#" -gt 0 ]; then
    CONTRACTS=("$@")
else
    # Default ordering: contracts with the most ops first so the operator
    # sees the meaty rows early and can ^C if RPC starts wobbling.
    CONTRACTS=(sep-oneonone sep-oligarchy sep-anarchy sep-democracy sep-tyranny)
fi

for c in "${CONTRACTS[@]}"; do
    driver="$SCRIPT_DIR/contracts/$c.sh"
    if [ ! -f "$driver" ]; then
        echo "no driver for $c at $driver" >&2
        exit 1
    fi
    bash "$driver" || echo "    [$c] driver exited non-zero — continuing"
done

echo "==> rendering markdown release body → $BENCH_BODY"
python3 "$SCRIPT_DIR/render.py" \
    --jsonl "$BENCH_JSONL" \
    --output "$BENCH_BODY" \
    --network "$BENCH_NETWORK" \
    --tag "${BENCH_TAG:-(untagged)}"

echo
echo "==> done"
echo "    jsonl: $BENCH_JSONL"
echo "    body:  $BENCH_BODY"
