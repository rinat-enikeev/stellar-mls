import SwiftMLS
import SwiftUI

struct SettingsView: View {
    @Environment(AppState.self) private var appState
    @State private var attestationStatus: String?
    @State private var newRelayURL = ""
    @State private var relayError: String?
    @State private var contractEndpoint = ""
    @State private var contractIDInput = ""
    @State private var relayerURLInput = ""
    @State private var relayerAuthTokenInput = ""
    @State private var contractSaveStatus: String?
    @State private var showAdvanced = false
    @State private var newBlossomURL = ""
    @State private var blossomError: String?
    @State private var newTurnURL = ""
    @State private var turnError: String?

    var body: some View {
        Form {
            // Invite Key (X25519 inbox key) — shown prominently for scanning
            Section {
                VStack(spacing: 12) {
                    QRCodeView(appState.keyManager.keyAgreementPublicKeyHex, size: 180)

                    Text(appState.keyManager.keyAgreementPublicKeyHex)
                        .font(.caption2)
                        .monospaced()
                        .multilineTextAlignment(.center)
                        .lineLimit(2)
                        .foregroundStyle(.secondary)

                    Button {
                        UIPasteboard.general.string = appState.keyManager.keyAgreementPublicKeyHex
                    } label: {
                        Label("Copy Invite Key", systemImage: "doc.on.doc")
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 8)
            } header: {
                Text("Your Invite Key")
            } footer: {
                Text("Share this QR code so others can find you.")
            }

            relayManagementSection

            blossomServerSection

            contractSection

            Section("Default Group Capacity") {
                Picker("Capacity", selection: Bindable(appState).defaultGroupTier) {
                    Text("32 members").tag(SEPTier.small)
                    Text("256 members").tag(SEPTier.medium)
                    Text("2,048 members").tag(SEPTier.large)
                }
                .pickerStyle(.segmented)

                Text("New groups will use this tier. Larger tiers support more members but use bigger ZK circuits.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

            turnServerSection

            Section {
                DisclosureGroup("Advanced", isExpanded: $showAdvanced) {
                    // Nostr Identity
                    LabeledContent("Nostr Public Key (secp256k1)") {
                        Text(appState.keyManager.publicKeyHex.prefix(16) + "...")
                            .font(.caption)
                            .monospaced()
                    }

                    Button("Copy Nostr Public Key") {
                        UIPasteboard.general.string = appState.keyManager.publicKeyHex
                    }

                    Text("Your Nostr identity key used to sign events on relays.")
                        .font(.caption2)
                        .foregroundStyle(.secondary)

                    Divider()

                    // Stellar Identity
                    LabeledContent("Stellar Account ID (Ed25519)") {
                        Text(appState.keyManager.stellarAccountID.prefix(16) + "...")
                            .font(.caption)
                            .monospaced()
                    }

                    Button("Copy Stellar Account ID") {
                        UIPasteboard.general.string = appState.keyManager.stellarAccountID
                    }

                    Text("Derived from Nostr key via HKDF-SHA256. StrKey encoded (G...).")
                        .font(.caption2)
                        .foregroundStyle(.secondary)

                    Divider()

                    // BLS Keys
                    if let blsHex = try? appState.keyManager.blsPublicKey
                        .map({ String(format: "%02x", $0) }).joined()
                    {
                        LabeledContent("BLS Public Key (BLS12-381)") {
                            Text(blsHex.prefix(16) + "...")
                                .font(.caption)
                                .monospaced()
                        }
                    }

                    Button("Create Key Attestation") {
                        createAttestation()
                    }

                    if let status = attestationStatus {
                        Text(status)
                            .font(.caption)
                            .foregroundStyle(status.hasPrefix("Error") ? .red : .green)
                    }

                    Text("Ed25519 signature binding BLS group key to Stellar identity per SEP-XXXX §1.1.")
                        .font(.caption2)
                        .foregroundStyle(.secondary)

                    Divider()

                    // Protocol Details
                    LabeledContent("Invitation Kind") { Text("34113") }
                    LabeledContent("Message Kind") { Text("24114") }
                    LabeledContent("Encryption") { Text("AES-256-GCM") }
                    LabeledContent("Invitation Encryption") { Text("X25519 ECDH + AES-256-GCM") }
                    LabeledContent("Key Derivation") { Text("HKDF-SHA256") }
                    LabeledContent("Topic Derivation") { Text("SHA256(secret)") }
                    LabeledContent("Nostr Signing") { Text("secp256k1 Schnorr") }
                    LabeledContent("Stellar Signing") { Text("Ed25519") }
                    LabeledContent("ZK Backend") { Text("Groth16 / BLS12-381") }
                    LabeledContent("Commitment") { Text("Poseidon-only") }
                }
            }

            Section("About") {
                LabeledContent("Version") { Text("1.0.3") }
                Text("Messages are encrypted end-to-end. Relays see only opaque ciphertext and hidden topic tags. Group IDs never appear in cleartext on the wire.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .navigationTitle("Settings")
    }

    // MARK: - Relay Management

    @ViewBuilder
    private var relayManagementSection: some View {
        Section("Relays") {
            ForEach(appState.relayURLs, id: \.absoluteString) { url in
                HStack {
                    Image(systemName: "antenna.radiowaves.left.and.right")
                        .foregroundStyle(.green)
                    Text(url.absoluteString)
                        .font(.caption)
                        .monospaced()
                }
            }
            .onDelete { offsets in
                appState.removeRelay(at: offsets)
            }
            .onMove { source, destination in
                appState.moveRelay(from: source, to: destination)
            }

            HStack {
                TextField("wss://relay.example.com", text: $newRelayURL)
                    .font(.caption)
                    .monospaced()
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)

                Button("Add") {
                    addRelay()
                }
                .disabled(newRelayURL.trimmingCharacters(in: .whitespaces).isEmpty)
            }

            if let error = relayError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            }

            Text("Drag to reorder. Swipe to remove. First relay has highest priority.")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - Blossom Servers

    @ViewBuilder
    private var blossomServerSection: some View {
        Section("Blossom Servers") {
            ForEach(appState.blossomServerURLs, id: \.absoluteString) { url in
                HStack {
                    Image(systemName: "photo.on.rectangle.angled")
                        .foregroundStyle(.purple)
                    Text(url.absoluteString)
                        .font(.caption)
                        .monospaced()
                }
            }
            .onDelete { offsets in
                appState.blossomServerURLs.remove(atOffsets: offsets)
            }

            HStack {
                TextField("https://blossom.example.com", text: $newBlossomURL)
                    .font(.caption)
                    .monospaced()
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)

                Button("Add") {
                    addBlossomServer()
                }
                .disabled(newBlossomURL.trimmingCharacters(in: .whitespaces).isEmpty)
            }

            if let error = blossomError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            }

            Text("Encrypted images are stored on Blossom servers. The server never sees plaintext.")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - TURN Servers

    @ViewBuilder
    private var turnServerSection: some View {
        Section("TURN Servers") {
            Toggle("Enable custom TURN", isOn: Bindable(appState).turnEnabled)
                .onChange(of: appState.turnEnabled) { appState.syncTurnConfig() }

            ForEach(appState.turnURLs, id: \.self) { url in
                HStack {
                    Image(systemName: "network")
                        .foregroundStyle(.blue)
                    Text(url)
                        .font(.caption)
                        .monospaced()
                }
            }
            .onDelete { offsets in
                appState.turnURLs.remove(atOffsets: offsets)
                appState.syncTurnConfig()
            }

            HStack {
                TextField("turn:host:443?transport=tcp", text: $newTurnURL)
                    .font(.caption)
                    .monospaced()
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)

                Button("Add") {
                    addTurnURL()
                }
                .disabled(newTurnURL.trimmingCharacters(in: .whitespaces).isEmpty)
            }

            if let error = turnError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            }

            TextField("Username", text: Bindable(appState).turnUsername)
                .font(.caption)
                .monospaced()
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
                .onChange(of: appState.turnUsername) { appState.syncTurnConfig() }

            SecureField("Password", text: Bindable(appState).turnPassword)
                .font(.caption)
                .monospaced()
                .onChange(of: appState.turnPassword) { appState.syncTurnConfig() }

            Text("Optional custom TURN servers for restrictive networks. Built-in EU TURN servers are always included. Use turn: or turns: URL schemes.")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - Stellar Contract

    @ViewBuilder
    private var contractSection: some View {
        Section("Stellar Contract") {
            HStack {
                Image(systemName: appState.isContractConfigured
                    ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(appState.isContractConfigured ? .green : .secondary)
                Text(appState.isContractConfigured ? "Connected" : "Not configured")
                    .font(.caption)
            }

            TextField("Endpoint URL", text: $contractEndpoint)
                .font(.caption)
                .monospaced()
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)

            TextField("Contract ID", text: $contractIDInput)
                .font(.caption)
                .monospaced()
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)

            TextField("Relayer URL (optional)", text: $relayerURLInput)
                .font(.caption)
                .monospaced()
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)

            TextField("Relayer Auth Token (optional)", text: $relayerAuthTokenInput)
                .font(.caption)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()

            Button("Save Contract Configuration") {
                saveContractConfig()
            }
            .disabled(
                contractEndpoint.trimmingCharacters(in: .whitespaces).isEmpty
                    || contractIDInput.trimmingCharacters(in: .whitespaces).isEmpty
            )

            if let status = contractSaveStatus {
                Text(status)
                    .font(.caption)
                    .foregroundStyle(status.hasPrefix("Error") ? .red : .green)
            }

            Text("Enter the Soroban RPC endpoint and deployed SEP-XXXX contract address. If a relayer URL is set, on-chain requests are sent through the relayer instead of directly to Soroban RPC.")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
        .onAppear {
            contractEndpoint = appState.contractEndpoint
            contractIDInput = appState.contractID
            relayerURLInput = appState.relayerURL
            relayerAuthTokenInput = appState.relayerAuthToken
        }
    }

    // MARK: - Actions

    private func addTurnURL() {
        turnError = nil
        let urlString = newTurnURL.trimmingCharacters(in: .whitespacesAndNewlines)
        guard urlString.hasPrefix("turn:") || urlString.hasPrefix("turns:"),
              !appState.turnURLs.contains(urlString) else {
            turnError = "Invalid URL. Must start with turn: or turns: and not already added."
            return
        }
        appState.turnURLs.append(urlString)
        appState.syncTurnConfig()
        newTurnURL = ""
    }

    private func addBlossomServer() {
        blossomError = nil
        let urlString = newBlossomURL.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URL(string: urlString),
              url.scheme == "https" || url.scheme == "http",
              !appState.blossomServerURLs.contains(url) else {
            blossomError = "Invalid URL. Must be http(s):// and not already added."
            return
        }
        appState.blossomServerURLs.append(url)
        newBlossomURL = ""
    }

    private func createAttestation() {
        do {
            let attestation = try appState.keyManager.createAttestation()
            let valid = KeyManager.verifyAttestation(attestation)
            let blsHex = attestation.blsPubkey.prefix(8)
                .map { String(format: "%02x", $0) }.joined()
            let ed25519Hex = attestation.ed25519Pubkey.prefix(8)
                .map { String(format: "%02x", $0) }.joined()
            attestationStatus = "Bound BLS \(blsHex)... to Stellar \(ed25519Hex)... (\(valid ? "verified" : "INVALID"))"
        } catch {
            attestationStatus = "Error: \(error.localizedDescription)"
        }
    }

    private func saveContractConfig() {
        appState.contractEndpoint = contractEndpoint
        appState.contractID = contractIDInput
        appState.relayerURL = relayerURLInput
        appState.relayerAuthToken = relayerAuthTokenInput
        appState.configureContract()
        contractSaveStatus = appState.isContractConfigured
            ? "Configured successfully"
            : "Error: Invalid endpoint URL or contract ID"
    }

    private func addRelay() {
        relayError = nil
        let urlString = newRelayURL.trimmingCharacters(in: .whitespacesAndNewlines)
        if appState.addRelay(urlString: urlString) {
            newRelayURL = ""
        } else {
            relayError = "Invalid URL. Must be ws:// or wss:// and not already added."
        }
    }
}
