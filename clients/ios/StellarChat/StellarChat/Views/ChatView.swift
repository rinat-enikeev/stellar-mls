import PhotosUI
import SwiftUI

struct ChatView: View {
    @Bindable var viewModel: ChatViewModel
    @Environment(AppState.self) private var appState
    @State private var showInvite = false
    @State private var showGroupInfo = false
    @State private var selectedPhotoItem: PhotosPickerItem?
    @State private var scrollTask: Task<Void, Never>?
    /// Tracks whether the user is scrolled near the bottom of the chat.
    /// When true, new messages auto-scroll into view. When false (user scrolled
    /// up to read history), we skip scrollTo — which otherwise forces the
    /// LazyVStack to render every item between the viewport and the target,
    /// freezing the main thread.
    @State private var isNearBottom = true

    var body: some View {
        VStack(spacing: 0) {
            if viewModel.messages.isEmpty {
                Spacer()
                ContentUnavailableView(
                    "No Messages Yet",
                    systemImage: "bubble.left.and.bubble.right",
                    description: Text("Send the first message to start the conversation")
                )
                Spacer()
            } else {
                ScrollViewReader { proxy in
                    ZStack(alignment: .bottomTrailing) {
                        ScrollView {
                            LazyVStack(spacing: 4) {
                                ForEach(Array(viewModel.messages.enumerated()), id: \.element.id) { index, message in
                                    // Date separator between messages on different days
                                    if shouldShowDateSeparator(at: index) {
                                        DateSeparator(date: message.timestamp)
                                    }

                                    // Unread message separator
                                    if message.id == viewModel.firstUnreadMessageID {
                                        UnreadSeparator()
                                    }

                                    let isGrouped = isGroupedWithPrevious(at: index)
                                    MessageBubble(message: message, isGrouped: isGrouped, onRetry: {
                                        viewModel.retryMessage(id: message.id)
                                    })
                                    .id(message.id)
                                    .transition(.opacity.combined(with: .move(edge: .bottom)))
                                }

                                // Invisible anchor — its onAppear/onDisappear tells us
                                // whether the user is scrolled to the bottom.
                                Color.clear
                                    .frame(height: 1)
                                    .id("bottom-anchor")
                                    .onAppear { isNearBottom = true }
                                    .onDisappear { isNearBottom = false }
                            }
                            .padding()
                        }

                        if !isNearBottom {
                            Button {
                                withAnimation(.easeOut(duration: 0.2)) {
                                    proxy.scrollTo("bottom-anchor", anchor: .bottom)
                                }
                            } label: {
                                Image(systemName: "chevron.down.circle.fill")
                                    .font(.title)
                                    .foregroundStyle(.white, .blue)
                                    .shadow(radius: 2)
                            }
                            .padding(.trailing, 16)
                            .padding(.bottom, 8)
                            .transition(.opacity)
                        }
                    }
                    .onChange(of: viewModel.messages.count) {
                        guard isNearBottom else { return }
                        scrollTask?.cancel()
                        scrollTask = Task {
                            try? await Task.sleep(for: .milliseconds(50))
                            guard !Task.isCancelled else { return }
                            withAnimation(.easeOut(duration: 0.2)) {
                                proxy.scrollTo("bottom-anchor", anchor: .bottom)
                            }
                        }
                    }
                }
            }

            Divider()

            // Image preview bar
            if let imageData = viewModel.selectedImageData,
               let uiImage = UIImage(data: imageData) {
                HStack {
                    Image(uiImage: uiImage)
                        .resizable()
                        .scaledToFill()
                        .frame(width: 60, height: 60)
                        .clipShape(RoundedRectangle(cornerRadius: 8))

                    Spacer()

                    if viewModel.isSendingImage {
                        ProgressView()
                    } else {
                        Button("Cancel", role: .cancel) {
                            viewModel.selectedImageData = nil
                        }
                        Button("Send") {
                            Task { await viewModel.sendImage() }
                        }
                        .buttonStyle(.borderedProminent)
                    }
                }
                .padding(.horizontal)
                .padding(.vertical, 8)
            }

            HStack(spacing: 12) {
                PhotosPicker(selection: $selectedPhotoItem, matching: .images) {
                    Image(systemName: "photo")
                        .font(.title3)
                        .foregroundStyle(viewModel.hasBlossomServers ? .primary : .secondary)
                }
                .disabled(!viewModel.hasBlossomServers)
                .onChange(of: selectedPhotoItem) { _, newItem in
                    guard let newItem else { return }
                    Task {
                        if let data = try? await newItem.loadTransferable(type: Data.self) {
                            viewModel.selectedImageData = data
                        }
                        selectedPhotoItem = nil
                    }
                }

                TextField("Message", text: $viewModel.inputText)
                    .textFieldStyle(.plain)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(.quaternary, in: RoundedRectangle(cornerRadius: 20))
                    .onSubmit {
                        UIImpactFeedbackGenerator(style: .light).impactOccurred()
                        Task { await viewModel.sendMessage() }
                    }

                Button {
                    UIImpactFeedbackGenerator(style: .light).impactOccurred()
                    Task { await viewModel.sendMessage() }
                } label: {
                    Image(systemName: "arrow.up.circle.fill")
                        .font(.title2)
                }
                .disabled(viewModel.inputText.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            .padding()
        }
        .navigationTitle(viewModel.group?.name ?? "Chat")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                HStack(spacing: 12) {
                    Button {
                        showGroupInfo = true
                    } label: {
                        Image(systemName: "person.3")
                    }
                    Button {
                        showInvite = true
                    } label: {
                        Image(systemName: "person.badge.plus")
                    }
                }
            }
        }
        .sheet(isPresented: $showInvite) {
            if let group = viewModel.group {
                InviteMemberView(group: group)
            }
        }
        .sheet(isPresented: $showGroupInfo) {
            if let group = viewModel.group {
                GroupInfoView(group: group)
            }
        }
        .onDisappear {
            viewModel.onDisappear()
        }
        .alert("Error", isPresented: Binding(
            get: { viewModel.errorMessage != nil },
            set: { if !$0 { viewModel.dismissError() } }
        )) {
            Button("OK") { viewModel.dismissError() }
        } message: {
            if let error = viewModel.errorMessage {
                Text(error)
            }
        }
    }

