import CryptoKit
import Foundation
import SwiftMLS

final class KeyManager: Codable {
    /// secp256k1 secret key for Nostr event signing.
    private(set) var nostrSecretKey: Data
    /// Independent BLS12-381 scalar for group membership (Poseidon Merkle tree).
    private(set) var blsSecretKey: Data
    /// secp256k1 x-only public key (32 bytes).
    private(set) var publicKey: Data
    private let signer: RustBackedNostrSigner

    init() {
        // Load or generate Nostr key (preserves existing identity)
        if let existing = Self.loadFromKeychain(key: Self.nostrKeychainKey) {
            self.nostrSecretKey = existing
        } else {
            self.nostrSecretKey = Self.generateRandom(count: 32)
            Self.saveToKeychain(self.nostrSecretKey, key: Self.nostrKeychainKey)
        }

        // Load or generate independent BLS key
        if let existing = Self.loadFromKeychain(key: Self.blsKeychainKey) {
            self.blsSecretKey = existing
        } else {
            self.blsSecretKey = Self.generateRandom(count: 32)
            Self.saveToKeychain(self.blsSecretKey, key: Self.blsKeychainKey)
        }

        self.signer = try! RustBackedNostrSigner(secretKey: self.nostrSecretKey)
        self.publicKey = try! signer.publicKey()
    }

    var publicKeyHex: String {
        publicKey.map { String(format: "%02x", $0) }.joined()
    }

    // MARK: - BLS12-381 (Group Membership)

    /// BLS12-381 leaf hash for SEP Merkle tree membership.
    var leafHash: Data {
        get throws {
            try SEPCommitmentBuilder.computeLeafHash(secretKey: blsSecretKey)
        }
    }

    /// BLS12-381 compressed public key (48 bytes) for SEP group membership.
    var blsPublicKey: Data {
        get throws {
            try SEPCommitmentBuilder.computePublicKey(secretKey: blsSecretKey)
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

    // MARK: - Nostr Signing

    func signEventID(_ eventID: Data) -> Data {
        try! signer.signEventID(eventID)
    }

    // MARK: - Key Attestation (SEP-XXXX §1.1)

    /// Create an attestation binding the BLS group key to the Nostr identity key.
    func createAttestation() throws -> KeyAttestation {
        let blsPub = try blsPublicKey
        let bindingMessage = KeyAttestation.bindingMessage(blsPubkey: blsPub)
        let signature = try signer.signEventID(bindingMessage)
        return KeyAttestation(
            blsPubkey: blsPub,
            nostrPubkey: publicKey,
            signature: signature
        )
    }

    // MARK: - Codable (exclude signer)

    enum CodingKeys: String, CodingKey {
        case nostrSecretKey, blsSecretKey, publicKey
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.nostrSecretKey = try container.decode(Data.self, forKey: .nostrSecretKey)
        self.blsSecretKey = try container.decode(Data.self, forKey: .blsSecretKey)
        self.publicKey = try container.decode(Data.self, forKey: .publicKey)
        self.signer = try RustBackedNostrSigner(secretKey: self.nostrSecretKey)
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(nostrSecretKey, forKey: .nostrSecretKey)
        try container.encode(blsSecretKey, forKey: .blsSecretKey)
        try container.encode(publicKey, forKey: .publicKey)
    }

    // MARK: - Keychain

    private static let nostrKeychainKey = "com.stellarmls.chat.nostrSecretKey"
    private static let blsKeychainKey = "com.stellarmls.chat.blsSecretKey"

    private static func generateRandom(count: Int) -> Data {
        var bytes = [UInt8](repeating: 0, count: count)
        _ = SecRandomCopyBytes(kSecRandomDefault, count, &bytes)
        return Data(bytes)
    }

    private static func loadFromKeychain(key: String) -> Data? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
        ]
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecSuccess, let data = result as? Data {
            return data
        }
        return nil
    }

    private static func saveToKeychain(_ data: Data, key: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key,
            kSecValueData as String: data,
        ]
        SecItemDelete(query as CFDictionary)
        SecItemAdd(query as CFDictionary, nil)
    }
}
