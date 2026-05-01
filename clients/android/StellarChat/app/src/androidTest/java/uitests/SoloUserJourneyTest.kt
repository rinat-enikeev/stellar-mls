package uitests

import android.util.Log
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performClick
import androidx.test.espresso.Espresso
import androidx.test.ext.junit.runners.AndroidJUnit4
import chat.onym.android.MainActivity
import chat.onym.android.ui.TestTags
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TestRule
import org.junit.runner.Description
import org.junit.runner.RunWith
import org.junit.runners.model.Statement

/**
 * Single-session solo-user journey for the Android client.
 *
 * The full suite of UI scenarios (onboarding, creation per governance,
 * settings, sub-screens, search, contacts, pin) lives inside ONE
 * `@Test` method. The Activity launches once at rule entry and stays in
 * the foreground for the whole journey — there are no intermediate
 * Activity restarts and no white screens between stages. Cleanup
 * (deleting the four `… QA` groups the journey created) lives in
 * `@After` so it runs even when an earlier stage throws — otherwise a
 * failure mid-journey would strand the temp groups on the device.
 *
 * **Production network is on.** Earlier iterations of this suite cleared
 * `relay_urls`, `endpoint`, `contract_id`, and `relayer_url` to make tests
 * deterministic, which had the side-effect of disabling all real I/O.
 * The user wants real coverage: groups are published to Soroban testnet
 * and messages are sent over Nostr. That makes some assertions slower
 * (`awaitDoneStage` waits up to 2 minutes) and tolerant of transient relay
 * back-pressure (`assertOutgoingStatusIconVisible` accepts FAILED as a valid
 * status, just not the absence of any status icon).
 *
 * **No demo seeding.** The Activity launches without `demo=true`, so there
 * are no `Climbing Crew` / `Family` fixtures. Every group the journey
 * touches is created fresh inside the run with a `… QA` suffix to keep it
 * distinguishable from the user's real groups (the Room DB is intentionally
 * not wiped — this device may be a daily driver).
 *
 * **Run command:**
 * ```
 * ./gradlew :app:connectedPlayDebugAndroidTest \
 *     -Pandroid.testInstrumentationRunnerArguments.class=uitests.SoloUserJourneyTest
 * ```
 */
@RunWith(AndroidJUnit4::class)
class SoloUserJourneyTest {

    // Outermost rule — captures stage timeline / failure / logcat into a
    // Markdown report under the app's external files dir. Must wrap the
    // compose rule so it sees teardown failures too.
    @get:Rule(order = 0)
    val reportRule = MarkdownReportRule()

    @get:Rule(order = 1)
    val prefsResetRule: TestRule = TestRule { base: Statement, _: Description ->
        object : Statement() {
            override fun evaluate() {
                clearOnboardingFlags()
                base.evaluate()
            }
        }
    }

    @get:Rule(order = 2)
    val composeTestRule = createAndroidComposeRule<MainActivity>()

    private val onboarding get() = OnboardingSheetRobot(composeTestRule)
    private val list get() = GroupListRobot(composeTestRule)
    private val chat get() = ChatRobot(composeTestRule)
    private val create get() = CreateGroupRobot(composeTestRule)
    private val nav get() = BottomNavRobot(composeTestRule)
    private val search get() = SearchRobot(composeTestRule)
    private val settings get() = SettingsRobot(composeTestRule)

    // Unique group names — the `QA` suffix keeps them out of any real groups
    // the user already has on the device.
    private val anarchy = "Anarchy Crew QA"
    private val oneOnOne = "Direct Bob QA"
    private val democracy = "Democracy Council QA"
    private val oligarchy = "Oligarchy Inner QA"
    private val createdGroups = listOf(anarchy, oneOnOne, democracy, oligarchy)

    @Test
    fun complete_solo_journey() {
        stage_01_onboarding_sheet_visible_on_first_launch()
        stage_02_walk_through_onboarding_and_dismiss()
        stage_03_create_anarchy_and_send_message()
        stage_04_create_one_on_one_and_send_message()
        stage_05_create_democracy_and_send_message()
        stage_06_create_oligarchy_and_send_message()
        stage_07_open_group_info_from_chat_and_back()
        stage_08_pin_group_then_long_press_shows_unpin()
        stage_09_search_finds_created_group_by_name()
        stage_10_settings_lands_on_invite_tab_by_default()
        stage_11_settings_preferences_shows_all_sections()
        stage_12_settings_sub_screens_open_and_back()
        stage_13_settings_recovery_phrase_intro_and_cancel()
        stage_14_contacts_tab_renders()
        stage_15_returning_to_chats_keeps_all_created_groups()
        log("✓ all stages passed")
    }

