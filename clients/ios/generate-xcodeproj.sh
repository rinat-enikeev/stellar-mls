#!/usr/bin/env bash
#
# generate-xcodeproj.sh — Run xcodegen with team/signing from apple.env
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APPLE_ENV="$SCRIPT_DIR/../apple.env"
ROOT_ENV="$REPO_ROOT/.env"
IOS_DIR="$SCRIPT_DIR/StellarChat"

# Resolve DEVELOPMENT_TEAM: apple.env → root .env → empty
if [ -f "$APPLE_ENV" ]; then
    DEVELOPMENT_TEAM=$(sed -n 's/^MATCH_TEAM_ID=//p' "$APPLE_ENV" | head -1)
fi
if [ -z "${DEVELOPMENT_TEAM:-}" ] && [ -f "$ROOT_ENV" ]; then
    DEVELOPMENT_TEAM=$(sed -n 's/^DEVELOPMENT_TEAM=//p' "$ROOT_ENV" | head -1)
fi
if [ -z "${DEVELOPMENT_TEAM:-}" ]; then
    echo "==> Warning: DEVELOPMENT_TEAM not found in apple.env or .env"
fi
export DEVELOPMENT_TEAM="${DEVELOPMENT_TEAM:-}"
echo "==> Team ID: $DEVELOPMENT_TEAM"

cd "$IOS_DIR"
xcodegen generate
echo "==> Xcode project generated"
