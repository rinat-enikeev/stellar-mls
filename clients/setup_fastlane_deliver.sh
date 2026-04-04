#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$ROOT_DIR/apple.env"
API_KEY_JSON_PATH="$ROOT_DIR/fastlane/.appstoreconnect_api_key.json"
PBXPROJ_FILE="$ROOT_DIR/ios/StellarChat/StellarChat.xcodeproj/project.pbxproj"
PROJECT_YML_FILE="$ROOT_DIR/ios/StellarChat/project.yml"

fail() {
  echo "setup_fastlane_deliver.sh: $1" >&2
  exit 1
}

require_cmd() {
  local cmd="$1"
  command -v "$cmd" >/dev/null 2>&1 || fail "required command not found: $cmd"
}

trim() {
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "$value"
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
      print $0
      exit
    }
  ' "$file"
}

upsert_env() {
  local key="$1"
  local value="$2"
  local file="$3"
  local tmp_file
  tmp_file="$(mktemp)"

  if [ -f "$file" ]; then
    awk -v key="$key" -v value="$value" '
      BEGIN { done = 0 }
      $0 ~ ("^[[:space:]]*" key "[[:space:]]*=") {
        if (!done) {
          print key "=" value
          done = 1
        }
        next
      }
      { print }
      END {
        if (!done) {
          print key "=" value
        }
      }
    ' "$file" > "$tmp_file"
  else
    printf "%s=%s\n" "$key" "$value" > "$tmp_file"
  fi

  mv "$tmp_file" "$file"
}

prompt_required() {
  local label="$1"
  local default_value="${2:-}"
  local value=""
  while [ -z "$value" ]; do
    if [ -n "$default_value" ]; then
      read -r -p "$label [$default_value]: " value
    else
      read -r -p "$label: " value
    fi
    value="$(trim "$value")"
    if [ -z "$value" ] && [ -n "$default_value" ]; then
      value="$default_value"
    fi
  done
  printf '%s' "$value"
}

prompt_yes_no() {
  local label="$1"
  local default_yes="$2"
  local input=""
  local normalized=""
  while true; do
    if [ "$default_yes" = "true" ]; then
      read -r -p "$label [Y/n]: " input
      normalized="$(printf '%s' "$input" | tr '[:upper:]' '[:lower:]' | xargs)"
      if [ -z "$normalized" ] || [ "$normalized" = "y" ] || [ "$normalized" = "yes" ]; then
        return 0
      fi
      if [ "$normalized" = "n" ] || [ "$normalized" = "no" ]; then
        return 1
      fi
    else
      read -r -p "$label [y/N]: " input
      normalized="$(printf '%s' "$input" | tr '[:upper:]' '[:lower:]' | xargs)"
      if [ -z "$normalized" ] || [ "$normalized" = "n" ] || [ "$normalized" = "no" ]; then
        return 1
      fi
      if [ "$normalized" = "y" ] || [ "$normalized" = "yes" ]; then
        return 0
      fi
    fi
    echo "Please answer y or n."
  done
}

read_private_key_p8_escaped() {
  local existing="$1"
  local path=""
  local key=""

  if [ -n "$existing" ]; then
    if prompt_yes_no "Reuse existing ASC private key from apple.env?" true; then
      printf '%s' "$existing"
      return
    fi
  fi

  read -r -p "Path to ASC API private key .p8 (leave empty to paste): " path
  path="$(trim "$path")"
  path="${path/#\~/$HOME}"

  if [ -n "$path" ]; then
    [ -f "$path" ] || fail "file not found: $path"
    key="$(cat "$path")"
  else
    echo "Paste ASC private key (.p8). End with line: END"
    local line
    local lines=()
    while IFS= read -r line; do
      if [ "$line" = "END" ]; then
        break
      fi
      lines+=("$line")
    done
    key="$(printf '%s\n' "${lines[@]}")"
  fi

  key="$(printf '%s' "$key" | sed -E 's/[[:space:]]+$//')"
  [ -n "$key" ] || fail "ASC private key is empty"
  printf '%s' "${key//$'\n'/\\n}"
}

extract_default_bundle_id() {
  if [ -f "$PROJECT_YML_FILE" ]; then
    awk '/PRODUCT_BUNDLE_IDENTIFIER:/ {print $2; exit}' "$PROJECT_YML_FILE"
    return 0
  fi
  [ -f "$PBXPROJ_FILE" ] || return 0
  awk '
    /PRODUCT_BUNDLE_IDENTIFIER = / {
      line = $0
      sub(/^.*PRODUCT_BUNDLE_IDENTIFIER = /, "", line)
      sub(/;.*/, "", line)
      if (line ~ /\.tests$/) next
      gsub(/[[:space:]]/, "", line)
      if (line != "") {
        print line
        exit
      }
    }
  ' "$PBXPROJ_FILE"
}

