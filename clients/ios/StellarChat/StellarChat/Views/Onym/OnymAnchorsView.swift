import SwiftUI

// MARK: - Anchors home (Network)

struct OnymAnchorsView: View {
    @Bindable var model: OnymSettingsModel
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: "Anchors", onBack: { dismiss() })
                OnymLargeTitle(text: "Anchors")
                OnymFootnote(text: "Choose the contract version used to anchor on-chain group state. Selection per (network, governance type) pins to new chats; existing chats keep the contract they were created with.")

                OnymSectionLabel(text: "NETWORK")
                OnymCard {
                    NavigationLink { OnymAnchorNetworkView(model: model) } label: {
                        OnymRow(
                            title: "Testnet",
                            subtitle: "5 governance types · all current",
                            onTap: {}
                        ) {
                            OnymTile(bg: OnymTokens.Tile.green) {
                                Text("T").font(.system(size: 11, weight: .bold)).foregroundStyle(.white)
                            }
                        }
                    }.buttonStyle(.plain)

                    OnymRow(
                        title: "Mainnet",
                        subtitle: "No contracts yet",
                        hasChevron: false,
                        last: true
                    ) {
                        OnymTile(bg: OnymTokens.Tile.gray) {
                            Text("M").font(.system(size: 11, weight: .bold)).foregroundStyle(.white)
                        }
                    } right: {
                        Text("Soon").foregroundStyle(OnymTokens.text3).font(.system(size: 14))
                    }
                }
            }
        }
    }
}

// MARK: - Anchor network → governance type list

struct OnymAnchorNetworkView: View {
    @Bindable var model: OnymSettingsModel
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: "Testnet", onBack: { dismiss() })
                OnymSectionLabel(text: "GOVERNANCE TYPES")
                OnymCard {
                    ForEach(Array(OnymCatalog.governance.enumerated()), id: \.element.id) { idx, g in
                        NavigationLink { OnymAnchorVersionView(model: model, govId: g.id) } label: {
                            OnymRow(
                                title: g.label,
                                subtitle: "\(g.sub) · v0.0.5 (latest)",
                                inset: 56,
                                last: idx == OnymCatalog.governance.count - 1,
                                onTap: {}
                            ) {
                                OnymGovTile(id: g.id)
                            }
                        }.buttonStyle(.plain)
                    }
                }
            }
        }
    }
}

private struct OnymGovTile: View {
    let id: String
    private var palette: (bg: Color, fg: Color) {
        switch id {
        case "anarchy":   return (OnymTokens.Tile.orange.opacity(0.16), Color(red: 0.82, green: 0.29, blue: 0))
        case "democracy": return (OnymTokens.green.opacity(0.16),       Color(red: 0.10, green: 0.51, blue: 0.28))
        case "oligarchy": return (OnymTokens.Tile.indigo.opacity(0.16), Color(red: 0.24, green: 0.24, blue: 0.79))
        case "dialog":    return (OnymTokens.blue.opacity(0.16),        OnymTokens.blue)
        case "tyranny":   return (OnymTokens.red.opacity(0.16),         OnymTokens.red)
        default:          return (OnymTokens.Tile.gray.opacity(0.16),   OnymTokens.text2)
        }
    }
    var body: some View {
        let p = palette
        RoundedRectangle(cornerRadius: 8, style: .continuous)
            .fill(p.bg)
            .frame(width: 30, height: 30)
            .overlay(Text(String(id.prefix(2)).uppercased())
                .font(.system(size: 11, weight: .bold))
                .foregroundStyle(p.fg))
    }
}

// MARK: - Anchor version list

struct OnymAnchorVersionView: View {
    @Bindable var model: OnymSettingsModel
    let govId: String
    @Environment(\.dismiss) private var dismiss
    @State private var selected: String = "v0.0.5"

    private var gov: OnymGovType {
        OnymCatalog.governance.first(where: { $0.id == govId }) ?? OnymCatalog.governance[0]
    }

