package uitests

import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.SemanticsNodeInteractionsProvider
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.test.onNodeWithTag

/**
 * Default timeout for ordinary UI assertions — 5s is comfortable for any
 * frame-driven Compose state change on a real device.
 */
const val DEFAULT_TIMEOUT_MS = 5_000L

/**
 * Long timeout for assertions that wait on real network / on-chain settlement.
 * Soroban testnet ledger close ranges from ~5s to ~30s in practice; allow
 * 2 minutes so transient relay backoff or testnet congestion doesn't ruin
 * a full-journey run.
 */
const val LONG_TIMEOUT_MS = 120_000L

/** Waits for a node with the given testTag to appear, then returns it. */
fun ComposeTestRule.awaitTag(
    tag: String,
    timeoutMs: Long = DEFAULT_TIMEOUT_MS
): SemanticsNodeInteraction {
    waitUntil(timeoutMs) {
        onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().isNotEmpty()
    }
    return onNodeWithTag(tag)
}

/** Waits for a node with the given text to appear, then returns it. */
fun ComposeTestRule.awaitText(
    text: String,
    substring: Boolean = false,
    timeoutMs: Long = DEFAULT_TIMEOUT_MS
): SemanticsNodeInteraction {
    waitUntil(timeoutMs) {
        onAllNodesWithText(text, substring = substring).fetchSemanticsNodes().isNotEmpty()
    }
    return onNodeWithText(text, substring = substring)
}

/** Long-timeout variant for waits on real-network/on-chain phases. */
fun ComposeTestRule.awaitTextLong(
    text: String,
    substring: Boolean = false
): SemanticsNodeInteraction = awaitText(text, substring = substring, timeoutMs = LONG_TIMEOUT_MS)

/** Waits until no node with the given testTag exists. */
fun ComposeTestRule.awaitTagGone(tag: String, timeoutMs: Long = DEFAULT_TIMEOUT_MS) {
    waitUntil(timeoutMs) {
        onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().isEmpty()
    }
}

/** Waits until no node with the given text exists. */
fun ComposeTestRule.awaitTextGone(
    text: String,
    substring: Boolean = false,
    timeoutMs: Long = DEFAULT_TIMEOUT_MS
) {
    waitUntil(timeoutMs) {
        onAllNodesWithText(text, substring = substring).fetchSemanticsNodes().isEmpty()
    }
}

private fun SemanticsNodeInteractionsProvider.onAllNodesWithText(
    text: String,
    substring: Boolean
) = onAllNodes(
    androidx.compose.ui.test.hasText(text, substring = substring)
)

private fun SemanticsNodeInteractionsProvider.onNodeWithText(
    text: String,
    substring: Boolean
) = onNode(
    androidx.compose.ui.test.hasText(text, substring = substring)
)
