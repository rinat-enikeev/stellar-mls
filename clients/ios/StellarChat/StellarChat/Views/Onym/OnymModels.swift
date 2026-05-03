import Foundation
import SwiftUI

// Local data model used to power the Onym Settings UI tree. The active
// identity is wired to the live KeyManager; additional identities are
// scaffolded in-memory so the multi-identity flows can be exercised.

struct OnymIdentity: Identifiable, Equatable {
    let id: String
    var name: String
    var npub: String
    var backedUp: Bool
    var active: Bool
    var created: String
}

struct OnymRelay: Identifiable, Equatable {
    let id: String
    var name: String
    var url: String
    var starred: Bool
    var network: String      // "TESTNET" / "MAINNET"
    var visibility: String   // "PUBLIC" / "PRIVATE"
    var latency: Int
}

struct OnymGovType: Identifiable, Equatable {
    let id: String
    let label: String
    let sub: String
}

struct OnymContractVersion: Identifiable, Equatable {
    var id: String { v }
    let v: String
    let date: String
    let current: Bool
    let audit: String
    let sha: String
}

enum OnymCatalog {
    static let governance: [OnymGovType] = [
        .init(id: "anarchy",   label: "Anarchy",     sub: "Open control"),
        .init(id: "democracy", label: "Democracy",   sub: "Majority vote"),
        .init(id: "oligarchy", label: "Oligarchy",   sub: "Council"),
        .init(id: "dialog",    label: "One-on-one",  sub: "Dialog"),
        .init(id: "tyranny",   label: "Tyranny",     sub: "Single admin"),
    ]

    static let versions: [OnymContractVersion] = [
        .init(v: "v0.0.5", date: "May 3, 2026", current: true,  audit: "Audit pending", sha: "0xa12c…f80d"),
        .init(v: "v0.0.4", date: "May 3, 2026", current: false, audit: "Audit pending", sha: "0x9c84…e210"),
        .init(v: "v0.0.3", date: "May 2, 2026", current: false, audit: "Audit pending", sha: "0x7f28…b341"),
        .init(v: "v0.0.2", date: "May 1, 2026", current: false, audit: "Audit pending", sha: "0x4a90…c88a"),
        .init(v: "v0.0.1", date: "May 1, 2026", current: false, audit: "Audit pending", sha: "0x18ad…0072"),
    ]

    static let recoveryWords = ["mistake","amateur","limit","foam","beef","first",
                                 "stuff","unfair","weird","spice","brick","coast"]

    /// Repos referenced from the in-app explainer screens. Linked from the
    /// "Run your own relay" and "Deploy from source" flows so users can fork
    /// the open-source projects.
    static let onymchatGitHubOrg = "github.com/onymchat"
    static let relayRepoURL    = URL(string: "https://github.com/onymchat/onym-relay")!
    static let contractRepoURL = URL(string: "https://github.com/onymchat/contracts")!
}

func onymInviteURL(for identity: OnymIdentity) -> String {
    let payload = String(identity.npub.prefix(44))
    return "https://onym.chat?payload=\(payload)"
}

@MainActor
@Observable
final class OnymSettingsModel {
    var identities: [OnymIdentity]
    var relays: [OnymRelay]
    var useMainnet: Bool = false

    init(activeIdentity: OnymIdentity) {
        self.identities = [
            activeIdentity,
            OnymIdentity(
                id: "id-secondary",
                name: "Identity 2",
                npub: "npub1a35c74815b150f8d9e2c3a4b5c6d7e8f9012345abcde6789f0123456789abcdef",
                backedUp: false,
                active: false,
                created: "Apr 18 2026"
            )
        ]

        self.relays = [
            .init(id: "r1", name: "Onym Official", url: "https://relay.onym.chat",
                  starred: true,  network: "TESTNET", visibility: "PUBLIC",  latency: 42),
            .init(id: "r2", name: "EU Mirror",     url: "https://eu.relay.onym.chat",
                  starred: false, network: "TESTNET", visibility: "PUBLIC",  latency: 71),
            .init(id: "r3", name: "Private",       url: "https://my-relay.fly.dev",
                  starred: false, network: "TESTNET", visibility: "PRIVATE", latency: 28),
        ]
    }

    var activeIdentity: OnymIdentity? { identities.first(where: { $0.active }) }

    func setActive(_ id: String) {
        identities = identities.map { var c = $0; c.active = $0.id == id; return c }
    }

    func toggleStarred(_ id: String) {
        relays = relays.map { var c = $0; c.starred = ($0.id == id) ? !$0.starred : false; return c }
    }

    func appendIdentity(name: String) {
        let new = OnymIdentity(
            id: "id-\(UUID().uuidString.prefix(8))",
            name: name.isEmpty ? "Identity \(identities.count + 1)" : name,
            npub: "npub1" + String((0..<60).map { _ in "0123456789abcdef".randomElement()! }),
            backedUp: false,
            active: false,
            created: Self.todayString()
        )
        identities.append(new)
    }

    func markBackedUp(_ id: String) {
        identities = identities.map { var c = $0; if $0.id == id { c.backedUp = true }; return c }
    }

    static func todayString() -> String {
        let f = DateFormatter()
        f.dateFormat = "MMM d yyyy"
        return f.string(from: Date())
    }
}
