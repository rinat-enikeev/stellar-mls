#!/usr/bin/env bash
# Build, sign, notarize, and staple the Onym Ceremony GUI .app and .dmg.
#
# Inputs (env or flags):
#   VERSION              version string baked into Info.plist (e.g. 1.5.5).
#   CLI_BIN_X86_64       path to the signed darwin x86_64 ceremony_tool binary.
#   CLI_BIN_ARM64        path to the signed darwin arm64 ceremony_tool binary.
#   OUT_DIR              where to emit CeremonyTool-<version>.dmg (default: dist/).
#
# Apple credentials are read from scripts/.env (or $SIGN_ENV) the same way
# scripts/sign-macos.sh reads them — APPLE_DEVELOPER_ID, APPLE_ID,
# APPLE_TEAM_ID, APPLE_APP_PASSWORD, plus optional
# APPLE_CERT_P12_BASE64 / APPLE_CERT_P12_PASSWORD for ephemeral keychain.

set -euo pipefail

usage() {
  cat <<EOF >&2
usage: $0 --version VER --x86_64 PATH --arm64 PATH [--out DIR]

Builds CeremonyTool.app (universal), signs + notarizes + staples it,
and packages a CeremonyTool-<version>.dmg.
EOF
  exit 1
}

VERSION=""
CLI_X86_64=""
CLI_ARM64=""
OUT_DIR="dist"

while [ $# -gt 0 ]; do
  case "$1" in
    --version)  VERSION="$2"; shift 2 ;;
    --x86_64)   CLI_X86_64="$2"; shift 2 ;;
    --arm64)    CLI_ARM64="$2"; shift 2 ;;
    --out)      OUT_DIR="$2"; shift 2 ;;
    -h|--help)  usage ;;
    *)          echo "unknown arg: $1" >&2; usage ;;
  esac
done

VERSION="${VERSION:-${VERSION_ENV:-}}"
[ -n "$VERSION" ] || { echo "missing --version" >&2; usage; }
[ -f "$CLI_X86_64" ] || { echo "missing --x86_64 binary: $CLI_X86_64" >&2; exit 1; }
[ -f "$CLI_ARM64" ]  || { echo "missing --arm64 binary: $CLI_ARM64" >&2; exit 1; }

case "$(uname -s)" in
  Darwin) ;;
  *) echo "this script must run on macOS" >&2; exit 1 ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ENV_FILE="${SIGN_ENV:-$REPO_ROOT/scripts/.env}"
if [ -r "$ENV_FILE" ]; then
  set -a
  # shellcheck disable=SC1090
  . "$ENV_FILE"
  set +a
else
  echo "warning: $ENV_FILE not found; relying on already-exported env" >&2
fi

: "${APPLE_DEVELOPER_ID:?set APPLE_DEVELOPER_ID}"
: "${APPLE_ID:?set APPLE_ID}"
: "${APPLE_TEAM_ID:?set APPLE_TEAM_ID}"
: "${APPLE_APP_PASSWORD:?set APPLE_APP_PASSWORD}"

# ----- Ephemeral keychain (mirrors scripts/sign-macos.sh) ---------------------
KEYCHAIN_TMP=""
cleanup_kc() {
  if [ -n "$KEYCHAIN_TMP" ] && [ -f "$KEYCHAIN_TMP" ]; then
    security delete-keychain "$KEYCHAIN_TMP" 2>/dev/null || true
    rm -f "$KEYCHAIN_TMP"
  fi
}
trap cleanup_kc EXIT

if [ -n "${APPLE_CERT_P12_BASE64:-}" ]; then
  : "${APPLE_CERT_P12_PASSWORD:?set APPLE_CERT_P12_PASSWORD when APPLE_CERT_P12_BASE64 is set}"
  KEYCHAIN_TMP="$(mktemp -t ceremony-gui-kc).keychain"
  KEYCHAIN_PWD="$(openssl rand -hex 16)"
  P12_PATH="$(mktemp -t ceremony-gui-cert).p12"
  STRIPPED=$(printf '%s' "$APPLE_CERT_P12_BASE64" | tr -d ' \t\r\n"'"'"'')
  printf '%s' "$STRIPPED" | base64 -d > "$P12_PATH"
  [ -s "$P12_PATH" ] || { echo "decoded .p12 is empty" >&2; exit 1; }
  security create-keychain -p "$KEYCHAIN_PWD" "$KEYCHAIN_TMP" >/dev/null
  security set-keychain-settings -lut 21600 "$KEYCHAIN_TMP" >/dev/null
  security unlock-keychain -p "$KEYCHAIN_PWD" "$KEYCHAIN_TMP"
  security import "$P12_PATH" -k "$KEYCHAIN_TMP" \
    -P "$APPLE_CERT_P12_PASSWORD" -T /usr/bin/codesign >/dev/null
  security set-key-partition-list -S apple-tool:,apple:,codesign: \
    -s -k "$KEYCHAIN_PWD" "$KEYCHAIN_TMP" >/dev/null
  EXISTING="$(security list-keychains -d user | sed -e 's/^[[:space:]]*"//' -e 's/"$//')"
  # shellcheck disable=SC2086
  security list-keychains -d user -s "$KEYCHAIN_TMP" $EXISTING
  shred -u "$P12_PATH" 2>/dev/null || rm -f "$P12_PATH"
fi

PROFILE="${APPLE_NOTARYTOOL_PROFILE:-ceremony-notarytool}"
xcrun notarytool store-credentials "$PROFILE" \
  --apple-id "$APPLE_ID" \
  --team-id "$APPLE_TEAM_ID" \
  --password "$APPLE_APP_PASSWORD" >/dev/null

