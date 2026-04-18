import AVKit
import PhotosUI
import SwiftMLS
import SwiftUI
import UniformTypeIdentifiers

struct ChatView: View {
    @Bindable var viewModel: ChatViewModel
    @Environment(AppState.self) private var appState
    @State private var showGroupInfo = false
    @State private var selectedPhotoItem: PhotosPickerItem?
    @State private var scrollTask: Task<Void, Never>?
    /// Tracks whether the user is scrolled near the bottom of the chat.
    /// When true, new messages auto-scroll into view. When false (user scrolled
    /// up to read history), we skip scrollTo — which otherwise forces the
    /// LazyVStack to render every item between the viewport and the target,
    /// freezing the main thread.
    @State private var isNearBottom = true
    @State private var videoThumbnail: UIImage?
    @State private var videoDuration: Int?
    @State private var showCallScreen = false
    @State private var showForkSheet = false
    @State private var pushBannerDismissed = false
    @State private var governanceBannerDismissed: [String: Bool] = [:]
    @AppStorage("hasSeenFirstGroupWelcome") private var hasSeenFirstGroupWelcome = false

    var body: some View {
        VStack(spacing: 0) {
            // Governance type banner — highlights the rules for this group.
            // Anarchy is surfaced as a warning (any member can kick/invite);
            // 1v1 / Democracy / Oligarchy get an informational notice.
            if let group = viewModel.group,
               governanceBannerDismissed[group.id] != true {
                GovernanceBanner(
                    groupType: group.groupType,
                    onDismiss: { governanceBannerDismissed[group.id] = true }
                )
            }

            // Welcome banner for first group
            if !hasSeenFirstGroupWelcome, appState.groups.count == 1 {
                HStack(spacing: 8) {
                    Image(systemName: "hand.wave")
                        .foregroundStyle(.primary)
                    if let group = viewModel.group, group.members.count <= 1 {
                        Text("Your group is ready. Messages are end-to-end encrypted — only members can read them.")
                            .font(.caption)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    } else {
                        Text("You're in. Your identity is protected — members see a key, not your name.")
                            .font(.caption)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    Button {
                        hasSeenFirstGroupWelcome = true
                    } label: {
                        Image(systemName: "xmark")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal)
                .padding(.vertical, 8)
                .background(Color.accentColor.opacity(0.1))
            }

            // Push notification banner
            if let group = viewModel.group,
               !group.pushNotificationsEnabled,
               !pushBannerDismissed {
                HStack(spacing: 8) {
                    Image(systemName: "bell.badge")
                        .foregroundStyle(.primary)
                    Text("Enable push notifications for this chat?")
                        .font(.caption)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Button("Enable") {
                        appState.setPushNotifications(enabled: true, forGroup: group.id)
                        pushBannerDismissed = true
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                    Button {
                        pushBannerDismissed = true
                    } label: {
                        Image(systemName: "xmark")
                            .font(.caption2)
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal)
                .padding(.vertical, 8)
                .background(Color.accentColor.opacity(0.1))
            }

            // Epoch pinning banner
            if let group = viewModel.group, let pinned = group.pinnedEpoch,
               let snapshot = appState.epochSnapshots[group.id]?[pinned] {
                let isPrivateBranch = snapshot.groupSecret != group.groupSecret
                VStack(spacing: 4) {
                    HStack {
                        Image(systemName: isPrivateBranch ? "lock.shield" : "arrow.triangle.branch")
                        Text("Epoch \(pinned)")
                            .fontWeight(.medium)
                        Text("\(snapshot.members.count) members")
                            .foregroundStyle(.secondary)
                        Spacer()
                        Button("Unpin") {
                            appState.unpinEpoch(groupID: group.id)
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                    }
                    Text(isPrivateBranch
                         ? "Private branch — only members from this epoch can read and write here"
                         : "Filtered view — messages are still visible to all group members")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .font(.caption)
                .padding(.horizontal)
                .padding(.vertical, 6)
                .background(isPrivateBranch ? Color.green.opacity(0.12) : Color.orange.opacity(0.12))
            }

            if viewModel.messages.isEmpty {
                Spacer()
                VStack(spacing: 16) {
                    Image(systemName: "lock.shield")
                        .font(.system(size: 36))
                        .foregroundStyle(.secondary.opacity(0.5))
                    Text("This conversation is encrypted")
                        .font(.headline)
                    Text("Only group members can read messages here.\nInvite someone to start chatting.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                    if let group = viewModel.group, group.members.count <= 1 {
                        Button { showGroupInfo = true } label: {
                            Label("Share invite link", systemImage: "square.and.arrow.up")
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                    }
                }
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

                                    if message.isSystemMessage {
                                        VStack(spacing: 4) {
                                            if message.text.hasPrefix("VOTE_PROPOSAL::") {
                                                VoteProposalCard(
                                                    rawPayload: message.text,
                                                    senderAlias: appState.contactAliasStore.displayName(for: message.senderPubkey),
                                                    targetAlias: { hex in appState.contactAliasStore.displayName(for: hex) }
                                                )
                                            } else {
                                                SystemMessageView(text: message.text)
                                            }
                                            if message.text.contains("joined the group"),
                                               appState.groups.count == 1,
                                               let group = viewModel.group,
                                               group.members.count == 2 {
                                                Text("Say hello — your messages are end-to-end encrypted")
                                                    .font(.caption2)
                                                    .foregroundStyle(.secondary.opacity(0.7))
                                                    .frame(maxWidth: .infinity)
                                            }
                                        }
                                        .id(message.id)
                                    } else {
                                        let isGrouped = isGroupedWithPrevious(at: index)
                                        let senderAlias = appState.contactAliasStore.displayName(for: message.senderPubkey)
                                        let replyParent = viewModel.parentMessage(for: message)
                                        MessageBubble(message: message, isGrouped: isGrouped, senderAlias: senderAlias, replyParent: replyParent, onTapReply: { targetID in
                                            withAnimation(.easeOut(duration: 0.3)) {
                                                proxy.scrollTo(targetID, anchor: .center)
                                            }
                                        }, onRetry: {
                                            viewModel.retryMessage(id: message.id)
                                        })
                                        .id(message.id)
                                        .transition(.opacity.combined(with: .move(edge: .bottom)))
                                        .swipeToReply {
                                            viewModel.replyingToMessage = message
                                        }
                                    }
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
                        .defaultScrollAnchor(.bottom)

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
                    .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardDidShowNotification)) { _ in
                        scrollTask?.cancel()
                        scrollTask = Task {
                            try? await Task.sleep(for: .milliseconds(100))
                            guard !Task.isCancelled else { return }
                            proxy.scrollTo("bottom-anchor", anchor: .bottom)
                        }
                    }
                }
            }

            Divider()

            // Reply preview bar
            if let replyMessage = viewModel.replyingToMessage {
                ReplyPreviewBar(
                    message: replyMessage,
                    senderAlias: appState.contactAliasStore.displayName(for: replyMessage.senderPubkey)
                ) {
                    viewModel.replyingToMessage = nil
                }
            }

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

            // Video preview bar
            if viewModel.selectedVideoURL != nil {
                HStack {
                    ZStack {
                        if let thumb = videoThumbnail {
                            Image(uiImage: thumb)
                                .resizable()
                                .scaledToFill()
                                .frame(width: 60, height: 60)
                                .clipShape(RoundedRectangle(cornerRadius: 8))
                        } else {
                            RoundedRectangle(cornerRadius: 8)
                                .fill(Color(.systemGray4))
                                .frame(width: 60, height: 60)
                        }
                        Image(systemName: "play.circle.fill")
                            .font(.title2)
                            .foregroundStyle(.white)
                        if let dur = videoDuration {
                            VStack {
                                Spacer()
                                HStack {
                                    Spacer()
                                    Text(formatDuration(dur))
                                        .font(.caption2)
                                        .foregroundStyle(.white)
                                        .padding(2)
                                        .background(Color.black.opacity(0.6), in: RoundedRectangle(cornerRadius: 4))
                                }
                            }
                            .frame(width: 60, height: 60)
                        }
                    }

                    Spacer()

                    if viewModel.isSendingVideo {
                        ProgressView()
                    } else {
                        Button("Cancel", role: .cancel) {
                            viewModel.selectedVideoURL = nil
                            videoThumbnail = nil
                            videoDuration = nil
                        }
                        Button("Send") {
                            Task { await viewModel.sendVideo() }
                        }
                        .buttonStyle(.borderedProminent)
                    }
                }
                .padding(.horizontal)
                .padding(.vertical, 8)
            }

            if let group = viewModel.group, !appState.canSend(in: group) {
                VStack(spacing: 8) {
                    Text("You were removed from this group")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                    if appState.canForkGroup(group) {
                        Button {
                            showForkSheet = true
                        } label: {
                            Label("Fork Group", systemImage: "arrow.triangle.branch")
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                    }
                }
                .frame(maxWidth: .infinity)
                .padding()
            } else {
                HStack(spacing: 12) {
                    PhotosPicker(selection: $selectedPhotoItem, matching: .any(of: [.images, .videos])) {
                        Image(systemName: "photo")
                            .font(.title3)
                            .foregroundStyle(viewModel.hasBlossomServers ? .primary : .secondary)
                    }
                    .disabled(!viewModel.hasBlossomServers)
                    .onChange(of: selectedPhotoItem) { _, newItem in
                        guard let newItem else { return }
                        Task {
                            let isVideo = newItem.supportedContentTypes.contains { $0.conforms(to: .movie) }
                            if isVideo {
                                if let data = try? await newItem.loadTransferable(type: Data.self) {
                                    let tempURL = FileManager.default.temporaryDirectory
                                        .appendingPathComponent(UUID().uuidString + ".mp4")
                                    try? data.write(to: tempURL)
                                    viewModel.selectedVideoURL = tempURL
                                }
                            } else {
                                if let data = try? await newItem.loadTransferable(type: Data.self) {
                                    viewModel.selectedImageData = data
                                }
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

                    if viewModel.inputText.trimmingCharacters(in: .whitespaces).isEmpty {
                        VoiceRecordButton(hasBlossomServers: viewModel.hasBlossomServers) { audioURL in
                            Task { await viewModel.sendVoice(audioURL: audioURL) }
                        }
                    } else {
                        Button {
                            UIImpactFeedbackGenerator(style: .light).impactOccurred()
                            Task { await viewModel.sendMessage() }
                        } label: {
                            Image(systemName: "arrow.up.circle.fill")
                                .font(.title2)
                        }
                    }
                }
                .padding()
            }
        }
        .navigationTitle(viewModel.group?.name ?? "Chat")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                HStack(spacing: 12) {
                    Button {
                        startCall(video: false)
                    } label: {
                        Image(systemName: "phone.fill")
                    }
                    Button {
                        startCall(video: true)
                    } label: {
                        Image(systemName: "video.fill")
                    }
                    Button {
                        showGroupInfo = true
                    } label: {
                        Image(systemName: "person.3")
                    }
                }
            }
        }
        .sheet(isPresented: $showGroupInfo) {
            if let group = viewModel.group {
                GroupInfoView(group: group)
            }
        }
        .sheet(isPresented: $showForkSheet) {
            if let group = viewModel.group {
                ForkGroupView(originalGroup: group)
            }
        }
        .fullScreenCover(isPresented: $showCallScreen) {
            CallView(
                callManager: appState.callManager,
                remoteName: viewModel.group?.name ?? "Call",
                onDismiss: { showCallScreen = false }
            )
        }
        .onChange(of: appState.callManager.state) { _, newState in
            if newState == .ringing || newState == .active {
                showCallScreen = true
            }
        }
        .onDisappear {
            viewModel.onDisappear()
        }
        .onChange(of: viewModel.selectedVideoURL) { _, newURL in
            guard let url = newURL else {
                videoThumbnail = nil
                videoDuration = nil
                return
            }
            Task {
                if let thumbData = await MediaCrypto.generateVideoThumbnail(url) {
                    videoThumbnail = UIImage(data: thumbData)
                }
                if let meta = await MediaCrypto.videoMetadata(url) {
                    videoDuration = meta.duration
                }
            }
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

    private func startCall(video: Bool) {
        guard let groupID = viewModel.group?.id else { return }
        appState.callManager.sendSignal = { [weak appState] callDict in
            try await appState?.sendCallSignal(callDict, groupID: groupID)
        }
        Task { try? await appState.callManager.startCall(video: video) }
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

struct SystemMessageView: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.vertical, 4)
            .padding(.horizontal, 12)
            .frame(maxWidth: .infinity)
    }
}

// MARK: - Governance Banner

struct GovernanceBanner: View {
    let groupType: SEPGroupType
    let onDismiss: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: icon)
                .foregroundStyle(tint)
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.caption)
                    .fontWeight(.semibold)
                Text(subtitle)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            Button(action: onDismiss) {
                Image(systemName: "xmark")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(tint.opacity(0.12))
    }

    private var icon: String {
        switch groupType {
        case .anarchy: return "exclamationmark.triangle"
        case .oneOnOne: return "person.2.fill"
        case .democracy: return "hand.raised.fill"
        case .oligarchy: return "star.fill"
        }
    }

    private var tint: Color {
        switch groupType {
        case .anarchy: return .orange
        case .oneOnOne: return .blue
        case .democracy: return .purple
        case .oligarchy: return .yellow
        }
    }

    private var title: String {
        switch groupType {
        case .anarchy: return "Anarchy group"
        case .oneOnOne: return "1-on-1 chat"
        case .democracy: return "Democracy group"
        case .oligarchy: return "Oligarchy group"
        }
    }

    private var subtitle: String {
        switch groupType {
        case .anarchy:
            return "Any member can kick or invite anyone, unilaterally. Choose members carefully."
        case .oneOnOne:
            return "Membership is frozen — no one else can be added. Leaving ends the chat."
        case .democracy:
            return "Kicks and invites require a majority vote from current members."
        case .oligarchy:
            return "Only admins can kick members or approve new invites."
        }
    }
}

// MARK: - Democracy Vote Proposal Card

/// Renders an inline ballot card for a `VOTE_PROPOSAL::v1::...` system message.
/// Yes / No buttons are stubs — actual vote submission is gated on the
/// Democracy circuit VK ceremony. The card surfaces who proposed what so
/// members can see the ballot exists.
struct VoteProposalCard: View {
    let rawPayload: String
    let senderAlias: String?
    let targetAlias: (String) -> String?

    @State private var myChoice: Bool?

    private var parsed: VoteProposalPayload? {
        VoteProposalPayload.parse(rawPayload)
    }

    var body: some View {
        if let parsed {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 6) {
                    Image(systemName: "hand.raised.fill")
                        .foregroundStyle(.purple)
                    Text("Ballot")
                        .font(.caption)
                        .fontWeight(.semibold)
                    Spacer()
                    Text("#\(parsed.ballotID)")
                        .font(.caption2)
                        .monospaced()
                        .foregroundStyle(.secondary)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text("\(senderAlias ?? "A member") proposed to remove:")
                        .font(.caption)
                    Text(displayTarget(parsed.targetPubkeyHex))
                        .font(.caption)
                        .monospaced()
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                HStack(spacing: 8) {
                    Button {
                        myChoice = true
                    } label: {
                        Label("Yes", systemImage: myChoice == true ? "checkmark.circle.fill" : "checkmark.circle")
                            .font(.caption)
                    }
                    .buttonStyle(.bordered)
                    .tint(.green)

                    Button {
                        myChoice = false
                    } label: {
                        Label("No", systemImage: myChoice == false ? "xmark.circle.fill" : "xmark.circle")
                            .font(.caption)
                    }
                    .buttonStyle(.bordered)
                    .tint(.red)
                    Spacer()
                }
                Text("On-chain voting activates once the Democracy circuit is deployed.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .padding(10)
            .background(Color.purple.opacity(0.08))
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .padding(.horizontal, 12)
            .frame(maxWidth: .infinity, alignment: .leading)
        } else {
            SystemMessageView(text: "Ballot (unrecognized)")
        }
    }

    private func displayTarget(_ hex: String) -> String {
        if let alias = targetAlias(hex) { return alias }
        return hex.prefix(12) + "…" + hex.suffix(6)
    }
}

struct VoteProposalPayload {
    let targetPubkeyHex: String
    let ballotID: String

    static func parse(_ raw: String) -> VoteProposalPayload? {
        // `VOTE_PROPOSAL::v1::remove::<targetHex>::<ballotID>`
        let parts = raw.components(separatedBy: "::")
        guard parts.count >= 5,
              parts[0] == "VOTE_PROPOSAL",
              parts[1] == "v1",
              parts[2] == "remove" else { return nil }
        return VoteProposalPayload(
            targetPubkeyHex: parts[3],
            ballotID: parts[4]
        )
    }
}

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
    var senderAlias: String? = nil
    var replyParent: ChatMessage? = nil
    var onTapReply: ((String) -> Void)?
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
                    AvatarView(pubkey: message.senderPubkey, alias: senderAlias)
                } else {
                    // Invisible spacer to keep alignment
                    Color.clear.frame(width: 28, height: 28)
                }
            }

            VStack(alignment: message.isMine ? .trailing : .leading, spacing: 2) {
                if !message.isMine && !isGrouped {
                    Text(senderAlias ?? String(message.senderPubkey.prefix(8)) + "...")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                HStack(alignment: .bottom, spacing: 4) {
                    if let media = message.mediaAttachment {
                        VStack(alignment: .leading, spacing: 0) {
                            if let parent = replyParent {
                                QuotedReplyView(parent: parent, isMine: message.isMine)
                                    .onTapGesture { onTapReply?(parent.id) }
                            } else if message.replyToID != nil {
                                MissingReplyView(isMine: message.isMine)
                            }
                            if media.isAudio {
                                VoiceBubbleContent(media: media, isMine: message.isMine)
                            } else if media.isVideo {
                                VideoBubbleContent(media: media, isMine: message.isMine)
                            } else {
                                ImageBubbleContent(media: media, isMine: message.isMine)
                            }
                        }
                    } else {
                        VStack(alignment: .leading, spacing: 0) {
                            if let parent = replyParent {
                                QuotedReplyView(parent: parent, isMine: message.isMine)
                                    .onTapGesture { onTapReply?(parent.id) }
                            } else if message.replyToID != nil {
                                MissingReplyView(isMine: message.isMine)
                            }
                            Text(message.text)
                                .padding(.horizontal, 12)
                                .padding(.vertical, 8)
                        }
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
    @State private var showFullScreen = false

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
            if fullImage != nil {
                showFullScreen = true
            } else if !isLoading {
                loadFullImage()
            }
        }
        .fullScreenCover(isPresented: $showFullScreen) {
            if let fullImage {
                FullScreenImageView(image: fullImage)
            }
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

// MARK: - Full Screen Image

struct FullScreenImageView: View {
    let image: UIImage
    @Environment(\.dismiss) private var dismiss
    @State private var scale: CGFloat = 1.0
    @State private var lastScale: CGFloat = 1.0
    @State private var offset: CGSize = .zero
    @State private var lastOffset: CGSize = .zero
    @State private var dragOffset: CGFloat = 0

    private var backgroundOpacity: Double {
        let progress = min(abs(dragOffset) / 300, 1.0)
        return 1.0 - progress * 0.6
    }

    var body: some View {
        ZStack {
            Color.black.opacity(backgroundOpacity).ignoresSafeArea()

            GeometryReader { geo in
                let imageSize = CGSize(width: image.size.width, height: image.size.height)
                let fitted = fitSize(imageSize, in: geo.size)

                Image(uiImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(width: fitted.width, height: fitted.height)
                    .scaleEffect(scale)
                    .offset(x: offset.width, y: offset.height + dragOffset)
                    .position(x: geo.size.width / 2, y: geo.size.height / 2)
                    .gesture(
                        MagnifyGesture()
                            .onChanged { value in
                                scale = lastScale * value.magnification
                            }
                            .onEnded { _ in
                                lastScale = scale
                                if scale < 1.0 {
                                    withAnimation(.easeOut(duration: 0.2)) {
                                        scale = 1.0
                                        lastScale = 1.0
                                        offset = .zero
                                        lastOffset = .zero
                                    }
                                }
                            }
                            .simultaneously(with:
                                DragGesture()
                                    .onChanged { value in
                                        if scale > 1.0 {
                                            offset = CGSize(
                                                width: lastOffset.width + value.translation.width,
                                                height: lastOffset.height + value.translation.height
                                            )
                                        } else {
                                            dragOffset = value.translation.height
                                        }
                                    }
                                    .onEnded { value in
                                        if scale > 1.0 {
                                            lastOffset = offset
                                        } else {
                                            if abs(dragOffset) > 150 {
                                                dismiss()
                                            } else {
                                                withAnimation(.easeOut(duration: 0.2)) {
                                                    dragOffset = 0
                                                }
                                            }
                                        }
                                    }
                            )
                    )
                    .onTapGesture(count: 2) {
                        withAnimation(.easeInOut(duration: 0.25)) {
                            if scale > 1.0 {
                                scale = 1.0
                                lastScale = 1.0
                                offset = .zero
                                lastOffset = .zero
                            } else {
                                scale = 2.5
                                lastScale = 2.5
                            }
                        }
                    }
                    .allowsHitTesting(true)
            }

            VStack {
                HStack {
                    Spacer()
                    Button {
                        dismiss()
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(.title)
                            .symbolRenderingMode(.palette)
                            .foregroundStyle(.white, .white.opacity(0.3))
                    }
                    .padding()
                }
                Spacer()
            }
        }
        .statusBarHidden()
    }

    private func fitSize(_ imageSize: CGSize, in containerSize: CGSize) -> CGSize {
        let widthRatio = containerSize.width / imageSize.width
        let heightRatio = containerSize.height / imageSize.height
        let ratio = min(widthRatio, heightRatio)
        return CGSize(width: imageSize.width * ratio, height: imageSize.height * ratio)
    }
}

// MARK: - Video Bubble

struct VideoBubbleContent: View {
    let media: MediaAttachment
    let isMine: Bool
    @State private var thumbnailImage: UIImage?
    @State private var isLoading = false
    @State private var showFullScreen = false
    @State private var videoFileURL: URL?

    var body: some View {
        ZStack {
            if let thumbnail = thumbnailImage {
                Image(uiImage: thumbnail)
                    .resizable()
                    .scaledToFit()
            } else {
                Rectangle()
                    .fill(Color(.systemGray4))
            }

            // Play icon overlay
            if !isLoading {
                Image(systemName: "play.circle.fill")
                    .font(.largeTitle)
                    .foregroundStyle(.white)
                    .shadow(radius: 2)
            } else {
                ProgressView()
                    .tint(.white)
            }

            // Duration label
            if let duration = media.duration {
                VStack {
                    Spacer()
                    HStack {
                        Spacer()
                        Text(formatDuration(duration))
                            .font(.caption2)
                            .foregroundStyle(.white)
                            .padding(.horizontal, 4)
                            .padding(.vertical, 2)
                            .background(Color.black.opacity(0.6), in: RoundedRectangle(cornerRadius: 4))
                            .padding(4)
                    }
                }
            }
        }
        .frame(maxWidth: 220, maxHeight: 280)
        .clipShape(RoundedRectangle(cornerRadius: 16))
        .onAppear {
            if let encThumb = media.encryptedThumbnail,
               let plainData = try? MediaCrypto.decryptMedia(encThumb, key: media.fileKey) {
                thumbnailImage = UIImage(data: plainData)
            }
        }
        .onTapGesture {
            if videoFileURL != nil {
                showFullScreen = true
            } else if !isLoading {
                loadVideo()
            }
        }
        .fullScreenCover(isPresented: $showFullScreen) {
            if let url = videoFileURL {
                FullScreenVideoView(videoURL: url)
            }
        }
    }

    private func loadVideo() {
        if let cached = VideoCache.shared.cachedFileURL(for: media.blobHash) {
            videoFileURL = cached
            showFullScreen = true
            return
        }

        isLoading = true
        Task {
            do {
                let servers = media.blossomServers.compactMap(URL.init(string:))
                let encrypted = try await BlossomClient.download(blobHash: media.blobHash, servers: servers)
                let plain = try MediaCrypto.decryptMedia(encrypted, key: media.fileKey)
                let url = VideoCache.shared.store(plain, for: media.blobHash)
                await MainActor.run {
                    videoFileURL = url
                    isLoading = false
                    showFullScreen = true
                }
            } catch {
                await MainActor.run { isLoading = false }
            }
        }
    }
}

// MARK: - Voice Bubble

struct VoiceBubbleContent: View {
    let media: MediaAttachment
    let isMine: Bool
    @State private var audioPlayer: AVAudioPlayer?
    @State private var isPlaying = false
    @State private var isLoading = false
    @State private var playbackProgress: Double = 0
    @State private var progressTimer: Timer?
    @State private var audioData: Data?

    var body: some View {
        HStack(spacing: 10) {
            Button {
                if isPlaying {
                    pause()
                } else if audioData != nil {
                    play()
                } else {
                    loadAndPlay()
                }
            } label: {
                if isLoading {
                    ProgressView()
                        .frame(width: 32, height: 32)
                } else {
                    Image(systemName: isPlaying ? "pause.circle.fill" : "play.circle.fill")
                        .font(.title)
                        .foregroundStyle(isMine ? .white : .blue)
                }
            }

            VStack(alignment: .leading, spacing: 4) {
                GeometryReader { geo in
                    ZStack(alignment: .leading) {
                        Capsule()
                            .fill(isMine ? Color.white.opacity(0.3) : Color.blue.opacity(0.2))
                            .frame(height: 4)
                        Capsule()
                            .fill(isMine ? .white : .blue)
                            .frame(width: geo.size.width * playbackProgress, height: 4)
                    }
                }
                .frame(height: 4)

                Text(formatDuration(media.duration ?? 0))
                    .font(.caption2)
                    .foregroundStyle(isMine ? .white.opacity(0.7) : .secondary)
                    .monospacedDigit()
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .frame(width: 200)
        .onDisappear {
            audioPlayer?.stop()
            progressTimer?.invalidate()
        }
    }

    private func loadAndPlay() {
        if let cached = AudioCache.shared.data(for: media.blobHash) {
            audioData = cached
            play()
            return
        }

        isLoading = true
        Task {
            do {
                let servers = media.blossomServers.compactMap(URL.init(string:))
                let encrypted = try await BlossomClient.download(blobHash: media.blobHash, servers: servers)
                let plain = try MediaCrypto.decryptMedia(encrypted, key: media.fileKey)
                AudioCache.shared.store(plain, for: media.blobHash)
                await MainActor.run {
                    audioData = plain
                    isLoading = false
                    play()
                }
            } catch {
                await MainActor.run { isLoading = false }
            }
        }
    }

    private func play() {
        guard let data = audioData else { return }
        do {
            try AVAudioSession.sharedInstance().setCategory(.playback)
            try AVAudioSession.sharedInstance().setActive(true)
            audioPlayer = try AVAudioPlayer(data: data)
            audioPlayer?.play()
            isPlaying = true
            startProgressTimer()
        } catch { }
    }

    private func pause() {
        audioPlayer?.pause()
        isPlaying = false
        progressTimer?.invalidate()
    }

    private func startProgressTimer() {
        progressTimer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { _ in
            guard let player = audioPlayer else { return }
            if player.isPlaying {
                playbackProgress = player.currentTime / player.duration
            } else {
                playbackProgress = 0
                isPlaying = false
                progressTimer?.invalidate()
            }
        }
    }
}

// MARK: - Full Screen Video

struct FullScreenVideoView: View {
    let videoURL: URL
    @Environment(\.dismiss) private var dismiss
    @State private var player: AVPlayer?

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()

            if let player {
                VideoPlayer(player: player)
                    .ignoresSafeArea()
            }

            VStack {
                HStack {
                    Spacer()
                    Button {
                        dismiss()
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(.title)
                            .symbolRenderingMode(.palette)
                            .foregroundStyle(.white, .white.opacity(0.3))
                    }
                    .padding()
                }
                Spacer()
            }
        }
        .statusBarHidden()
        .onAppear {
            player = AVPlayer(url: videoURL)
            player?.play()
        }
        .onDisappear {
            player?.pause()
            player = nil
        }
    }
}

// MARK: - Helpers

private func formatDuration(_ seconds: Int) -> String {
    let mins = seconds / 60
    let secs = seconds % 60
    return String(format: "%d:%02d", mins, secs)
}

// MARK: - Swipe to Reply

struct SwipeToReplyModifier: ViewModifier {
    let onSwipe: () -> Void
    @State private var offset: CGFloat = 0
    private let threshold: CGFloat = 60

    func body(content: Content) -> some View {
        content
            .offset(x: offset)
            .gesture(
                DragGesture(minimumDistance: 20, coordinateSpace: .local)
                    .onChanged { value in
                        if value.translation.width > 0 {
                            offset = min(value.translation.width, threshold + 20)
                        }
                    }
                    .onEnded { value in
                        if value.translation.width >= threshold {
                            UIImpactFeedbackGenerator(style: .medium).impactOccurred()
                            onSwipe()
                        }
                        withAnimation(.spring(response: 0.3)) {
                            offset = 0
                        }
                    }
            )
            .overlay(alignment: .leading) {
                if offset > 10 {
                    Image(systemName: "arrowshape.turn.up.left.fill")
                        .foregroundStyle(.secondary)
                        .opacity(min(offset / threshold, 1.0))
                        .offset(x: -28)
                }
            }
    }
}

extension View {
    func swipeToReply(onSwipe: @escaping () -> Void) -> some View {
        modifier(SwipeToReplyModifier(onSwipe: onSwipe))
    }
}

// MARK: - Reply Preview Bar

struct ReplyPreviewBar: View {
    let message: ChatMessage
    let senderAlias: String?
    let onDismiss: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            RoundedRectangle(cornerRadius: 2)
                .fill(Color.blue)
                .frame(width: 3)

            VStack(alignment: .leading, spacing: 2) {
                Text(senderAlias ?? String(message.senderPubkey.prefix(8)) + "...")
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundStyle(.blue)

                if let media = message.mediaAttachment {
                    Label(mediaLabel(for: media), systemImage: mediaIcon(for: media))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                } else {
                    Text(message.text)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }

            Spacer()

            Button { onDismiss() } label: {
                Image(systemName: "xmark.circle.fill")
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal)
        .padding(.vertical, 6)
        .background(Color(.systemGray6))
    }

    private func mediaLabel(for media: MediaAttachment) -> String {
        if media.isAudio { return "Voice message" }
        if media.isVideo { return "Video" }
        return "Photo"
    }

    private func mediaIcon(for media: MediaAttachment) -> String {
        if media.isAudio { return "waveform" }
        if media.isVideo { return "video" }
        return "photo"
    }
}

// MARK: - Quoted Reply

struct QuotedReplyView: View {
    let parent: ChatMessage
    var isMine: Bool = false

    var body: some View {
        HStack(spacing: 4) {
            RoundedRectangle(cornerRadius: 1.5)
                .fill(isMine ? Color.white.opacity(0.5) : Color.blue.opacity(0.6))
                .frame(width: 2.5)

            VStack(alignment: .leading, spacing: 1) {
                Text(String(parent.senderPubkey.prefix(8)) + "...")
                    .font(.caption2)
                    .fontWeight(.semibold)

                if let media = parent.mediaAttachment {
                    Text(mediaLabel(for: media))
                        .font(.caption2)
                        .lineLimit(1)
                } else {
                    Text(parent.text)
                        .font(.caption2)
                        .lineLimit(1)
                }
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 4)
        .opacity(0.8)
    }

    private func mediaLabel(for media: MediaAttachment) -> String {
        if media.isAudio { return "Voice message" }
        if media.isVideo { return "Video" }
        return "Photo"
    }
}

struct MissingReplyView: View {
    var isMine: Bool = false

    var body: some View {
        HStack(spacing: 4) {
            RoundedRectangle(cornerRadius: 1.5)
                .fill(isMine ? Color.white.opacity(0.3) : Color.secondary.opacity(0.4))
                .frame(width: 2.5)
            Text("Original message")
                .font(.caption2)
                .foregroundStyle(isMine ? .white.opacity(0.6) : .secondary)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 4)
    }
}

// MARK: - Avatar

struct AvatarView: View {
    let pubkey: String
    var alias: String? = nil

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
        if let alias, let first = alias.first {
            return String(first).uppercased()
        }
        return String(pubkey.prefix(2)).uppercased()
    }

    private var avatarColor: Color {
        // Deterministic color from first byte of pubkey hex
        let index = Int(pubkey.prefix(2), radix: 16) ?? 0
        return Self.palette[index % Self.palette.count]
    }
}
