import Foundation
import SwiftMLS

/// Synthetic content for App Store screenshots. Activated only when the
/// process is launched with the `--demo` argument (see `AppState.isDemoMode`).
/// Never executed in shipped builds reaching real users.
@MainActor
enum DemoFixtures {
    static func seed(into appState: AppState) {
        let now = Date()

        let alice = "alice0000000000000000000000000000000000000000000000000000000000aa"
        let bob   = "bob0000000000000000000000000000000000000000000000000000000000bbbb"
        let carol = "carol000000000000000000000000000000000000000000000000000000000ccc"
        let dave  = "dave0000000000000000000000000000000000000000000000000000000000ddd"

        appState.contactAliasStore.setAlias(pubkey: alice, name: "Alice")
        appState.contactAliasStore.setAlias(pubkey: bob,   name: "Bob")
        appState.contactAliasStore.setAlias(pubkey: carol, name: "Carol")
        appState.contactAliasStore.setAlias(pubkey: dave,  name: "Dave")

        // Include the local user's actual member leaf so canSend(in:) returns
        // true and the "You were removed from this group" banner doesn't show.
        let selfLeaf = (try? appState.keyManager.memberLeaf).flatMap { Optional.some($0) }

        let groupA = makeGroup(
            id: "a1b2c3d4e5f60718293a4b5c6d7e8f9001112233445566778899aabbccddeeff",
            name: "Climbing Crew",
            createdAt: now.addingTimeInterval(-7 * 86_400),
            members: [alice, bob, carol],
            selfLeaf: selfLeaf
        )
        let groupB = makeGroup(
            id: "1122334455667788990aabbccddeeff0a1b2c3d4e5f60718293a4b5c6d7e8f99",
            name: "Family",
            createdAt: now.addingTimeInterval(-30 * 86_400),
            members: [alice, dave],
            selfLeaf: selfLeaf
        )

        let messagesA = makeMessages(
            groupID: groupA.id,
            now: now,
            entries: [
                (alice, "Anyone up for the gym tomorrow?", false, -3600 * 6),
                (bob,   "I'm in. 6pm?",                    false, -3600 * 5),
                (carol, "Make it 7, just got off work",    false, -3600 * 4),
                (alice, "7 it is",                          false, -3600 * 3),
                ("me",  "I'll bring the chalk",            true,  -3600 * 2),
                (bob,   "Pulling my fingers off this week 💪", false, -3600),
                (carol, "There's a new V5 on the cave wall", false, -1800),
                ("me",  "Can't wait",                       true,  -1500),
                (alice, "See you all there 🧗",             false, -600),
                ("me",  "🔥",                                true,  -120),
            ]
        )

        let messagesB = makeMessages(
            groupID: groupB.id,
            now: now,
            entries: [
                (dave,  "Sunday dinner at our place?",      false, -86_400 * 2),
                ("me",  "We'll be there around 6",          true,  -86_400 * 2 + 3600),
                (alice, "Bringing dessert!",                false, -86_400),
                (dave,  "Perfect ❤️",                        false, -86_400 + 1800),
                ("me",  "Need anything else?",              true,  -3600 * 5),
                (alice, "Just yourselves",                   false, -3600 * 4),
                (dave,  "And maybe a bottle of red 🍷",      false, -3600 * 3),
                ("me",  "Done",                              true,  -3600 * 2),
                (alice, "See you Sunday!",                   false, -1200),
                (dave,  "👋",                                false, -300),
            ]
        )

        for msg in messagesA + messagesB {
            appState.chatMessages[msg.groupID, default: []].append(msg)
        }

        var seededGroups: [ChatGroup] = []
        for var group in [groupA, groupB] {
            let groupMessages = appState.chatMessages[group.id] ?? []
            group.lastMessageAt = groupMessages.last?.timestamp
            seededGroups.append(group)
        }
        appState.groups = seededGroups
    }

    private static func makeGroup(
        id: String,
        name: String,
        createdAt: Date,
        members: [String],
        selfLeaf: SEPGroupMemberLeaf?
    ) -> ChatGroup {
        var leaves: [SEPGroupMemberLeaf] = members.enumerated().map { idx, _ in
            SEPGroupMemberLeaf(
                publicKeyCompressed: Data(repeating: UInt8(idx + 1), count: 48),
                leafHash: Data(repeating: UInt8(idx + 0x80), count: 32)
            )
        }
        if let selfLeaf { leaves.append(selfLeaf) }
        return ChatGroup(
            id: id,
            name: name,
            groupSecret: Data(repeating: 0xAB, count: 32),
            createdAt: createdAt,
            relayHints: [],
            members: leaves,
            epoch: 0,
            salt: Data(repeating: 0, count: 32),
            tier: .medium
        )
    }

    private static func makeMessages(
        groupID: String,
        now: Date,
        entries: [(sender: String, text: String, isMine: Bool, offsetSeconds: TimeInterval)]
    ) -> [ChatMessage] {
        entries.enumerated().map { idx, entry in
            ChatMessage(
                id: "demo-\(groupID.prefix(8))-\(idx)",
                groupID: groupID,
                senderPubkey: entry.isMine ? "" : entry.sender,
                text: entry.text,
                timestamp: now.addingTimeInterval(entry.offsetSeconds),
                isMine: entry.isMine,
                status: .delivered
            )
        }
    }
}
