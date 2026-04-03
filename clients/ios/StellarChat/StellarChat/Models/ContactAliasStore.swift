import Foundation

/// In-memory cache of contact aliases (pubkey → display name), backed by
/// encrypted SwiftData persistence. All views observe `aliases` reactively.
@MainActor
@Observable
final class ContactAliasStore {
    private(set) var aliases: [String: String] = [:]
    private let store: PersistenceStore

    init(store: PersistenceStore) {
        self.store = store
        self.aliases = store.loadContactAliases()
    }

    /// Returns the display name for a pubkey, or nil if no alias is set.
    func displayName(for pubkey: String) -> String? {
        aliases[pubkey]
    }

    /// Set or update an alias for a pubkey. Persists encrypted to disk.
    func setAlias(pubkey: String, name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            removeAlias(pubkey: pubkey)
            return
        }
        aliases[pubkey] = trimmed
        store.saveContactAliasAsync(pubkey: pubkey, name: trimmed)
    }

    /// Remove an alias for a pubkey.
    func removeAlias(pubkey: String) {
        aliases.removeValue(forKey: pubkey)
        store.deleteContactAliasAsync(pubkey: pubkey)
    }
}
