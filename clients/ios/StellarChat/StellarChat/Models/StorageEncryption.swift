import CryptoKit
import Foundation

/// Field-level encryption for at-rest data using AES-256-GCM.
///
/// The storage key is derived from the Keychain-stored root secret via HKDF,
/// ensuring that even with physical disk access, persisted fields are unreadable
/// without Keychain access (which requires device unlock + biometrics/passcode).
enum StorageEncryption {
    private static let keychainKey = "chat.onym.ios.storageRootKey"
    /// Shared Keychain access group so the NSE can read the same root key.
    private static let accessGroup = "group.chat.onym.ios"

    /// Memoized root secret so the session stays consistent even when the
    /// Keychain write silently fails (e.g. simulator builds without the
    /// keychain-access-groups entitlement under `CODE_SIGNING_ALLOWED=NO`).
    private static let cachedRootSecret: Data = loadOrCreateRootSecret()

    /// AES-256-GCM key derived from a Keychain-stored root secret.
    /// The root secret is generated once and stored in the Keychain with
    /// kSecAttrAccessibleWhenUnlockedThisDeviceOnly protection.
    static var storageKey: SymmetricKey {
        HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: cachedRootSecret),
            salt: Data("chat.onym.ios.storage".utf8),
            info: Data("local-storage-v1".utf8),
            outputByteCount: 32
        )
    }

    // MARK: - Encrypt / Decrypt

    /// Encrypt arbitrary data. Returns `nonce (12 bytes) || ciphertext || tag (16 bytes)`.
    static func encrypt(_ plaintext: Data) throws -> Data {
        let sealed = try AES.GCM.seal(plaintext, using: storageKey)
        // combined = nonce + ciphertext + tag
        guard let combined = sealed.combined else {
            throw StorageEncryptionError.encryptionFailed
        }
        return combined
    }

    /// Encrypt a UTF-8 string.
    static func encrypt(_ string: String) throws -> Data {
        try encrypt(Data(string.utf8))
    }

    /// Decrypt data produced by `encrypt(_:)`.
    static func decrypt(_ combined: Data) throws -> Data {
        let box = try AES.GCM.SealedBox(combined: combined)
        return try AES.GCM.open(box, using: storageKey)
    }

    /// Decrypt to a UTF-8 string.
    static func decryptString(_ combined: Data) throws -> String {
        let data = try decrypt(combined)
        guard let string = String(data: data, encoding: .utf8) else {
            throw StorageEncryptionError.decodingFailed
        }
        return string
    }

    // MARK: - Keychain

    private static func loadOrCreateRootSecret() -> Data {
        // Try shared access group first (main app + NSE)
        if let existing = loadFromKeychain(accessGroup: accessGroup) {
            return existing
        }
        // Migrate from old non-shared Keychain item
        if let legacy = loadFromKeychain(accessGroup: nil) {
            saveToKeychain(legacy)
            // Delete old item so we don't have duplicates
            let deleteQuery: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrAccount as String: keychainKey,
            ]
            SecItemDelete(deleteQuery as CFDictionary)
            return legacy
        }
        var bytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, 32, &bytes)
        let secret = Data(bytes)
        saveToKeychain(secret)
        return secret
    }

    private static func loadFromKeychain(accessGroup: String?) -> Data? {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: keychainKey,
            kSecReturnData as String: true,
        ]
        if let group = accessGroup {
            query[kSecAttrAccessGroup as String] = group
        }
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecSuccess, let data = result as? Data {
            return data
        }
        return nil
    }

    private static func saveToKeychain(_ data: Data) {
        // Delete any existing item (from any access group) first
        let deleteQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: keychainKey,
            kSecAttrAccessGroup as String: accessGroup,
        ]
        SecItemDelete(deleteQuery as CFDictionary)

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: keychainKey,
            kSecAttrAccessGroup as String: accessGroup,
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        SecItemAdd(query as CFDictionary, nil)
    }
}

enum StorageEncryptionError: LocalizedError {
    case encryptionFailed
    case decodingFailed

    var errorDescription: String? {
        switch self {
        case .encryptionFailed: return "Failed to encrypt data for storage"
        case .decodingFailed: return "Failed to decode decrypted data"
        }
    }
}
