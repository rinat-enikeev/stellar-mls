import Contacts
import Foundation

/// Local lifecycle state for a Telegram-invited contact.
enum InvitedContactStatus: String {
    case pending   // row created, user about to hand off to Telegram
    case sent      // Telegram handoff completed
    case joined    // invitee posted / onboard-ack reconciled
}

enum InvitedContactKind: String {
    case group
    case onboard
}

struct InvitedContact: Identifiable, Equatable {
    var phone: String
    var displayName: String
    var inviteKind: InvitedContactKind
    var inviteURL: String
    var handshakeNonce: String?
    var groupID: String?
    var invitedAt: Date
    var status: InvitedContactStatus
    var resolvedPubkey: String?

    var id: String { phone }
}

/// Observable cache of invited contacts backed by encrypted SwiftData persistence.
@MainActor
@Observable
final class InvitedContactStore {
    private(set) var contacts: [InvitedContact] = []
    private let store: PersistenceStore

    init(store: PersistenceStore) {
        self.store = store
        reload()
    }

    private func reload() {
        contacts = store.loadInvitedContacts().map { rec in
            InvitedContact(
                phone: rec.phone,
                displayName: rec.displayName,
                inviteKind: InvitedContactKind(rawValue: rec.inviteKind) ?? .group,
                inviteURL: rec.inviteURL,
                handshakeNonce: rec.handshakeNonce,
                groupID: rec.groupID,
                invitedAt: rec.invitedAt,
                status: InvitedContactStatus(rawValue: rec.status) ?? .pending,
                resolvedPubkey: rec.resolvedPubkey
            )
        }
    }

    func contains(phone: String) -> Bool {
        contacts.contains { $0.phone == phone }
    }

    func upsert(_ contact: InvitedContact) {
        if let index = contacts.firstIndex(where: { $0.phone == contact.phone }) {
            contacts[index] = contact
        } else {
            contacts.insert(contact, at: 0)
        }
        store.saveInvitedContactAsync(Self.toRecord(contact))
    }

    func setStatus(phone: String, status: InvitedContactStatus) {
        if let index = contacts.firstIndex(where: { $0.phone == phone }) {
            contacts[index].status = status
        }
        store.updateInvitedContactStatusAsync(phone: phone, status: status.rawValue)
    }

    func reconcile(nonce: String, resolvedPubkey: String) -> InvitedContact? {
        guard let rec = store.reconcileInvitedContact(nonce: nonce, resolvedPubkey: resolvedPubkey) else {
            return nil
        }
        let updated = InvitedContact(
            phone: rec.phone,
            displayName: rec.displayName,
            inviteKind: InvitedContactKind(rawValue: rec.inviteKind) ?? .group,
            inviteURL: rec.inviteURL,
            handshakeNonce: rec.handshakeNonce,
            groupID: rec.groupID,
            invitedAt: rec.invitedAt,
            status: InvitedContactStatus(rawValue: rec.status) ?? .joined,
            resolvedPubkey: rec.resolvedPubkey
        )
        if let index = contacts.firstIndex(where: { $0.phone == updated.phone }) {
            contacts[index] = updated
        }
        return updated
    }

    func remove(phone: String) {
        contacts.removeAll { $0.phone == phone }
        store.deleteInvitedContact(phone: phone)
    }

    private static func toRecord(_ c: InvitedContact) -> PersistenceStore.InvitedContactRecord {
        PersistenceStore.InvitedContactRecord(
            phone: c.phone,
            displayName: c.displayName,
            inviteKind: c.inviteKind.rawValue,
            inviteURL: c.inviteURL,
            handshakeNonce: c.handshakeNonce,
            groupID: c.groupID,
            invitedAt: c.invitedAt,
            status: c.status.rawValue,
            resolvedPubkey: c.resolvedPubkey
        )
    }
}

// MARK: - System Contacts Importer

struct PhoneContact: Identifiable, Equatable {
    let identifier: String     // CNContact identifier
    let displayName: String
    let phone: String          // E.164

    var id: String { identifier }
}

enum ContactsPermission {
    case notDetermined
    case denied
    case authorized
}

/// Fetches phone contacts from the system address book, normalizing numbers to E.164.
enum SystemContactsImporter {

