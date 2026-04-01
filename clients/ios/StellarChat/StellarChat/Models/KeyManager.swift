import CryptoKit
import Foundation
import SwiftMLS

final class KeyManager: Codable {
    private(set) var secretKey: Data
    private(set) var publicKey: Data
    private let signer: RustBackedNostrSigner

    init() {
        let storedKey = Self.loadFromKeychain()
        if let key = storedKey {
            self.secretKey = key
        } else {
            var bytes = [UInt8](repeating: 0, count: 32)
            _ = SecRandomCopyBytes(kSecRandomDefault, 32, &bytes)
            self.secretKey = Data(bytes)
            Self.saveToKeychain(self.secretKey)
        }
        self.signer = try! RustBackedNostrSigner(secretKey: self.secretKey)
        self.publicKey = try! signer.publicKey()
    }

    var publicKeyHex: String {
        publicKey.map { String(format: "%02x", $0) }.joined()
    }

    /// BLS12-381 leaf hash for SEP Merkle tree membership.
    var leafHash: Data {
        get throws {
            try SEPCommitmentBuilder.computeLeafHash(secretKey: secretKey)
        }
    }

    /// BLS12-381 compressed public key (48 bytes) for SEP group membership.
    var blsPublicKey: Data {
        get throws {
            try SEPCommitmentBuilder.computePublicKey(secretKey: secretKey)
        }
    }

    /// SEP group member leaf for Merkle tree construction.
    var memberLeaf: SEPGroupMemberLeaf {
        get throws {
            SEPGroupMemberLeaf(
                publicKeyCompressed: try blsPublicKey,
                leafHash: try leafHash
            )
        }
    }

    func signEventID(_ eventID: Data) -> Data {
        // Real secp256k1 Schnorr signature via Rust FFI
        return try! signer.signEventID(eventID)
    }

    // MARK: - Codable (exclude signer)

    enum CodingKeys: String, CodingKey {
        case secretKey, publicKey
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.secretKey = try container.decode(Data.self, forKey: .secretKey)
        self.publicKey = try container.decode(Data.self, forKey: .publicKey)
        self.signer = try RustBackedNostrSigner(secretKey: self.secretKey)
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(secretKey, forKey: .secretKey)
        try container.encode(publicKey, forKey: .publicKey)
    }

    // MARK: - Keychain

    private static let keychainKey = "com.stellarmls.chat.nostrSecretKey"

    private static func loadFromKeychain() -> Data? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: keychainKey,
            kSecReturnData as String: true,
        ]
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecSuccess, let data = result as? Data {
            return data
        }
        return nil
    }

    private static func saveToKeychain(_ data: Data) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: keychainKey,
            kSecValueData as String: data,
        ]
        SecItemDelete(query as CFDictionary)
        SecItemAdd(query as CFDictionary, nil)
    }
}
