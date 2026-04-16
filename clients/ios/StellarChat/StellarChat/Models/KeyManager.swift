import CryptoKit
import Foundation
import SwiftMLS

enum KeyManagerError: LocalizedError {
    case keychainReadFailed(key: String, status: OSStatus)
    case keychainWriteFailed(key: String, status: OSStatus)
    case invalidStoredKey(key: String, reason: String)
    case signerInitializationFailed(reason: String)

    var errorDescription: String? {
        switch self {
        case let .keychainReadFailed(key, status):
            return "Keychain read failed for \(key) (status \(status))"
        case let .keychainWriteFailed(key, status):
            return "Keychain write failed for \(key) (status \(status))"
        case let .invalidStoredKey(key, reason):
            return "Stored key \(key) is invalid: \(reason)"
        case let .signerInitializationFailed(reason):
            return "Nostr signer initialization failed: \(reason)"
        }
    }
}

final class KeyManager: Codable {
    /// secp256k1 secret key for Nostr event signing.
    private(set) var nostrSecretKey: Data
    /// Independent BLS12-381 scalar for group membership (Poseidon Merkle tree).
    private(set) var blsSecretKey: Data
    /// secp256k1 x-only public key (32 bytes).
    private(set) var publicKey: Data
    private var signer: RustBackedNostrSigner
    /// Ed25519 private key for Stellar, derived from nostrSecretKey via HKDF.
    private let stellarPrivateKey: Curve25519.Signing.PrivateKey
    /// X25519 private key for invitation ECDH, derived from nostrSecretKey via HKDF.
    private let keyAgreementKey: Curve25519.KeyAgreement.PrivateKey

    init() throws {
        // Load or generate Nostr key (preserves existing identity)
        if let existing = try Self.loadFromKeychain(key: Self.nostrKeychainKey) {
            self.nostrSecretKey = existing
        } else {
            self.nostrSecretKey = Self.generateRandom(count: 32)
            try Self.saveToKeychain(self.nostrSecretKey, key: Self.nostrKeychainKey)
        }

        // Load or generate independent BLS key
        if let existing = try Self.loadFromKeychain(key: Self.blsKeychainKey) {
            self.blsSecretKey = existing
        } else {
            self.blsSecretKey = Self.generateRandom(count: 32)
            try Self.saveToKeychain(self.blsSecretKey, key: Self.blsKeychainKey)
        }

        guard nostrSecretKey.count == 32 else {
            throw KeyManagerError.invalidStoredKey(
                key: Self.nostrKeychainKey,
                reason: "expected 32 bytes, got \(nostrSecretKey.count)"
            )
        }
        guard blsSecretKey.count == 32 else {
            throw KeyManagerError.invalidStoredKey(
                key: Self.blsKeychainKey,
                reason: "expected 32 bytes, got \(blsSecretKey.count)"
            )
        }

        // Refuse to rotate identity silently if a stored key is unreadable or invalid.
        do {
            self.signer = try RustBackedNostrSigner(secretKey: self.nostrSecretKey)
            self.publicKey = try signer.publicKey()
        } catch {
            throw KeyManagerError.signerInitializationFailed(reason: error.localizedDescription)
        }
        self.stellarPrivateKey = Self.deriveStellarKey(from: self.nostrSecretKey)
        self.keyAgreementKey = Self.deriveKeyAgreementKey(from: self.nostrSecretKey)
    }

    var publicKeyHex: String {
        publicKey.map { String(format: "%02x", $0) }.joined()
    }

    // MARK: - Stellar Ed25519 (derived from Nostr key)

    /// Ed25519 public key (32 bytes) for Stellar account binding.
    var stellarPublicKey: Data {
        Data(stellarPrivateKey.publicKey.rawRepresentation)
    }

    /// Stellar account ID in StrKey format (G...).
    var stellarAccountID: String {
        StellarStrKey.encodeAccountID(stellarPublicKey)
    }

    /// Sign arbitrary data with the Stellar Ed25519 key.
    func stellarSign(_ message: Data) throws -> Data {
        let signature = try stellarPrivateKey.signature(for: message)
        return Data(signature)
    }

