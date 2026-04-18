#!/usr/bin/env bash
# Mirror scripts/.env to GitHub Actions secrets for the current repo.
#
# Each KEY=VALUE line in the env file becomes a secret with the same name.
# Empty values are skipped (so a half-filled .env doesn't clobber CI).
# Comments and blank lines are ignored.
#
# Requires: gh CLI authenticated (`gh auth status`).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="${1:-$SCRIPT_DIR/.env}"

if ! command -v gh >/dev/null 2>&1; then
  echo "gh CLI not found — install from https://cli.github.com/" >&2
  exit 1
fi
if ! gh auth status >/dev/null 2>&1; then
  echo "gh not authenticated — run 'gh auth login' first" >&2
  exit 1
fi
if [ ! -r "$ENV_FILE" ]; then
  echo "env file not readable: $ENV_FILE" >&2
  echo "  cp scripts/.env.example $ENV_FILE  # then edit" >&2
  exit 1
fi

REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner)"
echo "syncing secrets to $REPO from $ENV_FILE"
echo

set_count=0
skip_count=0

while IFS= read -r line || [ -n "$line" ]; do
  # strip CR (Windows line endings) and leading whitespace
  line="${line%$'\r'}"
  line="${line#"${line%%[![:space:]]*}"}"
  case "$line" in ''|'#'*) continue;; esac
  case "$line" in *=*) ;; *) continue;; esac

  key="${line%%=*}"
  val="${line#*=}"

  # strip surrounding double-quotes if present
  if [ "${val#\"}" != "$val" ] && [ "${val%\"}" != "$val" ]; then
    val="${val#\"}"
    val="${val%\"}"
  fi

  if [ -z "$val" ]; then
    printf '  skip  %s (empty)\n' "$key"
    skip_count=$((skip_count + 1))
    continue
  fi

  printf '  set   %s\n' "$key"
  # Omit --body entirely so gh reads the value from stdin.
  # `--body -` would store the literal string "-" (1 char) — DO NOT use.
  printf '%s' "$val" | gh secret set "$key" --repo "$REPO"
  set_count=$((set_count + 1))
done < "$ENV_FILE"

echo
echo "done. set $set_count secret(s), skipped $skip_count empty."