    /**
     * Cleanup runs unconditionally — even if any earlier stage threw — so the
     * device returns to its prior state (the journey created `… QA` groups,
     * and this is the user's daily-driver phone). After a failure we don't
     * know which screen we're on, so we try up to 5 back-presses to dismiss
     * any open dialog/sub-screen, stopping early once the GroupList screen
     * tag becomes visible. `pressBackUnconditionally` avoids throwing if a
     * back-press at the root would dismiss the activity; we never press back
     * once the list is already on top. Each per-group delete is wrapped in
     * `runCatching` so one stuck delete doesn't strand the others.
     */
    @After
    fun cleanupCreatedGroups() {
        log("@After cleanup → delete created groups")
        repeat(5) {
            if (groupListVisible()) return@repeat
            runCatching { Espresso.pressBackUnconditionally() }
        }
        runCatching { nav.openChats() }
        runCatching { list.assertOnScreen() }
        createdGroups.forEach { name ->
            // Group may not exist (creation failed) or already be deleted.
            val present = composeTestRule.onAllNodesWithText(name).fetchSemanticsNodes().isNotEmpty()
            if (!present) {
                log("@After skip $name (not present)")
                return@forEach
            }
            runCatching {
                list.longPressGroup(name)
                    .chooseDeleteInContextMenu()
                    .confirmDeleteAction()
                list.assertGroupGone(name)
                log("@After deleted $name")
            }.onFailure { Log.w(TAG, "@After failed to delete $name: ${it.message}") }
        }
    }

    private fun groupListVisible(): Boolean = runCatching {
        composeTestRule
            .onAllNodes(androidx.compose.ui.test.hasTestTag(TestTags.GroupList.Screen))
            .fetchSemanticsNodes()
            .isNotEmpty()
    }.getOrDefault(false)

    // ── 01–02: onboarding ───────────────────────────────────────────────
    //
    // Note: the "Restore from Recovery Phrase" flow is intentionally NOT
    // covered here. Its TextButton onClick calls both onRestore() (navigate)
    // AND onDismiss() (write has_seen_onboarding=true, hide sheet) — see
    // GroupListScreen.kt around line 649. Once you tap it, the onboarding
    // sheet is gone for the rest of the session, which would cut off all
    // subsequent onboarding-page assertions. Restore-identity also overwrites
    // the live KeyManager via replaceKeyManager(), which is destructive and
    // outside the user's "solo, non-destructive" scope.

    private fun stage_01_onboarding_sheet_visible_on_first_launch() {
        log("stage_01 onboarding sheet visible")
        onboarding.assertVisible()
        composeTestRule.awaitText("metadata are encrypted", substring = true)
    }

    private fun stage_02_walk_through_onboarding_and_dismiss() {
        log("stage_02 walk through onboarding pages and dismiss")
        // Page 1 → 2
        onboarding.tapPrimary()
        composeTestRule.awaitText("Private by design", substring = true)
        // Page 2 → 3
        onboarding.tapPrimary()
        composeTestRule.awaitText("Truly shared ownership", substring = true)
        // Page 3 → 4
        onboarding.tapPrimary()
        composeTestRule.awaitText("What makes this different")
        composeTestRule.awaitText("Get Started")
        // Get Started → dismiss
        onboarding.tapPrimary()
        composeTestRule.awaitTagGone(TestTags.GroupList.OnboardingSheet)
        list.assertOnScreen()
    }

    // ── 03–06: create one group of each governance and send a message ──

