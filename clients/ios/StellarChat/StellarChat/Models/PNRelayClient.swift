import CryptoKit
import Foundation

/// Client for the privacy-preserving push notification relay.
/// All registrations use ephemeral X25519 keys — the PN relay never learns our Nostr identity.
actor PNRelayClient {
    private let relayURL: URL
    private var relayPublicKey: Curve25519.KeyAgreement.PublicKey?
    private let session = URLSession.shared

    init(relayURL: URL) {
        self.relayURL = relayURL
    }

    /// Fetch the PN relay's X25519 public key from GET /v1/info.
    func fetchInfo() async throws -> Curve25519.KeyAgreement.PublicKey {
        let url = relayURL.appendingPathComponent("v1/info")
        let (data, _) = try await session.data(from: url)
        let json = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        guard let pubkeyB64 = json?["x25519_public_key"] as? String,
              let pubkeyData = Data(base64Encoded: pubkeyB64) else {
            throw PNRelayError.invalidInfoResponse
        }
        let key = try Curve25519.KeyAgreement.PublicKey(rawRepresentation: pubkeyData)
        relayPublicKey = key
        return key
    }

    /// Register a push subscription for a single group topic.
    func subscribe(
        subscriptionID: String,
        filter: [String: Any],
        pushToken: String,
        notificationKey: Data,
        platform: String = "ios"
    ) async throws {
        let relayPubkey = try await getRelayPublicKey()

        let payload: [String: Any] = [
            "action": "subscribe",
            "subscription_id": subscriptionID,
            "filter": filter,
            "token": pushToken,
            "notification_key": notificationKey.base64EncodedString(),
            "platform": platform
        ]

        try await sendEncryptedEnvelope(payload: payload, relayPubkey: relayPubkey)
    }

    /// Update an existing subscription's filter (e.g., after topic rotation).
    func update(
        subscriptionID: String,
        filter: [String: Any]
    ) async throws {
        let relayPubkey = try await getRelayPublicKey()

        let payload: [String: Any] = [
            "action": "update",
            "subscription_id": subscriptionID,
            "filter": filter
        ]

        try await sendEncryptedEnvelope(payload: payload, relayPubkey: relayPubkey)
    }

    /// Unsubscribe from push notifications.
    func unsubscribe(subscriptionID: String) async throws {
        let relayPubkey = try await getRelayPublicKey()

        let payload: [String: Any] = [
            "action": "unsubscribe",
            "subscription_id": subscriptionID
        ]

        try await sendEncryptedEnvelope(payload: payload, relayPubkey: relayPubkey)
    }

    // MARK: - Private

    private func getRelayPublicKey() async throws -> Curve25519.KeyAgreement.PublicKey {
        if let key = relayPublicKey { return key }
        return try await fetchInfo()
    }

    /// Encrypt a registration payload using ephemeral X25519 + AES-256-GCM and POST it.
    private func sendEncryptedEnvelope(
        payload: [String: Any],
        relayPubkey: Curve25519.KeyAgreement.PublicKey
    ) async throws {
        let payloadData = try JSONSerialization.data(withJSONObject: payload)
        let envelope = try GroupCrypto.encryptInvitation(
            payloadData,
            recipientKeyAgreementPubkey: Data(relayPubkey.rawRepresentation),
            senderSigningKey: nil  // No signing — anonymous registration
        )
        let envelopeJSON = try JSONEncoder().encode(envelope)

        var request = URLRequest(url: relayURL.appendingPathComponent("v1/subscription"))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = envelopeJSON
        request.timeoutInterval = 15

        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse,
              (200...299).contains(httpResponse.statusCode) else {
            let body = String(data: data, encoding: .utf8) ?? ""
            throw PNRelayError.serverError(body)
        }
    }
}

enum PNRelayError: LocalizedError {
    case invalidInfoResponse
    case serverError(String)
    case notConfigured

    var errorDescription: String? {
        switch self {
        case .invalidInfoResponse: return "Invalid PN relay info response"
        case .serverError(let msg): return "PN relay error: \(msg)"
        case .notConfigured: return "PN relay URL not configured"
        }
    }
}
