#!/usr/bin/env bash
set -euo pipefail
set +x

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$ROOT_DIR/google.env"
PLAY_JSON_PATH="$ROOT_DIR/fastlane/.google_play_service_account.json"

METADATA_PATH_DEFAULT="$ROOT_DIR/fastlane/metadata/android"
METADATA_PATH="$METADATA_PATH_DEFAULT"
SKIP_SCREENSHOTS=true
DRY_RUN=false

usage() {
  cat <<'EOF'
Usage:
  ./deliver-android.sh [options]

Uploads Android metadata to Google Play using fastlane supply.
Binary upload is always skipped.

Options:
  --metadata-path=<path>   Metadata folder (default: fastlane/metadata/android)
  --with-screenshots       Upload screenshots too (skipped by default)
  --dry-run                Print commands without executing fastlane
  -h, --help               Show this help
EOF
}

for arg in "$@"; do
  case "$arg" in
    --metadata-path=*)
      METADATA_PATH="${arg#*=}"
      ;;
    --with-screenshots)
      SKIP_SCREENSHOTS=false
      ;;
    --dry-run)
      DRY_RUN=true
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $arg" >&2
      usage >&2
      exit 1
      ;;
  esac
done

fail() {
  echo "deliver-android.sh: $1" >&2
  exit 1
}

require_cmd() {
  local cmd="$1"
  command -v "$cmd" >/dev/null 2>&1 || fail "required command not found: $cmd"
}

print_cmd() {
  local out=""
  local part
  for part in "$@"; do
    if [ -z "$out" ]; then
      out="$(printf '%q' "$part")"
    else
      out="$out $(printf '%q' "$part")"
    fi
  done
  echo "$out"
}

run_in_dir() {
  local dir="$1"
  shift
  if $DRY_RUN; then
    echo "[dry-run] (cd $(printf '%q' "$dir") && $(print_cmd "$@"))"
    return 0
  fi
  (
    cd "$dir"
    "$@"
  )
}

read_env_value() {
  local key="$1"
  local file="$2"
  if [ ! -f "$file" ]; then
    return 0
  fi
  awk -F '=' -v key="$key" '
    $1 == key {
      $1=""
      sub(/^=/, "", $0)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", $0)
      print $0
      exit
    }
  ' "$file"
}

require_cmd bundle

[ -f "$ENV_FILE" ] || fail "missing $ENV_FILE — run ./setup-android-deliver.sh first"
[ -d "$METADATA_PATH" ] || fail "metadata path not found: $METADATA_PATH"
[ -f "$PLAY_JSON_PATH" ] || fail "service account JSON not found: $PLAY_JSON_PATH — run ./setup-android-deliver.sh first"

PLAY_PACKAGE_NAME="$(read_env_value "PLAY_PACKAGE_NAME" "$ENV_FILE" || true)"
PLAY_TRACK="$(read_env_value "PLAY_TRACK" "$ENV_FILE" || true)"

[ -n "$PLAY_PACKAGE_NAME" ] || fail "PLAY_PACKAGE_NAME is missing in $ENV_FILE"
if [ -z "$PLAY_TRACK" ]; then
  PLAY_TRACK="production"
fi

run_in_dir "$ROOT_DIR" bundle config set --local path 'vendor/bundle'
run_in_dir "$ROOT_DIR" bundle install

supply_cmd=(
  bundle exec fastlane supply
  --json_key "$PLAY_JSON_PATH"
  --package_name "$PLAY_PACKAGE_NAME"
  --track internal
  --metadata_path "$METADATA_PATH"
  --skip_upload_aab true
  --skip_upload_apk true
  --skip_upload_metadata false
  --skip_upload_changelogs true
  --skip_upload_images true
  --version_code 1
)

if $SKIP_SCREENSHOTS; then
  supply_cmd+=(--skip_upload_screenshots true)
else
  supply_cmd+=(--skip_upload_screenshots false)
fi

echo "Uploading Android metadata to Google Play..."
echo "Package name: $PLAY_PACKAGE_NAME"
echo "Track: $PLAY_TRACK"
echo "Metadata path: $METADATA_PATH"

run_in_dir "$ROOT_DIR" "${supply_cmd[@]}"

echo "Done."
