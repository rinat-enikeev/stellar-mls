#!/usr/bin/env bash
set -euo pipefail
set +x

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IOS_DIR="$ROOT_DIR/ios/StellarChat"
ENV_FILE="$ROOT_DIR/apple.env"
API_KEY_JSON_PATH="$ROOT_DIR/fastlane/.appstoreconnect_api_key.json"
PBXPROJ_FILE="$IOS_DIR/StellarChat.xcodeproj/project.pbxproj"
PROJECT_YML_FILE="$IOS_DIR/project.yml"
ARCHIVE_PATH="$ROOT_DIR/build/ios/archives/StellarChat.xcarchive"
EXPORT_DIR="$ROOT_DIR/build/ios/ipa"
EXPORT_OPTIONS_PLIST="$ROOT_DIR/fastlane/.export_options.plist"
IPA_PATH_DEFAULT="$EXPORT_DIR/StellarChat.ipa"
DRY_RUN=false

usage() {
  cat <<'EOF'
Usage:
  ./release_to_appstore.sh [--dry-run]

Options:
  --dry-run   Print every step and command without mutating remote systems.
              Local files (apple.env, API key json, export options) are not written in dry-run mode.
EOF
}

for arg in "$@"; do
  case "$arg" in
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
  echo "release_to_appstore.sh: $1" >&2
  exit 1
}

