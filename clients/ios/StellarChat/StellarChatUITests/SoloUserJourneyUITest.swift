import XCTest

/// Single-session solo-user journey for the iOS client.
///
/// The full suite of UI scenarios (onboarding, creation per governance,
/// settings, sub-screens, search, contacts, pin) lives inside ONE
/// `test_complete_solo_journey` method. The app launches once at `setUp` and
/// stays in the foreground for the whole journey — there are no intermediate
/// app restarts and no interstitials between stages. Cleanup (deleting the
/// four `… QA` groups the journey created) lives in `tearDown` so it runs
/// even when an earlier stage fails — otherwise a failure mid-journey would
/// strand the temp groups on the device.
///
/// **Production network is on.** Earlier iterations of the matching Android
/// suite cleared `relay_urls` / `endpoint` / `contract_id` / `relayer_url` to
/// make tests deterministic, with the side-effect of disabling all real I/O.
/// This iOS suite mirrors the user's preference for real coverage: groups
/// are published to Soroban testnet and messages are sent over Nostr. That
/// makes some assertions slower (`awaitDoneStage` waits up to 2 minutes)
/// and tolerant of transient relay back-pressure
/// (`assertOutgoingStatusIconVisible` accepts the failed icon as a valid
/// status, just not the absence of any status icon).
///
/// **No demo seeding.** The app launches without `--demo`, so there are no
/// `Climbing Crew` / `Family` fixtures. Every group the journey touches is
/// created fresh inside the run with a `… QA` suffix to keep it
/// distinguishable from the user's real groups (CoreData / `PersistenceStore`
/// is intentionally not wiped — this device may be a daily driver).
///
/// ## Run command
///
/// ```
/// cd clients/ios/StellarChat
/// xcodebuild test \
///     -scheme StellarChat \
///     -destination 'platform=iOS Simulator,name=iPhone 16 Pro' \
///     -only-testing:StellarChatUITests/SoloUserJourneyUITest
/// ```
///
/// Wrapper script: `scripts/run-solo-journey.sh` — runs the above, parses
/// the xcodebuild log for the Markdown report, saves it under
/// `result_autotest/`.
final class SoloUserJourneyUITest: XCTestCase {

    private var app: XCUIApplication!
    private var reporter: MarkdownReporter!

    // Robots — lazy so they can be re-instantiated if needed.
    private var onboarding: OnboardingRobot { OnboardingRobot(app) }
    private var list: GroupListRobot { GroupListRobot(app) }
    private var chat: ChatRobot { ChatRobot(app) }
    private var create: CreateGroupRobot { CreateGroupRobot(app) }
    private var nav: BottomNavRobot { BottomNavRobot(app) }
    private var search: SearchRobot { SearchRobot(app) }
    private var settings: SettingsRobot { SettingsRobot(app) }

    // Unique group names — the `QA` suffix keeps them out of any real groups
    // the user already has on the device.
    private let anarchy = "Anarchy Crew QA"
    private let oneOnOne = "Direct Bob QA"
    private let democracy = "Democracy Council QA"
    private let oligarchy = "Oligarchy Inner QA"
    private var createdGroups: [String] {
        [anarchy, oneOnOne, democracy, oligarchy]
    }

    override func setUp() {
        super.setUp()
        continueAfterFailure = false
        app = XCUIApplication()
        LaunchArgs.configure(app)
        reporter = MarkdownReporter(testName: "\(type(of: self)).test_complete_solo_journey")
        app.launch()
    }

    override func tearDown() {
        cleanupCreatedGroups()
        let passed = (testRun?.failureCount ?? 0) == 0
        if !passed, let summary = testRun?.failureCount {
            reporter.recordFailure("XCTest reported \(summary) failure(s)")
        }
        reporter.emit(passed: passed, testCase: self)
        app = nil
        super.tearDown()
    }

    // MARK: - Journey

