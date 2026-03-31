import Foundation

public enum SEPError: Error, LocalizedError, Equatable, Sendable {
    case invalidFieldByteLength(expected: Int, actual: Int)
    case invalidSaltLength(actual: Int)
    case invalidPublicKeyLength(index: Int, actual: Int)
    case invalidLeafHashLength(index: Int, actual: Int)
    case invalidProverIndex(Int)
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
        case let .ffiFailure(message):
            return message
        case let .invalidResponse(statusCode, body):
            return "HTTP \(statusCode): \(body)"
        }
    }
}
