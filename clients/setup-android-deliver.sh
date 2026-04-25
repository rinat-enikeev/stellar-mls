#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$ROOT_DIR/google.env"
PLAY_JSON_PATH="$ROOT_DIR/android/StellarChat/google-play-secret.json"

fail() {
  echo "setup-android-deliver.sh: $1" >&2
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
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", $0)
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
        if (!done) print key "=" value
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
  default_value="$(trim "$default_value")"
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
    else
      read -r -p "$label [y/N]: " input
    fi
    normalized="$(printf '%s' "$input" | tr '[:upper:]' '[:lower:]' | xargs)"
    if [ -z "$normalized" ]; then
      if [ "$default_yes" = "true" ]; then return 0; else return 1; fi
    fi
    case "$normalized" in
      y|yes) return 0 ;;
      n|no) return 1 ;;
    esac
    echo "Please answer y or n."
  done
}

normalize_service_account_json() {
  local input="$1"
  python3 - "$input" <<'PY'
import base64
import json
import sys

raw = sys.argv[1]

def parse_json(text: str):
    obj = json.loads(text)
    if not isinstance(obj, dict):
        raise ValueError("json root must be object")
    required = ["type", "client_email", "private_key"]
    for key in required:
        if key not in obj or not obj[key]:
            raise ValueError(f"missing {key}")
    if obj["type"] != "service_account":
        raise ValueError("type must be service_account")
    return obj

obj = None
try:
    obj = parse_json(raw)
except Exception:
    try:
        decoded = base64.b64decode(raw).decode("utf-8")
        obj = parse_json(decoded)
    except Exception:
        raise SystemExit(1)

print(json.dumps(obj, separators=(",", ":"), ensure_ascii=False))
PY
}

read_service_account_json() {
  local path=""
  local json=""
  local normalized_json=""

  if [ -f "$PLAY_JSON_PATH" ] && \
    prompt_yes_no "Reuse existing service account JSON at $PLAY_JSON_PATH?" true; then
    return
  fi

  read -r -p "Path to Google Play service account JSON (leave empty to paste): " path
  path="$(trim "$path")"
  path="${path/#\~/$HOME}"

  if [ -n "$path" ]; then
    [ -f "$path" ] || fail "file not found: $path"
    json="$(cat "$path")"
  else
    echo "Paste JSON. End with a line containing only: END"
    local line
    local lines=()
    while IFS= read -r line; do
      if [ "$line" = "END" ]; then
        break
      fi
      lines+=("$line")
    done
    json="$(printf '%s\n' "${lines[@]}")"
  fi

  normalized_json="$(normalize_service_account_json "$json" || true)"
  [ -n "$normalized_json" ] || fail "invalid service account JSON"

  mkdir -p "$(dirname "$PLAY_JSON_PATH")"
  printf '%s' "$normalized_json" > "$PLAY_JSON_PATH"
  chmod 600 "$PLAY_JSON_PATH"
}

require_cmd ruby
require_cmd bundle
require_cmd python3

existing_package_name="$(read_env_value "PLAY_PACKAGE_NAME" "$ENV_FILE" || true)"
existing_track="$(read_env_value "PLAY_TRACK" "$ENV_FILE" || true)"
existing_default_language="$(read_env_value "PLAY_DEFAULT_LANGUAGE" "$ENV_FILE" || true)"

if [ -z "$existing_package_name" ]; then
  existing_package_name="chat.onym.android"
fi

echo "Google Play deliver setup (metadata upload)"
echo

PLAY_PACKAGE_NAME="$(prompt_required "Android package name" "$existing_package_name")"
PLAY_TRACK="$(prompt_required "Release track (internal|alpha|beta|production)" "${existing_track:-production}")"
PLAY_DEFAULT_LANGUAGE="$(prompt_required "Default language" "${existing_default_language:-en-US}")"

read_service_account_json

upsert_env "PLAY_PACKAGE_NAME" "$PLAY_PACKAGE_NAME" "$ENV_FILE"
upsert_env "PLAY_TRACK" "$PLAY_TRACK" "$ENV_FILE"
upsert_env "PLAY_DEFAULT_LANGUAGE" "$PLAY_DEFAULT_LANGUAGE" "$ENV_FILE"

cd "$ROOT_DIR"
bundle config set --local path 'vendor/bundle'
bundle install

cat <<EOF

Setup complete.
  Package name:       $PLAY_PACKAGE_NAME
  Track:              $PLAY_TRACK
  Default language:   $PLAY_DEFAULT_LANGUAGE
  Env file:           $ENV_FILE
  Service account:    $PLAY_JSON_PATH
  Metadata path:      $ROOT_DIR/fastlane/metadata/android

Run ./deliver-android.sh to upload metadata to Google Play.
EOF
