#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$ROOT_DIR/apple.env"
API_KEY_JSON_PATH="$ROOT_DIR/fastlane/.appstoreconnect_api_key.json"
PBXPROJ_FILE="$ROOT_DIR/ios/StellarChat/StellarChat.xcodeproj/project.pbxproj"
PROJECT_YML_FILE="$ROOT_DIR/ios/StellarChat/project.yml"

fail() {
  echo "setup_fastlane_match.sh: $1" >&2
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

csv_first_value() {
  local value="$1"
  value="${value%%,*}"
  trim "$value"
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

prompt_secret() {
  local label="$1"
  local value=""
  while [ -z "$value" ]; do
    read -r -s -p "$label: " value
    echo
    value="$(trim "$value")"
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
    echo "Paste ASC private key (.p8). Finish with line: END"
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

extract_default_match_app_identifiers() {
  if [ -f "$PROJECT_YML_FILE" ]; then
    awk '
      /PRODUCT_BUNDLE_IDENTIFIER:/ {
        value = $2
        gsub(/"/, "", value)
        gsub(/[[:space:]]/, "", value)
        if (value == "" || value ~ /\.tests$/ || seen[value]++) next
        values[++count] = value
      }
      END {
        for (i = 1; i <= count; i++) {
          if (i > 1) {
            printf ","
          }
          printf "%s", values[i]
        }
      }
    ' "$PROJECT_YML_FILE"
    return 0
  fi
  [ -f "$PBXPROJ_FILE" ] || return 0
  awk '
    /PRODUCT_BUNDLE_IDENTIFIER = / {
      line = $0
      sub(/^.*PRODUCT_BUNDLE_IDENTIFIER = /, "", line)
      sub(/;.*/, "", line)
      gsub(/[[:space:]]/, "", line)
      if (line == "" || line ~ /\.tests$/ || seen[line]++) next
      values[++count] = line
    }
    END {
      for (i = 1; i <= count; i++) {
        if (i > 1) {
          printf ","
        }
        printf "%s", values[i]
      }
    }
  ' "$PBXPROJ_FILE"
}

extract_default_team_id() {
  if [ -f "$PROJECT_YML_FILE" ]; then
    awk '
      /DEVELOPMENT_TEAM:/ {
        value = $2
        gsub(/"/, "", value)
        if (value != "") {
          print value
          exit
        }
      }
    ' "$PROJECT_YML_FILE"
  fi
  [ -f "$PBXPROJ_FILE" ] || return 0
  awk '
    /DEVELOPMENT_TEAM = / {
      line = $0
      sub(/^.*DEVELOPMENT_TEAM = /, "", line)
      sub(/;.*/, "", line)
      gsub(/[[:space:]]/, "", line)
      if (line != "" && line != "\"\"") {
        gsub(/"/, "", line)
        print line
        exit
      }
    }
  ' "$PBXPROJ_FILE"
}

parse_github_owner_repo() {
  local url="$1"
  python3 - "$url" <<'PY'
import re
import sys
u = sys.argv[1].strip()
patterns = [
    r'^git@github\.com:([^/]+)/([^/]+?)(?:\.git)?$',
    r'^ssh://git@github\.com/([^/]+)/([^/]+?)(?:\.git)?$',
]
for p in patterns:
    m = re.match(p, u)
    if m:
        print(f"{m.group(1)}/{m.group(2)}")
        sys.exit(0)
print("")
PY
}

default_match_git_url() {
  local origin_url owner_repo repo_name
  origin_url="$(git -C "$ROOT_DIR" remote get-url origin 2>/dev/null || true)"
  owner_repo="$(parse_github_owner_repo "$origin_url")"
  if [ -z "$owner_repo" ]; then
    return 0
  fi
  repo_name="${owner_repo#*/}"
  printf 'git@github.com:%s/%s-match.git' "${owner_repo%%/*}" "$repo_name"
}

create_github_repo_if_needed() {
  local git_url="$1"
  if git ls-remote "$git_url" >/dev/null 2>&1; then
    return 0
  fi

  local owner_repo=""
  owner_repo="$(parse_github_owner_repo "$git_url")"
  if [ -z "$owner_repo" ]; then
    fail "cannot access repo '$git_url' and URL is not a recognized GitHub SSH format"
  fi

  if ! command -v gh >/dev/null 2>&1; then
    fail "cannot access repo '$git_url' and gh CLI is not installed to create it"
  fi

  if ! prompt_yes_no "Repo '$owner_repo' is not reachable. Create it as private via gh CLI?" true; then
    fail "match repo is required"
  fi

  gh repo create "$owner_repo" --private --disable-issues --disable-wiki --confirm
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
  echo "Running: bundle exec fastlane match $type"
  bundle exec fastlane match "$type"
}

require_cmd ruby
require_cmd bundle
require_cmd git
require_cmd python3

cd "$ROOT_DIR"

existing_repo="$(read_env_value "MATCH_GIT_URL" "$ENV_FILE" || true)"
existing_match_app_identifier="$(read_env_value "MATCH_APP_IDENTIFIER" "$ENV_FILE" || true)"
existing_bundle_id="$(read_env_value "APP_IDENTIFIER" "$ENV_FILE" || true)"
if [ -z "$existing_bundle_id" ]; then
  existing_bundle_id="$(csv_first_value "$existing_match_app_identifier")"
fi
existing_team_id="$(read_env_value "MATCH_TEAM_ID" "$ENV_FILE" || true)"
existing_match_password="$(read_env_value "MATCH_PASSWORD" "$ENV_FILE" || true)"
existing_key_id="$(read_env_value "ASC_KEY_ID" "$ENV_FILE" || true)"
existing_issuer_id="$(read_env_value "ASC_ISSUER_ID" "$ENV_FILE" || true)"
existing_key_escaped="$(read_env_value "ASC_PRIVATE_KEY_P8" "$ENV_FILE" || true)"
existing_in_house="$(read_env_value "ASC_IN_HOUSE" "$ENV_FILE" || true)"

default_bundle_id="$(extract_default_bundle_id)"
default_match_app_identifier="$(extract_default_match_app_identifiers)"
default_team_id="$(extract_default_team_id)"
default_match_repo="$(default_match_git_url)"

if [ -z "$existing_bundle_id" ] && [ -n "$default_bundle_id" ]; then
  existing_bundle_id="$default_bundle_id"
fi
if [ -z "$existing_match_app_identifier" ] && [ -n "$default_match_app_identifier" ]; then
  existing_match_app_identifier="$default_match_app_identifier"
fi
if [ -z "$existing_team_id" ] && [ -n "$default_team_id" ]; then
  existing_team_id="$default_team_id"
fi
if [ -z "$existing_repo" ] && [ -n "$default_match_repo" ]; then
  existing_repo="$default_match_repo"
fi

echo "Fastlane Match setup (files live at repo root)"

match_git_url="$(prompt_required "GitHub SSH repo URL for match storage" "$existing_repo")"
app_identifier="$(prompt_required "Main app identifier (bundle id)" "$existing_bundle_id")"
match_app_identifier="$(prompt_required "Match app identifiers (comma-separated bundle ids)" "${existing_match_app_identifier:-$app_identifier}")"
match_team_id="$(prompt_required "Apple Team ID" "$existing_team_id")"

if [ -n "$existing_match_password" ]; then
  if prompt_yes_no "Reuse existing MATCH_PASSWORD from apple.env?" true; then
    match_password="$existing_match_password"
  else
    match_password="$(prompt_secret "MATCH_PASSWORD (encryption password for cert repo)")"
  fi
else
  match_password="$(prompt_secret "MATCH_PASSWORD (encryption password for cert repo)")"
fi

asc_key_id="$(prompt_required "ASC Key ID" "$existing_key_id")"
asc_issuer_id="$(prompt_required "ASC Issuer ID (UUID)" "$existing_issuer_id")"
asc_private_key_escaped="$(read_private_key_p8_escaped "$existing_key_escaped")"
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

create_github_repo_if_needed "$match_git_url"

upsert_env "MATCH_GIT_URL" "$match_git_url" "$ENV_FILE"
upsert_env "MATCH_APP_IDENTIFIER" "$match_app_identifier" "$ENV_FILE"
upsert_env "APP_IDENTIFIER" "$app_identifier" "$ENV_FILE"
upsert_env "MATCH_TEAM_ID" "$match_team_id" "$ENV_FILE"
upsert_env "MATCH_PASSWORD" "$match_password" "$ENV_FILE"
upsert_env "ASC_KEY_ID" "$asc_key_id" "$ENV_FILE"
upsert_env "ASC_ISSUER_ID" "$asc_issuer_id" "$ENV_FILE"
upsert_env "ASC_PRIVATE_KEY_P8" "$asc_private_key_escaped" "$ENV_FILE"
upsert_env "ASC_IN_HOUSE" "$asc_in_house" "$ENV_FILE"

write_api_key_json "$asc_key_id" "$asc_issuer_id" "$asc_private_key_escaped" "$asc_in_house"

export MATCH_GIT_URL="$match_git_url"
export MATCH_APP_IDENTIFIER="$match_app_identifier"
export APP_IDENTIFIER="$app_identifier"
export MATCH_TEAM_ID="$match_team_id"
export MATCH_PASSWORD="$match_password"
export APP_STORE_CONNECT_API_KEY_PATH="$API_KEY_JSON_PATH"
export SPACESHIP_CONNECT_API_IN_HOUSE="$asc_in_house"

bundle config set --local path 'vendor/bundle'
bundle install

run_match development
run_match adhoc
run_match appstore

cat <<EOF
Fastlane Match setup completed.
  Env file: $ENV_FILE
  API key json: $API_KEY_JSON_PATH
  Match repo: $MATCH_GIT_URL
EOF
