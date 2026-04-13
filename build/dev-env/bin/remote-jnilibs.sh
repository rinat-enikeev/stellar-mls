#!/bin/bash
# remote-jnilibs <sha> [out-dir]
#
# Ask the Mac host to build the Android JNI .so files for the given SHA and
# stream them back as a tar on stdout. Logs come over stderr. The tar unpacks
# into <out-dir>/jniLibs/{arm64-v8a,x86_64}/libsep_xxxx_circuits.so, which is
# exactly where scripts/build-android.sh writes them and where Gradle expects
# to find them (via copy into app/src/main/jniLibs).
#
# Default out-dir is $PWD/build/android, matching scripts/build-android.sh.
set -euo pipefail

: "${MAC_HOST:?MAC_HOST must be set (see qa.env / release.env)}"
: "${MAC_HOST_USER:?MAC_HOST_USER must be set}"
MAC_HOST_PORT="${MAC_HOST_PORT:-22}"

SHA="${1:-HEAD}"
OUT_DIR="${2:-$PWD/build/android}"

if [ "$SHA" = "HEAD" ]; then
    SHA=$(git -C "${PWD}" rev-parse HEAD 2>/dev/null || echo "")
fi
if [ -z "$SHA" ]; then
    echo "usage: remote-jnilibs <sha> [out-dir]   (or run inside a git checkout)" >&2
    exit 2
fi

mkdir -p "$OUT_DIR"
echo "==> fetching jniLibs for $SHA from ${MAC_HOST_USER}@${MAC_HOST}:${MAC_HOST_PORT}" >&2
echo "    extract target: $OUT_DIR/jniLibs/" >&2

ssh \
    -p "$MAC_HOST_PORT" \
    -o BatchMode=yes \
    -o StrictHostKeyChecking=accept-new \
    -o ServerAliveInterval=30 \
    "${MAC_HOST_USER}@${MAC_HOST}" \
    "jnilibs $SHA" \
  | tar -x -C "$OUT_DIR"

echo "==> jniLibs extracted" >&2
ls -lh "$OUT_DIR"/jniLibs/*/*.so 2>/dev/null >&2 || {
    echo "ERROR: no .so files produced" >&2
    exit 1
}
