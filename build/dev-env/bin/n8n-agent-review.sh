#!/usr/bin/env bash
# Run Claude in a read-only worktree at a PR's head SHA and post a review
# comment. Invoked by n8n workflow 04-pr-review-request.
#
# Args:
#   $1 = PR number
#   $2 = PR base ref
#   $3 = PR head SHA
#   $4 = prompt (base64)
#
# Output contract (consumed downstream):
#   CLAUDE_RC=<int>
#   REVIEW_RC=<int>
set +e
set -o pipefail

PR="$1"
BASE="$2"
SHA="$3"
PROMPT_B64="$4"

if [ -z "$PR" ] || [ -z "$BASE" ] || [ -z "$SHA" ] || [ -z "$PROMPT_B64" ]; then
  echo "ERROR: n8n-agent-review.sh requires PR, base, SHA, and prompt" >&2
  exit 2
fi

PRIMARY="$HOME/workspace"
WT_ROOT="$HOME/workspace-worktrees"
WT_DIR="$WT_ROOT/review-pr-$PR"
PROMPT_FILE="/tmp/review-$PR.txt"
BODY_FILE="/tmp/review-body-$PR.md"

cleanup() {
  cd "$PRIMARY" 2>/dev/null || return
  git worktree remove --force "$WT_DIR" >/dev/null 2>&1 || rm -rf "$WT_DIR" 2>/dev/null
  git worktree prune >/dev/null 2>&1
}

cd "$PRIMARY" || { echo "ERROR: $PRIMARY not found" >&2; exit 10; }

# Fetch the PR head commit (via the pull ref) and the base ref so the diff
# below is correct. Using FETCH_HEAD is enough because we reference $SHA
# explicitly when adding the worktree.
git fetch origin "pull/$PR/head" >/dev/null 2>&1
git fetch origin "$BASE" >/dev/null 2>&1

mkdir -p "$WT_ROOT"
git worktree prune >/dev/null 2>&1
if [ -d "$WT_DIR" ]; then
  git worktree remove --force "$WT_DIR" >/dev/null 2>&1 || rm -rf "$WT_DIR"
fi
GIT_ERR="/tmp/git-wt-err-$$"
if ! git worktree add --detach "$WT_DIR" "$SHA" >/dev/null 2>"$GIT_ERR"; then
  echo "ERROR: could not create worktree $WT_DIR at $SHA" >&2
  sed 's/^/  git: /' "$GIT_ERR" >&2
  rm -f "$GIT_ERR"
  exit 11
fi
rm -f "$GIT_ERR"

cd "$WT_DIR" || { cleanup; exit 12; }

echo "$PROMPT_B64" | base64 -d > "$PROMPT_FILE"
echo "" >> "$PROMPT_FILE"
echo "=== DIFF ===" >> "$PROMPT_FILE"
git diff "origin/$BASE...HEAD" >> "$PROMPT_FILE"
REVIEW=$(claude --print < "$PROMPT_FILE")
CLAUDE_RC=$?
rm -f "$PROMPT_FILE"
echo "CLAUDE_RC=$CLAUDE_RC"

if [ -z "$REVIEW" ]; then
  echo "ERROR: empty review from claude" >&2
  cleanup
  exit 0
fi

printf '%s\n' "$REVIEW" > "$BODY_FILE"
gh pr review "$PR" --comment --body-file "$BODY_FILE" 2>&1
REVIEW_RC=$?
rm -f "$BODY_FILE"
echo "REVIEW_RC=$REVIEW_RC"

cleanup
exit 0
