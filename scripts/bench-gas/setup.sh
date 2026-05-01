#!/usr/bin/env bash
# Provision the bench environment: deployer identity, friendbot fund,
# contract builds. Idempotent — safe to re-run.
set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"

BENCH_NETWORK="${BENCH_NETWORK:-testnet}"
BENCH_DEPLOYER="${BENCH_DEPLOYER:-bench-gas-deployer}"
BENCH_CONFIG_DIR="${BENCH_CONFIG_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/bench-gas-config.XXXXXX")}"
BENCH_ARTIFACT_DIR="${BENCH_ARTIFACT_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/bench-gas-artifacts.XXXXXX")}"

export BENCH_NETWORK BENCH_DEPLOYER BENCH_CONFIG_DIR BENCH_ARTIFACT_DIR

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "missing required command: $1" >&2
        exit 1
    fi
}
require_cmd cargo
require_cmd stellar
require_cmd jq
require_cmd xxd

echo "==> bench config"
echo "    network:      $BENCH_NETWORK"
echo "    deployer:     $BENCH_DEPLOYER"
echo "    config-dir:   $BENCH_CONFIG_DIR"
echo "    artifact-dir: $BENCH_ARTIFACT_DIR"

echo "==> deployer identity"
if stellar keys public-key "$BENCH_DEPLOYER" --config-dir "$BENCH_CONFIG_DIR" >/dev/null 2>&1; then
    echo "    reusing existing identity"
else
    stellar keys generate "$BENCH_DEPLOYER" \
        --config-dir "$BENCH_CONFIG_DIR" \
        --network "$BENCH_NETWORK" \
        --overwrite >/dev/null
    echo "    generated new identity"
fi

BENCH_DEPLOYER_ADDRESS="$(stellar keys public-key "$BENCH_DEPLOYER" --config-dir "$BENCH_CONFIG_DIR" | tr -d '\n')"
export BENCH_DEPLOYER_ADDRESS

echo "==> friendbot fund (best-effort)"
stellar keys fund "$BENCH_DEPLOYER" \
    --config-dir "$BENCH_CONFIG_DIR" \
    --network "$BENCH_NETWORK" >/dev/null 2>&1 || \
    echo "    (friendbot returned non-zero; proceeding — account may already be funded)"

echo "==> building contracts"
mkdir -p "$BENCH_ARTIFACT_DIR"
for c in sep-anarchy sep-democracy sep-oligarchy sep-oneonone sep-tyranny; do
    echo "    $c"
    stellar contract build \
        --manifest-path "$REPO_ROOT/contracts/$c/Cargo.toml" \
        --out-dir "$BENCH_ARTIFACT_DIR" >/dev/null
done

# V2 will compile `gen-membership-proof` / `gen-update-proof` here
# once the deferred contract drivers consume them. V1 doesn't, so we
# don't burn CI time on the `gen-proof-tool` feature build (and a
# regression in those binaries can't fail the V1 bench).

echo "==> setup complete"
