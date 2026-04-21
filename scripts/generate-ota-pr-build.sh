#!/usr/bin/env bash
# Generates the OTA manifest.plist + install page for a per-PR iOS ad-hoc build.
#
# Usage: scripts/generate-ota-pr-build.sh <pr-number> <short-sha> <out-dir>
#   e.g. scripts/generate-ota-pr-build.sh 93 62308dc /tmp/ios-build
#
# Writes into <out-dir>:
#   manifest.plist  — points at https://<DOMAIN>/ota/pr-<N>/<short-sha>/StellarChat.ipa
#   index.html      — landing page with itms-services install button
#
# The n8n /build flow then scp's these two files plus StellarChat.ipa to
# droplet:/opt/onym-chat/deploy/website/ota/pr-<N>/<short-sha>/. The root
# /ota/index.html that lists all builds is rebuilt separately on the droplet
# by scripts/regenerate-ota-index.sh.
#
# Env overrides (normally unset; mirror generate-ota-manifest.sh):
#   BUNDLE_ID  — iOS bundle identifier (default chat.onym.ios)
#   TITLE      — app title shown in the iOS install prompt (default Onym)
#   DOMAIN     — domain the manifest is served from (default onym.chat)

set -euo pipefail

PR_NUM="${1:?pr number required}"
SHORT_SHA="${2:?short sha required}"
OUT_DIR="${3:?out dir required}"

# Sanity-check inputs. The values end up in filesystem paths, a URL, and the
# plist body, so a stray shell metacharacter would be bad.
if ! [[ "$PR_NUM" =~ ^[0-9]+$ ]]; then
    echo "ERROR: pr number '$PR_NUM' must be digits" >&2
    exit 2
fi
if ! [[ "$SHORT_SHA" =~ ^[0-9a-f]{7,40}$ ]]; then
    echo "ERROR: short sha '$SHORT_SHA' must be 7–40 hex chars" >&2
    exit 2
fi

BUNDLE_ID="${BUNDLE_ID:-chat.onym.ios}"
TITLE="${TITLE:-Onym}"
DOMAIN="${DOMAIN:-onym.chat}"

LABEL="PR #${PR_NUM} · ${SHORT_SHA}"
IPA_URL="https://${DOMAIN}/ota/pr-${PR_NUM}/${SHORT_SHA}/StellarChat.ipa"
MANIFEST_URL="https://${DOMAIN}/ota/pr-${PR_NUM}/${SHORT_SHA}/manifest.plist"

mkdir -p "$OUT_DIR"

cat > "${OUT_DIR}/manifest.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>items</key>
    <array>
        <dict>
            <key>assets</key>
            <array>
                <dict>
                    <key>kind</key>
                    <string>software-package</string>
                    <key>url</key>
                    <string>${IPA_URL}</string>
                </dict>
                <dict>
                    <key>kind</key>
                    <string>display-image</string>
                    <key>url</key>
                    <string>https://${DOMAIN}/icon.png</string>
                </dict>
                <dict>
                    <key>kind</key>
                    <string>full-size-image</string>
                    <key>url</key>
                    <string>https://${DOMAIN}/icon.png</string>
                </dict>
            </array>
            <key>metadata</key>
            <dict>
                <key>bundle-identifier</key>
                <string>${BUNDLE_ID}</string>
                <key>bundle-version</key>
                <string>pr-${PR_NUM}.${SHORT_SHA}</string>
                <key>kind</key>
                <string>software</string>
                <key>title</key>
                <string>${TITLE} (${LABEL})</string>
            </dict>
        </dict>
    </array>
</dict>
</plist>
PLIST

cat > "${OUT_DIR}/index.html" <<HTML
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Install ${TITLE} — ${LABEL}</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            max-width: 420px;
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
        .version { color: #6e6e73; font-size: 15px; margin-bottom: 32px; }
        a.install {
            display: inline-block;
            background: #6c5ce7;
            color: #fff;
            text-decoration: none;
            padding: 14px 32px;
            border-radius: 22px;
            font-size: 17px;
            font-weight: 600;
        }
        a.install:active { opacity: 0.85; }
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
    <img class="icon" src="/icon.png" alt="${TITLE}">
    <h1>${TITLE}</h1>
    <div class="version">${LABEL} &middot; debug build</div>

    <a class="install"
       href="itms-services://?action=download-manifest&url=${MANIFEST_URL}">
        Install on iPhone
    </a>

    <div class="note">
        <strong>Requirements:</strong> Open this page in Safari on iOS. Your device UDID must be registered in the ad-hoc provisioning profile used to sign this build. PR debug builds reuse the release ad-hoc profile, so if release installs work for you, this will too.
    </div>
</body>
</html>
HTML

echo "Wrote ${OUT_DIR}/manifest.plist"
echo "Wrote ${OUT_DIR}/index.html"
