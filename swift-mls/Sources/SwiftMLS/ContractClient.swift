import Foundation

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
