package uitests

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.performTouchInput
import chat.onym.android.ui.TestTags

/**
 * Page-object/robot helpers for the solo-user journey suite.
 *
 * Each robot exposes a small, intention-oriented vocabulary for one screen so
 * tests read top-down and don't need to know which testTag or text label
 * drives a particular action. Robots are stateless — each method takes the
 * [ComposeTestRule] in via the constructor.
 *
 * This file is the *only* robot definition in the test suite (the older
 * per-class test files and their robots have been folded into one).
 */

// ── GroupListScreen ────────────────────────────────────────────────────

class GroupListRobot(private val rule: ComposeTestRule) {

    fun assertOnScreen() = apply {
        rule.awaitTag(TestTags.GroupList.Screen).assertIsDisplayed()
    }

    fun assertGroupVisible(name: String) = apply {
        rule.awaitText(name)
    }

    fun assertGroupGone(name: String) = apply {
        rule.awaitTextGone(name)
    }

    fun openGroup(name: String) = apply {
        rule.awaitText(name).performClick()
    }

    fun longPressGroup(name: String) = apply {
        rule.awaitText(name).performTouchInput { longClick() }
    }

    fun tapAddFab() = apply {
        rule.awaitTag(TestTags.GroupList.Fab).performClick()
    }

    fun assertAddDialogVisible() = apply {
        rule.awaitText("Add Group")
        rule.onNodeWithText("Create a new group or join an existing one?").assertIsDisplayed()
    }

    fun chooseCreateInAddDialog() = apply {
        rule.onNodeWithText("Create").performClick()
    }

    // Pin and Unpin share one testTag — only one of the two labels is rendered
    // at a time depending on group.isPinned, so a single tag is unambiguous and
    // avoids text-matching against the dialog title (which is the group name).
    fun choosePinInContextMenu() = apply {
        rule.awaitTag(TestTags.GroupList.ContextMenuPin).performClick()
    }

    fun chooseUnpinInContextMenu() = apply {
        rule.awaitTag(TestTags.GroupList.ContextMenuPin).performClick()
    }

    fun chooseDeleteInContextMenu() = apply {
        rule.awaitTag(TestTags.GroupList.ContextMenuDelete).performClick()
    }

    fun chooseCancelInContextMenu() = apply {
        rule.awaitTag(TestTags.GroupList.ContextMenuCancel).performClick()
    }

    /** Confirms the destructive Delete action in the confirm-delete AlertDialog. */
    fun confirmDeleteAction() = apply {
        rule.awaitTag(TestTags.GroupList.DeleteDialogConfirm).performClick()
    }
}

// ── ChatScreen ────────────────────────────────────────────────────────

class ChatRobot(private val rule: ComposeTestRule) {

    fun assertOnScreen(groupName: String) = apply {
        rule.awaitTag(TestTags.Chat.Screen).assertIsDisplayed()
        rule.onNodeWithText(groupName).assertIsDisplayed()
    }

    fun assertMessageVisible(text: String) = apply {
        rule.awaitText(text)
    }

    fun typeAndSend(text: String) = apply {
        rule.awaitTag(TestTags.Chat.MessageInput).performTextInput(text)
        rule.awaitTag(TestTags.Chat.SendButton).performClick()
    }

    fun goBack() = apply {
        rule.awaitTag(TestTags.Chat.Back).performClick()
    }

    fun openGroupInfo() = apply {
        rule.awaitTag(TestTags.Chat.GroupInfo).performClick()
    }

    /**
     * Soft check: at least one of {Sending, Sent, Delivered} is visible on
     * a freshly-sent "me" message. We don't insist on FAILED never appearing
     * because real public Nostr relays may rate-limit a freshly-keyed test
     * client; the "Tap to retry" indicator is informational, not a fatal
     * failure for the journey.
     */
    fun assertOutgoingStatusIconVisible() = apply {
        rule.waitUntil(LONG_TIMEOUT_MS) {
            val sending = rule.onAllNodesWithContentDescription("Sending").fetchSemanticsNodes()
            val sent = rule.onAllNodesWithContentDescription("Sent").fetchSemanticsNodes()
            val delivered = rule.onAllNodesWithContentDescription("Delivered").fetchSemanticsNodes()
            val failed = rule.onAllNodesWithContentDescription("Tap to retry").fetchSemanticsNodes()
            sending.isNotEmpty() || sent.isNotEmpty() || delivered.isNotEmpty() || failed.isNotEmpty()
        }
    }
}

// ── CreateGroupScreen ─────────────────────────────────────────────────

class CreateGroupRobot(private val rule: ComposeTestRule) {

    fun assertOnScreen() = apply {
        rule.awaitTag(TestTags.CreateGroup.Screen).assertIsDisplayed()
        rule.onNodeWithText("New Group").assertIsDisplayed()
        rule.onNodeWithText("Step 1 of 2").assertIsDisplayed()
    }

    fun typeGroupName(name: String) = apply {
        rule.awaitTag(TestTags.CreateGroup.NameField).performTextInput(name)
    }

