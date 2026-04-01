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
                        Text("Share this invite code with group members. Anyone with this code can join and read messages.")
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

    private func createGroup() {
        do {
            let (group, code) = try appState.createGroup(name: groupName)
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
