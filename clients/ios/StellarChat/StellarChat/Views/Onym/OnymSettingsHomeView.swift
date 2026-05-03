import SwiftUI

/// Top-level entry the rest of the app composes. Wraps the home screen in a
/// NavigationStack so every subscreen pushes/pops with the system gesture.
struct OnymSettingsHomeView: View {
    @Environment(AppState.self) private var appState
    @State private var model: OnymSettingsModel?

    var body: some View {
        Group {
            if let model {
                OnymSettingsHome(model: model)
            } else {
                Color.clear.onAppear { model = makeModel() }
            }
        }
    }

    private func makeModel() -> OnymSettingsModel {
        let active = OnymIdentity(
            id: "id-active",
            name: "Identity",
            npub: appState.keyManager.publicKeyHex,
            backedUp: appState.keyManager.isBip39Backed,
            active: true,
            created: "Apr 2 2026"
        )
        return OnymSettingsModel(activeIdentity: active)
    }
}

struct OnymSettingsHome: View {
    @Bindable var model: OnymSettingsModel

    var body: some View {
        let active = model.activeIdentity ?? model.identities[0]
        let allBackedUp = model.identities.allSatisfy { $0.backedUp }
        @Bindable var bm = model

        return OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: "Settings")
                OnymLargeTitle(text: "Settings")

                identityHero(active)
                qrHero(active)

                if !allBackedUp {
                    notBackedUpBanner(count: model.identities.filter { !$0.backedUp }.count)
                }

                OnymSectionLabel(text: "SECURITY")
                OnymCard {
                    NavigationLink { OnymIdentitiesView(model: model) } label: {
                        OnymRow(
                            title: "Identities",
                            subtitle: "\(model.identities.count) · \(model.identities.filter { $0.backedUp }.count) backed up",
                            onTap: {}
                        ) {
                            OnymSymbolTile(symbol: "person.fill", bg: OnymTokens.Tile.purple)
                        }
                    }
                    .buttonStyle(.plain)

                    NavigationLink { OnymPrivacyEncryptionView(model: model) } label: {
                        OnymRow(
                            title: "Privacy & Encryption",
                            subtitle: "End-to-end · BIP-39",
                            last: true,
                            onTap: {}
                        ) {
                            OnymSymbolTile(symbol: "lock.shield.fill", bg: OnymTokens.Tile.blue)
                        }
                    }
                    .buttonStyle(.plain)
                }

                OnymSectionLabel(text: "NETWORK")
                OnymCard {
                    NavigationLink { OnymRelaysView(model: model) } label: {
                        OnymRow(
                            title: "Relays",
                            subtitle: "\(model.relays.count) configured · Onym Official",
                            onTap: {}
                        ) {
                            OnymSymbolTile(symbol: "antenna.radiowaves.left.and.right", bg: OnymTokens.Tile.indigo)
                        }
                    }.buttonStyle(.plain)

                    NavigationLink { OnymAnchorsView(model: model) } label: {
                        OnymRow(
                            title: "Anchors",
                            subtitle: "Stellar · Testnet",
                            onTap: {}
                        ) {
                            OnymSymbolTile(symbol: "link", bg: OnymTokens.Tile.orange)
                        }
                    }.buttonStyle(.plain)

                    OnymRow(
                        title: "Use Mainnet",
                        subtitle: "Testnet by default while contracts are staged",
                        hasChevron: false,
                        last: true
                    ) {
                        OnymSymbolTile(symbol: "hammer.fill", bg: OnymTokens.Tile.gray)
                    } accessory: {
                        OnymSwitch(on: $bm.useMainnet)
                    }
                }

                OnymSectionLabel(text: "APP")
                OnymCard {
                    NavigationLink { OnymAppearanceView() } label: {
                        OnymRow(
                            title: "Appearance",
                            subtitle: "Light · Blue accent",
                            onTap: {}
                        ) {
                            OnymSymbolTile(symbol: "circle.lefthalf.filled", bg: OnymTokens.Tile.gray)
                        }
                    }.buttonStyle(.plain)

                    NavigationLink { OnymAboutView() } label: {
                        OnymRow(
                            title: "About Onym",
                            subtitle: "Version 1.4.2 (build 220)",
                            last: true,
                            onTap: {}
                        ) {
                            OnymSymbolTile(symbol: "info.circle.fill", bg: OnymTokens.Tile.teal)
                        }
                    }.buttonStyle(.plain)
                }