    func test_complete_solo_journey() {
        stage_01_onboarding_sheet_visible_on_first_launch()
        stage_02_walk_through_onboarding_and_dismiss()
        stage_03_create_anarchy_and_send_message()
        stage_04_create_one_on_one_and_send_message()
        stage_05_create_democracy_and_send_message()
        stage_06_create_oligarchy_and_send_message()
        stage_07_open_group_info_from_chat_and_back()
        stage_08_pin_group_then_swipe_shows_unpin()
        // stage_09 (search) is intentionally deferred to the end — see
        // the comment block before its call below for the reasoning.
        stage_10_settings_lands_on_invite_tab_by_default()
        stage_11_settings_preferences_shows_all_sections()
        stage_12_settings_sub_screens_open_and_back()
        stage_13_settings_recovery_phrase_intro_and_cancel()
        stage_14_contacts_tab_renders()
        stage_15_returning_to_chats_keeps_all_created_groups()
        // **Search runs last.** iOS 26's `Tab(role: .search)` minimizes the
        // bottom tab bar on Cancel, leaving only the previously-active tab
        // visible as a pill (`value: Collapsed`). Subsequent tab navigation
        // (Settings, Contacts, Chats) cannot find their buttons until the
        // bar expands, and the documented user gestures to expand it
        // (tapping the pill, swiping up on the bar, scrolling the content)
        // didn't reliably restore it under XCUITest in our environment.
        // Running search at the end means we never need to leave the Search
        // tab — cleanup recovers from collapsed state via swipe-on-row,
        // which works regardless of tab-bar visual state.
        stage_09_search_finds_created_group_by_name()
        log("✓ all stages passed")
    }

    // MARK: - 01–02: onboarding
    //
    // Note: the "Restore from Recovery Phrase" link on the onboarding sheet
    // is intentionally NOT covered here. Its action both presents the restore
    // sheet AND will set `hasSeenOnboarding = true` once a successful restore
    // completes — that path is destructive (overwrites the live KeyManager
    // via `replaceKeyManager`) and outside the user's "solo, non-destructive"
    // scope.

    private func stage_01_onboarding_sheet_visible_on_first_launch() {
        log("stage_01 onboarding sheet visible")
        onboarding.assertVisible()
        // Substring check on the page-1 copy to avoid coupling to the exact
        // line break in the title.
        let firstPageMarker = app.staticTexts
            .matching(NSPredicate(format: "label CONTAINS 'metadata are encrypted'"))
            .firstMatch
        app.awaitElement(firstPageMarker)
    }

    private func stage_02_walk_through_onboarding_and_dismiss() {
        log("stage_02 walk through onboarding pages and dismiss")
        // Each page transition is post-validated by:
        //   (a) the previous page's marker text disappearing, and
        //   (b) the new page's marker text appearing.
        // Together they prove the TabView page index advanced — checking
        // only (b) would let a stale page slip through if the new copy
        // happened to be visible already.
        let page1 = app.staticTexts.matching(NSPredicate(format: "label CONTAINS 'metadata are encrypted'")).firstMatch
        let page2 = app.staticTexts.matching(NSPredicate(format: "label CONTAINS 'Private by design'")).firstMatch
        let page3 = app.staticTexts.matching(NSPredicate(format: "label CONTAINS 'Truly shared ownership'")).firstMatch
        // Page 1 → 2
        onboarding.tapPrimary()
        app.awaitGone(page1)
        app.awaitElement(page2)
        // Page 2 → 3
        onboarding.tapPrimary()
        app.awaitGone(page2)
        app.awaitElement(page3)
        // Page 3 → 4 (final differentiator screen with Get Started CTA)
        onboarding.tapPrimary()
        app.awaitGone(page3)
        app.awaitElement(app.staticTexts["What makes this different"])
        app.awaitElement(app.buttons["Get Started"])
        // Get Started → sheet dismisses, GroupList is the active screen.
        onboarding.tapPrimary()
        onboarding.assertGone()
        list.assertOnScreen()
    }

    // MARK: - 03–06: create one group of each governance and send a message

    private func stage_03_create_anarchy_and_send_message() {
        log("stage_03 create anarchy + send")
        list.tapCreateInPlusMenu()
        create
            .assertOnIdentityStep()
            .typeGroupName(anarchy)
            .selectGovernance("Anarchy")
            .tapNext()
            .assertOnPeopleStep()
            .createAndOpen()
        chat.assertOnScreen(anarchy)
            .typeAndSend("Hello anarchy")
            .assertMessageVisible("Hello anarchy")
            .assertOutgoingStatusIconVisible()
            .goBack()
        list.assertOnScreen().assertGroupVisible(anarchy)
    }

