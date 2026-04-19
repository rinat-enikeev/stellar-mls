import SwiftMLS
import SwiftUI

struct ContentView: View {
    private enum RootTab: Hashable {
        case contacts
        case chats
        case search
        case settings
    }

    @Environment(AppState.self) private var appState
    @State private var showDeepLinkJoin = false
    @State private var showOnboardLanding = false
    @State private var onboardInviter: String = ""
    @State private var onboardNonce: String = ""
    @State private var selectedTab: RootTab = .chats
    @State private var chatNavigationPath = NavigationPath()
    @AppStorage("hasSeenOnboarding") private var hasSeenOnboarding = false

    var body: some View {
        TabView(selection: $selectedTab) {
            Tab("Contacts", systemImage: "person.2", value: .contacts) {
                NavigationStack {
                    ContactsView()
                }
            }

            Tab("Chats", systemImage: "bubble.left.and.bubble.right", value: .chats) {
                NavigationStack(path: $chatNavigationPath) {
                    GroupListView()
                }
            }

            Tab("Search", systemImage: "magnifyingglass", value: .search, role: .search) {
                NavigationStack {
                    SearchView()
                }
            }

            Tab("Settings", systemImage: "gearshape", value: .settings) {
                NavigationStack {
                    SettingsView()
                }
            }
        }
        .onChange(of: appState.deepLinkInviteCode) { _, newValue in
            if newValue != nil {
                showDeepLinkJoin = true
            }
        }
        .onChange(of: appState.deepLinkOnboard?.nonce) { _, _ in
            if let onboard = appState.deepLinkOnboard {
                onboardInviter = onboard.inviter
                onboardNonce = onboard.nonce
                showOnboardLanding = true
            }
        }
        .onChange(of: appState.navigateToGroupID) { _, newValue in
            if let groupID = newValue {
                selectedTab = .chats
                // Pop to root then push the chat
                chatNavigationPath = NavigationPath()
                chatNavigationPath.append(groupID)
                appState.navigateToGroupID = nil
            }
        }
        .sheet(isPresented: $showDeepLinkJoin, onDismiss: {
            appState.deepLinkInviteCode = nil
        }) {
            DeepLinkJoinGroupView(code: appState.deepLinkInviteCode ?? "")
        }
        .sheet(isPresented: $showOnboardLanding, onDismiss: {
            appState.deepLinkOnboard = nil
        }) {
            OnboardLandingView(
                inviterX25519Hex: onboardInviter,
                nonceHex: onboardNonce
            )
        }
        .sheet(isPresented: .constant(!hasSeenOnboarding)) {
            OnboardingView(hasSeenOnboarding: $hasSeenOnboarding)
                .interactiveDismissDisabled()
        }
    }
}

// MARK: - Onboarding

struct OnboardingView: View {
    @Binding var hasSeenOnboarding: Bool
    var isRevisit: Bool = false
    @Environment(\.dismiss) private var dismiss
    @State private var currentPage = 0
    @State private var showRestore = false

    private let pages: [(icon: String, title: String, subtitle: String)] = [
        ("eye.trianglebadge.exclamationmark", "Now your messages and\nmetadata are encrypted", "Most messengers encrypt your messages but still collect who you talk to, when, and how often. That metadata tells a complete story about you."),
        ("person.badge.shield.checkmark", "Private by design.\nAnonymous by default.", "No phone numbers. No accounts. No social graph. Even other group members won't know anything about you beyond what you choose to share."),
        ("person.3.sequence", "Truly shared ownership.\nNo super-admin.", "Your group's legacy doesn't depend on a single super-admin. From the start, set transparent rules for adding and removing members — like via voting.")
    ]

