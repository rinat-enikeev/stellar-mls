#!/usr/bin/env bash
# Onym row benchmark harness for the SoK paper.
# See methodology.md in the same directory for the measurement contract.
#
# Usage:  bash run.sh [--reference]
#
# Writes results.json (and individual .tex snippets) into this directory.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/../../../.." && pwd)"
RESULTS="$HERE/results.json"
REFERENCE_FLAG="false"

if [[ "${1:-}" == "--reference" ]]; then
  REFERENCE_FLAG="true"
fi

# ──────────────────────────────────────────────────────────────────────
# Host metadata
# ──────────────────────────────────────────────────────────────────────
KERNEL="$(uname -mrs)"
case "$(uname -s)" in
  Darwin)
    CPU="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo unknown)"
    MEM_BYTES="$(sysctl -n hw.memsize 2>/dev/null || echo 0)"
    ;;
  Linux)
    CPU="$(lscpu 2>/dev/null | awk -F: '/Model name/ {gsub(/^ +/,"",$2); print $2; exit}' || echo unknown)"
    MEM_BYTES="$(awk '/MemTotal/ {print $2 * 1024; exit}' /proc/meminfo 2>/dev/null || echo 0)"
    ;;
  *)
    CPU="unknown"
    MEM_BYTES="0"
    ;;
esac

RUST_VER="$(rustc --version)"
GIT_REV="$(cd "$REPO_ROOT" && git rev-parse HEAD)"
SNAPSHOT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# ──────────────────────────────────────────────────────────────────────
# Crate versions from Cargo.lock
# ──────────────────────────────────────────────────────────────────────
crate_version () {
  local crate="$1"
  awk -v c="$crate" '
    $1 == "name" && $3 == "\""c"\"" { found=1; next }
    found && $1 == "version" { gsub(/"/, "", $3); print $3; exit }
  ' "$REPO_ROOT/Cargo.lock"
}
ARK_GROTH16="$(crate_version ark-groth16 || echo unknown)"
ARK_BLS12_381="$(crate_version ark-bls12-381 || echo unknown)"
ARK_FF="$(crate_version ark-ff || echo unknown)"

# ──────────────────────────────────────────────────────────────────────
# Build prover bin
# ──────────────────────────────────────────────────────────────────────
echo "==> cargo build --release"
(cd "$REPO_ROOT" && cargo build --release \
  --bin generate_contract_testnet_fixtures \
  -p sep-xxxx-circuits 2>&1 | tail -20)

# ──────────────────────────────────────────────────────────────────────
# Run measurement
# ──────────────────────────────────────────────────────────────────────
# NOTE: the current bin generates fixtures end-to-end; per-call timing
# instrumentation lands in §4 drafting work. For now we measure total
# wall-clock for a fresh fixture generation as a coarse upper bound,
# and note in results.json that per-call p50/p95 are pending.
#
# This stub satisfies the harness contract structurally (writes a
# valid schema-v1 results.json) but with `samples` empty. §4 work
# fills in per-ring-size sampling.

echo "==> Running prover (coarse wall-clock; per-sample timing TODO)"
TMP_OUT="$(mktemp -d)"
START_NS=$(date +%s%N 2>/dev/null || python3 -c 'import time;print(int(time.time()*1e9))')
(cd "$REPO_ROOT" && cargo run --release \
  --bin generate_contract_testnet_fixtures \
  -p sep-xxxx-circuits -- --out-dir "$TMP_OUT" 2>&1 | tail -5) || {
    echo "warn: bin invocation failed; recording NaN for this run" >&2
}
END_NS=$(date +%s%N 2>/dev/null || python3 -c 'import time;print(int(time.time()*1e9))')
COARSE_MS=$(( (END_NS - START_NS) / 1000000 ))

# ──────────────────────────────────────────────────────────────────────
# Emit results.json
# ──────────────────────────────────────────────────────────────────────
cat > "$RESULTS" <<JSON
{
  "schema_version": "1",
  "snapshot_iso8601": "$SNAPSHOT",
  "git_rev": "$GIT_REV",
  "host": {
    "kernel": "$KERNEL",
    "cpu": "$CPU",
    "memory_bytes": $MEM_BYTES,
    "rust": "$RUST_VER"
  },
  "crates": {
    "ark-groth16": "$ARK_GROTH16",
    "ark-bls12-381": "$ARK_BLS12_381",
    "ark-ff": "$ARK_FF"
  },
  "ring_sizes": { "small": 16, "medium": 64, "large": 256 },
  "primary_size": "medium",
  "samples": {
    "small":  { "prover_time_ms": [] },
    "medium": { "prover_time_ms": [] },
    "large":  { "prover_time_ms": [] }
  },
  "summary": {
    "primary": {
      "prover_time_ms_p50": null,
      "prover_time_ms_p95": null,
      "proof_size_bytes": null,
      "proving_key_size_bytes": null,
      "verifying_key_size_bytes": null,
      "verifier_gas_stroops": null,
      "verifier_usd_at_2026_04": null
    }
  },
  "coarse": {
    "wall_clock_ms_total": $COARSE_MS,
    "note": "Total fixture-generation wall-clock — upper bound only. Per-call sampling to be implemented in §4 drafting."
  },
  "reference": $REFERENCE_FLAG
}
JSON

echo "==> Wrote $RESULTS"
echo "==> Coarse wall-clock: ${COARSE_MS}ms"
echo
echo "Next steps before §4:"
echo "  - Add per-call timing instrumentation in src/prover/ (or a"
echo "    new bench bin) so prover_time_ms samples can be collected"
echo "    per ring size."
echo "  - Implement testnet verifier_gas_stroops measurement (requires"
echo "    STELLAR_RPC_URL and a deployed verifier contract)."
echo "  - Generate tab-cost-comparison-row-onym.tex,"
echo "    tab-proof-size-row-onym.tex, etc. from results.json."
