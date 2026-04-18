#!/bin/bash
# One-shot n8n deploy: pushes every credential under n8n/secrets/*.json and
# every workflow under n8n/workflows/*.json into the running stellar-n8n
# container, rewriting REPLACE_ME credential IDs in workflow JSONs to match
# whatever the n8n DB ended up with. Idempotent via name-based upsert.
#
# Requires:
#   - docker compose stack already up (./bin/up.sh)
#   - build/dev-env/n8n/secrets/ populated from secrets.example
#   - jq on the host
set -euo pipefail

DEV_ENV_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$DEV_ENV_DIR"

N8N_CONTAINER="stellar-n8n"
SECRETS_DIR="$DEV_ENV_DIR/n8n/secrets"
SECRETS_EXAMPLE_DIR="$DEV_ENV_DIR/n8n/secrets.example"
WORKFLOWS_DIR="$DEV_ENV_DIR/n8n/workflows"
SCRATCH=""

cleanup() {
    [ -n "$SCRATCH" ] && [ -d "$SCRATCH" ] && rm -rf "$SCRATCH"
    docker exec "$N8N_CONTAINER" rm -rf /tmp/n8n-deploy 2>/dev/null || true
}
trap cleanup EXIT

red()   { printf "\033[31m%s\033[0m" "$1"; }
green() { printf "\033[32m%s\033[0m" "$1"; }

die() {
    echo "$(red ERROR): $*" >&2
    exit 1
}

# ---------- Phase 0: preflight ---------------------------------------------
echo "==> preflight"

command -v jq >/dev/null 2>&1 || die "jq is required on the host (brew install jq)"
command -v docker >/dev/null 2>&1 || die "docker is required"

if ! docker compose -f docker-compose.dev.yml ps "$N8N_CONTAINER" \
        --status running --quiet | grep -q .; then
    die "$N8N_CONTAINER is not running — run ./bin/up.sh first"
fi

if [ ! -d "$SECRETS_DIR" ]; then
    cat >&2 <<EOF
$(red ERROR): $SECRETS_DIR does not exist.

Copy the templates and fill in real values:
  cp -R $SECRETS_EXAMPLE_DIR $SECRETS_DIR
  # then edit each file under $SECRETS_DIR
EOF
    exit 1
fi

REQUIRED_FILES=(
    "ssh-qa-agent.json"
    "ssh-release-agent.json"
    "github-qa-bot.json"
    "github-release-bot.json"
)
MISSING=()
for f in "${REQUIRED_FILES[@]}"; do
    [ -f "$SECRETS_DIR/$f" ] || MISSING+=("$SECRETS_DIR/$f")
done
if [ "${#MISSING[@]}" -gt 0 ]; then
    {
        echo "$(red ERROR): required secret file(s) missing:"
        for m in "${MISSING[@]}"; do echo "  - $m"; done
        echo
        echo "See $SECRETS_EXAMPLE_DIR/README.md for what each file should contain."
    } >&2
    exit 1
fi

