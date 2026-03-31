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
}
