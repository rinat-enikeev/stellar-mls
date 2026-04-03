import SwiftUI

struct ContactsView: View {
    @Environment(AppState.self) private var appState

    private var contacts: [(pubkey: String, groupNames: String, lastSeen: Date)] {
        var map: [String: (groups: Set<String>, lastSeen: Date)] = [:]

        for (groupID, messages) in appState.chatMessages {
            for message in messages where !message.isMine {
                var entry = map[message.senderPubkey] ?? (Set(), .distantPast)
                entry.groups.insert(groupID)
                entry.lastSeen = max(entry.lastSeen, message.timestamp)
                map[message.senderPubkey] = entry
            }
        }

        return map.map { pubkey, info in
            let names = appState.groups
                .filter { info.groups.contains($0.id) }
                .map(\.name)
                .joined(separator: ", ")
            return (pubkey: pubkey, groupNames: names, lastSeen: info.lastSeen)
        }
        .sorted { $0.lastSeen > $1.lastSeen }
    }

    var body: some View {
        List {
            if contacts.isEmpty {
                ContentUnavailableView(
                    "No Contacts",
                    systemImage: "person.2",
                    description: Text("People you chat with will appear here.")
                )
            } else {
                ForEach(contacts, id: \.pubkey) { contact in
                    HStack(spacing: 12) {
                        AvatarView(pubkey: contact.pubkey)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(String(contact.pubkey.prefix(12)) + "...")
                                .font(.body)
                                .monospaced()
                            Text(contact.groupNames)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                        Spacer()
                        Text(relativeTimestamp(contact.lastSeen))
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 4)
                }
            }
        }
        .navigationTitle("Contacts")
    }

    private func relativeTimestamp(_ date: Date) -> String {
        let seconds = -date.timeIntervalSinceNow
        if seconds < 60 { return "Just now" }
        if seconds < 3600 { return "\(Int(seconds / 60))m" }
        if seconds < 86400 { return "\(Int(seconds / 3600))h" }
        if Calendar.current.isDateInYesterday(date) { return "Yesterday" }
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d"
        return formatter.string(from: date)
    }
}