    private fun stage_03_create_anarchy_and_send_message() {
        log("stage_03 create anarchy + send")
        list.tapAddFab().assertAddDialogVisible().chooseCreateInAddDialog()
        create
            .assertOnScreen()
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

    private fun stage_04_create_one_on_one_and_send_message() {
        log("stage_04 create 1v1 + send")
        list.tapAddFab().chooseCreateInAddDialog()
        create
            .assertOnScreen()
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

    private fun stage_05_create_democracy_and_send_message() {
        log("stage_05 create democracy + send")
        list.tapAddFab().chooseCreateInAddDialog()
        create
            .assertOnScreen()
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

    private fun stage_06_create_oligarchy_and_send_message() {
        log("stage_06 create oligarchy + send")
        list.tapAddFab().chooseCreateInAddDialog()
        create
            .assertOnScreen()
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

    // ── 07: GroupInfoScreen open + back ─────────────────────────────────

    private fun stage_07_open_group_info_from_chat_and_back() {
        log("stage_07 group info open + back")
        list.openGroup(anarchy)
        chat.assertOnScreen(anarchy).openGroupInfo()
        // Hero shows the group name + "End-to-end encrypted" chip.
        composeTestRule.awaitText(anarchy)
        composeTestRule.awaitText("End-to-end encrypted")
        composeTestRule.awaitText("MEMBERS")
        composeTestRule.tapBackArrow()
        chat.assertOnScreen(anarchy).goBack()
        list.assertOnScreen()
    }

    // ── 08: pin / unpin via context menu ───────────────────────────────

    private fun stage_08_pin_group_then_long_press_shows_unpin() {
        log("stage_08 pin → context menu shows Unpin")
        list.longPressGroup(anarchy)
        composeTestRule.awaitTag(TestTags.GroupList.ContextMenu)
        list.choosePinInContextMenu()
        // togglePinGroup re-sorts the list (pinned floats to top) AND closes
        // the dialog. Wait for the dialog tag to be gone before the next
        // long-press — otherwise the gesture lands on the still-open
        // AlertDialog scrim and the context menu never reopens.
        composeTestRule.awaitTagGone(TestTags.GroupList.ContextMenu)
        list.assertOnScreen()
        list.longPressGroup(anarchy)
        composeTestRule.awaitTag(TestTags.GroupList.ContextMenu)
        list.chooseUnpinInContextMenu()
        composeTestRule.awaitTagGone(TestTags.GroupList.ContextMenu)
        // Restore default (unpinned) state for the rest of the journey.
        list.assertOnScreen().assertGroupVisible(anarchy)
    }

    // ── 09: search ─────────────────────────────────────────────────────

    private fun stage_09_search_finds_created_group_by_name() {
        log("stage_09 search by name")
        nav.openSearch()
        search
            .assertOnScreen()
            .typeQuery("Anarchy Crew")
            .assertResultVisible(anarchy)
            .clearQuery()
    }

    // ── 10–13: settings ────────────────────────────────────────────────

    private fun stage_10_settings_lands_on_invite_tab_by_default() {
        log("stage_10 settings invite tab default")
        nav.openSettings()
        settings
            .assertOnScreen()
            .assertShareLinkButtonVisible()
    }

    private fun stage_11_settings_preferences_shows_all_sections() {
        log("stage_11 settings preferences sections")
        settings
            .selectPreferencesTab()
            .assertNetworkSectionVisible()
            .assertProtocolSectionVisible()
            .assertSecuritySectionVisible()
            .assertAdvancedSectionVisible()
            .assertAboutSectionVisible()
    }

    private fun stage_12_settings_sub_screens_open_and_back() {
        log("stage_12 sub-screens open + back ×4")
        // Relays
        settings.tapRelaysRow()
        composeTestRule.awaitText("ADD RELAY")
        composeTestRule.tapBackArrow()
        settings.assertOnScreen()
        // Blossom
        settings.tapBlossomRow()
        composeTestRule.awaitText("ADD SERVER")
        composeTestRule.tapBackArrow()
        settings.assertOnScreen()
        // Stellar Contract — anchor on a header unique to that screen
        settings.tapStellarContractRow()
        composeTestRule.awaitText("RELAYER (OPTIONAL)")
        composeTestRule.tapBackArrow()
        settings.assertOnScreen()
        // Advanced
        settings.tapAdvancedRow()
        composeTestRule.awaitText("NOSTR IDENTITY")
        composeTestRule.tapBackArrow()
        settings.assertOnScreen()
    }

    private fun stage_13_settings_recovery_phrase_intro_and_cancel() {
        log("stage_13 recovery phrase intro + cancel")
        settings.selectPreferencesTab().tapBackupRecoveryPhraseRow()
        // Intro step's TopAppBar title is "Back up keys".
        composeTestRule.awaitText("Back up keys")
        // Intro step nav icon is a TextButton labelled "Cancel".
        composeTestRule.onNode(androidx.compose.ui.test.hasText("Cancel")).performClick()
        settings.assertOnScreen()
    }

    // ── 14–15: bottom-nav tour ─────────────────────────────────────────

    private fun stage_14_contacts_tab_renders() {
        log("stage_14 contacts tab renders")
        nav.openContacts()
        composeTestRule.awaitTag(TestTags.Contacts.Screen)
    }

    private fun stage_15_returning_to_chats_keeps_all_created_groups() {
        log("stage_15 back to chats keeps all 4 groups")
        nav.openChats()
        list.assertOnScreen()
        createdGroups.forEach { list.assertGroupVisible(it) }
    }

    // Cleanup of created groups happens unconditionally in `@After`
    // (`cleanupCreatedGroups`), not in the journey body — so groups still get
    // removed when an earlier stage throws.

    private fun log(stage: String) {
        Log.i(TAG, "▶︎ $stage")
        reportRule.logStage(stage)
    }

    companion object {
        private const val TAG = "SoloUserJourneyTest"
    }
}
