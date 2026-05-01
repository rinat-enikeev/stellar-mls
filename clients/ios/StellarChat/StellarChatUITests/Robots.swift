import XCTest

/// Page-object/robot helpers for the iOS solo-user-journey suite. Each robot
/// exposes a small, intention-oriented vocabulary for one screen so tests read
/// top-down and don't need to know which accessibilityIdentifier or visible
/// label drives a particular action.
///
/// All robots take the `XCUIApplication` in their constructor and are
/// stateless. They mirror the Kotlin `Robots.kt` from the Android suite.

// MARK: - GroupList (Chats tab)

final class GroupListRobot {
    private let app: XCUIApplication
    init(_ app: XCUIApplication) { self.app = app }

    @discardableResult
    func assertOnScreen() -> Self {
        // The "Chats" navigation bar is the most reliable signal that we're
        // on the GroupList screen. The custom identifier on the List
        // (`TestIDs.GroupList.screen`) doesn't always surface as a queryable
        // element — `accessibilityIdentifier` on a SwiftUI List propagates to
        // its underlying element, but XCUITest may or may not expose it as
        // a separate node depending on iOS version and List style.
        app.awaitElement(app.navigationBars["Chats"])
        return self
    }

    @discardableResult
    func assertGroupVisible(_ name: String) -> Self {
        app.awaitElement(app.staticTexts[name])
        return self
    }

    @discardableResult
    func assertGroupGone(_ name: String) -> Self {
        app.awaitGone(app.staticTexts[name])
        return self
    }

    @discardableResult
    func openGroup(_ name: String) -> Self {
        app.awaitElement(app.staticTexts[name]).tap()
        // Post-action: ChatView's nav bar shows the group name.
        app.awaitElement(app.navigationBars[name])
        return self
    }

    /// Open the toolbar plus menu and tap "Create Group". The Android suite
    /// goes through a dedicated `Add Group` confirmation dialog — iOS uses a
    /// `Menu` with `Create Group` / `Join Group` items, so this single tap
    /// replaces both Android stages. Self-validates by waiting for the
    /// CreateGroup screen's `Step 1 of 2` subtitle to appear.
    @discardableResult
    func tapCreateInPlusMenu() -> Self {
        app.awaitElement(app.buttons[TestIDs.GroupList.plusMenu]).tap()
        app.awaitElement(app.buttons["Create Group"]).tap()
        // Post-action: CreateGroup sheet's Step-1 subtitle proves the
        // navigation actually happened (the alternative is the menu staying
        // open or "Join Group" being tapped by mistake).
        app.awaitElement(app.staticTexts["Step 1 of 2"])
        return self
    }

    /// iOS does not have a long-press context menu on rows — the equivalent is
    /// a trailing-edge swipe that surfaces Pin / Delete buttons. The on-screen
    /// `Pin` toggles to `Unpin` once a group is pinned (the SwiftUI view
    /// renders one or the other based on `group.isPinned`). Each mutating
    /// method below post-validates its own effect so the test fails at the
    /// exact action that didn't take, not later.
    @discardableResult
    func swipeAndPin(_ name: String) -> Self {
        let cell = cellContaining(name)
        app.awaitElement(cell).swipeLeft()
        app.awaitElement(app.buttons["Pin"]).tap()
        // Post-action: the orange `pin.fill` indicator must appear next to
        // the group name. If it doesn't, the togglePinGroup state didn't
        // propagate to the view and we should fail here.
        app.awaitElement(app.images[TestIDs.GroupList.pinnedIndicator].firstMatch)
        return self
    }

    @discardableResult
    func swipeAndUnpin(_ name: String) -> Self {
        let cell = cellContaining(name)
        app.awaitElement(cell).swipeLeft()
        app.awaitElement(app.buttons["Unpin"]).tap()
        // Post-action: the pinned indicator must disappear.
        app.awaitGone(app.images[TestIDs.GroupList.pinnedIndicator].firstMatch)
        return self
    }

    @discardableResult
    func swipeAndDelete(_ name: String) -> Self {
        let cell = cellContaining(name)
        app.awaitElement(cell).swipeLeft()
        app.awaitElement(app.buttons["Delete"]).tap()
        // Post-action: the row's name must be gone from the list.
        app.awaitGone(app.staticTexts[name])
        return self
    }

