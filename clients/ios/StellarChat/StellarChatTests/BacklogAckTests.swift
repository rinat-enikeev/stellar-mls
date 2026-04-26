import Foundation
import Testing

@testable import StellarChat

/// Pure unit tests for `AppState.computeBacklogAcks` and `firstUnreadMessageID` —
/// both share the same isMine-skipping traversal that was added to fix the cold-start
/// regression in PR #105.
@Suite("BacklogAck")
struct BacklogAckTests {

    private func msg(_ id: String, isMine: Bool) -> ChatMessage {
        ChatMessage(
            id: id, groupID: "g", senderPubkey: "x", text: "t",
            timestamp: Date(timeIntervalSince1970: 0), isMine: isMine
        )
    }

    @Test("Cold-start with empty acked set ACKs all non-mine until cap")
    func coldStart_emptyAckedSet_acksAllNonMineUntilCap() {
        // Cold-start regression scenario: app restarted, `ackedEventIDs` empty,
        // `unreadCounts` was 0. Old `count`-bound logic returned early, leaving
        // received-but-not-opened messages permanently stuck on single ✓.
        let msgs = (1...10).map { msg("m\($0)", isMine: false) }
        let toAck = AppState.computeBacklogAcks(msgs: msgs, alreadyAcked: [])
        #expect(toAck == ["m10","m9","m8","m7","m6","m5","m4","m3","m2","m1"])
    }

    @Test("Stops walking when it hits an isMine anchor")
    func stopsAtIsMineAnchor() {
        // Anchor: when we sent, prior msgs were ACKed at that time, so don't re-walk.
        let msgs = [
            msg("old1", isMine: false), msg("old2", isMine: false), msg("mine", isMine: true),
            msg("new1", isMine: false), msg("new2", isMine: false),
        ]
        let toAck = AppState.computeBacklogAcks(msgs: msgs, alreadyAcked: [])
        #expect(toAck == ["new2", "new1"])
    }

    @Test("Skips handler-acked head but keeps walking past it")
    func skipsHandlerAckedHeadButContinuesWalking() {
        // Micro-race: handler ACKed the newest message between activeGroupID set
        // and the backlog walk. Must keep walking past handler-acked entries instead
        // of stopping, otherwise cold-start + first-arrival dies on the first lookup.
        let msgs = (1...5).map { msg("m\($0)", isMine: false) }
        let toAck = AppState.computeBacklogAcks(msgs: msgs, alreadyAcked: ["m5"])
        #expect(toAck == ["m4", "m3", "m2", "m1"])
    }

    @Test("Fully-acked history produces no new ACKs")
    func fullyAckedHistory_noNewAcks() {
        // Repeat-open within session: every msg already in `ackedEventIDs`. We should
        // walk (bounded by maxScan) and ACK nothing.
        let msgs = (1...20).map { msg("m\($0)", isMine: false) }
        let acked = Set(msgs.map { $0.id })
        let toAck = AppState.computeBacklogAcks(msgs: msgs, alreadyAcked: acked)
        #expect(toAck.isEmpty)
    }

    @Test("maxAcks cap bounds output")
    func maxAcksCap_bounds() {
        let msgs = (1...500).map { msg("m\($0)", isMine: false) }
        let toAck = AppState.computeBacklogAcks(msgs: msgs, alreadyAcked: [], maxAcks: 3, maxScan: 999)
        #expect(toAck == ["m500", "m499", "m498"])
    }

    @Test("maxScan cap bounds walk over handler-acked tail")
    func maxScanCap_bounds() {
        // Pathological: huge tail of already-acked. Verify we don't scan unboundedly.
        let msgs = (1...500).map { msg("m\($0)", isMine: false) }
        let acked = Set(msgs.suffix(100).map { $0.id })
        let toAck = AppState.computeBacklogAcks(msgs: msgs, alreadyAcked: acked, maxAcks: 50, maxScan: 10)
        #expect(toAck.isEmpty)
    }

    @Test("firstUnreadMessageID skips interleaved isMine")
    func firstUnread_skipsInterleavedIsMine() {
        // Reviewer point #4: previously assumed the last `unreadCount` messages were
        // all non-mine — wrong if an outgoing echo lands inside the unread tail.
        let msgs = [
            msg("a", isMine: false), msg("b", isMine: false), msg("mine", isMine: true),
            msg("c", isMine: false), msg("d", isMine: false),
        ]
        // The 3 most-recent non-mine entries are d, c, b — anchor on the oldest ("b").
        // Old fixed-tail formula would have returned "mine" — wrong (it's outgoing).
        #expect(AppState.firstUnreadMessageID(msgs: msgs, unreadCount: 3) == "b")
    }

    @Test("firstUnreadMessageID returns nil for zero count")
    func firstUnread_zeroCount() {
        let msgs = [msg("a", isMine: false)]
        #expect(AppState.firstUnreadMessageID(msgs: msgs, unreadCount: 0) == nil)
    }
}
