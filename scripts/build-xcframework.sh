#!/bin/sh
# Build the Rust static library for Apple targets and package as an XCFramework.
#
# Usage:
#   ./scripts/build-xcframework.sh [--out-dir DIR]
#
# Prerequisites:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim
#
# Output:
#   <out-dir>/SEPMLSFFI.xcframework   — XCFramework containing the static library + C header
#
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

OUT_DIR="${REPO_ROOT}/build"
while [ $# -gt 0 ]; do
    case "$1" in
        --out-dir) OUT_DIR="$2"; shift 2 ;;
        *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
done

LIB_NAME="libsep_xxxx_circuits.a"
HEADER_DIR="$REPO_ROOT/swift-mls/Sources/CSEPMLSFFI/include"
STAGING="$OUT_DIR/staging"
XCFRAMEWORK="$OUT_DIR/SEPMLSFFI.xcframework"

require_target() {
    if ! rustup target list --installed | grep -q "^$1\$"; then
        echo "installing Rust target: $1"
        rustup target add "$1"
    fi
}

build_target() {
    local target="$1"
    require_target "$target"
    echo "==> Building release for $target"
    cargo build \
        --manifest-path "$REPO_ROOT/Cargo.toml" \
        --release \
        --target "$target"
}

# Clean previous build
rm -rf "$STAGING" "$XCFRAMEWORK"
mkdir -p "$STAGING"

# ================================================================
# macOS: arm64 + x86_64 → universal binary
# ================================================================
build_target aarch64-apple-darwin

MACOS_DIR="$STAGING/macos"
mkdir -p "$MACOS_DIR"

# Check if x86_64 target is available (cross-compilation may not work on all setups)
if rustup target list --installed | grep -q "^x86_64-apple-darwin$"; then
    build_target x86_64-apple-darwin
    echo "==> Creating macOS universal binary"
    lipo -create \
        "$REPO_ROOT/target/aarch64-apple-darwin/release/$LIB_NAME" \
        "$REPO_ROOT/target/x86_64-apple-darwin/release/$LIB_NAME" \
        -output "$MACOS_DIR/$LIB_NAME"
else
    echo "==> x86_64-apple-darwin not installed, using arm64-only macOS library"
    cp "$REPO_ROOT/target/aarch64-apple-darwin/release/$LIB_NAME" "$MACOS_DIR/$LIB_NAME"
fi

XCFRAMEWORK_ARGS="-library $MACOS_DIR/$LIB_NAME -headers $HEADER_DIR"

# ================================================================
# iOS device: arm64
# ================================================================
if rustup target list --installed | grep -q "^aarch64-apple-ios$"; then
    build_target aarch64-apple-ios
    IOS_DIR="$STAGING/ios"
    mkdir -p "$IOS_DIR"
    cp "$REPO_ROOT/target/aarch64-apple-ios/release/$LIB_NAME" "$IOS_DIR/$LIB_NAME"
    XCFRAMEWORK_ARGS="$XCFRAMEWORK_ARGS -library $IOS_DIR/$LIB_NAME -headers $HEADER_DIR"
fi

# ================================================================
# iOS simulator: arm64 (+ x86_64 if available)
# ================================================================
if rustup target list --installed | grep -q "^aarch64-apple-ios-sim$"; then
    build_target aarch64-apple-ios-sim
    IOS_SIM_DIR="$STAGING/ios-sim"
    mkdir -p "$IOS_SIM_DIR"

    if rustup target list --installed | grep -q "^x86_64-apple-ios$"; then
        build_target x86_64-apple-ios
        lipo -create \
            "$REPO_ROOT/target/aarch64-apple-ios-sim/release/$LIB_NAME" \
            "$REPO_ROOT/target/x86_64-apple-ios/release/$LIB_NAME" \
            -output "$IOS_SIM_DIR/$LIB_NAME"
    else
        cp "$REPO_ROOT/target/aarch64-apple-ios-sim/release/$LIB_NAME" "$IOS_SIM_DIR/$LIB_NAME"
    fi
    XCFRAMEWORK_ARGS="$XCFRAMEWORK_ARGS -library $IOS_SIM_DIR/$LIB_NAME -headers $HEADER_DIR"
fi

# ================================================================
# Create XCFramework
# ================================================================
echo "==> Creating XCFramework"
eval xcodebuild -create-xcframework $XCFRAMEWORK_ARGS -output "$XCFRAMEWORK"

echo
echo "XCFramework built successfully:"
echo "  $XCFRAMEWORK"
echo
echo "To use in Package.swift, replace the CSEPMLSFFI target with:"
echo "  .binaryTarget(name: \"CSEPMLSFFI\", path: \"path/to/SEPMLSFFI.xcframework\")"
