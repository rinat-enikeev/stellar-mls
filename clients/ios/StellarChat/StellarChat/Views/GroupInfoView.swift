import PhotosUI
import SwiftUI
import SwiftMLS

struct GroupInfoView: View {
    let group: ChatGroup
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    // Rename
    @State private var showRenameAlert = false
    @State private var newGroupName = ""

    // Member removal
    @State private var memberToRemove: SEPGroupMemberLeaf?
    @State private var showRemoveConfirmation = false
    @State private var removalStatus: String?
    @State private var removalStatusIsError = false
    @State private var isRemovingMember = false

    // Alias
    @State private var aliasTarget: String?
    @State private var aliasText = ""

    // Epoch pin
    @State private var epochToPin: UInt64?
    @State private var showPinConfirmation = false
    @State private var showEpochHistory = false

    // Rotate key
    @State private var showRotateConfirmation = false
    @State private var isRotatingKey = false

    // Leave
    @State private var showLeaveConfirmation = false
    @State private var isLeaving = false

    // Fork
    @State private var showForkSheet = false

    // Invite
    @State private var showInviteSheet = false
    @State private var recipientInboxKey = ""
    @State private var inviteStatus: String?
    @State private var showScanner = false
    @State private var inviteLinkCopied = false

    // Search
    @State private var showSearch = false

    // Avatar
    @State private var selectedAvatarItem: PhotosPickerItem?

    private var currentGroup: ChatGroup {
        appState.groups.first(where: { $0.id == group.id }) ?? group
    }

    private var isMember: Bool { appState.isMember(of: currentGroup) }

    private var inviteLinkString: String {
        let code = InviteCode(
            groupID: currentGroup.groupIDData,
            groupSecret: currentGroup.groupSecret,
            name: currentGroup.name,
            relayHints: currentGroup.relayHints.map(\.absoluteString),
            members: currentGroup.members,
            epoch: currentGroup.epoch,
            salt: currentGroup.salt,
            commitment: currentGroup.commitment,
            tierRawValue: currentGroup.tier.rawValue,
            groupTypeRawValue: currentGroup.groupType.rawValue,
            adminPubkeys: Array(currentGroup.adminPubkeys)
        )
        return "https://onym.chat/join?code=\(code.encode())"
    }

