import SwiftUI

struct CreateGroupView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var groupName = ""
    @State private var inviteCode = ""
    @State private var copied = false
    @State private var errorMessage: String?
    @State private var onChainStatus: OnChainPublishStatus = .idle
    @State private var createdGroup: ChatGroup?

    private enum OnChainPublishStatus {
        case idle
        case publishing
        case published
        case failed(String)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Group Name") {
                    TextField("e.g. Team Alpha", text: $groupName)
                }

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
                        let deepLink = "stellarchat://join?code=\(inviteCode)"
                        ShareLink(item: deepLink) {
                            Label("Share Invite Link", systemImage: "square.and.arrow.up")
                        }

                        Text("Share the QR code, invite link, or invite code with group members.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }

                if !inviteCode.isEmpty && appState.isContractConfigured {
                    Section("On-Chain Status") {
                        switch onChainStatus {
                        case .idle:
                            EmptyView()
                        case .publishing:
                            HStack(spacing: 8) {
                                ProgressView()
                                    .controlSize(.small)
                                Text("Publishing to Stellar testnet...")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        case .published:
                            Label("Published on-chain", systemImage: "checkmark.seal.fill")
                                .font(.caption)
                                .foregroundStyle(.green)
                        case .failed(let reason):
                            VStack(alignment: .leading, spacing: 4) {
                                Label("Publication failed", systemImage: "exclamationmark.triangle.fill")
                                    .font(.caption)
                                    .foregroundStyle(.red)
                                Text(reason)
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                                Button("Retry") { publishOnChain() }
                                    .font(.caption)
                            }
                        }
                    }
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
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    if inviteCode.isEmpty {
                        Button("Create") { createGroup() }
                            .disabled(groupName.trimmingCharacters(in: .whitespaces).isEmpty)
                    } else {
                        Button("Done") { dismiss() }
                    }
                }
            }
        }
    }

    /// M-15: Sanitize group name — strip control characters and enforce length limit.
    private static func sanitizeGroupName(_ name: String) -> String? {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        // Remove Unicode control characters (C0, C1, and format characters)
        let sanitized = trimmed.unicodeScalars.filter {
            !$0.properties.isDefaultIgnorableCodePoint &&
            ($0.value >= 0x20 || $0 == "\n" || $0 == "\t")
        }
        let result = String(String.UnicodeScalarView(sanitized))
        guard !result.isEmpty, result.count <= 100 else { return nil }
        return result
    }

    private func createGroup() {
        guard let sanitizedName = Self.sanitizeGroupName(groupName) else {
            errorMessage = "Group name must be 1-100 characters with no control characters"
            return
        }
        do {
            let (group, code) = try appState.createGroup(name: sanitizedName)
            inviteCode = code
            createdGroup = group
            errorMessage = nil

            // Auto-publish on-chain if contract is configured
            if appState.isContractConfigured {
                publishOnChain()
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func publishOnChain() {
        guard let group = createdGroup else { return }
        onChainStatus = .publishing

        Task {
            do {
                try await appState.publishGroupOnChain(group)
                onChainStatus = .published
            } catch {
                onChainStatus = .failed(error.localizedDescription)
            }
        }
    }
}
