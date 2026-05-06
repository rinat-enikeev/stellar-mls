import PhotosUI
import SwiftMLS
import SwiftUI

struct CreateGroupView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    // Identity (Step 1)
    @State private var groupName = ""
    @State private var accent: GroupAccent = .blue
    @State private var groupType: SEPGroupType = .anarchy
    @State private var selectedPhotoItem: PhotosPickerItem?
    @State private var avatarImage: UIImage?
    @State private var avatarData: Data?

    // Participants (Step 2)
    @State private var participantKeys: [String] = []
    @State private var searchText = ""
    @State private var showKeyDetail = false
    @State private var showScanner = false

    // Creation state
    @State private var step: Step = .identity
    @State private var phase: CreationPhase = .input
    @State private var createdGroup: ChatGroup?
    @State private var inviteCode = ""
    @State private var errorMessage: String?
    @State private var onChainStatus: OnChainPublishStatus = .idle
    @State private var invitationStatuses: [String: InvitationStatus] = [:]
    @State private var showRawInviteCode = false
    @State private var linkCopied = false

    private enum Step { case identity, people }

    private enum CreationPhase { case input, creating, publishing, inviting, done }

    private enum OnChainPublishStatus {
        case idle, publishing, published, failed(String)
    }

    private enum InvitationStatus {
        case sending, sent, failed(String)
    }

    private var isProcessing: Bool {
        switch phase {
        case .creating, .publishing, .inviting: return true
        default: return false
        }
    }

    private var nameIsValid: Bool {
        !groupName.trimmingCharacters(in: .whitespaces).isEmpty
    }

    private var governanceHelpText: String {
        switch groupType {
        case .anarchy:    return "Any member can add or remove anyone. Default behavior."
        case .oneOnOne:   return "Sealed 2-person chat. Membership cannot change."
        case .democracy:  return "Membership changes require a majority ballot."
        case .oligarchy:  return "Only admins can add or remove members. Creator is the first admin."
        }
    }

    var body: some View {
        NavigationStack {
            Group {
                switch phase {
                case .input:
                    inputStage
                case .creating, .publishing, .inviting:
                    progressStage
                case .done:
                    doneStage
                }
            }
            .interactiveDismissDisabled(isProcessing)
            .sheet(isPresented: $showScanner) {
                QRScannerView { scannedCode in
                    let trimmed = scannedCode.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
                    if isValidKey(trimmed) { addParticipantKey(trimmed) }
                    showScanner = false
                }
            }
            .sheet(isPresented: $showKeyDetail) {
                InviteByKeyDetailView(
                    ownInboxKey: appState.keyManager.keyAgreementPublicKeyHex,
                    existing: participantKeys,
                    onAdd: { addParticipantKey($0) }
                )
            }
        }
    }

    // MARK: - Stage 1 · Input (split into Step 1 identity + Step 2 people)

    @ViewBuilder
    private var inputStage: some View {
        switch step {
        case .identity:
            identityStep
        case .people:
            peopleStep
        }
    }

    // MARK: Identity step

    private var identityStep: some View {
        ScrollView {
            VStack(spacing: 0) {
                // Hero avatar — tap to pick a photo.
                PhotosPicker(selection: $selectedPhotoItem, matching: .images) {
                    ZStack(alignment: .bottomTrailing) {
                        groupAvatar(
                            size: 108,
                            filled: nameIsValid,
                            corner: 54
                        )
                        Circle()
                            .fill(Color.accentColor)
                            .frame(width: 36, height: 36)
                            .overlay(Image(systemName: "camera.fill").font(.system(size: 15, weight: .semibold)).foregroundStyle(.white))
                            .overlay(Circle().stroke(Color(.systemGroupedBackground), lineWidth: 3))
                            .offset(x: 2, y: 2)
                    }
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Change group photo")
                .padding(.top, 12)
                .padding(.bottom, 20)
                .onChange(of: selectedPhotoItem) { _, newItem in
                    guard let newItem else { return }
                    Task {
                        if let data = try? await newItem.loadTransferable(type: Data.self),
                           let image = UIImage(data: data) {
                            let processed = Self.downscaleAvatar(image)
                            avatarImage = processed.image
                            avatarData = processed.data
                        }
                    }
                }

                // Name input
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        TextField("Group name", text: $groupName)
                            .font(.system(size: 19))
                            .textInputAutocapitalization(.words)
                    }
                    .padding(.vertical, 16)
                    .padding(.horizontal, 18)
                    .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14))

                    Text("Visible to members. You can change this anytime.")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 4)
                }
                .padding(.horizontal, 20)
                .padding(.bottom, 24)

                // Accent picker
                VStack(alignment: .leading, spacing: 10) {
                    Text("ACCENT COLOR")
                        .font(.caption)
                        .fontWeight(.semibold)
                        .foregroundStyle(.secondary)
                        .kerning(0.4)
                        .padding(.horizontal, 4)
                    HStack(spacing: 12) {
                        ForEach(GroupAccent.allCases, id: \.self) { option in
                            Button {
                                withAnimation(.snappy) { accent = option }
                            } label: {
                                ZStack {
                                    Circle()
                                        .fill(option.gradient)
                                        .frame(width: 40, height: 40)
                                    if accent == option {
                                        Image(systemName: "checkmark")
                                            .font(.system(size: 15, weight: .bold))
                                            .foregroundStyle(.white)
                                    }
                                }
                                .padding(3)
                                .overlay(
                                    Circle().stroke(
                                        accent == option ? Color.accentColor : .clear,
                                        lineWidth: 2
                                    )
                                )
                            }
                            .buttonStyle(.plain)
                        }
                        Spacer()
                    }
                }
                .padding(.horizontal, 20)
                .padding(.bottom, 20)

                // Governance picker
                VStack(alignment: .leading, spacing: 10) {
                    Text("GOVERNANCE")
                        .font(.caption)
                        .fontWeight(.semibold)
                        .foregroundStyle(.secondary)
                        .kerning(0.4)
                        .padding(.horizontal, 4)
                    Picker("Governance", selection: $groupType) {
                        Text("Anarchy").tag(SEPGroupType.anarchy)
                        Text("1v1").tag(SEPGroupType.oneOnOne)
                        Text("Democracy").tag(SEPGroupType.democracy)
                        Text("Oligarchy").tag(SEPGroupType.oligarchy)
                    }
                    .pickerStyle(.segmented)
                    .onChange(of: groupType) { _, newValue in
                        if newValue == .oneOnOne, participantKeys.count > 1 {
                            participantKeys = Array(participantKeys.prefix(1))
                        }
                    }
                    Text(governanceHelpText)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 4)
                }
                .padding(.horizontal, 20)
                .padding(.bottom, 20)

                // E2E banner
                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: "lock.fill")
                        .font(.system(size: 18))
                        .foregroundStyle(.green)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("End-to-end encrypted")
                            .font(.subheadline)
                            .fontWeight(.semibold)
                        Text("Only members can read messages. Onym never sees the content.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 0)
                }
                .padding(14)
                .background(Color.green.opacity(0.10), in: RoundedRectangle(cornerRadius: 14))
                .padding(.horizontal, 20)

                if let errorMessage {
                    Text(errorMessage)
                        .font(.footnote)
                        .foregroundStyle(.red)
                        .padding(.top, 12)
                        .padding(.horizontal, 20)
                }

                Spacer(minLength: 40)
            }
            .padding(.top, 4)
        }
        .background(Color(.systemGroupedBackground))
        .navigationTitle("New Group")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .principal) {
                VStack(spacing: 1) {
                    Text("New Group").font(.system(size: 17, weight: .semibold))
                    Text("Step 1 of 2").font(.caption2).foregroundStyle(.secondary)
                }
            }
            ToolbarItem(placement: .cancellationAction) {
                Button("Cancel") { dismiss() }
            }
            ToolbarItem(placement: .confirmationAction) {
                Button("Next") { step = .people }
                    .fontWeight(.semibold)
                    .disabled(!nameIsValid)
            }
        }
    }

    // MARK: People step

    private var peopleStep: some View {
        ScrollView {
            VStack(spacing: 0) {
                if !participantKeys.isEmpty {
                    selectedChipBar
                }

                // Search
                HStack(spacing: 6) {
                    Image(systemName: "magnifyingglass")
                        .foregroundStyle(.secondary)
                    TextField("Search name or paste invite key", text: $searchText)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .onSubmit { tryAddFromSearch() }
                        .onChange(of: searchText) { _, newValue in
                            let trimmed = newValue.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
                            if isValidKey(trimmed) {
                                addParticipantKey(trimmed)
                                searchText = ""
                            }
                        }
                    if !searchText.isEmpty {
                        Button { searchText = "" } label: {
                            Image(systemName: "xmark.circle.fill")
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                .padding(.vertical, 10)
                .padding(.horizontal, 12)
                .background(Color(.systemGray5).opacity(0.6), in: RoundedRectangle(cornerRadius: 10))
                .padding(.horizontal, 16)
                .padding(.top, participantKeys.isEmpty ? 12 : 4)
                .padding(.bottom, 12)

                // From your chats
                if !filteredFromChats.isEmpty {
                    sectionHeader("FROM YOUR CHATS")
                    VStack(spacing: 0) {
                        ForEach(Array(filteredFromChats.enumerated()), id: \.element.inboxHex) { index, contact in
                            ContactRow(
                                alias: contact.alias ?? String(contact.blsHex.prefix(12)) + "…",
                                subtitle: contact.subtitle,
                                seed: contact.blsHex,
                                selected: participantKeys.contains(contact.inboxHex)
                            ) {
                                toggleParticipant(contact.inboxHex)
                            }
                            if index < filteredFromChats.count - 1 {
                                Divider().padding(.leading, 70)
                            }
                        }
                    }
                    .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14))
                    .padding(.horizontal, 16)
                }

                // Invite by key (advanced)
                sectionHeader(filteredFromChats.isEmpty ? "ADD PEOPLE" : "", topPadding: filteredFromChats.isEmpty ? 0 : 20)
                Button {
                    showKeyDetail = true
                } label: {
                    HStack(spacing: 12) {
                        Image(systemName: "key.fill")
                            .font(.system(size: 15))
                            .frame(width: 36, height: 36)
                            .background(Color(.tertiarySystemFill), in: RoundedRectangle(cornerRadius: 10))
                            .foregroundStyle(Color.accentColor)
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Invite by inbox key")
                                .font(.system(size: 15))
                                .foregroundStyle(.primary)
                            Text("Paste or scan a 64-char key")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        Image(systemName: "chevron.right")
                            .font(.system(size: 13, weight: .semibold))
                            .foregroundStyle(.tertiary)
                    }
                    .padding(.vertical, 12)
                    .padding(.horizontal, 14)
                    .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14))
                }
                .buttonStyle(.plain)
                .padding(.horizontal, 16)

                if filteredFromChats.isEmpty {
                    Text("People you've chatted with will appear here. Until then, paste a contact's inbox key to invite them.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 20)
                        .padding(.top, 12)
                }

                if let errorMessage {
                    Text(errorMessage)
                        .font(.footnote)
                        .foregroundStyle(.red)
                        .padding(.top, 12)
                        .padding(.horizontal, 20)
                }

                Spacer(minLength: 40)
            }
        }
        .background(Color(.systemGroupedBackground))
        .navigationTitle("Add People")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .principal) {
                VStack(spacing: 1) {
                    Text(participantKeys.isEmpty ? "Add People" : "Add People · \(participantKeys.count)")
                        .font(.system(size: 17, weight: .semibold))
                    Text("Step 2 of 2").font(.caption2).foregroundStyle(.secondary)
                }
            }
            ToolbarItem(placement: .cancellationAction) {
                Button("Back") { step = .identity }
            }
            ToolbarItem(placement: .confirmationAction) {
                Button("Create") { startCreation() }
                    .fontWeight(.semibold)
            }
        }
    }

    private var selectedChipBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(participantKeys, id: \.self) { key in
                    let contact = fromChats.first(where: { $0.inboxHex == key })
                    HStack(spacing: 6) {
                        avatarCircle(seed: contact?.blsHex ?? key, size: 26, name: contact?.alias ?? "?")
                        Text(contact?.alias ?? shortKey(key))
                            .font(.system(size: 13, weight: .medium))
                            .lineLimit(1)
                        Button {
                            participantKeys.removeAll { $0 == key }
                        } label: {
                            Image(systemName: "xmark")
                                .font(.system(size: 10, weight: .bold))
                                .foregroundStyle(.secondary)
                        }
                    }
                    .padding(.vertical, 4)
                    .padding(.leading, 4)
                    .padding(.trailing, 10)
                    .background(Color(.secondarySystemGroupedBackground), in: Capsule())
                }
            }
            .padding(.horizontal, 16)
            .padding(.top, 12)
            .padding(.bottom, 6)
        }
    }

    @ViewBuilder
    private func sectionHeader(_ title: String, topPadding: CGFloat = 16) -> some View {
        if !title.isEmpty {
            HStack {
                Text(title)
                    .font(.caption)
                    .fontWeight(.semibold)
                    .kerning(0.4)
                    .foregroundStyle(.secondary)
                Spacer()
            }
            .padding(.horizontal, 20)
            .padding(.top, topPadding)
            .padding(.bottom, 6)
        }
    }

    // MARK: - Data sourcing

    private struct ChatContact {
        let blsHex: String
        let inboxHex: String
        let alias: String?
        let subtitle: String
    }

    private var fromChats: [ChatContact] {
        let aliasStore = appState.contactAliasStore
        let inboxMap = appState.lookupContactInboxKeys()

        // Build: BLS hex → set of group names containing this member
        var groupNamesByBLS: [String: Set<String>] = [:]
        for group in appState.groups {
            for member in group.members {
                let hex = member.publicKeyCompressed.map { String(format: "%02x", $0) }.joined()
                groupNamesByBLS[hex, default: []].insert(group.name)
            }
        }

        return inboxMap.compactMap { (blsHex, inboxHex) -> ChatContact? in
            let groups = groupNamesByBLS[blsHex] ?? []
            let subtitle = groups.sorted().joined(separator: ", ")
            // Translate BLS hex to Nostr pubkey for alias lookup isn't trivial, so use aliasStore
            // directly by BLS hex — aliasStore accepts any pubkey string as key
            let alias = aliasStore?.displayName(for: blsHex)
            return ChatContact(
                blsHex: blsHex,
                inboxHex: inboxHex,
                alias: alias,
                subtitle: subtitle.isEmpty ? String(blsHex.prefix(12)) + "…" : subtitle
            )
        }
        .sorted { ($0.alias ?? $0.blsHex) < ($1.alias ?? $1.blsHex) }
    }

    private var filteredFromChats: [ChatContact] {
        let needle = searchText.trimmingCharacters(in: .whitespaces).lowercased()
        guard !needle.isEmpty else { return fromChats }
        return fromChats.filter { contact in
            (contact.alias ?? "").lowercased().contains(needle) ||
                contact.blsHex.lowercased().contains(needle) ||
                contact.subtitle.lowercased().contains(needle)
        }
    }

    // MARK: - Participant management

    private func toggleParticipant(_ inboxKey: String) {
        if participantKeys.contains(inboxKey) {
            participantKeys.removeAll { $0 == inboxKey }
        } else {
            addParticipantKey(inboxKey)
        }
    }

    private func addParticipantKey(_ raw: String) {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard isValidKey(trimmed) else {
            errorMessage = "Key must be exactly 64 hex characters."
            return
        }
        if trimmed == appState.keyManager.keyAgreementPublicKeyHex {
            errorMessage = "Cannot add your own inbox key."
            return
        }
        if participantKeys.contains(trimmed) { return }
        if groupType == .oneOnOne && !participantKeys.isEmpty {
            errorMessage = "1v1 groups allow only one other participant."
            return
        }
        errorMessage = nil
        participantKeys.append(trimmed)
    }

    private func tryAddFromSearch() {
        let trimmed = searchText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if isValidKey(trimmed) {
            addParticipantKey(trimmed)
            searchText = ""
        }
    }

    private func isValidKey(_ key: String) -> Bool {
        key.count == 64 && key.allSatisfy { $0.isHexDigit }
    }

    private func shortKey(_ hex: String) -> String {
        guard hex.count >= 12 else { return hex }
        return String(hex.prefix(8)) + "…"
    }

    // MARK: - Progress stage

    private var progressStage: some View {
        ScrollView {
            VStack(spacing: 0) {
                // Animated hero avatar
                ZStack {
                    Circle()
                        .stroke(Color.accentColor.opacity(0.2), lineWidth: 2)
                        .frame(width: 140, height: 140)
                    Circle()
                        .trim(from: 0, to: 0.16)
                        .stroke(Color.accentColor, style: StrokeStyle(lineWidth: 2, lineCap: .round))
                        .frame(width: 140, height: 140)
                        .rotationEffect(.degrees(spinnerAngle))
                    groupAvatar(size: 120, filled: true, corner: 60)
                        .shadow(color: accent.shadow, radius: 16, y: 8)
                }
                .padding(.top, 32)
                .padding(.bottom, 32)
                .onAppear(perform: startSpinner)

                // Steps
                VStack(spacing: 0) {
                    progressRow(
                        title: "Setting up encrypted group",
                        subtitle: "Generating keys on your device",
                        state: stageState(for: .creating)
                    )
                    if appState.isContractConfigured {
                        Divider().padding(.leading, 52)
                        progressRow(
                            title: onChainStatusFailed ? "Anchoring on Stellar (failed)" : "Anchoring on Stellar",
                            subtitle: onChainStatusSubtitle,
                            state: stageState(for: .publishing),
                            failed: onChainStatusFailed
                        )
                        if onChainStatusFailed {
                            HStack(spacing: 12) {
                                Button("Retry") { retryOnChainPublish() }
                                    .buttonStyle(.bordered)
                                Button("Skip") { skipOnChainAndContinue() }
                                    .buttonStyle(.bordered)
                                    .tint(.orange)
                                Spacer()
                            }
                            .padding(.leading, 52)
                            .padding(.trailing, 16)
                            .padding(.bottom, 12)
                        }
                    }
                    if !participantKeys.isEmpty {
                        Divider().padding(.leading, 52)
                        progressRow(
                            title: "Sending \(participantKeys.count) invitation\(participantKeys.count == 1 ? "" : "s")",
                            subtitle: "Delivered via relays",
                            state: stageState(for: .inviting)
                        )
                    }
                }
                .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14))
                .padding(.horizontal, 16)

                Text("This usually takes a few seconds. It's safe to close this — we'll finish in the background.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 32)
                    .padding(.top, 20)
                    .padding(.bottom, 20)
            }
        }
        .background(Color(.systemGroupedBackground))
        .navigationTitle("Creating \(groupName.isEmpty ? "group" : groupName)")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar(.hidden, for: .navigationBar)
        .overlay(alignment: .top) {
            Text("Creating \(groupName)")
                .font(.system(size: 17, weight: .semibold))
                .padding(.top, 14)
        }
    }

    private var onChainStatusFailed: Bool {
        if case .failed = onChainStatus { return true }
        return false
    }

    private var onChainStatusSubtitle: String {
        switch onChainStatus {
        case .idle, .publishing: return "So anyone can verify this group is real"
        case .published: return "Verified publicly"
        case .failed(let reason): return reason
        }
    }

    private enum StageState { case pending, active, done }

    private func stageState(for stage: CreationPhase) -> StageState {
        let order: [CreationPhase] = [.creating, .publishing, .inviting, .done]
        guard let currentIdx = order.firstIndex(of: phase),
              let targetIdx = order.firstIndex(of: stage) else { return .pending }
        if currentIdx > targetIdx { return .done }
        if currentIdx == targetIdx { return .active }
        return .pending
    }

    private func progressRow(title: String, subtitle: String, state: StageState, failed: Bool = false) -> some View {
        HStack(alignment: .top, spacing: 12) {
            ZStack {
                if failed {
                    Image(systemName: "exclamationmark.circle.fill")
                        .font(.system(size: 20))
                        .foregroundStyle(.orange)
                } else {
                    switch state {
                    case .done:
                        Image(systemName: "checkmark.circle.fill")
                            .font(.system(size: 20))
                            .foregroundStyle(.green)
                    case .active:
                        ProgressView().controlSize(.small)
                    case .pending:
                        Circle()
                            .stroke(Color.secondary.opacity(0.4), lineWidth: 1.5)
                            .frame(width: 18, height: 18)
                    }
                }
            }
            .frame(width: 24, height: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 15, weight: state == .active ? .semibold : .medium))
                    .foregroundStyle(state == .pending ? .secondary : .primary)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
        }
        .padding(14)
        .opacity(state == .pending ? 0.6 : 1)
    }

    @State private var spinnerAngle: Double = 0

    private func startSpinner() {
        withAnimation(.linear(duration: 1.5).repeatForever(autoreverses: false)) {
            spinnerAngle = 360
        }
    }

    // MARK: - Done stage

    private var doneStage: some View {
        ScrollView {
            VStack(spacing: 0) {
                // Hero: avatar + check badge
                ZStack(alignment: .bottomTrailing) {
                    groupAvatar(size: 84, filled: true, corner: 42)
                    Circle()
                        .fill(.green)
                        .frame(width: 32, height: 32)
                        .overlay(Image(systemName: "checkmark").font(.system(size: 15, weight: .bold)).foregroundStyle(.white))
                        .overlay(Circle().stroke(Color(.systemGroupedBackground), lineWidth: 4))
                        .offset(x: 6, y: 6)
                }
                .padding(.top, 16)
                .padding(.bottom, 14)

                Text(groupName)
                    .font(.system(size: 22, weight: .bold))
                Label("End-to-end encrypted", systemImage: "lock.fill")
                    .labelStyle(CompactLabelStyle())
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .padding(.top, 4)
                    .padding(.bottom, 16)

                // Status chips
                HStack(spacing: 8) {
                    if appState.isContractConfigured {
                        statusChip(
                            text: onChainStatusFailed ? "On-chain pending" : "Published on-chain",
                            systemImage: onChainStatusFailed ? "clock.fill" : "checkmark.seal.fill",
                            color: onChainStatusFailed ? .orange : .green
                        )
                    }
                    if !invitationStatuses.isEmpty {
                        let sent = invitationStatuses.values.filter { if case .sent = $0 { return true }; return false }.count
                        statusChip(
                            text: "\(sent) invite\(sent == 1 ? "" : "s") sent",
                            systemImage: "checkmark.circle.fill",
                            color: .green
                        )
                    }
                }
                .padding(.horizontal, 20)
                .padding(.bottom, 16)

                // QR + share surface
                VStack(spacing: 0) {
                    VStack(spacing: 10) {
                        QRCodeView(inviteCode, size: 168)
                            .padding(8)
                            .background(Color.white, in: RoundedRectangle(cornerRadius: 12))
                        Text("Invite more people")
                            .font(.subheadline)
                            .fontWeight(.medium)
                        Text("Scan this QR, or share the link")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 18)
                    .frame(maxWidth: .infinity)

                    Divider()

                    HStack(spacing: 0) {
                        Button {
                            UIPasteboard.general.string = inviteLink
                            UINotificationFeedbackGenerator().notificationOccurred(.success)
                            withAnimation(.snappy) { linkCopied = true }
                            DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
                                withAnimation(.snappy) { linkCopied = false }
                            }
                        } label: {
                            Label(
                                linkCopied ? "Copied!" : "Copy Link",
                                systemImage: linkCopied ? "checkmark" : "link"
                            )
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 14)
                        }
                        .buttonStyle(.plain)
                        .foregroundStyle(linkCopied ? Color.green : Color.accentColor)

                        Divider().frame(height: 44)

                        ShareLink(item: inviteLink) {
                            Label("Share…", systemImage: "square.and.arrow.up")
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 14)
                        }
                        .foregroundStyle(Color.accentColor)
                    }
                }
                .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 16))
                .padding(.horizontal, 16)
                .padding(.bottom, 16)

                // Advanced — raw invite code
                VStack(spacing: 0) {
                    Button {
                        withAnimation(.snappy) { showRawInviteCode.toggle() }
                    } label: {
                        HStack {
                            Text("Show raw invite code")
                                .font(.subheadline)
                                .foregroundStyle(.primary)
                            Spacer()
                            Image(systemName: "chevron.right")
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(.tertiary)
                                .rotationEffect(.degrees(showRawInviteCode ? 90 : 0))
                        }
                        .padding(.vertical, 12)
                        .padding(.horizontal, 16)
                    }
                    .buttonStyle(.plain)

                    if showRawInviteCode {
                        Divider().padding(.leading, 16)
                        VStack(alignment: .leading, spacing: 8) {
                            Text(inviteCode)
                                .font(.system(size: 11, design: .monospaced))
                                .foregroundStyle(.secondary)
                                .lineLimit(6)
                                .textSelection(.enabled)
                            Button {
                                UIPasteboard.general.string = inviteCode
                            } label: {
                                Label("Copy code", systemImage: "doc.on.doc")
                                    .font(.footnote)
                            }
                        }
                        .padding(.horizontal, 16)
                        .padding(.vertical, 12)
                    }
                }
                .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14))
                .padding(.horizontal, 16)

                Spacer(minLength: 24)
            }
            .padding(.bottom, 24)
        }
        .background(Color(.systemGroupedBackground))
        .navigationTitle(groupName.isEmpty ? "Group created" : "\(groupName) is live")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Button("Open") {
                    if let groupID = createdGroup?.id {
                        appState.navigateToGroupID = groupID
                    }
                    dismiss()
                }
                .fontWeight(.semibold)
                .accessibilityIdentifier(TestIDs.CreateGroup.openButton)
            }
        }
    }

    private var inviteLink: String {
        "https://onym.chat/join?code=\(inviteCode)"
    }

    private func statusChip(text: String, systemImage: String, color: Color) -> some View {
        Label(text, systemImage: systemImage)
            .font(.caption.weight(.semibold))
            .foregroundStyle(color)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(color.opacity(0.12), in: Capsule())
    }

    // MARK: - Avatar

    @ViewBuilder
    private func groupAvatar(size: CGFloat, filled: Bool, corner: CGFloat) -> some View {
        let label = nameIsValid ? groupName.trimmingCharacters(in: .whitespaces) : ""
        ZStack {
            if let avatarImage {
                Image(uiImage: avatarImage)
                    .resizable()
                    .scaledToFill()
                    .frame(width: size, height: size)
                    .clipShape(RoundedRectangle(cornerRadius: corner))
            } else if filled {
                RoundedRectangle(cornerRadius: corner)
                    .fill(accent.gradient)
                Text(initials(for: label))
                    .font(.system(size: size * 0.36, weight: .bold))
                    .foregroundStyle(.white)
            } else {
                RoundedRectangle(cornerRadius: corner)
                    .fill(Color(.systemGray5))
                Image(systemName: "camera.fill")
                    .font(.system(size: size * 0.26))
                    .foregroundStyle(.secondary)
            }
        }
        .frame(width: size, height: size)
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

    private func avatarCircle(seed: String, size: CGFloat, name: String) -> some View {
        let accent = GroupAccent.fromSeed(seed)
        return ZStack {
            Circle().fill(accent.gradient)
            Text(initials(for: name))
                .font(.system(size: size * 0.38, weight: .bold))
                .foregroundStyle(.white)
        }
        .frame(width: size, height: size)
    }

    private func initials(for name: String) -> String {
        let parts = name.trimmingCharacters(in: .whitespaces).split(separator: " ")
        if parts.isEmpty { return "?" }
        if parts.count == 1 { return parts[0].prefix(2).uppercased() }
        let first = parts[0].first.map { String($0) } ?? ""
        let second = parts[1].first.map { String($0) } ?? ""
        return (first + second).uppercased()
    }

    // MARK: - Creation Flow

    private func startCreation() {
        guard let sanitizedName = Self.sanitizeGroupName(groupName) else {
            errorMessage = "Group name must be 1-100 characters with no control characters"
            return
        }

        errorMessage = nil
        phase = .creating

        Task {
            do {
                let (group, code) = try appState.createGroup(
                    name: sanitizedName,
                    groupType: groupType,
                    avatarData: avatarData
                )
                createdGroup = group
                inviteCode = code
                GroupAccentStore.set(accent, for: group.id)

                if appState.isContractConfigured {
                    phase = .publishing
                    onChainStatus = .publishing
                    do {
                        try await appState.publishGroupOnChain(group)
                        onChainStatus = .published
                    } catch {
                        onChainStatus = .failed(error.localizedDescription)
                        return
                    }
                }

                if !participantKeys.isEmpty {
                    phase = .inviting
                    fireInvitations(to: group)
                }
                phase = .done
            } catch {
                errorMessage = error.localizedDescription
                phase = .input
            }
        }
    }

    private func retryOnChainPublish() {
        guard let group = createdGroup else { return }
        onChainStatus = .publishing

        Task {
            do {
                try await appState.publishGroupOnChain(group)
                onChainStatus = .published
                if !participantKeys.isEmpty {
                    phase = .inviting
                    fireInvitations(to: group)
                }
                phase = .done
            } catch {
                onChainStatus = .failed(error.localizedDescription)
            }
        }
    }

    private func skipOnChainAndContinue() {
        guard let group = createdGroup else { return }
        if !participantKeys.isEmpty {
            phase = .inviting
            fireInvitations(to: group)
        }
        phase = .done
    }

    private func fireInvitations(to group: ChatGroup) {
        let keys = participantKeys
        let transport = appState.invitationTransport
        let relayURLs = appState.relayURLs
        let keyManager = appState.keyManager

        for key in keys { invitationStatuses[key] = .sending }

        Task {
            await transport.connect(to: relayURLs)

            for (index, key) in keys.enumerated() {
                if index > 0 { try? await Task.sleep(nanoseconds: 500_000_000) }
                do {
                    guard let pubkeyData = hexToData(key), pubkeyData.count == 32 else {
                        invitationStatuses[key] = .failed("Invalid key format")
                        continue
                    }
                    let payload = BootstrapPayload.from(
                        group: group,
                        senderPubkey: keyManager.publicKeyHex
                    )
                    try await transport.sendInvitation(
                        payload: payload,
                        recipientKeyAgreementPubkey: pubkeyData,
                        keyManager: keyManager
                    )
                    invitationStatuses[key] = .sent
                } catch {
                    invitationStatuses[key] = .failed(error.localizedDescription)
                }
            }
        }
    }

    private static func sanitizeGroupName(_ name: String) -> String? {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let sanitized = trimmed.unicodeScalars.filter {
            !$0.properties.isDefaultIgnorableCodePoint &&
            ($0.value >= 0x20 || $0 == "\n" || $0 == "\t")
        }
        let result = String(String.UnicodeScalarView(sanitized))
        guard !result.isEmpty, result.count <= 100 else { return nil }
        return result
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

// MARK: - Supporting views

private struct ContactRow: View {
    let alias: String
    let subtitle: String
    let seed: String
    let selected: Bool
    let onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            HStack(spacing: 12) {
                let accent = GroupAccent.fromSeed(seed)
                ZStack {
                    Circle().fill(accent.gradient)
                    Text(initials)
                        .font(.system(size: 16, weight: .bold))
                        .foregroundStyle(.white)
                }
                .frame(width: 42, height: 42)

                VStack(alignment: .leading, spacing: 2) {
                    Text(alias)
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(.primary)
                        .lineLimit(1)
                    if !subtitle.isEmpty {
                        Text(subtitle)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }
                Spacer()
                if selected {
                    ZStack {
                        Circle().fill(Color.accentColor)
                        Image(systemName: "checkmark")
                            .font(.system(size: 10, weight: .bold))
                            .foregroundStyle(.white)
                    }
                    .frame(width: 24, height: 24)
                } else {
                    Circle()
                        .stroke(Color.secondary.opacity(0.4), lineWidth: 1.5)
                        .frame(width: 24, height: 24)
                }
            }
            .padding(.vertical, 10)
            .padding(.horizontal, 16)
            .contentShape(Rectangle())
            .background(selected ? Color.accentColor.opacity(0.08) : .clear)
        }
        .buttonStyle(.plain)
    }

    private var initials: String {
        let parts = alias.trimmingCharacters(in: .whitespaces).split(separator: " ")
        if parts.isEmpty { return "?" }
        if parts.count == 1 { return parts[0].prefix(2).uppercased() }
        let first = parts[0].first.map { String($0) } ?? ""
        let second = parts[1].first.map { String($0) } ?? ""
        return (first + second).uppercased()
    }
}

private struct InviteByKeyDetailView: View {
    @Environment(\.dismiss) private var dismiss
    let ownInboxKey: String
    let existing: [String]
    let onAdd: (String) -> Void

    @State private var keyInput = ""
    @State private var error: String?
    @State private var showScanner = false

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text("Ask for their **inbox key** — they can find it in **Settings → Advanced**, or share a QR code from there.")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 20)
                        .padding(.top, 12)

                    VStack(alignment: .leading, spacing: 6) {
                        TextField("Paste 64-char inbox key", text: $keyInput, axis: .vertical)
                            .font(.system(size: 13, design: .monospaced))
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .lineLimit(3)
                            .padding(14)
                            .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14))
                        if let error {
                            Text(error)
                                .font(.caption)
                                .foregroundStyle(.red)
                        }
                    }
                    .padding(.horizontal, 16)

                    HStack(spacing: 10) {
                        Button {
                            showScanner = true
                        } label: {
                            Label("Scan QR", systemImage: "qrcode.viewfinder")
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 14)
                        }
                        .buttonStyle(.borderedProminent)

                        Button {
                            if let text = UIPasteboard.general.string {
                                keyInput = text.trimmingCharacters(in: .whitespacesAndNewlines)
                            }
                        } label: {
                            Label("Paste", systemImage: "doc.on.clipboard")
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 14)
                        }
                        .buttonStyle(.bordered)
                    }
                    .padding(.horizontal, 16)

                    HStack(alignment: .top, spacing: 10) {
                        Image(systemName: "info.circle")
                            .foregroundStyle(.secondary)
                        Text("Don't have their key? Create the group, then share the invite link — they can join instantly without sharing any keys.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .padding(14)
                    .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14))
                    .padding(.horizontal, 16)

                    Spacer(minLength: 20)
                }
            }
            .background(Color(.systemGroupedBackground))
            .navigationTitle("Invite by Inbox Key")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") { addTapped() }
                        .fontWeight(.semibold)
                        .disabled(!isValidKey(keyInput))
                }
            }
            .sheet(isPresented: $showScanner) {
                QRScannerView { scanned in
                    showScanner = false
                    let t = scanned.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
                    guard isValidKey(t) else {
                        error = "Scanned code is not a valid 64-character inbox key."
                        return
                    }
                    if t == ownInboxKey.lowercased() {
                        error = "Cannot add your own inbox key."
                        return
                    }
                    if existing.contains(t) {
                        error = "This user has already been added."
                        return
                    }
                    error = nil
                    onAdd(t)
                    dismiss()
                }
            }
        }
    }

    private func addTapped() {
        let trimmed = keyInput.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard isValidKey(trimmed) else {
            error = "Key must be exactly 64 hex characters."
            return
        }
        if trimmed == ownInboxKey.lowercased() {
            error = "Cannot add your own inbox key."
            return
        }
        if existing.contains(trimmed) {
            error = "This key has already been added."
            return
        }
        onAdd(trimmed)
        dismiss()
    }

    private func isValidKey(_ key: String) -> Bool {
        let t = key.trimmingCharacters(in: .whitespacesAndNewlines)
        return t.count == 64 && t.allSatisfy { $0.isHexDigit }
    }
}

