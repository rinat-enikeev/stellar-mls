#!/usr/bin/env bash
# Sign + notarize a single ceremony_tool Mach-O binary.
#
# Reads scripts/.env (or $SIGN_ENV) for Apple credentials. See
# docs/ceremony-tool-signing.md for the full setup walkthrough.

set -euo pipefail

usage() {
  cat <<EOF >&2
usage: $0 <path-to-binary>

Reads scripts/.env (or \$SIGN_ENV) for Apple credentials and produces a
notarized binary in place. Idempotent — safe to re-run.
EOF
  exit 1
}

[ $# -eq 1 ] || usage
BIN="$1"
[ -f "$BIN" ] || { echo "no such file: $BIN" >&2; exit 1; }

case "$(uname -s)" in
  Darwin) ;;
  *) echo "this script must run on macOS" >&2; exit 1 ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="${SIGN_ENV:-$SCRIPT_DIR/.env}"
if [ -r "$ENV_FILE" ]; then
  set -a
  # shellcheck disable=SC1090
  . "$ENV_FILE"
  set +a
else
  echo "warning: $ENV_FILE not found; relying on already-exported env" >&2
fi

: "${APPLE_DEVELOPER_ID:?set APPLE_DEVELOPER_ID in $ENV_FILE}"
: "${APPLE_ID:?set APPLE_ID in $ENV_FILE}"
: "${APPLE_TEAM_ID:?set APPLE_TEAM_ID in $ENV_FILE}"
: "${APPLE_APP_PASSWORD:?set APPLE_APP_PASSWORD in $ENV_FILE}"

# If a .p12 is provided, import it into a temporary keychain and use it
# for this run. Lets the same script work in CI where there's no login
# keychain, while staying out of the way locally.
KEYCHAIN_TMP=""
cleanup() {
  if [ -n "$KEYCHAIN_TMP" ] && [ -f "$KEYCHAIN_TMP" ]; then
    security delete-keychain "$KEYCHAIN_TMP" 2>/dev/null || true
    rm -f "$KEYCHAIN_TMP"
  fi
}
trap cleanup EXIT

if [ -n "${APPLE_CERT_P12_BASE64:-}" ]; then
  : "${APPLE_CERT_P12_PASSWORD:?set APPLE_CERT_P12_PASSWORD when APPLE_CERT_P12_BASE64 is set}"
  KEYCHAIN_TMP="$(mktemp -t ceremony-keychain).keychain"
  KEYCHAIN_PWD="$(openssl rand -hex 16)"
  P12_PATH="$(mktemp -t ceremony-cert).p12"
  printf '%s' "$APPLE_CERT_P12_BASE64" | base64 -d > "$P12_PATH"
  security create-keychain -p "$KEYCHAIN_PWD" "$KEYCHAIN_TMP" >/dev/null
  security set-keychain-settings -lut 21600 "$KEYCHAIN_TMP" >/dev/null
  security unlock-keychain -p "$KEYCHAIN_PWD" "$KEYCHAIN_TMP"
  security import "$P12_PATH" -k "$KEYCHAIN_TMP" \
    -P "$APPLE_CERT_P12_PASSWORD" \
    -T /usr/bin/codesign >/dev/null
  security set-key-partition-list -S apple-tool:,apple:,codesign: \
    -s -k "$KEYCHAIN_PWD" "$KEYCHAIN_TMP" >/dev/null
  # Make this keychain searchable in addition to the existing list.
  EXISTING="$(security list-keychains -d user | sed -e 's/^[[:space:]]*"//' -e 's/"$//')"
  # shellcheck disable=SC2086
  security list-keychains -d user -s "$KEYCHAIN_TMP" $EXISTING
  shred -u "$P12_PATH" 2>/dev/null || rm -f "$P12_PATH"
fi

PROFILE="${APPLE_NOTARYTOOL_PROFILE:-ceremony-notarytool}"
echo "==> store-credentials notarytool profile: $PROFILE"
xcrun notarytool store-credentials "$PROFILE" \
  --apple-id "$APPLE_ID" \
  --team-id "$APPLE_TEAM_ID" \
  --password "$APPLE_APP_PASSWORD" >/dev/null

echo "==> codesign $BIN"
codesign --force --options runtime --timestamp \
  --sign "$APPLE_DEVELOPER_ID" "$BIN"
codesign --verify --strict --verbose=2 "$BIN"

ZIP="${BIN}.notarize.zip"
echo "==> ditto $ZIP"
rm -f "$ZIP"
ditto -c -k --keepParent "$BIN" "$ZIP"

echo "==> notarytool submit (this can take a few minutes)"
xcrun notarytool submit "$ZIP" --keychain-profile "$PROFILE" --wait

rm -f "$ZIP"

echo "==> spctl assess"
spctl --assess -vvv --type execute "$BIN" || true

echo "done. signed + notarized: $BIN"
