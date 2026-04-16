import Foundation

public struct RustBackedNostrSigner: SEPNostrEventSigner, Sendable {
    public let secretKey: Data

    public init(secretKey: Data) throws {
        if secretKey.count != 32 {
            throw SEPError.invalidNostrSecretKeyLength(actual: secretKey.count)
        }
        self.secretKey = secretKey
    }

    public func publicKey() throws -> Data {
        try RustBridge.deriveNostrPublicKey(secretKey: secretKey)
    }

    public func signEventID(_ eventID: Data) throws -> Data {
        if eventID.count != 32 {
            throw SEPError.invalidNostrEventIDLength(actual: eventID.count)
        }
        return try RustBridge.signNostrEventID(secretKey: secretKey, eventID: eventID)
    }

    /// Fresh per-event signer backed by a random 32-byte secp256k1 scalar from the OS CSPRNG.
    public static func ephemeral() throws -> RustBackedNostrSigner {
        var bytes = [UInt8](repeating: 0, count: 32)
        let status = SecRandomCopyBytes(kSecRandomDefault, 32, &bytes)
        guard status == errSecSuccess else {
            throw SEPError.ffiFailure("SecRandomCopyBytes failed: OSStatus \(status)")
        }
        return try RustBackedNostrSigner(secretKey: Data(bytes))
    }
}
