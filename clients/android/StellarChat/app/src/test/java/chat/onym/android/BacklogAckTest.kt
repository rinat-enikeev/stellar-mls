package chat.onym.android

import chat.onym.android.model.ChatMessage
import chat.onym.android.viewmodel.GroupListViewModel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.util.Date

/**
 * Pure JVM unit tests for the `sendBacklogAcks` walk-back logic and the
 * `firstUnreadMessageID` separator anchor — both share the same isMine-skipping
 * traversal that was added to fix the cold-start regression in PR #105.
 */
class BacklogAckTest {

    private fun msg(id: String, isMine: Boolean): ChatMessage = ChatMessage(
        id = id, groupID = "g", senderPubkey = "x", text = "t",
        timestamp = Date(0), isMine = isMine
    )

    @Test
    fun coldStart_emptyAckedSet_acksAllNonMineUntilCap() {
        // Cold-start regression scenario: app restarted, `ackedEventIDs` empty,
        // `unreadCounts` was 0. Old `count`-bound logic returned early — leaving
        // received-but-not-opened messages permanently stuck on single ✓.
        val msgs = (1..10).map { msg("m$it", isMine = false) }
        val toAck = GroupListViewModel.computeBacklogAcks(msgs, emptySet())
        assertEquals(listOf("m10","m9","m8","m7","m6","m5","m4","m3","m2","m1"), toAck)
    }

    @Test
    fun stopsAtIsMineAnchor() {
        // Anchor: when we sent, prior msgs were ACKed at that time, so don't re-walk.
        val msgs = listOf(
            msg("old1", false), msg("old2", false), msg("mine", true),
            msg("new1", false), msg("new2", false)
        )
        val toAck = GroupListViewModel.computeBacklogAcks(msgs, emptySet())
        assertEquals(listOf("new2", "new1"), toAck)
    }

    @Test
    fun skipsHandlerAckedHeadButContinuesWalking() {
        // Micro-race: handler ACKed the newest message between activeGroupID set
        // and the backlog walk. Old code (after the count-bound fix) handled this
        // because it counted toAck.size, not iterations. The new code must keep
        // walking past handler-acked entries instead of stopping.
        val msgs = (1..5).map { msg("m$it", false) }
        val alreadyAcked = setOf("m5") // handler beat us to the newest
        val toAck = GroupListViewModel.computeBacklogAcks(msgs, alreadyAcked)
        assertEquals(listOf("m4", "m3", "m2", "m1"), toAck)
    }

    @Test
    fun fullyAckedHistory_noNewAcks() {
        // Repeat-open within session: every msg already in `ackedEventIDs`. We
        // should walk (bounded by maxScan) and ACK nothing.
        val msgs = (1..20).map { msg("m$it", false) }
        val acked = msgs.map { it.id }.toSet()
        val toAck = GroupListViewModel.computeBacklogAcks(msgs, acked)
        assertEquals(emptyList<String>(), toAck)
    }

    @Test
    fun maxAcksCap_bounds() {
        val msgs = (1..500).map { msg("m$it", false) }
        val toAck = GroupListViewModel.computeBacklogAcks(msgs, emptySet(), maxAcks = 3, maxScan = 999)
        assertEquals(listOf("m500", "m499", "m498"), toAck)
    }

    @Test
    fun maxScanCap_bounds_walkOverHandlerAckedTail() {
        // Pathological: huge tail of already-acked. Verify we don't scan unboundedly.
        val msgs = (1..500).map { msg("m$it", false) }
        val acked = msgs.takeLast(100).map { it.id }.toSet()
        // maxScan=10 means we only look at the last 10 entries (all acked) → no ACKs.
        val toAck = GroupListViewModel.computeBacklogAcks(msgs, acked, maxAcks = 50, maxScan = 10)
        assertEquals(emptyList<String>(), toAck)
    }

    @Test
    fun firstUnread_skipsInterleavedIsMine() {
        // PR #105 reviewer point #4: `firstUnreadMessageID` previously assumed the last
        // `unreadCount` messages were all non-mine — wrong if an outgoing echo (e.g.,
        // multi-device) lands inside the unread tail. Walk skipping isMine matches
        // the new sendBacklogAcks behaviour.
        val msgs = listOf(
            msg("a", false), msg("b", false), msg("mine", true),
            msg("c", false), msg("d", false)
        )
        // The 3 most-recent non-mine entries are d, c, b. Anchor on the oldest of those
        // (b) so the separator lands above the unread block. The old fixed-tail formula
        // would have returned "mine" — wrong on two counts (it's outgoing, not unread).
        val anchor = GroupListViewModel.firstUnreadMessageID(msgs, 3)
        assertEquals("b", anchor)
    }

    @Test
    fun firstUnread_zeroCount_returnsNull() {
        val msgs = listOf(msg("a", false))
        assertNull(GroupListViewModel.firstUnreadMessageID(msgs, 0))
    }
}
