#!/bin/sh
#
# Generate a production keyset with real randomness (OsRng).
#
# Usage:
#   ./scripts/generate-keyset.sh [--version N] [--out-dir DIR]
#
# Default output: keyset-v1/ in the repo root.
#
# After generation:
#   1. Copy proving keys to mobile assets (see below)
#   2. Use VK JSONs for contract deployment
#   3. DESTROY any intermediate files you don't need
#
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

VERSION="1"
OUT_DIR=""

while [ $# -gt 0 ]; do
    case "$1" in
        --version) VERSION="$2"; shift 2 ;;
        --out-dir) OUT_DIR="$2"; shift 2 ;;
        *) echo "Unknown arg: $1" >&2; exit 1 ;;
    esac
done

if [ -z "$OUT_DIR" ]; then
    OUT_DIR="$REPO_ROOT/keyset-v${VERSION}"
fi

echo "Generating keyset v${VERSION} → $OUT_DIR"
echo ""

cargo run --quiet \
    --release \
    --manifest-path "$REPO_ROOT/Cargo.toml" \
    --bin generate_keyset \
    -- \
    --out-dir "$OUT_DIR" \
    --version "$VERSION"

echo ""
echo "To install proving keys into mobile apps:"
echo ""
echo "  # Android"
echo "  mkdir -p clients/android/StellarChat/app/src/main/assets/keyset-v${VERSION}"
echo "  for tier in small medium large; do"
echo "    cp $OUT_DIR/\$tier/proving_key.bin clients/android/StellarChat/app/src/main/assets/keyset-v${VERSION}/\$tier.bin"
echo "  done"
echo ""
echo "  # iOS"
echo "  mkdir -p clients/ios/StellarChat/StellarChat/Resources/keyset-v${VERSION}"
echo "  for tier in small medium large; do"
echo "    cp $OUT_DIR/\$tier/proving_key.bin clients/ios/StellarChat/StellarChat/Resources/keyset-v${VERSION}/\$tier.bin"
echo "  done"
echo ""
echo "To deploy contract with these VKs:"
echo "  IDENTITY=my-deployer VK_DIR=$OUT_DIR ./scripts/deploy-mainnet.sh"
