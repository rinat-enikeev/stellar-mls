import Foundation

public enum SEPError: Error, LocalizedError, Equatable, Sendable {
    case invalidFieldByteLength(expected: Int, actual: Int)
    case invalidSaltLength(actual: Int)
    case invalidPublicKeyLength(index: Int, actual: Int)
    case invalidLeafHashLength(index: Int, actual: Int)
    case invalidProverIndex(Int)
    case invalidNostrPublicKeyLength(actual: Int)
    case invalidNostrSignatureLength(actual: Int)
    case invalidNostrSecretKeyLength(actual: Int)
    case invalidNostrEventIDLength(actual: Int)
    case invalidRelayURL(String)
    case invalidRelayResponse(String)
    case emptyRelayList
    case ffiFailure(String)
    case invalidResponse(statusCode: Int, body: String)

    public var errorDescription: String? {
        switch self {
        case let .invalidFieldByteLength(expected, actual):
            return "Expected \(expected)-byte field element, got \(actual) bytes"
        case let .invalidSaltLength(actual):
            return "Expected 32-byte salt, got \(actual) bytes"
        case let .invalidPublicKeyLength(index, actual):
            return "Compressed public key at index \(index) must be 48 bytes, got \(actual)"
        case let .invalidLeafHashLength(index, actual):
            return "Leaf hash at index \(index) must be 32 bytes, got \(actual)"
        case let .invalidProverIndex(index):
            return "Invalid prover index \(index)"
        case let .invalidNostrPublicKeyLength(actual):
            return "Nostr public key must be 32 bytes, got \(actual)"
        case let .invalidNostrSignatureLength(actual):
            return "Nostr signature must be 64 bytes, got \(actual)"
        case let .invalidNostrSecretKeyLength(actual):
            return "Nostr secret key must be 32 bytes, got \(actual)"
        case let .invalidNostrEventIDLength(actual):
            return "Nostr event id must be 32 bytes, got \(actual)"
        case let .invalidRelayURL(url):
            return "Relay URL must use ws or wss: \(url)"
        case let .invalidRelayResponse(response):
            return "Invalid relay response: \(response)"
        case .emptyRelayList:
            return "At least one relay URL is required"
        case let .ffiFailure(message):
            return message
        case let .invalidResponse(statusCode, body):
            return "HTTP \(statusCode): \(body)"
        }
    }
}
