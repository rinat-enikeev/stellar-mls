import SwiftUI

struct BlossomServersView: View {
    @Environment(AppState.self) private var appState
    @State private var newBlossomURL = ""
    @State private var blossomError: String?
    @FocusState private var addFocused: Bool

    var body: some View {
        List {
            Section {
                ForEach(Array(appState.blossomServerURLs.enumerated()), id: \.element.absoluteString) { index, url in
                    serverRow(url: url, isPrimary: index == 0)
                }
                .onDelete { offsets in
                    appState.blossomServerURLs.remove(atOffsets: offsets)
                }
            } header: {
                if !appState.blossomServerURLs.isEmpty {
                    Text("My servers")
                }
            }

            Section {
                HStack {
                    TextField("https://blossom.example.com", text: $newBlossomURL)
                        .font(.callout)
                        .monospaced()
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .focused($addFocused)
                        .onSubmit(addBlossomServer)

                    if !newBlossomURL.trimmingCharacters(in: .whitespaces).isEmpty {
                        Button("Add", action: addBlossomServer)
                            .font(.callout.weight(.semibold))
                    }
                }

                if let error = blossomError {
                    Label(error, systemImage: "exclamationmark.triangle.fill")
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            } footer: {
                Text("Encrypted images are stored on Blossom servers. The server never sees plaintext.")
            }

            if appState.blossomServerURLs.isEmpty {
                Section {
                    emptyState
                        .listRowBackground(Color.clear)
                        .listRowSeparator(.hidden)
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Blossom Servers")
        .navigationBarTitleDisplayMode(.large)
        .toolbar {
            if !appState.blossomServerURLs.isEmpty {
                ToolbarItem(placement: .topBarTrailing) {
                    EditButton()
                }
            }
        }
    }

    private func serverRow(url: URL, isPrimary: Bool) -> some View {
        HStack(spacing: 12) {
            SettingsIconBox(systemImage: "photo.on.rectangle.angled", background: .purple)

            VStack(alignment: .leading, spacing: 2) {
                Text(url.absoluteString)
                    .font(.footnote)
                    .monospaced()
                    .lineLimit(1)
                    .truncationMode(.middle)

                if isPrimary {
                    Text("PRIMARY")
                        .font(.system(size: 10, weight: .bold))
                        .foregroundStyle(Color.accentColor)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .background(Color.accentColor.opacity(0.15), in: RoundedRectangle(cornerRadius: 4))
                }
            }

            Spacer(minLength: 8)
        }
        .padding(.vertical, 2)
    }

    private var emptyState: some View {
        VStack(spacing: 14) {
            ZStack {
                Circle()
                    .fill(Color(UIColor.tertiarySystemFill))
                    .frame(width: 64, height: 64)
                Image(systemName: "photo.on.rectangle.angled")
                    .font(.system(size: 28))
                    .foregroundStyle(.secondary)
            }
            Text("No Blossom servers")
                .font(.title3.weight(.semibold))
            Text("Add a server URL to store encrypted media.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)
            Button {
                addFocused = true
            } label: {
                Text("Add your first server")
                    .font(.body.weight(.semibold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 24)
                    .padding(.vertical, 12)
                    .background(Color.accentColor, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 40)
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
}