    @discardableResult
    func assertPinnedIndicatorVisible() -> Self {
        app.awaitElement(
            app.images[TestIDs.GroupList.pinnedIndicator].firstMatch
        )
        return self
    }

    @discardableResult
    func assertPinnedIndicatorGone() -> Self {
        app.awaitGone(
            app.images[TestIDs.GroupList.pinnedIndicator].firstMatch
        )
        return self
    }

    /// Helper — locate the row that contains the given group name. On
    /// iOS 26, SwiftUI's `List` rows wrapped in `NavigationLink` are NOT
    /// surfaced as XCUIElement.Cell types (the accessibility tree exposes
    /// them as button-like containers instead), so a `cells` query returns
    /// no matches even when the rows are visible.
    ///
    /// The group name's staticText is always hit-testable, and XCUITest's
    /// swipe gestures dispatched on a child element propagate to the
    /// parent row container — so swiping on the text reveals the row's
    /// swipe-action buttons (Pin/Unpin/Delete) the same way swiping on
    /// the cell wrapper would.
    private func cellContaining(_ name: String) -> XCUIElement {
        return app.staticTexts[name].firstMatch
    }
}

// MARK: - CreateGroup

final class CreateGroupRobot {
    private let app: XCUIApplication
    init(_ app: XCUIApplication) { self.app = app }

    @discardableResult
    func assertOnIdentityStep() -> Self {
        app.awaitElement(app.staticTexts["Step 1 of 2"])
        return self
    }

    @discardableResult
    func typeGroupName(_ name: String) -> Self {
        let field = app.textFields["Group name"]
        app.awaitElement(field).tap()
        field.typeText(name)
        return self
    }

    @discardableResult
    func selectGovernance(_ label: String) -> Self {
        // The segmented Picker exposes each option as a button with the
        // option label; the strings match Android exactly:
        // Anarchy / 1v1 / Democracy / Oligarchy.
        app.awaitElement(app.buttons[label]).tap()
        return self
    }

    @discardableResult
    func tapNext() -> Self {
        app.awaitElement(app.buttons["Next"]).tap()
        // Post-action: Step 1 subtitle disappears and Step 2 appears.
        app.awaitGone(app.staticTexts["Step 1 of 2"])
        app.awaitElement(app.staticTexts["Step 2 of 2"])
        return self
    }

    @discardableResult
    func assertOnPeopleStep() -> Self {
        app.awaitElement(app.staticTexts["Step 2 of 2"])
        app.awaitElement(app.buttons["Create"])
        return self
    }

    @discardableResult
    func tapCreate() -> Self {
        app.awaitElement(app.buttons["Create"]).tap()
        return self
    }

    /// Wait for the DONE phase: the topbar `Open` button only appears once the
    /// pipeline (createGroup → addGroup → on-chain publish → invitations →
    /// DONE) completes. With production network enabled this involves a real
    /// Soroban testnet round-trip, hence `LONG_TIMEOUT`.
    @discardableResult
    func awaitDoneStage() -> Self {
        let openButton = app.buttons[TestIDs.CreateGroup.openButton]
        app.awaitElement(openButton, timeout: LONG_TIMEOUT)
        return self
    }

    @discardableResult
    func tapOpen() -> Self {
        app.awaitElement(app.buttons[TestIDs.CreateGroup.openButton]).tap()
        return self
    }

    @discardableResult
    func createAndOpen() -> Self {
        tapCreate()
        awaitDoneStage()
        tapOpen()
        return self
    }
}

// MARK: - Chat

final class ChatRobot {
    private let app: XCUIApplication
    init(_ app: XCUIApplication) { self.app = app }

    @discardableResult
    func assertOnScreen(_ groupName: String) -> Self {
        // The navigation title shows the group name; XCUITest exposes it as
        // a staticText inside the navigation bar.
        app.awaitElement(app.navigationBars[groupName])
        app.awaitElement(app.textFields[TestIDs.Chat.messageInput])
        return self
    }

    @discardableResult
    func assertMessageVisible(_ text: String) -> Self {
        app.awaitElement(app.staticTexts[text])
        return self
    }

