import SwiftMLS
import SwiftUI

struct SettingsView: View {
    @Environment(AppState.self) private var appState
    @State private var tab: SettingsTab = .invite
    @State private var showJoin = false
    @State private var showAbout = false
    @State private var showShareSheet = false

    enum SettingsTab: Hashable {
        case invite, preferences
    }

    var body: some View {
        VStack(spacing: 0) {
            Picker("", selection: $tab) {
                Text("Invite Key").tag(SettingsTab.invite)
                Text("Preferences").tag(SettingsTab.preferences)
            }
            .pickerStyle(.segmented)
            .padding(.horizontal, 16)
            .padding(.top, 4)
            .padding(.bottom, 8)

            switch tab {
            case .invite:
                inviteKeyTab
            case .preferences:
                preferencesTab
            }
        }
        .navigationTitle("Settings")
        .sheet(isPresented: $showJoin) {
            JoinGroupView()
        }
        .sheet(isPresented: $showAbout) {
            OnboardingView(hasSeenOnboarding: .constant(true), isRevisit: true)
        }
        .sheet(isPresented: $showShareSheet) {
            ShareSheet(items: [appState.keyManager.keyAgreementPublicKeyHex])
        }
    }

    // MARK: - Invite Key Tab

    private var inviteKeyTab: some View {
        List {
            Section {
                qrHeroCard
                    .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
                    .listRowSeparator(.hidden)
                    .listRowBackground(Color.clear)
            }

            Section {
                Button(action: { showJoin = true }) {
                    HStack(spacing: 12) {
                        SettingsIconBox(systemImage: "qrcode.viewfinder", background: .green)
                        Text("Scan invite QR")
                            .foregroundStyle(Color.primary)
                        Spacer()
                        Image(systemName: "chevron.right")
                            .font(.footnote.weight(.semibold))
                            .foregroundStyle(.tertiary)
                    }
                }
                Button(action: { showJoin = true }) {
                    HStack(spacing: 12) {
                        SettingsIconBox(systemImage: "key.fill", background: .purple)
                        Text("Paste invite key")
                            .foregroundStyle(Color.primary)
                        Spacer()
                        Image(systemName: "chevron.right")
                            .font(.footnote.weight(.semibold))
                            .foregroundStyle(.tertiary)
                    }
                }
            } header: {
                Text("Find friends")
            } footer: {
                Text("Share this QR code so others can find you. Your invite key is your X25519 inbox key.")
            }
        }
        .listStyle(.insetGrouped)
    }

