#!/usr/bin/env bash
#
# configure-ios.sh — Configure iOS app bundle ID and display name
#
# Substitutes the bundle identifier and display name across project.yml,
# entitlements, Info.plist, and Swift source files, then regenerates the
# Xcode project via xcodegen.
#
# Usage:
#   ./scripts/configure-ios.sh
#
# You will be prompted for:
#   - Bundle ID        (e.g. com.example.myapp)
#   - Display name     (e.g. MyChat)
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
IOS_DIR="$REPO_ROOT/clients/ios/StellarChat"

# ─── Colors ──────────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'

info() { echo -e "${CYAN}==> $*${NC}"; }
ok()   { echo -e "${GREEN}==> $*${NC}"; }
err()  { echo -e "${RED}==> ERROR: $*${NC}" >&2; }

# ─── Defaults ────────────────────────────────────────────────────────

OLD_BUNDLE_ID="com.stellarmls.chat"
OLD_DISPLAY_NAME="StellarChat"
OLD_URL_SCHEME="stellarchat"
OLD_APP_GROUP="group.com.stellarmls.chat"
OLD_BUNDLE_PREFIX="com.stellarmls"

# ─── Prompt ──────────────────────────────────────────────────────────

echo ""
echo "════════════════════════════════════════════════"
echo "  iOS App Configuration"
echo "════════════════════════════════════════════════"
echo ""
echo "  Current bundle ID:    $OLD_BUNDLE_ID"
echo "  Current display name: $OLD_DISPLAY_NAME"
echo ""

read -rp "New bundle ID (e.g. com.example.myapp): " NEW_BUNDLE_ID
if [ -z "$NEW_BUNDLE_ID" ]; then
    err "Bundle ID cannot be empty."
    exit 1
fi

# Validate bundle ID format
if ! echo "$NEW_BUNDLE_ID" | grep -qE '^[a-zA-Z][a-zA-Z0-9-]*(\.[a-zA-Z][a-zA-Z0-9-]*)+$'; then
    err "Invalid bundle ID format. Use reverse-domain notation (e.g. com.example.myapp)."
    exit 1
fi

read -rp "New display name (e.g. MyChat): " NEW_DISPLAY_NAME
if [ -z "$NEW_DISPLAY_NAME" ]; then
    err "Display name cannot be empty."
    exit 1
fi

# Derive related identifiers
NEW_APP_GROUP="group.${NEW_BUNDLE_ID}"
NEW_URL_SCHEME=$(echo "$NEW_DISPLAY_NAME" | tr '[:upper:]' '[:lower:]' | tr -cd '[:alnum:]')
# Bundle prefix = everything except last component
NEW_BUNDLE_PREFIX="${NEW_BUNDLE_ID%.*}"

echo ""
info "Configuration:"
echo "  Bundle ID:    $OLD_BUNDLE_ID -> $NEW_BUNDLE_ID"
echo "  Display name: $OLD_DISPLAY_NAME -> $NEW_DISPLAY_NAME"
echo "  App Group:    $OLD_APP_GROUP -> $NEW_APP_GROUP"
echo "  URL scheme:   $OLD_URL_SCHEME -> $NEW_URL_SCHEME"
echo "  Bundle prefix:$OLD_BUNDLE_PREFIX -> $NEW_BUNDLE_PREFIX"
echo ""

read -rp "Proceed? [y/N] " CONFIRM
if [ "$CONFIRM" != "y" ] && [ "$CONFIRM" != "Y" ]; then
    echo "Aborted."
    exit 0
fi

# ─── Substitutions ───────────────────────────────────────────────────

info "Updating project.yml..."
sed -i '' \
    -e "s|bundleIdPrefix: ${OLD_BUNDLE_PREFIX}|bundleIdPrefix: ${NEW_BUNDLE_PREFIX}|g" \
    -e "s|PRODUCT_BUNDLE_IDENTIFIER: ${OLD_BUNDLE_ID}|PRODUCT_BUNDLE_IDENTIFIER: ${NEW_BUNDLE_ID}|g" \
    -e "s|PRODUCT_BUNDLE_IDENTIFIER: ${OLD_BUNDLE_ID}.tests|PRODUCT_BUNDLE_IDENTIFIER: ${NEW_BUNDLE_ID}.tests|g" \
    -e "s|PRODUCT_BUNDLE_IDENTIFIER: ${OLD_BUNDLE_ID}.NotificationService|PRODUCT_BUNDLE_IDENTIFIER: ${NEW_BUNDLE_ID}.NotificationService|g" \
    -e "s|${OLD_APP_GROUP}|${NEW_APP_GROUP}|g" \
    "$IOS_DIR/project.yml"
