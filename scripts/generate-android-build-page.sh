#!/usr/bin/env bash
# Emits a static install landing page for a PR debug Android build.
#
# Usage: scripts/generate-android-build-page.sh <mode> <pr> <short_sha> <full_sha> <download_url> <repo>
#   mode         — "permalink" (per-sha landing page) or "latest" (per-pr redirect to latest sha)
#   pr           — PR number (e.g. 91)
#   short_sha    — 7-char commit SHA (e.g. 6a0438c)
#   full_sha     — full commit SHA
#   download_url — direct URL to the APK asset
#   repo         — owner/repo (e.g. rinat-enikeev/stellar-mls)
#
# Output: HTML on stdout. Caller is responsible for writing to disk.

set -euo pipefail

MODE="${1:?mode required (permalink|latest)}"
PR="${2:?pr number required}"
SHORT_SHA="${3:?short_sha required}"
FULL_SHA="${4:?full_sha required}"
DOWNLOAD_URL="${5:?download_url required}"
REPO="${6:?repo required}"

BUILT_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
COMMIT_URL="https://github.com/${REPO}/commit/${FULL_SHA}"
PR_URL="https://github.com/${REPO}/pull/${PR}"

if [ "$MODE" = "latest" ]; then
cat <<HTML
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <meta http-equiv="refresh" content="0; url=/build/pr-${PR}/${SHORT_SHA}/">
    <title>PR #${PR} &mdash; latest Android build</title>
</head>
<body>
    <p>Redirecting to <a href="/build/pr-${PR}/${SHORT_SHA}/">latest build (${SHORT_SHA})</a>&hellip;</p>
</body>
</html>
HTML
    exit 0
fi

cat <<HTML
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Onym PR #${PR} &mdash; Android build ${SHORT_SHA}</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            max-width: 440px;
            margin: 0 auto;
            padding: 48px 24px;
            color: #1c1c1e;
            background: #f2f2f7;
            text-align: center;
        }
        img.icon {
            width: 96px;
            height: 96px;
            border-radius: 22px;
            box-shadow: 0 4px 14px rgba(0,0,0,0.12);
        }
        h1 { font-size: 24px; margin: 16px 0 4px; }
        .meta { color: #6e6e73; font-size: 14px; margin-bottom: 32px; line-height: 1.6; }
        .meta a { color: #6e6e73; }
        a.download {
            display: inline-block;
            background: #6c5ce7;
            color: #fff;
            text-decoration: none;
            padding: 14px 32px;
            border-radius: 22px;
            font-size: 17px;
            font-weight: 600;
        }
        a.download:active { opacity: 0.85; }
        .note {
            margin-top: 32px;
            padding: 16px;
            background: #fff;
            border-radius: 12px;
            font-size: 14px;
            color: #3c3c43;
            text-align: left;
            line-height: 1.5;
        }
        .note strong { color: #1c1c1e; }
        code {
            background: #e5e5ea;
            padding: 2px 6px;
            border-radius: 4px;
            font-size: 13px;
        }
    </style>
</head>
<body>
    <img class="icon" src="/icon.png" alt="Onym">
    <h1>Onym &mdash; PR #${PR}</h1>
    <div class="meta">
        Debug build of <a href="${COMMIT_URL}"><code>${SHORT_SHA}</code></a><br>
        <a href="${PR_URL}">PR #${PR}</a> &middot; built ${BUILT_AT}
    </div>

    <a class="download" href="${DOWNLOAD_URL}">Download APK</a>

    <div class="note">
        <strong>Install on Android:</strong> Open this page on your device, tap <em>Download APK</em>, then open the file to install. You may need to enable <em>Install unknown apps</em> for your browser.
        <br><br>
        <strong>Note:</strong> this is an unsigned debug build from a pull request &mdash; use only for review and testing.
    </div>
</body>
</html>
HTML