    fun selectGovernance(label: String) = apply {
        rule.onNodeWithText(label).performScrollTo().performClick()
    }

    fun tapNext() = apply {
        rule.awaitTag(TestTags.CreateGroup.NextButton).performClick()
    }

    fun assertOnPeopleStep() = apply {
        rule.awaitText("Step 2 of 2")
        rule.awaitTag(TestTags.CreateGroup.CreateButton).assertIsDisplayed()
    }

    fun tapCreate() = apply {
        rule.awaitTag(TestTags.CreateGroup.CreateButton).performClick()
    }

    /**
     * Wait for the DONE phase: the topbar action becomes "Open" once the
     * pipeline (createGroup → addGroup → on-chain publish → invitations →
     * DONE) completes. With the production network enabled this involves a
     * real Soroban testnet round-trip, hence the long timeout.
     */
    fun awaitDoneStage() = apply {
        rule.awaitTextLong("Open")
    }

    fun tapOpen() = apply {
        rule.onNodeWithText("Open").performClick()
    }

    fun createAndOpen() = apply {
        tapCreate()
        awaitDoneStage()
        tapOpen()
    }
}

// ── SettingsScreen ────────────────────────────────────────────────────

class SettingsRobot(private val rule: ComposeTestRule) {

    fun assertOnScreen() = apply {
        rule.awaitTag(TestTags.Settings.Screen).assertIsDisplayed()
    }

    fun selectPreferencesTab() = apply {
        rule.awaitTag(TestTags.Settings.PreferencesTab).performClick()
    }

    fun selectInviteTab() = apply {
        rule.awaitTag(TestTags.Settings.InviteTab).performClick()
    }

    fun assertNetworkSectionVisible() = apply { rule.awaitText("NETWORK") }
    fun assertSecuritySectionVisible() = apply { rule.awaitText("SECURITY") }
    fun assertAboutSectionVisible() = apply { rule.awaitText("ABOUT") }
    fun assertProtocolSectionVisible() = apply { rule.awaitText("PROTOCOL") }
    fun assertAdvancedSectionVisible() = apply { rule.awaitText("ADVANCED") }
    fun assertShareLinkButtonVisible() = apply { rule.awaitText("Share link") }

    fun tapRelaysRow() = apply { rule.awaitText("Relays").performClick() }
    fun tapBlossomRow() = apply { rule.awaitText("Blossom Servers").performClick() }
    fun tapStellarContractRow() = apply { rule.awaitText("Stellar Contract").performClick() }
    fun tapAdvancedRow() = apply { rule.awaitText("Advanced").performClick() }
    fun tapBackupRecoveryPhraseRow() = apply { rule.awaitText("Backup Recovery Phrase").performClick() }
}

// ── SearchScreen ──────────────────────────────────────────────────────

class SearchRobot(private val rule: ComposeTestRule) {

    fun assertOnScreen() = apply {
        rule.awaitTag(TestTags.Search.Screen).assertIsDisplayed()
    }

    fun typeQuery(text: String) = apply {
        rule.awaitTag(TestTags.Search.Field).performTextInput(text)
    }

    fun clearQuery() = apply {
        rule.awaitTag(TestTags.Search.Field).performTextReplacement("")
    }

    fun assertResultVisible(text: String) = apply {
        rule.awaitText(text)
    }
}

// ── BottomNav ─────────────────────────────────────────────────────────

class BottomNavRobot(private val rule: ComposeTestRule) {
    fun openContacts() = apply { rule.awaitTag(TestTags.BottomNav.item("contacts")).performClick() }
    fun openChats() = apply { rule.awaitTag(TestTags.BottomNav.item("groups")).performClick() }
    fun openSearch() = apply { rule.awaitTag(TestTags.BottomNav.item("search")).performClick() }
    fun openSettings() = apply { rule.awaitTag(TestTags.BottomNav.item("settings")).performClick() }
}

// ── OnboardingSheet ───────────────────────────────────────────────────

class OnboardingSheetRobot(private val rule: ComposeTestRule) {

    fun assertVisible() = apply {
        rule.awaitTag(TestTags.GroupList.OnboardingSheet).assertIsDisplayed()
    }

    fun tapPrimary() = apply {
        rule.awaitTag(TestTags.GroupList.OnboardingPrimaryButton).performClick()
    }

    fun tapRestoreFromRecoveryPhrase() = apply {
        rule.awaitText("Restore from Recovery Phrase").performClick()
    }
}

// ── Generic helpers ───────────────────────────────────────────────────

/**
 * Sub-screens (Relays, Blossom, Stellar Contract, Advanced, Recovery Phrase,
 * Restore Identity, Group Info) all use the standard back-arrow with
 * contentDescription "Back" for their TopAppBar nav icon. This is the only
 * "Back" content description in the app, so a single generic helper avoids
 * a dedicated robot for each one.
 */
fun ComposeTestRule.tapBackArrow() {
    onNode(hasContentDescription("Back")).performClick()
}
