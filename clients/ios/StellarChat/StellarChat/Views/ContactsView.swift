import SwiftUI

struct ContactsView: View {
    @Environment(AppState.self) private var appState
    @State private var aliasTarget: String?
    @State private var aliasText = ""

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
                    let alias = appState.contactAliasStore.displayName(for: contact.pubkey)
                    HStack(spacing: 12) {
                        AvatarView(pubkey: contact.pubkey, alias: alias)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(alias ?? String(contact.pubkey.prefix(12)) + "...")
                                .font(.body)
                                .monospaced(alias == nil)
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
                    .contextMenu {
                        Button {
                            aliasText = alias ?? ""
                            aliasTarget = contact.pubkey
                        } label: {
                            Label("Set Name", systemImage: "pencil")
                        }
                        if alias != nil {
                            Button(role: .destructive) {
                                appState.contactAliasStore.removeAlias(pubkey: contact.pubkey)
                            } label: {
                                Label("Remove Name", systemImage: "trash")
                            }
                        }
                    }
                }
            }
        }
        .navigationTitle("Contacts")
        .alert("Set Name", isPresented: .init(
            get: { aliasTarget != nil },
            set: { if !$0 { aliasTarget = nil } }
        )) {
            TextField("Display name", text: $aliasText)
            Button("Save") {
                if let pubkey = aliasTarget {
                    appState.contactAliasStore.setAlias(pubkey: pubkey, name: aliasText)
                }
                aliasTarget = nil
            }
            Button("Cancel", role: .cancel) { aliasTarget = nil }
        } message: {
            if let pubkey = aliasTarget {
                Text(String(pubkey.prefix(16)) + "...")
            }
        }
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
