#!/usr/bin/env bash
# Runs the Compose UI solo-user-journey instrumentation test on the connected
# device, then `adb pull`s the auto-generated Markdown reports written by
# MarkdownReportRule into ./result_autotest/ on the host.
#
# Usage:
#   ./scripts/run-solo-journey.sh
#
# Env overrides:
#   ADB     — path to adb (defaults to `adb` on $PATH; falls back to
#             $ANDROID_HOME/platform-tools/adb when ADB is unset and `adb`
#             isn't found).
#   GRADLE  — path to gradle wrapper (defaults to ./gradlew).
#
# Exit codes mirror the gradle test exit code.

set -u
set -o pipefail

# Resolve repo paths from this script's location so it works regardless of CWD.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_DIR"

# Resolve adb.
if [[ -z "${ADB:-}" ]]; then
  if command -v adb >/dev/null 2>&1; then
    ADB="adb"
  elif [[ -n "${ANDROID_HOME:-}" && -x "$ANDROID_HOME/platform-tools/adb" ]]; then
    ADB="$ANDROID_HOME/platform-tools/adb"
  elif [[ -x "$HOME/AppData/Local/Android/Sdk/platform-tools/adb.exe" ]]; then
    ADB="$HOME/AppData/Local/Android/Sdk/platform-tools/adb.exe"
  else
    echo "✗ adb not found — set ADB or ANDROID_HOME" >&2
    exit 127
  fi
fi

GRADLE="${GRADLE:-./gradlew}"
APP_PKG="chat.onym.android"
DEVICE_REPORT_DIR="/sdcard/Android/data/$APP_PKG/files/autotest-reports"
HOST_REPORT_DIR="$PROJECT_DIR/result_autotest"
TEST_CLASS="uitests.SoloUserJourneyTest"

mkdir -p "$HOST_REPORT_DIR"

# Verify a device is attached before we burn 2+ minutes on a build.
DEVICES=$("$ADB" devices | awk 'NR>1 && $2=="device" {print $1}')
if [[ -z "$DEVICES" ]]; then
  echo "✗ no device attached — connect a device or start an emulator" >&2
  exit 1
fi

echo "▶ Device(s):"
echo "$DEVICES" | sed 's/^/    /'
echo "▶ Running $TEST_CLASS …"
echo

# Run gradle, preserving its exit code.
"$GRADLE" :app:connectedPlayDebugAndroidTest \
  -Pandroid.testInstrumentationRunnerArguments.class="$TEST_CLASS"
GRADLE_RC=$?

# Always try to pull whatever the rule managed to write — even on failure
# (especially on failure: that's where logcat lives).
echo
echo "▶ Pulling reports from $DEVICE_REPORT_DIR …"
PULL_OUT=$("$ADB" pull "$DEVICE_REPORT_DIR" "$HOST_REPORT_DIR/" 2>&1) || true
echo "$PULL_OUT" | tail -5

# Show a summary of what's now in the host dir (newest first).
echo
echo "▶ Reports in $HOST_REPORT_DIR/$(basename "$DEVICE_REPORT_DIR")/:"
LATEST_DIR="$HOST_REPORT_DIR/$(basename "$DEVICE_REPORT_DIR")"
if [[ -d "$LATEST_DIR" ]]; then
  ls -1t "$LATEST_DIR" 2>/dev/null | head -5 | sed 's/^/    /'
else
  echo "    (no reports yet — was the test built and installed?)"
fi

echo
if [[ $GRADLE_RC -eq 0 ]]; then
  echo "✓ Test passed"
else
  echo "✗ Test failed (gradle exit $GRADLE_RC) — see latest -FAIL.md for stage timeline + logcat"
fi
exit $GRADLE_RC
