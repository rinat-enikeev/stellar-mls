#!/bin/sh
#
# Generate a DEV-ONLY Democracy VK for local/testnet iteration.
#
# ⚠  NOT FOR PRODUCTION — the trapdoor is held in a single OsRng process. Anyone
# ⚠  with the proving key can forge Democracy update proofs. Real production
# ⚠  VKs must come from the Phase 2 ceremony documented in
# ⚠  docs/democracy-circuit-ceremony.md.
#
# Usage:
#   ./scripts/generate-democracy-vk-dev.sh [--out-dir DIR]
#
# Default output: keyset-democracy-dev/ in the repo root.
#
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

OUT_DIR=""
while [ $# -gt 0 ]; do
    case "$1" in
        --out-dir) OUT_DIR="$2"; shift 2 ;;
        -h|--help)
            sed -n '3,17p' "$0"
            exit 0
            ;;
        *) echo "Unknown arg: $1" >&2; exit 1 ;;
    esac
done

if [ -z "$OUT_DIR" ]; then
    OUT_DIR="$REPO_ROOT/keyset-democracy-dev"
fi

echo "DEV-ONLY Democracy VK → $OUT_DIR"
echo "(NOT for production — see docs/democracy-circuit-ceremony.md)"
echo ""

cargo run --quiet \
    --release \
    --manifest-path "$REPO_ROOT/Cargo.toml" \
    --bin generate_democracy_vk_dev \
    -- \
    --out-dir "$OUT_DIR"
