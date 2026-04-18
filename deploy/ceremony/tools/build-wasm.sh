#!/usr/bin/env bash
# Build the ceremony-wasm verifier and stage it under deploy/ceremony/wasm/
# so nginx serves it alongside the static site.
#
# This is invoked from the deploy script; run locally to preview /verify.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
CRATE_DIR="$REPO_ROOT/crates/ceremony-wasm"
OUT_DIR="$REPO_ROOT/deploy/ceremony/wasm"

if ! command -v wasm-pack >/dev/null 2>&1; then
  echo "error: wasm-pack not found. Install: curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh" >&2
  exit 1
fi

if ! rustup target list --installed 2>/dev/null | grep -q '^wasm32-unknown-unknown$'; then
  echo "installing wasm32-unknown-unknown target..."
  rustup target add wasm32-unknown-unknown
fi

echo "building ceremony-wasm..."
cd "$REPO_ROOT"
wasm-pack build --target web --release "$CRATE_DIR" --out-dir pkg >&2

mkdir -p "$OUT_DIR"
# Remove only the generated artifacts, not the directory itself, so we
# don't clobber .gitkeep or any manually-staged loader scripts.
rm -f "$OUT_DIR"/*.wasm "$OUT_DIR"/*.js "$OUT_DIR"/*.d.ts
cp "$CRATE_DIR/pkg/ceremony_wasm.js" "$OUT_DIR/"
cp "$CRATE_DIR/pkg/ceremony_wasm_bg.wasm" "$OUT_DIR/"
cp "$CRATE_DIR/pkg/ceremony_wasm.d.ts" "$OUT_DIR/" 2>/dev/null || true

WASM_SIZE=$(wc -c < "$OUT_DIR/ceremony_wasm_bg.wasm" | tr -d ' ')
GZIP_SIZE=$(gzip -c "$OUT_DIR/ceremony_wasm_bg.wasm" | wc -c | tr -d ' ')
echo "wasm: $WASM_SIZE bytes ($GZIP_SIZE bytes gzipped) -> $OUT_DIR"