SHOPT_SET_NULLGLOB=0
shopt -q nullglob && SHOPT_SET_NULLGLOB=1 || shopt -s nullglob
WF_FILES=( "$WORKFLOWS_DIR"/*.json )
[ "$SHOPT_SET_NULLGLOB" -eq 1 ] || shopt -u nullglob
[ "${#WF_FILES[@]}" -gt 0 ] || die "no workflow JSONs under $WORKFLOWS_DIR"

WEBHOOK_URL=""
if [ -f "$DEV_ENV_DIR/n8n.env" ]; then
    WEBHOOK_URL="$(awk -F= '/^WEBHOOK_URL=/{sub(/^WEBHOOK_URL=/,""); print; exit}' \
                     "$DEV_ENV_DIR/n8n.env" | tr -d '[:space:]')"
fi

SCRATCH="$(mktemp -d -t n8n-deploy.XXXXXXXX)"
docker exec "$N8N_CONTAINER" mkdir -p /tmp/n8n-deploy

echo "  $(green OK)  stellar-n8n running; secrets present; ${#WF_FILES[@]} workflow(s) found"

# ---------- Phase 1: credentials -------------------------------------------
echo
echo "==> credentials"

jq -s '.' "$SECRETS_DIR"/*.json > "$SCRATCH/to-import.json"

# Export whatever credentials already exist so we can upsert by id.
if docker exec "$N8N_CONTAINER" n8n export:credentials --all \
        --output=/tmp/n8n-deploy/existing-creds.json >/dev/null 2>&1; then
    docker cp "$N8N_CONTAINER:/tmp/n8n-deploy/existing-creds.json" \
              "$SCRATCH/existing-creds.json"
else
    echo '[]' > "$SCRATCH/existing-creds.json"
fi

# Join on name → inject existing id into the to-import record so import
# UPDATEs instead of INSERTing a duplicate.
jq -s '
  . as $all
  | $all[1] | map(
      . as $c
      | ($c.name) as $n
      | ($all[0] | map(select(.name == $n)) | .[0] // null) as $match
      | if $match == null then $c
        else $c + {id: $match.id}
        end
    )
' "$SCRATCH/existing-creds.json" "$SCRATCH/to-import.json" \
    > "$SCRATCH/creds-with-ids.json"

docker cp "$SCRATCH/creds-with-ids.json" \
          "$N8N_CONTAINER:/tmp/n8n-deploy/creds.json"
docker exec "$N8N_CONTAINER" n8n import:credentials \
    --input=/tmp/n8n-deploy/creds.json

# Re-export so we have authoritative (name, id) pairs for Phase 2.
docker exec "$N8N_CONTAINER" n8n export:credentials --all \
    --output=/tmp/n8n-deploy/creds-after.json >/dev/null
docker cp "$N8N_CONTAINER:/tmp/n8n-deploy/creds-after.json" \
          "$SCRATCH/creds-after.json"

# Build two lookup files jq can consume: {"name": "id", ...}
jq 'map({key: .name, value: .id}) | from_entries' \
    "$SCRATCH/creds-after.json" > "$SCRATCH/cred-id-by-name.json"

CRED_COUNT=$(jq 'length' "$SCRATCH/to-import.json")
echo "  $(green OK)  imported/updated $CRED_COUNT credential(s)"

# ---------- Phase 2: workflows ---------------------------------------------
echo
echo "==> workflows"

# Export existing workflows so we can upsert by name.
docker exec "$N8N_CONTAINER" n8n export:workflow --all \
    --output=/tmp/n8n-deploy/existing-wfs.json >/dev/null 2>&1 \
    || echo '[]' | docker exec -i "$N8N_CONTAINER" \
        sh -c 'cat > /tmp/n8n-deploy/existing-wfs.json'
docker cp "$N8N_CONTAINER:/tmp/n8n-deploy/existing-wfs.json" \
          "$SCRATCH/existing-wfs.json"
jq 'map({key: .name, value: .id}) | from_entries' \
    "$SCRATCH/existing-wfs.json" > "$SCRATCH/wf-id-by-name.json"

mkdir -p "$SCRATCH/workflows"
for wf in "${WF_FILES[@]}"; do
    BN="$(basename "$wf")"
    OUT="$SCRATCH/workflows/$BN"

    # 1. Rewrite credentials[].id via cred-id-by-name map. Fail loudly if
    #    any referenced name is missing from the map.
    jq --slurpfile cmap "$SCRATCH/cred-id-by-name.json" '
      def cmap: $cmap[0];
      .nodes |= map(
        if has("credentials") then
          .credentials |= with_entries(
            .value as $c
            | (cmap[$c.name]) as $id
            | if $id == null
              then error("unknown credential name: " + $c.name)
              else .value = {id: $id, name: $c.name}
              end
          )
        else . end
      )
    ' "$wf" > "$OUT" 2> "$SCRATCH/err.txt" || {
        {
            echo "$(red ERROR): rewriting $BN failed:"
            cat "$SCRATCH/err.txt"
            echo
            echo "Credential map contains:"
            jq -r 'keys[]' "$SCRATCH/cred-id-by-name.json" | sed 's/^/  - /'
        } >&2
        exit 1
    }

    # 2. If a workflow with the same name already exists, inject its id so
    #    import:workflow UPDATEs instead of INSERTing.
    WF_NAME="$(jq -r '.name' "$OUT")"
    EXISTING_ID="$(jq -r --arg n "$WF_NAME" '.[$n] // empty' \
                     "$SCRATCH/wf-id-by-name.json")"
    if [ -n "$EXISTING_ID" ]; then
        jq --arg id "$EXISTING_ID" '. + {id: $id}' "$OUT" > "$OUT.tmp" \
            && mv "$OUT.tmp" "$OUT"
    fi
done

docker cp "$SCRATCH/workflows/." \
          "$N8N_CONTAINER:/tmp/n8n-deploy/workflows/"
docker exec "$N8N_CONTAINER" n8n import:workflow \
    --separate --input=/tmp/n8n-deploy/workflows

echo "  $(green OK)  imported/updated ${#WF_FILES[@]} workflow(s)"

# ---------- Phase 3: activate ----------------------------------------------
echo
echo "==> activate"

docker exec "$N8N_CONTAINER" n8n export:workflow --all \
    --output=/tmp/n8n-deploy/wfs-after.json >/dev/null
docker cp "$N8N_CONTAINER:/tmp/n8n-deploy/wfs-after.json" \
          "$SCRATCH/wfs-after.json"

for wf in "${WF_FILES[@]}"; do
    NAME="$(jq -r '.name' "$wf")"
    ID="$(jq -r --arg n "$NAME" \
            'map(select(.name == $n)) | .[0].id // empty' \
            "$SCRATCH/wfs-after.json")"
    if [ -z "$ID" ]; then
        echo "  $(red SKIP)  $NAME — not found in DB after import" >&2
        continue
    fi
    docker exec "$N8N_CONTAINER" n8n update:workflow \
        --id="$ID" --active=true >/dev/null
    echo "  $(green OK)  $NAME (id=$ID) active"
done

# ---------- Phase 4: summary -----------------------------------------------
echo
echo "==> summary"

BASE="${WEBHOOK_URL:-http://127.0.0.1:5678/}"
BASE="${BASE%/}"
echo
printf "  %-32s %s\n" "WORKFLOW" "WEBHOOK URL"
printf "  %-32s %s\n" "--------" "-----------"
for wf in "${WF_FILES[@]}"; do
    NAME="$(jq -r '.name' "$wf")"
    # Extract the webhook path from the first webhook node.
    PATHS="$(jq -r '[.nodes[] | select(.type == "n8n-nodes-base.webhook") | .parameters.path] | unique | .[]' "$wf")"
    if [ -z "$PATHS" ]; then
        printf "  %-32s %s\n" "$NAME" "(no webhook trigger)"
    else
        FIRST=1
        while IFS= read -r P; do
            if [ "$FIRST" -eq 1 ]; then
                printf "  %-32s %s/webhook/%s\n" "$NAME" "$BASE" "$P"
                FIRST=0
            else
                printf "  %-32s %s/webhook/%s\n" "" "$BASE" "$P"
            fi
        done <<< "$PATHS"
    fi
done

echo
echo "  CREDENTIALS"
echo "  -----------"
jq -r '.[] | "  \(.name)  (id=\(.id))"' "$SCRATCH/creds-after.json"

cat <<EOF

Next: point the GitHub App webhook URL at one of the paths above (or fan
out via your reverse proxy / cloudflared routing) and subscribe to:
  issues, pull_request, pull_request_review,
  pull_request_review_comment, issue_comment

Re-run ./bin/n8n-deploy.sh any time — it upserts by name.
EOF
