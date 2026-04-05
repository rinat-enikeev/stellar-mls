#!/usr/bin/env bash
set -euo pipefail

# Configure Android package name and display name for StellarChat.
# Renames package references, moves source directories, and updates build files.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$SCRIPT_DIR/StellarChat"

OLD_PACKAGE="com.stellarmls.chat"
OLD_PACKAGE_PATH="com/stellarmls/chat"

echo "=== StellarChat Android Configuration ==="
echo ""
read -rp "New package name [chat.onym.android]: " NEW_PACKAGE
NEW_PACKAGE="${NEW_PACKAGE:-chat.onym.android}"

read -rp "App display name [StellarChat]: " DISPLAY_NAME
DISPLAY_NAME="${DISPLAY_NAME:-StellarChat}"

NEW_PACKAGE_PATH="$(echo "$NEW_PACKAGE" | tr '.' '/')"

echo ""
echo "Package:      $OLD_PACKAGE → $NEW_PACKAGE"
echo "Display name: $DISPLAY_NAME"
echo "Source path:   $OLD_PACKAGE_PATH → $NEW_PACKAGE_PATH"
echo ""
read -rp "Proceed? [Y/n] " confirm
if [[ "${confirm:-Y}" =~ ^[Nn] ]]; then
    echo "Aborted."
    exit 0
fi

# 1. Update build.gradle.kts (namespace + applicationId)
echo "→ Updating app/build.gradle.kts..."
sed -i '' "s|namespace = \"$OLD_PACKAGE\"|namespace = \"$NEW_PACKAGE\"|g" "$PROJECT_DIR/app/build.gradle.kts"
sed -i '' "s|applicationId = \"$OLD_PACKAGE\"|applicationId = \"$NEW_PACKAGE\"|g" "$PROJECT_DIR/app/build.gradle.kts"

# 2. Update AndroidManifest.xml (app label)
echo "→ Updating AndroidManifest.xml..."
sed -i '' "s|android:label=\"[^\"]*\"|android:label=\"$DISPLAY_NAME\"|g" "$PROJECT_DIR/app/src/main/AndroidManifest.xml"

# 3. Update all package declarations and imports in Kotlin source files
echo "→ Updating package declarations and imports..."
find "$PROJECT_DIR/app/src" -name "*.kt" -o -name "*.kt.disabled" | while read -r file; do
    sed -i '' "s|package $OLD_PACKAGE|package $NEW_PACKAGE|g" "$file"
    sed -i '' "s|import $OLD_PACKAGE|import $NEW_PACKAGE|g" "$file"
    # Fully qualified references (e.g., com.stellarmls.chat.BuildConfig)
    sed -i '' "s|$OLD_PACKAGE\.|$NEW_PACKAGE.|g" "$file"
done

# 4. Update XML files that may reference the old package
find "$PROJECT_DIR/app/src" -name "*.xml" | while read -r file; do
    sed -i '' "s|$OLD_PACKAGE|$NEW_PACKAGE|g" "$file"
done

# 5. Move main source directories
echo "→ Moving main source tree..."
MAIN_SRC="$PROJECT_DIR/app/src/main/java"
if [ -d "$MAIN_SRC/$OLD_PACKAGE_PATH" ]; then
    mkdir -p "$MAIN_SRC/$NEW_PACKAGE_PATH"
    # Move all contents (files and subdirectories)
    for item in "$MAIN_SRC/$OLD_PACKAGE_PATH"/*; do
        [ -e "$item" ] && mv "$item" "$MAIN_SRC/$NEW_PACKAGE_PATH/"
    done
    # Remove old empty directory tree
    rm -rf "$MAIN_SRC/com/stellarmls"
    # Remove com/ if empty
    rmdir "$MAIN_SRC/com" 2>/dev/null || true
fi

# 6. Move androidTest source directories
echo "→ Moving androidTest source tree..."
TEST_SRC="$PROJECT_DIR/app/src/androidTest/java"
if [ -d "$TEST_SRC/$OLD_PACKAGE_PATH" ]; then
    mkdir -p "$TEST_SRC/$NEW_PACKAGE_PATH"
    for item in "$TEST_SRC/$OLD_PACKAGE_PATH"/*; do
        [ -e "$item" ] && mv "$item" "$TEST_SRC/$NEW_PACKAGE_PATH/"
    done
    rm -rf "$TEST_SRC/com/stellarmls"
    rmdir "$TEST_SRC/com" 2>/dev/null || true
fi

# 7. Update pn-relay default bundle ID for Android (if applicable)
PN_RELAY_CONFIG="$SCRIPT_DIR/../../pn-relay/src/config.rs"
if [ -f "$PN_RELAY_CONFIG" ]; then
    echo "→ Updating pn-relay Android defaults..."
    sed -i '' "s|$OLD_PACKAGE|$NEW_PACKAGE|g" "$PN_RELAY_CONFIG"
fi

# 8. Clean IDE caches
echo "→ Cleaning IDE caches..."
rm -rf "$PROJECT_DIR/.idea/caches" 2>/dev/null || true
rm -f "$PROJECT_DIR/.idea/workspace.xml" 2>/dev/null || true

echo ""
echo "Done! Package renamed to: $NEW_PACKAGE"
echo ""
echo "Next steps:"
echo "  1. Open project in Android Studio and let it sync Gradle"
echo "  2. If using Firebase, update package name in Firebase Console"
echo "  3. Download new google-services.json with the updated package name"
