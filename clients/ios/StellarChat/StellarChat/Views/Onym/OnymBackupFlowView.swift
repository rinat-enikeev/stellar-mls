import SwiftUI

struct OnymBackupFlowView: View {
    @Bindable var model: OnymSettingsModel
    let identityId: String
    @Environment(\.dismiss) private var dismiss
    @State private var step: Step = .intro

    enum Step { case intro, reveal, verify, done }

    private var identity: OnymIdentity {
        model.identities.first(where: { $0.id == identityId }) ?? model.identities[0]
    }

    var body: some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(
                    title: step == .done ? "Backup verified" : "Recovery phrase",
                    subtitle: step == .done ? nil : "For \(identity.name)",
                    onBack: step == .done ? nil : { dismiss() }
                )
                switch step {
                case .intro:  intro
                case .reveal: reveal
                case .verify: verify
                case .done:   done
                }
            }
        }
    }

    // MARK: Intro

    private var intro: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(spacing: 16) {
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .fill(LinearGradient(colors: [Color(red: 0.78, green: 0.7, blue: 1.0), OnymTokens.purple],
                                          startPoint: .topLeading, endPoint: .bottomTrailing))
                    .frame(width: 72, height: 72)
                    .overlay(OnymMark(size: 42, color: .white))
                Text("Your identity, in 12 words")
                    .font(.system(size: 22, weight: .bold))
                    .tracking(-0.26)
                    .foregroundStyle(OnymTokens.text)
                Text("Write them down. Keep them offline. This phrase restores \(identity.name)'s Nostr, Stellar, and BLS keys on any device.")
                    .font(.system(size: 14))
                    .foregroundStyle(OnymTokens.text2)
                    .lineSpacing(3)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 320)
            }
            .padding(.horizontal, 20).padding(.vertical, 28)
            .frame(maxWidth: .infinity)
            .background(OnymTokens.card, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            .padding(.horizontal, 16)

            OnymSectionLabel(text: "BEFORE YOU START")
            OnymCard {
                OnymRow(title: "Never share or photograph", hasChevron: false) {
                    OnymTile(bg: OnymTokens.Tile.orange) {
                        Text("!").font(.system(size: 14, weight: .bold)).foregroundStyle(.white)
                    }
                }
                OnymRow(title: "Store offline (paper or metal)", hasChevron: false) {
                    OnymSymbolTile(symbol: "shield.fill", bg: OnymTokens.Tile.green)
                }
                OnymRow(title: "Anyone with it can read your chats", hasChevron: false, last: true) {
                    OnymSymbolTile(symbol: "lock.fill", bg: OnymTokens.Tile.gray)
                }
            }

            OnymPrimaryButton(action: { withAnimation { step = .reveal } }) {
                HStack(spacing: 8) {
                    Image(systemName: "faceid")
                    Text("Continue with Face ID")
                }
            }
            .padding(.horizontal, 16)
            .padding(.top, 24)
        }
    }

    // MARK: Reveal

    private var reveal: some View {
        VStack(alignment: .leading, spacing: 0) {
            OnymStepIndicator(step: 0)
            Text("Write down these 12 words in order. You'll confirm three of them on the next screen.")
                .font(.system(size: 14))
                .foregroundStyle(OnymTokens.text2)
                .lineSpacing(3)
                .padding(.horizontal, 20).padding(.bottom, 16)

            wordsCard

            HStack(spacing: 8) {
                Button { UIPasteboard.general.string = OnymCatalog.recoveryWords.joined(separator: " ") } label: {
                    HStack(spacing: 8) {
                        Image(systemName: "doc.on.doc")
                        Text("Copy").font(.system(size: 15, weight: .semibold))
                    }
                    .foregroundStyle(revealed ? OnymTokens.blue : OnymTokens.text3)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .background(revealed ? Color(red: 0.863, green: 0.918, blue: 0.988) : Color(red: 0.914, green: 0.914, blue: 0.922),
                                in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                }
                .disabled(!revealed)
            }
            .padding(.horizontal, 16)
            .padding(.top, 14)

            OnymPrimaryButton(disabled: !revealed,
                              action: { withAnimation { step = .verify } }) {
                Text("I've written it down")
            }
            .padding(.horizontal, 16)
            .padding(.top, 12)

            OnymFootnote(text: "The phrase is generated on-device and never sent off the device.")
        }
    }

    @State private var revealed = false

    private var wordsCard: some View {
        ZStack {
            VStack(spacing: 8) {
                ForEach(0..<6, id: \.self) { row in
                    HStack(spacing: 8) {
                        wordCell(row * 2)
                        wordCell(row * 2 + 1)
                    }
                }
            }
            .padding(14)
            .background(OnymTokens.card, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            .blur(radius: revealed ? 0 : 8)
            .padding(.horizontal, 16)

            if !revealed {
                Button { withAnimation { revealed = true } } label: {
                    VStack(spacing: 8) {
                        ZStack {
                            Circle().fill(.white.opacity(0.85))
                                .frame(width: 44, height: 44)
                                .shadow(color: .black.opacity(0.08), radius: 4, y: 2)
                            Image(systemName: "eye.slash.fill")
                                .font(.system(size: 18))
                                .foregroundStyle(OnymTokens.text)
                        }
                        Text("Tap to reveal")
                            .font(.system(size: 14, weight: .semibold))
                    }
                }
                .buttonStyle(.plain)
            }
        }
    }

    private func wordCell(_ idx: Int) -> some View {
        HStack(spacing: 8) {
            Text("\(idx + 1)")
                .font(.system(size: 11, design: .monospaced))
                .foregroundStyle(OnymTokens.text3)
                .frame(width: 16, alignment: .leading)
            Text(OnymCatalog.recoveryWords[idx])
                .font(.system(size: 16, weight: .medium))
                .foregroundStyle(OnymTokens.text)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 12).padding(.vertical, 10)
        .background(OnymTokens.card2, in: RoundedRectangle(cornerRadius: 10))
    }

    // MARK: Verify

    @State private var picked: String?
    @State private var error = false

    private var verify: some View {
        let wordIndex = 3
        let correct = OnymCatalog.recoveryWords[wordIndex]
        let options: [String] = {
            let others = OnymCatalog.recoveryWords.filter { $0 != correct }.shuffled().prefix(2)
            return Array(others) + [correct]
        }().shuffled()

        return VStack(alignment: .leading, spacing: 0) {
            OnymStepIndicator(step: 1)

            VStack(spacing: 4) {
                Text("Select word number")
                    .font(.system(size: 13))
                    .foregroundStyle(OnymTokens.text2)
                Text("\(wordIndex + 1)")
                    .font(.system(size: 56, weight: .bold))
                    .tracking(-1.12)
                    .foregroundStyle(OnymTokens.blue)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 24)
            .background(OnymTokens.card, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            .padding(.horizontal, 16)

            VStack(spacing: 8) {
                ForEach(options, id: \.self) { w in
                    Button {
                        choose(w, correct: correct)
                    } label: {
                        Text(w)
                            .font(.system(size: 17, weight: .medium))
                            .foregroundStyle(picked == w && error ? OnymTokens.red : OnymTokens.text)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, 16).padding(.vertical, 14)
                            .background(OnymTokens.card, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous)
                                .stroke(picked == w && error ? OnymTokens.red : .clear, lineWidth: 1.5))
                    }
                    .disabled(picked != nil)
                }
            }
            .padding(.horizontal, 16).padding(.top, 12)

            if error {
                Text("Not the right word. Check your phrase and try again.")
                    .font(.system(size: 13))
                    .foregroundStyle(OnymTokens.red)
                    .frame(maxWidth: .infinity)
                    .padding(.top, 12)
            }
        }
    }

    private func choose(_ w: String, correct: String) {
        picked = w
        if w == correct {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { withAnimation { step = .done } }
        } else {
            error = true
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.4) { picked = nil; error = false }
        }
    }

    // MARK: Done

    private var done: some View {
        VStack(spacing: 24) {
            Circle().fill(OnymTokens.green)
                .frame(width: 88, height: 88)
                .overlay(Circle().stroke(OnymTokens.green.opacity(0.18), lineWidth: 12))
                .overlay(Image(systemName: "checkmark")
                    .font(.system(size: 36, weight: .bold))
                    .foregroundStyle(.white))
                .padding(.top, 40)
            VStack(spacing: 8) {
                Text("Backup verified")
                    .font(.system(size: 26, weight: .bold))
                    .foregroundStyle(OnymTokens.text)
                Text("\(identity.name)'s recovery phrase is confirmed. Store it somewhere safe — you'll only need it if you lose this device.")
                    .font(.system(size: 14))
                    .foregroundStyle(OnymTokens.text2)
                    .multilineTextAlignment(.center)
                    .lineSpacing(3)
                    .frame(maxWidth: 320)
            }

            OnymPrimaryButton(action: {
                model.markBackedUp(identityId)
                dismiss()
            }) { Text("Done") }
            .padding(.horizontal, 16).padding(.top, 12)

            Spacer(minLength: 24)
            Text("Backed up \(OnymSettingsModel.todayString()) · BIP-39 English")
                .font(.system(size: 12))
                .foregroundStyle(OnymTokens.text3)
        }
        .frame(maxWidth: .infinity)
        .padding(.bottom, 24)
    }
}