    private let totalPages = 4

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
                            .multilineTextAlignment(.center)
                        Text(page.subtitle)
                            .font(.body)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.center)
                            .padding(.horizontal, 32)
                        Spacer()
                    }
                    .tag(index)
                }

                // Screen D: Differentiator
                VStack(spacing: 20) {
                    Spacer()
                    Text("What makes this different")
                        .font(.title2)
                        .fontWeight(.bold)
                    VStack(alignment: .leading, spacing: 16) {
                        DiffRow(icon: "checkmark.circle.fill", iconColor: .green, text: "Your content is encrypted", detail: "like other apps")
                        DiffRow(icon: "checkmark.seal.fill", iconColor: .blue, text: "Your identity is protected", detail: "unlike other apps")
                        DiffRow(icon: "checkmark.seal.fill", iconColor: .blue, text: "Your metadata can't be harvested", detail: "unlike other apps")
                        DiffRow(icon: "checkmark.seal.fill", iconColor: .blue, text: "No single admin holds the keys", detail: "unlike other apps")
                    }
                    .padding(.horizontal, 32)
                    Spacer()
                }
                .tag(pages.count)
            }
            .tabViewStyle(.page(indexDisplayMode: .always))

            Button {
                if currentPage < totalPages - 1 {
                    withAnimation { currentPage += 1 }
                } else if isRevisit {
                    dismiss()
                } else {
                    hasSeenOnboarding = true
                }
            } label: {
                let lastPageText = isRevisit ? "Done" : "Get Started"
                Text(currentPage < totalPages - 1 ? "Next" : lastPageText)
                    .fontWeight(.semibold)
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .padding(.horizontal, 32)

            if !isRevisit {
                Button("Restore from Recovery Phrase") {
                    showRestore = true
                }
                .font(.footnote)
                .padding(.top, 8)
            }

            Spacer().frame(height: 32)
        }
        .sheet(isPresented: $showRestore) {
            RestoreIdentityView(onRestoreComplete: {
                hasSeenOnboarding = true
            })
        }
    }
}

private struct DiffRow: View {
    let icon: String
    let iconColor: Color
    let text: String
    let detail: String

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: icon)
                .font(.title3)
                .foregroundStyle(iconColor)
            VStack(alignment: .leading, spacing: 2) {
                Text(text)
                    .font(.body)
                    .fontWeight(.medium)
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
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
    @State private var decodedGroup: ChatGroup?
    @State private var verificationResult: OnChainVerificationResult?
    @State private var isVerifying = false

    var body: some View {
        NavigationStack {
            Form {
                Section("Invite Code") {
                    Text(code)
                        .font(.caption)
                        .monospaced()
                        .lineLimit(3)
                }

                if let group = decodedGroup {
                    Section("Group Preview") {
                        LabeledContent("Name", value: group.name)
                        LabeledContent("Members", value: "\(group.members.count)")
                        LabeledContent("Epoch", value: "\(group.epoch)")

                        if let result = verificationResult {
                            VerificationBadgeView(result: result)
                            if result == .inactive {
                                Text("This group has been deactivated on-chain and cannot be joined.")
                                    .font(.caption)
                                    .foregroundStyle(.red)
                            }
                        } else if isVerifying {
                            HStack(spacing: 4) {
                                ProgressView()
                                    .controlSize(.mini)
                                Text("Verifying on-chain...")
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
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
                        Button("Join") { confirmJoin() }
                            .disabled(isSyncing || decodedGroup == nil || verificationResult == .inactive)
                    }
                }
            }
            .interactiveDismissDisabled(isSyncing)
            .task { await decodeAndVerify() }
        }
    }

    private func decodeAndVerify() async {
        do {
            let invite = try InviteCode.decode(from: code)
            let groupIDHex = invite.groupID.map { String(format: "%02x", $0) }.joined()
            if appState.groups.contains(where: { $0.id == groupIDHex }) {
                errorMessage = "You're already in this group."
                return
            }
            let codeRelays = invite.relayHints.compactMap(URL.init(string:))
            let mergedRelays = Array(Set(appState.relayURLs + codeRelays))
            let group = ChatGroup(
                id: groupIDHex,
                name: invite.name,
                groupSecret: invite.groupSecret,
                createdAt: Date(),
                relayHints: mergedRelays.isEmpty ? appState.relayURLs : mergedRelays,
                members: invite.members,
                epoch: invite.epoch,
                salt: invite.salt,
                commitment: invite.commitment,
                tier: SEPTier(rawValue: invite.tierRawValue) ?? .large
            )
            decodedGroup = group

            guard appState.isContractConfigured else { return }
            isVerifying = true
            let result = await appState.verifyGroupOnChain(group)
            verificationResult = result
            isVerifying = false
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func confirmJoin() {
        guard var group = decodedGroup else { return }
        if verificationResult == .verified {
            group.isPublishedOnChain = true
        }
        // Add ourselves to the member list so our own messages pass BLS auth
        if let myLeaf = try? appState.keyManager.memberLeaf,
           !group.members.contains(where: { $0.publicKeyCompressed == myLeaf.publicKeyCompressed }) {
            group.members.append(myLeaf)
        }
        appState.addGroup(group)
        joined = true
        Task {
            await appState.announceMemberJoined(group: group)
        }
    }
}
