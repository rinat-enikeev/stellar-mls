#!/bin/bash
# reset-agent.sh qa|release
# Nukes a single agent's home and cache volumes and rebuilds the container.
# The fresh agent will generate a new SSH keypair — you'll need to
# re-authorise it on the Mac host.
set -euo pipefail

DEV_ENV_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$DEV_ENV_DIR"

ROLE="${1:-}"
case "$ROLE" in
    qa|release) ;;
    *) echo "usage: reset-agent.sh qa|release" >&2; exit 2 ;;
esac

SERVICE="${ROLE}-agent"
CONTAINER="stellar-${ROLE}-agent"

echo "==> stopping ${SERVICE}"
docker compose -f docker-compose.dev.yml stop "$SERVICE" || true
docker compose -f docker-compose.dev.yml rm -f "$SERVICE" || true

echo "==> removing volumes"
# Compose namespaces volumes as <project>_<name>. Project name is taken from
# the top-level `name:` key in docker-compose.dev.yml (stellar-devenv).
docker volume rm "stellar-devenv_${ROLE}-home" "stellar-devenv_${ROLE}-cache" 2>&1 || true

echo "==> rebuilding ${SERVICE}"
docker compose -f docker-compose.dev.yml up -d --build "$SERVICE"

echo
echo "==> new public key (authorise on stellar-builder@mac-host):"
sleep 2
docker logs "$CONTAINER" 2>&1 | awk '/^ssh-ed25519/{print}' | tail -1 || true
