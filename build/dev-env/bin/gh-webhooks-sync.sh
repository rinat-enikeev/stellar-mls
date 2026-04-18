#!/bin/bash
# Create or update GitHub repo webhooks so each workflow in
# build/dev-env/n8n/workflows/ receives the events it filters on.
# Idempotent: matches by webhook URL. Re-run after any path change or
# to flip events on/off.
#
# Runs on your dev machine (the one `gh auth login` is authenticated on),
# not on the Mac mini. Hits the GitHub REST API directly.
#
# Config:
#   Defaults are read from build/dev-env/gh-webhooks.env (gitignored).
#   Copy gh-webhooks.env.example → gh-webhooks.env and fill it in.
#   Positional args override the env file.
#
# Usage:
#   ./gh-webhooks-sync.sh                                   # uses env
#   ./gh-webhooks-sync.sh <owner/repo> <webhook-base-url>   # overrides
#
# Requirements:
#   - gh CLI authed with 'admin:repo_hook' scope. If missing, run:
#       gh auth refresh -h github.com -s admin:repo_hook
set -euo pipefail

DEV_ENV_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
ENV_FILE="$DEV_ENV_DIR/gh-webhooks.env"
if [ -f "$ENV_FILE" ]; then
    # shellcheck disable=SC1090
    set -a; . "$ENV_FILE"; set +a
fi

REPO="${1:-${GH_REPO:-}}"
BASE="${2:-${WEBHOOK_BASE_URL:-}}"
if [ -z "$REPO" ] || [ -z "$BASE" ]; then
    cat >&2 <<EOF
ERROR: GH_REPO and WEBHOOK_BASE_URL must be set.

Option A — create the env file (recommended):
  cp $DEV_ENV_DIR/gh-webhooks.env.example $ENV_FILE
  chmod 600 $ENV_FILE
  # then edit it with real values

Option B — pass as positional args:
  $0 <owner/repo> <webhook-base-url>
EOF
    exit 2
fi

BASE="${BASE%/}"   # strip trailing slash

# Verify gh scope before bothering the user with failed API calls.
if ! gh auth status 2>&1 | grep -q 'admin:repo_hook'; then
    cat >&2 <<EOF
ERROR: current gh token lacks 'admin:repo_hook' scope.

Refresh with:
  gh auth refresh -h github.com -s admin:repo_hook
EOF
    exit 1
fi

# path → space-separated GitHub event names. Matches what each workflow's
# IF node filters on. Kept explicit so a workflow edit forces a review
# here; fewer surprises than autodetection.
declare -a DESIRED
DESIRED+=("qa-issue                  issues")
DESIRED+=("release-merge             pull_request")
DESIRED+=("pr-build-comment          issue_comment")
DESIRED+=("pr-review-request         pull_request")
DESIRED+=("pr-address-comment        issue_comment")
DESIRED+=("pr-address-review-comment pull_request_review_comment")
DESIRED+=("pr-merge                  issue_comment")

echo "==> fetching existing webhooks from $REPO"
gh api "repos/$REPO/hooks" --paginate > /tmp/gh-hooks.json

summarize_hook_by_id() {
    jq -r --argjson id "$1" \
        '.[] | select(.id == $id)
             | "id=\(.id) url=\(.config.url) events=\(.events | join(","))"' \
        /tmp/gh-hooks.json
}

for line in "${DESIRED[@]}"; do
    # shellcheck disable=SC2086
    set -- $line
    PATH_SEG="$1"; shift
    EVENTS=("$@")
    URL="$BASE/webhook/$PATH_SEG"

    EVENTS_JSON="$(printf '%s\n' "${EVENTS[@]}" | jq -Rs \
        'split("\n") | map(select(length > 0))')"

    # Match by /webhook/<path> suffix so changing the base URL still
    # finds the existing hook and updates it in place (rather than
    # leaking a new hook alongside a stale one).
    EXISTING_ID="$(jq -r --arg suffix "/webhook/$PATH_SEG" \
        '[.[] | select(.config.url | endswith($suffix)) | .id] | .[0] // empty' \
        /tmp/gh-hooks.json)"

    if [ -z "$EXISTING_ID" ]; then
        echo "==> creating   $PATH_SEG  →  $URL"
        BODY=$(jq -n --arg url "$URL" --argjson events "$EVENTS_JSON" '{
            name: "web",
            active: true,
            events: $events,
            config: {
                url: $url,
                content_type: "json",
                insecure_ssl: "0"
            }
        }')
        gh api -X POST "repos/$REPO/hooks" \
            --input - <<<"$BODY" >/dev/null
        NEW_ID="$(gh api "repos/$REPO/hooks" --paginate \
            --jq --arg url "$URL" '.[] | select(.config.url == $url) | .id')"
        echo "    id=$NEW_ID events=${EVENTS[*]}"
        continue
    fi

    CURRENT_URL="$(jq -r --argjson id "$EXISTING_ID" \
        '.[] | select(.id == $id) | .config.url' /tmp/gh-hooks.json)"
    CURRENT_EVENTS="$(jq -r --argjson id "$EXISTING_ID" \
        '.[] | select(.id == $id) | .events | sort | join(",")' \
        /tmp/gh-hooks.json)"
    WANT_EVENTS="$(printf '%s\n' "${EVENTS[@]}" | sort | paste -sd, -)"

    if [ "$CURRENT_URL" = "$URL" ] && [ "$CURRENT_EVENTS" = "$WANT_EVENTS" ]; then
        echo "==> up-to-date $PATH_SEG  (id=$EXISTING_ID)"
        continue
    fi

    CHANGES=()
    [ "$CURRENT_URL" != "$URL" ] && CHANGES+=("url: $CURRENT_URL → $URL")
    [ "$CURRENT_EVENTS" != "$WANT_EVENTS" ] && \
        CHANGES+=("events: $CURRENT_EVENTS → $WANT_EVENTS")
    echo "==> updating   $PATH_SEG  (id=$EXISTING_ID)"
    for c in "${CHANGES[@]}"; do echo "    $c"; done

    BODY=$(jq -n --arg url "$URL" --argjson events "$EVENTS_JSON" '{
        active: true,
        events: $events,
        config: {
            url: $url,
            content_type: "json",
            insecure_ssl: "0"
        }
    }')
    gh api -X PATCH "repos/$REPO/hooks/$EXISTING_ID" \
        --input - <<<"$BODY" >/dev/null
done

# Surface strays: hooks pointing at the same base URL but not in our
# desired list. Common causes: renamed a workflow path, or left a hook
# from a prior manual setup.
echo
echo "==> stray hooks under $BASE (not in desired set)"
DESIRED_URLS_JSON=$(printf '%s\n' "${DESIRED[@]}" | awk '{print "'"$BASE"'/webhook/"$1}' \
    | jq -Rs 'split("\n") | map(select(length > 0))')
STRAYS=$(jq -r --argjson want "$DESIRED_URLS_JSON" --arg base "$BASE" '
    .[] | select(.config.url | startswith($base))
        | select([.config.url] | inside($want) | not)
        | "  id=\(.id) url=\(.config.url) events=\(.events | join(","))"
' /tmp/gh-hooks.json)
if [ -n "$STRAYS" ]; then
    echo "$STRAYS"
    echo
    echo "  delete with:  gh api -X DELETE repos/$REPO/hooks/<id>"
else
    echo "  (none)"
fi

rm -f /tmp/gh-hooks.json
echo
echo "done. 7 webhooks synced."
