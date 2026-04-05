import SwiftUI
import SwiftMLS

struct GroupInfoView: View {
    let group: ChatGroup
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var memberToRemove: SEPGroupMemberLeaf?
    @State private var showRemoveConfirmation = false
    @State private var removalStatus: String?
    @State private var removalStatusIsError = false
    @State private var isRemovingMember = false
    @State private var showRenameAlert = false
    @State private var newGroupName = ""

    // Invite member
    @State private var recipientInboxKey = ""
    @State private var inviteStatus: String?
    @State private var showScanner = false

    // Copy invite link
    @State private var inviteLinkCopied = false

    // Epoch pin confirmation
    @State private var epochToPin: UInt64?
    @State private var showPinConfirmation = false

    // Rotate key confirmation
    @State private var showRotateConfirmation = false
    @State private var isRotatingKey = false

    // Fork group
    @State private var showForkSheet = false

    var body: some View {
        NavigationStack {
            List {
                Section("Group") {
                    Button {
                        newGroupName = group.name
                        showRenameAlert = true
                    } label: {
                        LabeledContent("Name") {
                            HStack(spacing: 4) {
                                Text(group.name)
                                Image(systemName: "pencil")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                    .foregroundStyle(.primary)
                    LabeledContent("Epoch") { Text("\(group.epoch)") }
                    LabeledContent("Members") { Text("\(group.members.count)") }
                    LabeledContent("Tier") { Text(group.tier.displayName) }
                    if group.isPublishedOnChain {
                        let removed = !appState.isMember(of: group)
                        LabeledContent("On-Chain") {
                            if removed {
                                HStack(spacing: 4) {
                                    Image(systemName: "exclamationmark.triangle.fill")
                                        .foregroundStyle(.orange)
                                    Text("Diverged")
                                        .font(.caption)
                                        .foregroundStyle(.orange)
                                }
                            } else {
                                Image(systemName: "checkmark.circle.fill")
                                    .foregroundStyle(.green)
                            }
                        }
                    }
                    Toggle("Push Notifications", isOn: Binding(
                        get: { group.pushNotificationsEnabled },
                        set: { enabled in
                            appState.setPushNotifications(enabled: enabled, forGroup: group.id)
                        }
                    ))
                }

                if appState.isMember(of: group) {
                    // MARK: - Invite Member
                    Section {
                        TextField("X25519 public key (hex)", text: $recipientInboxKey)
                            .font(.caption)
                            .monospaced()
                            .autocorrectionDisabled()
                            .textInputAutocapitalization(.never)

                        Button("Paste from Clipboard") {
                            if let text = UIPasteboard.general.string {
                                recipientInboxKey = text.trimmingCharacters(in: .whitespacesAndNewlines)
                            }
                        }

                        Button {
                            showScanner = true
                        } label: {
                            Label("Scan Inbox Key QR", systemImage: "qrcode.viewfinder")
                        }

                        Button {
                            sendInvitation()
                        } label: {
                            Text("Send Invitation")
                        }
                        .disabled(recipientInboxKey.count != 64)

                        if let inviteStatus {
                            Text(inviteStatus)
                                .font(.caption)
                                .foregroundStyle(inviteStatus.hasPrefix("Error") ? .red : .green)
                        }
                    } header: {
                        Text("Invite Member")
                    } footer: {
                        Text("Enter the recipient's inbox key. They can find it in Settings → Advanced.")
                    }

                    // MARK: - Copy Invite Link
                    Section {
                        Button {
                            copyInviteLink()
                        } label: {
                            Label(
                                inviteLinkCopied ? "Link Copied!" : "Copy Invite Link",
                                systemImage: inviteLinkCopied ? "checkmark" : "link"
                            )
                        }
                    } footer: {
                        Text("Copy a deep link that anyone can use to join this group.")
                    }
                }

                Section("Members") {
                    ForEach(group.members, id: \.publicKeyCompressed) { member in
                        HStack {
                            VStack(alignment: .leading) {
                                let pubkeyHex = member.publicKeyCompressed.map { String(format: "%02x", $0) }.joined()
                                Text(pubkeyHex.prefix(16) + "...")
                                    .font(.caption)
                                    .monospaced()

                                if isMyKey(member) {
                                    Text("You")
                                        .font(.caption2)
                                        .foregroundStyle(.blue)
                                }
                            }

                            Spacer()

                            if !isMyKey(member) {
                                Button(role: .destructive) {
                                    memberToRemove = member
                                    showRemoveConfirmation = true
                                } label: {
                                    Image(systemName: "person.badge.minus")
                                }
                                .disabled(isRemovingMember)
                            }
                        }
                    }
                }

                // Epoch History (epoch branching)
                if let snapshots = appState.epochSnapshots[group.id], !snapshots.isEmpty {
                    Section {
                        let currentGroup = appState.groups.first(where: { $0.id == group.id })
                        let pinnedEpoch = currentGroup?.pinnedEpoch
                        let currentSecret = currentGroup?.groupSecret
                        let sorted = snapshots.values.sorted { $0.epoch > $1.epoch }

                        if pinnedEpoch != nil {
                            Button {
                                appState.unpinEpoch(groupID: group.id)
                            } label: {
                                Label("Follow Latest Epoch", systemImage: "arrow.uturn.forward")
                            }
                        }

                        ForEach(sorted, id: \.epoch) { snapshot in
                            let isRekeyBoundary = currentSecret != nil && snapshot.groupSecret != currentSecret!
                            Button {
                                if pinnedEpoch == snapshot.epoch {
                                    appState.unpinEpoch(groupID: group.id)
                                } else {
                                    epochToPin = snapshot.epoch
                                    showPinConfirmation = true
                                }
                            } label: {
                                HStack {
                                    Image(systemName: isRekeyBoundary ? "lock.shield" : "circle")
                                        .font(.caption)
                                        .foregroundStyle(isRekeyBoundary ? .green : .secondary)
                                        .frame(width: 20)
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text("Epoch \(snapshot.epoch)")
                                            .font(.subheadline)
                                            .fontWeight(.medium)
                                        Text(snapshot.changeDescription)
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                        HStack(spacing: 4) {
                                            Text("\(snapshot.members.count) members")
                                            if isRekeyBoundary {
                                                Text("- private branch")
                                                    .foregroundStyle(.green)
                                            }
                                        }
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                    }
                                    Spacer()
                                    if pinnedEpoch == snapshot.epoch {
                                        Image(systemName: "checkmark.circle.fill")
                                            .foregroundStyle(.blue)
                                    }
                                }
                            }
                            .foregroundStyle(.primary)
                        }
                    } header: {
                        Text("Epoch History")
                    } footer: {
                        Text("Switch to a previous epoch to communicate with members who were present at that point. Epochs marked with \(Image(systemName: "lock.shield")) use a different encryption key — only members from that epoch can read messages there.")
                    }
                }

                if appState.isMember(of: group) {
                    Section {
                        Button {
                            showRotateConfirmation = true
                        } label: {
                            HStack {
                                Text("Rotate Group Key")
                                if isRotatingKey {
                                    Spacer()
                                    ProgressView()
                                }
                            }
                        }
                        .disabled(isRotatingKey)
                        Text("Generate a new encryption key without changing membership. Provides forward secrecy.")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }

                // MARK: - Fork Group (removed members only)
                if !appState.isMember(of: group) && appState.canForkGroup(group) {
                    Section {
                        Button {
                            showForkSheet = true
                        } label: {
                            Label("Fork This Group", systemImage: "arrow.triangle.branch")
                        }
                    } header: {
                        Text("Fork")
                    } footer: {
                        Text("Create a new group from a past epoch, excluding members you choose. You will need to invite remaining members to join the fork.")
                    }
                }

                if isRemovingMember {
                    Section {
                        HStack(spacing: 8) {
                            ProgressView()
                            Text("Removing member and updating on-chain state...")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }

                if let status = removalStatus {
                    Section {
                        Text(status)
                            .font(.caption)
                            .foregroundStyle(removalStatusIsError ? .red : .green)
                    }
                }
            }
            .navigationTitle("Group Info")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .confirmationDialog(
                "Remove this member?",
                isPresented: $showRemoveConfirmation,
                titleVisibility: .visible
            ) {
                Button("Remove", role: .destructive) {
                    guard let member = memberToRemove else { return }
                    removeMember(member)
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("The member will be removed and the group key will be rotated. They will not be able to decrypt future messages.")
            }
            .confirmationDialog(
                "Pin to Epoch \(epochToPin ?? 0)?",
                isPresented: $showPinConfirmation,
                titleVisibility: .visible
            ) {
                Button("Pin Epoch") {
                    if let epoch = epochToPin {
                        appState.pinEpoch(groupID: group.id, epoch: epoch)
                        dismiss()
                    }
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("You will switch to viewing and sending messages at this epoch. Other members at the current epoch won't see your messages here.")
            }
            .confirmationDialog(
                "Rotate Group Key?",
                isPresented: $showRotateConfirmation,
                titleVisibility: .visible
            ) {
                Button("Rotate Key") {
                    rotateKey()
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("A new encryption key will be generated for all members. Previous messages remain readable but future messages will use the new key.")
            }
            .alert("Rename Group", isPresented: $showRenameAlert) {
                TextField("Group name", text: $newGroupName)
                Button("Rename") {
                    appState.renameGroup(groupID: group.id, newName: newGroupName)
                    removalStatus = "Group renamed."
                    removalStatusIsError = false
                }
                Button("Cancel", role: .cancel) {}
            }
            .sheet(isPresented: $showScanner) {
                QRScannerView { scannedCode in
                    let trimmed = scannedCode.trimmingCharacters(in: .whitespacesAndNewlines)
                    if trimmed.count == 64, trimmed.allSatisfy({ $0.isHexDigit }) {
                        recipientInboxKey = trimmed
                    }
                    showScanner = false
                }
            }
            .sheet(isPresented: $showForkSheet) {
                ForkGroupView(originalGroup: group)
            }
        }
    }

    private func isMyKey(_ member: SEPGroupMemberLeaf) -> Bool {
        guard let myLeaf = try? appState.keyManager.memberLeaf else { return false }
        return member.publicKeyCompressed == myLeaf.publicKeyCompressed
    }

    private func removeMember(_ member: SEPGroupMemberLeaf) {
        isRemovingMember = true
        removalStatus = nil
        Task {
            do {
                try await appState.removeMember(blsPubkey: member.publicKeyCompressed, from: group.id)
                let epoch = appState.groups.first(where: { $0.id == group.id })?.epoch ?? 0
                removalStatus = "Member removed. Key rotated to epoch \(epoch)."
                removalStatusIsError = false
            } catch {
                removalStatus = error.localizedDescription
                removalStatusIsError = true
            }
            isRemovingMember = false
        }
    }

    private func copyInviteLink() {
        let code = InviteCode(
            groupID: group.groupIDData,
            groupSecret: group.groupSecret,
            name: group.name,
            relayHints: group.relayHints.map(\.absoluteString),
            members: group.members,
            epoch: group.epoch,
            salt: group.salt,
            commitment: group.commitment,
            tierRawValue: group.tier.rawValue
        )
        UIPasteboard.general.string = "onym://join?code=\(code.encode())"
        inviteLinkCopied = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) { inviteLinkCopied = false }
    }

    private func sendInvitation() {
        guard let recipientPubkeyData = hexToData(recipientInboxKey), recipientPubkeyData.count == 32 else {
            inviteStatus = "Error: Invalid inbox key. Must be 64 hex characters (32 bytes)."
            return
        }

        inviteStatus = nil

        // Fire-and-forget: validate synchronously, send in background
        let payload = BootstrapPayload.from(
            group: group,
            senderPubkey: appState.keyManager.publicKeyHex
        )
        Task {
            await appState.invitationTransport.connect(to: appState.relayURLs)
            try? await appState.invitationTransport.sendInvitation(
                payload: payload,
                recipientKeyAgreementPubkey: recipientPubkeyData,
                keyManager: appState.keyManager
            )
        }
        inviteStatus = "Invitation sent!"
    }

    private func rotateKey() {
        isRotatingKey = true
        Task {
            await appState.rotateGroupKey(groupID: group.id)
            isRotatingKey = false
            dismiss()
        }
    }

    private func hexToData(_ hex: String) -> Data? {
        guard hex.count % 2 == 0 else { return nil }
        let bytes = stride(from: 0, to: hex.count, by: 2).compactMap { i -> UInt8? in
            let start = hex.index(hex.startIndex, offsetBy: i)
            let end = hex.index(start, offsetBy: 2)
            return UInt8(hex[start..<end], radix: 16)
        }
        guard bytes.count == hex.count / 2 else { return nil }
        return Data(bytes)
    }
}

private extension SEPTier {
    var displayName: String {
        switch self {
        case .small: return "Small (up to 32)"
        case .medium: return "Medium (up to 256)"
        case .large: return "Large (up to 2,048)"
        @unknown default: return "Unknown"
        }
    }
}
