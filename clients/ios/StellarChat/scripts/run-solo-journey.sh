#!/usr/bin/env bash
# Runs the XCUITest solo-user-journey on a simulator (default) or an attached
# device, then extracts the auto-generated Markdown report that
# MarkdownReporter prints to the xcodebuild log and saves it under
# `result_autotest/`.
#
# Usage:
#   ./scripts/run-solo-journey.sh                 # iPhone 16 Pro simulator
#   ./scripts/run-solo-journey.sh "iPhone 15"     # named simulator
#   DESTINATION='platform=iOS,id=00008140-…' ./scripts/run-solo-journey.sh
#
# Env overrides:
#   XCODEGEN     — path to xcodegen (defaults to `xcodegen` on $PATH).
#                  If absent and project.yml is newer than the .xcodeproj,
#                  the script will refuse to run.
#   DESTINATION  — full xcodebuild -destination string. Overrides the named
#                  simulator argument.
#   SCHEME       — xcodebuild scheme (default StellarChat).
#
# Exit codes mirror the xcodebuild test exit code.

set -u
set -o pipefail

# Resolve repo paths from this script's location so it works regardless of CWD.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_DIR"

SCHEME="${SCHEME:-StellarChat}"
DEFAULT_SIM_NAME="iPhone 16 Pro"
SIM_NAME="${1:-$DEFAULT_SIM_NAME}"

if [[ -n "${DESTINATION:-}" ]]; then
  DEST="$DESTINATION"
else
  DEST="platform=iOS Simulator,name=$SIM_NAME"
fi

HOST_REPORT_DIR="$PROJECT_DIR/result_autotest"
TEST_BUNDLE="StellarChatUITests"
TEST_CLASS="SoloUserJourneyUITest"

mkdir -p "$HOST_REPORT_DIR"

# Regenerate the Xcode project if project.yml is newer than the .xcodeproj
# wrapper — this is the most common cause of "tests not found" right after
# editing `project.yml`. We only do this when xcodegen is available.
if [[ -f "project.yml" ]]; then
  XCODEGEN="${XCODEGEN:-xcodegen}"
  if [[ -d "${SCHEME}.xcodeproj" ]] && [[ "project.yml" -nt "${SCHEME}.xcodeproj" ]]; then
    if command -v "$XCODEGEN" >/dev/null 2>&1; then
      echo "▶ project.yml newer than ${SCHEME}.xcodeproj — regenerating with $XCODEGEN"
      "$XCODEGEN" generate
    else
      echo "✗ project.yml is newer than ${SCHEME}.xcodeproj but xcodegen not on PATH" >&2
      echo "  install xcodegen (\`brew install xcodegen\`) or run \`xcodegen generate\` yourself" >&2
      exit 127
    fi
  elif [[ ! -d "${SCHEME}.xcodeproj" ]]; then
    if command -v "$XCODEGEN" >/dev/null 2>&1; then
      echo "▶ no .xcodeproj — generating with $XCODEGEN"
      "$XCODEGEN" generate
    else
      echo "✗ no ${SCHEME}.xcodeproj and xcodegen not on PATH" >&2
      exit 127
    fi
  fi
fi

LOG_FILE="$(mktemp -t onym-uitests-XXXXXX.log)"
trap 'rm -f "$LOG_FILE"' EXIT

echo "▶ Destination: $DEST"
echo "▶ Running ${TEST_BUNDLE}/${TEST_CLASS} …"
echo

# Run xcodebuild, tee'ing both to terminal and to a log file. xcodebuild prints
# a lot — `xcbeautify` is nicer if installed, but we don't require it.
set +e
if command -v xcbeautify >/dev/null 2>&1; then
  xcodebuild test \
    -scheme "$SCHEME" \
    -destination "$DEST" \
    -only-testing:"${TEST_BUNDLE}/${TEST_CLASS}" \
    2>&1 | tee "$LOG_FILE" | xcbeautify
  XCB_RC=${PIPESTATUS[0]}
else
  xcodebuild test \
    -scheme "$SCHEME" \
    -destination "$DEST" \
    -only-testing:"${TEST_BUNDLE}/${TEST_CLASS}" \
    2>&1 | tee "$LOG_FILE"
  XCB_RC=${PIPESTATUS[0]}
fi
set -e

# Extract the markdown report block. MarkdownReporter prints
#   === MARKDOWN REPORT BEGIN <filename> ===
#   <markdown content>
#   === MARKDOWN REPORT END <filename> ===
# Multiple blocks may be present if the test re-ran on retry; we pick the
# last one (most recent run).
echo
echo "▶ Extracting Markdown report …"
REPORT_FILE=""
LAST_BEGIN=$(grep -n '=== MARKDOWN REPORT BEGIN ' "$LOG_FILE" | tail -1 || true)
if [[ -n "$LAST_BEGIN" ]]; then
  BEGIN_LINE="${LAST_BEGIN%%:*}"
  FILE_NAME=$(echo "$LAST_BEGIN" | sed -E 's/.*BEGIN ([^ ]+) ===/\1/')
  END_LINE=$(awk -v start="$BEGIN_LINE" -v fname="$FILE_NAME" '
    NR > start && index($0, "=== MARKDOWN REPORT END " fname " ===") {print NR; exit}' "$LOG_FILE")
  if [[ -n "$END_LINE" ]]; then
    REPORT_FILE="$HOST_REPORT_DIR/$FILE_NAME"
    awk -v start="$BEGIN_LINE" -v end="$END_LINE" 'NR > start && NR < end' "$LOG_FILE" > "$REPORT_FILE"
    echo "    saved $REPORT_FILE"
  else
    echo "    ✗ found BEGIN marker but no matching END — report may be truncated" >&2
  fi
else
  echo "    ✗ no MARKDOWN REPORT block found in xcodebuild log" >&2
  echo "    (the test may have crashed before tearDown ran — see $LOG_FILE)" >&2
fi

# Show a summary of what's now in the host dir (newest first).
echo
echo "▶ Reports in $HOST_REPORT_DIR/:"
ls -1t "$HOST_REPORT_DIR" 2>/dev/null | head -5 | sed 's/^/    /' || true

echo
if [[ $XCB_RC -eq 0 ]]; then
  echo "✓ Test passed"
else
  echo "✗ Test failed (xcodebuild exit $XCB_RC) — see latest -FAIL.md for stage timeline"
  if [[ -n "$REPORT_FILE" ]]; then
    echo "    report: $REPORT_FILE"
  fi
fi
exit $XCB_RC
