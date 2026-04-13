#!/bin/bash
# Stop the dev-env stack. Does NOT remove volumes — agent state (workspace,
# Claude login, generated SSH keys) survives. Use reset-agent.sh to wipe a
# single agent's volumes.
set -euo pipefail

DEV_ENV_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$DEV_ENV_DIR"

docker compose -f docker-compose.dev.yml down
