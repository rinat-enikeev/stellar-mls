import SwiftUI

// MARK: - Identities list

struct OnymIdentitiesView: View {
    @Bindable var model: OnymSettingsModel
    @Environment(\.dismiss) private var dismiss
    @State private var showAdd = false

    var body: some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: "Identities", onBack: { dismiss() })
                OnymLargeTitle(text: "Identities")
                OnymFootnote(text: "Tap an identity to open it. Each identity has its own keys, chats, and recovery phrase.")

                OnymSectionLabel(text: "YOUR IDENTITIES")
                OnymCard {
                    ForEach(Array(model.identities.enumerated()), id: \.element.id) { index, id in
                        NavigationLink { OnymIdentityDetailView(model: model, identityId: id.id) } label: {
                            OnymRow(
                                title: id.name,
                                subtitle: String(id.npub.prefix(18)) + "…",
                                subtitleMono: true,
                                inset: 68,
                                last: index == model.identities.count - 1,
                                onTap: {}
                            ) {
                                OnymIdentityTile(active: id.active, size: 40)
                            } right: {
                                HStack(spacing: 6) {
                                    if id.active {
                                        Text("Active")
                                            .font(.system(size: 11, weight: .semibold))
                                            .padding(.horizontal, 8).padding(.vertical, 3)
                                            .background(OnymTokens.green.opacity(0.14),
                                                        in: RoundedRectangle(cornerRadius: 10))
                                            .foregroundStyle(OnymTokens.green)
                                    }
                                    if !id.backedUp {
                                        Image(systemName: "exclamationmark.triangle.fill")
                                            .font(.system(size: 11))
                                            .foregroundStyle(OnymTokens.amber)
                                    }
                                }
                            }
                        }.buttonStyle(.plain)
                    }
                }

                Button { showAdd = true } label: {
                    HStack(spacing: 10) {
                        Circle().fill(OnymTokens.blue).frame(width: 22, height: 22)
                            .overlay(Image(systemName: "plus")
                                .font(.system(size: 13, weight: .bold))
                                .foregroundStyle(.white))
                        Text("Add Identity")
                            .font(.system(size: 16, weight: .medium))
                            .foregroundStyle(OnymTokens.blue)
                        Spacer()
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 13)
                    .background(OnymTokens.card, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .padding(.horizontal, 16)
                    .padding(.top, 12)
                }
                .buttonStyle(.plain)

                OnymFootnote(text: "Only the active identity's chats are visible. Switch between identities to see different inboxes.")
            }
        }
        .sheet(isPresented: $showAdd) {
            OnymAddIdentityView(model: model)
        }
    }
}

// MARK: - Identity detail

struct OnymIdentityDetailView: View {
    @Bindable var model: OnymSettingsModel
    let identityId: String
    @Environment(\.dismiss) private var dismiss
    @State private var editing = false
    @State private var editName = ""

    private var identity: OnymIdentity? {
        model.identities.first(where: { $0.id == identityId })
    }

    var body: some View {
        guard let id = identity else {
            return AnyView(OnymPage { EmptyView() })
        }
        return AnyView(content(id: id))
    }