                VStack(spacing: 8) {
                    OnymMark(size: 26, color: OnymTokens.text3)
                        .padding(.top, 28)
                    Text("onym · open · anonymous · onchain")
                        .font(.system(size: 11))
                        .tracking(0.22)
                        .foregroundStyle(OnymTokens.text3)
                }
                .frame(maxWidth: .infinity)
                .padding(.bottom, 32)
            }
        }
    }

    private func identityHero(_ active: OnymIdentity) -> some View {
        NavigationLink {
            OnymIdentitiesView(model: model)
        } label: {
            HStack(spacing: 14) {
                ZStack(alignment: .bottomTrailing) {
                    Circle()
                        .fill(LinearGradient(colors: [Color(red: 0.933, green: 0.961, blue: 1.0),
                                                       Color(red: 0.878, green: 0.933, blue: 0.996)],
                                              startPoint: .topLeading, endPoint: .bottomTrailing))
                        .frame(width: 56, height: 56)
                        .overlay(Circle().stroke(OnymTokens.blue, lineWidth: 1.5))
                        .overlay(OnymMark(size: 36, color: OnymTokens.blue))
                    Circle()
                        .fill(OnymTokens.green)
                        .frame(width: 16, height: 16)
                        .overlay(Circle().stroke(.white, lineWidth: 2))
                        .offset(x: 2, y: 2)
                }

                VStack(alignment: .leading, spacing: 2) {
                    Text("ACTIVE IDENTITY")
                        .font(.system(size: 11.5, weight: .medium))
                        .foregroundStyle(OnymTokens.text2)
                        .tracking(0.22)
                    Text(active.name)
                        .font(.system(size: 19, weight: .semibold))
                        .foregroundStyle(OnymTokens.text)
                        .tracking(-0.19)
                    Text(active.npub.prefix(18) + "…")
                        .font(.system(size: 11.5, design: .monospaced))
                        .foregroundStyle(OnymTokens.text2)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                Spacer(minLength: 8)
                Image(systemName: "chevron.right")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(OnymTokens.text3)
            }
            .padding(16)
            .background(OnymTokens.card, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            .padding(.horizontal, 16)
            .padding(.bottom, 4)
        }
        .buttonStyle(.plain)
    }

    private func qrHero(_ active: OnymIdentity) -> some View {
        NavigationLink {
            OnymShareKeyView(identity: active)
        } label: {
            HStack(spacing: 16) {
                OnymQRCode(value: onymInviteURL(for: active), size: 92)
                    .padding(8)
                    .background(.white, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous)
                        .stroke(.black.opacity(0.04), lineWidth: 1))

                VStack(alignment: .leading, spacing: 4) {
                    Text("INVITE KEY")
                        .font(.system(size: 11.5, weight: .medium))
                        .foregroundStyle(OnymTokens.text2)
                        .tracking(0.46)
                    Text("Start a chat by scanning")
                        .font(.system(size: 17, weight: .semibold))
                        .foregroundStyle(OnymTokens.text)
                        .tracking(-0.16)
                    Text("Have someone scan this code with Onym to open a private chat with \(active.name).")
                        .font(.system(size: 12.5))
                        .foregroundStyle(OnymTokens.text2)
                        .lineSpacing(2)
                        .lineLimit(3)
                }
                Spacer(minLength: 4)
                Image(systemName: "chevron.right")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(OnymTokens.text3)
            }
            .padding(18)
            .background(OnymTokens.card, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            .padding(.horizontal, 16)
            .padding(.top, 12)
        }
        .buttonStyle(.plain)
    }

    private func notBackedUpBanner(count: Int) -> some View {
        NavigationLink {
            OnymIdentitiesView(model: model)
        } label: {
            HStack(spacing: 10) {
                Circle()
                    .fill(OnymTokens.amber)
                    .frame(width: 22, height: 22)
                    .overlay(Image(systemName: "exclamationmark")
                        .font(.system(size: 12, weight: .bold))
                        .foregroundStyle(.white))
                Text("\(count) identity hasn't been backed up yet.")
                    .font(.system(size: 13))
                    .foregroundStyle(Color(red: 0.36, green: 0.227, blue: 0))
                Spacer(minLength: 4)
                Image(systemName: "chevron.right")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(Color(red: 0.36, green: 0.227, blue: 0))
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(Color(red: 1, green: 0.965, blue: 0.898),
                        in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(Color(red: 1, green: 0.847, blue: 0.627), lineWidth: 0.5))
            .padding(.horizontal, 16)
            .padding(.top, 12)
        }
        .buttonStyle(.plain)
    }
}
