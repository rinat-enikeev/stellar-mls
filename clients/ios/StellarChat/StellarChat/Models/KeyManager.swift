import CryptoKit
import Foundation

final class KeyManager: Codable {
    private(set) var secretKey: Data
    private(set) var publicKey: Data

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
        self.publicKey = Self.derivePublicKey(from: self.secretKey)
    }

    var publicKeyHex: String {
        publicKey.map { String(format: "%02x", $0) }.joined()
    }

    func signEventID(_ eventID: Data) -> Data {
        // secp256k1 Schnorr signing using CryptoKit P256 as a placeholder.
        // In production, this would use the Rust bridge (RustBackedNostrSigner).
        // For the demo, we use a simplified HMAC-based signature that produces
        // 64 bytes. Real Nostr validators won't accept this, but the relay will
        // forward the event (relays don't verify signatures on custom kinds).
        let key = SymmetricKey(data: secretKey)
        let mac = HMAC<SHA256>.authenticationCode(for: eventID, using: key)
        var sig = Data(mac)
        sig.append(Data(repeating: 0, count: 32)) // pad to 64 bytes
        return sig
    }

    private static func derivePublicKey(from secretKey: Data) -> Data {
        // Derive a deterministic 32-byte "public key" from the secret key.
        // In production, this uses sep_nostr_derive_public_key (secp256k1 x-only).
        // For the demo, we use SHA-256 of the secret key as the public key.
        let hash = SHA256.hash(data: secretKey)
        return Data(hash)
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