    @discardableResult
    func typeAndSend(_ text: String) -> Self {
        let field = app.textFields[TestIDs.Chat.messageInput]
        app.awaitElement(field).tap()
        field.typeText(text)
        // The send button only renders once inputText is non-empty; voice
        // record button takes its place when the field is empty.
        app.awaitElement(app.buttons[TestIDs.Chat.send]).tap()
        // Post-action: the message text must appear in the chat (as a
        // bubble label). With production network on, this proves the
        // ViewModel committed the message; the status icon (Sending/Sent/
        // Delivered/Failed) is checked separately by the caller.
        app.awaitElement(app.staticTexts[text])
        return self
    }

    /// Soft check: at least one of {Sending, Sent, Delivered, Tap to retry}
    /// is visible on a freshly-sent "me" message. We don't insist on the
    /// failed icon never appearing because real public Nostr relays may
    /// rate-limit a freshly-keyed test client; the retry indicator is
    /// informational, not a fatal failure for the journey.
    @discardableResult
    func assertOutgoingStatusIconVisible() -> Self {
        let candidates: [XCUIElement] = [
            app.images[TestIDs.Chat.statusSending],
            app.images[TestIDs.Chat.statusSent],
            app.images[TestIDs.Chat.statusDelivered],
            app.buttons[TestIDs.Chat.statusFailed]
        ]
        app.awaitAny(candidates, timeout: LONG_TIMEOUT)
        return self
    }

    @discardableResult
    func openGroupInfo() -> Self {
        app.awaitElement(app.buttons[TestIDs.Chat.groupInfo]).tap()
        // Post-action: the GroupInfo sheet's toolbar Close button has a
        // dedicated identifier (which avoids colliding with SF-Symbol
        // `xmark` dismiss buttons in ChatView banners that iOS auto-labels
        // as "Close").
        app.awaitElement(app.buttons[TestIDs.GroupInfo.close])
        return self
    }

    @discardableResult
    func goBack() -> Self {
        app.tapBackArrow()
        // Post-action: the message input is no longer present (we left
        // the chat). Caller separately checks the destination screen's
        // own root signal.
        app.awaitGone(app.textFields[TestIDs.Chat.messageInput])
        return self
    }
}

// MARK: - Onboarding sheet

final class OnboardingRobot {
    private let app: XCUIApplication
    init(_ app: XCUIApplication) { self.app = app }

    /// Use the primary button's identifier as the sheet-visibility signal.
    /// The custom identifier on the sheet's content view doesn't always
    /// surface as a queryable element, but the button (a leaf accessibility
    /// element) is reliably exposed.
    @discardableResult
    func assertVisible() -> Self {
        app.awaitElement(app.buttons[TestIDs.Onboarding.primary])
        return self
    }

    @discardableResult
    func assertGone() -> Self {
        app.awaitGone(app.buttons[TestIDs.Onboarding.primary])
        return self
    }

    @discardableResult
    func tapPrimary() -> Self {
        // The button label cycles through "Next" (pages 1-3) and
        // "Get Started" (page 4) — using the stable identifier avoids the
        // need to switch on currentPage.
        app.awaitElement(app.buttons[TestIDs.Onboarding.primary]).tap()
        return self
    }
}

// MARK: - Settings

final class SettingsRobot {
    private let app: XCUIApplication
    init(_ app: XCUIApplication) { self.app = app }

    @discardableResult
    func assertOnScreen() -> Self {
        app.awaitElement(app.navigationBars["Settings"])
        return self
    }

    @discardableResult
    func selectInviteTab() -> Self {
        app.awaitElement(app.buttons["Invite Key"]).tap()
        // Post-action: the QR-tab signature button "Share link" is visible.
        app.awaitElement(app.buttons["Share link"])
        return self
    }

    @discardableResult
    func selectPreferencesTab() -> Self {
        app.awaitElement(app.buttons["Preferences"]).tap()
        // Post-action: the Preferences-tab signature row "Backup Recovery
        // Phrase" is visible. This confirms the segmented picker actually
        // switched tabs (and the Invite-tab content is gone).
        app.awaitElement(app.buttons["Backup Recovery Phrase"])
        return self
    }

    @discardableResult
    func assertShareLinkButtonVisible() -> Self {
        app.awaitElement(app.buttons["Share link"])
        return self
    }

