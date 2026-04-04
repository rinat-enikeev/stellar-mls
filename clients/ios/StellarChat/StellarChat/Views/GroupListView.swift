import SwiftUI

struct GroupListView: View {
    @Environment(AppState.self) private var appState
    @State private var showCreateGroup = false
    @State private var showJoinGroup = false
    @State private var showInvitations = false
    @State private var inviteMemberGroup: ChatGroup?
    @State private var verificationResult: OnChainVerificationResult?
    @State private var verifyingGroupID: String?
    @State private var showVerificationAlert = false

    var body: some View {
        List {
            if appState.groups.isEmpty {
                ContentUnavailableView(
                    "No Chats",
                    systemImage: "bubble.left.and.bubble.right",
                    description: Text("Create a group or join one with an invite code.")
                )
            } else {
                ForEach(appState.groups) { group in
                    NavigationLink(value: group.id) {
                        GroupRow(
                            group: group,
                            lastMessage: appState.chatMessages[group.id]?.last,
                            unreadCount: appState.unreadCounts[group.id] ?? 0,
                            isContractConfigured: appState.isContractConfigured
                        )
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
        .navigationTitle("Chats")
        .toolbarTitleMenu {
            if !appState.isRelayConnected {
                Label("Disconnected", systemImage: "bolt.slash.fill")
                    .foregroundStyle(.red)
            }
        }
        .safeAreaInset(edge: .top) {
            if !appState.isRelayConnected {
                HStack(spacing: 6) {
                    Image(systemName: "bolt.slash.fill")
                        .font(.caption2)
                    Text("No relay connection")
                        .font(.caption2)
                }
                .foregroundStyle(.white)
                .padding(.vertical, 4)
                .frame(maxWidth: .infinity)
                .background(.red.opacity(0.85))
            }
        }
        .refreshable {
            await appState.reconnectRelays()
        }
        .navigationDestination(for: String.self) { groupID in
            ChatViewContainer(groupID: groupID)
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                HStack(spacing: 4) {
                    Button {
                        showInvitations = true
                    } label: {
                        ZStack(alignment: .topTrailing) {
                            Image(systemName: "envelope")
                            if !appState.pendingInvitations.isEmpty {
                                Circle()
                                    .fill(.red)
                                    .frame(width: 8, height: 8)
                                    .offset(x: 4, y: -4)
                            }
                        }
                    }
                    Menu {
                        Button("Create Group", systemImage: "plus.circle") {
                            showCreateGroup = true
                        }
                        Button("Join Group", systemImage: "person.badge.plus") {
                            showJoinGroup = true
                        }
                    } label: {
                        Image(systemName: "plus")
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

// MARK: - Group Row

struct GroupRow: View {
    let group: ChatGroup
    let lastMessage: ChatMessage?
    let unreadCount: Int
    let isContractConfigured: Bool

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(group.name)
                        .font(.headline)
                    Spacer()
                    if let lastMsg = lastMessage {
                        Text(relativeTimestamp(lastMsg.timestamp))
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
                HStack {
                    Text("\(group.members.count) members")
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    if let lastMsg = lastMessage {
                        Text("· \(lastMsg.text)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }

                    Spacer()

                    if unreadCount > 0 {
                        Text("\(unreadCount)")
                            .font(.caption2)
                            .fontWeight(.bold)
                            .foregroundStyle(.white)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(.red, in: Capsule())
                    }
                }
            }

            if group.forkedFromGroupID != nil {
                Image(systemName: "arrow.triangle.branch")
                    .foregroundStyle(.purple)
                    .font(.caption)
            }
            if group.isPublishedOnChain {
                Image(systemName: "checkmark.seal.fill")
                    .foregroundStyle(.green)
                    .font(.caption)
            } else if isContractConfigured {
                Image(systemName: "circle")
                    .foregroundStyle(.secondary)
                    .font(.caption2)
            }
        }
        .padding(.vertical, 4)
    }

    private func relativeTimestamp(_ date: Date) -> String {
        let seconds = -date.timeIntervalSinceNow
        if seconds < 60 { return "Just now" }
        if seconds < 3600 { return "\(Int(seconds / 60))m" }
        if seconds < 86400 { return "\(Int(seconds / 3600))h" }
        if Calendar.current.isDateInYesterday(date) { return "Yesterday" }
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d"
        return formatter.string(from: date)
    }
}

// MARK: - Chat View Container

/// Holds `ChatViewModel` in `@State` so that `GroupListView` re-renders
/// (triggered by incoming messages updating `chatMessages` / `groups`) do NOT
/// recreate the view model. Without this wrapper the `navigationDestination`
/// closure ran on every parent re-render, building a brand-new ChatViewModel
/// and ChatView — forcing ForEach to rebuild every visible message cell from
/// scratch instead of diffing, and triggering observation cascades from the
/// init side-effects (`activeGroupID`, `unreadCounts`).
struct ChatViewContainer: View {
    let groupID: String
    @Environment(AppState.self) private var appState
    @State private var viewModel: ChatViewModel?

    var body: some View {
        Group {
            if let viewModel {
                ChatView(viewModel: viewModel)
            }
        }
        .onAppear {
            if viewModel == nil {
                viewModel = ChatViewModel(groupID: groupID, appState: appState)
            }
        }
    }
}