    private var qrHeroCard: some View {
        VStack(spacing: 14) {
            // QR inside a framed inner card
            QRCodeView(appState.keyManager.keyAgreementPublicKeyHex, size: 260)
                .padding(14)
                .background(
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .fill(Color(UIColor.systemBackground))
                        .overlay(
                            RoundedRectangle(cornerRadius: 16, style: .continuous)
                                .strokeBorder(Color.primary.opacity(0.04), lineWidth: 1)
                        )
                )

            HStack(spacing: 8) {
                Button(action: { showShareSheet = true }) {
                    Label("Share link", systemImage: "square.and.arrow.up")
                        .font(.body.weight(.semibold))
                        .foregroundStyle(.white)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 12)
                        .background(Color.accentColor, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                }
                Button {
                    UIPasteboard.general.string = appState.keyManager.keyAgreementPublicKeyHex
                } label: {
                    Label("Copy", systemImage: "doc.on.doc")
                        .font(.body.weight(.semibold))
                        .foregroundStyle(Color.accentColor)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 12)
                        .background(Color.accentColor.opacity(0.12), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                }
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity)
        .background(
            RoundedRectangle(cornerRadius: 22, style: .continuous)
                .fill(Color(UIColor.secondarySystemGroupedBackground))
        )
    }

    // MARK: - Preferences Tab

    private var preferencesTab: some View {
        @Bindable var appState = appState

        return List {
            // Network
            Section {
                NavigationLink {
                    RelaysView()
                } label: {
                    row(
                        icon: SettingsIconBox(systemImage: "antenna.radiowaves.left.and.right", background: .orange),
                        title: "Relays",
                        detail: "\(appState.relayURLs.count)"
                    )
                }
                NavigationLink {
                    BlossomServersView()
                } label: {
                    row(
                        icon: SettingsIconBox(systemImage: "photo.on.rectangle.angled", background: .purple),
                        title: "Blossom Servers",
                        detail: "\(appState.blossomServerURLs.count)"
                    )
                }
                NavigationLink {
                    TurnServersView()
                } label: {
                    row(
                        icon: SettingsIconBox(systemImage: "globe", background: Color(red: 0.35, green: 0.78, blue: 0.98)),
                        title: "TURN Servers",
                        detail: appState.turnEnabled ? "Custom" : "EU default"
                    )
                }
            } header: {
                Text("Network")
            } footer: {
                Text("Relays carry encrypted messages. Blossom servers store encrypted media. TURN aids calls behind restrictive networks.")
            }

            // Protocol
            Section {
                NavigationLink {
                    StellarContractView()
                } label: {
                    HStack(spacing: 12) {
                        SettingsIconBox(systemImage: "star.fill", background: .yellow)
                        Text("Stellar Contract")
                        Spacer()
                        HStack(spacing: 5) {
                            Circle()
                                .fill(appState.isContractConfigured ? Color.green : Color.gray)
                                .frame(width: 6, height: 6)
                            Text(appState.isContractConfigured ? "Connected" : "Not configured")
                                .font(.callout)
                                .foregroundStyle(appState.isContractConfigured ? .green : .secondary)
                        }
                    }
                }

                Picker(selection: $appState.defaultGroupTier) {
                    Text("32 members").tag(SEPTier.small)
                    Text("256 members").tag(SEPTier.medium)
                    Text("2,048 members").tag(SEPTier.large)
                } label: {
                    row(
                        icon: SettingsIconBox(systemImage: "person.3.fill", background: .green),
                        title: "Default Group Capacity",
                        detail: nil
                    )
                }
            } header: {
                Text("Protocol")
            } footer: {
                Text("New groups will use this tier. Larger tiers support more members but use bigger ZK circuits.")
            }

            // Advanced
            Section {
                NavigationLink {
                    AdvancedSettingsView()
                } label: {
                    row(
                        icon: SettingsIconBox(systemImage: "gearshape.fill", background: .gray),
                        title: "Advanced",
                        detail: nil
                    )
                }
            } header: {
                Text("Advanced")
            }

            // About
            Section {
                row(
                    icon: SettingsIconBox(systemImage: "info.circle.fill", background: .gray),
                    title: "Version",
                    detail: "1.6.1"
                )
                row(
                    icon: SettingsIconBox(systemImage: "lock.shield.fill", background: .blue),
                    title: "End-to-end encryption",
                    detail: nil
                )
                Button {
                    showAbout = true
                } label: {
                    row(
                        icon: SettingsIconBox(systemImage: "sparkles", background: .pink),
                        title: "About Stellar Chat",
                        detail: nil,
                        titleColor: .accentColor
                    )
                }
            } header: {
                Text("About")
            } footer: {
                Text("Messages are encrypted end-to-end. Relays see only opaque ciphertext and hidden topic tags. Group IDs never appear in cleartext on the wire.")
            }
        }
        .listStyle(.insetGrouped)
    }

    // MARK: - Row helper

    private func row(
        icon: SettingsIconBox,
        title: String,
        detail: String?,
        titleColor: Color = .primary
    ) -> some View {
        HStack(spacing: 12) {
            icon
            Text(title)
                .foregroundStyle(titleColor)
            Spacer()
            if let detail {
                Text(detail)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

// MARK: - Share sheet bridge

private struct ShareSheet: UIViewControllerRepresentable {
    let items: [Any]

    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: items, applicationActivities: nil)
    }

    func updateUIViewController(_ uiViewController: UIActivityViewController, context: Context) {}
}
