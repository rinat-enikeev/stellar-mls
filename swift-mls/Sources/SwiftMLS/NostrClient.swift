import Foundation

public struct URLSessionSEPNostrRelayTransport: SEPNostrRelayTransport {
    public let session: URLSession

    public init(session: URLSession? = nil) {
        self.session = session ?? Self.makeEphemeralSession()
    }

    public func publish(event: SEPNostrEvent, to relayURL: URL) async throws -> SEPNostrRelaySendResult {
        guard let scheme = relayURL.scheme?.lowercased(), scheme == "ws" || scheme == "wss" else {
            throw SEPError.invalidRelayURL(relayURL.absoluteString)
        }

        let task = session.webSocketTask(with: relayURL)
        task.resume()
        defer {
            task.cancel(with: .normalClosure, reason: nil)
        }

        let frame = try makeEventFrame(for: event)
        try await task.send(.string(frame))

        let message = try await task.receive()
        let payload: String
        switch message {
        case let .string(text):
            payload = text
        case let .data(data):
            payload = String(decoding: data, as: UTF8.self)
        @unknown default:
            throw SEPError.invalidRelayResponse("Relay returned an unknown WebSocket frame type")
        }

        return try parseRelayResponse(payload, relayURL: relayURL, expectedEventID: event.id)
    }

    private func makeEventFrame(for event: SEPNostrEvent) throws -> String {
        let object: [Any] = ["EVENT", event.jsonObject]
        let json = try JSONSerialization.data(withJSONObject: object, options: [])
        return String(decoding: json, as: UTF8.self)
    }

    private func parseRelayResponse(
        _ payload: String,
        relayURL: URL,
        expectedEventID: String
    ) throws -> SEPNostrRelaySendResult {
        guard let data = payload.data(using: .utf8),
              let object = try JSONSerialization.jsonObject(with: data) as? [Any],
              let kind = object.first as? String
        else {
            throw SEPError.invalidRelayResponse(payload)
        }

        switch kind {
        case "OK":
            guard object.count >= 4,
                  let eventID = object[1] as? String,
                  let accepted = object[2] as? Bool
            else {
                throw SEPError.invalidRelayResponse(payload)
            }

            if eventID != expectedEventID {
                throw SEPError.invalidRelayResponse("Relay ACK event id mismatch: \(payload)")
            }

            let message = object[3] as? String
            return SEPNostrRelaySendResult(relayURL: relayURL, accepted: accepted, message: message)

        case "NOTICE":
            let message = object.count > 1 ? object[1] as? String : nil
            return SEPNostrRelaySendResult(relayURL: relayURL, accepted: false, message: message)

        default:
            throw SEPError.invalidRelayResponse(payload)
        }
    }

    private static func makeEphemeralSession() -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.urlCache = nil
        configuration.httpCookieStorage = nil
        configuration.httpShouldSetCookies = false
        return URLSession(configuration: configuration)
    }
}

extension SEPNostrEvent {
    var jsonObject: [String: Any] {
        [
            "id": id,
            "pubkey": pubkey,
            "created_at": createdAt,
            "kind": kind,
            "tags": tags,
            "content": content,
            "sig": sig,
        ]
    }
}