step() {
  echo
  echo "==> $1"
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

run_cmd() {
  if $DRY_RUN; then
    echo "[dry-run] $(print_cmd "$@")"
    return 0
  fi
  "$@"
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

run_match_with_auto_renew() {
  local match_type="$1"

  if $DRY_RUN; then
    run_in_dir "$ROOT_DIR" bundle exec fastlane match "$match_type"
    return 0
  fi

  local log_file
  log_file="$(mktemp)"

  set +e
  (
    cd "$ROOT_DIR"
    bundle exec fastlane match "$match_type" 2>&1 | tee "$log_file"
  )
  local status=${PIPESTATUS[0]}
  set -e

  if [ "$status" -eq 0 ]; then
    rm -f "$log_file"
    return 0
  fi

  if grep -q "is not valid, please check end date and renew it if necessary" "$log_file"; then
    echo "Detected invalid/expired certificate for '$match_type'. Retrying with --force to renew."
  else
    echo "fastlane match '$match_type' failed. Retrying once with --force."
  fi

  rm -f "$log_file"
  set +e
  (
    cd "$ROOT_DIR"
    bundle exec fastlane match "$match_type" --force
  )
  status=$?
  set -e

  if [ "$status" -ne 0 ]; then
    echo "fastlane match '$match_type' still failed after --force." >&2
    return "$status"
  fi
  return 0
}

run_deliver_with_retry() {
  if $DRY_RUN; then
    run_in_dir "$ROOT_DIR" "$@"
    return 0
  fi

  local max_attempts="${DELIVER_MAX_ATTEMPTS:-4}"
  local wait_seconds="${DELIVER_RETRY_SECONDS:-30}"
  local attempt=1
  local status=0
  local log_file

  while true; do
    log_file="$(mktemp)"
    set +e
    (
      cd "$ROOT_DIR"
      "$@" 2>&1 | tee "$log_file"
    )
    status=${PIPESTATUS[0]}
    set -e

    if [ "$status" -eq 0 ]; then
      rm -f "$log_file"
      return 0
    fi

    if grep -q "No data" "$log_file"; then
      if [ "$attempt" -lt "$max_attempts" ]; then
        echo "fastlane deliver returned 'No data' (ASC propagation). Retrying in ${wait_seconds}s (${attempt}/${max_attempts})..."
        rm -f "$log_file"
        sleep "$wait_seconds"
        attempt=$((attempt + 1))
        continue
      fi
      echo "fastlane deliver still returns 'No data' after ${max_attempts} attempts." >&2
    fi

    rm -f "$log_file"
    return "$status"
  done
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

read_api_key_json_field() {
  local field="$1"
  if [ ! -f "$API_KEY_JSON_PATH" ]; then
    return 0
  fi
  python3 - "$API_KEY_JSON_PATH" "$field" <<'PY'
import json
import sys

path, field = sys.argv[1:3]
try:
    with open(path, "r", encoding="utf-8") as f:
        data = json.load(f)
except Exception:
    print("")
    raise SystemExit(0)

if field == "key_escaped":
    value = data.get("key", "")
    if isinstance(value, str):
        print(value.replace("\n", "\\n"))
    else:
        print("")
else:
    value = data.get(field, "")
    print(value if isinstance(value, str) else "")
PY
}

upsert_env() {
  local key="$1"
  local value="$2"
  local file="$3"
  python3 - "$file" "$key" "$value" <<'PY'
import os
import re
import sys

path, key, value = sys.argv[1:4]
pattern = re.compile(r"^\s*" + re.escape(key) + r"\s*=")
lines = []
if os.path.exists(path):
    with open(path, "r", encoding="utf-8") as f:
        lines = f.read().splitlines()

out = []
done = False
for line in lines:
    if pattern.match(line):
        if not done:
            out.append(f"{key}={value}")
            done = True
        continue
    out.append(line)

if not done:
    out.append(f"{key}={value}")

with open(path, "w", encoding="utf-8", newline="\n") as f:
    f.write("\n".join(out))
    f.write("\n")
PY
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

prompt_secret() {
  local label="$1"
  local value=""
  while [ -z "$value" ]; do
    read -r -s -p "$label: " value
    echo >&2
    value="$(trim "$value")"
  done
  printf '%s' "$value"
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
    echo "Paste ASC private key (.p8). End with line: END" >&2
    local line
    local lines=()
    while IFS= read -r line; do
      if [ "$line" = "END" ]; then
        break
      fi
      lines+=("$line")
    done
    if [ "${#lines[@]}" -gt 0 ]; then
      key="$(printf '%s\n' "${lines[@]}")"
    else
      key=""
    fi
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

extract_current_build_number() {
  if [ -f "$PROJECT_YML_FILE" ]; then
    awk '/CURRENT_PROJECT_VERSION:/ {print $2; exit}' "$PROJECT_YML_FILE"
    return 0
  fi
  [ -f "$PBXPROJ_FILE" ] || return 0
  awk '
    /CURRENT_PROJECT_VERSION = / {
      line = $0
      sub(/^.*CURRENT_PROJECT_VERSION = /, "", line)
      sub(/;.*/, "", line)
      gsub(/[[:space:]]/, "", line)
      if (line != "") {
        print line
        exit
      }
    }
  ' "$PBXPROJ_FILE"
}

increment_number() {
  local value="$1"
  if [[ "$value" =~ ^[0-9]+$ ]]; then
    printf '%s' "$((value + 1))"
  else
    printf '1'
  fi
}

default_match_git_url() {
  local origin_url owner repo
  origin_url="$(git -C "$ROOT_DIR" remote get-url origin 2>/dev/null || true)"
  owner="$(printf '%s' "$origin_url" | sed -nE 's#^git@github\.com:([^/]+)/([^/]+?)(\.git)?$#\1#p')"
  repo="$(printf '%s' "$origin_url" | sed -nE 's#^git@github\.com:([^/]+)/([^/]+?)(\.git)?$#\2#p')"
  if [ -z "$owner" ] || [ -z "$repo" ]; then
    return 0
  fi
  printf 'git@github.com:%s/%s-match.git' "$owner" "$repo"
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

create_github_repo_if_needed() {
  local git_url="$1"
  if $DRY_RUN; then
    echo "[dry-run] would verify match repo exists: $git_url"
    echo "[dry-run] if missing and approved, would create private repo via gh CLI"
    return 0
  fi

  if git ls-remote "$git_url" >/dev/null 2>&1; then
    return 0
  fi

  local owner_repo=""
  owner_repo="$(parse_github_owner_repo "$git_url")"
  [ -n "$owner_repo" ] || fail "match repo not reachable and URL is not a recognized GitHub SSH address"

  command -v gh >/dev/null 2>&1 || fail "gh CLI is required to create missing repo: $owner_repo"
  if ! prompt_yes_no "Repo '$owner_repo' is not reachable. Create it as private via gh CLI?" true; then
    fail "match repository is required"
  fi

  run_cmd gh repo create "$owner_repo" --private --disable-issues --disable-wiki --confirm
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
  if $DRY_RUN; then
    echo "[dry-run] mkdir -p $(printf '%q' "$path")"
    return 0
  fi
  mkdir -p "$path"
}

has_any_metadata_files() {
  local dir="$1"
  [ -d "$dir" ] || return 1
  find "$dir" -type f ! -name '.DS_Store' | grep -q .
}

has_any_screenshots() {
  local dir="$1"
  [ -d "$dir" ] || return 1
  find "$dir" -type f \( -iname '*.png' -o -iname '*.jpg' -o -iname '*.jpeg' \) | grep -q .
}

update_project_build_number() {
  local build_number="$1"
  if $DRY_RUN; then
    echo "[dry-run] would set CURRENT_PROJECT_VERSION to $build_number in project files"
    return 0
  fi

  if [ -f "$PROJECT_YML_FILE" ]; then
    python3 - "$PROJECT_YML_FILE" "$build_number" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
build = sys.argv[2]
text = path.read_text(encoding="utf-8")
new_text, count = re.subn(
    r'(^\s*CURRENT_PROJECT_VERSION:\s*).*$',
    r'\g<1>' + build,
    text,
    count=1,
    flags=re.MULTILINE,
)
if count != 1:
    raise SystemExit("failed to update CURRENT_PROJECT_VERSION in project.yml")
path.write_text(new_text, encoding="utf-8", newline="\n")
PY
  fi
}

generate_xcode_project_if_needed() {
  if [ ! -f "$PROJECT_YML_FILE" ]; then
    return 0
  fi
  require_cmd xcodegen
  run_in_dir "$IOS_DIR" xcodegen generate
}

write_export_options_plist() {
  local team_id="$1"
  if $DRY_RUN; then
    echo "[dry-run] would write export options plist: $EXPORT_OPTIONS_PLIST"
    return 0
  fi
  mkdir -p "$(dirname "$EXPORT_OPTIONS_PLIST")"
  cat > "$EXPORT_OPTIONS_PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>destination</key>
  <string>export</string>
  <key>manageAppVersionAndBuildNumber</key>
  <false/>
  <key>method</key>
  <string>app-store</string>
  <key>signingStyle</key>
  <string>automatic</string>
  <key>stripSwiftSymbols</key>
  <true/>
  <key>teamID</key>
  <string>$team_id</string>
  <key>uploadSymbols</key>
  <true/>
</dict>
</plist>
EOF
}

locate_ipa() {
  find "$EXPORT_DIR" -maxdepth 1 -name '*.ipa' -print | head -n1
}

require_cmd ruby
require_cmd bundle
require_cmd python3
require_cmd git
require_cmd xcodebuild

[ -d "$IOS_DIR" ] || fail "iOS directory not found: $IOS_DIR"

existing_app_identifier="$(read_env_value "APP_IDENTIFIER" "$ENV_FILE" || true)"
existing_match_app_identifier="$(read_env_value "MATCH_APP_IDENTIFIER" "$ENV_FILE" || true)"
if [ -z "$existing_app_identifier" ]; then
  existing_app_identifier="$(csv_first_value "$existing_match_app_identifier")"
fi
existing_match_git_url="$(read_env_value "MATCH_GIT_URL" "$ENV_FILE" || true)"
existing_match_team_id="$(read_env_value "MATCH_TEAM_ID" "$ENV_FILE" || true)"
existing_match_password="$(read_env_value "MATCH_PASSWORD" "$ENV_FILE" || true)"
existing_key_id="$(read_env_value "ASC_KEY_ID" "$ENV_FILE" || true)"
existing_issuer_id="$(read_env_value "ASC_ISSUER_ID" "$ENV_FILE" || true)"
existing_private_key="$(read_env_value "ASC_PRIVATE_KEY_P8" "$ENV_FILE" || true)"
existing_in_house="$(read_env_value "ASC_IN_HOUSE" "$ENV_FILE" || true)"

if [ -z "$existing_key_id" ]; then
  existing_key_id="$(read_api_key_json_field "key_id" || true)"
fi
if [ -z "$existing_issuer_id" ]; then
  existing_issuer_id="$(read_api_key_json_field "issuer_id" || true)"
fi
if [ -z "$existing_private_key" ]; then
  existing_private_key="$(read_api_key_json_field "key_escaped" || true)"
fi

default_bundle_id="$(extract_default_bundle_id)"
default_match_app_identifier="$(extract_default_match_app_identifiers)"
default_team_id="$(extract_default_team_id)"
default_build_number="$(extract_current_build_number)"
default_build_number="$(increment_number "$default_build_number")"
default_match_repo="$(default_match_git_url)"

if [ -z "$existing_app_identifier" ] && [ -n "$default_bundle_id" ]; then
  existing_app_identifier="$default_bundle_id"
fi
if [ -z "$existing_match_app_identifier" ] && [ -n "$default_match_app_identifier" ]; then
  existing_match_app_identifier="$default_match_app_identifier"
fi
if [ -z "$existing_match_team_id" ] && [ -n "$default_team_id" ]; then
  existing_match_team_id="$default_team_id"
fi
if [ -z "$existing_match_git_url" ] && [ -n "$default_match_repo" ]; then
  existing_match_git_url="$default_match_repo"
fi
if [ -z "$existing_in_house" ]; then
  existing_in_house="false"
fi

step "Collect release inputs"
APP_IDENTIFIER="$(prompt_required "App identifier (bundle id)" "$existing_app_identifier")"
MATCH_APP_IDENTIFIER="${existing_match_app_identifier:-$APP_IDENTIFIER}"
MATCH_GIT_URL="$(prompt_required "Match GitHub SSH repo URL" "$existing_match_git_url")"
MATCH_TEAM_ID="$(prompt_required "Apple Team ID" "$existing_match_team_id")"
BUILD_NUMBER="$(prompt_required "Build number" "$default_build_number")"

if [ -n "$existing_match_password" ] && prompt_yes_no "Reuse existing MATCH_PASSWORD from apple.env?" true; then
  MATCH_PASSWORD="$existing_match_password"
else
  MATCH_PASSWORD="$(prompt_secret "MATCH_PASSWORD (encryption password for cert repo)")"
fi

ASC_KEY_ID="$(prompt_required "ASC Key ID" "$existing_key_id")"
ASC_ISSUER_ID="$(prompt_required "ASC Issuer ID (UUID)" "$existing_issuer_id")"
ASC_PRIVATE_KEY_P8="$(read_private_key_p8_escaped "$existing_private_key")"
if [ "$existing_in_house" = "true" ] || [ "$existing_in_house" = "1" ] || [ "$existing_in_house" = "yes" ]; then
  default_in_house=true
else
  default_in_house=false
fi
if prompt_yes_no "Is this an Enterprise (in-house) Apple account?" "$default_in_house"; then
  ASC_IN_HOUSE="true"
else
  ASC_IN_HOUSE="false"
fi

METADATA_PATH="$(prompt_required "Metadata path" "$ROOT_DIR/fastlane/metadata")"
SCREENSHOTS_PATH="$(prompt_required "Screenshots path" "$ROOT_DIR/fastlane/screenshots")"

step "Persist local config"
if ! $DRY_RUN; then
  upsert_env "APP_IDENTIFIER" "$APP_IDENTIFIER" "$ENV_FILE"
  upsert_env "MATCH_APP_IDENTIFIER" "$MATCH_APP_IDENTIFIER" "$ENV_FILE"
  upsert_env "MATCH_GIT_URL" "$MATCH_GIT_URL" "$ENV_FILE"
  upsert_env "MATCH_TEAM_ID" "$MATCH_TEAM_ID" "$ENV_FILE"
  upsert_env "MATCH_PASSWORD" "$MATCH_PASSWORD" "$ENV_FILE"
  upsert_env "ASC_KEY_ID" "$ASC_KEY_ID" "$ENV_FILE"
  upsert_env "ASC_ISSUER_ID" "$ASC_ISSUER_ID" "$ENV_FILE"
  upsert_env "ASC_PRIVATE_KEY_P8" "$ASC_PRIVATE_KEY_P8" "$ENV_FILE"
  upsert_env "ASC_IN_HOUSE" "$ASC_IN_HOUSE" "$ENV_FILE"
fi
write_api_key_json "$ASC_KEY_ID" "$ASC_ISSUER_ID" "$ASC_PRIVATE_KEY_P8" "$ASC_IN_HOUSE"
ensure_dir "$METADATA_PATH"
ensure_dir "$SCREENSHOTS_PATH"

step "Prepare signing repository"
create_github_repo_if_needed "$MATCH_GIT_URL"

step "Install fastlane dependencies"
run_in_dir "$ROOT_DIR" bundle config set --local path 'vendor/bundle'
run_in_dir "$ROOT_DIR" bundle install

export MATCH_GIT_URL
export MATCH_APP_IDENTIFIER
export MATCH_TEAM_ID
export MATCH_PASSWORD
export APP_STORE_CONNECT_API_KEY_PATH="$API_KEY_JSON_PATH"
export SPACESHIP_CONNECT_API_IN_HOUSE="$ASC_IN_HOUSE"

step "Sync certificates and provisioning profiles via match"
run_match_with_auto_renew development
run_match_with_auto_renew adhoc
run_match_with_auto_renew appstore

step "Update build number"
update_project_build_number "$BUILD_NUMBER"

step "Regenerate Xcode project"
generate_xcode_project_if_needed

step "Build iOS archive"
ensure_dir "$(dirname "$ARCHIVE_PATH")"
ensure_dir "$EXPORT_DIR"
run_in_dir "$IOS_DIR" xcodebuild \
  -project StellarChat.xcodeproj \
  -scheme StellarChat \
  -configuration Release \
  -destination generic/platform=iOS \
  -archivePath "$ARCHIVE_PATH" \
  DEVELOPMENT_TEAM="$MATCH_TEAM_ID" \
  clean archive

step "Export IPA"
write_export_options_plist "$MATCH_TEAM_ID"
run_in_dir "$IOS_DIR" xcodebuild \
  -exportArchive \
  -archivePath "$ARCHIVE_PATH" \
  -exportPath "$EXPORT_DIR" \
  -exportOptionsPlist "$EXPORT_OPTIONS_PLIST"

IPA_PATH="$IPA_PATH_DEFAULT"
if ! $DRY_RUN; then
  IPA_PATH="$(locate_ipa)"
fi
[ -n "$IPA_PATH" ] || fail "no ipa found under $EXPORT_DIR"

step "Upload build and optional metadata via fastlane deliver"
deliver_cmd=(
  bundle exec fastlane deliver
  --api_key_path "$API_KEY_JSON_PATH"
  --app_identifier "$APP_IDENTIFIER"
  --platform ios
  --ipa "$IPA_PATH"
  --force true
  --run_precheck_before_submit false
  --precheck_include_in_app_purchases false
)
if has_any_metadata_files "$METADATA_PATH"; then
  deliver_cmd+=(--metadata_path "$METADATA_PATH" --skip_metadata false)
else
  deliver_cmd+=(--skip_metadata true)
fi
if has_any_screenshots "$SCREENSHOTS_PATH"; then
  deliver_cmd+=(--screenshots_path "$SCREENSHOTS_PATH" --skip_screenshots false)
else
  deliver_cmd+=(--skip_screenshots true)
fi
run_deliver_with_retry "${deliver_cmd[@]}"

cat <<EOF

Release pipeline completed.
  Dry run: $DRY_RUN
  App identifier: $APP_IDENTIFIER
  Build number: $BUILD_NUMBER
  Match repo: $MATCH_GIT_URL
  IPA path: $IPA_PATH
  Metadata path: $METADATA_PATH
  Screenshots path: $SCREENSHOTS_PATH
  Env file: $ENV_FILE
  API key json: $API_KEY_JSON_PATH
EOF
