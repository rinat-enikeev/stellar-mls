#!/bin/bash
# remote-xcodebuild <sha>
#
# Invoke the Mac host forced-command dispatcher to run an iOS build for the
# given commit SHA. The container's SSH key is pinned on the host with a
# command= directive, so no arguments other than $SSH_ORIGINAL_COMMAND get
# honoured — the "ios <sha>" string below is parsed by host-build-dispatch.
#
# Build logs stream over stderr; a final "ARTIFACT: <path>" line names the
# .ipa the dispatcher produced on the host (not copied back automatically —
# use scp with the same key if you need the file inside the container).
set -euo pipefail

: "${MAC_HOST:?MAC_HOST must be set (see qa.env / release.env)}"
: "${MAC_HOST_USER:?MAC_HOST_USER must be set}"
MAC_HOST_PORT="${MAC_HOST_PORT:-22}"

SHA="${1:-HEAD}"
if [ "$SHA" = "HEAD" ]; then
    SHA=$(git -C "${PWD}" rev-parse HEAD 2>/dev/null || echo "")
fi
if [ -z "$SHA" ]; then
    echo "usage: remote-xcodebuild <sha>    (or run from inside a git checkout)" >&2
    exit 2
fi

exec ssh \
    -p "$MAC_HOST_PORT" \
    -o BatchMode=yes \
    -o StrictHostKeyChecking=accept-new \
    -o ServerAliveInterval=30 \
    "${MAC_HOST_USER}@${MAC_HOST}" \
    "ios $SHA"