    private func stage_04_create_one_on_one_and_send_message() {
        log("stage_04 create 1v1 + send")
        list.tapCreateInPlusMenu()
        create
            .assertOnIdentityStep()
            .typeGroupName(oneOnOne)
            .selectGovernance("1v1")
            .tapNext()
            .assertOnPeopleStep()
            .createAndOpen()
        chat.assertOnScreen(oneOnOne)
            .typeAndSend("Hi Bob")
            .assertMessageVisible("Hi Bob")
            .assertOutgoingStatusIconVisible()
            .goBack()
        list.assertOnScreen().assertGroupVisible(oneOnOne)
    }

    private func stage_05_create_democracy_and_send_message() {
        log("stage_05 create democracy + send")
        list.tapCreateInPlusMenu()
        create
            .assertOnIdentityStep()
            .typeGroupName(democracy)
            .selectGovernance("Democracy")
            .tapNext()
            .assertOnPeopleStep()
            .createAndOpen()
        chat.assertOnScreen(democracy)
            .typeAndSend("Vote on motion")
            .assertMessageVisible("Vote on motion")
            .assertOutgoingStatusIconVisible()
            .goBack()
        list.assertOnScreen().assertGroupVisible(democracy)
    }

    private func stage_06_create_oligarchy_and_send_message() {
        log("stage_06 create oligarchy + send")
        list.tapCreateInPlusMenu()
        create
            .assertOnIdentityStep()
            .typeGroupName(oligarchy)
            .selectGovernance("Oligarchy")
            .tapNext()
            .assertOnPeopleStep()
            .createAndOpen()
        chat.assertOnScreen(oligarchy)
            .typeAndSend("Quorum check")
            .assertMessageVisible("Quorum check")
            .assertOutgoingStatusIconVisible()
            .goBack()
        list.assertOnScreen().assertGroupVisible(oligarchy)
    }

    // MARK: - 07: GroupInfo open + back

    private func stage_07_open_group_info_from_chat_and_back() {
        log("stage_07 group info open + back")
        list.openGroup(anarchy)
        chat.assertOnScreen(anarchy).openGroupInfo()
        // Hero shows group name + "End-to-end encrypted" chip + "MEMBERS"
        // section header — matches Android exactly. The MEMBERS header is
        // rendered uppercase by the inset-grouped list style; case-insensitive
        // match handles both `Members` (source) and `MEMBERS` (rendered).
        let membersHeader = app.staticTextCaseInsensitive("Members")
        app.awaitElement(app.staticTexts[anarchy])
        app.awaitElement(app.staticTexts["End-to-end encrypted"])
        app.awaitElement(membersHeader)
        // GroupInfo is presented as a sheet on iOS (Android uses a
        // NavigationLink). The sheet has a "Close" toolbar button instead of
        // a back arrow; we tap by identifier to avoid collisions with banner
        // dismiss buttons in the underlying ChatView. After Close, the
        // MEMBERS header must be gone — that proves the sheet actually
        // dismissed and we're back on Chat.
        app.awaitElement(app.buttons[TestIDs.GroupInfo.close]).tap()
        app.awaitGone(membersHeader)
        chat.assertOnScreen(anarchy).goBack()
        list.assertOnScreen()
    }

    // MARK: - 08: pin / unpin via swipe action
    //
    // Android uses a long-press context menu with Pin / Unpin / Delete items.
    // iOS doesn't have a context menu on rows — equivalent functionality is
    // a trailing-edge swipe that surfaces the same actions as buttons.

