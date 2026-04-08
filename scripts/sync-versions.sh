#!/bin/bash
# Synchronize version across Android, iOS, and AltStore source.
#
# Usage:
#   ./scripts/sync-versions.sh <version> [--build-number <n>]
#
# Examples:
#   ./scripts/sync-versions.sh 1.1.0           # auto-increments build number
#   ./scripts/sync-versions.sh 1.1.0 --build-number 15
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GRADLE="$REPO_ROOT/clients/android/StellarChat/app/build.gradle.kts"
PROJECT_YML="$REPO_ROOT/clients/ios/StellarChat/project.yml"
ALTSTORE="$REPO_ROOT/deploy/website/altstore-source.json"

VERSION=""
BUILD_NUMBER=""

while [ $# -gt 0 ]; do
    case "$1" in
        --build-number) BUILD_NUMBER="$2"; shift 2 ;;
        -*) echo "Usage: $0 <version> [--build-number <n>]" >&2; exit 1 ;;
        *) VERSION="$1"; shift ;;
    esac
done

if [ -z "$VERSION" ]; then
    echo "Usage: $0 <version> [--build-number <n>]" >&2
    exit 1
fi

# Auto-increment build number if not provided
if [ -z "$BUILD_NUMBER" ]; then
    CURRENT=$(grep 'versionCode' "$GRADLE" | head -1 | sed 's/[^0-9]//g')
    BUILD_NUMBER=$((CURRENT + 1))
fi

echo "Version: $VERSION"
echo "Build number: $BUILD_NUMBER"
echo

# 1. Android — build.gradle.kts
sed -i '' "s/versionCode = [0-9]*/versionCode = $BUILD_NUMBER/" "$GRADLE"
sed -i '' "s/versionName = \"[^\"]*\"/versionName = \"$VERSION\"/" "$GRADLE"
echo "Updated $GRADLE"

# 2. iOS — project.yml (both targets)
python3 - "$PROJECT_YML" "$VERSION" "$BUILD_NUMBER" <<'PY'
import pathlib, re, sys

path = pathlib.Path(sys.argv[1])
version, build = sys.argv[2], sys.argv[3]
text = path.read_text()

text = re.sub(
    r'(MARKETING_VERSION:\s*")[^"]*(")',
    rf'\g<1>{version}\2',
    text,
)
text = re.sub(
    r'(MARKETING_VERSION:\s*)([0-9][^\n]*)',
    rf'\g<1>"{version}"',
    text,
)
text = re.sub(
    r'(CURRENT_PROJECT_VERSION:\s*)\d+',
    rf'\g<1>{build}',
    text,
)
path.write_text(text)
PY
echo "Updated $PROJECT_YML"

# 3. AltStore — altstore-source.json (prepend version entry)
python3 - "$ALTSTORE" "$VERSION" <<'PY'
import json, sys
from datetime import date

path, version = sys.argv[1], sys.argv[2]
with open(path) as f:
    source = json.load(f)

versions = source["apps"][0]["versions"]
# Remove existing entry for this version if present
versions = [v for v in versions if v.get("version") != version]
# Prepend new entry (downloadURL and size filled by CI)
versions.insert(0, {
    "version": version,
    "date": date.today().isoformat(),
    "localizedDescription": f"Release {version}",
    "downloadURL": "",
    "size": 0,
})
source["apps"][0]["versions"] = versions

with open(path, "w") as f:
    json.dump(source, f, indent=2)
    f.write("\n")
PY
echo "Updated $ALTSTORE"

echo
echo "All files updated to version $VERSION (build $BUILD_NUMBER)."
echo "Next steps: git add, commit, tag v$VERSION, push."