    private func content(id: OnymIdentity) -> some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: id.name, onBack: { dismiss() })
                hero(id)
                inviteCard(id)
                OnymFootnote(text: "Anyone with this code can start a chat with you. The chat itself is end-to-end encrypted.")

                OnymSectionLabel(text: "BACKUP")
                OnymCard {
                    HStack(spacing: 12) {
                        OnymTile(bg: id.backedUp ? OnymTokens.green : OnymTokens.amber, size: 36) {
                            Image(systemName: id.backedUp ? "checkmark" : "exclamationmark")
                                .font(.system(size: 16, weight: .bold))
                                .foregroundStyle(.white)
                        }
                        VStack(alignment: .leading, spacing: 1) {
                            Text(id.backedUp ? "Recovery phrase saved" : "Not backed up yet")
                                .font(.system(size: 15, weight: .semibold))
                                .foregroundStyle(OnymTokens.text)
                            Text(id.backedUp
                                 ? "You verified your 12 words. Keep them safe."
                                 : "Without your phrase you can't restore this identity.")
                                .font(.system(size: 12.5))
                                .foregroundStyle(OnymTokens.text2)
                        }
                        Spacer()
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 14)
                    Rectangle().fill(OnymTokens.hairline).frame(height: 0.5).padding(.leading, 16)

                    NavigationLink { OnymBackupFlowView(model: model, identityId: id.id) } label: {
                        OnymRow(
                            title: id.backedUp ? "View recovery phrase" : "Back up now",
                            subtitle: "12 words · BIP-39",
                            last: true,
                            onTap: {}
                        ) {
                            OnymSymbolTile(symbol: "key.fill", bg: OnymTokens.Tile.orange)
                        }
                    }.buttonStyle(.plain)
                }

                OnymSectionLabel(text: "STATE")
                OnymCard {
                    OnymRow(
                        title: "Set as active",
                        hasChevron: !id.active,
                        onTap: id.active ? nil : { model.setActive(id.id) }
                    ) {
                        OnymSymbolTile(symbol: "checkmark.circle.fill", bg: OnymTokens.Tile.green)
                    } right: {
                        if id.active { Text("Active").font(.system(size: 14.5)).foregroundStyle(OnymTokens.text2) }
                    }
                    NavigationLink { OnymShareKeyView(identity: id) } label: {
                        OnymRow(
                            title: "Share invite key",
                            subtitle: "QR code or link",
                            last: true,
                            onTap: {}
                        ) {
                            OnymSymbolTile(symbol: "square.and.arrow.up", bg: OnymTokens.Tile.indigo)
                        }
                    }.buttonStyle(.plain)
                }

                OnymSectionLabel(text: "ADVANCED")
                OnymCard {
                    OnymRow(
                        title: "Copy public key",
                        hasChevron: false,
                        onTap: { UIPasteboard.general.string = id.npub }
                    ) {
                        OnymSymbolTile(symbol: "doc.on.doc.fill", bg: OnymTokens.Tile.gray)
                    }
                    OnymRow(
                        title: "Delete identity",
                        danger: true,
                        hasChevron: false,
                        last: true,
                        onTap: {}
                    ) {
                        OnymSymbolTile(symbol: "trash.fill", bg: OnymTokens.Tile.red)
                    }
                }

                OnymFootnote(text: "Deleting an identity removes its keys from this device. If you've backed up the recovery phrase, you can restore it later.")
            }
        }
    }

    private func hero(_ id: OnymIdentity) -> some View {
        VStack(spacing: 4) {
            ZStack(alignment: .bottomTrailing) {
                Circle()
                    .fill(id.active
                          ? AnyShapeStyle(LinearGradient(colors: [Color(red: 0.933, green: 0.961, blue: 1.0),
                                                                   Color(red: 0.835, green: 0.910, blue: 0.996)],
                                                         startPoint: .topLeading, endPoint: .bottomTrailing))
                          : AnyShapeStyle(Color(red: 0.937, green: 0.937, blue: 0.949)))
                    .frame(width: 96, height: 96)
                    .overlay(Circle().stroke(id.active ? OnymTokens.blue : .clear, lineWidth: 2))
                    .overlay(OnymMark(size: 64, color: id.active ? OnymTokens.blue : Color(red: 142/255, green: 142/255, blue: 147/255)))
                if id.backedUp {
                    Circle().fill(OnymTokens.green)
                        .frame(width: 28, height: 28)
                        .overlay(Circle().stroke(.white, lineWidth: 3))
                        .overlay(Image(systemName: "checkmark")
                            .font(.system(size: 13, weight: .bold))
                            .foregroundStyle(.white))
                }
            }
            .padding(.top, 8)

            if editing {
                TextField("", text: $editName, onCommit: commit)
                    .font(.system(size: 24, weight: .bold))
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 10).padding(.vertical, 4)
                    .background(RoundedRectangle(cornerRadius: 8).stroke(OnymTokens.blue, lineWidth: 1.5))
                    .frame(maxWidth: 240)
                    .padding(.top, 14)
            } else {
                Button {
                    editName = id.name
                    editing = true
                } label: {
                    HStack(spacing: 6) {
                        Text(id.name)
                            .font(.system(size: 24, weight: .bold))
                            .foregroundStyle(OnymTokens.text)
                        Image(systemName: "pencil")
                            .font(.system(size: 13))
                            .foregroundStyle(OnymTokens.text3)
                    }
                }
                .buttonStyle(.plain)
                .padding(.top, 14)
            }

            Text(id.npub.prefix(22) + "…")
                .font(.system(size: 12, design: .monospaced))
                .foregroundStyle(OnymTokens.text2)
            Text("Created \(id.created)")
                .font(.system(size: 12))
                .foregroundStyle(OnymTokens.text3)
        }
        .frame(maxWidth: .infinity)
        .padding(.bottom, 24)
    }

    private func inviteCard(_ id: OnymIdentity) -> some View {
        Group {
            OnymSectionLabel(text: "INVITE KEY")
            OnymCard {
                VStack(spacing: 14) {
                    OnymQRCode(value: onymInviteURL(for: id), size: 200)
                        .padding(12)
                        .background(.white, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                        .overlay(RoundedRectangle(cornerRadius: 18, style: .continuous)
                            .stroke(.black.opacity(0.04), lineWidth: 1))
                    Text("Scan with Onym on another device to open a private chat with this identity.")
                        .font(.system(size: 13))
                        .foregroundStyle(OnymTokens.text2)
                        .multilineTextAlignment(.center)
                        .lineSpacing(2)
                        .frame(maxWidth: 280)
                    Text(onymInviteURL(for: id))
                        .font(.system(size: 11.5, design: .monospaced))
                        .foregroundStyle(OnymTokens.text2)
                        .lineLimit(1).truncationMode(.middle)
                        .padding(.horizontal, 10).padding(.vertical, 6)
                        .background(OnymTokens.card2, in: RoundedRectangle(cornerRadius: 8))
                }
                .padding(.horizontal, 16).padding(.top, 20).padding(.bottom, 16)

                Rectangle().fill(OnymTokens.hairline).frame(height: 0.5).padding(.leading, 16)

                HStack(spacing: 0) {
                    Button { UIPasteboard.general.string = onymInviteURL(for: id) } label: {
                        Label("Copy link", systemImage: "doc.on.doc")
                            .font(.system(size: 15, weight: .medium))
                            .foregroundStyle(OnymTokens.blue)
                            .frame(maxWidth: .infinity, minHeight: 44)
                    }
                    Rectangle().fill(OnymTokens.hairline).frame(width: 0.5)
                    Button {} label: {
                        Label("Share", systemImage: "square.and.arrow.up")
                            .font(.system(size: 15, weight: .medium))
                            .foregroundStyle(OnymTokens.blue)
                            .frame(maxWidth: .infinity, minHeight: 44)
                    }
                }
            }
        }
    }

    private func commit() {
        let trimmed = editName.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
            model.identities = model.identities.map { var c = $0; if $0.id == identityId { c.name = String(trimmed.prefix(30)) }; return c }
        }
        editing = false
    }
}