    static func permission() -> ContactsPermission {
        switch CNContactStore.authorizationStatus(for: .contacts) {
        case .notDetermined: return .notDetermined
        case .authorized: return .authorized
        #if compiler(>=5.9)
        case .limited: return .authorized
        #endif
        case .denied, .restricted: return .denied
        @unknown default: return .denied
        }
    }

    static func requestAccess() async -> Bool {
        let store = CNContactStore()
        return await withCheckedContinuation { cont in
            store.requestAccess(for: .contacts) { granted, _ in
                cont.resume(returning: granted)
            }
        }
    }

    /// Fetch all contacts that have at least one phone number. Duplicates by
    /// E.164 number are collapsed to the first-seen display name.
    static func fetchAll() throws -> [PhoneContact] {
        let store = CNContactStore()
        let keys: [CNKeyDescriptor] = [
            CNContactGivenNameKey as CNKeyDescriptor,
            CNContactFamilyNameKey as CNKeyDescriptor,
            CNContactOrganizationNameKey as CNKeyDescriptor,
            CNContactPhoneNumbersKey as CNKeyDescriptor,
        ]
        let request = CNContactFetchRequest(keysToFetch: keys)
        var results: [PhoneContact] = []
        var seen = Set<String>()

        let regionCode = Locale.current.region?.identifier ?? "US"

        try store.enumerateContacts(with: request) { contact, _ in
            let name = Self.formatName(contact)
            for labelled in contact.phoneNumbers {
                let raw = labelled.value.stringValue
                guard let e164 = Self.normalize(raw, defaultRegion: regionCode) else { continue }
                if seen.insert(e164).inserted {
                    results.append(PhoneContact(
                        identifier: contact.identifier,
                        displayName: name,
                        phone: e164
                    ))
                }
            }
        }

        return results.sorted { $0.displayName.localizedCaseInsensitiveCompare($1.displayName) == .orderedAscending }
    }

    private static func formatName(_ contact: CNContact) -> String {
        let first = contact.givenName
        let last = contact.familyName
        let full = [first, last].filter { !$0.isEmpty }.joined(separator: " ")
        if !full.isEmpty { return full }
        if !contact.organizationName.isEmpty { return contact.organizationName }
        return "(unnamed)"
    }

    /// Best-effort E.164 normalizer using only ASCII digits.
    /// If the raw number starts with '+', we keep it; otherwise prepend the
    /// region's calling code. This avoids a dependency on libphonenumber while
    /// handling the common cases (stored as +1…, or stored as national digits).
    static func normalize(_ raw: String, defaultRegion: String) -> String? {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }

        let hasPlus = trimmed.hasPrefix("+")
        let digits = trimmed.unicodeScalars
            .filter { CharacterSet.decimalDigits.contains($0) }
            .map(Character.init)
        let digitsString = String(digits)
        guard digitsString.count >= 7 else { return nil }

        if hasPlus {
            return "+" + digitsString
        }

        if let cc = Self.callingCode(forRegion: defaultRegion) {
            var national = digitsString
            if national.hasPrefix("0") {
                national.removeFirst()
            }
            return "+" + cc + national
        }
        return "+" + digitsString
    }

    /// Small table of common calling codes; falls back to US if unknown.
    /// A full libphonenumber is out of scope for v1.
    private static func callingCode(forRegion region: String) -> String? {
        let table: [String: String] = [
            "US": "1", "CA": "1", "GB": "44", "DE": "49", "FR": "33",
            "IT": "39", "ES": "34", "NL": "31", "RU": "7", "UA": "380",
            "BY": "375", "KZ": "7", "IN": "91", "CN": "86", "JP": "81",
            "KR": "82", "AU": "61", "NZ": "64", "BR": "55", "MX": "52",
            "AR": "54", "TR": "90", "PL": "48", "SE": "46", "NO": "47",
            "DK": "45", "FI": "358", "CZ": "420", "AT": "43", "CH": "41",
            "BE": "32", "PT": "351", "GR": "30", "IE": "353", "IL": "972",
            "AE": "971", "SA": "966", "EG": "20", "ZA": "27",
        ]
        return table[region.uppercased()]
    }
}
