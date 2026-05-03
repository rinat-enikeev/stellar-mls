import SwiftUI

struct OnymRelaysView: View {
    @Bindable var model: OnymSettingsModel
    @Environment(\.dismiss) private var dismiss
    @State private var mode: RelayMode = .random
    @State private var customURL = ""

    enum RelayMode { case random, primary }

    var body: some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: "Relays", onBack: { dismiss() })
                OnymLargeTitle(text: "Relays")

                strategyToggle

                OnymSectionLabel(text: "CONFIGURED · \(model.relays.count)")
                OnymCard {
                    ForEach(Array(model.relays.enumerated()), id: \.element.id) { index, r in
                        OnymRow(
                            title: r.name,
                            subtitle: r.url,
                            subtitleMono: true,
                            inset: 56,
                            last: index == model.relays.count - 1,
                            onTap: {}
                        ) {
                            Button { model.toggleStarred(r.id) } label: {
                                Image(systemName: r.starred ? "star.fill" : "star")
                                    .font(.system(size: 17))
                                    .foregroundStyle(r.starred ? OnymTokens.amber : OnymTokens.text3)
                                    .frame(width: 30, height: 30)
                            }
                            .buttonStyle(.plain)
                        } right: {
                            HStack(spacing: 4) {
                                OnymChip(text: r.network,
                                         fg: Color(red: 0.10, green: 0.51, blue: 0.28),
                                         bg: OnymTokens.green.opacity(0.14))
                                OnymChip(text: r.visibility,
                                         fg: r.visibility == "PRIVATE"
                                             ? Color(red: 0.494, green: 0.114, blue: 0.2)
                                             : Color(red: 0.572, green: 0.114, blue: 0.18),
                                         bg: r.visibility == "PRIVATE"
                                             ? OnymTokens.purple.opacity(0.12)
                                             : OnymTokens.red.opacity(0.12))
                            }
                        }
                    }
                }

                OnymSectionLabel(text: "ADD FROM PUBLISHED LIST")
                OnymCard {
                    Text("All published relays added.")
                        .font(.system(size: 14))
                        .foregroundStyle(OnymTokens.text3)
                        .frame(maxWidth: .infinity)
                        .padding(.horizontal, 16).padding(.vertical, 14)
                }
                OnymFootnote(text: "Published by the onym-relay project. Tap to add.")

                OnymSectionLabel(text: "ADD CUSTOM URL")
                OnymCard {
                    TextField("https://relay.example.com", text: $customURL)
                        .font(.system(size: 16, design: .monospaced))
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .padding(.horizontal, 16).padding(.vertical, 12)
                    Rectangle().fill(OnymTokens.hairline).frame(height: 0.5).padding(.leading, 16)
                    OnymRow(
                        title: "Add Custom URL",
                        hasChevron: false,
                        inset: 0,
                        last: true,
                        onTap: {
                            // Wire up: model.addCustomRelay(customURL); customURL = ""
                        }
                    ) {
                        Circle().fill(OnymTokens.blue).frame(width: 22, height: 22)
                            .overlay(Image(systemName: "plus")
                                .font(.system(size: 13, weight: .bold))
                                .foregroundStyle(.white))
                    }
                }
                OnymFootnote(text: "Use a private deployment, localhost, or any relay not in the published list.")

                NavigationLink { OnymRunRelayView() } label: {
                    HStack(spacing: 14) {
                        ZStack {
                            RoundedRectangle(cornerRadius: 10, style: .continuous)
                                .fill(.white.opacity(0.08))
                                .frame(width: 44, height: 44)
                            Image(systemName: "chevron.left.forwardslash.chevron.right")
                                .font(.system(size: 18))
                                .foregroundStyle(.white)
                        }
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Run your own relay")
                                .font(.system(size: 15.5, weight: .semibold))
                                .foregroundStyle(.white)
                            Text("Deploy onym-relay from GitHub in 5 minutes")
                                .font(.system(size: 12.5))
                                .foregroundStyle(.white.opacity(0.65))
                        }
                        Spacer()
                        Image(systemName: "chevron.right")
                            .font(.system(size: 13, weight: .semibold))
                            .foregroundStyle(.white.opacity(0.5))
                    }
                    .padding(18)
                    .background(LinearGradient(colors: [Color(red: 0.106, green: 0.122, blue: 0.141),
                                                        Color(red: 0.051, green: 0.067, blue: 0.090)],
                                                startPoint: .topLeading, endPoint: .bottomTrailing),
                                in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                    .padding(.horizontal, 16)
                    .padding(.top, 24)
                }
                .buttonStyle(.plain)
            }
        }
    }

    private var strategyToggle: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 0) {
                ForEach([RelayMode.random, .primary], id: \.self) { m in
                    let label = m == .random ? "Random" : "Primary"
                    Button { mode = m } label: {
                        Text(label)
                            .font(.system(size: 13.5, weight: mode == m ? .semibold : .medium))
                            .foregroundStyle(OnymTokens.text)
                            .frame(maxWidth: .infinity, minHeight: 32)
                            .background(mode == m
                                        ? AnyShapeStyle(Color.white)
                                        : AnyShapeStyle(Color.clear),
                                        in: RoundedRectangle(cornerRadius: 7, style: .continuous))
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(2)
            .background(Color(red: 0.898, green: 0.898, blue: 0.918),
                        in: RoundedRectangle(cornerRadius: 9, style: .continuous))
            .padding(.horizontal, 16)

            Text(mode == .random
                 ? "Pick a random relay for each request. Spreads load across redundant deployments and is the default."
                 : "Use your starred relay first. Fall back to others if it's down.")
                .font(.system(size: 12.5))
                .foregroundStyle(OnymTokens.text2)
                .lineSpacing(2)
                .padding(.horizontal, 20).padding(.top, 10)
        }
    }
}

