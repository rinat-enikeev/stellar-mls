#!/bin/bash
# Bring up the dev-env compose stack. Builds the base + agent images first
# if they're missing, then brings up all three services.
set -euo pipefail

DEV_ENV_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$DEV_ENV_DIR"

for env in qa.env release.env n8n.env; do
    if [ ! -f "$env" ]; then
        echo "ERROR: $DEV_ENV_DIR/$env missing — copy from ${env}.example and fill in" >&2
        exit 1
    fi
done

if ! docker image inspect stellar-mls/dev-env-base:latest >/dev/null 2>&1; then
    echo "==> building base image (one-time, ~3 min)"
    docker build --platform=linux/arm64 \
        -t stellar-mls/dev-env-base:latest \
        -f Dockerfile.base \
        "$DEV_ENV_DIR"
fi

echo "==> building agent image + starting stack"
docker compose -f docker-compose.dev.yml up -d --build

echo
echo "==> services"
docker compose -f docker-compose.dev.yml ps

echo
echo "==> waiting for first-run.sh to emit container pubkeys (up to 30s each)"
for c in stellar-qa-agent stellar-release-agent; do
    key=""
    for _ in $(seq 1 30); do
        key=$(docker logs "$c" 2>&1 | awk '/^ssh-ed25519/{print; exit}')
        [ -n "$key" ] && break
        sleep 1
    done
    echo "--- $c ---"
    if [ -n "$key" ]; then
        echo "$key"
    else
        echo "(pubkey not printed yet — check: docker logs $c)"
    fi
done

cat <<'EOF'

Next steps:
  1. Copy each container's ssh-ed25519 line above into
     /Users/stellar-builder/.ssh/authorized_keys on the Mac host,
     prefixed with:
       command="/Users/stellar-builder/bin/host-build-dispatch",restrict
  2. From the Mac host test: `ssh -p 2201 agent@127.0.0.1 echo ok`
     (after also adding your own pubkey via IPHONE_SSH_PUBKEY in qa.env).
  3. Configure cloudflared tunnel → n8n and register webhooks.
  4. Run ./bin/doctor.sh for the verification checklist.
EOF
