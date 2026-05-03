import SwiftUI

// MARK: - Privacy & Encryption

struct OnymPrivacyEncryptionView: View {
    @Bindable var model: OnymSettingsModel
    @Environment(\.dismiss) private var dismiss

    @State private var readReceipts = false
    @State private var screenLock = true
    @State private var autoLock: AutoLock = .oneMin

    enum AutoLock: CaseIterable { case immediate, oneMin, fiveMin, fifteenMin, never
        var label: String {
            switch self {
            case .immediate: return "Immediately"
            case .oneMin:    return "After 1 min"
            case .fiveMin:   return "After 5 min"
            case .fifteenMin:return "After 15 min"
            case .never:     return "Never"
            }
        }
        var next: AutoLock {
            switch self {
            case .immediate: return .oneMin
            case .oneMin:    return .fiveMin
            case .fiveMin:   return .fifteenMin
            case .fifteenMin:return .never
            case .never:     return .immediate
            }
        }
    }

    var body: some View {
        let active = model.activeIdentity ?? model.identities[0]
        return OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: "Privacy & Encryption", onBack: { dismiss() })

                heroCard.padding(.top, 8)

                OnymSectionLabel(text: "HOW IT WORKS")
                OnymCard {
                    OnymRow(title: "End-to-end encryption",
                            subtitle: "MLS · forward secrecy",
                            hasChevron: false,
                            onTap: {}) {
                        OnymSymbolTile(symbol: "key.fill", bg: OnymTokens.Tile.purple)
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }
                    OnymRow(title: "Anonymous on-chain",
                            subtitle: "No phone, no email, no IP",
                            hasChevron: false,
                            onTap: {}) {
                        OnymSymbolTile(symbol: "sparkles", bg: OnymTokens.Tile.indigo)
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }
                    OnymRow(title: "Verifiable by anyone",
                            subtitle: "Group state anchored on Stellar",
                            hasChevron: false,
                            last: true,
                            onTap: {}) {
                        OnymSymbolTile(symbol: "shield.fill", bg: OnymTokens.Tile.green)
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }
                }

                OnymSectionLabel(text: "YOUR KEYS")
                OnymCard {
                    OnymRow(title: "Active identity",
                            subtitle: active.name,
                            onTap: {}) {
                        OnymIdentityTile(active: true, size: 30)
                    } right: {
                        Text("Backed up").foregroundStyle(OnymTokens.green).font(.system(size: 13.5))
                    }
                    OnymRow(title: "BIP-39 wordlist", hasChevron: false) {
                        OnymSymbolTile(symbol: "checkmark", bg: OnymTokens.Tile.gray)
                    } right: {
                        Text("English").foregroundStyle(OnymTokens.text2).font(.system(size: 14))
                    }
                    OnymRow(title: "Identity key", hasChevron: false) {
                        OnymTile(bg: OnymTokens.Tile.indigo) {
                            Text("npub").font(.system(size: 9, weight: .bold)).foregroundStyle(.white)
                        }
                    } right: {
                        Text("Nostr (npub)").foregroundStyle(OnymTokens.text2).font(.system(size: 14))
                    }
                    OnymRow(title: "Signature scheme", hasChevron: false, last: true) {
                        OnymTile(bg: OnymTokens.Tile.gray) {
                            Text("BLS").font(.system(size: 9.5, weight: .bold)).foregroundStyle(.white)
                        }
                    } right: {
                        Text("BLS12-381").foregroundStyle(OnymTokens.text2).font(.system(size: 14))
                    }
                }
                OnymFootnote(text: "Your recovery phrase generates a master seed. Onym derives a Nostr keypair (your public identity, shown as npub1…), a Stellar keypair (for anchoring), and a BLS key (for group signatures).")

                OnymSectionLabel(text: "APP LOCK")
                OnymCard {
                    OnymRow(title: "Require Face ID",
                            subtitle: "Unlock Onym with biometrics",
                            hasChevron: false) {
                        OnymSymbolTile(symbol: "faceid", bg: OnymTokens.Tile.gray)
                    } accessory: {
                        OnymSwitch(on: $screenLock)
                    }
                    OnymRow(title: "Auto-lock",
                            inset: 16,
                            last: true,
                            onTap: { autoLock = autoLock.next }) {
                        EmptyView()
                    } right: {
                        Text(autoLock.label).foregroundStyle(OnymTokens.text2).font(.system(size: 14))
                    }
                }

