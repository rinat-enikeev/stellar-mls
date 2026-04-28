package chat.onym.android

import chat.onym.android.model.ChatGroup
import chat.onym.android.model.ChatMessage
import chat.onym.android.model.MessageStatus
import chat.onym.android.viewmodel.GroupListViewModel
import com.stellarmls.mls.SEPGroupMemberLeaf
import com.stellarmls.mls.SEPTier
import java.util.Date

/**
 * Synthetic content for Play Store screenshots. Activated only when
 * `MainActivity` receives the `demo=true` intent extra (see
 * `GroupListViewModel.isDemoMode`). Never executed in shipped builds.
 */
object DemoFixtures {
    private const val ALICE = "alice0000000000000000000000000000000000000000000000000000000000aa"
    private const val BOB   = "bob0000000000000000000000000000000000000000000000000000000000bbbb"
    private const val CAROL = "carol000000000000000000000000000000000000000000000000000000000ccc"
    private const val DAVE  = "dave0000000000000000000000000000000000000000000000000000000000ddd"

    fun seed(vm: GroupListViewModel) {
        // Bypass the suspend persistence path — we only need the in-memory map.
        vm.contactAliasStore.aliases[ALICE] = "Alice"
        vm.contactAliasStore.aliases[BOB]   = "Bob"
        vm.contactAliasStore.aliases[CAROL] = "Carol"
        vm.contactAliasStore.aliases[DAVE]  = "Dave"

        // Include the local user's actual member leaf so canSendInGroup returns
        // true and the "You were removed from this group" footer doesn't show.
        val selfLeaf = runCatching { vm.keyManager.memberLeaf() }.getOrNull()

        val now = System.currentTimeMillis()
        val groupA = makeGroup(
            id = "a1b2c3d4e5f60718293a4b5c6d7e8f9001112233445566778899aabbccddeeff",
            name = "Climbing Crew",
            createdAtMs = now - 7L * 86_400_000L,
            memberPubkeyTags = listOf(0x01, 0x02, 0x03),
            selfLeaf = selfLeaf
        )
        val groupB = makeGroup(
            id = "1122334455667788990aabbccddeeff0a1b2c3d4e5f60718293a4b5c6d7e8f99",
            name = "Family",
            createdAtMs = now - 30L * 86_400_000L,
            memberPubkeyTags = listOf(0x04, 0x05),
            selfLeaf = selfLeaf
        )

        val messagesA = makeMessages(
            groupID = groupA.id,
            now = now,
            entries = listOf(
                Entry(ALICE, "Anyone up for the gym tomorrow?", false, -3600 * 6),
                Entry(BOB,   "I'm in. 6pm?",                    false, -3600 * 5),
                Entry(CAROL, "Make it 7, just got off work",    false, -3600 * 4),
                Entry(ALICE, "7 it is",                          false, -3600 * 3),
                Entry("me",  "I'll bring the chalk",            true,  -3600 * 2),
                Entry(BOB,   "Pulling my fingers off this week 💪", false, -3600),
                Entry(CAROL, "There's a new V5 on the cave wall", false, -1800),
                Entry("me",  "Can't wait",                       true,  -1500),
                Entry(ALICE, "See you all there 🧗",             false, -600),
                Entry("me",  "🔥",                                true,  -120),
            )
        )

        val messagesB = makeMessages(
            groupID = groupB.id,
            now = now,
            entries = listOf(
                Entry(DAVE,  "Sunday dinner at our place?",      false, -86_400 * 2),
                Entry("me",  "We'll be there around 6",          true,  -86_400 * 2 + 3600),
                Entry(ALICE, "Bringing dessert!",                false, -86_400),
                Entry(DAVE,  "Perfect ❤️",                        false, -86_400 + 1800),
                Entry("me",  "Need anything else?",              true,  -3600 * 5),
                Entry(ALICE, "Just yourselves",                   false, -3600 * 4),
                Entry(DAVE,  "And maybe a bottle of red 🍷",      false, -3600 * 3),
                Entry("me",  "Done",                              true,  -3600 * 2),
                Entry(ALICE, "See you Sunday!",                   false, -1200),
                Entry(DAVE,  "👋",                                false, -300),
            )
        )

        groupA.lastMessageAt = messagesA.last().timestamp.time
        groupB.lastMessageAt = messagesB.last().timestamp.time

        vm.chatMessages[groupA.id] = messagesA
        vm.chatMessages[groupB.id] = messagesB
        vm.groups.clear()
        vm.groups.add(groupA)
        vm.groups.add(groupB)
    }

    private fun makeGroup(
        id: String,
        name: String,
        createdAtMs: Long,
        memberPubkeyTags: List<Int>,
        selfLeaf: SEPGroupMemberLeaf?
    ): ChatGroup {
        val members = memberPubkeyTags.mapIndexed { idx, tag ->
            SEPGroupMemberLeaf(
                publicKeyCompressed = ByteArray(48) { tag.toByte() },
                leafHash = ByteArray(32) { (0x80 + idx).toByte() }
            )
        }.toMutableList()
        if (selfLeaf != null) members.add(selfLeaf)
        return ChatGroup(
            id = id,
            name = name,
            groupSecret = ByteArray(32) { 0xAB.toByte() },
            createdAt = Date(createdAtMs),
            relayHints = emptyList(),
            members = members,
            epoch = 0,
            salt = ByteArray(32),
            commitment = null,
            tier = SEPTier.MEDIUM
        )
    }

    private data class Entry(
        val sender: String,
        val text: String,
        val isMine: Boolean,
        val offsetSeconds: Long
    )

    private fun makeMessages(
        groupID: String,
        now: Long,
        entries: List<Entry>
    ): List<ChatMessage> = entries.mapIndexed { idx, e ->
        ChatMessage(
            id = "demo-${groupID.take(8)}-$idx",
            groupID = groupID,
            senderPubkey = if (e.isMine) "" else e.sender,
            text = e.text,
            timestamp = Date(now + e.offsetSeconds * 1000L),
            isMine = e.isMine,
            status = MessageStatus.DELIVERED
        )
    }
}
