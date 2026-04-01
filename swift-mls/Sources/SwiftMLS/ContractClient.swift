import CryptoKit
import Foundation
import Security

public struct SEPContractInvocation<Payload: Encodable & Sendable>: Encodable, Sendable {
    public let contractID: String
    public let function: String
    public let payload: Payload

    public init(contractID: String, function: String, payload: Payload) {
        self.contractID = contractID
        self.function = function
        self.payload = payload
    }
}

public protocol SEPContractTransport {
    func invoke<Payload: Encodable & Sendable, Response: Decodable & Sendable>(
        _ invocation: SEPContractInvocation<Payload>,
        responseType: Response.Type
    ) async throws -> Response
}

public struct SEPEmptyResponse: Codable, Equatable, Sendable {
    public init() {}
}

public struct URLSessionSEPContractTransport: SEPContractTransport {
    public let endpoint: URL
    public let session: URLSession
    public let encoder: JSONEncoder
    public let decoder: JSONDecoder

    public init(
        endpoint: URL,
        session: URLSession = .shared,
        encoder: JSONEncoder = JSONEncoder(),
        decoder: JSONDecoder = JSONDecoder()
    ) {
        self.endpoint = endpoint
        self.session = session
        self.encoder = encoder
        self.decoder = decoder
    }

    public func invoke<Payload: Encodable & Sendable, Response: Decodable & Sendable>(
        _ invocation: SEPContractInvocation<Payload>,
        responseType: Response.Type
    ) async throws -> Response {
        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try encoder.encode(invocation)

        let (data, response) = try await session.data(for: request)
        let httpResponse = response as? HTTPURLResponse
        let statusCode = httpResponse?.statusCode ?? -1
        guard (200 ..< 300).contains(statusCode) else {
            let body = String(data: data, encoding: .utf8) ?? "<non-UTF8 body>"
            throw SEPError.invalidResponse(statusCode: statusCode, body: body)
        }

        return try decoder.decode(Response.self, from: data)
    }
}

public struct SEPContractClient {
    public let contractID: String
    public let transport: any SEPContractTransport

    public init(contractID: String, transport: any SEPContractTransport) {
        self.contractID = contractID
        self.transport = transport
    }

    public func createGroup(_ request: SEPCreateGroupRequest) async throws -> SEPSubmissionResponse {
        try await invoke("create_group", payload: request, responseType: SEPSubmissionResponse.self)
    }

    public func updateCommitment(_ request: SEPUpdateCommitmentRequest) async throws -> SEPSubmissionResponse {
        try await invoke("update_commitment", payload: request, responseType: SEPSubmissionResponse.self)
    }

    public func verifyMembership(_ request: SEPVerifyMembershipRequest) async throws -> SEPVerifyMembershipResponse {
        try await invoke("verify_membership", payload: request, responseType: SEPVerifyMembershipResponse.self)
    }

    public func deactivateGroup(_ request: SEPDeactivateGroupRequest) async throws -> SEPSubmissionResponse {
        try await invoke("deactivate_group", payload: request, responseType: SEPSubmissionResponse.self)
    }

    public func getState(groupID: Data) async throws -> SEPCommitmentEntry {
        try await invoke(
            "get_state",
            payload: SEPGetStateRequest(groupID: groupID),
            responseType: SEPCommitmentEntry.self
        )
    }

    public func getHistory(groupID: Data, maxEntries: UInt32) async throws -> [SEPCommitmentEntry] {
        try await invoke(
            "get_history",
            payload: SEPGetHistoryRequest(groupID: groupID, maxEntries: maxEntries),
            responseType: [SEPCommitmentEntry].self
        )
    }

    private func invoke<Payload: Encodable & Sendable, Response: Decodable & Sendable>(
        _ function: String,
        payload: Payload,
        responseType: Response.Type
    ) async throws -> Response {
        try await transport.invoke(
            SEPContractInvocation(contractID: contractID, function: function, payload: payload),
            responseType: responseType
        )
    }
}

// MARK: - Relayer Transport (Fee Decoupling)

/// A transport that routes contract invocations through a fee-paying relayer.
///
/// The relayer receives the same JSON payload as a direct Soroban RPC call,
/// but wraps it in a Stellar transaction signed with its own keypair.
/// This prevents identity leakage through the transaction signer address
/// (SEP-XXXX §4.1: fee decoupling).
public struct SEPRelayerTransport: SEPContractTransport {
    public let relayerURL: URL
    public let authToken: String?
    public let session: URLSession
    public let encoder: JSONEncoder
    public let decoder: JSONDecoder

    public init(
        config: SEPRelayerConfig,
        session: URLSession? = nil,
        encoder: JSONEncoder = JSONEncoder(),
        decoder: JSONDecoder = JSONDecoder()
    ) {
        self.relayerURL = config.relayerURL
        self.authToken = config.authToken
        self.encoder = encoder
        self.decoder = decoder

        // If certificate pinning hashes are provided, create a pinned session (H-14)
        if let session {
            self.session = session
        } else if !config.pinnedCertificateHashes.isEmpty {
            let delegate = SEPCertificatePinningDelegate(pinnedHashes: config.pinnedCertificateHashes)
            self.session = URLSession(configuration: .default, delegate: delegate, delegateQueue: nil)
        } else {
            self.session = .shared
        }
    }

    public func invoke<Payload: Encodable & Sendable, Response: Decodable & Sendable>(
        _ invocation: SEPContractInvocation<Payload>,
        responseType: Response.Type
    ) async throws -> Response {
        var request = URLRequest(url: relayerURL)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let token = authToken {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
        request.httpBody = try encoder.encode(invocation)

        let (data, response) = try await session.data(for: request)
        let httpResponse = response as? HTTPURLResponse
        let statusCode = httpResponse?.statusCode ?? -1
        guard (200 ..< 300).contains(statusCode) else {
            let body = String(data: data, encoding: .utf8) ?? "<non-UTF8 body>"
            throw SEPError.invalidResponse(statusCode: statusCode, body: body)
        }

        return try decoder.decode(Response.self, from: data)
    }
}

// MARK: - TLS Certificate Pinning (H-14)

/// URLSession delegate that validates the server's TLS certificate public key
/// against a set of pinned SHA-256 hashes. Rejects connections to relayers
/// whose certificate doesn't match, preventing MITM interception of proofs.
final class SEPCertificatePinningDelegate: NSObject, URLSessionDelegate {
    private let pinnedHashes: Set<String>

    init(pinnedHashes: [String]) {
        self.pinnedHashes = Set(pinnedHashes)
    }

    func urlSession(
        _ session: URLSession,
        didReceive challenge: URLAuthenticationChallenge
    ) async -> (URLSession.AuthChallengeDisposition, URLCredential?) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
              let serverTrust = challenge.protectionSpace.serverTrust,
              let chain = SecTrustCopyCertificateChain(serverTrust) as? [SecCertificate],
              let certificate = chain.first
        else {
            return (.cancelAuthenticationChallenge, nil)
        }

        guard let publicKey = SecCertificateCopyKey(certificate),
              let publicKeyData = SecKeyCopyExternalRepresentation(publicKey, nil) as? Data
        else {
            return (.cancelAuthenticationChallenge, nil)
        }

        let hash = SHA256.hash(data: publicKeyData)
        let hashBase64 = Data(hash).base64EncodedString()

        if pinnedHashes.contains(hashBase64) {
            return (.useCredential, URLCredential(trust: serverTrust))
        } else {
            return (.cancelAuthenticationChallenge, nil)
        }
    }
}
