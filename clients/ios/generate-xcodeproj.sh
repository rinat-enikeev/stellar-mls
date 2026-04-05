#!/usr/bin/env bash
#
# generate-xcodeproj.sh — Run xcodegen with team/signing from apple.env
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
APPLE_ENV="$SCRIPT_DIR/../apple.env"
IOS_DIR="$SCRIPT_DIR/StellarChat"

if [ -f "$APPLE_ENV" ]; then
    DEVELOPMENT_TEAM=$(sed -n 's/^MATCH_TEAM_ID=//p' "$APPLE_ENV" | head -1)
    export DEVELOPMENT_TEAM
    echo "==> Team ID: $DEVELOPMENT_TEAM"
else
    export DEVELOPMENT_TEAM=""
    echo "==> Warning: clients/apple.env not found, DEVELOPMENT_TEAM will be empty"
fi

cd "$IOS_DIR"
xcodegen generate
echo "==> Xcode project generated"
