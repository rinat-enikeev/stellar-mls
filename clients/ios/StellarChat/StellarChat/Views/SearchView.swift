import SwiftUI

struct SearchView: View {
    @Environment(AppState.self) private var appState
    @State private var searchText = ""
    @State private var isSearching = false

    private var filteredGroups: [ChatGroup] {
        guard !searchText.isEmpty else { return [] }
        return appState.groups.filter { $0.name.localizedCaseInsensitiveContains(searchText) }
    }

    private var messageResults: [(group: ChatGroup, message: ChatMessage)] {
        guard !searchText.isEmpty else { return [] }
        var results: [(ChatGroup, ChatMessage)] = []
        for group in appState.groups {
            guard let messages = appState.chatMessages[group.id] else { continue }
            for msg in messages where msg.text.localizedCaseInsensitiveContains(searchText) {
                results.append((group, msg))
            }
        }
        return Array(results.sorted { $0.1.timestamp > $1.1.timestamp }.prefix(50))
    }

    var body: some View {
        List {
            if searchText.isEmpty {
                ContentUnavailableView(
                    "Search Messages",
                    systemImage: "magnifyingglass",
                    description: Text("Search across all your chats and messages.")
                )
            } else if filteredGroups.isEmpty && messageResults.isEmpty {
                ContentUnavailableView.search(text: searchText)
            } else {
                if !filteredGroups.isEmpty {
                    Section("Chats") {
                        ForEach(filteredGroups) { group in
                            NavigationLink(value: group.id) {
                                HStack {
                                    Text(group.name)
                                        .font(.body)
                                    Spacer()
                                    Text("\(group.members.count) members")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                    }
                }

                if !messageResults.isEmpty {
                    Section("Messages") {
                        ForEach(messageResults, id: \.message.id) { result in
                            NavigationLink(value: result.group.id) {
                                VStack(alignment: .leading, spacing: 4) {
                                    HStack {
                                        Text(result.group.name)
                                            .font(.caption)
                                            .fontWeight(.semibold)
                                        Spacer()
                                        Text(relativeTimestamp(result.message.timestamp))
                                            .font(.caption2)
                                            .foregroundStyle(.secondary)
                                    }
                                    Text(result.message.text)
                                        .font(.subheadline)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(2)
                                }
                                .padding(.vertical, 2)
                            }
                        }
                    }
                }
            }
        }
        .navigationDestination(for: String.self) { groupID in
            ChatViewContainer(groupID: groupID)
        }
        .searchable(text: $searchText, isPresented: $isSearching, prompt: "Chats and messages")
        .navigationTitle("Search")
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
