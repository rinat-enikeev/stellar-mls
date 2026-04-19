import SwiftUI

struct StellarContractView: View {
    @Environment(AppState.self) private var appState
    @State private var contractEndpoint = ""
    @State private var contractIDInput = ""
    @State private var relayerURLInput = ""
    @State private var relayerAuthTokenInput = ""
    @State private var contractSaveStatus: String?

    var body: some View {
        List {
            Section {
                statusBanner
                    .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
                    .listRowSeparator(.hidden)
                    .listRowBackground(Color.clear)
            }

            Section {
                labeledField(label: "ENDPOINT URL", text: $contractEndpoint, placeholder: "https://soroban-testnet.stellar.org")
                labeledField(label: "CONTRACT ID", text: $contractIDInput, placeholder: "C…")
            } header: {
                Text("Endpoint")
            }

            Section {
                labeledField(label: "RELAYER URL", text: $relayerURLInput, placeholder: "https://relay.example.com")
                labeledField(label: "RELAYER AUTH TOKEN", text: $relayerAuthTokenInput, placeholder: "Optional token", secure: true)
            } header: {
                Text("Relayer (optional)")
            } footer: {
                Text("If a relayer URL is set, on-chain requests are sent through the relayer instead of directly to Soroban RPC.")
            }

            Section {
                Button(action: saveContractConfig) {
                    Text("Save Contract Configuration")
                        .frame(maxWidth: .infinity)
                        .font(.body.weight(.semibold))
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(
                    contractEndpoint.trimmingCharacters(in: .whitespaces).isEmpty ||
                    contractIDInput.trimmingCharacters(in: .whitespaces).isEmpty
                )
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets(top: 4, leading: 16, bottom: 4, trailing: 16))

                if let status = contractSaveStatus {
                    Text(status)
                        .font(.caption)
                        .foregroundStyle(status.hasPrefix("Error") ? .red : .green)
                }
            }

            Section {
                LabeledContent("Status") {
                    Text(appState.isContractConfigured ? "Connected" : "Not configured")
                        .foregroundStyle(appState.isContractConfigured ? .green : .secondary)
                }
                LabeledContent("Network") { Text("Testnet") }
            } header: {
                Text("Diagnostics")
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Stellar Contract")
        .navigationBarTitleDisplayMode(.large)
        .onAppear {
            contractEndpoint = appState.contractEndpoint
            contractIDInput = appState.contractID
            relayerURLInput = appState.relayerURL
            relayerAuthTokenInput = appState.relayerAuthToken
        }
    }

    // MARK: - Banner

    private var statusBanner: some View {
        HStack(spacing: 12) {
            ZStack {
                Circle()
                    .fill(appState.isContractConfigured ? Color.green : Color.gray)
                    .frame(width: 38, height: 38)
                Image(systemName: appState.isContractConfigured ? "checkmark" : "questionmark")
                    .font(.system(size: 18, weight: .bold))
                    .foregroundStyle(.white)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(appState.isContractConfigured ? "Connected" : "Not configured")
                    .font(.subheadline.weight(.semibold))
                Text(appState.relayerURL.isEmpty ? "Direct RPC" : "Via relayer")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .background(Color(UIColor.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }

    // MARK: - Field row

    @ViewBuilder
    private func labeledField(label: String, text: Binding<String>, placeholder: String, secure: Bool = false) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label)
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
                .tracking(0.2)
            if secure {
                SecureField(placeholder, text: text)
                    .font(.callout)
                    .monospaced()
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            } else {
                TextField(placeholder, text: text)
                    .font(.callout)
                    .monospaced()
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            }
        }
        .padding(.vertical, 4)
    }

    // MARK: - Actions

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
}