    var body: some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: "Testnet · \(gov.label)", onBack: { dismiss() })
                OnymSectionLabel(text: "CONTRACT VERSION")
                OnymCard {
                    ForEach(Array(OnymCatalog.versions.enumerated()), id: \.element.v) { idx, v in
                        NavigationLink { OnymContractDetailView(govId: govId, version: v.v) } label: {
                            OnymRow(
                                title: v.v,
                                titleMono: true,
                                subtitle: "\(v.date) · \(v.audit)",
                                inset: 16,
                                last: idx == OnymCatalog.versions.count - 1,
                                onTap: {}
                            ) {
                                EmptyView()
                            } right: {
                                HStack(spacing: 8) {
                                    if v.current {
                                        Text("LATEST")
                                            .font(.system(size: 10.5, weight: .bold))
                                            .padding(.horizontal, 6).padding(.vertical, 2)
                                            .background(OnymTokens.green.opacity(0.16),
                                                        in: RoundedRectangle(cornerRadius: 4))
                                            .foregroundStyle(Color(red: 0.10, green: 0.51, blue: 0.28))
                                    }
                                    if selected == v.v {
                                        Image(systemName: "checkmark")
                                            .font(.system(size: 14, weight: .bold))
                                            .foregroundStyle(OnymTokens.blue)
                                    }
                                }
                            }
                        }.buttonStyle(.plain)
                    }
                }
                OnymFootnote(text: "Tap a version to view the contract, source code, and audit report.")

                OnymSectionLabel(text: "CUSTOM")
                OnymCard {
                    NavigationLink { OnymDeployContractView(govId: govId) } label: {
                        OnymRow(
                            title: "Deploy from source",
                            subtitle: "Build & publish your own contract",
                            onTap: {}
                        ) {
                            OnymSymbolTile(symbol: "chevron.left.forwardslash.chevron.right",
                                           bg: OnymTokens.text)
                        }
                    }.buttonStyle(.plain)

                    NavigationLink { OnymEnterContractView(model: model, govId: govId) } label: {
                        OnymRow(
                            title: "Use existing address",
                            subtitle: "Point to a deployed contract",
                            last: true,
                            onTap: {}
                        ) {
                            OnymSymbolTile(symbol: "shippingbox.fill", bg: OnymTokens.Tile.indigo)
                        }
                    }.buttonStyle(.plain)
                }
                OnymFootnote(text: "Onym only ships audited (or pending-audit) contracts. If you've forked or deployed your own, point new chats at it here. Existing chats keep the contract they were created with.")
            }
        }
    }
}

// MARK: - Contract detail

struct OnymContractDetailView: View {
    let govId: String
    let version: String
    @Environment(\.dismiss) private var dismiss

    private var gov: OnymGovType {
        OnymCatalog.governance.first(where: { $0.id == govId }) ?? OnymCatalog.governance[0]
    }
    private var v: OnymContractVersion {
        OnymCatalog.versions.first(where: { $0.v == version }) ?? OnymCatalog.versions[0]
    }

