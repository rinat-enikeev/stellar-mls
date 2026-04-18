#!/usr/bin/env bash
#
# verify-ceremony-tool.sh
#
# Rebuilds ceremony_tool for a given released tag + target inside the pinned
# Docker image and compares the unsigned SHA-256 to the published
# buildinfo.json. Prints OK on match, diff and exit 1 otherwise.
#
# Usage:
#   scripts/verify-ceremony-tool.sh <tag> <target>
#
# Example:
#   scripts/verify-ceremony-tool.sh v0.1.0 x86_64-unknown-linux-musl
#
# Linux targets are fully reproducible. macOS/Windows reproducibility covers
# only the unsigned pre-image; release artifacts have platform signatures
# that introduce non-determinism captured separately.

set -euo pipefail

TAG="${1:?Usage: $0 <tag> <target>}"
TARGET="${2:?Usage: $0 <tag> <target>}"

case "$TARGET" in
  x86_64-unknown-linux-musl|aarch64-unknown-linux-musl) ;;
  *)
    echo "Target '$TARGET' is not reproducible via this script." >&2
    echo "Only Linux musl targets reproduce byte-for-byte; macOS/Windows" >&2
    echo "artifacts are verifiable via the published buildinfo.json." >&2
    exit 2
    ;;
esac

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Determine rustc channel from the repo's rust-toolchain.toml
RUSTC_CHANNEL=$(awk -F'"' '/^channel/{print $2}' rust-toolchain.toml)
if [ -z "$RUSTC_CHANNEL" ]; then
  echo "Could not read channel from rust-toolchain.toml" >&2
  exit 1
fi

# Fetch published buildinfo.json for comparison.
NAME="ceremony_tool-$TAG-$TARGET"
BI_URL="https://github.com/rinat-enikeev/stellar-mls/releases/download/$TAG/$NAME.buildinfo.json"
echo "==> fetching $BI_URL"
curl -fsSL "$BI_URL" > "$WORK/buildinfo.json"
PUBLISHED_SHA=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["sha256"])' "$WORK/buildinfo.json")
PUBLISHED_EPOCH=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["source_date_epoch"])' "$WORK/buildinfo.json")
echo "==> published sha256: $PUBLISHED_SHA"
echo "==> SOURCE_DATE_EPOCH: $PUBLISHED_EPOCH"

# Build inside pinned Docker image.
IMG="rust:${RUSTC_CHANNEL}-alpine3.20"
echo "==> building in $IMG"

docker run --rm \
  -v "$PWD:/src:ro" \
  -v "$WORK:/out" \
  -e SOURCE_DATE_EPOCH="$PUBLISHED_EPOCH" \
  -e TZ=UTC -e LC_ALL=C \
  -w /src \
  "$IMG" \
  sh -c "
    set -eux
    apk add --no-cache musl-dev gcc git
    rustup target add $TARGET
    export RUSTFLAGS='--remap-path-prefix=/src=/build'
    cargo build --locked --profile release-ceremony --target $TARGET --bin ceremony_tool
    cp target/$TARGET/release-ceremony/ceremony_tool /out/$NAME
  "

LOCAL_SHA=$(shasum -a 256 "$WORK/$NAME" | awk '{print $1}')
echo "==> local     sha256: $LOCAL_SHA"

if [ "$LOCAL_SHA" = "$PUBLISHED_SHA" ]; then
  echo "OK: local rebuild matches published $NAME"
  exit 0
fi

echo "MISMATCH: local rebuild does not match published artifact" >&2
echo "  published: $PUBLISHED_SHA" >&2
echo "  local:     $LOCAL_SHA" >&2
exit 1
