import SwiftUI
import SwiftMLS

struct CreateGroupView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    // Input state
    @State private var groupName = ""
    @State private var groupType: SEPGroupType = .anarchy
    @State private var participantKeys: [String] = []
    @State private var keyInput = ""
    @State private var showScanner = false
    @State private var keyValidationError: String?

    // Creation state
    @State private var phase: CreationPhase = .input
    @State private var createdGroup: ChatGroup?
    @State private var inviteCode = ""
    @State private var errorMessage: String?
    @State private var onChainStatus: OnChainPublishStatus = .idle
    @State private var invitationStatuses: [String: InvitationStatus] = [:]
    @State private var copied = false

    private enum CreationPhase {
        case input, creating, publishing, inviting, done
    }

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

    var body: some View {
        NavigationStack {
            Form {
                switch phase {
                case .input:
                    inputSections
                case .creating, .publishing, .inviting:
                    progressSections
                case .done:
                    doneSections
                }

                if let errorMessage {
                    Section {
                        Text(errorMessage)
                            .foregroundStyle(.red)
                            .font(.caption)
                    }
                }
            }
            .navigationTitle("Create Group")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    if phase == .input {
                        Button("Cancel") { dismiss() }
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    switch phase {
                    case .input:
                        Button(participantKeys.isEmpty ? "Create" : "Create & Invite") { startCreation() }
                            .disabled(!canSubmit)
                    case .done:
                        Button("Done") {
                            if let groupID = createdGroup?.id {
                                appState.navigateToGroupID = groupID
                            }
                            dismiss()
                        }
                    default:
                        EmptyView()
                    }
                }
            }
            .interactiveDismissDisabled(isProcessing)
            .sheet(isPresented: $showScanner) {
                QRScannerView { scannedCode in
                    let trimmed = scannedCode.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
                    if trimmed.count == 64, trimmed.allSatisfy({ $0.isHexDigit }) {
                        keyInput = trimmed
                        addParticipant()
                    }
                    showScanner = false
                }
            }
        }
    }

    // MARK: - Input Phase

    @ViewBuilder
    private var inputSections: some View {
        Section("Group Name") {
            TextField("e.g. Team Alpha", text: $groupName)
        }

        Section {
            Picker("Governance", selection: $groupType) {
                Text("Anarchy").tag(SEPGroupType.anarchy)
                Text("1v1").tag(SEPGroupType.oneOnOne)
                Text("Democracy").tag(SEPGroupType.democracy)
                Text("Oligarchy").tag(SEPGroupType.oligarchy)
            }
            .pickerStyle(.segmented)
            .onChange(of: groupType) { _, newValue in
                // 1v1 allows exactly one other participant.
                if newValue == .oneOnOne, participantKeys.count > 1 {
                    participantKeys = Array(participantKeys.prefix(1))
                }
            }
            Text(governanceHelpText)
                .font(.caption2)
                .foregroundStyle(.secondary)
        } header: {
            Text("Governance Type")
        }

        Section {
            if !participantKeys.isEmpty {
                ForEach(participantKeys, id: \.self) { key in
                    HStack {
                        Text(String(key.prefix(8)) + "..." + String(key.suffix(8)))
                            .font(.caption)
                            .monospaced()
                        Spacer()
                        Button {
                            participantKeys.removeAll { $0 == key }
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                                .foregroundStyle(.secondary)
                        }
                        .buttonStyle(.plain)
                    }
                }
            }

            TextField("Inbox key (64 hex characters)", text: $keyInput)
                .font(.caption)
                .monospaced()
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)

            Button("Paste from Clipboard") {
                if let text = UIPasteboard.general.string {
                    keyInput = text.trimmingCharacters(in: .whitespacesAndNewlines)
                }
            }

            Button {
                showScanner = true
            } label: {
                Label("Scan Inbox Key QR", systemImage: "qrcode.viewfinder")
            }

            Button("Add Participant") { addParticipant() }
                .disabled(!isValidKey(keyInput) || participantSlotsRemaining == 0)

            if let keyValidationError {
                Text(keyValidationError)
                    .font(.caption2)
                    .foregroundStyle(.red)
            }

            Text(participantsHelpText)
                .font(.caption2)
                .foregroundStyle(.secondary)
        } header: {
            Text("Participants (\(participantKeys.count))")
        }
    }

    // MARK: - Progress Phase

    @ViewBuilder
    private var progressSections: some View {
        Section("Progress") {
            // Step 1: Creating group
            statusRow(
                label: "Creating group...",
                isDone: phase != .creating,
                isActive: phase == .creating,
                isFailed: false
            )

            // Step 2: On-chain publishing (if configured)
            if appState.isContractConfigured {
                switch onChainStatus {
                case .idle:
                    EmptyView()
                case .publishing:
                    statusRow(label: "Publishing to Stellar blockchain...", isDone: false, isActive: true, isFailed: false)
                case .published:
                    statusRow(label: "Published on-chain", isDone: true, isActive: false, isFailed: false)
                case .failed(let reason):
                    VStack(alignment: .leading, spacing: 8) {
                        statusRow(label: "On-chain publication failed", isDone: false, isActive: false, isFailed: true)
                        Text(reason)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                        HStack {
                            Button("Retry") { retryOnChainPublish() }
                                .font(.caption)
                            Button("Skip") { skipOnChainAndContinue() }
                                .font(.caption)
                                .foregroundStyle(.orange)
                        }
                    }
                }
            }

            // Step 3: Sending invitations
            if !participantKeys.isEmpty {
                if phase == .inviting || phase == .done {
                    ForEach(participantKeys, id: \.self) { key in
                        let displayKey = String(key.prefix(8)) + "..." + String(key.suffix(8))
                        switch invitationStatuses[key] {
                        case .sending:
                            statusRow(label: "Inviting \(displayKey)", isDone: false, isActive: true, isFailed: false)
                        case .sent:
                            statusRow(label: "Invited \(displayKey)", isDone: true, isActive: false, isFailed: false)
                        case .failed(let reason):
                            VStack(alignment: .leading, spacing: 4) {
                                statusRow(label: "Failed: \(displayKey)", isDone: false, isActive: false, isFailed: true)
                                Text(reason)
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                            }
                        case nil:
                            statusRow(label: "Pending: \(displayKey)", isDone: false, isActive: false, isFailed: false)
                        }
                    }
                }
            }
        }
    }

    // MARK: - Done Phase

    @ViewBuilder
    private var doneSections: some View {
        if !inviteCode.isEmpty {
            Section("QR Code") {
                HStack {
                    Spacer()
                    QRCodeView(inviteCode, size: 200)
                    Spacer()
                }
                .padding(.vertical, 8)
            }

            Section("Invite Code") {
                Text(inviteCode)
                    .font(.caption)
                    .monospaced()
                    .textSelection(.enabled)

                Button {
                    UIPasteboard.general.string = inviteCode
                    copied = true
                } label: {
                    Label(
                        copied ? "Copied!" : "Copy to Clipboard",
                        systemImage: copied ? "checkmark" : "doc.on.doc"
                    )
                }
            }

            Section {
                let deepLink = "https://onym.chat/join?code=\(inviteCode)"
                ShareLink(item: deepLink) {
                    Label("Share Invite Link", systemImage: "square.and.arrow.up")
                }

                Text("Share the QR code, invite link, or invite code with additional members.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }

        if !invitationStatuses.isEmpty {
            Section("Invitations") {
                let sendingCount = invitationStatuses.values.filter { if case .sending = $0 { return true }; return false }.count
                let sentCount = invitationStatuses.values.filter { if case .sent = $0 { return true }; return false }.count
                let failedCount = invitationStatuses.values.filter { if case .failed = $0 { return true }; return false }.count

                if sendingCount > 0 {
                    HStack(spacing: 8) {
                        ProgressView()
                            .controlSize(.small)
                        Text("Sending \(sendingCount) invitation\(sendingCount == 1 ? "" : "s")...")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                if sentCount > 0 {
                    Label("\(sentCount) invitation\(sentCount == 1 ? "" : "s") sent", systemImage: "checkmark.circle.fill")
                        .font(.caption)
                        .foregroundStyle(.green)
                }
                if failedCount > 0 {
                    Label("\(failedCount) invitation\(failedCount == 1 ? "" : "s") failed — invite from group settings", systemImage: "exclamationmark.triangle.fill")
                        .font(.caption)
                        .foregroundStyle(.orange)
                }
            }
        }

        if appState.isContractConfigured {
            Section("On-Chain Status") {
                switch onChainStatus {
                case .published:
                    Label("Published on-chain", systemImage: "checkmark.seal.fill")
                        .font(.caption)
                        .foregroundStyle(.green)
                case .failed(let reason):
                    VStack(alignment: .leading, spacing: 4) {
                        Label("Not published on-chain", systemImage: "exclamationmark.triangle.fill")
                            .font(.caption)
                            .foregroundStyle(.orange)
                        Text(reason)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                default:
                    EmptyView()
                }
            }
        }
    }

    // MARK: - Helpers

    private func statusRow(label: String, isDone: Bool, isActive: Bool, isFailed: Bool) -> some View {
        HStack(spacing: 8) {
            if isDone {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(.green)
                    .font(.caption)
            } else if isActive {
                ProgressView()
                    .controlSize(.small)
            } else if isFailed {
                Image(systemName: "xmark.circle.fill")
                    .foregroundStyle(.red)
                    .font(.caption)
            } else {
                Image(systemName: "circle")
                    .foregroundStyle(.secondary)
                    .font(.caption)
            }
            Text(label)
                .font(.caption)
                .foregroundStyle(isFailed ? .red : isDone ? .primary : .secondary)
        }
    }

    private var participantsHelpText: String {
        switch groupType {
        case .oneOnOne:
            return "1v1 chats pair you with exactly one other participant. Add one inbox key to continue."
        default:
            return "Enter each participant's inbox key (found in their Settings → Advanced). Participants are optional — you can invite members later."
        }
    }

    private var governanceHelpText: String {
        switch groupType {
        case .anarchy:
            return "Any current member can add or remove anyone. Fastest, lowest friction; no protection against a rogue member."
        case .oneOnOne:
            return "Exactly two participants. Membership is frozen after creation — no adds or removes. Either party can end the chat."
        case .democracy:
            return "Adding or removing a member requires at least 50% of current members to vote yes. Ceremony-gated — updates are blocked until the Democracy VK is published."
        case .oligarchy:
            return "You become the first admin. Any admin can promote/demote admins, invite members, or remove members. Ceremony-gated — updates are blocked until the Oligarchy VK is published."
        }
    }

    /// How many more participants we can accept before hitting the
    /// governance-type cap. `nil` → unbounded (the tier size is the real
    /// limit and is enforced elsewhere).
    private var participantSlotsRemaining: Int {
        switch groupType {
        case .oneOnOne:
            return max(0, 1 - participantKeys.count)
        default:
            return Int.max
        }
    }

    private var canSubmit: Bool {
        guard !groupName.trimmingCharacters(in: .whitespaces).isEmpty else { return false }
        if groupType == .oneOnOne, participantKeys.count != 1 { return false }
        return true
    }

    private func isValidKey(_ key: String) -> Bool {
        let trimmed = key.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.count == 64 && trimmed.allSatisfy { $0.isHexDigit }
    }

    private func addParticipant() {
        let trimmed = keyInput.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        keyValidationError = nil

        guard isValidKey(trimmed) else {
            keyValidationError = "Key must be exactly 64 hex characters."
            return
        }

        if trimmed == appState.keyManager.keyAgreementPublicKeyHex {
            keyValidationError = "Cannot add your own inbox key."
            return
        }

        if participantKeys.contains(trimmed) {
            keyValidationError = "This key has already been added."
            return
        }

        participantKeys.append(trimmed)
        keyInput = ""
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
                let (group, code) = try appState.createGroup(name: sanitizedName, groupType: groupType)
                createdGroup = group
                inviteCode = code

                // On-chain publishing (blocking if configured)
                if appState.isContractConfigured {
                    phase = .publishing
                    onChainStatus = .publishing
                    do {
                        try await appState.publishGroupOnChain(group)
                        onChainStatus = .published
                    } catch {
                        onChainStatus = .failed(error.localizedDescription)
                        return // UI shows retry/skip buttons
                    }
                }

                // Send invitations in background, transition to done immediately
                if !participantKeys.isEmpty {
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
            fireInvitations(to: group)
        }
        phase = .done
    }

    /// Fire invitation sending in the background — does not block the UI.
    private func fireInvitations(to group: ChatGroup) {
        let keys = participantKeys
        let transport = appState.invitationTransport
        let relayURLs = appState.relayURLs
        let keyManager = appState.keyManager

        // Mark all as sending immediately
        for key in keys {
            invitationStatuses[key] = .sending
        }

        Task {
            await transport.connect(to: relayURLs)

            for (index, key) in keys.enumerated() {
                if index > 0 { try? await Task.sleep(nanoseconds: 500_000_000) } // Space out relay publishes
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

    /// M-15: Sanitize group name — strip control characters and enforce length limit.
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
