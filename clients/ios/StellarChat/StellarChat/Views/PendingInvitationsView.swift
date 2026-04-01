import SwiftUI

struct PendingInvitationsView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var verificationResults: [String: OnChainVerificationResult] = [:]
    @State private var verifyingIDs: Set<String> = []
    @State private var syncingID: String?
    @State private var syncedID: String?

    var body: some View {
        NavigationStack {
            Group {
                if appState.pendingInvitations.isEmpty {
                    ContentUnavailableView(
                        "No Invitations",
                        systemImage: "envelope",
                        description: Text("Invitations sent to your inbox key will appear here.")
                    )
                } else {
                    List {
                        ForEach(appState.pendingInvitations) { invitation in
                            VStack(alignment: .leading, spacing: 6) {
                                Text(invitation.payload.name)
                                    .font(.headline)

                                HStack {
                                    Text("From: \(invitation.payload.senderNostrPubkey.prefix(12))...")
                                        .font(.caption)
                                        .monospaced()
                                        .foregroundStyle(.secondary)
                                }

                                HStack {
                                    Text("\(invitation.payload.members.count) members")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                    Text("Epoch \(invitation.payload.epoch)")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }

                                // On-chain verification status
                                if let result = verificationResults[invitation.id] {
                                    verificationBadge(result)
                                } else if verifyingIDs.contains(invitation.id) {
                                    HStack(spacing: 4) {
                                        ProgressView()
                                            .controlSize(.mini)
                                        Text("Verifying on-chain...")
                                            .font(.caption2)
                                            .foregroundStyle(.secondary)
                                    }
                                }

                                if syncingID == invitation.id {
                                    HStack(spacing: 8) {
                                        ProgressView()
                                            .controlSize(.small)
                                        Text("Syncing with group members…")
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                    }
                                    .padding(.top, 4)
                                } else if syncedID == invitation.id {
                                    Label("Joined successfully!", systemImage: "checkmark.circle")
                                        .font(.caption)
                                        .foregroundStyle(.green)
                                        .padding(.top, 4)
                                } else {
                                    HStack(spacing: 12) {
                                        Button("Accept") {
                                            acceptInvitation(invitation)
                                        }
                                        .buttonStyle(.borderedProminent)
                                        .controlSize(.small)
                                        .disabled(syncingID != nil)

                                        Button("Decline") {
                                            declineInvitation(invitation)
                                        }
                                        .buttonStyle(.bordered)
                                        .controlSize(.small)
                                        .tint(.red)
                                        .disabled(syncingID != nil)
                                    }
                                    .padding(.top, 4)
                                }
                            }
                            .padding(.vertical, 4)
                            .task {
                                await verifyInvitation(invitation)
                            }
                        }
                    }
                }
            }
            .navigationTitle("Invitations")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .disabled(syncingID != nil)
                }
            }
            .interactiveDismissDisabled(syncingID != nil)
        }
    }

    @ViewBuilder
    private func verificationBadge(_ result: OnChainVerificationResult) -> some View {
        HStack(spacing: 4) {
            switch result {
            case .verified:
                Image(systemName: "checkmark.seal.fill")
                    .foregroundStyle(.green)
                Text("Verified on-chain")
                    .foregroundStyle(.green)
            case .notPublished:
                Image(systemName: "circle")
                    .foregroundStyle(.secondary)
                Text("Not yet published on-chain")
                    .foregroundStyle(.secondary)
            case .epochMismatch(let local, let onChain):
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                Text("Epoch mismatch: local \(local), on-chain \(onChain)")
                    .foregroundStyle(.orange)
            case .commitmentMismatch:
                Image(systemName: "xmark.seal.fill")
                    .foregroundStyle(.red)
                Text("Commitment mismatch")
                    .foregroundStyle(.red)
            case .inactive:
                Image(systemName: "minus.circle.fill")
                    .foregroundStyle(.red)
                Text("Group deactivated")
                    .foregroundStyle(.red)
            case .error:
                Image(systemName: "questionmark.circle")
                    .foregroundStyle(.secondary)
                Text("Could not verify")
                    .foregroundStyle(.secondary)
            }
        }
        .font(.caption2)
    }

    /// Verify invitation's commitment and epoch against on-chain state.
    private func verifyInvitation(_ invitation: PendingInvitation) async {
        guard appState.isContractConfigured else { return }
        guard !verifyingIDs.contains(invitation.id) else { return }

        verifyingIDs.insert(invitation.id)

        let payload = invitation.payload
        let tempGroup = payload.toChatGroup()
        let result = await appState.verifyGroupOnChain(tempGroup)

        verificationResults[invitation.id] = result
        verifyingIDs.remove(invitation.id)
    }

    private func acceptInvitation(_ invitation: PendingInvitation) {
        var group = invitation.payload.toChatGroup()
        let groupIDHex = group.id

        // Check if already joined
        if appState.groups.contains(where: { $0.id == groupIDHex }) {
            appState.removePendingInvitation(id: invitation.id)
            return
        }

        // Mark as published if on-chain verification passed
        if let result = verificationResults[invitation.id], result == .verified {
            group.isPublishedOnChain = true
        }

        // Add ourselves to the member list so our messages pass BLS auth
        if let myLeaf = try? appState.keyManager.memberLeaf,
           !group.members.contains(where: { $0.publicKeyCompressed == myLeaf.publicKeyCompressed }) {
            group.members.append(myLeaf)
        }

        appState.addGroup(group)
        syncedID = invitation.id
        appState.removePendingInvitation(id: invitation.id)

        // Announce ourselves to existing members (fire-and-forget)
        Task {
            await appState.announceMemberJoined(group: group)
        }
    }

    private func declineInvitation(_ invitation: PendingInvitation) {
        appState.removePendingInvitation(id: invitation.id)
    }
}
