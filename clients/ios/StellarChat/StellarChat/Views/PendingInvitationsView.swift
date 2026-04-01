import SwiftUI

struct PendingInvitationsView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

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

                                HStack(spacing: 12) {
                                    Button("Accept") {
                                        acceptInvitation(invitation)
                                    }
                                    .buttonStyle(.borderedProminent)
                                    .controlSize(.small)

                                    Button("Decline") {
                                        declineInvitation(invitation)
                                    }
                                    .buttonStyle(.bordered)
                                    .controlSize(.small)
                                    .tint(.red)
                                }
                                .padding(.top, 4)
                            }
                            .padding(.vertical, 4)
                        }
                    }
                }
            }
            .navigationTitle("Invitations")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }

    private func acceptInvitation(_ invitation: PendingInvitation) {
        let group = invitation.payload.toChatGroup()
        let groupIDHex = group.id

        // Check if already joined
        if appState.groups.contains(where: { $0.id == groupIDHex }) {
            // Already in this group, just remove the invitation
            appState.removePendingInvitation(id: invitation.id)
            return
        }

        appState.addGroup(group)
        appState.removePendingInvitation(id: invitation.id)
    }

    private func declineInvitation(_ invitation: PendingInvitation) {
        appState.removePendingInvitation(id: invitation.id)
    }
}
