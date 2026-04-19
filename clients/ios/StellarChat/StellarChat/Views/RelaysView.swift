import SwiftUI

struct RelaysView: View {
    @Environment(AppState.self) private var appState
    @State private var newRelayURL = ""
    @State private var relayError: String?
    @FocusState private var addFocused: Bool

    var body: some View {
        List {
            if !appState.relayURLs.isEmpty {
                summaryCard
                    .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
                    .listRowSeparator(.hidden)
                    .listRowBackground(Color.clear)
            }

            Section {
                ForEach(Array(appState.relayURLs.enumerated()), id: \.element.absoluteString) { index, url in
                    relayRow(url: url, isPrimary: index == 0)
                }
                .onDelete { offsets in
                    appState.removeRelay(at: offsets)
                }
                .onMove { source, destination in
                    appState.moveRelay(from: source, to: destination)
                }
            } header: {
                if !appState.relayURLs.isEmpty {
                    Text("My relays")
                }
            }

            Section {
                HStack {
                    TextField("wss://relay.example.com", text: $newRelayURL)
                        .font(.callout)
                        .monospaced()
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .focused($addFocused)
                        .onSubmit(addRelay)

                    if !newRelayURL.trimmingCharacters(in: .whitespaces).isEmpty {
                        Button("Add", action: addRelay)
                            .font(.callout.weight(.semibold))
                    }
                }

                if let error = relayError {
                    Label(error, systemImage: "exclamationmark.triangle.fill")
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            } footer: {
                Text("Drag to reorder. Swipe to remove. Your primary relay is tried first.")
            }

            if appState.relayURLs.isEmpty {
                Section {
                    emptyState
                        .listRowBackground(Color.clear)
                        .listRowSeparator(.hidden)
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Relays")
        .navigationBarTitleDisplayMode(.large)
        .toolbar {
            if !appState.relayURLs.isEmpty {
                ToolbarItem(placement: .topBarTrailing) {
                    EditButton()
                }
            }
        }
    }

    // MARK: - Summary

    private var summaryCard: some View {
        HStack(spacing: 12) {
            ZStack {
                Circle()
                    .fill(
                        LinearGradient(
                            colors: [.green, .blue],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        )
                    )
                    .frame(width: 38, height: 38)
                Image(systemName: "antenna.radiowaves.left.and.right")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(.white)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text("\(appState.relayURLs.count) relay\(appState.relayURLs.count == 1 ? "" : "s") configured")
                    .font(.subheadline.weight(.semibold))
                Text("First relay is tried first")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            Text("Healthy")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.green)
                .padding(.horizontal, 10)
                .padding(.vertical, 4)
                .background(Color.green.opacity(0.15), in: Capsule())
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .background(Color(UIColor.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }

    private func relayRow(url: URL, isPrimary: Bool) -> some View {
        HStack(spacing: 10) {
            Circle()
                .fill(Color.green)
                .frame(width: 8, height: 8)
                .shadow(color: .green.opacity(0.5), radius: 3)

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

    // MARK: - Empty state

    private var emptyState: some View {
        VStack(spacing: 14) {
            ZStack {
                Circle()
                    .fill(Color(UIColor.tertiarySystemFill))
                    .frame(width: 64, height: 64)
                Image(systemName: "antenna.radiowaves.left.and.right")
                    .font(.system(size: 28, weight: .regular))
                    .foregroundStyle(.secondary)
            }

            Text("No relays yet")
                .font(.title3.weight(.semibold))

            Text("Add a relay to start sending and receiving messages.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)

            Button {
                addFocused = true
            } label: {
                Text("Add your first relay")
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

    // MARK: - Actions

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