    // MARK: - Helpers

    /// Show a date separator if this is the first message or on a different day than the previous.
    private func shouldShowDateSeparator(at index: Int) -> Bool {
        guard index > 0 else { return true }
        let prev = viewModel.messages[index - 1].timestamp
        let curr = viewModel.messages[index].timestamp
        return !Calendar.current.isDate(prev, inSameDayAs: curr)
    }

    /// A message is "grouped" if the previous message is from the same sender within 2 minutes.
    private func isGroupedWithPrevious(at index: Int) -> Bool {
        guard index > 0 else { return false }
        let prev = viewModel.messages[index - 1]
        let curr = viewModel.messages[index]
        return prev.senderPubkey == curr.senderPubkey
            && curr.timestamp.timeIntervalSince(prev.timestamp) < 120
    }
}

// MARK: - Unread Separator

struct UnreadSeparator: View {
    var body: some View {
        HStack {
            Rectangle().fill(.blue.opacity(0.4)).frame(height: 1)
            Text("New Messages")
                .font(.caption2)
                .fontWeight(.semibold)
                .foregroundStyle(.blue)
            Rectangle().fill(.blue.opacity(0.4)).frame(height: 1)
        }
        .padding(.vertical, 6)
    }
}

// MARK: - Date Separator

struct DateSeparator: View {
    let date: Date

    private static let olderDateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "EEE, MMM d"
        return formatter
    }()

    var body: some View {
        Text(formattedDate)
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.vertical, 8)
            .frame(maxWidth: .infinity)
    }

    private var formattedDate: String {
        if Calendar.current.isDateInToday(date) {
            return "Today"
        } else if Calendar.current.isDateInYesterday(date) {
            return "Yesterday"
        } else {
            return Self.olderDateFormatter.string(from: date)
        }
    }
}

// MARK: - Message Bubble

struct MessageBubble: View {
    let message: ChatMessage
    var isGrouped: Bool = false
    var onRetry: (() -> Void)?