    private func stage_08_pin_group_then_swipe_shows_unpin() {
        log("stage_08 pin → swipe shows Unpin")
        // Pre-check: make sure no stale pinned indicator is hanging around
        // from a previous run (paranoid — should never happen on a fresh
        // test, but a leftover pin would silently make the post-pin assert
        // pass against the wrong row).
        list.assertPinnedIndicatorGone()
        // swipeAndPin self-validates the pin-indicator appears.
        list.swipeAndPin(anarchy)
        list.assertOnScreen().assertGroupVisible(anarchy)
        // swipeAndUnpin self-validates the pin-indicator disappears.
        list.swipeAndUnpin(anarchy)
        // Restore default (unpinned) state — confirm the row is still in
        // the list, just without the pin badge.
        list.assertOnScreen().assertGroupVisible(anarchy).assertPinnedIndicatorGone()
    }

    // MARK: - 09: search

    private func stage_09_search_finds_created_group_by_name() {
        log("stage_09 search by name")
        nav.openSearch()
        // Pre-check: empty-state placeholder is visible before we type.
        // The test for "result appears" is only meaningful if the result
        // wasn't already on screen.
        search.assertResultGone(anarchy)
        search
            .assertOnScreen()
            .typeQuery("Anarchy Crew")
            .assertResultVisible(anarchy)
            .clearQuery()
        // Post-action: after Cancel/clear, the result must disappear from
        // the list — proves the search bar's clear actually filtered the
        // results, not just dismissed itself.
        search.assertResultGone(anarchy)
    }

    // MARK: - 10–13: settings

    private func stage_10_settings_lands_on_invite_tab_by_default() {
        log("stage_10 settings invite tab default")
        nav.openSettings()
        settings
            .assertOnScreen()
            .assertShareLinkButtonVisible()
    }

    private func stage_11_settings_preferences_shows_all_sections() {
        log("stage_11 settings preferences sections")
        settings
            .selectPreferencesTab()
            .assertNetworkSectionVisible()
            .assertProtocolSectionVisible()
            .assertSecuritySectionVisible()
            .assertAdvancedSectionVisible()
            .assertAboutSectionVisible()
    }

    private func stage_12_settings_sub_screens_open_and_back() {
        log("stage_12 sub-screens open + back ×4")
        // Each sub-screen Robot method already post-validates the destination
        // nav title; here we additionally verify Back returned us to Settings
        // AND that the sub-screen's nav title is gone (so we're really on
        // Settings, not stranded on an intermediate state).
        settings.tapRelaysRow()
        app.tapBackArrow()
        app.awaitGone(app.navigationBars["Relays"])
        settings.assertOnScreen()
        // Preferences tab must still be active (rememberSaveable parity).
        app.awaitElement(app.buttons["Backup Recovery Phrase"])

        settings.tapBlossomRow()
        app.tapBackArrow()
        app.awaitGone(app.navigationBars["Blossom Servers"])
        settings.assertOnScreen()
        app.awaitElement(app.buttons["Backup Recovery Phrase"])

        settings.tapStellarContractRow()
        app.tapBackArrow()
        app.awaitGone(app.navigationBars["Stellar Contract"])
        settings.assertOnScreen()
        app.awaitElement(app.buttons["Backup Recovery Phrase"])

        settings.tapAdvancedRow()
        app.tapBackArrow()
        app.awaitGone(app.navigationBars["Advanced"])
        settings.assertOnScreen()
        app.awaitElement(app.buttons["Backup Recovery Phrase"])
    }

    private func stage_13_settings_recovery_phrase_intro_and_cancel() {
        log("stage_13 recovery phrase intro + cancel")
        settings.selectPreferencesTab().tapBackupRecoveryPhraseRow()
        // Two possible outcomes depending on whether the user's identity is
        // BIP39-backed (see SettingsView line 235-247):
        //   1. RecoveryPhraseView sheet → nav title "Back up keys" → Cancel
        //   2. "Generate new identity?" alert → Cancel to dismiss
        //
        // The smoke target is path 1 (fresh / recent identity), but path 2 is
        // a real possibility on a long-lived daily-driver device. Both paths
        // satisfy the smoke goal of "the row tapped → returned to Settings".
        let introBar = app.navigationBars["Back up keys"]
        let regenAlert = app.alerts["Generate new identity?"]
        if introBar.waitForExistence(timeout: 5) {
            // The toolbar Cancel button dismisses the sheet without invoking
            // LAContext — biometric prompts cannot be driven from XCUITest.
            introBar.buttons["Cancel"].tap()
            // Post-action: intro nav bar is gone (sheet dismissed).
            app.awaitGone(introBar)
        } else if regenAlert.waitForExistence(timeout: 1) {
            log("stage_13 identity not BIP39-backed — dismissed regenerate alert")
            regenAlert.buttons["Cancel"].tap()
            app.awaitGone(regenAlert)
        } else {
            XCTFail("Backup Recovery Phrase tap opened neither the wizard nor the regenerate alert")
        }
        settings.assertOnScreen()
    }

