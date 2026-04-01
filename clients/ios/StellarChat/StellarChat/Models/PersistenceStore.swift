import Foundation

enum PersistenceStore {
    private static let fileManager = FileManager.default

    private static var baseDirectory: URL {
        let appSupport = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let dir = appSupport.appendingPathComponent("StellarChat", isDirectory: true)
        try? fileManager.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    private static var groupsFile: URL {
        baseDirectory.appendingPathComponent("groups.json")
    }

    private static func messagesFile(groupID: String) -> URL {
        let dir = baseDirectory.appendingPathComponent("messages", isDirectory: true)
        try? fileManager.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("\(groupID).json")
    }

    // MARK: - Groups

    static func loadGroups() -> [ChatGroup] {
        guard let data = try? Data(contentsOf: groupsFile) else { return [] }
        return (try? JSONDecoder().decode([ChatGroup].self, from: data)) ?? []
    }

    static func saveGroups(_ groups: [ChatGroup]) {
        guard let data = try? JSONEncoder().encode(groups) else { return }
        try? data.write(to: groupsFile, options: .atomic)
    }

    // MARK: - Messages

    static func loadMessages(groupID: String) -> [ChatMessage] {
        guard let data = try? Data(contentsOf: messagesFile(groupID: groupID)) else { return [] }
        return (try? JSONDecoder().decode([ChatMessage].self, from: data)) ?? []
    }

    static func saveMessages(_ messages: [ChatMessage], groupID: String) {
        guard let data = try? JSONEncoder().encode(messages) else { return }
        try? data.write(to: messagesFile(groupID: groupID), options: .atomic)
    }
}
