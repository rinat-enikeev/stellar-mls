import SwiftUI

struct ContactsView: View {
    @Environment(AppState.self) private var appState
    @State private var aliasTarget: String?
    @State private var aliasText = ""
    @State private var phonebook: [PhoneContact] = []
    @State private var contactsPermission: ContactsPermission = SystemContactsImporter.permission()
    @State private var importing = false
    @State private var importError: String?
    @State private var pickerTarget: PhoneContact?
    @State private var phoneFilter = ""
    @State private var resendError: String?

    private var activeContacts: [(pubkey: String, groupNames: String, lastSeen: Date)] {
        var map: [String: (groups: Set<String>, lastSeen: Date)] = [:]

        for (groupID, messages) in appState.chatMessages {
            for message in messages where !message.isMine && !message.isSystemMessage {
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

    private var invitedPending: [InvitedContact] {
        appState.invitedContactStore.contacts.filter { $0.status != .joined }
    }

    private var filteredPhonebook: [PhoneContact] {
        let alreadyInvited = Set(appState.invitedContactStore.contacts.map(\.phone))
        let base = phonebook.filter { !alreadyInvited.contains($0.phone) }
        guard !phoneFilter.isEmpty else { return base }
        let needle = phoneFilter.lowercased()
        return base.filter {
            $0.displayName.lowercased().contains(needle)
                || $0.phone.contains(phoneFilter)
        }
    }

    var body: some View {
        List {
            activeSection
            invitedSection
            phonebookSection
        }
        .navigationTitle("Contacts")
        .sheet(item: $pickerTarget) { contact in
            TelegramInvitePickerView(contact: contact)
        }
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

    // MARK: - Sections

    @ViewBuilder
    private var activeSection: some View {
        if activeContacts.isEmpty && invitedPending.isEmpty && phonebook.isEmpty {
            Section {
                ContentUnavailableView(
                    "No Contacts",
                    systemImage: "person.2",
                    description: Text("People you chat with will appear here. You can also invite friends from your phone contacts.")
                )
            }
        } else if !activeContacts.isEmpty {
            Section("Active") {
                ForEach(activeContacts, id: \.pubkey) { contact in
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
    }

    @ViewBuilder
    private var invitedSection: some View {
        if !invitedPending.isEmpty {
            Section {
                ForEach(invitedPending) { contact in
                    HStack(spacing: 12) {
                        Image(systemName: "paperplane.circle.fill")
                            .font(.system(size: 28))
                            .foregroundStyle(.tint)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(contact.displayName)
                                .font(.body)
                            Text(contact.phone)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        statusBadge(for: contact.status)
                        Button {
                            resendInvite(contact)
                        } label: {
                            Label("Resend", systemImage: "paperplane")
                                .labelStyle(.iconOnly)
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                    }
                    .padding(.vertical, 4)
                    .swipeActions(edge: .leading) {
                        Button {
                            resendInvite(contact)
                        } label: {
                            Label("Resend", systemImage: "paperplane")
                        }
                        .tint(.blue)
                    }
                    .swipeActions(edge: .trailing) {
                        Button(role: .destructive) {
                            appState.invitedContactStore.remove(phone: contact.phone)
                        } label: {
                            Label("Remove", systemImage: "trash")
                        }
                    }
                }
                if let resendError {
                    Text(resendError)
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            } header: {
                Text("Invited")
            } footer: {
                Text("Tap the paperplane to re-send the invite to Telegram.")
                    .font(.caption2)
            }
        }
    }

    @ViewBuilder
    private var phonebookSection: some View {
        Section {
            switch contactsPermission {
            case .notDetermined:
                VStack(alignment: .leading, spacing: 8) {
                    Text("Sync your phone contacts to invite friends to Onym via Telegram. Contacts never leave this device.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                    Button {
                        Task { await requestAndLoad() }
                    } label: {
                        Label("Sync Contacts", systemImage: "person.crop.circle.badge.plus")
                            .fontWeight(.semibold)
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.regular)
                }
                .padding(.vertical, 4)

            case .denied:
                VStack(alignment: .leading, spacing: 8) {
                    Text("Contacts access is off. Enable it in Settings → Onym to sync your phonebook.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                    Button {
                        openSystemSettings()
                    } label: {
                        Label("Open Settings", systemImage: "gearshape")
                    }
                    .buttonStyle(.bordered)
                }
                .padding(.vertical, 4)

            case .authorized:
                if importing {
                    HStack {
                        ProgressView().controlSize(.small)
                        Text("Loading contacts…")
                    }
                } else if phonebook.isEmpty {
                    Button {
                        Task { await loadPhonebook() }
                    } label: {
                        Label("Load Contacts", systemImage: "arrow.clockwise")
                    }
                }
                if let importError {
                    Text(importError)
                        .font(.caption)
                        .foregroundStyle(.red)
                }
                if !filteredPhonebook.isEmpty {
                    TextField("Search", text: $phoneFilter)
                        .textFieldStyle(.plain)
                        .submitLabel(.search)
                    ForEach(filteredPhonebook) { contact in
                        HStack(spacing: 12) {
                            Image(systemName: "person.crop.circle")
                                .font(.system(size: 28))
                                .foregroundStyle(.secondary)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(contact.displayName)
                                    .font(.body)
                                Text(contact.phone)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            Button("Invite") {
                                pickerTarget = contact
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                        }
                        .padding(.vertical, 4)
                    }
                }
            }
        } header: {
            Text("From Phonebook")
        } footer: {
            if contactsPermission == .authorized, !phonebook.isEmpty {
                Text("Tap Invite to open Telegram with an encrypted chat link pre-filled.")
                    .font(.caption2)
            }
        }
    }

    private func statusBadge(for status: InvitedContactStatus) -> some View {
        let (label, color): (String, Color) = {
            switch status {
            case .pending: return ("Pending", .orange)
            case .sent: return ("Sent", .blue)
            case .joined: return ("Joined", .green)
            }
        }()
        return Text(label)
            .font(.caption2)
            .fontWeight(.semibold)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(color.opacity(0.15), in: Capsule())
            .foregroundStyle(color)
    }

    // MARK: - Helpers

    private func resendInvite(_ contact: InvitedContact) {
        resendError = nil
        // Rebuild the URL from current state: identity may have rotated (BIP39
        // restore/regenerate) or group state may have advanced since the original
        // invite was stored. Using the cached URL would point to a stale inbox or
        // stale group snapshot that no longer reconciles.
        let freshURL = rebuildInviteURL(for: contact) ?? contact.inviteURL
        if freshURL != contact.inviteURL {
            var updated = contact
            updated.inviteURL = freshURL
            appState.invitedContactStore.upsert(updated)
        }
        TelegramInviteLauncher.open(
            url: freshURL,
            phone: contact.phone,
            store: appState.invitedContactStore,
            onFailure: { msg in resendError = msg }
        )
    }

    private func rebuildInviteURL(for contact: InvitedContact) -> String? {
        switch contact.inviteKind {
        case .onboard:
            guard let nonce = contact.handshakeNonce else { return nil }
            let inviterHex = appState.keyManager.keyAgreementPublicKeyHex
            return "https://onym.chat/onboard?inviter=\(inviterHex)&nonce=\(nonce)"
        case .group:
            guard let groupID = contact.groupID,
                  let group = appState.groups.first(where: { $0.id == groupID }) else {
                return nil
            }
            let code = InviteCode(
                groupID: group.groupIDData,
                groupSecret: group.groupSecret,
                name: group.name,
                relayHints: group.relayHints.map(\.absoluteString),
                members: group.members,
                epoch: group.epoch,
                salt: group.salt,
                commitment: group.commitment,
                tierRawValue: group.tier.rawValue,
                groupTypeRawValue: group.groupType.rawValue,
                adminPubkeys: Array(group.adminPubkeys)
            ).encode()
            return "https://onym.chat/join?code=\(code)"
        }
    }

    private func requestAndLoad() async {
        let granted = await SystemContactsImporter.requestAccess()
        contactsPermission = granted ? .authorized : .denied
        if granted {
            await loadPhonebook()
        }
    }

    private func loadPhonebook() async {
        importing = true
        importError = nil
        defer { importing = false }
        do {
            phonebook = try SystemContactsImporter.fetchAll()
        } catch {
            importError = "Couldn't load contacts: \(error.localizedDescription)"
        }
    }

    private func openSystemSettings() {
        guard let url = URL(string: UIApplication.openSettingsURLString) else { return }
        UIApplication.shared.open(url)
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
