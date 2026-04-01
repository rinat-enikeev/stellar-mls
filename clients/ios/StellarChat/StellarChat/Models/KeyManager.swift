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
    /// Ed25519 private key for Stellar, derived from nostrSecretKey via HKDF.
    private let stellarPrivateKey: Curve25519.Signing.PrivateKey
    /// X25519 private key for invitation ECDH, derived from nostrSecretKey via HKDF.
    private let keyAgreementKey: Curve25519.KeyAgreement.PrivateKey

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
    func stellarSign(_ message: Data) -> Data {
        let signature = try! stellarPrivateKey.signature(for: message)
        return Data(signature)
    }

    /// Derive a deterministic Ed25519 private key from the Nostr secret via HKDF.
    private static func deriveStellarKey(from nostrSecret: Data) -> Curve25519.Signing.PrivateKey {
        let derived = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: nostrSecret),
            salt: Data("com.stellarmls.chat".utf8),
            info: Data("stellar-ed25519-v1".utf8),
            outputByteCount: 32
        )
        let seed = derived.withUnsafeBytes { Data($0) }
        return try! Curve25519.Signing.PrivateKey(rawRepresentation: seed)
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
            salt: Data("com.stellarmls.chat".utf8),
            info: Data("x25519-key-agreement-v1".utf8),
            outputByteCount: 32
        )
        let seed = derived.withUnsafeBytes { Data($0) }
        return try! Curve25519.KeyAgreement.PrivateKey(rawRepresentation: seed)
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

    /// Create a spec-compliant attestation binding the BLS group key to the Stellar Ed25519 key.
    /// Signature: Ed25519 over SHA-256("SEP-XXXX:key-binding" || bls_pubkey).
    func createAttestation() throws -> KeyAttestation {
        let blsPub = try blsPublicKey
        let bindingMessage = KeyAttestation.bindingMessage(blsPubkey: blsPub)
        let signature = stellarSign(bindingMessage)
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