write_api_key_json() {
  local key_id="$1"
  local issuer_id="$2"
  local key_escaped="$3"
  local in_house="$4"
  mkdir -p "$(dirname "$API_KEY_JSON_PATH")"
  python3 - "$key_id" "$issuer_id" "$key_escaped" "$in_house" "$API_KEY_JSON_PATH" <<'PY'
import json
import sys

key_id = sys.argv[1]
issuer_id = sys.argv[2]
key_escaped = sys.argv[3]
in_house_raw = sys.argv[4]
path = sys.argv[5]
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

ensure_dir() {
  local path="$1"
  if [ -d "$path" ]; then
    return
  fi
  if prompt_yes_no "Directory '$path' does not exist. Create it?" true; then
    mkdir -p "$path"
  fi
}

require_cmd ruby
require_cmd bundle
require_cmd python3

cd "$ROOT_DIR"

existing_app_identifier="$(read_env_value "APP_IDENTIFIER" "$ENV_FILE" || true)"
if [ -z "$existing_app_identifier" ]; then
  existing_app_identifier="$(read_env_value "MATCH_APP_IDENTIFIER" "$ENV_FILE" || true)"
fi
if [ -z "$existing_app_identifier" ]; then
  existing_app_identifier="$(extract_default_bundle_id)"
fi
existing_key_id="$(read_env_value "ASC_KEY_ID" "$ENV_FILE" || true)"
existing_issuer_id="$(read_env_value "ASC_ISSUER_ID" "$ENV_FILE" || true)"
existing_private_key_escaped="$(read_env_value "ASC_PRIVATE_KEY_P8" "$ENV_FILE" || true)"
existing_in_house="$(read_env_value "ASC_IN_HOUSE" "$ENV_FILE" || true)"

echo "Fastlane deliver setup (metadata/screenshots upload)"

app_identifier="$(prompt_required "App identifier (bundle id)" "$existing_app_identifier")"
asc_key_id="$(prompt_required "ASC Key ID" "$existing_key_id")"
asc_issuer_id="$(prompt_required "ASC Issuer ID (UUID)" "$existing_issuer_id")"
asc_private_key_escaped="$(read_private_key_p8_escaped "$existing_private_key_escaped")"
if [ -z "$existing_in_house" ]; then
  existing_in_house="false"
fi
default_in_house=false
if [ "$existing_in_house" = "true" ] || [ "$existing_in_house" = "1" ] || [ "$existing_in_house" = "yes" ]; then
  default_in_house=true
fi
if prompt_yes_no "Is this an Enterprise (in-house) Apple account?" "$default_in_house"; then
  asc_in_house="true"
else
  asc_in_house="false"
fi

metadata_path_default="$ROOT_DIR/fastlane/metadata"
screenshots_path_default="$ROOT_DIR/fastlane/screenshots"
metadata_path="$(prompt_required "Metadata path" "$metadata_path_default")"
screenshots_path="$(prompt_required "Screenshots path" "$screenshots_path_default")"
upload_binary=false
if prompt_yes_no "Upload binary (ipa) too?" false; then
  upload_binary=true
fi

upsert_env "APP_IDENTIFIER" "$app_identifier" "$ENV_FILE"
upsert_env "MATCH_APP_IDENTIFIER" "$app_identifier" "$ENV_FILE"
upsert_env "ASC_KEY_ID" "$asc_key_id" "$ENV_FILE"
upsert_env "ASC_ISSUER_ID" "$asc_issuer_id" "$ENV_FILE"
upsert_env "ASC_PRIVATE_KEY_P8" "$asc_private_key_escaped" "$ENV_FILE"
upsert_env "ASC_IN_HOUSE" "$asc_in_house" "$ENV_FILE"

write_api_key_json "$asc_key_id" "$asc_issuer_id" "$asc_private_key_escaped" "$asc_in_house"
ensure_dir "$metadata_path"
ensure_dir "$screenshots_path"

bundle config set --local path 'vendor/bundle'
bundle install

export SPACESHIP_CONNECT_API_IN_HOUSE="$asc_in_house"

deliver_cmd=(
  bundle exec fastlane deliver
  --api_key_path "$API_KEY_JSON_PATH"
  --app_identifier "$app_identifier"
  --platform ios
  --metadata_path "$metadata_path"
  --screenshots_path "$screenshots_path"
  --skip_metadata false
  --skip_screenshots false
  --force true
)

if [ "$upload_binary" = false ]; then
  deliver_cmd+=(--skip_binary_upload true)
fi

echo "Running fastlane deliver..."
"${deliver_cmd[@]}"

cat <<EOF
Fastlane deliver completed.
  App identifier: $app_identifier
  Metadata path: $metadata_path
  Screenshots path: $screenshots_path
  Credentials env: $ENV_FILE
  API key json: $API_KEY_JSON_PATH
EOF
