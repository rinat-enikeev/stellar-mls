import SwiftUI

struct GroupListView: View {
    @Environment(AppState.self) private var appState
    @State private var showCreateGroup = false
    @State private var showJoinGroup = false
    @State private var showSettings = false

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
                }
                .onDelete { indices in
                    for index in indices {
                        appState.removeGroup(id: appState.groups[index].id)
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
                        keyManager: appState.keyManager
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
                    Button("Settings", systemImage: "gear") {
                        showSettings = true
                    }
                } label: {
                    Image(systemName: "plus")
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
    }
}