                OnymSectionLabel(text: "METADATA")
                OnymCard {
                    OnymRow(title: "Send read receipts",
                            subtitle: "Show others when you've read their messages",
                            hasChevron: false,
                            last: true) {
                        OnymSymbolTile(symbol: "checkmark.circle.fill", bg: OnymTokens.Tile.blue)
                    } accessory: {
                        OnymSwitch(on: $readReceipts)
                    }
                }
                OnymFootnote(text: "Read receipts are end-to-end encrypted, but they reveal you're online. Turn off for stricter privacy.")

                OnymSectionLabel(text: "DATA")
                OnymCard {
                    OnymRow(title: "Clear local message cache",
                            subtitle: "Re-download from your relay on next open",
                            hasChevron: false,
                            last: true,
                            onTap: {}) {
                        OnymSymbolTile(symbol: "trash.fill", bg: OnymTokens.Tile.red)
                    }
                }

                OnymFootnote(text: "Onym never stores your messages on our servers. Cached messages live only on this device, encrypted by your identity keys.")
            }
        }
    }

    private var heroCard: some View {
        HStack(spacing: 14) {
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(LinearGradient(colors: [Color(red: 0.875, green: 0.98, blue: 0.918),
                                                Color(red: 0.71, green: 0.94, blue: 0.804)],
                                      startPoint: .topLeading, endPoint: .bottomTrailing))
                .frame(width: 56, height: 56)
                .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .stroke(OnymTokens.green.opacity(0.35), lineWidth: 1.5))
                .overlay(Image(systemName: "lock.shield.fill")
                    .font(.system(size: 28))
                    .foregroundStyle(OnymTokens.green))
            VStack(alignment: .leading, spacing: 3) {
                Text("Everything is encrypted")
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(OnymTokens.text)
                Text("Messages, group state, and keys are encrypted on this device. No one — not even Onym — can read your chats.")
                    .font(.system(size: 13))
                    .foregroundStyle(OnymTokens.text2)
                    .lineSpacing(2)
            }
        }
        .padding(18)
        .background(OnymTokens.card, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .padding(.horizontal, 16)
    }
}

// MARK: - Appearance

struct OnymAppearanceView: View {
    @Environment(\.dismiss) private var dismiss

    @State private var theme: AppearanceTheme = .light
    @State private var accent: AppearanceAccent = .blue
    @State private var font: AppearanceFont = .system
    @State private var textSize = 2
    @State private var bubble: BubbleStyle = .rounded
    @State private var reduceMotion = false

    enum AppearanceTheme: CaseIterable { case light, dark, system
        var label: String { self == .light ? "Light" : self == .dark ? "Dark" : "System" }
    }
    enum AppearanceAccent: CaseIterable, Identifiable {
        case blue, purple, pink, orange, green, teal
        var id: String { String(describing: self) }
        var color: Color {
            switch self {
            case .blue:   return Color(red: 10/255,  green: 132/255, blue: 255/255)
            case .purple: return Color(red: 160/255, green: 76/255,  blue: 224/255)
            case .pink:   return Color(red: 224/255, green: 50/255,  blue: 83/255)
            case .orange: return Color(red: 255/255, green: 122/255, blue: 45/255)
            case .green:  return Color(red: 48/255,  green: 180/255, blue: 90/255)
            case .teal:   return Color(red: 43/255,  green: 179/255, blue: 207/255)
            }
        }
    }
    enum AppearanceFont: CaseIterable { case system, mono, serif
        var label: String {
            switch self {
            case .system: return "San Francisco"
            case .mono:   return "Mono everywhere"
            case .serif:  return "New York"
            }
        }
        var next: AppearanceFont {
            switch self { case .system: return .mono; case .mono: return .serif; case .serif: return .system }
        }
    }
    enum BubbleStyle: String, CaseIterable, Identifiable {
        case rounded, square
        var id: String { rawValue }
        var label: String { rawValue.capitalized }
        var radius: CGFloat { self == .rounded ? 18 : 6 }
    }