private struct CompactLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 4) {
            configuration.icon
                .font(.caption2)
            configuration.title
        }
    }
}

// MARK: - Accent color

enum GroupAccent: String, CaseIterable, Codable {
    case orange, blue, green, purple, pink, yellow

    var gradient: LinearGradient {
        LinearGradient(colors: stops, startPoint: .topLeading, endPoint: .bottomTrailing)
    }

    private var stops: [Color] {
        switch self {
        case .orange: return [Color(hex: 0xFF9500), Color(hex: 0xFF2D55)]
        case .blue:   return [Color(hex: 0x0A84FF), Color(hex: 0x5AC8FA)]
        case .green:  return [Color(hex: 0x30D158), Color(hex: 0x34C759)]
        case .purple: return [Color(hex: 0xAF52DE), Color(hex: 0xBF5AF2)]
        case .pink:   return [Color(hex: 0xFF375F), Color(hex: 0xFF2D55)]
        case .yellow: return [Color(hex: 0xFFD60A), Color(hex: 0xFF9F0A)]
        }
    }

    var shadow: Color {
        switch self {
        case .orange: return Color(hex: 0xFF9500).opacity(0.35)
        case .blue:   return Color(hex: 0x0A84FF).opacity(0.35)
        case .green:  return Color(hex: 0x30D158).opacity(0.35)
        case .purple: return Color(hex: 0xAF52DE).opacity(0.35)
        case .pink:   return Color(hex: 0xFF375F).opacity(0.35)
        case .yellow: return Color(hex: 0xFFD60A).opacity(0.35)
        }
    }

    /// Derive a stable accent from an arbitrary seed string.
    static func fromSeed(_ seed: String) -> GroupAccent {
        var hash: UInt64 = 5381
        for byte in seed.utf8 { hash = (hash &* 33) &+ UInt64(byte) }
        let all = GroupAccent.allCases
        return all[Int(hash % UInt64(all.count))]
    }
}

/// Simple per-group accent persistence (UserDefaults keyed by group ID).
enum GroupAccentStore {
    private static let prefix = "chat.onym.groupAccent."

    static func set(_ accent: GroupAccent, for groupID: String) {
        UserDefaults.standard.set(accent.rawValue, forKey: prefix + groupID)
    }

    static func get(for groupID: String) -> GroupAccent {
        if let raw = UserDefaults.standard.string(forKey: prefix + groupID),
           let accent = GroupAccent(rawValue: raw) {
            return accent
        }
        return .fromSeed(groupID)
    }
}

private extension Color {
    init(hex: UInt32) {
        let r = Double((hex >> 16) & 0xFF) / 255
        let g = Double((hex >> 8) & 0xFF) / 255
        let b = Double(hex & 0xFF) / 255
        self.init(red: r, green: g, blue: b)
    }
}
