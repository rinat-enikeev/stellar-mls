#!/usr/bin/env bash
# Fetch the published Ethereum Foundation 2023 KZG ceremony transcript.
#
# Output: /tmp/ef-kzg-2023-transcript.json (~253 MB)
#
# Pair with `cargo run --bin extract-ef-kzg --features extract-tool --release`
# to extract the n=4096 PoT subset into the form `src/prover/srs/ef-kzg-2023.bin`
# expects.
#
# This is a one-shot operator step. Re-running it should produce a transcript
# whose SHA-256 matches the value recorded in `src/prover/srs/README.md`.

set -euo pipefail

DEST=${1:-/tmp/ef-kzg-2023-transcript.json}
URL="https://seq.ceremony.ethereum.org/info/current_state"

echo "fetching $URL → $DEST"
curl -fsSL --max-time 600 "$URL" -o "$DEST"

# Sanity-check the response shape: the endpoint name is misleading, but by
# 2026 the ceremony is finalised and the response is the canonical
# BatchTranscript JSON, not "live state". If the format ever changes, fail
# loudly here rather than producing a broken SRS.
if ! head -c 64 "$DEST" | grep -q '"transcripts"'; then
    echo "error: response does not start with '{\"transcripts\":' — endpoint format may have changed"
    exit 1
fi

ls -lh "$DEST"
shasum -a 256 "$DEST"
echo
echo "next step:"
echo "  cargo run --bin extract-ef-kzg --features extract-tool --release -- \\"
echo "    $DEST src/prover/srs/ef-kzg-2023.bin"