    private static let todayTimeFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "h:mm a"
        return formatter
    }()

    private static let olderTimeFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d, h:mm a"
        return formatter
    }()

    var body: some View {
        HStack(alignment: .bottom, spacing: 6) {
            if message.isMine { Spacer(minLength: 60) }

            // Avatar for received messages — only on first of a group
            if !message.isMine {
                if !isGrouped {
                    AvatarView(pubkey: message.senderPubkey)
                } else {
                    // Invisible spacer to keep alignment
                    Color.clear.frame(width: 28, height: 28)
                }
            }

            VStack(alignment: message.isMine ? .trailing : .leading, spacing: 2) {
                if !message.isMine && !isGrouped {
                    Text(String(message.senderPubkey.prefix(8)) + "...")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                HStack(alignment: .bottom, spacing: 4) {
                    if let media = message.mediaAttachment {
                        ImageBubbleContent(media: media, isMine: message.isMine)
                    } else {
                        Text(message.text)
                            .padding(.horizontal, 12)
                            .padding(.vertical, 8)
                            .background(
                                message.isMine ? Color.blue : Color(.systemGray5),
                                in: RoundedRectangle(cornerRadius: 16)
                            )
                            .foregroundStyle(message.isMine ? .white : .primary)
                    }

                    if message.isMine {
                        statusIcon
                    }
                }

                Text(formattedTimestamp)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

            if !message.isMine { Spacer(minLength: 60) }
        }
        .padding(.top, isGrouped ? 0 : 4)
    }

    @ViewBuilder
    private var statusIcon: some View {
        switch message.status {
        case .sending:
            Image(systemName: "clock")
                .font(.caption2)
                .foregroundStyle(.secondary)
        case .sent:
            Image(systemName: "checkmark")
                .font(.caption2)
                .foregroundStyle(.secondary)
        case .delivered:
            Image(systemName: "checkmark.circle.fill")
                .font(.caption2)
                .foregroundStyle(.blue)
        case .failed:
            Button {
                onRetry?()
            } label: {
                Label("Retry", systemImage: "exclamationmark.circle.fill")
                    .labelStyle(.iconOnly)
                    .font(.caption2)
                    .foregroundStyle(.red)
            }
        }
    }

    private var formattedTimestamp: String {
        if Calendar.current.isDateInToday(message.timestamp) {
            return Self.todayTimeFormatter.string(from: message.timestamp)
        } else {
            return Self.olderTimeFormatter.string(from: message.timestamp)
        }
    }
}

// MARK: - Image Bubble

struct ImageBubbleContent: View {
    let media: MediaAttachment
    let isMine: Bool
    @State private var fullImage: UIImage?
    @State private var isLoading = false
    @State private var cachedThumbnail: UIImage?

    var body: some View {
        Group {
            if let fullImage {
                Image(uiImage: fullImage)
                    .resizable()
                    .scaledToFit()
            } else if let thumbnail = cachedThumbnail {
                Image(uiImage: thumbnail)
                    .resizable()
                    .scaledToFit()
                    .overlay {
                        if isLoading {
                            ProgressView()
                                .tint(.white)
                        }
                    }
            } else {
                Rectangle()
                    .fill(Color(.systemGray4))
                    .overlay {
                        if isLoading {
                            ProgressView()
                        } else {
                            Image(systemName: "photo")
                                .foregroundStyle(.secondary)
                        }
                    }
            }
        }
        .frame(maxWidth: 220, maxHeight: 280)
        .clipShape(RoundedRectangle(cornerRadius: 16))
        .onAppear {
            if cachedThumbnail == nil, let encThumb = media.encryptedThumbnail,
               let plainData = try? MediaCrypto.decryptMedia(encThumb, key: media.fileKey) {
                cachedThumbnail = UIImage(data: plainData)
            }
            loadFullImage()
        }
        .onTapGesture {
            if fullImage == nil && !isLoading { loadFullImage() }
        }
    }

    private func loadFullImage() {
        // Check cache first
        if let cached = ImageCache.shared.image(for: media.blobHash) {
            fullImage = cached
            return
        }

        isLoading = true
        Task {
            do {
                let servers = media.blossomServers.compactMap(URL.init(string:))
                let encryptedBlob = try await BlossomClient.download(blobHash: media.blobHash, servers: servers)
                let plainData = try MediaCrypto.decryptMedia(encryptedBlob, key: media.fileKey)
                if let image = UIImage(data: plainData) {
                    ImageCache.shared.store(image, imageData: plainData, for: media.blobHash)
                    await MainActor.run {
                        fullImage = image
                        isLoading = false
                    }
                } else {
                    await MainActor.run { isLoading = false }
                }
            } catch {
                await MainActor.run { isLoading = false }
            }
        }
    }
}

// MARK: - Avatar

struct AvatarView: View {
    let pubkey: String

    private static let palette: [Color] = [
        .red, .orange, .yellow, .green, .teal, .blue, .indigo, .purple, .pink, .brown
    ]

    var body: some View {
        ZStack {
            Circle()
                .fill(avatarColor)
                .frame(width: 28, height: 28)
            Text(initials)
                .font(.system(size: 11, weight: .bold))
                .foregroundStyle(.white)
        }
    }

    private var initials: String {
        String(pubkey.prefix(2)).uppercased()
    }

    private var avatarColor: Color {
        // Deterministic color from first byte of pubkey hex
        let index = Int(pubkey.prefix(2), radix: 16) ?? 0
        return Self.palette[index % Self.palette.count]
    }
}
