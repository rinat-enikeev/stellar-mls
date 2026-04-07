import Foundation
import Testing

@testable import StellarChat

@Suite("NostrEvent")
struct NostrEventTests {
    @Test("Event ID is a 64-character hex string (SHA-256)")
    func eventIDFormat() throws {
        let keyManager = try KeyManager()
        let event = try NostrEvent.build(
            kind: 24114,
            tags: [["t", "test-topic"]],
            content: "hello",
            keyManager: keyManager
        )
        #expect(event.id.count == 64)
        // All hex characters
        let hexChars = CharacterSet(charactersIn: "0123456789abcdef")
        #expect(event.id.unicodeScalars.allSatisfy { hexChars.contains($0) })
    }

    @Test("Event pubkey is 64-character hex (secp256k1 x-only)")
    func pubkeyFormat() throws {
        let keyManager = try KeyManager()
        let event = try NostrEvent.build(
            kind: 24114,
            tags: [],
            content: "test",
            keyManager: keyManager
        )
        #expect(event.pubkey.count == 64)
    }

    @Test("Event signature is 128-character hex (Schnorr)")
    func signatureFormat() throws {
        let keyManager = try KeyManager()
        let event = try NostrEvent.build(
            kind: 24114,
            tags: [],
            content: "test",
            keyManager: keyManager
        )
        #expect(event.sig.count == 128)
    }

    @Test("Event includes ms tag for sub-second ordering")
    func msTag() throws {
        let keyManager = try KeyManager()
        let beforeMs = Int64(Date().timeIntervalSince1970 * 1000)
        let event = try NostrEvent.build(
            kind: 24114,
            tags: [["t", "topic"]],
            content: "hello",
            keyManager: keyManager
        )
        let afterMs = Int64(Date().timeIntervalSince1970 * 1000)

        let msTag = event.tags.first { $0.first == "ms" }
        #expect(msTag != nil, "Event must include an ms tag")

        if let msValue = msTag?[1], let ms = Int64(msValue) {
            #expect(ms >= beforeMs && ms <= afterMs, "ms tag must be current time in milliseconds")
        }
    }

    @Test("verifyEventID returns true for unmodified event")
    func verifyEventIDValid() throws {
        let keyManager = try KeyManager()
        let event = try NostrEvent.build(
            kind: 24114,
            tags: [["t", "test"]],
            content: "hello",
            keyManager: keyManager
        )
        #expect(event.verifyEventID() == true)
    }

    @Test("Event kind is preserved")
    func kindPreserved() throws {
        let keyManager = try KeyManager()
        let event = try NostrEvent.build(
            kind: 34113,
            tags: [],
            content: "test",
            keyManager: keyManager
        )
        #expect(event.kind == 34113)
    }

    @Test("Event tags are preserved")
    func tagsPreserved() throws {
        let keyManager = try KeyManager()
        let tags: [[String]] = [["t", "my-topic"], ["d", "my-identifier"]]
        let event = try NostrEvent.build(
            kind: 24114,
            tags: tags,
            content: "hello",
            keyManager: keyManager
        )
        // Input tags should be present (ms tag is appended)
        #expect(event.tags.contains { $0 == ["t", "my-topic"] })
        #expect(event.tags.contains { $0 == ["d", "my-identifier"] })
    }

    @Test("displayMilliseconds uses ms tag when available")
    func displayMilliseconds() throws {
        let keyManager = try KeyManager()
        let event = try NostrEvent.build(
            kind: 24114,
            tags: [],
            content: "test",
            keyManager: keyManager
        )
        // displayMilliseconds should return the ms tag value
        #expect(event.displayMilliseconds > 0)
    }
}