ok "project.yml updated"

info "Updating entitlements..."
for f in "$IOS_DIR/StellarChat/StellarChat.entitlements" \
         "$IOS_DIR/StellarChatNotificationService/StellarChatNotificationService.entitlements"; do
    if [ -f "$f" ]; then
        sed -i '' \
            -e "s|${OLD_APP_GROUP}|${NEW_APP_GROUP}|g" \
            "$f"
        ok "  $(basename "$f")"
    fi
done

info "Updating Info.plist..."
sed -i '' \
    -e "s|<string>${OLD_URL_SCHEME}</string>|<string>${NEW_URL_SCHEME}</string>|g" \
    -e "s|<string>${OLD_BUNDLE_ID}</string>|<string>${NEW_BUNDLE_ID}</string>|g" \
    -e "s|${OLD_DISPLAY_NAME} needs|${NEW_DISPLAY_NAME} needs|g" \
    "$IOS_DIR/StellarChat/Info.plist"
ok "Info.plist updated"

info "Updating Swift source files..."
SWIFT_COUNT=0
while IFS= read -r -d '' file; do
    if grep -q "$OLD_BUNDLE_ID\|$OLD_APP_GROUP\|${OLD_URL_SCHEME}://" "$file" 2>/dev/null; then
        sed -i '' \
            -e "s|${OLD_BUNDLE_ID}|${NEW_BUNDLE_ID}|g" \
            -e "s|${OLD_APP_GROUP}|${NEW_APP_GROUP}|g" \
            -e "s|${OLD_URL_SCHEME}://|${NEW_URL_SCHEME}://|g" \
            "$file"
        SWIFT_COUNT=$((SWIFT_COUNT + 1))
    fi
done < <(find "$IOS_DIR/StellarChat" "$IOS_DIR/StellarChatNotificationService" -name "*.swift" -print0 2>/dev/null)
ok "$SWIFT_COUNT Swift files updated"

info "Updating PN relay default bundle ID..."
PN_ENV_EXAMPLE="$REPO_ROOT/pn-relay/.env.example"
if [ -f "$PN_ENV_EXAMPLE" ]; then
    sed -i '' "s|APNS_BUNDLE_ID=${OLD_BUNDLE_ID}|APNS_BUNDLE_ID=${NEW_BUNDLE_ID}|g" "$PN_ENV_EXAMPLE"
    ok ".env.example updated"
fi
PN_CONFIG="$REPO_ROOT/pn-relay/src/config.rs"
if [ -f "$PN_CONFIG" ]; then
    sed -i '' "s|\"${OLD_BUNDLE_ID}\"|\"${NEW_BUNDLE_ID}\"|g" "$PN_CONFIG"
    ok "pn-relay config.rs default updated"
fi

# ─── Regenerate Xcode project ───────────────────────────────────────

if command -v xcodegen &>/dev/null; then
    info "Regenerating Xcode project..."
    cd "$IOS_DIR" && xcodegen generate --quiet
    ok "Xcode project regenerated"
else
    echo ""
    echo -e "${RED}  xcodegen not found. Run 'xcodegen generate' manually in:${NC}"
    echo "  $IOS_DIR"
fi

# ─── Summary ─────────────────────────────────────────────────────────

echo ""
echo "════════════════════════════════════════════════════════════"
echo ""
echo -e "  ${GREEN}iOS app configured!${NC}"
echo ""
echo "  Bundle ID:     $NEW_BUNDLE_ID"
echo "  Display name:  $NEW_DISPLAY_NAME"
echo "  App Group:     $NEW_APP_GROUP"
echo "  URL scheme:    $NEW_URL_SCHEME"
echo ""
echo "  Don't forget to also set APNS_BUNDLE_ID=$NEW_BUNDLE_ID"
echo "  in pn-relay/.env before deploying."
echo ""
echo "════════════════════════════════════════════════════════════"
