#!/bin/sh
# Build the Rust shared library for Android targets.
#
# Usage:
#   ./scripts/build-android.sh [--out-dir DIR]
#
# Prerequisites:
#   rustup target add aarch64-linux-android x86_64-linux-android
#   Android NDK installed via Android Studio
#
# Output:
#   <out-dir>/jniLibs/arm64-v8a/libsep_xxxx_circuits.so
#   <out-dir>/jniLibs/x86_64/libsep_xxxx_circuits.so
#
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

OUT_DIR="${REPO_ROOT}/build/android"
while [ $# -gt 0 ]; do
    case "$1" in
        --out-dir) OUT_DIR="$2"; shift 2 ;;
        *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
done

# Locate NDK
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
    # Try standard SDK location
    SDK_ROOT="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
    NDK_DIR=$(ls -d "$SDK_ROOT/ndk/"* 2>/dev/null | sort -V | tail -1)
    if [ -z "$NDK_DIR" ]; then
        echo "ERROR: No Android NDK found. Set ANDROID_NDK_HOME or install via Android Studio." >&2
        exit 1
    fi
    ANDROID_NDK_HOME="$NDK_DIR"
fi

echo "Using NDK: $ANDROID_NDK_HOME"

# NDK toolchain bin directory
TOOLCHAIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin"
if [ ! -d "$TOOLCHAIN" ]; then
    TOOLCHAIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/darwin-arm64/bin"
fi
if [ ! -d "$TOOLCHAIN" ]; then
    echo "ERROR: Cannot find NDK toolchain in $ANDROID_NDK_HOME" >&2
    exit 1
fi

API_LEVEL=26  # minSdk from the app

# Create cargo config for cross-compilation
mkdir -p "$REPO_ROOT/.cargo"
cat > "$REPO_ROOT/.cargo/config.toml" <<EOF
[target.aarch64-linux-android]
linker = "$TOOLCHAIN/aarch64-linux-android${API_LEVEL}-clang"

[target.x86_64-linux-android]
linker = "$TOOLCHAIN/x86_64-linux-android${API_LEVEL}-clang"
EOF

LIB_NAME="libsep_xxxx_circuits.so"

build_target() {
    local target="$1"
    local abi="$2"
    echo "==> Building release for $target ($abi)"
    cargo build \
        --manifest-path "$REPO_ROOT/Cargo.toml" \
        --release \
        --target "$target" \
        --lib
    mkdir -p "$OUT_DIR/jniLibs/$abi"
    cp "$REPO_ROOT/target/$target/release/$LIB_NAME" "$OUT_DIR/jniLibs/$abi/$LIB_NAME"
}

build_target aarch64-linux-android arm64-v8a
build_target x86_64-linux-android x86_64

echo
echo "Android native libraries built:"
ls -lh "$OUT_DIR/jniLibs/"*/*.so
echo
echo "Copy to your Android project:"
echo "  cp -r $OUT_DIR/jniLibs/ clients/android/StellarChat/app/src/main/jniLibs/"
