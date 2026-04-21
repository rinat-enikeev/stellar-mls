#!/usr/bin/env bash
# Generates the OTA manifest.plist + install page for an iOS ad-hoc release.
#
# Usage: scripts/generate-ota-manifest.sh <version>
#   e.g. scripts/generate-ota-manifest.sh 1.10.2
#
# Env overrides (normally unset):
#   GITHUB_REPOSITORY  — owner/repo for the IPA URL (default rinat-enikeev/stellar-mls)
#   BUNDLE_ID          — iOS bundle identifier (default chat.onym.ios)
#   TITLE              — app title shown in the iOS install prompt (default Onym)
#   DOMAIN             — domain the manifest is served from (default onym.chat)
#
# Writes:
#   deploy/website/ota/<version>/manifest.plist
#   deploy/website/ota/<version>/index.html
#   deploy/website/ota/index.html  (redirect to latest version)

set -euo pipefail

VERSION="${1:?version required (without leading v), e.g. 1.10.2}"
REPO="${GITHUB_REPOSITORY:-rinat-enikeev/stellar-mls}"
BUNDLE_ID="${BUNDLE_ID:-chat.onym.ios}"
TITLE="${TITLE:-Onym}"
DOMAIN="${DOMAIN:-onym.chat}"

IPA_URL="https://github.com/${REPO}/releases/download/v${VERSION}/StellarChat-${VERSION}.ipa"

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
ota_root="${repo_root}/deploy/website/ota"
version_dir="${ota_root}/${VERSION}"
mkdir -p "$version_dir"

cat > "${version_dir}/manifest.plist" <<PLIST
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
                <string>${VERSION}</string>
                <key>kind</key>
                <string>software</string>
                <key>title</key>
                <string>${TITLE}</string>
            </dict>
        </dict>
    </array>
</dict>
</plist>
PLIST

cat > "${version_dir}/index.html" <<HTML
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Install ${TITLE} ${VERSION}</title>
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
    <div class="version">Version ${VERSION} &middot; ad-hoc build</div>

    <a class="install"
       href="itms-services://?action=download-manifest&url=https://${DOMAIN}/ota/${VERSION}/manifest.plist">
        Install on iPhone
    </a>

    <div class="note">
        <strong>Requirements:</strong> Open this page in Safari on iOS. Your device UDID must be registered in the ad-hoc provisioning profile used to sign <code>v${VERSION}</code>. If the install fails with "Unable to Install", the UDID was not included in that build.
    </div>
</body>
</html>
HTML

cat > "${ota_root}/index.html" <<ROOT
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta http-equiv="refresh" content="0; url=/ota/${VERSION}/">
    <title>${TITLE} OTA — latest</title>
</head>
<body>
    <p>Redirecting to <a href="/ota/${VERSION}/">${TITLE} ${VERSION}</a>&hellip;</p>
</body>
</html>
ROOT

echo "Wrote ${version_dir}/manifest.plist"
echo "Wrote ${version_dir}/index.html"
echo "Updated ${ota_root}/index.html -> /ota/${VERSION}/"
