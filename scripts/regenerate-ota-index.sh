#!/usr/bin/env bash
# Rebuilds /opt/onym-chat/deploy/website/ota/index.html on the droplet by
# scanning the filesystem. Runs remotely — callers pipe this script in via
# `ssh droplet bash -s`, so it must be self-contained (no external helpers,
# no args).
#
# Source of truth is the directory tree:
#   /opt/onym-chat/deploy/website/ota/<semver>/manifest.plist         → release
#   /opt/onym-chat/deploy/website/ota/pr-<N>/<short-sha>/manifest.plist → PR build
#
# Two callers keep the index in sync:
#   1. .github/workflows/release.yml — after rsyncing a new release manifest.
#   2. build/dev-env/n8n/workflows/03-pr-build-comment.json — after publishing
#      a PR debug build.
#
# The output is written atomically (tmpfile + mv) so nginx never serves a
# half-written page under concurrent regenerations.

set -euo pipefail

OTA_ROOT="${OTA_ROOT:-/opt/onym-chat/deploy/website/ota}"
TITLE="${TITLE:-Onym}"

if [ ! -d "$OTA_ROOT" ]; then
    echo "ERROR: $OTA_ROOT does not exist" >&2
    exit 1
fi

# Releases: top-level dirs whose basename looks like X.Y.Z and which have a
# manifest.plist one level down. Sort descending by semver.
release_versions=$(
    find "$OTA_ROOT" -mindepth 2 -maxdepth 2 -name manifest.plist -type f 2>/dev/null \
        | while IFS= read -r path; do
            d=$(basename "$(dirname "$path")")
            if [[ "$d" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
                echo "$d"
            fi
        done \
        | sort -rV
)

latest_release=$(printf '%s\n' "$release_versions" | head -n1)

release_rows=""
while IFS= read -r v; do
    [ -z "$v" ] && continue
    if [ "$v" = "$latest_release" ]; then
        badge=' <span class="badge badge-latest">latest</span>'
    else
        badge=''
    fi
    release_rows+="            <li><a href=\"/ota/${v}/\">Version ${v}${badge}</a></li>"$'\n'
done <<< "$release_versions"

# PR builds: dirs of the form pr-<N>/<short-sha>/manifest.plist. Sort by the
# manifest's mtime descending so freshest is on top. Show PR number + sha.
pr_rows=""
pr_tmp=$(mktemp)
trap 'rm -f "$pr_tmp"' EXIT

find "$OTA_ROOT" -mindepth 3 -maxdepth 3 -name manifest.plist -type f 2>/dev/null \
    | while IFS= read -r path; do
        sha_dir=$(basename "$(dirname "$path")")
        pr_dir=$(basename "$(dirname "$(dirname "$path")")")
        if ! [[ "$pr_dir" =~ ^pr-[0-9]+$ ]]; then
            continue
        fi
        # stat -c on GNU (droplet), stat -f on BSD. The droplet is Ubuntu so
        # -c is safe, but keep the fallback for local dev runs on macOS.
        mtime=$(stat -c '%Y' "$path" 2>/dev/null || stat -f '%m' "$path")
        printf '%s\t%s\t%s\n' "$mtime" "$pr_dir" "$sha_dir" >> "$pr_tmp"
    done

if [ -s "$pr_tmp" ]; then
    # Newest first. awk strips the mtime field after sorting.
    sorted_prs=$(sort -rn "$pr_tmp" | awk -F'\t' '{print $2"\t"$3}')
    while IFS=$'\t' read -r pr_dir sha_dir; do
        [ -z "$pr_dir" ] && continue
        pr_num="${pr_dir#pr-}"
        pr_rows+="            <li><a href=\"/ota/${pr_dir}/${sha_dir}/\">PR #${pr_num} <code>${sha_dir}</code></a></li>"$'\n'
    done <<< "$sorted_prs"
fi

out_html="${OTA_ROOT}/index.html"
tmp_html=$(mktemp "${OTA_ROOT}/.index.XXXXXX.html")

cat > "$tmp_html" <<ROOT
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>${TITLE} OTA — all builds</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            max-width: 520px;
            margin: 0 auto;
            padding: 48px 24px;
            color: #1c1c1e;
            background: #f2f2f7;
        }
        h1 { font-size: 24px; margin: 0 0 4px; text-align: center; }
        p.sub { color: #6e6e73; font-size: 15px; text-align: center; margin: 0 0 32px; }
        h2 {
            font-size: 13px;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            color: #6e6e73;
            margin: 32px 0 8px 4px;
            font-weight: 600;
        }
        ul { list-style: none; padding: 0; margin: 0; }
        li { margin: 0 0 8px; }
        li a {
            display: block;
            background: #fff;
            color: #1c1c1e;
            text-decoration: none;
            padding: 14px 18px;
            border-radius: 12px;
            font-size: 16px;
            font-weight: 500;
            box-shadow: 0 1px 2px rgba(0,0,0,0.04);
        }
        li a:active { background: #eceef3; }
        li a code {
            font-size: 13px;
            color: #6e6e73;
            background: transparent;
            padding: 0;
        }
        .badge {
            display: inline-block;
            color: #fff;
            font-size: 11px;
            font-weight: 600;
            padding: 2px 8px;
            border-radius: 8px;
            margin-left: 8px;
            vertical-align: middle;
            text-transform: uppercase;
            letter-spacing: 0.03em;
        }
        .badge-latest { background: #6c5ce7; }
        .note {
            margin-top: 32px;
            padding: 16px;
            background: #fff;
            border-radius: 12px;
            font-size: 14px;
            color: #3c3c43;
            line-height: 1.5;
        }
        .empty {
            color: #6e6e73;
            font-style: italic;
            padding: 0 4px;
        }
    </style>
</head>
<body>
    <h1>${TITLE}</h1>
    <p class="sub">Ad-hoc iOS builds</p>

    <h2>Releases</h2>
    <ul>
ROOT

if [ -n "$release_rows" ]; then
    printf '%s' "$release_rows" >> "$tmp_html"
else
    printf '        <li class="empty">No releases yet.</li>\n' >> "$tmp_html"
fi

cat >> "$tmp_html" <<ROOT
    </ul>

    <h2>PR debug builds</h2>
    <ul>
ROOT

if [ -n "$pr_rows" ]; then
    printf '%s' "$pr_rows" >> "$tmp_html"
else
    printf '        <li class="empty">No open PR builds.</li>\n' >> "$tmp_html"
fi

cat >> "$tmp_html" <<ROOT
    </ul>

    <div class="note">
        Open this page in <strong>Safari on iOS</strong>. Installation requires the device UDID to be in the ad-hoc provisioning profile used to sign the chosen build.
    </div>
</body>
</html>
ROOT

chmod 644 "$tmp_html"
mv "$tmp_html" "$out_html"

release_count=$(printf '%s\n' "$release_versions" | grep -c . || true)
pr_count=$(wc -l < "$pr_tmp" | tr -d ' ')

echo "Regenerated ${out_html}: ${release_count} release(s), ${pr_count} PR build(s)"