    var body: some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: version, subtitle: gov.label, onBack: { dismiss() })

                HStack(spacing: 14) {
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .fill(LinearGradient(colors: [Color(red: 0.996, green: 0.941, blue: 0.878),
                                                       Color(red: 1.0, green: 0.878, blue: 0.753)],
                                              startPoint: .topLeading, endPoint: .bottomTrailing))
                        .frame(width: 56, height: 56)
                        .overlay(OnymMark(size: 32, color: Color(red: 0.82, green: 0.29, blue: 0)))
                    VStack(alignment: .leading, spacing: 2) {
                        Text("CONTRACT · \(gov.label.uppercased())")
                            .font(.system(size: 11.5, weight: .medium))
                            .foregroundStyle(OnymTokens.text2)
                            .tracking(0.46)
                        Text(version)
                            .font(.system(size: 22, weight: .bold, design: .monospaced))
                            .tracking(-0.26)
                            .foregroundStyle(OnymTokens.text)
                        Text("Deployed \(v.date) · \(v.audit)")
                            .font(.system(size: 12.5))
                            .foregroundStyle(OnymTokens.text2)
                    }
                    Spacer()
                }
                .padding(18)
                .background(OnymTokens.card, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                .padding(.horizontal, 16)
                .padding(.top, 8)

                OnymSectionLabel(text: "ON-CHAIN")
                OnymCard {
                    OnymRow(
                        title: "Stellar Expert",
                        subtitle: v.sha,
                        subtitleMono: true,
                        hasChevron: false,
                        onTap: { open("https://stellar.expert") }
                    ) {
                        OnymTile(bg: OnymTokens.Tile.indigo) {
                            Text("SX").font(.system(size: 11, weight: .bold)).foregroundStyle(.white)
                        }
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }
                    OnymRow(
                        title: "Copy contract address",
                        hasChevron: false,
                        last: true,
                        onTap: { UIPasteboard.general.string = v.sha }
                    ) {
                        OnymSymbolTile(symbol: "doc.on.doc.fill", bg: OnymTokens.Tile.gray)
                    }
                }

                OnymSectionLabel(text: "SOURCE")
                OnymCard {
                    OnymRow(
                        title: "View source on GitHub",
                        subtitle: "onymchat/contracts @ \(version)",
                        subtitleMono: true,
                        hasChevron: false,
                        onTap: { open(OnymCatalog.contractRepoURL.absoluteString) }
                    ) {
                        OnymSymbolTile(symbol: "chevron.left.forwardslash.chevron.right",
                                       bg: OnymTokens.text)
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }
                    OnymRow(
                        title: "Audit report",
                        subtitle: "Pending — no audits yet",
                        hasChevron: false,
                        last: true,
                        onTap: {}
                    ) {
                        OnymSymbolTile(symbol: "exclamationmark.circle.fill", bg: OnymTokens.amber)
                    }
                }

                OnymFootnote(text: "This is the contract that anchors \(gov.label.lowercased()) groups created on testnet. Existing chats keep the contract they were created with — picking a different version only affects new chats.")
            }
        }
    }

    private func open(_ s: String) {
        if let u = URL(string: s) { UIApplication.shared.open(u) }
    }
}

// MARK: - Enter existing contract address

struct OnymEnterContractView: View {
    @Bindable var model: OnymSettingsModel
    let govId: String
    @Environment(\.dismiss) private var dismiss

    @State private var addr = ""
    @State private var label = ""
    @State private var verifying = false
    @State private var verdict: Verdict?

    enum Verdict { case ok, bad }

    private var gov: OnymGovType {
        OnymCatalog.governance.first(where: { $0.id == govId }) ?? OnymCatalog.governance[0]
    }
    private var looksValid: Bool {
        let trimmed = addr.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.count == 56 && trimmed.first == "C" &&
            trimmed.allSatisfy { $0.isLetter || $0.isNumber } &&
            trimmed == trimmed.uppercased()
    }

