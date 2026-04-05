#!/usr/bin/env bash
set -euo pipefail
set +x

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IOS_DIR="$ROOT_DIR/ios/StellarChat"
ENV_FILE="$ROOT_DIR/apple.env"
API_KEY_JSON_PATH="$ROOT_DIR/fastlane/.appstoreconnect_api_key.json"

METADATA_PATH_DEFAULT="$ROOT_DIR/fastlane/metadata"
SCREENSHOTS_PATH_DEFAULT="$ROOT_DIR/fastlane/screenshots"

METADATA_PATH="$METADATA_PATH_DEFAULT"
SCREENSHOTS_PATH="$SCREENSHOTS_PATH_DEFAULT"
SKIP_SCREENSHOTS=false
SCREENSHOTS_ONLY=false
DRY_RUN=false

usage() {
  cat <<'EOF'
Usage:
  ./deliver-ios.sh [options]

Uploads iOS metadata to App Store Connect using fastlane deliver.
Binary upload is always skipped.

Options:
  --metadata-path=<path>      Metadata folder (default: fastlane/metadata)
  --screenshots-path=<path>   Screenshots folder (default: fastlane/screenshots)
  --skip-screenshots          Upload metadata only, skip screenshots
  --screenshots-only          Upload screenshots only, skip metadata
  --dry-run                   Print commands without executing fastlane
  -h, --help                  Show this help
EOF
}

for arg in "$@"; do
  case "$arg" in
    --metadata-path=*)
      METADATA_PATH="${arg#*=}"
      ;;
    --screenshots-path=*)
      SCREENSHOTS_PATH="${arg#*=}"
      ;;
    --skip-screenshots)
      SKIP_SCREENSHOTS=true
      ;;
    --screenshots-only)
      SCREENSHOTS_ONLY=true
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
  echo "deliver-ios.sh: $1" >&2
  exit 1
}

trim() {
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "$value"
}

csv_first_value() {
  local value="$1"
  value="${value%%,*}"
  trim "$value"
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
  awk -v key="$key" '
    index($0, key "=") == 1 {
      print substr($0, length(key) + 2)
      exit
    }
  ' "$file"
}

write_api_key_json() {
  local key_id="$1"
  local issuer_id="$2"
  local key_escaped="$3"
  local in_house="$4"

  if $DRY_RUN; then
    echo "[dry-run] would write API key json: $API_KEY_JSON_PATH"
    return 0
  fi

  mkdir -p "$(dirname "$API_KEY_JSON_PATH")"
  python3 - "$key_id" "$issuer_id" "$key_escaped" "$in_house" "$API_KEY_JSON_PATH" <<'PY'
import json
import sys

key_id, issuer_id, key_escaped, in_house_raw, path = sys.argv[1:6]
in_house = str(in_house_raw).strip().lower() in ("1", "true", "yes", "y")

payload = {
    "key_id": key_id,
    "issuer_id": issuer_id,
    "key": key_escaped.replace("\\n", "\n"),
    "in_house": in_house,
}

with open(path, "w", encoding="utf-8") as f:
    json.dump(payload, f, ensure_ascii=True, indent=2)
PY
}

has_any_screenshots() {
  local dir="$1"
  [ -d "$dir" ] || return 1
  find "$dir" -type f \( -iname '*.png' -o -iname '*.jpg' -o -iname '*.jpeg' \) | grep -q .
}

require_cmd bundle
require_cmd python3

[ -d "$IOS_DIR" ] || fail "iOS directory not found: $IOS_DIR"
[ -f "$ENV_FILE" ] || fail "missing $ENV_FILE"
if ! $SCREENSHOTS_ONLY; then
  [ -d "$METADATA_PATH" ] || fail "metadata path not found: $METADATA_PATH"
fi
if $SCREENSHOTS_ONLY && $SKIP_SCREENSHOTS; then
  fail "--screenshots-only conflicts with --skip-screenshots"
fi

APP_IDENTIFIER="$(read_env_value "APP_IDENTIFIER" "$ENV_FILE" || true)"
if [ -z "$APP_IDENTIFIER" ]; then
  APP_IDENTIFIER="$(csv_first_value "$(read_env_value "MATCH_APP_IDENTIFIER" "$ENV_FILE" || true)")"
fi
[ -n "$APP_IDENTIFIER" ] || fail "APP_IDENTIFIER (or MATCH_APP_IDENTIFIER) is missing in $ENV_FILE"

ASC_KEY_ID="$(read_env_value "ASC_KEY_ID" "$ENV_FILE" || true)"
ASC_ISSUER_ID="$(read_env_value "ASC_ISSUER_ID" "$ENV_FILE" || true)"
ASC_PRIVATE_KEY_P8="$(read_env_value "ASC_PRIVATE_KEY_P8" "$ENV_FILE" || true)"
ASC_IN_HOUSE="$(read_env_value "ASC_IN_HOUSE" "$ENV_FILE" || true)"

if [ -z "$ASC_IN_HOUSE" ]; then
  ASC_IN_HOUSE="false"
fi

if [ -n "$ASC_KEY_ID" ] && [ -n "$ASC_ISSUER_ID" ] && [ -n "$ASC_PRIVATE_KEY_P8" ]; then
  write_api_key_json "$ASC_KEY_ID" "$ASC_ISSUER_ID" "$ASC_PRIVATE_KEY_P8" "$ASC_IN_HOUSE"
elif [ ! -f "$API_KEY_JSON_PATH" ]; then
  fail "ASC credentials not found in $ENV_FILE and API key json is missing at $API_KEY_JSON_PATH"
fi

run_in_dir "$ROOT_DIR" bundle config set --local path 'vendor/bundle'
run_in_dir "$ROOT_DIR" bundle install

deliver_cmd=(
  bundle exec fastlane deliver
  --api_key_path "$API_KEY_JSON_PATH"
  --app_identifier "$APP_IDENTIFIER"
  --platform ios
  --skip_binary_upload true
  --precheck_include_in_app_purchases false
  --run_precheck_before_submit false
  --force true
)

if $SCREENSHOTS_ONLY; then
  deliver_cmd+=(--skip_metadata true)
else
  deliver_cmd+=(--metadata_path "$METADATA_PATH" --skip_metadata false)
fi

if $SKIP_SCREENSHOTS; then
  deliver_cmd+=(--skip_screenshots true)
else
  if has_any_screenshots "$SCREENSHOTS_PATH"; then
    deliver_cmd+=(
      --screenshots_path "$SCREENSHOTS_PATH"
      --skip_screenshots false
      --overwrite_screenshots true
    )
  else
    echo "No screenshots found under: $SCREENSHOTS_PATH"
    echo "Proceeding with metadata upload only."
    deliver_cmd+=(--skip_screenshots true)
  fi
fi

echo "Uploading iOS metadata to App Store Connect..."
echo "App identifier: $APP_IDENTIFIER"
if $SCREENSHOTS_ONLY; then
  echo "Mode: screenshots only"
else
  echo "Metadata path: $METADATA_PATH"
fi
if ! $SKIP_SCREENSHOTS; then
  echo "Screenshots path: $SCREENSHOTS_PATH"
fi

run_in_dir "$ROOT_DIR" "${deliver_cmd[@]}"

echo "Done."
