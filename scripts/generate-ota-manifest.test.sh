#!/usr/bin/env bash
# Smoke test for scripts/generate-ota-manifest.sh.
#
# Copies the generator into an isolated fake-repo tree (so it writes to a
# temp directory rather than the real deploy/website/ota/), then checks:
#   - First-run single version produces manifest + install page + root listing.
#   - Multiple versions are listed in descending semver order, with the
#     highest semver (not lexicographic max) badged "latest".
#   - Re-running with the same args is idempotent (byte-identical output).
#   - Env overrides (GITHUB_REPOSITORY, BUNDLE_ID, TITLE, DOMAIN) are honored.
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

if [ -f "${ota}/index.html" ]; then
    pass "root index.html written"
else
    fail "root index.html missing"
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

# -------- Test 2: multiple versions → semver descending with badge --------
echo "Test 2: multiple versions sorted descending"
run_gen 1.9.0
run_gen 1.10.10
run_gen 2.0.0
run_gen 1.10.2   # re-run to rebuild root listing

root_index="${ota}/index.html"
# Extract version strings in the order they appear in the listing.
order=$(grep -oE '/ota/[0-9]+\.[0-9]+\.[0-9]+/' "$root_index" \
    | sed 's|/ota/||; s|/||')
expected=$(printf '2.0.0\n1.10.10\n1.10.2\n1.9.0\n')
if [ "$order" = "$expected" ]; then
    pass "versions in descending semver order"
else
    fail "version order wrong"
    printf '    got:\n%s\n    expected:\n%s\n' "$order" "$expected" >&2
fi

# Badge must be on 2.0.0 row only.
badge_line=$(grep -n 'class="badge"' "$root_index" | head -1 || true)
if [ -n "$badge_line" ] && printf '%s' "$badge_line" | grep -q '2.0.0'; then
    pass "latest badge on highest semver"
else
    fail "latest badge missing or on wrong version (line: ${badge_line})"
fi

badge_count=$(grep -c 'class="badge"' "$root_index" || true)
if [ "$badge_count" = "1" ]; then
    pass "exactly one latest badge"
else
    fail "expected 1 badge, got ${badge_count}"
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
