import Foundation
import Testing

@testable import StellarChat

@Suite("SyncState")
struct SyncStateTests {
    @Test("Idle reports unlocked")
    func idleNotLocked() {
        #expect(SyncState.idle.isLocked == false)
    }

    @Test("Pending reports locked")
    func pendingLocked() {
        let state = SyncState.pending(
            reason: .awaitingChainConfirmation,
            since: Date(),
            attempts: 0
        )
        #expect(state.isLocked == true)
    }

    @Test("Failed reports locked")
    func failedLocked() {
        let state = SyncState.failed(
            reason: .awaitingChainConfirmation,
            lastError: "timeout"
        )
        #expect(state.isLocked == true)
    }
}

@Suite("QueuedStateUpdate")
struct QueuedStateUpdateTests {
    @Test("Codable round-trip preserves all fields")
    func codableRoundtrip() throws {
        let now = Date(timeIntervalSince1970: 1_700_000_000)
        let entry = QueuedStateUpdate(
            updateJSON: Data(repeating: 0xAB, count: 32),
            epoch: 7,
            firstAttemptAt: now,
            lastAttemptAt: now.addingTimeInterval(15),
            attempts: 3,
            lastError: "chain epoch 6 < update epoch 7"
        )

        let encoded = try JSONEncoder().encode(entry)
        let decoded = try JSONDecoder().decode(QueuedStateUpdate.self, from: encoded)

        #expect(decoded == entry)
    }
}

@Suite("AppState.mergeQueuedUpdate")
struct MergeQueuedUpdateTests {
    private func make(epoch: UInt64, error: String = "") -> QueuedStateUpdate {
        QueuedStateUpdate(
            updateJSON: Data([UInt8(epoch & 0xff)]),
            epoch: epoch,
            firstAttemptAt: Date(),
            lastAttemptAt: Date(),
            attempts: 1,
            lastError: error
        )
    }

    @Test("Empty queue accepts new entry")
    func emptyAcceptsEntry() {
        let merged = AppState.mergeQueuedUpdate(existing: [], newEntry: make(epoch: 5))
        #expect(merged.count == 1)
        #expect(merged[0].epoch == 5)
    }

    @Test("Newer epoch supersedes older queued entries")
    func newerSupersedesOlder() {
        let existing = [make(epoch: 3), make(epoch: 5)]
        let merged = AppState.mergeQueuedUpdate(existing: existing, newEntry: make(epoch: 6))
        #expect(merged.count == 1)
        #expect(merged[0].epoch == 6)
    }

    @Test("Same epoch as queued entry replaces it")
    func sameEpochReplaces() {
        let existing = [make(epoch: 4, error: "old error")]
        let merged = AppState.mergeQueuedUpdate(
            existing: existing,
            newEntry: make(epoch: 4, error: "new error")
        )
        #expect(merged.count == 1)
        #expect(merged[0].lastError == "new error")
    }

    @Test("Older entry does not displace newer queued entry")
    func olderDoesNotDisplaceNewer() {
        // Edge case: shouldn't really happen (we only enqueue on a fresh update
        // with a higher epoch than current), but the merge logic should still
        // behave deterministically — keep the newer queued entry.
        let existing = [make(epoch: 7)]
        let merged = AppState.mergeQueuedUpdate(existing: existing, newEntry: make(epoch: 5))
        #expect(merged.count == 2)
        #expect(merged[0].epoch == 5)
        #expect(merged[1].epoch == 7)
    }

    @Test("Queue stays sorted by epoch ascending")
    func sortedAscending() {
        // Insert order: 10 (lone), 1 (lower than 10 — kept, queue [1,10]),
        // 5 (kept, supersedes 1 → queue [5,10]),
        // 3 (kept, doesn't supersede 5 or 10 → queue [3,5,10] after sort).
        var queue: [QueuedStateUpdate] = []
        for epoch: UInt64 in [10, 1, 5, 3] {
            queue = AppState.mergeQueuedUpdate(existing: queue, newEntry: make(epoch: epoch))
        }
        #expect(queue.map(\.epoch) == [3, 5, 10])
    }
}
