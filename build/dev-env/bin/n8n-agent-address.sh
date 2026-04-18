#!/usr/bin/env bash
# Run Claude in a worktree to address a PR review comment, push if it made
# changes. Invoked by n8n workflow 05-pr-address-comment.
#
# Args:
#   $1 = PR number
#   $2 = PR branch (head ref)
#   $3 = triggering comment id (used to namespace the worktree / prompt)
#   $4 = prompt (base64)
#
# Output contract (consumed downstream):
#   CLAUDE_RC=<int>
#   NO_CHANGES=1        (if claude produced no diff)
#   PUSH_RC=<int>
#   NEW_SHA=<sha>
set +e
set -o pipefail

PR="$1"
BRANCH="$2"
CID="$3"
PROMPT_B64="$4"

if [ -z "$PR" ] || [ -z "$BRANCH" ] || [ -z "$PROMPT_B64" ]; then
  echo "ERROR: n8n-agent-address.sh requires PR, branch, and prompt" >&2
  exit 2
fi

PRIMARY="$HOME/workspace"
WT_ROOT="$HOME/workspace-worktrees"
WT_DIR="$WT_ROOT/pr-$PR-comment-$CID"
PROMPT_FILE="/tmp/fix-$PR-$CID.txt"

cleanup() {
  cd "$PRIMARY" 2>/dev/null || return
  git worktree remove --force "$WT_DIR" 2>&1 || rm -rf "$WT_DIR"
  git worktree prune 2>&1
}

cd "$PRIMARY" || { echo "ERROR: $PRIMARY not found" >&2; exit 10; }

git fetch origin "$BRANCH" 2>&1

mkdir -p "$WT_ROOT"
git worktree prune 2>&1
if [ -d "$WT_DIR" ]; then
  git worktree remove --force "$WT_DIR" 2>&1 || rm -rf "$WT_DIR"
fi
git branch -D "$BRANCH" 2>&1
git worktree add -B "$BRANCH" "$WT_DIR" "origin/$BRANCH" 2>&1 || {
  echo "ERROR: could not create worktree $WT_DIR for $BRANCH" >&2
  exit 11
}

cd "$WT_DIR" || { cleanup; exit 12; }

echo "$PROMPT_B64" | base64 -d > "$PROMPT_FILE"
claude --print < "$PROMPT_FILE"
CLAUDE_RC=$?
rm -f "$PROMPT_FILE"
echo "CLAUDE_RC=$CLAUDE_RC"

if git diff --quiet "origin/$BRANCH..HEAD"; then
  echo "NO_CHANGES=1"
  cleanup
  exit 0
fi

git push origin "HEAD:$BRANCH" 2>&1
PUSH_RC=$?
echo "PUSH_RC=$PUSH_RC"
NEW_SHA=$(git rev-parse HEAD)
echo "NEW_SHA=$NEW_SHA"

cleanup
exit 0
