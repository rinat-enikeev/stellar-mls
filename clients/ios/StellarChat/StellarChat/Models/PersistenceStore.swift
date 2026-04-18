import Foundation
import SwiftData
import SwiftMLS

/// SwiftData-backed persistence with field-level AES-256-GCM encryption
/// and FileProtectionType.complete on the store directory.
final class PersistenceStore {
    private static let writeQueue = DispatchQueue(
        label: "chat.onym.ios.persistence.write",
        qos: .utility
    )

    let container: ModelContainer
    private let context: ModelContext

    init() throws {
        let schema = Schema([
            PersistedGroup.self, PersistedMessage.self, PersistedContactAlias.self,
            PersistedTransportBundle.self, PersistedPendingRekey.self,
            PersistedEpochSnapshot.self
        ])

        // Store in a directory with complete file protection
        let appSupport = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask
        )[0]
        let storeDir = appSupport.appendingPathComponent("StellarChat", isDirectory: true)
        try FileManager.default.createDirectory(
            at: storeDir,
            withIntermediateDirectories: true,
            attributes: [.protectionKey: FileProtectionType.complete]
        )

        let storeURL = storeDir.appendingPathComponent("StellarChat.store")
        let config = ModelConfiguration(
            schema: schema,
            url: storeURL,
            cloudKitDatabase: .none
        )
        self.container = try ModelContainer(for: schema, configurations: [config])
        self.context = ModelContext(container)
    }

    /// N-15: In-memory fallback when on-disk persistence fails (e.g., disk full, corrupted DB).
    /// Data will not survive app restarts but the app remains usable.
    static func inMemory() -> PersistenceStore {
        let schema = Schema([
            PersistedGroup.self, PersistedMessage.self, PersistedContactAlias.self,
            PersistedTransportBundle.self, PersistedPendingRekey.self,
            PersistedEpochSnapshot.self
        ])
        let config = ModelConfiguration(schema: schema, isStoredInMemoryOnly: true)
        let container = try! ModelContainer(for: schema, configurations: [config])
        let store = PersistenceStore(container: container)
        return store
    }

    private init(container: ModelContainer) {
        self.container = container
        self.context = ModelContext(container)
    }

    // MARK: - Groups

    func loadGroups() -> [ChatGroup] {
        let descriptor = FetchDescriptor<PersistedGroup>(
            sortBy: [SortDescriptor(\.createdAt, order: .forward)]
        )
        guard let persisted = try? context.fetch(descriptor) else { return [] }
        return persisted.compactMap { decryptGroup($0) }
    }

    func saveGroup(_ group: ChatGroup) {
        guard let persisted = encryptGroup(group) else { return }

        // Upsert: delete existing, insert new
        let groupID = group.id
        let descriptor = FetchDescriptor<PersistedGroup>(
            predicate: #Predicate { $0.id == groupID }
        )
        if let existing = try? context.fetch(descriptor) {
            for item in existing { context.delete(item) }
        }
        context.insert(persisted)
        try? context.save()
    }

    func deleteGroup(id: String) {
        let descriptor = FetchDescriptor<PersistedGroup>(
            predicate: #Predicate { $0.id == id }
        )
        if let existing = try? context.fetch(descriptor) {
            for item in existing { context.delete(item) }
        }

        // Also delete messages for this group
        let msgDescriptor = FetchDescriptor<PersistedMessage>(
            predicate: #Predicate { $0.groupID == id }
        )
        if let messages = try? context.fetch(msgDescriptor) {
            for msg in messages { context.delete(msg) }
        }
        try? context.save()
    }

    // MARK: - Messages

    func loadMessages(groupID: String) -> [ChatMessage] {
        let descriptor = FetchDescriptor<PersistedMessage>(
            predicate: #Predicate { $0.groupID == groupID },
            sortBy: [SortDescriptor(\.timestamp, order: .forward)]
        )
        guard let persisted = try? context.fetch(descriptor) else { return [] }
        return persisted.compactMap { decryptMessage($0) }
    }

    func saveMessage(_ message: ChatMessage) {
        // Skip if already persisted (dedup by id)
        let messageID = message.id
        let descriptor = FetchDescriptor<PersistedMessage>(
            predicate: #Predicate { $0.id == messageID }
        )
        if let count = try? context.fetchCount(descriptor), count > 0 { return }

        guard let persisted = encryptMessage(message) else { return }
        context.insert(persisted)
        bumpLastMessageAt(groupID: message.groupID, timestamp: message.timestamp)
        try? context.save()
    }

    /// Set the owning group's `lastMessageAt` to max(existing, timestamp).
    /// Called from saveMessage so every persisted message promotes the chat.
    private func bumpLastMessageAt(groupID: String, timestamp: Date) {
        let descriptor = FetchDescriptor<PersistedGroup>(
            predicate: #Predicate { $0.id == groupID }
        )
        guard let group = (try? context.fetch(descriptor))?.first else { return }
        if let existing = group.lastMessageAt, existing >= timestamp { return }
        group.lastMessageAt = timestamp
    }

    func saveMessageAsync(_ message: ChatMessage) {
        let message = message
        Self.writeQueue.async {
            autoreleasepool {
                guard let writer = try? PersistenceStore() else { return }
                writer.saveMessage(message)
            }
        }
    }

    func deleteMessage(id: String) {
        let descriptor = FetchDescriptor<PersistedMessage>(
            predicate: #Predicate { $0.id == id }
        )
        if let existing = try? context.fetch(descriptor) {
            for item in existing { context.delete(item) }
        }
        try? context.save()
    }

    func deleteMessageAsync(id: String) {
        let id = id
        Self.writeQueue.async {
            autoreleasepool {
                guard let writer = try? PersistenceStore() else { return }
                writer.deleteMessage(id: id)
            }
        }
    }

    /// Atomically replace a persisted message: delete `oldID` and insert `message`
    /// on the same context with a single `context.save()`. Mirrors the Android
    /// `@Transaction replaceMessageTransaction` so a crash between delete and
    /// insert cannot lose the row.
    func replaceMessage(oldID: String, with message: ChatMessage) {
        let oldDescriptor = FetchDescriptor<PersistedMessage>(
            predicate: #Predicate { $0.id == oldID }
        )
        if let existing = try? context.fetch(oldDescriptor) {
            for item in existing { context.delete(item) }
        }

        let newID = message.id
        let dupDescriptor = FetchDescriptor<PersistedMessage>(
            predicate: #Predicate { $0.id == newID }
        )
        let alreadyExists = (try? context.fetchCount(dupDescriptor)).map { $0 > 0 } ?? false
        if !alreadyExists, let persisted = encryptMessage(message) {
            context.insert(persisted)
            bumpLastMessageAt(groupID: message.groupID, timestamp: message.timestamp)
        }
        try? context.save()
    }

    func replaceMessageAsync(oldID: String, with message: ChatMessage) {
        let message = message
        Self.writeQueue.async {
            autoreleasepool {
                guard let writer = try? PersistenceStore() else { return }
                writer.replaceMessage(oldID: oldID, with: message)
            }
        }
    }

    func updateMessageStatus(id: String, status: MessageStatus) {
        let descriptor = FetchDescriptor<PersistedMessage>(
            predicate: #Predicate { $0.id == id }
        )
        guard let existing = try? context.fetch(descriptor), !existing.isEmpty else { return }
        for item in existing { item.status = status.rawValue }
        try? context.save()
    }

    func updateMessageStatusAsync(id: String, status: MessageStatus) {
        let id = id
        let status = status
        Self.writeQueue.async {
            autoreleasepool {
                guard let writer = try? PersistenceStore() else { return }
                writer.updateMessageStatus(id: id, status: status)
            }
        }
    }

    func saveGroupAsync(_ group: ChatGroup) {
        let group = group
        Self.writeQueue.async {
            autoreleasepool {
                guard let writer = try? PersistenceStore() else { return }
                writer.saveGroup(group)
            }
        }
    }

    // MARK: - Contact Aliases

    func loadContactAliases() -> [String: String] {
        let descriptor = FetchDescriptor<PersistedContactAlias>()
        guard let persisted = try? context.fetch(descriptor) else { return [:] }
        var result: [String: String] = [:]
        for alias in persisted {
            if let name = try? StorageEncryption.decryptString(alias.encryptedName) {
                result[alias.pubkey] = name
            }
        }
        return result
    }

    func saveContactAlias(pubkey: String, name: String) {
        guard let encName = try? StorageEncryption.encrypt(name) else { return }
        // Upsert
        let descriptor = FetchDescriptor<PersistedContactAlias>(
            predicate: #Predicate { $0.pubkey == pubkey }
        )
        if let existing = try? context.fetch(descriptor) {
            for item in existing { context.delete(item) }
        }
        context.insert(PersistedContactAlias(pubkey: pubkey, encryptedName: encName, updatedAt: Date()))
        try? context.save()
    }

    func deleteContactAlias(pubkey: String) {
        let descriptor = FetchDescriptor<PersistedContactAlias>(
            predicate: #Predicate { $0.pubkey == pubkey }
        )
        if let existing = try? context.fetch(descriptor) {
            for item in existing { context.delete(item) }
        }
        try? context.save()
    }

    func saveContactAliasAsync(pubkey: String, name: String) {
        Self.writeQueue.async {
            autoreleasepool {
                guard let writer = try? PersistenceStore() else { return }
                writer.saveContactAlias(pubkey: pubkey, name: name)
            }
        }
    }

    func deleteContactAliasAsync(pubkey: String) {
        Self.writeQueue.async {
            autoreleasepool {
                guard let writer = try? PersistenceStore() else { return }
                writer.deleteContactAlias(pubkey: pubkey)
            }
        }
    }

    // MARK: - Transport Bundles

    func loadTransportBundles(groupID: String) -> [String: SEPMemberTransportBundle] {
        let descriptor = FetchDescriptor<PersistedTransportBundle>(
            predicate: #Predicate { $0.groupID == groupID }
        )
        guard let persisted = try? context.fetch(descriptor) else { return [:] }
        var result: [String: SEPMemberTransportBundle] = [:]
        for item in persisted {
            if let bundleData = try? StorageEncryption.decrypt(item.encryptedBundle),
               let bundle = try? JSONDecoder().decode(SEPMemberTransportBundle.self, from: bundleData) {
                result[item.blsPubkeyHex] = bundle
            }
        }
        return result
    }

    func saveTransportBundle(_ bundle: SEPMemberTransportBundle, groupID: String) {
        guard let bundleData = try? JSONEncoder().encode(bundle),
              let encrypted = try? StorageEncryption.encrypt(bundleData) else { return }
        let hex = bundle.blsPubkey.map { String(format: "%02x", $0) }.joined()

        // Upsert
        let descriptor = FetchDescriptor<PersistedTransportBundle>(
            predicate: #Predicate { $0.groupID == groupID && $0.blsPubkeyHex == hex }
        )
        if let existing = try? context.fetch(descriptor) {
            for item in existing { context.delete(item) }
        }
        context.insert(PersistedTransportBundle(groupID: groupID, blsPubkeyHex: hex, encryptedBundle: encrypted))
        try? context.save()
    }

    func saveTransportBundleAsync(_ bundle: SEPMemberTransportBundle, groupID: String) {
        let bundle = bundle
        let groupID = groupID
        Self.writeQueue.async {
            autoreleasepool {
                guard let writer = try? PersistenceStore() else { return }
                writer.saveTransportBundle(bundle, groupID: groupID)
            }
        }
    }

    func deleteTransportBundles(groupID: String) {
        let descriptor = FetchDescriptor<PersistedTransportBundle>(
            predicate: #Predicate { $0.groupID == groupID }
        )
        if let existing = try? context.fetch(descriptor) {
            for item in existing { context.delete(item) }
        }
        try? context.save()
    }

    // MARK: - Pending Rekeys

    func savePendingRekey(groupID: String, epoch: Int, envelope: Data, unackedKeys: [String], isRemovalEpoch: Bool) {
        guard let encEnvelope = try? StorageEncryption.encrypt(envelope) else { return }
        let keysJSON = (try? JSONEncoder().encode(unackedKeys)) ?? Data()
        context.insert(PersistedPendingRekey(
            groupID: groupID, epoch: epoch, encryptedEnvelope: encEnvelope,
            unackedMemberKeysJSON: keysJSON, isRemovalEpoch: isRemovalEpoch
        ))
        try? context.save()
    }

    func loadPendingRekeys(groupID: String) -> [PersistedPendingRekey] {
        let descriptor = FetchDescriptor<PersistedPendingRekey>(
            predicate: #Predicate { $0.groupID == groupID }
        )
        return (try? context.fetch(descriptor)) ?? []
    }

    /// Remove a single member from the unacked set. Deletes the record if all members have acked.
    func updatePendingRekeyAck(groupID: String, epoch: Int, ackedMemberHex: String) {
        let descriptor = FetchDescriptor<PersistedPendingRekey>(
            predicate: #Predicate { $0.groupID == groupID && $0.epoch == epoch }
        )
        guard let records = try? context.fetch(descriptor), let record = records.first else { return }
        var keys = (try? JSONDecoder().decode([String].self, from: record.unackedMemberKeysJSON)) ?? []
        keys.removeAll { $0 == ackedMemberHex }
        if keys.isEmpty {
            context.delete(record)
        } else {
            record.unackedMemberKeysJSON = (try? JSONEncoder().encode(keys)) ?? Data()
        }
        try? context.save()
    }

    /// Increment retry count and return the updated value.
    @discardableResult
    func incrementPendingRekeyRetry(groupID: String, epoch: Int) -> Int {
        let descriptor = FetchDescriptor<PersistedPendingRekey>(
            predicate: #Predicate { $0.groupID == groupID && $0.epoch == epoch }
        )
        guard let records = try? context.fetch(descriptor), let record = records.first else { return -1 }
        record.retryCount += 1
        try? context.save()
        return record.retryCount
    }

    /// Load ALL pending rekeys across all groups.
    func loadAllPendingRekeys() -> [PersistedPendingRekey] {
        let descriptor = FetchDescriptor<PersistedPendingRekey>()
        return (try? context.fetch(descriptor)) ?? []
    }

    func deletePendingRekey(groupID: String, epoch: Int) {
        let descriptor = FetchDescriptor<PersistedPendingRekey>(
            predicate: #Predicate { $0.groupID == groupID && $0.epoch == epoch }
        )
        if let existing = try? context.fetch(descriptor) {
            for item in existing { context.delete(item) }
        }
        try? context.save()
    }

    func isRemovalEpoch(groupID: String, epoch: Int) -> Bool {
        let descriptor = FetchDescriptor<PersistedPendingRekey>(
            predicate: #Predicate { $0.groupID == groupID && $0.epoch == epoch && $0.isRemovalEpoch == true }
        )
        return ((try? context.fetchCount(descriptor)) ?? 0) > 0
    }

    // MARK: - Epoch Snapshots

    func saveEpochSnapshot(_ snapshot: EpochSnapshot, groupID: String) {
        guard let encMembers = try? StorageEncryption.encrypt(JSONEncoder().encode(snapshot.members)),
              let encSalt = try? StorageEncryption.encrypt(snapshot.salt),
              let encSecret = try? StorageEncryption.encrypt(snapshot.groupSecret)
        else { return }

        // Upsert
        let epoch = Int(snapshot.epoch)
        let descriptor = FetchDescriptor<PersistedEpochSnapshot>(
            predicate: #Predicate { $0.groupID == groupID && $0.epoch == epoch }
        )
        if let existing = try? context.fetch(descriptor) {
            for item in existing { context.delete(item) }
        }
        let persisted = PersistedEpochSnapshot(
            groupID: groupID,
            epoch: epoch,
            encryptedMembers: encMembers,
            encryptedSalt: encSalt,
            encryptedGroupSecret: encSecret,
            changeDescription: snapshot.changeDescription
        )
        context.insert(persisted)
        try? context.save()
    }

    func saveEpochSnapshotAsync(_ snapshot: EpochSnapshot, groupID: String) {
        Self.writeQueue.async {
            autoreleasepool {
                guard let writer = try? PersistenceStore() else { return }
                writer.saveEpochSnapshot(snapshot, groupID: groupID)
            }
        }
    }

    func loadEpochSnapshots(groupID: String) -> [UInt64: EpochSnapshot] {
        let descriptor = FetchDescriptor<PersistedEpochSnapshot>(
            predicate: #Predicate { $0.groupID == groupID },
            sortBy: [SortDescriptor(\.epoch, order: .forward)]
        )
        guard let persisted = try? context.fetch(descriptor) else { return [:] }
        var result: [UInt64: EpochSnapshot] = [:]
        for p in persisted {
            guard let membersData = try? StorageEncryption.decrypt(p.encryptedMembers),
                  let members = try? JSONDecoder().decode([SEPGroupMemberLeaf].self, from: membersData),
                  let salt = try? StorageEncryption.decrypt(p.encryptedSalt),
                  let secret = try? StorageEncryption.decrypt(p.encryptedGroupSecret)
            else { continue }
            let epoch = UInt64(p.epoch)
            result[epoch] = EpochSnapshot(
                epoch: epoch, members: members, salt: salt,
                groupSecret: secret, changeDescription: p.changeDescription
            )
        }
        return result
    }

    func deleteEpochSnapshots(groupID: String) {
        let descriptor = FetchDescriptor<PersistedEpochSnapshot>(
            predicate: #Predicate { $0.groupID == groupID }
        )
        if let existing = try? context.fetch(descriptor) {
            for item in existing { context.delete(item) }
        }
        try? context.save()
    }

    /// Trim epoch snapshots to keep only the most recent `maxCount` per group.
    func trimEpochSnapshots(groupID: String, maxCount: Int) {
        let descriptor = FetchDescriptor<PersistedEpochSnapshot>(
            predicate: #Predicate { $0.groupID == groupID },
            sortBy: [SortDescriptor(\.epoch, order: .forward)]
        )
        guard let existing = try? context.fetch(descriptor), existing.count > maxCount else { return }
        let toRemove = existing.prefix(existing.count - maxCount)
        for item in toRemove { context.delete(item) }
        try? context.save()
    }

    // MARK: - Encryption Helpers

    private func encryptGroup(_ group: ChatGroup) -> PersistedGroup? {
        guard let encName = try? StorageEncryption.encrypt(group.name),
              let encSecret = try? StorageEncryption.encrypt(group.groupSecret),
              let encSalt = try? StorageEncryption.encrypt(group.salt)
        else { return nil }

        let membersData = (try? JSONEncoder().encode(group.members)) ?? Data()
        guard let encMembers = try? StorageEncryption.encrypt(membersData) else { return nil }

        let encCommitment: Data?
        if let commitment = group.commitment {
            encCommitment = try? StorageEncryption.encrypt(commitment)
        } else {
            encCommitment = nil
        }

        let relayStrings = group.relayHints.map(\.absoluteString)
        let relayJSON = (try? JSONEncoder().encode(relayStrings)) ?? Data()

        let encAdmins: Data?
        if group.adminPubkeys.isEmpty {
            encAdmins = nil
        } else {
            let sorted = Array(group.adminPubkeys).sorted()
            if let adminJSON = try? JSONEncoder().encode(sorted),
               let encrypted = try? StorageEncryption.encrypt(adminJSON) {
                encAdmins = encrypted
            } else {
                encAdmins = nil
            }
        }

        return PersistedGroup(
            id: group.id,
            encryptedName: encName,
            encryptedGroupSecret: encSecret,
            createdAt: group.createdAt,
            relayHintsJSON: relayJSON,
            encryptedMembers: encMembers,
            epoch: Int(group.epoch),
            encryptedSalt: encSalt,
            encryptedCommitment: encCommitment,
            tierRawValue: group.tier.rawValue,
            isPublishedOnChain: group.isPublishedOnChain,
            pinnedEpoch: group.pinnedEpoch.map { Int($0) },
            forkedFromGroupID: group.forkedFromGroupID,
            forkedAtEpoch: group.forkedAtEpoch.map { Int($0) },
            removedByPubkeyHex: group.removedByPubkeyHex,
            pushNotificationsEnabled: group.pushNotificationsEnabled,
            lastMessageAt: group.lastMessageAt,
            groupTypeRawValue: Int(group.groupType.rawValue),
            encryptedAdminPubkeys: encAdmins
        )
    }

    private func decryptGroup(_ persisted: PersistedGroup) -> ChatGroup? {
        guard let name = try? StorageEncryption.decryptString(persisted.encryptedName),
              let secret = try? StorageEncryption.decrypt(persisted.encryptedGroupSecret),
              let salt = try? StorageEncryption.decrypt(persisted.encryptedSalt)
        else { return nil }

        let membersData = (try? StorageEncryption.decrypt(persisted.encryptedMembers)) ?? Data()
        let members = (try? JSONDecoder().decode([SEPGroupMemberLeaf].self, from: membersData)) ?? []

        let commitment: Data?
        if let encCommitment = persisted.encryptedCommitment {
            commitment = try? StorageEncryption.decrypt(encCommitment)
        } else {
            commitment = nil
        }

        let relayStrings = (try? JSONDecoder().decode([String].self, from: persisted.relayHintsJSON)) ?? []
        let relayURLs = relayStrings.compactMap(URL.init(string:))

        var group = ChatGroup(
            id: persisted.id,
            name: name,
            groupSecret: secret,
            createdAt: persisted.createdAt,
            relayHints: relayURLs,
            members: members,
            epoch: UInt64(persisted.epoch),
            salt: salt,
            commitment: commitment,
            tier: SEPTier(rawValue: persisted.tierRawValue) ?? .small
        )
        group.isPublishedOnChain = persisted.isPublishedOnChain
        group.pinnedEpoch = persisted.pinnedEpoch.map { UInt64($0) }
        group.forkedFromGroupID = persisted.forkedFromGroupID
        group.forkedAtEpoch = persisted.forkedAtEpoch.map { UInt64($0) }
        group.removedByPubkeyHex = persisted.removedByPubkeyHex
        group.pushNotificationsEnabled = persisted.pushNotificationsEnabled
        group.lastMessageAt = persisted.lastMessageAt
        if let raw = persisted.groupTypeRawValue, let parsed = SEPGroupType(rawValue: UInt32(raw)) {
            group.groupType = parsed
        }
        if let encAdmins = persisted.encryptedAdminPubkeys,
           let adminJSON = try? StorageEncryption.decrypt(encAdmins),
           let admins = try? JSONDecoder().decode([String].self, from: adminJSON) {
            group.adminPubkeys = Set(admins)
        }
        return group
    }

    private func encryptMessage(_ message: ChatMessage) -> PersistedMessage? {
        guard let encText = try? StorageEncryption.encrypt(message.text) else { return nil }
        let encMedia: Data?
        if let media = message.mediaAttachment,
           let mediaJSON = try? JSONEncoder().encode(media),
           let encrypted = try? StorageEncryption.encrypt(mediaJSON) {
            encMedia = encrypted
        } else {
            encMedia = nil
        }
        return PersistedMessage(
            id: message.id,
            groupID: message.groupID,
            senderPubkey: message.senderPubkey,
            encryptedText: encText,
            timestamp: message.timestamp,
            isMine: message.isMine,
            encryptedMediaAttachment: encMedia,
            isSystemMessage: message.isSystemMessage,
            epoch: message.epoch.map { Int($0) },
            replyToID: message.replyToID,
            status: message.status.rawValue
        )
    }

    private func decryptMessage(_ persisted: PersistedMessage) -> ChatMessage? {
        guard let text = try? StorageEncryption.decryptString(persisted.encryptedText) else { return nil }
        let media: MediaAttachment?
        if let encMedia = persisted.encryptedMediaAttachment,
           let mediaJSON = try? StorageEncryption.decrypt(encMedia),
           let decoded = try? JSONDecoder().decode(MediaAttachment.self, from: mediaJSON) {
            media = decoded
        } else {
            media = nil
        }
        // Recover SENDING → FAILED on app restart: if a message was mid-send when
        // the process died, the relay never confirmed it and it must surface as
        // FAILED so the user can retry. Matches Android PersistenceStore.decryptMessage.
        let rawStatus = persisted.status.flatMap { MessageStatus(rawValue: $0) } ?? .sent
        let recoveredStatus: MessageStatus = (rawStatus == .sending) ? .failed : rawStatus
        return ChatMessage(
            id: persisted.id,
            groupID: persisted.groupID,
            senderPubkey: persisted.senderPubkey,
            text: text,
            timestamp: persisted.timestamp,
            isMine: persisted.isMine,
            status: recoveredStatus,
            mediaAttachment: media,
            isSystemMessage: persisted.isSystemMessage ?? false,
            epoch: persisted.epoch.map { UInt64($0) },
            replyToID: persisted.replyToID
        )
    }
}
