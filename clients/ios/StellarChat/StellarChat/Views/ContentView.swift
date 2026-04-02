import SwiftUI

struct ContentView: View {
    @Environment(AppState.self) private var appState
    @State private var showDeepLinkJoin = false
    @AppStorage("hasSeenOnboarding") private var hasSeenOnboarding = false

    var body: some View {
        NavigationStack {
            GroupListView()
        }
        .onChange(of: appState.deepLinkInviteCode) { _, newValue in
            if newValue != nil {
                showDeepLinkJoin = true
            }
        }
        .sheet(isPresented: $showDeepLinkJoin, onDismiss: {
            appState.deepLinkInviteCode = nil
        }) {
            DeepLinkJoinGroupView(code: appState.deepLinkInviteCode ?? "")
        }
        .sheet(isPresented: .constant(!hasSeenOnboarding)) {
            OnboardingView(hasSeenOnboarding: $hasSeenOnboarding)
                .interactiveDismissDisabled()
        }
    }
}

// MARK: - Onboarding

private struct OnboardingView: View {
    @Binding var hasSeenOnboarding: Bool
    @State private var currentPage = 0

    private let pages: [(icon: String, title: String, subtitle: String)] = [
        ("lock.shield.fill", "End-to-End Encrypted", "Messages are secured with MLS group encryption. Only group members can read them."),
        ("person.3.fill", "Create & Join Groups", "Start a new group or join one with an invite code. Add members anytime."),
        ("arrow.triangle.2.circlepath", "Key Rotation", "Group keys rotate automatically when members change, keeping conversations secure.")
    ]

    var body: some View {
        VStack(spacing: 0) {
            TabView(selection: $currentPage) {
                ForEach(Array(pages.enumerated()), id: \.offset) { index, page in
                    VStack(spacing: 20) {
                        Spacer()
                        Image(systemName: page.icon)
                            .font(.system(size: 64))
                            .foregroundStyle(.tint)
                        Text(page.title)
                            .font(.title2)
                            .fontWeight(.bold)
                        Text(page.subtitle)
                            .font(.body)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.center)
                            .padding(.horizontal, 32)
                        Spacer()
                    }
                    .tag(index)
                }
            }
            .tabViewStyle(.page(indexDisplayMode: .always))

            Button {
                if currentPage < pages.count - 1 {
                    withAnimation { currentPage += 1 }
                } else {
                    hasSeenOnboarding = true
                }
            } label: {
                Text(currentPage < pages.count - 1 ? "Next" : "Get Started")
                    .fontWeight(.semibold)
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .padding(.horizontal, 32)
            .padding(.bottom, 32)
        }
    }
}

/// A join view pre-filled with the deep link invite code.
private struct DeepLinkJoinGroupView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    let code: String
    @State private var errorMessage: String?
    @State private var joined = false
    @State private var isSyncing = false

    var body: some View {
        NavigationStack {
            Form {
                Section("Invite Code") {
                    Text(code)
                        .font(.caption)
                        .monospaced()
                        .lineLimit(3)
                }
                if let error = errorMessage {
                    Section {
                        Label(error, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(.red)
                    }
                }
                if isSyncing {
                    Section {
                        HStack {
                            ProgressView()
                            Text("Syncing with group members…")
                                .foregroundStyle(.secondary)
                        }
                    }
                } else if joined {
                    Section {
                        Label("Joined successfully!", systemImage: "checkmark.circle")
                            .foregroundStyle(.green)
                    }
                }
            }
            .navigationTitle("Join via Link")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    if !isSyncing {
                        Button("Cancel") { dismiss() }
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    if joined {
                        Button("Done") { dismiss() }
                    } else {
                        Button("Join") { joinGroup() }
                            .disabled(isSyncing)
                    }
                }
            }
            .interactiveDismissDisabled(isSyncing)
        }
    }

    private func joinGroup() {
        do {
            let invite = try InviteCode.decode(from: code)
            let groupIDHex = invite.groupID.map { String(format: "%02x", $0) }.joined()
            if appState.groups.contains(where: { $0.id == groupIDHex }) {
                errorMessage = "You're already in this group."
                return
            }
            let codeRelays = invite.relayHints.compactMap(URL.init(string:))
            let mergedRelays = Array(Set(appState.relayURLs + codeRelays))
            // Add ourselves to the member list so our own messages pass BLS auth
            var members = invite.members
            let myLeaf = try appState.keyManager.memberLeaf
            if !members.contains(where: { $0.publicKeyCompressed == myLeaf.publicKeyCompressed }) {
                members.append(myLeaf)
            }

            let group = ChatGroup(
                id: groupIDHex,
                name: invite.name,
                groupSecret: invite.groupSecret,
                createdAt: Date(),
                relayHints: mergedRelays.isEmpty ? appState.relayURLs : mergedRelays,
                members: members,
                epoch: invite.epoch,
                salt: invite.salt,
                commitment: invite.commitment
            )
            appState.addGroup(group)
            joined = true
            Task {
                await appState.announceMemberJoined(group: group)
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