    var body: some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: "Appearance", onBack: { dismiss() })
                OnymLargeTitle(text: "Appearance")

                OnymSectionLabel(text: "THEME")
                themeRow.padding(.horizontal, 16)

                OnymSectionLabel(text: "ACCENT COLOR")
                OnymCard {
                    HStack(spacing: 12) {
                        ForEach(AppearanceAccent.allCases) { a in
                            Button { accent = a } label: {
                                ZStack {
                                    Circle().fill(a.color).frame(width: 36, height: 36)
                                    if accent == a {
                                        Circle().stroke(.white, lineWidth: 2).frame(width: 36, height: 36)
                                        Circle().stroke(a.color, lineWidth: 2).frame(width: 42, height: 42)
                                        Image(systemName: "checkmark")
                                            .font(.system(size: 13, weight: .bold))
                                            .foregroundStyle(.white)
                                    }
                                }
                                .frame(maxWidth: .infinity)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, 16).padding(.vertical, 14)
                }
                OnymFootnote(text: "Used for buttons, links, and active states throughout the app.")

                OnymSectionLabel(text: "TEXT")
                OnymCard {
                    OnymRow(title: "Font",
                            inset: 16,
                            onTap: { font = font.next }) {
                        EmptyView()
                    } right: {
                        Text(font.label).foregroundStyle(OnymTokens.text2).font(.system(size: 14))
                    }
                    VStack(alignment: .leading, spacing: 8) {
                        HStack {
                            Text("Text size").font(.system(size: 16.5)).foregroundStyle(OnymTokens.text)
                            Spacer()
                            Text(["Smallest","Small","Default","Large","Largest"][textSize])
                                .font(.system(size: 13)).foregroundStyle(OnymTokens.text2)
                        }
                        textSizeSlider
                    }
                    .padding(.horizontal, 16).padding(.vertical, 12)
                    Rectangle().fill(OnymTokens.hairline).frame(height: 0.5)
                    VStack(alignment: .leading, spacing: 0) {
                        Text("The quick brown fox jumps over the lazy dog.")
                            .font(fontPreview)
                            .foregroundStyle(OnymTokens.text2)
                            .padding(.horizontal, 16).padding(.vertical, 14)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(OnymTokens.card2)
                    }
                }

                OnymSectionLabel(text: "CHATS")
                OnymCard {
                    VStack(alignment: .leading, spacing: 10) {
                        Text("Bubble style").font(.system(size: 16.5)).foregroundStyle(OnymTokens.text)
                        HStack(spacing: 10) {
                            ForEach(BubbleStyle.allCases) { b in
                                Button { bubble = b } label: {
                                    bubblePreview(b)
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                    .padding(.horizontal, 16).padding(.vertical, 14)
                }

                OnymSectionLabel(text: "ACCESSIBILITY")
                OnymCard {
                    OnymRow(title: "Reduce motion",
                            subtitle: "Disable transitions and animated avatars",
                            hasChevron: false,
                            last: true) {
                        OnymSymbolTile(symbol: "tortoise.fill", bg: OnymTokens.Tile.gray)
                    } accessory: {
                        OnymSwitch(on: $reduceMotion)
                    }
                }
            }
        }
    }

    private var themeRow: some View {
        HStack(spacing: 10) {
            ForEach(AppearanceTheme.allCases, id: \.self) { t in
                Button { theme = t } label: { themeCard(t) }
                .buttonStyle(.plain)
            }
        }
    }

    private func themeCard(_ t: AppearanceTheme) -> some View {
        let sel = theme == t
        let bg: AnyShapeStyle = {
            switch t {
            case .light:  return AnyShapeStyle(Color.white)
            case .dark:   return AnyShapeStyle(Color(red: 0.110, green: 0.110, blue: 0.118))
            case .system: return AnyShapeStyle(LinearGradient(
                colors: [.white, Color(red: 0.110, green: 0.110, blue: 0.118)],
                startPoint: .topLeading, endPoint: .bottomTrailing))
            }
        }()
        let fg: Color = t == .dark ? .white : OnymTokens.text
        return VStack(spacing: 8) {
            ZStack(alignment: .topLeading) {
                RoundedRectangle(cornerRadius: 14, style: .continuous).fill(bg)
                    .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous)
                        .stroke(sel ? OnymTokens.blue : OnymTokens.hairline, lineWidth: sel ? 2.5 : 1))
                    .frame(height: 110)
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 4) {
                        OnymMark(size: 14, color: fg)
                        Capsule().fill(fg.opacity(0.18)).frame(height: 4)
                    }
                    VStack(spacing: 4) {
                        Capsule().fill(fg.opacity(0.5)).frame(width: 60, height: 3)
                        Capsule().fill(fg.opacity(0.25)).frame(maxWidth: .infinity, alignment: .leading).frame(height: 3)
                        Capsule().fill(fg.opacity(0.25)).frame(width: 40, height: 3)
                    }
                    .padding(6)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(t == .dark
                                ? Color(red: 0.173, green: 0.173, blue: 0.180)
                                : (t == .system ? Color.gray.opacity(0.18) : Color(red: 0.949, green: 0.949, blue: 0.957)),
                                in: RoundedRectangle(cornerRadius: 6))
                }
                .padding(10)
            }
            Text(t.label)
                .font(.system(size: 13, weight: sel ? .semibold : .medium))
                .foregroundStyle(sel ? OnymTokens.blue : OnymTokens.text)
        }
        .frame(maxWidth: .infinity)
    }

    private var textSizeSlider: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                Capsule().fill(Color(red: 0.898, green: 0.898, blue: 0.918)).frame(height: 2)
                Capsule().fill(OnymTokens.blue).frame(width: max(0, CGFloat(textSize) * geo.size.width / 4), height: 2)
                HStack(spacing: 0) {
                    ForEach(0..<5, id: \.self) { i in
                        Button { textSize = i } label: {
                            Circle()
                                .fill(i == textSize ? Color.white : Color.clear)
                                .frame(width: 28, height: 28)
                                .overlay(Circle().fill(i <= textSize ? OnymTokens.blue : Color(red: 0.78, green: 0.78, blue: 0.80))
                                    .frame(width: 8, height: 8))
                                .shadow(color: i == textSize ? .black.opacity(0.18) : .clear, radius: 4, y: 2)
                        }
                        .buttonStyle(.plain)
                        if i < 4 { Spacer() }
                    }
                }
            }
        }
        .frame(height: 28)
    }

    private var fontPreview: Font {
        let pt: CGFloat = 12 + CGFloat(textSize) * 1.5
        switch font {
        case .system: return .system(size: pt)
        case .mono:   return .system(size: pt, design: .monospaced)
        case .serif:  return .system(size: pt, design: .serif)
        }
    }

    private func bubblePreview(_ b: BubbleStyle) -> some View {
        let sel = bubble == b
        return VStack(alignment: .leading, spacing: 6) {
            Text("Hi there")
                .font(.system(size: 11))
                .foregroundStyle(OnymTokens.text)
                .padding(.horizontal, 10).padding(.vertical, 6)
                .background(Color(red: 0.898, green: 0.898, blue: 0.918), in: RoundedRectangle(cornerRadius: b.radius))
            Text("Hey!")
                .font(.system(size: 11))
                .foregroundStyle(.white)
                .padding(.horizontal, 10).padding(.vertical, 6)
                .background(OnymTokens.blue, in: RoundedRectangle(cornerRadius: b.radius))
                .frame(maxWidth: .infinity, alignment: .trailing)
            Text(b.label)
                .font(.system(size: 12.5, weight: sel ? .semibold : .medium))
                .foregroundStyle(sel ? OnymTokens.blue : OnymTokens.text2)
        }
        .padding(12)
        .background(OnymTokens.card2, in: RoundedRectangle(cornerRadius: 14))
        .overlay(RoundedRectangle(cornerRadius: 14)
            .stroke(sel ? OnymTokens.blue : OnymTokens.hairline, lineWidth: sel ? 1.5 : 1))
        .frame(maxWidth: .infinity)
    }
}

