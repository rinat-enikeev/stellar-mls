#!/usr/bin/env bash
# Smoke test for scripts/generate-ota-manifest.sh.
#
# Copies the generator into an isolated fake-repo tree (so it writes to a
# temp directory rather than the real deploy/website/ota/), then checks:
#   - First-run single version produces manifest + per-version install page.
#   - Multiple invocations create separate version dirs without clobbering.
#   - Re-running with the same args is idempotent (byte-identical output).
#   - Env overrides (GITHUB_REPOSITORY, BUNDLE_ID, TITLE, DOMAIN) are honored.
#
# The root /ota/index.html is no longer this script's concern — it's
# regenerated on the droplet by scripts/regenerate-ota-index.sh so it can
# see both release and PR builds at once.
#
# Run: bash scripts/generate-ota-manifest.test.sh

set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
generator="${script_dir}/generate-ota-manifest.sh"

if [ ! -x "$generator" ] && [ ! -r "$generator" ]; then
    echo "FAIL: generator not found at ${generator}" >&2
    exit 1
fi

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

# Stage a fake repo: scripts/<generator>, plus the dirs the generator writes into.
mkdir -p "${tmpdir}/scripts" "${tmpdir}/deploy/website"
cp "$generator" "${tmpdir}/scripts/generate-ota-manifest.sh"
chmod +x "${tmpdir}/scripts/generate-ota-manifest.sh"

fail=0
pass() { printf '  ok  %s\n' "$1"; }
fail() { printf '  FAIL %s\n' "$1" >&2; fail=1; }

run_gen() {
    # Invoke the staged copy so the generator's `pwd` walk picks up the tmp repo.
    ( cd "$tmpdir" && bash scripts/generate-ota-manifest.sh "$@" ) >/dev/null
}

ota="${tmpdir}/deploy/website/ota"

# -------- Test 1: first-run with a single version --------
echo "Test 1: single version first run"
run_gen 1.10.2

if [ -f "${ota}/1.10.2/manifest.plist" ]; then
    pass "manifest.plist written"
else
    fail "manifest.plist missing"
fi

if [ -f "${ota}/1.10.2/index.html" ]; then
    pass "per-version index.html written"
else
    fail "per-version index.html missing"
fi

if [ ! -e "${ota}/index.html" ]; then
    pass "root index.html not written (delegated to regenerator)"
else
    fail "root index.html written — this script should no longer touch it"
fi

if grep -q 'v1.10.2/StellarChat-1.10.2.ipa' "${ota}/1.10.2/manifest.plist"; then
    pass "manifest references correct IPA URL"
else
    fail "manifest IPA URL wrong"
fi

if grep -q 'chat.onym.ios' "${ota}/1.10.2/manifest.plist"; then
    pass "default bundle-id present"
else
    fail "default bundle-id missing"
fi

# -------- Test 2: multiple versions produce separate dirs --------
echo "Test 2: multiple versions — each lands in its own dir"
run_gen 1.9.0
run_gen 1.10.10
run_gen 2.0.0

all_present=1
for v in 1.9.0 1.10.2 1.10.10 2.0.0; do
    if [ ! -f "${ota}/${v}/manifest.plist" ]; then
        fail "manifest missing for ${v}"
        all_present=0
    fi
done
if [ "$all_present" = "1" ]; then
    pass "all four versions have their own manifest.plist"
fi

# Each manifest must reference its own version — no cross-talk.
if grep -q 'v2.0.0/StellarChat-2.0.0.ipa' "${ota}/2.0.0/manifest.plist" \
    && grep -q 'v1.10.10/StellarChat-1.10.10.ipa' "${ota}/1.10.10/manifest.plist"; then
    pass "per-version manifests reference their own IPA"
else
    fail "cross-version IPA reference detected"
fi

# -------- Test 3: idempotency (same args → byte-identical output) --------
echo "Test 3: idempotency"
hash_before=$(find "$ota" -type f -exec shasum {} \; | sort | shasum | awk '{print $1}')
run_gen 2.0.0
run_gen 1.10.2
hash_after=$(find "$ota" -type f -exec shasum {} \; | sort | shasum | awk '{print $1}')
if [ "$hash_before" = "$hash_after" ]; then
    pass "re-run produces byte-identical tree"
else
    fail "re-run changed output (${hash_before} → ${hash_after})"
fi

# -------- Test 4: env overrides --------
echo "Test 4: env overrides"
rm -rf "$ota"
( cd "$tmpdir" && \
    GITHUB_REPOSITORY="someone/fork" \
    BUNDLE_ID="chat.example.ios" \
    TITLE="Example" \
    DOMAIN="example.test" \
    bash scripts/generate-ota-manifest.sh 3.0.0 ) >/dev/null

m="${ota}/3.0.0/manifest.plist"
if grep -q 'someone/fork/releases/download/v3.0.0/StellarChat-3.0.0.ipa' "$m"; then
    pass "GITHUB_REPOSITORY override applied"
else
    fail "GITHUB_REPOSITORY not applied"
fi
if grep -q 'chat.example.ios' "$m"; then
    pass "BUNDLE_ID override applied"
else
    fail "BUNDLE_ID not applied"
fi
if grep -q '<string>Example</string>' "$m"; then
    pass "TITLE override applied"
else
    fail "TITLE not applied"
fi
if grep -q 'example.test/ota/3.0.0/manifest.plist' "${ota}/3.0.0/index.html"; then
    pass "DOMAIN override applied in install-page itms-services URL"
else
    fail "DOMAIN not applied"
fi

echo
if [ "$fail" = "0" ]; then
    echo "PASS"
else
    echo "FAIL" >&2
    exit 1
fi