    var body: some View {
        NavigationStack {
            List {
                heroSection
                if isMember {
                    quickActionsSection
                } else {
                    divergedBannerSection
                }
                membersSection
                if isMember {
                    notificationsSection
                    securitySection
                    dangerZoneSection
                } else {
                    forkOptionsSection
                }
                statusSection
            }
            .listStyle(.insetGrouped)
            .navigationTitle("")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") { dismiss() }
                }
                if isMember {
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Edit") {
                            newGroupName = currentGroup.name
                            showRenameAlert = true
                        }
                        .fontWeight(.semibold)
                    }
                }
            }
            // Invite sheet
            .sheet(isPresented: $showInviteSheet) {
                InviteGroupSheet(
                    groupName: currentGroup.name,
                    inviteLink: inviteLinkString,
                    recipientInboxKey: $recipientInboxKey,
                    inviteStatus: $inviteStatus,
                    onSendInvite: sendInvitation,
                    onScanQR: { showInviteSheet = false; showScanner = true },
                    onPasteKey: {
                        if let text = UIPasteboard.general.string {
                            recipientInboxKey = text.trimmingCharacters(in: .whitespacesAndNewlines)
                        }
                    }
                )
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
            }
            .sheet(isPresented: $showScanner) {
                QRScannerView { scannedCode in
                    let trimmed = scannedCode.trimmingCharacters(in: .whitespacesAndNewlines)
                    if trimmed.count == 64, trimmed.allSatisfy({ $0.isHexDigit }) {
                        recipientInboxKey = trimmed
                        sendInvitation()
                    }
                    showScanner = false
                    showInviteSheet = true
                }
            }
            .sheet(isPresented: $showSearch) {
                SearchView()
            }
            .sheet(isPresented: $showForkSheet) {
                ForkGroupView(originalGroup: currentGroup)
            }
            // Alerts
            .alert("Rename Group", isPresented: $showRenameAlert) {
                TextField("Group name", text: $newGroupName)
                Button("Rename") {
                    appState.renameGroup(groupID: currentGroup.id, newName: newGroupName)
                }
                Button("Cancel", role: .cancel) {}
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
                if aliasTarget.flatMap({ appState.contactAliasStore.displayName(for: $0) }) != nil {
                    Button("Remove Name", role: .destructive) {
                        if let pubkey = aliasTarget {
                            appState.contactAliasStore.removeAlias(pubkey: pubkey)
                        }
                        aliasTarget = nil
                    }
                }
                Button("Cancel", role: .cancel) { aliasTarget = nil }
            } message: {
                if let pubkey = aliasTarget {
                    Text(String(pubkey.prefix(16)) + "...")
                }
            }
            .confirmationDialog("Remove this member?", isPresented: $showRemoveConfirmation, titleVisibility: .visible) {
                Button("Remove", role: .destructive) {
                    if let m = memberToRemove { removeMember(m) }
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("The member will be removed and the group key will be rotated.")
            }
            .confirmationDialog("Rotate Group Key?", isPresented: $showRotateConfirmation, titleVisibility: .visible) {
                Button("Rotate Key") { rotateKey() }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("A new encryption key will be generated for all members. Previous messages remain readable.")
            }
            .confirmationDialog("Pin to Epoch \(epochToPin ?? 0)?", isPresented: $showPinConfirmation, titleVisibility: .visible) {
                Button("Pin Epoch") {
                    if let epoch = epochToPin {
                        appState.pinEpoch(groupID: currentGroup.id, epoch: epoch)
                        dismiss()
                    }
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("You will switch to viewing this epoch. Other members at the current epoch won't see your messages here.")
            }
            .confirmationDialog("Leave Group?", isPresented: $showLeaveConfirmation, titleVisibility: .visible) {
                Button("Leave Group", role: .destructive) { leaveGroup() }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("You'll no longer receive messages. The group key will be rotated so future messages stay private from you.")
            }
        }
    }

    // MARK: - Section builders

    @ViewBuilder
    private var heroSection: some View {
        Section {
            hero
                .listRowInsets(EdgeInsets())
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
        }
    }

    @ViewBuilder
    private var quickActionsSection: some View {
        Section {
            quickActions
                .listRowInsets(EdgeInsets(top: 0, leading: 8, bottom: 8, trailing: 8))
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
        }
    }

    @ViewBuilder
    private var divergedBannerSection: some View {
        Section {
            divergedBanner
                .listRowInsets(EdgeInsets(top: 4, leading: 16, bottom: 8, trailing: 16))
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
        }
    }

    @ViewBuilder
    private var addPeopleRow: some View {
        Button {
            showInviteSheet = true
        } label: {
            HStack(spacing: 12) {
                ZStack {
                    Circle().fill(Color.accentColor.opacity(0.14))
                    Image(systemName: "plus")
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(Color.accentColor)
                }
                .frame(width: 40, height: 40)
                Text("Add people")
                    .font(.body)
                    .fontWeight(.medium)
                    .foregroundStyle(Color.accentColor)
                Spacer()
                Image(systemName: "chevron.right")
                    .font(.footnote)
                    .foregroundStyle(.tertiary)
            }
        }
    }

    @ViewBuilder
    private var membersSection: some View {
        Section {
            if isMember && currentGroup.groupType != .oneOnOne && canInviteMembers {
                addPeopleRow
            }
            ForEach(currentGroup.members, id: \.publicKeyCompressed) { member in
                memberListRow(for: member)
            }
        } header: {
            membersSectionHeader
        } footer: {
            membersSectionFooter
        }
    }

    private var membersSectionHeader: some View {
        HStack {
            Text("Members")
            Spacer()
            Text("\(currentGroup.members.count) of \(currentGroup.tier.memberCap)")
                .textCase(.none)
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private var membersSectionFooter: some View {
        if isMember {
            Text("Swipe a member to remove. Tap to set a nickname.")
        } else {
            Text("Members at epoch \(currentGroup.epoch). You won't see members added after you were removed.")
        }
    }

    @ViewBuilder
    private func memberListRow(for member: SEPGroupMemberLeaf) -> some View {
        let isOligarchyAdmin = currentGroup.groupType == .oligarchy &&
            currentGroup.adminPubkeys.contains(hexKey(member))
        MemberRow(
            member: member,
            isMe: isMyKey(member),
            isAdmin: isOligarchyAdmin,
            alias: appState.contactAliasStore.displayName(for: hexKey(member))
        )
        .contentShape(Rectangle())
        .onTapGesture {
            let pubkey = hexKey(member)
            aliasText = appState.contactAliasStore.displayName(for: pubkey) ?? ""
            aliasTarget = pubkey
        }
        .swipeActions(edge: .trailing, allowsFullSwipe: false) {
            if isMember && !isMyKey(member) {
                if currentGroup.groupType == .democracy {
                    Button {
                        appState.proposeRemovalVote(
                            groupID: currentGroup.id,
                            targetPubkeyHex: hexKey(member)
                        )
                    } label: {
                        Label("Propose vote", systemImage: "hand.raised")
                    }
                    .tint(.purple)
                } else if currentGroup.groupType == .oligarchy {
                    // Only existing admins can kick in Oligarchy groups.
                    if iAmOligarchyAdmin {
                        Button(role: .destructive) {
                            memberToRemove = member
                            showRemoveConfirmation = true
                        } label: {
                            Label("Remove", systemImage: "person.crop.circle.badge.minus")
                        }
                        .disabled(isRemovingMember)
                        if isOligarchyAdmin {
                            Button {
                                appState.demoteFromAdmin(groupID: currentGroup.id, targetPubkeyHex: hexKey(member))
                            } label: {
                                Label("Demote", systemImage: "shield.slash")
                            }
                            .tint(.orange)
                        } else {
                            Button {
                                appState.promoteToAdmin(groupID: currentGroup.id, targetPubkeyHex: hexKey(member))
                            } label: {
                                Label("Promote", systemImage: "shield.lefthalf.filled")
                            }
                            .tint(.blue)
                        }
                    }
                } else if currentGroup.groupType != .oneOnOne {
                    Button(role: .destructive) {
                        memberToRemove = member
                        showRemoveConfirmation = true
                    } label: {
                        Label("Remove", systemImage: "person.crop.circle.badge.minus")
                    }
                    .disabled(isRemovingMember)
                }
            }
        }
    }

    private var iAmOligarchyAdmin: Bool {
        guard currentGroup.groupType == .oligarchy,
              let myLeaf = try? appState.keyManager.memberLeaf else { return false }
        let myHex = myLeaf.publicKeyCompressed.map { String(format: "%02x", $0) }.joined()
        return currentGroup.adminPubkeys.contains(myHex)
    }

    /// In Oligarchy groups, only admins can invite. Other governance types allow any member.
    private var canInviteMembers: Bool {
        guard isMember else { return false }
        if currentGroup.groupType == .oligarchy {
            return iAmOligarchyAdmin
        }
        return true
    }

    @ViewBuilder
    private var notificationsSection: some View {
        Section("Notifications") {
            HStack(spacing: 12) {
                SettingsIconBox(systemImage: "bell.fill", background: .orange)
                VStack(alignment: .leading, spacing: 1) {
                    Text("Push notifications")
                    Text("New messages and key events")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Toggle("", isOn: pushNotificationsBinding)
                    .labelsHidden()
            }
        }
    }

    private var pushNotificationsBinding: Binding<Bool> {
        Binding(
            get: { currentGroup.pushNotificationsEnabled },
            set: { enabled in
                appState.setPushNotifications(enabled: enabled, forGroup: currentGroup.id)
            }
        )
    }

    @ViewBuilder
    private var securitySection: some View {
        Section {
            if currentGroup.isPublishedOnChain {
                anchoredRow
            }
            rotateKeyRow
            if let snapshots = appState.epochSnapshots[currentGroup.id], !snapshots.isEmpty {
                epochHistoryDisclosure(snapshots: snapshots)
            }
        } header: {
            Text("Security")
        } footer: {
            Text("Each key change creates a new epoch. You can time-travel to read messages from a past epoch.")
        }
    }

    @ViewBuilder
    private var anchoredRow: some View {
        HStack(spacing: 12) {
            SettingsIconBox(systemImage: "checkmark.seal.fill", background: .green)
            VStack(alignment: .leading, spacing: 1) {
                Text("Anchored on Stellar")
                Text("Verified · epoch \(currentGroup.epoch)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
        }
    }

    @ViewBuilder
    private var rotateKeyRow: some View {
        Button {
            showRotateConfirmation = true
        } label: {
            HStack(spacing: 12) {
                SettingsIconBox(systemImage: "key.fill", background: .blue)
                VStack(alignment: .leading, spacing: 1) {
                    Text("Rotate encryption key")
                        .foregroundStyle(.primary)
                    Text("Forward secrecy · keeps all members")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                if isRotatingKey {
                    ProgressView()
                } else {
                    Image(systemName: "chevron.right")
                        .font(.footnote)
                        .foregroundStyle(.tertiary)
                }
            }
        }
        .disabled(isRotatingKey)
    }

    @ViewBuilder
    private var dangerZoneSection: some View {
        Section {
            Button(role: .destructive) {
                showLeaveConfirmation = true
            } label: {
                HStack(spacing: 12) {
                    Image(systemName: "rectangle.portrait.and.arrow.right")
                        .font(.system(size: 17, weight: .regular))
                        .frame(width: 30, height: 30)
                        .foregroundStyle(.red)
                    Text("Leave Group")
                        .foregroundStyle(.red)
                    Spacer()
                    if isLeaving { ProgressView() }
                }
            }
            .disabled(isLeaving)
        }
    }

    @ViewBuilder
    private var forkOptionsSection: some View {
        if appState.canForkGroup(currentGroup) {
            Section {
                Button {
                    showForkSheet = true
                } label: {
                    HStack(spacing: 12) {
                        SettingsIconBox(systemImage: "arrow.triangle.branch", background: .indigo)
                        VStack(alignment: .leading, spacing: 1) {
                            Text("Fork this group")
                                .foregroundStyle(.primary)
                            Text("Start fresh with members you choose")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        Image(systemName: "chevron.right")
                            .font(.footnote)
                            .foregroundStyle(.tertiary)
                    }
                }
                if let snapshots = appState.epochSnapshots[currentGroup.id], !snapshots.isEmpty {
                    epochHistoryDisclosure(snapshots: snapshots)
                }
            } header: {
                Text("Options")
            } footer: {
                Text("Create a new group from a past epoch, excluding members you choose.")
            }
        }
    }

    @ViewBuilder
    private var statusSection: some View {
        if let status = removalStatus {
            Section {
                Text(status)
                    .font(.caption)
                    .foregroundStyle(removalStatusIsError ? .red : .green)
                    .listRowBackground(Color.clear)
            }
        }
    }

    // MARK: - Hero

    private var hero: some View {
        let (c1, c2) = groupGradient(seed: currentGroup.id)
        let avatarUIImage: UIImage? = currentGroup.avatarData.flatMap { UIImage(data: $0) }
        return VStack(spacing: 14) {
            ZStack {
                // soft halo
                Circle()
                    .fill(c1.opacity(0.18))
                    .frame(width: 240, height: 240)
                    .blur(radius: 40)
                    .offset(y: -30)
                // avatar — tap anywhere to pick a new photo
                PhotosPicker(selection: $selectedAvatarItem, matching: .images) {
                    ZStack(alignment: .bottomTrailing) {
                        Group {
                            if let avatarUIImage {
                                Image(uiImage: avatarUIImage)
                                    .resizable()
                                    .scaledToFill()
                                    .frame(width: 96, height: 96)
                                    .clipShape(Circle())
                            } else {
                                Circle()
                                    .fill(
                                        LinearGradient(colors: [c1, c2], startPoint: .topLeading, endPoint: .bottomTrailing)
                                    )
                                    .frame(width: 96, height: 96)
                                    .overlay(
                                        Text(initials(currentGroup.name))
                                            .font(.system(size: 38, weight: .semibold))
                                            .foregroundStyle(.white)
                                            .minimumScaleFactor(0.6)
                                    )
                            }
                        }
                        .shadow(color: c1.opacity(0.35), radius: 14, y: 8)
                        if isMember {
                            ZStack {
                                Circle()
                                    .fill(Color(.systemGroupedBackground))
                                    .frame(width: 30, height: 30)
                                Image(systemName: "pencil")
                                    .font(.system(size: 14, weight: .semibold))
                                    .foregroundStyle(Color.accentColor)
                            }
                            .overlay(
                                Circle().stroke(Color(.systemGroupedBackground), lineWidth: 3)
                            )
                            .offset(x: 2, y: 2)
                        }
                    }
                }
                .buttonStyle(.plain)
                .disabled(!isMember)
                .onChange(of: selectedAvatarItem) { _, newItem in
                    guard let newItem else { return }
                    Task {
                        if let data = try? await newItem.loadTransferable(type: Data.self),
                           let image = UIImage(data: data) {
                            let processed = Self.downscaleAvatar(image)
                            appState.setGroupAvatar(groupID: currentGroup.id, avatarData: processed.data)
                        }
                        selectedAvatarItem = nil
                    }
                }
            }
            .padding(.top, 20)

            Text(currentGroup.name)
                .font(.system(size: 26, weight: .bold))
                .multilineTextAlignment(.center)
                .padding(.horizontal, 20)

            HStack(spacing: 6) {
                chip(icon: "person.2.fill", text: "\(currentGroup.members.count) members", tint: .secondary)
                chip(icon: "lock.fill", text: "End-to-end encrypted", tint: .green, bgAlpha: 0.14)
                if currentGroup.isPublishedOnChain && isMember {
                    chip(icon: "checkmark.seal.fill", text: "Anchored on Stellar", tint: .green, bgAlpha: 0.14)
                }
            }
            .font(.caption)
            .padding(.horizontal, 16)
        }
        .frame(maxWidth: .infinity)
        .padding(.bottom, 14)
    }

    /// Downscale the picked image so the long edge is at most 256 px and re-encode
    /// as JPEG quality 0.85 so the persisted row stays small.
    private static func downscaleAvatar(_ source: UIImage) -> (image: UIImage, data: Data) {
        let maxEdge: CGFloat = 256
        let longest = max(source.size.width, source.size.height)
        let scale = longest > maxEdge ? maxEdge / longest : 1
        let targetSize = CGSize(
            width: source.size.width * scale,
            height: source.size.height * scale
        )
        let renderer = UIGraphicsImageRenderer(size: targetSize)
        let resized = renderer.image { _ in
            source.draw(in: CGRect(origin: .zero, size: targetSize))
        }
        let data = resized.jpegData(compressionQuality: 0.85) ?? Data()
        return (resized, data)
    }

    private func chip(icon: String, text: String, tint: Color, bgAlpha: Double = 0.06) -> some View {
        HStack(spacing: 4) {
            Image(systemName: icon)
                .font(.system(size: 11, weight: .semibold))
            Text(text)
                .font(.caption)
                .fontWeight(.medium)
        }
        .foregroundStyle(tint)
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(
            Capsule().fill(tint.opacity(bgAlpha))
        )
    }

    // MARK: - Quick actions

    private var quickActions: some View {
        HStack(spacing: 10) {
            if currentGroup.groupType == .oneOnOne {
                QuickActionTile(
                    icon: "lock.fill",
                    label: "Sealed",
                    subtitle: "1v1",
                    tint: .gray
                ) {}
            } else {
                QuickActionTile(
                    icon: "square.and.arrow.up",
                    label: "Share",
                    subtitle: "Invite link",
                    tint: .accentColor
                ) {
                    showInviteSheet = true
                }
            }
            QuickActionTile(
                icon: currentGroup.pushNotificationsEnabled ? "bell.fill" : "bell.slash.fill",
                label: currentGroup.pushNotificationsEnabled ? "Muted" : "Unmute",
                subtitle: currentGroup.pushNotificationsEnabled ? "On" : "Off",
                tint: currentGroup.pushNotificationsEnabled ? .orange : .gray,
                invertedLabel: true
            ) {
                appState.setPushNotifications(
                    enabled: !currentGroup.pushNotificationsEnabled,
                    forGroup: currentGroup.id
                )
            }
            QuickActionTile(
                icon: "magnifyingglass",
                label: "Search",
                subtitle: "Messages",
                tint: .gray
            ) {
                showSearch = true
            }
        }
    }

    // MARK: - Diverged banner

    private var divergedBanner: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.orange)
                .font(.title3)
            VStack(alignment: .leading, spacing: 4) {
                Text("You were removed from this group")
                    .font(.subheadline)
                    .fontWeight(.semibold)
                    .foregroundStyle(.orange)
                Text("You can still read messages from epoch \(currentGroup.epoch), but won't see anything posted after that. Fork this epoch to continue with the members you choose.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 14)
                .fill(Color.orange.opacity(0.12))
        )
    }

    // MARK: - Epoch History disclosure

    @ViewBuilder
    private func epochHistoryDisclosure(snapshots: [UInt64: EpochSnapshot]) -> some View {
        let sorted = snapshots.values.sorted { $0.epoch > $1.epoch }
        let pinnedEpoch = currentGroup.pinnedEpoch

        DisclosureGroup(isExpanded: $showEpochHistory) {
            VStack(alignment: .leading, spacing: 0) {
                if pinnedEpoch != nil {
                    Button {
                        appState.unpinEpoch(groupID: currentGroup.id)
                    } label: {
                        Label("Follow Latest Epoch", systemImage: "arrow.uturn.forward")
                            .font(.footnote)
                            .padding(.vertical, 8)
                    }
                }
                ForEach(Array(sorted.enumerated()), id: \.element.epoch) { idx, snapshot in
                    EpochTimelineRow(
                        epoch: snapshot.epoch,
                        isCurrent: snapshot.epoch == currentGroup.epoch,
                        isPinned: snapshot.epoch == pinnedEpoch,
                        isRekeyBoundary: snapshot.groupSecret != currentGroup.groupSecret,
                        changeDescription: snapshot.changeDescription,
                        memberCount: snapshot.members.count,
                        isLast: idx == sorted.count - 1
                    )
                    .contentShape(Rectangle())
                    .onTapGesture {
                        if pinnedEpoch == snapshot.epoch {
                            appState.unpinEpoch(groupID: currentGroup.id)
                        } else {
                            epochToPin = snapshot.epoch
                            showPinConfirmation = true
                        }
                    }
                }
            }
            .padding(.top, 4)
        } label: {
            HStack(spacing: 12) {
                SettingsIconBox(systemImage: "clock.fill", background: .indigo)
                VStack(alignment: .leading, spacing: 1) {
                    Text("Epoch history")
                    Text("\(sorted.count) \(sorted.count == 1 ? "epoch" : "epochs") · current is epoch \(currentGroup.epoch)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    // MARK: - Row helper

    @ViewBuilder
    private func row<Icon: View, Trailing: View>(
        icon: Icon,
        title: String,
        subtitle: String? = nil,
        @ViewBuilder trailing: () -> Trailing = { EmptyView() }
    ) -> some View {
        HStack(spacing: 12) {
            icon
            VStack(alignment: .leading, spacing: 1) {
                Text(title)
                    .foregroundStyle(.primary)
                if let subtitle {
                    Text(subtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer()
            trailing()
        }
    }

    @ViewBuilder
    private func row<Icon: View, Trailing: View>(
        icon: Icon,
        title: String,
        subtitle: String? = nil,
        trailing: Trailing
    ) -> some View {
        row(icon: icon, title: title, subtitle: subtitle) { trailing }
    }

    // MARK: - Helpers

    private func isMyKey(_ member: SEPGroupMemberLeaf) -> Bool {
        guard let myLeaf = try? appState.keyManager.memberLeaf else { return false }
        return member.publicKeyCompressed == myLeaf.publicKeyCompressed
    }

    /// Creator of the group is the first member at epoch 0.
    private func isCreator(_ member: SEPGroupMemberLeaf) -> Bool {
        guard let snapshots = appState.epochSnapshots[currentGroup.id],
              let zero = snapshots[0],
              let first = zero.members.first else { return false }
        return first.publicKeyCompressed == member.publicKeyCompressed
    }

    private func hexKey(_ member: SEPGroupMemberLeaf) -> String {
        member.publicKeyCompressed.map { String(format: "%02x", $0) }.joined()
    }

    private func removeMember(_ member: SEPGroupMemberLeaf) {
        isRemovingMember = true
        removalStatus = nil
        Task {
            do {
                try await appState.removeMember(blsPubkey: member.publicKeyCompressed, from: currentGroup.id)
                let epoch = appState.groups.first(where: { $0.id == currentGroup.id })?.epoch ?? 0
                removalStatus = "Member removed. Key rotated to epoch \(epoch)."
                removalStatusIsError = false
            } catch {
                removalStatus = error.localizedDescription
                removalStatusIsError = true
            }
            isRemovingMember = false
        }
    }

    private func leaveGroup() {
        guard let myLeaf = try? appState.keyManager.memberLeaf else { return }
        isLeaving = true
        Task {
            do {
                try await appState.removeMember(blsPubkey: myLeaf.publicKeyCompressed, from: currentGroup.id)
            } catch {
                removalStatus = error.localizedDescription
                removalStatusIsError = true
            }
            isLeaving = false
            dismiss()
        }
    }

    private func sendInvitation() {
        guard let recipientPubkeyData = hexToData(recipientInboxKey), recipientPubkeyData.count == 32 else {
            inviteStatus = "Error: Invalid inbox key. Must be 64 hex characters (32 bytes)."
            return
        }
        inviteStatus = nil
        let payload = BootstrapPayload.from(
            group: currentGroup,
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
            await appState.rotateGroupKey(groupID: currentGroup.id)
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

    @ViewBuilder
    private func statusBanner(status: String, isError: Bool) -> some View {
        EmptyView()
    }
}

// MARK: - Member row

private struct MemberRow: View {
    let member: SEPGroupMemberLeaf
    let isMe: Bool
    let isAdmin: Bool
    let alias: String?

    private var hex: String {
        member.publicKeyCompressed.map { String(format: "%02x", $0) }.joined()
    }
    private var displayName: String {
        if let alias, !alias.isEmpty { return alias }
        if isMe { return "You" }
        return String(hex.prefix(8)) + "…" + String(hex.suffix(4))
    }
    private var subtitle: String {
        let short = String(hex.prefix(10)) + "…"
        return isMe ? "\(short) · You" : short
    }

    var body: some View {
        HStack(spacing: 12) {
            GradientAvatar(seed: hex, name: displayName, size: 40)
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 6) {
                    Text(displayName)
                        .font(.body)
                        .fontWeight(.semibold)
                        .lineLimit(1)
                    if isAdmin {
                        HStack(spacing: 3) {
                            Image(systemName: "crown.fill")
                                .font(.system(size: 9, weight: .semibold))
                            Text("ADMIN")
                                .font(.system(size: 10, weight: .semibold))
                        }
                        .foregroundStyle(.orange)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .background(
                            RoundedRectangle(cornerRadius: 4)
                                .fill(Color.orange.opacity(0.14))
                        )
                    }
                }
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer()
        }
    }
}

// MARK: - Gradient avatar

struct GradientAvatar: View {
    let seed: String
    let name: String
    let size: CGFloat

    var body: some View {
        let (c1, c2) = groupGradient(seed: seed)
        return Circle()
            .fill(LinearGradient(colors: [c1, c2], startPoint: .topLeading, endPoint: .bottomTrailing))
            .frame(width: size, height: size)
            .overlay(
                Text(initials(name))
                    .font(.system(size: size * 0.38, weight: .semibold))
                    .foregroundStyle(.white)
                    .minimumScaleFactor(0.6)
            )
    }
}

// MARK: - Quick action tile

private struct QuickActionTile: View {
    let icon: String
    let label: String
    let subtitle: String
    let tint: Color
    var invertedLabel: Bool = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 6) {
                ZStack {
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .fill(tint)
                    Image(systemName: icon)
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(.white)
                }
                .frame(width: 36, height: 36)
                Text(label)
                    .font(.footnote)
                    .fontWeight(.medium)
                    .foregroundStyle(.primary)
                Text(subtitle)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 12)
            .padding(.horizontal, 8)
            .background(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .fill(Color(.secondarySystemGroupedBackground))
            )
        }
        .buttonStyle(.plain)
    }
}

// MARK: - Epoch timeline row

private struct EpochTimelineRow: View {
    let epoch: UInt64
    let isCurrent: Bool
    let isPinned: Bool
    let isRekeyBoundary: Bool
    let changeDescription: String
    let memberCount: Int
    let isLast: Bool

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            VStack(spacing: 0) {
                ZStack {
                    Circle()
                        .fill(isCurrent ? Color.accentColor.opacity(0.25) : Color.secondary.opacity(0.2))
                        .frame(width: 16, height: 16)
                    Circle()
                        .fill(isCurrent ? Color.accentColor : Color(.secondaryLabel))
                        .frame(width: 8, height: 8)
                }
                if !isLast {
                    Rectangle()
                        .fill(Color(.separator))
                        .frame(width: 1.5)
                        .frame(maxHeight: .infinity)
                }
            }
            .frame(width: 18)
            .padding(.top, 2)

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text("Epoch \(epoch)")
                        .font(.subheadline)
                        .fontWeight(.semibold)
                    if isCurrent {
                        Text("CURRENT")
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(Color.accentColor)
                    }
                    if isPinned {
                        Image(systemName: "pin.fill")
                            .font(.caption2)
                            .foregroundStyle(Color.accentColor)
                    }
                    if isRekeyBoundary {
                        Image(systemName: "lock.shield.fill")
                            .font(.caption2)
                            .foregroundStyle(.green)
                    }
                }
                Text(changeDescription)
                    .font(.caption)
                    .foregroundStyle(.primary)
                Text("\(memberCount) \(memberCount == 1 ? "member" : "members")")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .padding(.bottom, isLast ? 0 : 12)
            Spacer()
        }
    }
}

// MARK: - Invite sheet

private struct InviteGroupSheet: View {
    let groupName: String
    let inviteLink: String
    @Binding var recipientInboxKey: String
    @Binding var inviteStatus: String?
    let onSendInvite: () -> Void
    let onScanQR: () -> Void
    let onPasteKey: () -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var linkCopied = false
    @State private var showAdvanced = false

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 16) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Invite to \(groupName)")
                            .font(.title2)
                            .fontWeight(.bold)
                        Text("Anyone with this link can join. It's safe — messages stay end-to-end encrypted.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)

                    // Link card
                    HStack(spacing: 12) {
                        Image(systemName: "link")
                            .foregroundStyle(Color.accentColor)
                        Text(shortLink)
                            .font(.footnote)
                            .monospaced()
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Spacer()
                        Button {
                            UIPasteboard.general.string = inviteLink
                            linkCopied = true
                            DispatchQueue.main.asyncAfter(deadline: .now() + 2) { linkCopied = false }
                        } label: {
                            Text(linkCopied ? "Copied" : "Copy")
                                .font(.footnote)
                                .fontWeight(.semibold)
                                .foregroundStyle(.white)
                                .padding(.horizontal, 12)
                                .padding(.vertical, 6)
                                .background(Capsule().fill(Color.accentColor))
                        }
                        .buttonStyle(.plain)
                    }
                    .padding(14)
                    .background(
                        RoundedRectangle(cornerRadius: 14)
                            .fill(Color(.secondarySystemGroupedBackground))
                    )

                    HStack(spacing: 10) {
                        ShareLink(item: inviteLink) {
                            HStack(spacing: 8) {
                                Image(systemName: "square.and.arrow.up")
                                Text("Share link…")
                                    .fontWeight(.semibold)
                            }
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 14)
                            .background(
                                RoundedRectangle(cornerRadius: 14)
                                    .fill(Color.accentColor)
                            )
                            .foregroundStyle(.white)
                        }
                        .buttonStyle(.plain)
                    }

                    // Advanced
                    DisclosureGroup(isExpanded: $showAdvanced) {
                        VStack(spacing: 10) {
                            HStack(spacing: 10) {
                                Button(action: onPasteKey) {
                                    Label("Paste", systemImage: "doc.on.clipboard")
                                        .frame(maxWidth: .infinity)
                                        .padding(.vertical, 10)
                                        .background(
                                            RoundedRectangle(cornerRadius: 12)
                                                .stroke(Color(.separator), lineWidth: 0.5)
                                        )
                                }
                                .buttonStyle(.plain)
                                Button(action: onScanQR) {
                                    Label("Scan QR", systemImage: "qrcode.viewfinder")
                                        .frame(maxWidth: .infinity)
                                        .padding(.vertical, 10)
                                        .background(
                                            RoundedRectangle(cornerRadius: 12)
                                                .stroke(Color(.separator), lineWidth: 0.5)
                                        )
                                }
                                .buttonStyle(.plain)
                            }
                            TextField("X25519 inbox key (64 hex chars)", text: $recipientInboxKey)
                                .font(.footnote)
                                .monospaced()
                                .autocorrectionDisabled()
                                .textInputAutocapitalization(.never)
                                .padding(10)
                                .background(
                                    RoundedRectangle(cornerRadius: 10)
                                        .fill(Color(.tertiarySystemFill))
                                )
                            Button {
                                onSendInvite()
                            } label: {
                                Text("Send invitation")
                                    .fontWeight(.semibold)
                                    .frame(maxWidth: .infinity)
                                    .padding(.vertical, 12)
                                    .background(
                                        RoundedRectangle(cornerRadius: 12)
                                            .fill(recipientInboxKey.count == 64 ? Color.accentColor : Color.gray.opacity(0.3))
                                    )
                                    .foregroundStyle(.white)
                            }
                            .buttonStyle(.plain)
                            .disabled(recipientInboxKey.count != 64)
                            if let status = inviteStatus {
                                Text(status)
                                    .font(.caption)
                                    .foregroundStyle(status.hasPrefix("Error") ? .red : .green)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                        .padding(.top, 10)
                    } label: {
                        Text("Or invite by inbox key")
                            .font(.footnote)
                            .fontWeight(.semibold)
                            .foregroundStyle(.secondary)
                            .textCase(.uppercase)
                    }
                    .padding(.top, 4)

                    Spacer(minLength: 0)
                }
                .padding(20)
            }
            .navigationTitle("")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }

    private var shortLink: String {
        // onym.chat/join?code=eyJ…
        if inviteLink.count <= 40 { return inviteLink }
        let prefix = inviteLink.prefix(28)
        return prefix + "…"
    }
}

// MARK: - Shared helpers

func groupGradient(seed: String) -> (Color, Color) {
    var hash: UInt64 = 5381
    for byte in seed.utf8 { hash = hash &* 33 &+ UInt64(byte) }
    let palette: [(Color, Color)] = [
        (Color(red: 1.00, green: 0.58, blue: 0.00), Color(red: 1.00, green: 0.18, blue: 0.33)),
        (Color(red: 0.04, green: 0.52, blue: 1.00), Color(red: 0.35, green: 0.78, blue: 0.98)),
        (Color(red: 0.19, green: 0.82, blue: 0.35), Color(red: 0.20, green: 0.78, blue: 0.35)),
        (Color(red: 0.69, green: 0.32, blue: 0.87), Color(red: 0.75, green: 0.35, blue: 0.95)),
        (Color(red: 1.00, green: 0.22, blue: 0.37), Color(red: 1.00, green: 0.18, blue: 0.33)),
        (Color(red: 1.00, green: 0.84, blue: 0.04), Color(red: 1.00, green: 0.62, blue: 0.04)),
        (Color(red: 0.39, green: 0.82, blue: 1.00), Color(red: 0.04, green: 0.52, blue: 1.00)),
        (Color(red: 0.75, green: 0.35, blue: 0.95), Color(red: 1.00, green: 0.22, blue: 0.37))
    ]
    return palette[Int(hash % UInt64(palette.count))]
}

func initials(_ name: String) -> String {
    let parts = name.trimmingCharacters(in: .whitespaces).split(separator: " ", maxSplits: 1)
    if parts.isEmpty { return "?" }
    if parts.count == 1 {
        return String(parts[0].prefix(2)).uppercased()
    }
    return (String(parts[0].prefix(1)) + String(parts[1].prefix(1))).uppercased()
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
    var memberCap: String {
        switch self {
        case .small: return "32"
        case .medium: return "256"
        case .large: return "2,048"
        @unknown default: return "?"
        }
    }
}
