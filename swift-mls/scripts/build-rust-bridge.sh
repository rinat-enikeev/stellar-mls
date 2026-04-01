#!/bin/sh
# Build the Rust static library for local Swift development.
#
# Usage:
#   ./scripts/build-rust-bridge.sh           # debug build (default)
#   ./scripts/build-rust-bridge.sh --release # release build
#
# The Swift package links against ../target/debug/libsep_xxxx_circuits.a
# by default. For release builds, update the linker path in Package.swift.
#
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"

PROFILE="debug"
CARGO_FLAGS=""
if [ "${1:-}" = "--release" ]; then
    PROFILE="release"
    CARGO_FLAGS="--release"
fi

cd "$REPO_ROOT"
cargo build $CARGO_FLAGS

LIB_PATH="$REPO_ROOT/target/$PROFILE/libsep_xxxx_circuits.a"
if [ ! -f "$LIB_PATH" ]; then
    echo "Expected library at: $LIB_PATH" >&2
    exit 1
fi

echo "Built: $LIB_PATH"