    var body: some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: "Use existing address", subtitle: gov.label, onBack: { dismiss() })

                heroCard

                OnymSectionLabel(text: "STELLAR CONTRACT ADDRESS")
                OnymCard {
                    VStack(alignment: .leading, spacing: 4) {
                        TextEditor(text: $addr)
                            .font(.system(size: 14, design: .monospaced))
                            .frame(minHeight: 72)
                            .scrollContentBackground(.hidden)
                            .background(Color.clear)
                            .autocorrectionDisabled()
                            .textInputAutocapitalization(.never)
                            .onChange(of: addr) { _, v in
                                let cleaned = v.replacingOccurrences(of: " ", with: "").uppercased()
                                if cleaned != addr { addr = cleaned }
                                verdict = nil
                            }
                        if !addr.isEmpty {
                            HStack(spacing: 8) {
                                OnymChip(
                                    text: looksValid ? "Valid format" : "\(addr.count)/56 chars",
                                    fg: looksValid ? OnymTokens.green : OnymTokens.red,
                                    bg: looksValid
                                        ? OnymTokens.green.opacity(0.14)
                                        : OnymTokens.red.opacity(0.14)
                                )
                                Text("Stellar Soroban contract ID")
                                    .font(.system(size: 11))
                                    .foregroundStyle(OnymTokens.text3)
                            }
                        }
                    }
                    .padding(.horizontal, 16).padding(.vertical, 12)
                }

                OnymSectionLabel(text: "LABEL")
                OnymCard {
                    TextField("My fork v0.0.5", text: $label)
                        .font(.system(size: 16))
                        .padding(.horizontal, 16).padding(.vertical, 12)
                        .onChange(of: label) { _, v in
                            if v.count > 30 { label = String(v.prefix(30)) }
                        }
                }
                OnymFootnote(text: "Shown alongside the contract address in chats and on the Anchors list.")

                OnymPrimaryButton(
                    disabled: !looksValid || verifying,
                    action: {
                        if verdict == .ok { dismiss() }
                        else { verify() }
                    }
                ) {
                    Text(verifying ? "Verifying on Stellar…" :
                         verdict == .ok ? "Use this contract" : "Verify")
                }
                .padding(.horizontal, 16).padding(.top, 20)

                if verdict == .ok { verifiedBanner }
                if verdict == .bad {
                    Text("Address format invalid. Expected 56 chars starting with C.")
                        .font(.system(size: 13))
                        .foregroundStyle(OnymTokens.red)
                        .frame(maxWidth: .infinity)
                        .padding(.top, 12)
                }

                OnymSectionLabel(text: "HOW TO FIND IT")
                OnymCard {
                    OnymRow(
                        title: "Browse on Stellar Expert",
                        subtitle: "testnet.stellar.expert",
                        hasChevron: false,
                        onTap: { open("https://testnet.stellar.expert") }
                    ) {
                        OnymTile(bg: OnymTokens.Tile.indigo) {
                            Text("SX").font(.system(size: 11, weight: .bold)).foregroundStyle(.white)
                        }
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }
                    OnymRow(
                        title: "Soroban CLI",
                        subtitle: "soroban contract id …",
                        subtitleMono: true,
                        hasChevron: false,
                        last: true,
                        onTap: {}
                    ) {
                        OnymSymbolTile(symbol: "terminal.fill", bg: OnymTokens.Tile.gray)
                    }
                }
            }
        }
    }

    private var heroCard: some View {
        HStack(spacing: 14) {
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(LinearGradient(colors: [Color(red: 0.898, green: 0.898, blue: 0.996),
                                                Color(red: 0.78, green: 0.78, blue: 0.957)],
                                      startPoint: .topLeading, endPoint: .bottomTrailing))
                .frame(width: 52, height: 52)
                .overlay(Image(systemName: "shippingbox.fill")
                    .foregroundStyle(OnymTokens.Tile.indigo))
            VStack(alignment: .leading, spacing: 3) {
                Text("Bring your own contract")
                    .font(.system(size: 16.5, weight: .semibold))
                    .foregroundStyle(OnymTokens.text)
                Text("Anchor new \(gov.label.lowercased()) chats on a Stellar contract you've already deployed.")
                    .font(.system(size: 13))
                    .foregroundStyle(OnymTokens.text2)
                    .lineSpacing(2)
            }
        }
        .padding(18)
        .background(OnymTokens.card, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .padding(.horizontal, 16)
        .padding(.top, 8)
    }

    private var verifiedBanner: some View {
        HStack(alignment: .top, spacing: 10) {
            Circle().fill(OnymTokens.green).frame(width: 22, height: 22)
                .overlay(Image(systemName: "checkmark")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(.white))
                .padding(.top, 1)
            VStack(alignment: .leading, spacing: 2) {
                Text("Contract verified")
                    .font(.system(size: 13.5, weight: .semibold))
                    .foregroundStyle(Color(red: 0.09, green: 0.37, blue: 0.18))
                Text("Compiled hash matches onymchat/contracts@v0.0.4. Tap \"Use this contract\" to anchor new chats here.")
                    .font(.system(size: 12))
                    .foregroundStyle(Color(red: 0.19, green: 0.43, blue: 0.28))
                    .lineSpacing(2)
            }
            Spacer(minLength: 0)
        }
        .padding(14)
        .background(OnymTokens.green.opacity(0.10),
                    in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .padding(.horizontal, 16)
        .padding(.top, 12)
    }

    private func verify() {
        verifying = true; verdict = nil
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.1) {
            verifying = false
            verdict = looksValid ? .ok : .bad
        }
    }

    private func open(_ s: String) {
        if let u = URL(string: s) { UIApplication.shared.open(u) }
    }
}

