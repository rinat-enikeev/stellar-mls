#!/bin/bash
# Per-container first-run, executed as the `agent` user on every boot.
# Idempotent: generates an SSH keypair, seeds authorized_keys from
# IPHONE_SSH_PUBKEY, clones the workspace repo, and authenticates gh.
set -euo pipefail

cd "$HOME"
mkdir -p .ssh workspace .config/gh
chmod 700 .ssh

# Per-container host→host SSH key (container → mac-host forced command).
if [ ! -f .ssh/id_ed25519 ]; then
    ssh-keygen -t ed25519 -N "" -f .ssh/id_ed25519 \
        -C "${ROLE:-unknown}-agent@$(hostname)"
fi

# Authorise inbound SSH keys. The iPhone key is for interactive use via
# ProxyJump; the n8n key is what the orchestration stack presents when it
# drives headless builds. sshd is key-only (see ssh/sshd_config), so any
# key the container should accept must land here.
touch .ssh/authorized_keys
chmod 600 .ssh/authorized_keys
for _var in IPHONE_SSH_PUBKEY N8N_SSH_PUBKEY; do
    _val="${!_var:-}"
    if [ -n "$_val" ] && ! grep -qxF "$_val" .ssh/authorized_keys; then
        echo "$_val" >> .ssh/authorized_keys
    fi
done
unset _var _val

# Clone the workspace repo if empty, and keep the token embedded in the
# remote URL so non-interactive git pushes (from n8n SSH sessions that
# strip container env vars) can authenticate without a credential helper.
if [ -n "${GITHUB_TOKEN:-}" ] && [ -n "${GITHUB_REPO:-}" ]; then
    if [ ! -d workspace/.git ]; then
        git clone "https://x-access-token:${GITHUB_TOKEN}@github.com/${GITHUB_REPO}.git" workspace
    else
        git -C workspace remote set-url origin \
            "https://x-access-token:${GITHUB_TOKEN}@github.com/${GITHUB_REPO}.git"
    fi
fi

# gh auth + git credential helper so push/fetch don't require the token in URL.
if [ -n "${GITHUB_TOKEN:-}" ]; then
    echo "$GITHUB_TOKEN" | gh auth login --with-token --hostname github.com 2>/dev/null || true
    gh auth setup-git 2>/dev/null || true
fi

# Sensible git identity. Override these via `git config` inside the container
# if you'd rather have per-agent committer identities.
git config --global user.name  "${GIT_COMMITTER_NAME:-${ROLE}-agent}"
git config --global user.email "${GIT_COMMITTER_EMAIL:-${ROLE}-agent@stellar-mls.local}"
git config --global init.defaultBranch main
git config --global pull.rebase true

# Announce the per-container pubkey so the operator can authorise it on the
# Mac host (forced-command entry in stellar-builder's authorized_keys).
echo "=== $(hostname) (${ROLE:-?}) public key — add to stellar-builder@mac-host ==="
cat .ssh/id_ed25519.pub
echo "=== end pubkey ==="
