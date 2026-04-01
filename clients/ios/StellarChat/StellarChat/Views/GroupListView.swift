import SwiftUI

struct GroupListView: View {
    @Environment(AppState.self) private var appState
    @State private var showCreateGroup = false
    @State private var showJoinGroup = false
    @State private var showSettings = false
    @State private var showInvitations = false
    @State private var inviteMemberGroup: ChatGroup?

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
                        VStack(alignment: .leading, spacing: 4) {
                            Text(group.name)
                                .font(.headline)
                            Text("Topic: \(group.topicTag)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
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
            if let group = appState.groups.first(where: { $0.id == groupID }) {
                ChatView(
                    viewModel: ChatViewModel(
                        group: group,
                        transport: NostrMessageTransport(),
                        keyManager: appState.keyManager,
                        store: appState.store
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
    }
}
