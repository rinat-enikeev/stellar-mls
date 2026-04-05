#!/bin/bash
# Build the F-Droid release APK using F-Droid's official buildserver image.
# This produces a deterministic APK identical to what F-Droid CI builds.
#
# Usage:
#   ./scripts/build-fdroid-release.sh [--sign /path/to/keystore.jks]
#
# Output:
#   app-fdroid-release-<version>.apk in the repo root
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
KEYSTORE=""

while [ $# -gt 0 ]; do
    case "$1" in
        --sign) KEYSTORE="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"; shift 2 ;;
        *) echo "Usage: $0 [--sign /path/to/keystore.jks]" >&2; exit 1 ;;
    esac
done

# Extract version from build.gradle.kts
VERSION_NAME=$(grep 'versionName' "$REPO_ROOT/clients/android/StellarChat/app/build.gradle.kts" | head -1 | sed 's/.*"\(.*\)".*/\1/')
echo "Building F-Droid release v${VERSION_NAME}"

echo "==> Building in F-Droid buildserver container..."
mkdir -p "$REPO_ROOT/build/fdroid-output"

docker run --rm --platform linux/amd64 \
    -v "$REPO_ROOT":/src:ro \
    -v "$REPO_ROOT/build/fdroid-output":/output \
    registry.gitlab.com/fdroid/fdroidserver:buildserver-trixie bash -c '
set -euo pipefail

# === sudo step (from fdroiddata metadata) ===
apt-get update
apt-get install -y rustup build-essential

# === init step (from fdroiddata metadata) ===
rustup default 1.94.1
rustup target add aarch64-linux-android x86_64-linux-android

# Copy source (read-only mount, need writable copy)
rm -rf /home/vagrant/build
mkdir -p /home/vagrant/build
cp -a /src/. /home/vagrant/build
cd /home/vagrant/build

# Sanity check: verify repo root layout
test -f scripts/build-android.sh
test -f clients/android/StellarChat/gradlew

# === scandelete step ===
rm -rf kotlin-mls/src/main/jniLibs/
rm -rf clients/android/StellarChat/app/src/main/jniLibs/

# === Install NDK r27c (matches ndk: r27c in fdroid metadata) ===
NDK_DIR="/opt/android-sdk/ndk/27.2.12479018"
echo "==> Existing NDK versions:"
ls /opt/android-sdk/ndk/ 2>/dev/null || echo "(none)"
if [ ! -d "$NDK_DIR" ]; then
    echo "==> Downloading NDK r27c..."
    apt-get install -y -q unzip curl > /dev/null 2>&1 || true
    curl -sLO "https://dl.google.com/android/repository/android-ndk-r27c-linux.zip"
    unzip -q android-ndk-r27c-linux.zip -d /opt/android-sdk/ndk-tmp
    mv /opt/android-sdk/ndk-tmp/android-ndk-r27c "$NDK_DIR"
    rm -rf /opt/android-sdk/ndk-tmp android-ndk-r27c-linux.zip
fi
echo "==> NDK at $NDK_DIR"

# === build step (from fdroiddata metadata) ===
export PATH="$HOME/.cargo/bin:$PATH"
ANDROID_NDK_HOME="$NDK_DIR" bash scripts/build-android.sh --out-dir kotlin-mls/src/main

# === prebuild step ===
rm -f clients/android/StellarChat/app/google-services.json

# === gradle build ===
cd clients/android/StellarChat
echo "sdk.dir=/opt/android-sdk" > local.properties
./gradlew assembleFdroidRelease

# Copy output
cp app/build/outputs/apk/fdroid/release/app-fdroid-release-unsigned.apk /output/
'

OUTPUT_APK="$REPO_ROOT/build/fdroid-output/app-fdroid-release-unsigned.apk"

if [ ! -f "$OUTPUT_APK" ]; then
    echo "ERROR: Build failed — no APK produced" >&2
    exit 1
fi

FINAL_APK="$REPO_ROOT/app-fdroid-release-${VERSION_NAME}.apk"
cp "$OUTPUT_APK" "$FINAL_APK"

# Sign if keystore provided
if [ -n "$KEYSTORE" ]; then
    echo "==> Signing APK with $KEYSTORE"
    APKSIGNER=$(find "${ANDROID_HOME:-}" ~/Library/Android/sdk -name "apksigner" 2>/dev/null | sort -V | tail -1)
    if [ -z "$APKSIGNER" ]; then
        echo "ERROR: apksigner not found" >&2
        exit 1
    fi
    "$APKSIGNER" sign --ks "$KEYSTORE" "$FINAL_APK"
    echo "==> Signed: $FINAL_APK"
else
    echo "==> Unsigned APK: $FINAL_APK"
    echo "    Sign with: apksigner sign --ks /path/to/keystore.jks $FINAL_APK"
fi

echo "==> Done: $FINAL_APK ($(du -h "$FINAL_APK" | cut -f1))"
