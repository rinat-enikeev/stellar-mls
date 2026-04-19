import SwiftUI

struct TurnServersView: View {
    @Environment(AppState.self) private var appState
    @State private var newTurnURL = ""
    @State private var turnError: String?

    var body: some View {
        @Bindable var appState = appState

        return List {
            Section {
                HStack(spacing: 12) {
                    SettingsIconBox(systemImage: "globe", background: Color(red: 0.35, green: 0.78, blue: 0.98))

                    VStack(alignment: .leading, spacing: 2) {
                        Text("EU TURN servers")
                            .font(.body)
                        Text("Built-in")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    Spacer()

                    HStack(spacing: 5) {
                        Circle().fill(Color.green).frame(width: 6, height: 6)
                        Text("Active")
                            .font(.callout)
                            .foregroundStyle(.green)
                    }
                }
            } header: {
                Text("Built-in")
            } footer: {
                Text("Always included for call relay through restrictive networks.")
            }

            Section("Custom") {
                Toggle("Enable custom TURN", isOn: $appState.turnEnabled)
                    .onChange(of: appState.turnEnabled) { appState.syncTurnConfig() }
            }

            if appState.turnEnabled {
                Section {
                    ForEach(appState.turnURLs, id: \.self) { url in
                        HStack(spacing: 10) {
                            Image(systemName: "network")
                                .foregroundStyle(.blue)
                            Text(url)
                                .font(.footnote)
                                .monospaced()
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }
                    }
                    .onDelete { offsets in
                        appState.turnURLs.remove(atOffsets: offsets)
                        appState.syncTurnConfig()
                    }

                    HStack {
                        TextField("turn:host:443?transport=tcp", text: $newTurnURL)
                            .font(.callout)
                            .monospaced()
                            .autocorrectionDisabled()
                            .textInputAutocapitalization(.never)
                            .onSubmit(addTurnURL)

                        if !newTurnURL.trimmingCharacters(in: .whitespaces).isEmpty {
                            Button("Add", action: addTurnURL)
                                .font(.callout.weight(.semibold))
                        }
                    }

                    if let error = turnError {
                        Label(error, systemImage: "exclamationmark.triangle.fill")
                            .font(.caption)
                            .foregroundStyle(.red)
                    }
                } header: {
                    Text("Server URL")
                }

                Section {
                    TextField("Username", text: $appState.turnUsername)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .onChange(of: appState.turnUsername) { appState.syncTurnConfig() }

                    SecureField("Password", text: $appState.turnPassword)
                        .onChange(of: appState.turnPassword) { appState.syncTurnConfig() }
                } header: {
                    Text("Credentials")
                } footer: {
                    Text("Optional custom TURN servers for restrictive networks. Use turn: or turns: URL schemes.")
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("TURN Servers")
        .navigationBarTitleDisplayMode(.large)
    }

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
}