    /// Derive a deterministic Ed25519 private key from the Nostr secret via HKDF.
    private static func deriveStellarKey(from nostrSecret: Data) -> Curve25519.Signing.PrivateKey {
        let derived = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: nostrSecret),
            salt: Data("chat.onym.ios".utf8),
            info: Data("stellar-ed25519-v1".utf8),
            outputByteCount: 32
        )
        let seed = derived.withUnsafeBytes { Data($0) }
        // HKDF output is always 32 bytes, CryptoKit PrivateKey accepts any 32 bytes.
        return try! Curve25519.Signing.PrivateKey(rawRepresentation: seed)
    }

    /// Ed25519 signing key for use in invitation transport (exposed for ephemeral key signing).
    var ed25519SigningKey: Curve25519.Signing.PrivateKey {
        stellarPrivateKey
    }

    // MARK: - X25519 Key Agreement (Invitation Encryption)

    /// X25519 public key (32 bytes) used as the inbox key for receiving invitations.
    var keyAgreementPublicKey: Data {
        Data(keyAgreementKey.publicKey.rawRepresentation)
    }

    var keyAgreementPublicKeyHex: String {
        keyAgreementPublicKey.map { String(format: "%02x", $0) }.joined()
    }

    /// Hidden inbox tag derived from the X25519 key agreement public key.
    var inboxTag: String {
        GroupCrypto.hiddenInboxTag(recipientPublicKey: keyAgreementPublicKey)
    }

    /// The X25519 private key for decrypting incoming invitations.
    var keyAgreementPrivateKey: Curve25519.KeyAgreement.PrivateKey {
        keyAgreementKey
    }

    /// Derive a deterministic X25519 key agreement key from the Nostr secret via HKDF.
    private static func deriveKeyAgreementKey(from nostrSecret: Data) -> Curve25519.KeyAgreement.PrivateKey {
        let derived = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: nostrSecret),
            salt: Data("chat.onym.ios".utf8),
            info: Data("x25519-key-agreement-v1".utf8),
            outputByteCount: 32
        )
        let seed = derived.withUnsafeBytes { Data($0) }
        // HKDF output is always 32 bytes, CryptoKit PrivateKey accepts any 32 bytes.
        return try! Curve25519.KeyAgreement.PrivateKey(rawRepresentation: seed)
    }

    /// Verify an Ed25519 signature (static, for verifying other members' signatures).
    static func verifyEd25519Signature(_ signature: Data, for message: Data, publicKey: Data) -> Bool {
        guard let key = try? Curve25519.Signing.PublicKey(rawRepresentation: publicKey) else {
            return false
        }
        return key.isValidSignature(signature, for: message)
    }

    // MARK: - Transport Bundle

    /// Create an authenticated transport bundle binding BLS, Stellar Ed25519, and X25519 inbox keys.
    func createTransportBundle() throws -> SEPMemberTransportBundle {
        let blsPub = try blsPublicKey
        let bundle = SEPMemberTransportBundle(
            blsPubkey: blsPub,
            stellarEd25519Pubkey: stellarPublicKey,
            x25519InboxPubkey: keyAgreementPublicKey,
            signature: Data()  // placeholder, replaced below
        )
        let message = bundle.computeBindingMessage()
        let signature = try stellarSign(message)
        return SEPMemberTransportBundle(
            blsPubkey: blsPub,
            stellarEd25519Pubkey: stellarPublicKey,
            x25519InboxPubkey: keyAgreementPublicKey,
            signature: signature
        )
    }

    /// Verify another member's transport bundle signature.
    static func verifyTransportBundle(_ bundle: SEPMemberTransportBundle) -> Bool {
        guard bundle.hasValidStructure else { return false }
        let message = bundle.computeBindingMessage()
        return verifyEd25519Signature(bundle.signature, for: message, publicKey: bundle.stellarEd25519Pubkey)
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

    func signEventID(_ eventID: Data) throws -> Data {
        try signer.signEventID(eventID)
    }

    // MARK: - Key Attestation (SEP-XXXX §1.1)

    /// Create a spec-compliant attestation binding the BLS group key to the Stellar Ed25519 key.
    /// Signature: Ed25519 over SHA-256("SEP-XXXX:key-binding" || bls_pubkey).
    func createAttestation() throws -> KeyAttestation {
        let blsPub = try blsPublicKey
        let bindingMessage = KeyAttestation.bindingMessage(blsPubkey: blsPub)
        let signature = try stellarSign(bindingMessage)
        return KeyAttestation(
            blsPubkey: blsPub,
            ed25519Pubkey: stellarPublicKey,
            signature: signature
        )
    }

    /// Verify an attestation from another member.
    static func verifyAttestation(_ attestation: KeyAttestation) -> Bool {
        guard attestation.blsPubkey.count == 48,
              attestation.ed25519Pubkey.count == 32,
              attestation.signature.count == 64
        else { return false }

        guard let verifyingKey = try? Curve25519.Signing.PublicKey(
            rawRepresentation: attestation.ed25519Pubkey
        ) else { return false }

        let message = KeyAttestation.bindingMessage(blsPubkey: attestation.blsPubkey)
        return verifyingKey.isValidSignature(attestation.signature, for: message)
    }

    // MARK: - Codable (exclude derived keys)

    enum CodingKeys: String, CodingKey {
        case nostrSecretKey, blsSecretKey, publicKey
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.nostrSecretKey = try container.decode(Data.self, forKey: .nostrSecretKey)
        self.blsSecretKey = try container.decode(Data.self, forKey: .blsSecretKey)
        self.publicKey = try container.decode(Data.self, forKey: .publicKey)
        self.signer = try RustBackedNostrSigner(secretKey: self.nostrSecretKey)
        self.stellarPrivateKey = Self.deriveStellarKey(from: self.nostrSecretKey)
        self.keyAgreementKey = Self.deriveKeyAgreementKey(from: self.nostrSecretKey)
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(nostrSecretKey, forKey: .nostrSecretKey)
        try container.encode(blsSecretKey, forKey: .blsSecretKey)
        try container.encode(publicKey, forKey: .publicKey)
    }

    // MARK: - Keychain

    private static let keychainService = "chat.onym.ios.keys"
    private static let nostrKeychainKey = "chat.onym.ios.nostrSecretKey"
    private static let blsKeychainKey = "chat.onym.ios.blsSecretKey"

    private static func generateRandom(count: Int) -> Data {
        var bytes = [UInt8](repeating: 0, count: count)
        _ = SecRandomCopyBytes(kSecRandomDefault, count, &bytes)
        return Data(bytes)
    }

    private static func keychainQuery(key: String, includeService: Bool) -> [String: Any] {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
        ]
        if includeService {
            query[kSecAttrService as String] = keychainService
        }
        return query
    }

    private static func loadFromKeychain(key: String) throws -> Data? {
        if let data = try readKeychainData(query: keychainQuery(key: key, includeService: true), key: key) {
            return data
        }

        guard let legacyData = try readKeychainData(query: keychainQuery(key: key, includeService: false), key: key) else {
            return nil
        }

        try saveToKeychain(legacyData, key: key)
        deleteLegacyKeychainItem(key: key)
        return legacyData
    }

    private static func readKeychainData(query: [String: Any], key: String) throws -> Data? {
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecSuccess, let data = result as? Data {
            return data
        }
        if status == errSecItemNotFound {
            return nil
        }
        // Simulator/CI builds without code signing lack the keychain-access-groups
        // entitlement and return errSecMissingEntitlement (-34018). Treat as "no
        // stored key" so the app generates ephemeral keys for the session.
        // Signed device builds never produce this status.
        if status == errSecMissingEntitlement {
            return nil
        }
        throw KeyManagerError.keychainReadFailed(key: key, status: status)
    }

    private static func saveToKeychain(_ data: Data, key: String) throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: keychainService,
            kSecAttrAccount as String: key,
            kSecValueData as String: data,
        ]
        let deleteQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: keychainService,
            kSecAttrAccount as String: key,
        ]
        let deleteStatus = SecItemDelete(deleteQuery as CFDictionary)
        if deleteStatus == errSecMissingEntitlement {
            return
        }
        if deleteStatus != errSecSuccess && deleteStatus != errSecItemNotFound {
            throw KeyManagerError.keychainWriteFailed(key: key, status: deleteStatus)
        }

        let addStatus = SecItemAdd(query as CFDictionary, nil)
        if addStatus == errSecMissingEntitlement {
            return
        }
        if addStatus != errSecSuccess {
            throw KeyManagerError.keychainWriteFailed(key: key, status: addStatus)
        }
    }

    private static func deleteLegacyKeychainItem(key: String) {
        let legacyQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key,
        ]
        SecItemDelete(legacyQuery as CFDictionary)
    }
}
