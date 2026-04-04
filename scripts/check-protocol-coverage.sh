#!/usr/bin/env bash
# Phase 7.3: CI check — verify applyMemberChanges covers all membership-modifying message types.
#
# If a new MESSAGE_TYPE is added to GroupStateUpdate on either platform,
# this script checks that applyMemberChanges is also updated.
# Run in CI on every PR that touches GroupStateUpdate or transport files.

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Membership-modifying types: the Swift struct / Kotlin class name that MUST appear
# in applyMemberChanges on the respective platform.
# iOS uses Codable struct names (decoded with JSONDecoder).
# Android uses MESSAGE_TYPE constants from imported classes.
IOS_MEMBERSHIP_STRUCTS=(
    "SEPGroupStateUpdate"
    "SEPMemberJoined"
    "SEPRemovalNotice"
)
ANDROID_MEMBERSHIP_CLASSES=(
    "SEPGroupStateUpdate.MESSAGE_TYPE"
    "SEPMemberJoined.MESSAGE_TYPE"
    "SEPRemovalNotice.MESSAGE_TYPE"
)

errors=0

echo "=== Protocol Message Coverage Check ==="

# Check iOS applyMemberChanges
IOS_TRANSPORT="$REPO_ROOT/clients/ios/StellarChat/StellarChat/Nostr/NostrMessageTransport.swift"
if [ -f "$IOS_TRANSPORT" ]; then
    echo -e "\n${YELLOW}Checking iOS NostrMessageTransport.swift — applyMemberChanges${NC}"
    for struct_name in "${IOS_MEMBERSHIP_STRUCTS[@]}"; do
        if grep -q "$struct_name" "$IOS_TRANSPORT"; then
            echo -e "  ${GREEN}✓${NC} $struct_name handled"
        else
            echo -e "  ${RED}✗${NC} $struct_name NOT found in iOS applyMemberChanges"
            errors=$((errors + 1))
        fi
    done
else
    echo -e "${RED}iOS transport file not found${NC}"
    errors=$((errors + 1))
fi

# Check Android applyMemberChanges
ANDROID_TRANSPORT="$REPO_ROOT/clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/nostr/NostrMessageTransport.kt"
if [ -f "$ANDROID_TRANSPORT" ]; then
    echo -e "\n${YELLOW}Checking Android NostrMessageTransport.kt — applyMemberChanges${NC}"
    for class_ref in "${ANDROID_MEMBERSHIP_CLASSES[@]}"; do
        if grep -q "$class_ref" "$ANDROID_TRANSPORT"; then
            echo -e "  ${GREEN}✓${NC} $class_ref handled"
        else
            echo -e "  ${RED}✗${NC} $class_ref NOT found in Android applyMemberChanges"
            errors=$((errors + 1))
        fi
    done
else
    echo -e "${RED}Android transport file not found${NC}"
    errors=$((errors + 1))
fi

# Check Android isProtocolMessage covers all classes with MESSAGE_TYPE
# Android's isProtocolMessage uses CLASS.MESSAGE_TYPE references, not raw strings
ANDROID_SDK="$REPO_ROOT/kotlin-mls/src/main/java/com/stellarmls/mls/GroupStateUpdate.kt"
if [ -f "$ANDROID_SDK" ] && [ -f "$ANDROID_TRANSPORT" ]; then
    echo -e "\n${YELLOW}Checking Android isProtocolMessage completeness${NC}"
    # Extract class names that have MESSAGE_TYPE (e.g., SEPMemberJoined, SEPGroupStateUpdate)
    ANDROID_CLASSES=$(grep -B5 'MESSAGE_TYPE = "' "$ANDROID_SDK" | grep '^data class\|^class' | sed 's/.*class \([A-Za-z]*\).*/\1/' || true)
    while IFS= read -r cls; do
        [ -z "$cls" ] && continue
        if grep -q "${cls}.MESSAGE_TYPE" "$ANDROID_TRANSPORT"; then
            echo -e "  ${GREEN}✓${NC} ${cls}.MESSAGE_TYPE in isProtocolMessage"
        else
            echo -e "  ${YELLOW}?${NC} ${cls}.MESSAGE_TYPE not in isProtocolMessage (may be inbox-only)"
        fi
    done <<< "$ANDROID_CLASSES"
fi

# Check Swift SDK SEPMessageType registry matches all messageType constants
SWIFT_SDK="$REPO_ROOT/swift-mls/Sources/SwiftMLS/GroupStateUpdate.swift"
if [ -f "$SWIFT_SDK" ]; then
    echo -e "\n${YELLOW}Checking Swift SEPMessageType registry completeness${NC}"
    SWIFT_ALL_TYPES=$(grep -o 'messageType = "[^"]*"' "$SWIFT_SDK" | sed 's/messageType = "//;s/"//' || true)
    while IFS= read -r type; do
        [ -z "$type" ] && continue
        if grep -q "\"$type\"" "$SWIFT_SDK" | head -1 && grep -c "\"$type\"" "$SWIFT_SDK" > /dev/null 2>&1; then
            # Check it appears at least twice (once in struct, once in enum)
            count=$(grep -c "\"$type\"" "$SWIFT_SDK" 2>/dev/null || echo "0")
            if [ "$count" -ge 2 ]; then
                echo -e "  ${GREEN}✓${NC} $type in SEPMessageType registry"
            else
                echo -e "  ${YELLOW}?${NC} $type found $count time(s) — may be missing from SEPMessageType enum"
            fi
        fi
    done <<< "$SWIFT_ALL_TYPES"
fi

# Check transport routing boundaries
echo -e "\n${YELLOW}Checking transport routing boundaries${NC}"

# iOS: kind 34113 should not be used in NostrMessageTransport (except in assertions)
if grep -n "34113" "$IOS_TRANSPORT" 2>/dev/null | grep -v "assert\|//\|precondition" > /dev/null 2>&1; then
    echo -e "  ${RED}✗${NC} iOS NostrMessageTransport uses kind 34113 outside assertions"
    errors=$((errors + 1))
else
    echo -e "  ${GREEN}✓${NC} iOS NostrMessageTransport — no kind 34113 usage"
fi

# Android: kind 34113 should not be used in NostrMessageTransport (except in check/assert/comments/strings)
if grep -n "34113" "$ANDROID_TRANSPORT" 2>/dev/null | grep -v 'check\|assert\|//\|import\|".*34113\| \* ' > /dev/null 2>&1; then
    echo -e "  ${RED}✗${NC} Android NostrMessageTransport uses kind 34113 outside checks"
    errors=$((errors + 1))
else
    echo -e "  ${GREEN}✓${NC} Android NostrMessageTransport — no kind 34113 usage"
fi

echo ""
if [ $errors -gt 0 ]; then
    echo -e "${RED}FAILED: $errors issue(s) found${NC}"
    exit 1
else
    echo -e "${GREEN}PASSED: All protocol message types properly covered${NC}"
    exit 0
fi
