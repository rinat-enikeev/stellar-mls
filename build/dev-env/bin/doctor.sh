#!/bin/bash
# Verification checklist for the dev-env stack. Each check reports PASS/FAIL
# and the script exits non-zero on any failure. Most checks are structural —
# the ones that talk to the Mac host will only pass on the Mac mini, not on
# a dev laptop.
set -u

DEV_ENV_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$DEV_ENV_DIR"

PASS=0
FAIL=0
check() {
    local label="$1"; shift
    if "$@" >/dev/null 2>&1; then
        printf "  [\033[32mPASS\033[0m] %s\n" "$label"
        PASS=$((PASS+1))
    else
        printf "  [\033[31mFAIL\033[0m] %s\n" "$label"
        FAIL=$((FAIL+1))
    fi
}

echo "== images =="
check "dev-env-base image present"  docker image inspect stellar-mls/dev-env-base:latest
check "dev-env-agent image present" docker image inspect stellar-mls/dev-env-agent:latest
check "base image is arm64" \
    bash -c 'test "$(docker image inspect stellar-mls/dev-env-base:latest --format "{{.Architecture}}")" = "arm64"'
check "agent image is arm64" \
    bash -c 'test "$(docker image inspect stellar-mls/dev-env-agent:latest --format "{{.Architecture}}")" = "arm64"'

echo
echo "== containers =="
for c in stellar-qa-agent stellar-release-agent stellar-n8n; do
    check "$c running" \
        bash -c "test \"\$(docker inspect -f '{{.State.Running}}' $c 2>/dev/null)\" = 'true'"
done

echo
echo "== sshd reachable on host ports =="
check "qa-agent sshd on 2201" \
    bash -c 'echo > /dev/tcp/127.0.0.1/2201'
check "release-agent sshd on 2202" \
    bash -c 'echo > /dev/tcp/127.0.0.1/2202'
check "n8n ui on 5678" \
    bash -c 'echo > /dev/tcp/127.0.0.1/5678'

echo
echo "== in-container toolchains =="
for c in stellar-qa-agent stellar-release-agent; do
    check "$c: claude cli"  docker exec -u agent "$c" claude --version
    check "$c: gh cli"      docker exec -u agent "$c" gh --version
    check "$c: rustc"       docker exec -u agent "$c" rustc --version
    check "$c: cargo"       docker exec -u agent "$c" cargo --version
    check "$c: java"        docker exec -u agent "$c" java -version
    check "$c: sdkmanager"  docker exec -u agent "$c" sdkmanager --version
    check "$c: remote-xcodebuild present" docker exec "$c" test -x /usr/local/bin/remote-xcodebuild
    check "$c: remote-jnilibs present"    docker exec "$c" test -x /usr/local/bin/remote-jnilibs
done

echo
echo "== workspace cloned =="
for c in stellar-qa-agent stellar-release-agent; do
    check "$c: workspace is a git repo" \
        docker exec -u agent "$c" test -d /home/agent/workspace/.git
done

echo
echo "== Mac host delegation (only on the Mac mini) =="
for c in stellar-qa-agent stellar-release-agent; do
    check "$c: reach mac-host sshd" \
        docker exec -u agent "$c" bash -c '
            ssh -o BatchMode=yes -o ConnectTimeout=5 \
                -o StrictHostKeyChecking=accept-new \
                -p "${MAC_HOST_PORT:-22}" \
                "${MAC_HOST_USER}@${MAC_HOST}" \
                true 2>&1 | grep -qvE "Permission denied|Connection refused|Could not resolve"
        '
done

echo
echo "== n8n health =="
check "n8n /healthz" curl -fsS http://127.0.0.1:5678/healthz

echo
echo "---------------------------------------"
printf "  %d passed, %d failed\n" "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
