#!/bin/bash
# Container entrypoint: generate host keys, run first-run as the agent user,
# then hand off to sshd in the foreground.
set -euo pipefail

# The compose stack tmpfs-mounts /run, which shadows the /run/sshd we
# created in the Dockerfile and comes up world-writable. sshd refuses to
# run in a world-writable privilege-separation dir, so recreate it here
# with 0755 on every boot.
mkdir -p /run/sshd
chmod 0755 /run/sshd

# Generate host keys on first boot if missing (volume may be empty).
ssh-keygen -A

# Volumes may land as root when first mounted; fix ownership idempotently.
chown -R agent:agent /home/agent /cache 2>/dev/null || true

# Per-container first-run (idempotent — safe to re-invoke on every boot).
sudo -u agent \
    --preserve-env=ROLE,GITHUB_TOKEN,GITHUB_REPO,ANTHROPIC_API_KEY,IPHONE_SSH_PUBKEY,N8N_SSH_PUBKEY,MAC_HOST,MAC_HOST_USER,MAC_HOST_PORT \
    /usr/local/bin/first-run

exec /usr/sbin/sshd -D -e
