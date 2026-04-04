#!/usr/bin/env bash
set -euo pipefail
set +x

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$ROOT_DIR/apple.env"
API_KEY_JSON_PATH="$ROOT_DIR/fastlane/.appstoreconnect_api_key.json"

fail() {
  echo "recreate_provisioning_profiles.sh: $1" >&2
  exit 1
}

require_cmd() {
  local cmd="$1"
  command -v "$cmd" >/dev/null 2>&1 || fail "required command not found: $cmd"
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

run_match() {
  local type="$1"
  echo "Running: bundle exec fastlane match $type --force"
  bundle exec fastlane match "$type" --force
}

require_cmd ruby
require_cmd bundle
require_cmd python3

[ -f "$ENV_FILE" ] || fail "missing $ENV_FILE"

MATCH_GIT_URL="$(read_env_value "MATCH_GIT_URL" "$ENV_FILE" || true)"
MATCH_APP_IDENTIFIER="$(read_env_value "MATCH_APP_IDENTIFIER" "$ENV_FILE" || true)"
MATCH_TEAM_ID="$(read_env_value "MATCH_TEAM_ID" "$ENV_FILE" || true)"
MATCH_PASSWORD="$(read_env_value "MATCH_PASSWORD" "$ENV_FILE" || true)"
ASC_KEY_ID="$(read_env_value "ASC_KEY_ID" "$ENV_FILE" || true)"
ASC_ISSUER_ID="$(read_env_value "ASC_ISSUER_ID" "$ENV_FILE" || true)"
ASC_PRIVATE_KEY_P8="$(read_env_value "ASC_PRIVATE_KEY_P8" "$ENV_FILE" || true)"
ASC_IN_HOUSE="$(read_env_value "ASC_IN_HOUSE" "$ENV_FILE" || true)"

[ -n "$MATCH_GIT_URL" ] || fail "MATCH_GIT_URL missing in $ENV_FILE"
[ -n "$MATCH_APP_IDENTIFIER" ] || fail "MATCH_APP_IDENTIFIER missing in $ENV_FILE"
[ -n "$MATCH_TEAM_ID" ] || fail "MATCH_TEAM_ID missing in $ENV_FILE"
[ -n "$MATCH_PASSWORD" ] || fail "MATCH_PASSWORD missing in $ENV_FILE"
[ -n "$ASC_KEY_ID" ] || fail "ASC_KEY_ID missing in $ENV_FILE"
[ -n "$ASC_ISSUER_ID" ] || fail "ASC_ISSUER_ID missing in $ENV_FILE"
[ -n "$ASC_PRIVATE_KEY_P8" ] || fail "ASC_PRIVATE_KEY_P8 missing in $ENV_FILE"

if [ -z "$ASC_IN_HOUSE" ]; then
  ASC_IN_HOUSE="false"
fi

write_api_key_json "$ASC_KEY_ID" "$ASC_ISSUER_ID" "$ASC_PRIVATE_KEY_P8" "$ASC_IN_HOUSE"

cd "$ROOT_DIR"
bundle config set --local path 'vendor/bundle'
bundle install

export MATCH_GIT_URL
export MATCH_APP_IDENTIFIER
export MATCH_TEAM_ID
export MATCH_PASSWORD
export APP_STORE_CONNECT_API_KEY_PATH="$API_KEY_JSON_PATH"
export SPACESHIP_CONNECT_API_IN_HOUSE="$ASC_IN_HOUSE"

run_match development
run_match adhoc
run_match appstore

cat <<EOF
Provisioning profiles recreated.
  Env file: $ENV_FILE
  API key json: $API_KEY_JSON_PATH
  Match repo: $MATCH_GIT_URL
EOF