    // Section headers come from `Text("Network")` etc. in source. With
    // `.listStyle(.insetGrouped)` iOS renders them in UPPERCASE visually,
    // but XCUITest may report either case depending on iOS version. Use
    // a case-insensitive match on the source label.

    @discardableResult
    func assertNetworkSectionVisible() -> Self {
        app.awaitElement(app.staticTextCaseInsensitive("Network"))
        return self
    }

    @discardableResult
    func assertProtocolSectionVisible() -> Self {
        app.awaitElement(app.staticTextCaseInsensitive("Protocol"))
        return self
    }

    @discardableResult
    func assertSecuritySectionVisible() -> Self {
        app.awaitElement(app.staticTextCaseInsensitive("Security"))
        return self
    }

    @discardableResult
    func assertAdvancedSectionVisible() -> Self {
        app.awaitElement(app.staticTextCaseInsensitive("Advanced"))
        return self
    }

    @discardableResult
    func assertAboutSectionVisible() -> Self {
        app.awaitElement(app.staticTextCaseInsensitive("About"))
        return self
    }

    // Each sub-screen tap is post-validated by waiting for the destination
    // screen's navigation title to appear. If the row didn't navigate (e.g.
    // accidentally became a different control), the test fails on the row
    // itself rather than later when the assert can't find the title.
    //
    // Three of the row labels are concatenated with their detail strings —
    // SwiftUI's NavigationLink composes the label from all visible
    // descendants, so e.g. `Relays` + count `6` surfaces as the button
    // label `"Relays, 6"`. We match by `BEGINSWITH` to tolerate the
    // dynamic suffix. `Advanced` and `Backup Recovery Phrase` have no
    // detail, so an exact-label match suffices.

    @discardableResult
    func tapRelaysRow() -> Self {
        let row = app.buttons
            .matching(NSPredicate(format: "label BEGINSWITH %@", "Relays"))
            .firstMatch
        app.awaitElement(row).tap()
        app.awaitElement(app.navigationBars["Relays"])
        return self
    }

    @discardableResult
    func tapBlossomRow() -> Self {
        let row = app.buttons
            .matching(NSPredicate(format: "label BEGINSWITH %@", "Blossom Servers"))
            .firstMatch
        app.awaitElement(row).tap()
        app.awaitElement(app.navigationBars["Blossom Servers"])
        return self
    }

    @discardableResult
    func tapStellarContractRow() -> Self {
        let row = app.buttons
            .matching(NSPredicate(format: "label BEGINSWITH %@", "Stellar Contract"))
            .firstMatch
        app.awaitElement(row).tap()
        app.awaitElement(app.navigationBars["Stellar Contract"])
        return self
    }

    @discardableResult
    func tapAdvancedRow() -> Self {
        app.awaitElement(app.buttons["Advanced"]).tap()
        app.awaitElement(app.navigationBars["Advanced"])
        return self
    }

    /// Tap the Backup Recovery Phrase row. Does NOT post-validate — the
    /// destination depends on identity state (BIP39-backed → Recovery sheet,
    /// otherwise → "Generate new identity?" alert). The caller stage_13
    /// branches on which one appears.
    @discardableResult
    func tapBackupRecoveryPhraseRow() -> Self {
        app.awaitElement(app.buttons["Backup Recovery Phrase"]).tap()
        return self
    }
}

// MARK: - Search

final class SearchRobot {
    private let app: XCUIApplication
    init(_ app: XCUIApplication) { self.app = app }

    /// On iOS 18, a `Tab` with `role: .search` activates the search field
    /// directly when selected — the "Search" navigation bar may be replaced
    /// by the search experience UI. Asserting on the searchField presence is
    /// more reliable than asserting on `navigationBars["Search"]`.
    @discardableResult
    func assertOnScreen() -> Self {
        app.awaitElement(app.searchFields.firstMatch)
        return self
    }

    @discardableResult
    func typeQuery(_ text: String) -> Self {
        // SwiftUI `.searchable(text:)` renders as a search field;
        // XCUITest surfaces it via the searchFields collection.
        let field = app.searchFields.firstMatch
        app.awaitElement(field).tap()
        field.typeText(text)
        return self
    }