// MARK: - Run your own relay (4-step explainer linking to github.com/onymchat)

struct OnymRunRelayView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var copied: String?

    private struct Step: Identifiable {
        let id = UUID()
        let n: Int
        let title: String
        let body: String
        let cmd: String?
    }

    private let steps: [Step] = [
        .init(n: 1, title: "Clone the repo",
              body: "onym-relay is open source. Grab it from GitHub.",
              cmd: "git clone github.com/onymchat/onym-relay"),
        .init(n: 2, title: "Configure your domain",
              body: "Set RELAY_URL in .env. This is what users will paste.",
              cmd: "cp .env.example .env\necho \"RELAY_URL=https://your-domain\" >> .env"),
        .init(n: 3, title: "Deploy",
              body: "Pick a host. Fly.io and Railway have one-click deploys.",
              cmd: "fly launch --copy-config\nfly deploy"),
        .init(n: 4, title: "Add it to Onym",
              body: "Back on Relays, paste your URL into \"Add Custom URL\".",
              cmd: nil),
    ]

    var body: some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: "Run your own relay", onBack: { dismiss() })

                heroCard

                ForEach(Array(steps.enumerated()), id: \.element.id) { idx, s in
                    stepRow(s)
                    if idx < steps.count - 1 {
                        Rectangle()
                            .fill(LinearGradient(colors: [OnymTokens.blue.opacity(0.4), OnymTokens.blue.opacity(0.1)],
                                                  startPoint: .top, endPoint: .bottom))
                            .frame(width: 2, height: 24)
                            .padding(.leading, 30)
                    }
                }

                OnymSectionLabel(text: "ONE-CLICK DEPLOY")
                OnymCard {
                    OnymRow(title: "Deploy to Fly.io",
                            subtitle: "Free tier · global edge",
                            hasChevron: false,
                            onTap: { open("https://fly.io/launch") }) {
                        OnymTile(bg: Color(red: 0.482, green: 0.247, blue: 0.894)) {
                            Text("✈").font(.system(size: 14)).foregroundStyle(.white)
                        }
                    } accessory: {
                        Image(systemName: "arrow.up.right.square")
                            .foregroundStyle(OnymTokens.text3)
                    }
                    OnymRow(title: "Deploy to Railway",
                            subtitle: "$5/mo · simple setup",
                            hasChevron: false,
                            onTap: { open("https://railway.app") }) {
                        OnymTile(bg: Color(red: 0.122, green: 0.122, blue: 0.122)) {
                            Text("▲").font(.system(size: 14)).foregroundStyle(.white)
                        }
                    } accessory: {
                        Image(systemName: "arrow.up.right.square")
                            .foregroundStyle(OnymTokens.text3)
                    }
                    OnymRow(title: "Run with Docker",
                            subtitle: "Self-host anywhere",
                            hasChevron: false,
                            last: true,
                            onTap: {}) {
                        OnymTile(bg: Color(red: 0, green: 0.502, blue: 1.0)) {
                            Text("🐳").font(.system(size: 14))
                        }
                    } accessory: {
                        Image(systemName: "arrow.up.right.square")
                            .foregroundStyle(OnymTokens.text3)
                    }
                }

                OnymFootnote(text: "Need help? Open an issue on GitHub or join the dev chat.")
            }
        }
    }

    private var heroCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                Image(systemName: "chevron.left.forwardslash.chevron.right")
                    .font(.system(size: 14))
                    .foregroundStyle(.white)
                Text("onymchat/onym-relay")
                    .font(.system(size: 13, design: .monospaced))
                    .foregroundStyle(.white.opacity(0.65))
            }
            Text("Your relay, your rules")
                .font(.system(size: 22, weight: .bold))
                .tracking(-0.26)
                .foregroundStyle(.white)
            Text("Run a relay for yourself, your team, or your org. End-to-end encryption stays intact — Onym never sees your messages.")
                .font(.system(size: 13.5))
                .foregroundStyle(.white.opacity(0.7))
                .lineSpacing(3)

            HStack(spacing: 8) {
                Button { open(OnymCatalog.relayRepoURL.absoluteString) } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "chevron.left.forwardslash.chevron.right")
                        Text("View on GitHub")
                            .font(.system(size: 13.5, weight: .semibold))
                        Image(systemName: "arrow.up.right.square")
                            .font(.system(size: 11))
                    }
                    .foregroundStyle(OnymTokens.text)
                    .padding(.horizontal, 12).padding(.vertical, 10)
                    .frame(maxWidth: .infinity)
                    .background(.white, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                }
                .buttonStyle(.plain)
                Button { open("https://docs.onym.chat") } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "book")
                        Text("Read the docs")
                            .font(.system(size: 13.5, weight: .semibold))
                        Image(systemName: "arrow.up.right.square")
                            .font(.system(size: 11))
                    }
                    .foregroundStyle(.white)
                    .padding(.horizontal, 12).padding(.vertical, 10)
                    .frame(maxWidth: .infinity)
                    .background(Color.white.opacity(0.12), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                }
                .buttonStyle(.plain)
            }
        }
        .padding(20)
        .background(LinearGradient(colors: [Color(red: 0.106, green: 0.122, blue: 0.141),
                                              Color(red: 0.051, green: 0.067, blue: 0.090)],
                                    startPoint: .topLeading, endPoint: .bottomTrailing),
                     in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .padding(.horizontal, 16)
        .padding(.top, 8)
        .padding(.bottom, 16)
    }

    private func stepRow(_ s: Step) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Circle()
                .fill(OnymTokens.blue)
                .frame(width: 28, height: 28)
                .overlay(Text("\(s.n)").font(.system(size: 14, weight: .bold)).foregroundStyle(.white))
            VStack(alignment: .leading, spacing: 4) {
                Text(s.title).font(.system(size: 16.5, weight: .semibold)).foregroundStyle(OnymTokens.text)
                Text(s.body).font(.system(size: 13.5)).foregroundStyle(OnymTokens.text2).lineSpacing(2)
                if let cmd = s.cmd {
                    codeBlock(cmd, label: s.title)
                        .padding(.top, 8)
                }
            }
        }
        .padding(.horizontal, 16).padding(.vertical, 12)
    }

    private func codeBlock(_ text: String, label: String) -> some View {
        ZStack(alignment: .topTrailing) {
            Text(text)
                .font(.system(size: 12, design: .monospaced))
                .foregroundStyle(Color(red: 0.65, green: 1.0, blue: 0.6))
                .lineSpacing(3)
                .padding(.horizontal, 12).padding(.vertical, 12)
                .padding(.trailing, 36)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color(red: 0.051, green: 0.067, blue: 0.090),
                            in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            Button {
                UIPasteboard.general.string = text
                copied = label
                DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { if copied == label { copied = nil } }
            } label: {
                Image(systemName: copied == label ? "checkmark" : "doc.on.doc")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(copied == label
                                     ? Color(red: 0.65, green: 1.0, blue: 0.6)
                                     : .white)
                    .frame(width: 26, height: 26)
                    .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 6))
            }
            .buttonStyle(.plain)
            .padding(8)
        }
    }

    private func open(_ s: String) {
        guard let u = URL(string: s) else { return }
        UIApplication.shared.open(u)
    }
}