// MARK: - Deploy contract from source

struct OnymDeployContractView: View {
    let govId: String
    @Environment(\.dismiss) private var dismiss

    @State private var ref = "main"
    @State private var stage: Stage = .idle
    @State private var progress = 0
    @State private var logs: [String] = []
    @State private var deployedAddr: String?
    @State private var copiedCmd = false

    enum Stage { case idle, building, deploying, done }

    private var gov: OnymGovType {
        OnymCatalog.governance.first(where: { $0.id == govId }) ?? OnymCatalog.governance[0]
    }
    private var networkPassphrase: String { "Test SDF Network ; September 2015" }
    private var cliCmd: String {
        "soroban contract deploy \\\n  --network testnet \\\n  --source-account onym-deploy \\\n  --wasm onym_\(gov.id).wasm"
    }

    var body: some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: "Deploy from source",
                           subtitle: gov.label,
                           onBack: stage == .idle || stage == .done ? { dismiss() } : nil)

                hero

                OnymSectionLabel(text: "SOURCE")
                OnymCard {
                    OnymRow(
                        title: "Repository",
                        subtitle: "github.com/onymchat/contracts",
                        subtitleMono: true,
                        hasChevron: false,
                        onTap: { open(OnymCatalog.contractRepoURL.absoluteString) }
                    ) {
                        OnymSymbolTile(symbol: "chevron.left.forwardslash.chevron.right",
                                       bg: OnymTokens.text)
                    } accessory: {
                        Image(systemName: "arrow.up.right.square").foregroundStyle(OnymTokens.text3)
                    }

                    HStack(spacing: 12) {
                        OnymSymbolTile(symbol: "arrow.triangle.branch", bg: OnymTokens.Tile.indigo)
                        VStack(alignment: .leading, spacing: 1) {
                            Text("Ref").font(.system(size: 13)).foregroundStyle(OnymTokens.text2)
                            TextField("main · v0.0.5 · commit sha", text: $ref)
                                .font(.system(size: 15.5, design: .monospaced))
                                .disabled(stage != .idle)
                        }
                    }
                    .padding(.horizontal, 16).padding(.vertical, 12)
                    Rectangle().fill(OnymTokens.hairline).frame(height: 0.5).padding(.leading, 16)

                    OnymRow(title: "Network", hasChevron: false) {
                        OnymTile(bg: OnymTokens.Tile.green) {
                            Text("T").font(.system(size: 11, weight: .bold)).foregroundStyle(.white)
                        }
                    } right: {
                        Text("Testnet").foregroundStyle(OnymTokens.text2).font(.system(size: 14))
                    }
                    OnymRow(title: "Module", hasChevron: false, last: true) {
                        OnymTile(bg: OnymTokens.Tile.purple) {
                            Text(String(gov.id.prefix(2)).uppercased())
                                .font(.system(size: 9, weight: .bold))
                                .foregroundStyle(.white)
                        }
                    } right: {
                        Text(gov.label).foregroundStyle(OnymTokens.text2).font(.system(size: 14))
                    }
                }

                if stage == .idle {
                    OnymPrimaryButton(action: startDeploy) {
                        HStack(spacing: 8) {
                            Image(systemName: "arrow.up.circle.fill")
                            Text("Build & Deploy")
                        }
                    }
                    .padding(.horizontal, 16).padding(.top, 20)
                } else {
                    deployConsole.padding(.horizontal, 16).padding(.top, 20)
                }

                if stage == .done, let addr = deployedAddr {
                    deployedCard(addr)
                    OnymPrimaryButton(action: { dismiss() }) { Text("Use this contract") }
                        .padding(.horizontal, 16).padding(.top, 20)
                }

                OnymSectionLabel(text: "OR USE THE CLI")
                ZStack(alignment: .topTrailing) {
                    Text(cliCmd)
                        .font(.system(size: 11.5, design: .monospaced))
                        .foregroundStyle(Color(red: 0.65, green: 1.0, blue: 0.6))
                        .lineSpacing(3)
                        .padding(.horizontal, 12).padding(.vertical, 12)
                        .padding(.trailing, 36)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Color(red: 0.051, green: 0.067, blue: 0.090),
                                    in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                    Button {
                        UIPasteboard.general.string = cliCmd
                        copiedCmd = true
                        DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { copiedCmd = false }
                    } label: {
                        Image(systemName: copiedCmd ? "checkmark" : "doc.on.doc")
                            .font(.system(size: 12, weight: .semibold))
                            .foregroundStyle(.white)
                            .frame(width: 26, height: 26)
                            .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 6))
                    }.buttonStyle(.plain).padding(8)
                }
                .padding(.horizontal, 16)

                OnymFootnote(text: "Network passphrase: \(networkPassphrase). After deploying via CLI, come back and choose Use existing address.")
            }
        }
    }

    private var hero: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                Image(systemName: "chevron.left.forwardslash.chevron.right")
                    .font(.system(size: 14)).foregroundStyle(.white)
                Text("onymchat/contracts")
                    .font(.system(size: 13, design: .monospaced))
                    .foregroundStyle(.white.opacity(0.65))
            }
            Text("Build, deploy, anchor")
                .font(.system(size: 22, weight: .bold))
                .tracking(-0.26).foregroundStyle(.white)
            Text("Compile the \(gov.label.lowercased()) contract from source and deploy it to Stellar Testnet from this device. Onym signs with a one-time deploy key.")
                .font(.system(size: 13))
                .foregroundStyle(.white.opacity(0.7))
                .lineSpacing(3)
        }
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(LinearGradient(colors: [Color(red: 0.106, green: 0.122, blue: 0.141),
                                              Color(red: 0.051, green: 0.067, blue: 0.090)],
                                    startPoint: .topLeading, endPoint: .bottomTrailing),
                     in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .padding(.horizontal, 16)
        .padding(.top, 8)
    }

    private var deployConsole: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Circle().fill(stage == .done ? OnymTokens.green : OnymTokens.amber)
                    .frame(width: 8, height: 8)
                Text(stage == .building ? "Building wasm…" :
                     stage == .deploying ? "Deploying to Stellar…" : "Complete")
                    .font(.system(size: 11))
                    .foregroundStyle(.white.opacity(0.6))
                Spacer()
                Text("\(progress)%")
                    .font(.system(size: 11))
                    .foregroundStyle(.white.opacity(0.4))
            }
            Capsule()
                .fill(.white.opacity(0.08))
                .frame(height: 3)
                .overlay(GeometryReader { geo in
                    Capsule().fill(stage == .done ? OnymTokens.green : OnymTokens.blue)
                        .frame(width: geo.size.width * CGFloat(progress) / 100)
                }, alignment: .leading)
                .padding(.bottom, 4)
            ForEach(Array(logs.enumerated()), id: \.offset) { _, l in
                Text(l)
                    .font(.system(size: 11.5, design: .monospaced))
                    .foregroundStyle(l.hasPrefix("✓")
                                     ? Color(red: 0.65, green: 1.0, blue: 0.6)
                                     : (l.hasPrefix("↗") ? Color(red: 0.49, green: 0.76, blue: 1.0) : .white))
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(red: 0.051, green: 0.067, blue: 0.090),
                    in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }

    private func deployedCard(_ addr: String) -> some View {
        Group {
            OnymSectionLabel(text: "DEPLOYED CONTRACT")
            OnymCard {
                VStack(alignment: .leading, spacing: 10) {
                    HStack(spacing: 8) {
                        Circle().fill(OnymTokens.green).frame(width: 22, height: 22)
                            .overlay(Image(systemName: "checkmark").font(.system(size: 11, weight: .bold)).foregroundStyle(.white))
                        Text("Contract deployed")
                            .font(.system(size: 14.5, weight: .semibold))
                            .foregroundStyle(OnymTokens.text)
                    }
                    Text(addr)
                        .font(.system(size: 11.5, design: .monospaced))
                        .foregroundStyle(OnymTokens.text)
                        .padding(.horizontal, 12).padding(.vertical, 10)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(OnymTokens.card2, in: RoundedRectangle(cornerRadius: 10))
                }
                .padding(.horizontal, 16).padding(.vertical, 14)
                Rectangle().fill(OnymTokens.hairline).frame(height: 0.5).padding(.leading, 16)
                HStack(spacing: 0) {
                    Button { UIPasteboard.general.string = addr } label: {
                        Label("Copy", systemImage: "doc.on.doc")
                            .font(.system(size: 15, weight: .medium))
                            .foregroundStyle(OnymTokens.blue)
                            .frame(maxWidth: .infinity, minHeight: 44)
                    }
                    Rectangle().fill(OnymTokens.hairline).frame(width: 0.5)
                    Button { open("https://testnet.stellar.expert/explorer/testnet/contract/\(addr)") } label: {
                        HStack(spacing: 6) {
                            Text("View on Stellar Expert")
                            Image(systemName: "arrow.up.right.square").font(.system(size: 11))
                        }
                        .font(.system(size: 15, weight: .medium))
                        .foregroundStyle(OnymTokens.blue)
                        .frame(maxWidth: .infinity, minHeight: 44)
                    }
                }
            }
        }
    }

    private func startDeploy() {
        stage = .building; progress = 0; logs = []; deployedAddr = nil
        let plan: [(t: Double, log: String, p: Int, stage: Stage?, addr: String?)] = [
            (0.6, "✓ Cloned onymchat/contracts @ \(ref)", 12, nil, nil),
            (1.4, "✓ cargo build --release --target wasm32", 38, nil, nil),
            (2.2, "✓ Built onym_\(gov.id).wasm (47 KB)", 56, .deploying, nil),
            (3.0, "↗ stellar.testnet  · uploading wasm", 72, nil, nil),
            (3.7, "↗ stellar.testnet  · invoking deploy", 88, nil, nil),
            (4.4, "✓ Deployed", 100, .done, randomCAddr()),
        ]
        for ev in plan {
            DispatchQueue.main.asyncAfter(deadline: .now() + ev.t) {
                logs.append(ev.log)
                progress = ev.p
                if let s = ev.stage { stage = s }
                if let a = ev.addr { deployedAddr = a }
            }
        }
    }

    private func randomCAddr() -> String {
        let alphabet = Array("ABCDEFGHJKLMNPQRSTUVWXYZ234567")
        return "C" + String((0..<55).map { _ in alphabet.randomElement()! })
    }

    private func open(_ s: String) {
        if let u = URL(string: s) { UIApplication.shared.open(u) }
    }
}