# ----- Compile Swift for both arches -----------------------------------------
mkdir -p "$OUT_DIR"
WORK="$(mktemp -d -t ceremony-gui-build)"
echo "==> work dir: $WORK"
trap 'cleanup_kc; rm -rf "$WORK"' EXIT

SRC_DIR="$SCRIPT_DIR/Sources/CeremonyTool"
# macOS ships bash 3.2; mapfile/readarray is bash 4+. Use a portable loop.
SRCS=()
while IFS= read -r f; do SRCS+=("$f"); done < <(find "$SRC_DIR" -name '*.swift' | sort)
echo "==> sources:"
printf '    %s\n' "${SRCS[@]}"

SDK_PATH="$(xcrun --sdk macosx --show-sdk-path)"

build_arch() {
  local arch="$1"
  local triple="$2"
  local out="$WORK/CeremonyTool-$arch"
  # Diagnostics go to stderr so command substitution captures only the path.
  echo "==> swiftc ($arch)" >&2
  xcrun swiftc \
    -sdk "$SDK_PATH" \
    -target "$triple" \
    -O \
    -parse-as-library \
    -module-name CeremonyTool \
    -o "$out" \
    "${SRCS[@]}" >&2
  echo "$out"
}

BIN_ARM64="$(build_arch arm64 arm64-apple-macos13.0)"
BIN_X86_64="$(build_arch x86_64 x86_64-apple-macos13.0)"

# ----- Lipo into universal app binary ----------------------------------------
APP_BIN="$WORK/CeremonyTool"
echo "==> lipo app binary"
lipo -create -output "$APP_BIN" "$BIN_ARM64" "$BIN_X86_64"
lipo -info "$APP_BIN"

# ----- Lipo bundled ceremony_tool CLI ----------------------------------------
TOOL_BIN="$WORK/ceremony_tool"
echo "==> lipo bundled ceremony_tool"
lipo -create -output "$TOOL_BIN" "$CLI_ARM64" "$CLI_X86_64"
lipo -info "$TOOL_BIN"
chmod +x "$TOOL_BIN"

# ----- Assemble .app bundle --------------------------------------------------
APP="$WORK/CeremonyTool.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
mv "$APP_BIN" "$APP/Contents/MacOS/CeremonyTool"
mv "$TOOL_BIN" "$APP/Contents/Resources/ceremony_tool"

# ----- Render app icon ------------------------------------------------------
# assets/make-iconset.swift draws every iconset size in CoreGraphics so the
# build has no external dependencies beyond the Swift toolchain.
ICONSET="$WORK/AppIcon.iconset"
echo "==> render iconset"
xcrun swift "$SCRIPT_DIR/assets/make-iconset.swift" "$ICONSET"
echo "==> iconutil -c icns"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

INFO_TMPL="$SCRIPT_DIR/Info.plist.template"
sed "s/__VERSION__/${VERSION}/g" "$INFO_TMPL" > "$APP/Contents/Info.plist"

ENT="$SCRIPT_DIR/CeremonyTool.entitlements"

# ----- Sign inner -> outer -> bundle -----------------------------------------
echo "==> codesign bundled ceremony_tool"
codesign --force --options runtime --timestamp \
  --sign "$APPLE_DEVELOPER_ID" \
  "$APP/Contents/Resources/ceremony_tool"

echo "==> codesign app binary"
codesign --force --options runtime --timestamp \
  --entitlements "$ENT" \
  --sign "$APPLE_DEVELOPER_ID" \
  "$APP/Contents/MacOS/CeremonyTool"

echo "==> codesign .app bundle"
codesign --force --options runtime --timestamp \
  --entitlements "$ENT" \
  --sign "$APPLE_DEVELOPER_ID" \
  "$APP"

codesign --verify --deep --strict --verbose=2 "$APP"

# ----- Notarize the .app via a transient zip (faster than notarizing dmg) ----
NOTARY_ZIP="$WORK/CeremonyTool.zip"
ditto -c -k --keepParent "$APP" "$NOTARY_ZIP"
echo "==> notarytool submit (app)"
xcrun notarytool submit "$NOTARY_ZIP" --keychain-profile "$PROFILE" --wait
echo "==> stapler staple app"
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"

# ----- Build .dmg ------------------------------------------------------------
DMG_NAME="CeremonyTool-${VERSION}.dmg"
DMG_OUT="$OUT_DIR/$DMG_NAME"
DMG_STAGE="$WORK/dmg-stage"
mkdir -p "$DMG_STAGE"
ditto "$APP" "$DMG_STAGE/CeremonyTool.app"
ln -s /Applications "$DMG_STAGE/Applications"

rm -f "$DMG_OUT"
echo "==> hdiutil create $DMG_OUT"
hdiutil create \
  -volname "Onym Ceremony Tool" \
  -srcfolder "$DMG_STAGE" \
  -ov -format UDZO \
  "$DMG_OUT"

# Sign + notarize the dmg too so Safari's quarantine extends-attrs honor it.
echo "==> codesign dmg"
codesign --force --timestamp --sign "$APPLE_DEVELOPER_ID" "$DMG_OUT"
echo "==> notarytool submit (dmg)"
xcrun notarytool submit "$DMG_OUT" --keychain-profile "$PROFILE" --wait
echo "==> stapler staple dmg"
xcrun stapler staple "$DMG_OUT"

shasum -a 256 "$DMG_OUT" > "$DMG_OUT.sha256"
echo "==> done: $DMG_OUT"
ls -lh "$DMG_OUT" "$DMG_OUT.sha256"
