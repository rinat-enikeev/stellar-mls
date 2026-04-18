#!/usr/bin/env bash
# Run Claude on a fresh worktree for a GitHub issue and open a PR.
#
# Invoked by n8n workflow 01-qa-agent-issue. The calling SSH command is
# expected to have already cd'd to ~/workspace, synced origin/main, and
# exported GH_TOKEN — see the wrapper snippet in the workflow JSON.
#
# Args:
#   $1 = issue number
#   $2 = prompt (base64)
#   $3 = reviewer login (may be empty)
#
# Output contract (consumed by the workflow's "Comment on issue" step):
#   CLAUDE_RC=<int>
#   PUSH_RC=<int>
#   GH_RC=<int>
#   PR_URL=<url or empty>
set +e
set -o pipefail

ISSUE="$1"
PROMPT_B64="$2"
REVIEWER="$3"

if [ -z "$ISSUE" ] || [ -z "$PROMPT_B64" ]; then
  echo "ERROR: n8n-agent-issue.sh requires issue number and prompt" >&2
  echo "PR_URL="
  exit 2
fi

PRIMARY="$HOME/workspace"
WT_ROOT="$HOME/workspace-worktrees"
WT_DIR="$WT_ROOT/issue-$ISSUE"
BRANCH="agent/issue-$ISSUE"
PROMPT_FILE="/tmp/prompt-issue-$ISSUE.txt"

cleanup() {
  cd "$PRIMARY" 2>/dev/null || return
  git worktree remove --force "$WT_DIR" >/dev/null 2>&1 || rm -rf "$WT_DIR" 2>/dev/null
  git worktree prune >/dev/null 2>&1
}

cd "$PRIMARY" || { echo "ERROR: $PRIMARY not found" >&2; echo "PR_URL="; exit 10; }

# Idempotency: GitHub fires several events per issue (opened, labeled,
# assigned) and the workflow filter accepts all of them, so the same issue
# can trigger this script multiple times. If a PR for this branch is
# already open, skip the whole run (no Claude, no push, no extra comment).
EXISTING=$(gh pr list --head "$BRANCH" --state open --json url --jq '.[0].url' 2>&1)
if [ -n "$EXISTING" ] && [ "${EXISTING#https://}" != "$EXISTING" ]; then
  echo "SKIPPED=existing-pr"
  echo "PR_URL=$EXISTING"
  exit 0
fi

mkdir -p "$WT_ROOT"
git worktree prune >/dev/null 2>&1
if [ -d "$WT_DIR" ]; then
  git worktree remove --force "$WT_DIR" >/dev/null 2>&1 || rm -rf "$WT_DIR"
fi
# Evict any OTHER worktree that has $BRANCH checked out — otherwise
# `worktree add -B` refuses to reset a branch held elsewhere.
CONFLICT_WT=$(git worktree list --porcelain 2>/dev/null | awk -v b="refs/heads/$BRANCH" -v me="$WT_DIR" '
  /^worktree / { wt=$2 }
  /^branch / && $2==b && wt!=me { print wt; exit }
')
if [ -n "$CONFLICT_WT" ] && [ "$CONFLICT_WT" != "$PRIMARY" ]; then
  git worktree remove --force "$CONFLICT_WT" >/dev/null 2>&1 || rm -rf "$CONFLICT_WT"
  git worktree prune >/dev/null 2>&1
fi
# Stale branch from a prior failed run would block `worktree add -B`.
git branch -D "$BRANCH" >/dev/null 2>&1
GIT_ERR="/tmp/git-wt-err-$$"
if ! git worktree add -B "$BRANCH" "$WT_DIR" origin/main >/dev/null 2>"$GIT_ERR"; then
  echo "ERROR: could not create worktree $WT_DIR" >&2
  sed 's/^/  git: /' "$GIT_ERR" >&2
  rm -f "$GIT_ERR"
  echo "PR_URL="
  exit 11
fi
rm -f "$GIT_ERR"

cd "$WT_DIR" || { cleanup; echo "PR_URL="; exit 12; }

CLAUDE_LOG="/tmp/claude-issue-$ISSUE.log"
echo "$PROMPT_B64" | base64 -d > "$PROMPT_FILE"
claude --print < "$PROMPT_FILE" > "$CLAUDE_LOG" 2>&1
CLAUDE_RC=$?
rm -f "$PROMPT_FILE"
echo "CLAUDE_RC=$CLAUDE_RC"
echo "CLAUDE_LOG_BYTES=$(wc -c < "$CLAUDE_LOG" 2>/dev/null || echo 0)"
echo "CLAUDE_LOG_TAIL<<__END__"
tail -n 40 "$CLAUDE_LOG" 2>/dev/null || true
echo "__END__"
rm -f "$CLAUDE_LOG"

if git diff --quiet origin/main..HEAD; then
  echo "PR_URL="
  echo "ERROR: no changes produced by claude" >&2
  cleanup
  exit 0
fi

git push -u origin "$BRANCH" --force 2>&1
PUSH_RC=$?
echo "PUSH_RC=$PUSH_RC"
if [ "$PUSH_RC" -ne 0 ]; then
  echo "PR_URL="
  cleanup
  exit 0
fi

EXISTING=$(gh pr list --head "$BRANCH" --state open --json url --jq '.[0].url' 2>&1)
if [ -n "$EXISTING" ] && [ "${EXISTING#https://}" != "$EXISTING" ]; then
  # Another concurrent run raced us to PR creation. Don't double-comment.
  echo "SKIPPED=reused-pr"
  echo "PR_URL=$EXISTING"
  cleanup
  exit 0
fi

if [ -n "$REVIEWER" ]; then
  GH_OUT=$(gh pr create --fill --base main --head "$BRANCH" \
             --label ai-generated --reviewer "$REVIEWER" 2>&1)
else
  GH_OUT=$(gh pr create --fill --base main --head "$BRANCH" \
             --label ai-generated 2>&1)
fi
GH_RC=$?
echo "GH_RC=$GH_RC"
echo "GH_OUT<<__END__"
echo "$GH_OUT"
echo "__END__"
PR_URL=$(printf '%s\n' "$GH_OUT" | grep -Eo 'https://github.com/[^ ]+/pull/[0-9]+' | tail -1)
echo "PR_URL=$PR_URL"

cleanup
exit 0
