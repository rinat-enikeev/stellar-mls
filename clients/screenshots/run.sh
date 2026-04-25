#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FLOWS_DIR="$ROOT_DIR/screenshots/flows"
FLOW_FILE="$FLOWS_DIR/screenshots.yaml"

IOS_APP_ID="chat.onym.ios"
ANDROID_APP_ID="chat.onym.android"

IOS_SCREENSHOTS_BASE="$ROOT_DIR/fastlane/screenshots/en-US"
ANDROID_SCREENSHOTS_DIR="$ROOT_DIR/fastlane/metadata/android/en-US/images/phoneScreenshots"

DEFAULT_IPHONE_DEVICE="iPhone 17 Pro Max"
DEFAULT_IPAD_DEVICE="iPad Pro 13-inch (M5) (16GB)"

usage() {
  cat <<'EOF'
Usage:
  ./run.sh <target>

Targets:
  ios-iphone     Capture flow on iPhone Air simulator
                 -> fastlane/screenshots/en-US/iPhone-6.9/
  ios-ipad       Capture flow on iPad Pro 13-inch (M4) simulator
                 -> fastlane/screenshots/en-US/iPad-13/
  android-phone  Capture flow on a connected/booted Android device or emulator
                 -> fastlane/metadata/android/en-US/images/phoneScreenshots/

Prerequisites:
  - maestro CLI installed (brew install maestro)
  - For iOS: Xcode + simctl
  - For Android: adb, an emulator running or device attached
  - The app must already be installed on the target device (this script does
    not build/install — use ./build-* scripts or Xcode/Android Studio first)
EOF
}

fail() {
  echo "run.sh: $1" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1 — install it first"
}

run_flow() {
  local app_id="$1"
  local out_dir="$2"
  local device_id="${3:-}"

  mkdir -p "$out_dir"
  local tmp_dir
  tmp_dir="$(mktemp -d -t onym-screenshots.XXXXXX)"
  # Expand $tmp_dir now (single quotes would defer until trap fires, by which
  # point the local var is out of scope and `set -u` would explode).
  trap "rm -rf '$tmp_dir'" EXIT

  echo "Running Maestro flow…"
  echo "  app:    $app_id"
  echo "  flow:   $FLOW_FILE"
  echo "  out:    $out_dir"
  [ -n "$device_id" ] && echo "  device: $device_id"

  local maestro_args=()
  [ -n "$device_id" ] && maestro_args+=(--device "$device_id")
  maestro_args+=(test -e "APP_ID=$app_id" "$FLOW_FILE")

  (cd "$tmp_dir" && maestro "${maestro_args[@]}")

  shopt -s nullglob
  local pngs=("$tmp_dir"/*.png)
  shopt -u nullglob
  [ "${#pngs[@]}" -gt 0 ] || fail "Maestro produced no PNGs — flow may have failed silently"

  cp -f "${pngs[@]}" "$out_dir/"
  echo "Copied ${#pngs[@]} screenshot(s) to $out_dir"
  ls -1 "$out_dir"
}

IOS_UDID=""

boot_ios_simulator() {
  local device="$1"
  local udid

  udid="$(xcrun simctl list devices available -j | python3 -c '
import json, sys
target = sys.argv[1]
data = json.load(sys.stdin)
for runtime, devices in data["devices"].items():
    for d in devices:
        if d.get("name") == target and d.get("isAvailable", True):
            print(d["udid"])
            sys.exit(0)
sys.exit(1)
' "$device")" || fail "no available simulator named: $device — run \`xcrun simctl list devices available\` to see options"

  local state
  state="$(xcrun simctl list devices -j | python3 -c '
import json, sys
udid = sys.argv[1]
data = json.load(sys.stdin)
for devices in data["devices"].values():
    for d in devices:
        if d.get("udid") == udid:
            print(d.get("state", "Shutdown"))
            sys.exit(0)
' "$udid")"

  if [ "$state" != "Booted" ]; then
    echo "Booting simulator: $device ($udid)"
    xcrun simctl boot "$udid"
  else
    echo "Simulator already booted: $device ($udid)"
  fi
  open -a Simulator >/dev/null 2>&1 || true

  if ! xcrun simctl get_app_container "$udid" "$IOS_APP_ID" >/dev/null 2>&1; then
    fail "$IOS_APP_ID not installed on simulator. Build & install it first (Xcode > Run, or \`xcodebuild -scheme StellarChat -destination 'id=$udid'\` then \`xcrun simctl install $udid <path-to-.app>\`)"
  fi

  IOS_UDID="$udid"
}

ANDROID_SERIAL_TARGET=""

require_android_device() {
  if ! adb get-state >/dev/null 2>&1; then
    fail "no Android device/emulator detected by adb. Start an emulator or attach a device first."
  fi
  if ! adb shell pm list packages "$ANDROID_APP_ID" 2>/dev/null | grep -q "package:$ANDROID_APP_ID"; then
    fail "$ANDROID_APP_ID not installed. Run \`adb install <apk>\` or build via Android Studio first."
  fi
  ANDROID_SERIAL_TARGET="$(adb get-serialno)"
}

target="${1:-}"
[ -n "$target" ] || { usage >&2; exit 1; }

case "$target" in
  -h|--help|help)
    usage
    exit 0
    ;;
esac

[ -f "$FLOW_FILE" ] || fail "flow file missing: $FLOW_FILE"
require_cmd maestro

case "$target" in
  ios-iphone)
    require_cmd xcrun
    boot_ios_simulator "$DEFAULT_IPHONE_DEVICE"
    run_flow "$IOS_APP_ID" "$IOS_SCREENSHOTS_BASE/iPhone-6.9" "$IOS_UDID"
    ;;
  ios-ipad)
    require_cmd xcrun
    boot_ios_simulator "$DEFAULT_IPAD_DEVICE"
    run_flow "$IOS_APP_ID" "$IOS_SCREENSHOTS_BASE/iPad-13" "$IOS_UDID"
    ;;
  android-phone)
    require_cmd adb
    require_android_device
    run_flow "$ANDROID_APP_ID" "$ANDROID_SCREENSHOTS_DIR" "$ANDROID_SERIAL_TARGET"
    ;;
  *)
    echo "Unknown target: $target" >&2
    usage >&2
    exit 1
    ;;
esac

echo "Done."
