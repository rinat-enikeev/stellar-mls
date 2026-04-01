import SwiftUI

struct GroupListView: View {
    @Environment(AppState.self) private var appState
    @State private var showCreateGroup = false
    @State private var showJoinGroup = false
    @State private var showSettings = false
    @State private var showInvitations = false
    @State private var inviteMemberGroup: ChatGroup?
    @State private var verificationResult: OnChainVerificationResult?
    @State private var verifyingGroupID: String?
    @State private var showVerificationAlert = false

    var body: some View {
        List {
            if appState.groups.isEmpty {
                ContentUnavailableView(
                    "No Groups",
                    systemImage: "bubble.left.and.bubble.right",
                    description: Text("Create a group or join one with an invite code.")
                )
            } else {
                ForEach(appState.groups) { group in
                    NavigationLink(value: group.id) {
                        HStack {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(group.name)
                                    .font(.headline)
                                Text("\(group.members.count) members | Epoch \(group.epoch)")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                Text("Topic: \(group.topicTag)")
                                    .font(.caption2)
                                    .foregroundStyle(.tertiary)
                            }
                            Spacer()
                            if group.isPublishedOnChain {
                                Image(systemName: "checkmark.seal.fill")
                                    .foregroundStyle(.green)
                                    .font(.caption)
                            } else if appState.isContractConfigured {
                                Image(systemName: "circle")
                                    .foregroundStyle(.secondary)
                                    .font(.caption2)
                            }
                        }
                        .padding(.vertical, 4)
                    }
                    .swipeActions(edge: .trailing) {
                        Button(role: .destructive) {
                            appState.removeGroup(id: group.id)
                        } label: {
                            Label("Delete", systemImage: "trash")
                        }
                    }
                    .swipeActions(edge: .leading) {
                        if appState.isContractConfigured {
                            Button {
                                verifyGroup(group)
                            } label: {
                                Label("Verify", systemImage: "checkmark.seal")
                            }
                            .tint(.green)
                        }
                        Button {
                            inviteMemberGroup = group
                        } label: {
                            Label("Invite", systemImage: "person.badge.plus")
                        }
                        .tint(.blue)
                    }
                }
            }
        }
        .navigationTitle("Stellar Chat")
        .navigationDestination(for: String.self) { groupID in
            if appState.groups.contains(where: { $0.id == groupID }) {
                ChatView(
                    viewModel: ChatViewModel(
                        groupID: groupID,
                        appState: appState
                    )
                )
            }
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Menu {
                    Button("Create Group", systemImage: "plus.circle") {
                        showCreateGroup = true
                    }
                    Button("Join Group", systemImage: "person.badge.plus") {
                        showJoinGroup = true
                    }
                    Divider()
                    Button {
                        showInvitations = true
                    } label: {
                        Label(
                            "Invitations\(appState.pendingInvitations.isEmpty ? "" : " (\(appState.pendingInvitations.count))")",
                            systemImage: "envelope"
                        )
                    }
                    Divider()
                    Button("Settings", systemImage: "gear") {
                        showSettings = true
                    }
                } label: {
                    ZStack(alignment: .topTrailing) {
                        Image(systemName: "plus")
                        if !appState.pendingInvitations.isEmpty {
                            Circle()
                                .fill(.red)
                                .frame(width: 8, height: 8)
                                .offset(x: 4, y: -4)
                        }
                    }
                }
            }
        }
        .sheet(isPresented: $showCreateGroup) {
            CreateGroupView()
        }
        .sheet(isPresented: $showJoinGroup) {
            JoinGroupView()
        }
        .sheet(isPresented: $showSettings) {
            SettingsView()
        }
        .sheet(isPresented: $showInvitations) {
            PendingInvitationsView()
        }
        .sheet(item: $inviteMemberGroup) { group in
            InviteMemberView(group: group)
        }
        .alert(
            "On-Chain Verification",
            isPresented: $showVerificationAlert,
            presenting: verificationResult
        ) { _ in
            Button("OK") {}
        } message: { result in
            Text(result.displayText)
        }
    }

    private func verifyGroup(_ group: ChatGroup) {
        verifyingGroupID = group.id
        Task {
            let result = await appState.verifyGroupOnChain(group)
            verificationResult = result
            verifyingGroupID = nil
            showVerificationAlert = true
        }
    }
}