// MARK: - Add identity sheet

struct OnymAddIdentityView: View {
    @Bindable var model: OnymSettingsModel
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var phrase = ""

    var body: some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                HStack {
                    Button("Cancel") { dismiss() }
                        .foregroundStyle(OnymTokens.blue)
                    Spacer()
                    Text("Add Identity").font(.system(size: 16, weight: .semibold))
                    Spacer()
                    Button("Add") {
                        model.appendIdentity(name: name)
                        dismiss()
                    }
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(OnymTokens.blue)
                }
                .padding(.horizontal, 16).padding(.vertical, 14)

                Circle().fill(Color(red: 0.937, green: 0.937, blue: 0.949))
                    .frame(width: 80, height: 80)
                    .overlay(OnymMark(size: 46, color: Color(red: 142/255, green: 142/255, blue: 147/255)))
                    .frame(maxWidth: .infinity)
                    .padding(.top, 16).padding(.bottom, 24)

                OnymSectionLabel(text: "NAME")
                OnymCard {
                    TextField("Identity name", text: $name)
                        .font(.system(size: 16.5))
                        .padding(.horizontal, 16).padding(.vertical, 12)
                        .onChange(of: name) { _, v in
                            if v.count > 30 { name = String(v.prefix(30)) }
                        }
                }
                OnymFootnote(text: "Defaults to \"Identity N\" if left blank.")

                OnymSectionLabel(text: "RESTORE FROM RECOVERY PHRASE")
                OnymCard {
                    TextEditor(text: $phrase)
                        .font(.system(size: 15))
                        .frame(minHeight: 96)
                        .padding(.horizontal, 12).padding(.vertical, 8)
                        .scrollContentBackground(.hidden)
                        .background(Color.clear)
                        .overlay(alignment: .topLeading) {
                            if phrase.isEmpty {
                                Text("Paste 12 or 24 words…")
                                    .font(.system(size: 15))
                                    .foregroundStyle(OnymTokens.text3)
                                    .padding(.horizontal, 16).padding(.vertical, 14)
                            }
                        }
                }
                OnymFootnote(text: "Leave blank to mint a fresh BIP-39 identity. Paste a 12 or 24-word phrase to restore.")
            }
        }
    }
}