    @discardableResult
    func clearQuery() -> Self {
        // The search-bar Cancel button lives next to the search field as a
        // sibling once the field has focus. Scoping to the search bar's own
        // toolbar avoids matching unrelated Cancel buttons elsewhere on
        // screen (e.g. in alerts).
        let cancelInSearchBar = app.otherElements.containing(.searchField, identifier: nil)
            .buttons["Cancel"].firstMatch
        if cancelInSearchBar.exists {
            cancelInSearchBar.tap()
            return self
        }
        // Fallback: tap the clear-text button inside the search field.
        let clearButton = app.searchFields.firstMatch.buttons["Clear text"]
        if clearButton.exists {
            clearButton.tap()
        }
        return self
    }

    @discardableResult
    func assertResultVisible(_ text: String) -> Self {
        app.awaitElement(app.staticTexts[text])
        return self
    }

    /// After clearing or before searching, the result should not be visible.
    @discardableResult
    func assertResultGone(_ text: String) -> Self {
        app.awaitGone(app.staticTexts[text])
        return self
    }
}

// MARK: - Bottom navigation (TabView)

final class BottomNavRobot {
    private let app: XCUIApplication
    init(_ app: XCUIApplication) { self.app = app }

    // Each tab tap is post-validated by waiting for the destination screen's
    // root signal. The Search tab is special on iOS 26 — `role: .search`
    // activates the search experience without a regular nav bar, so we wait
    // for the search field instead.
    //
    // **iOS 26 collapsed TabBar.** After leaving the Search tab (role:.search),
    // iOS leaves the bottom bar in a collapsed/minimized state showing only
    // the currently-active tab (`value: Collapsed` in the accessibility
    // dump). Other tabs are hidden until the bar expands. Tapping the
    // already-visible active tab is the documented gesture to restore the
    // expanded state — `ensureTabBarExpanded()` does this on demand.

    @discardableResult
    func openContacts() -> Self {
        ensureVisible("Contacts")
        app.awaitElement(app.tabBars.buttons["Contacts"]).tap()
        app.awaitElement(app.navigationBars["Contacts"])
        return self
    }

    @discardableResult
    func openChats() -> Self {
        ensureVisible("Chats")
        app.awaitElement(app.tabBars.buttons["Chats"]).tap()
        app.awaitElement(app.navigationBars["Chats"])
        return self
    }

    @discardableResult
    func openSearch() -> Self {
        ensureVisible("Search")
        app.awaitElement(app.tabBars.buttons["Search"]).tap()
        app.awaitElement(app.searchFields.firstMatch)
        return self
    }

    @discardableResult
    func openSettings() -> Self {
        ensureVisible("Settings")
        app.awaitElement(app.tabBars.buttons["Settings"]).tap()
        app.awaitElement(app.navigationBars["Settings"])
        return self
    }

    /// iOS 26 minimizes the bottom tab bar after some interactions
    /// (notably exiting a `Tab(role: .search)` via Cancel — which leaves
    /// only the active tab visible with `value: Collapsed`). To recover
    /// we cycle through escalating gestures until the target button is
    /// addressable: tap the visible button (sometimes enough), then
    /// swipe-up on the bar (the documented expand gesture), then a
    /// last-ditch coordinate tap on the bar to force-toggle.
    private func ensureVisible(_ tabLabel: String) {
        let target = app.tabBars.buttons[tabLabel]
        if target.waitForExistence(timeout: 1) { return }

        // Strategy 1: tap the single visible button. Acts as a no-op when
        // already on that tab but may be enough on some iOS variants.
        let visible = app.tabBars.buttons.firstMatch
        if visible.exists { visible.tap() }
        if target.waitForExistence(timeout: 1) { return }

        // Strategy 2: swipe up on the tab bar to expand the minimized
        // state. This is the documented user-facing gesture for restoring
        // a collapsed iOS 26 tab bar.
        let bar = app.tabBars.firstMatch
        if bar.exists { bar.swipeUp() }
        if target.waitForExistence(timeout: 1) { return }

        // Strategy 3: tap somewhere outside the tab bar (top of window)
        // so iOS reconsiders the bar's state, then swipe up again.
        app.windows.firstMatch
            .coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.1))
            .tap()
        if bar.exists { bar.swipeUp() }
    }
}