    // MARK: - 14–15: bottom-nav tour

    private func stage_14_contacts_tab_renders() {
        log("stage_14 contacts tab renders")
        nav.openContacts()
        // ContactsView's navigation title is "Contacts" — assert the nav bar
        // exists rather than tapping the system contacts-permission prompt.
        app.awaitElement(app.navigationBars["Contacts"])
    }

    private func stage_15_returning_to_chats_keeps_all_created_groups() {
        log("stage_15 back to chats keeps all 4 groups")
        nav.openChats()
        list.assertOnScreen()
        for name in createdGroups {
            list.assertGroupVisible(name)
        }
    }

    // MARK: - Cleanup
    //
    // Cleanup runs unconditionally — even if any earlier stage failed — so
    // the device returns to its prior state (the journey created `… QA`
    // groups, and this is the user's daily-driver phone). After a failure
    // we don't know which screen we're on, so we try to dismiss any
    // open sheet/sub-screen first, then fall through to the Chats tab.

    private func cleanupCreatedGroups() {
        log("tearDown cleanup → delete created groups")
        // Dismiss any open sheet/modal so we land on the GroupList. Up to 3
        // attempts: each iteration looks for the GroupInfo sheet (by its
        // explicit Close-button identifier, since label "Close" collides
        // with banner dismiss buttons in chat), an alert with Cancel, or
        // the back arrow.
        for _ in 0..<3 {
            if app.buttons[TestIDs.GroupInfo.close].exists {
                app.buttons[TestIDs.GroupInfo.close].tap()
            } else if app.alerts.element.exists,
                      app.alerts.buttons["Cancel"].exists {
                app.alerts.buttons["Cancel"].tap()
            } else if app.navigationBars.buttons.element(boundBy: 0).exists {
                // Only press back if it's not the root nav bar — heuristic:
                // root nav bars are titled "Chats" / "Contacts" / "Search" /
                // "Settings", and pressing back there is a no-op.
                let title = app.navigationBars.firstMatch.identifier
                let rootTitles = ["Chats", "Contacts", "Search", "Settings"]
                if !rootTitles.contains(title) {
                    app.navigationBars.buttons.element(boundBy: 0).tap()
                }
            }
        }
        // Make sure we're on the Chats tab. After Search, iOS 26 may leave
        // the tab bar in collapsed state with only the pill (Chats or
        // Search) visible. Tap whichever is reachable — the swipe-on-row
        // delete that follows works whether the bar is fully expanded or
        // not, as long as we end up on the GroupList content.
        if app.tabBars.buttons["Chats"].exists {
            app.tabBars.buttons["Chats"].tap()
        } else if app.tabBars.buttons.firstMatch.exists {
            // Collapsed pill might be Search/Settings/Contacts — tap it,
            // wait briefly for the bar to expand, then try Chats again.
            app.tabBars.buttons.firstMatch.tap()
            if app.tabBars.buttons["Chats"].waitForExistence(timeout: 3) {
                app.tabBars.buttons["Chats"].tap()
            }
        }
        // Each per-group delete is wrapped in an existence check — one
        // missing group (creation may have failed mid-journey) doesn't
        // strand the others.
        for name in createdGroups {
            guard app.staticTexts[name].exists else {
                log("tearDown skip \(name) (not present)")
                continue
            }
            list.swipeAndDelete(name)
            // Allow the row animation to settle before the next swipe so
            // SwiftUI doesn't dispatch the gesture to a stale cell.
            _ = app.staticTexts[name].waitForNonExistence(timeout: 5)
            log("tearDown deleted \(name)")
        }
    }

    // MARK: - Logging

    private func log(_ stage: String) {
        reporter.logStage(stage)
    }
}
