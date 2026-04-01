import Foundation
import SwiftData
import SwiftMLS

/// SwiftData-backed persistence with field-level AES-256-GCM encryption
/// and FileProtectionType.complete on the store directory.
final class PersistenceStore {
    let container: ModelContainer
    private let context: ModelContext

    init() throws {
        let schema = Schema([PersistedGroup.self, PersistedMessage.self])

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
            tierRawValue: group.tier.rawValue
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

        return ChatGroup(
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
    }

    private func encryptMessage(_ message: ChatMessage) -> PersistedMessage? {
        guard let encText = try? StorageEncryption.encrypt(message.text) else { return nil }
        return PersistedMessage(
            id: message.id,
            groupID: message.groupID,
            senderPubkey: message.senderPubkey,
            encryptedText: encText,
            timestamp: message.timestamp,
            isMine: message.isMine
        )
    }

    private func decryptMessage(_ persisted: PersistedMessage) -> ChatMessage? {
        guard let text = try? StorageEncryption.decryptString(persisted.encryptedText) else { return nil }
        return ChatMessage(
            id: persisted.id,
            groupID: persisted.groupID,
            senderPubkey: persisted.senderPubkey,
            text: text,
            timestamp: persisted.timestamp,
            isMine: persisted.isMine
        )
    }
}