// MARK: - About

struct OnymAboutView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var taps = 0

    var body: some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: "About", onBack: { dismiss() })

                hero

                OnymSectionLabel(text: "VERSION")
                OnymCard {
                    OnymRow(title: "Version", hasChevron: false, inset: 16) {
                        EmptyView()
                    } right: {
                        Text("1.4.2").foregroundStyle(OnymTokens.text2)
                    }
                    OnymRow(title: "Build", hasChevron: false, inset: 16) {
                        EmptyView()
                    } right: {
                        Text("220 · a4f9b2e")
                            .font(.system(size: 13.5, design: .monospaced))
                            .foregroundStyle(OnymTokens.text2)
                    }
                    OnymRow(title: "Released", hasChevron: false, inset: 16) {
                        EmptyView()
                    } right: {
                        Text("May 2, 2026").foregroundStyle(OnymTokens.text2)
                    }
                    OnymRow(title: "Check for updates", last: true, onTap: {}) {
                        OnymSymbolTile(symbol: "arrow.up.circle.fill", bg: OnymTokens.Tile.blue)
                    }
                }

                OnymSectionLabel(text: "RESOURCES")
                OnymCard {
                    OnymRow(title: "Source code",
                            subtitle: "github.com/onymchat/onym-ios",
                            subtitleMono: true,
                            hasChevron: false,
                            onTap: { open("https://github.com/onymchat/onym-ios") }) {
                        OnymSymbolTile(symbol: "chevron.left.forwardslash.chevron.right",
                                       bg: OnymTokens.text)
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }
                    OnymRow(title: "Documentation",
                            subtitle: "docs.onym.chat",
                            hasChevron: false,
                            onTap: { open("https://docs.onym.chat") }) {
                        OnymSymbolTile(symbol: "doc.text.fill", bg: OnymTokens.Tile.indigo)
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }
                    OnymRow(title: "Whitepaper",
                            subtitle: "The Onym protocol · v1.0",
                            hasChevron: false,
                            onTap: {}) {
                        OnymSymbolTile(symbol: "sparkles", bg: OnymTokens.Tile.green)
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }
                    OnymRow(title: "Changelog",
                            subtitle: "What's new",
                            last: true,
                            onTap: {}) {
                        OnymSymbolTile(symbol: "list.star", bg: OnymTokens.Tile.purple)
                    }
                }

                OnymSectionLabel(text: "HELP")
                OnymCard {
                    OnymRow(title: "FAQ", hasChevron: false, onTap: {}) {
                        OnymSymbolTile(symbol: "questionmark.circle.fill", bg: OnymTokens.Tile.blue)
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }
                    OnymRow(title: "Community chat",
                            subtitle: "Join the dev group on Onym",
                            hasChevron: false, onTap: {}) {
                        OnymSymbolTile(symbol: "bubble.left.fill", bg: OnymTokens.Tile.green)
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }
                    OnymRow(title: "Contact support",
                            subtitle: "hello@onym.chat",
                            hasChevron: false, last: true,
                            onTap: { open("mailto:hello@onym.chat") }) {
                        OnymSymbolTile(symbol: "envelope.fill", bg: OnymTokens.Tile.orange)
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }
                }

                OnymSectionLabel(text: "LEGAL")
                OnymCard {
                    OnymRow(title: "Privacy policy", hasChevron: false, inset: 16, onTap: {}) {
                        EmptyView()
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }
                    OnymRow(title: "Terms of service", hasChevron: false, inset: 16, onTap: {}) {
                        EmptyView()
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }
                    OnymRow(title: "Open source licenses", inset: 16, last: true, onTap: {}) {
                        EmptyView()
                    }
                }

                VStack(spacing: 12) {
                    OnymMark(size: 22, color: OnymTokens.text3)
                    Text("Built by people who think privacy is a right.\nReleased under the MIT license.")
                        .font(.system(size: 11.5))
                        .foregroundStyle(OnymTokens.text3)
                        .multilineTextAlignment(.center)
                        .lineSpacing(4)
                    Text("© 2026 · Onym Foundation")
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundStyle(OnymTokens.text4)
                    if taps >= 5 {
                        Text("🎉 Hello, builder. Want to contribute?")
                            .font(.system(size: 12, weight: .semibold))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 12).padding(.vertical, 8)
                            .background(LinearGradient(colors: [OnymTokens.purple, OnymTokens.blue],
                                                        startPoint: .leading, endPoint: .trailing),
                                         in: RoundedRectangle(cornerRadius: 10))
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.top, 36)
                .padding(.bottom, 16)
            }
        }
    }

    private var hero: some View {
        VStack(spacing: 4) {
            Button { taps += 1 } label: {
                RoundedRectangle(cornerRadius: 26, style: .continuous)
                    .fill(LinearGradient(colors: [Color(red: 0.106, green: 0.122, blue: 0.141),
                                                    Color(red: 0.051, green: 0.067, blue: 0.090)],
                                          startPoint: .topLeading, endPoint: .bottomTrailing))
                    .frame(width: 104, height: 104)
                    .overlay(OnymMark(size: 64, color: .white, spin: taps >= 5))
                    .shadow(color: .black.opacity(0.18), radius: 12, y: 6)
            }
            .buttonStyle(.plain)
            .padding(.top, 12)

            Text("Onym")
                .font(.system(size: 30, weight: .bold))
                .tracking(-0.6)
                .foregroundStyle(OnymTokens.text)
                .padding(.top, 18)
            Text("open · anonymous · onchain")
                .font(.system(size: 13))
                .foregroundStyle(OnymTokens.text2)
                .tracking(0.26)
                .padding(.top, 4)
            HStack(spacing: 6) {
                Text("Up to date")
                    .font(.system(size: 11.5, weight: .semibold))
                    .padding(.horizontal, 8).padding(.vertical, 3)
                    .background(OnymTokens.green.opacity(0.14),
                                in: RoundedRectangle(cornerRadius: 999))
                    .foregroundStyle(OnymTokens.green)
                Text("·").font(.system(size: 12)).foregroundStyle(OnymTokens.text3)
                Text("1.4.2 (220)")
                    .font(.system(size: 12, design: .monospaced))
                    .foregroundStyle(OnymTokens.text2)
            }
            .padding(.top, 12)
        }
        .frame(maxWidth: .infinity)
        .padding(.bottom, 28)
    }

    private func open(_ s: String) {
        if let u = URL(string: s) { UIApplication.shared.open(u) }
    }
}
