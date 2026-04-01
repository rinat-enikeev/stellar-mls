import Foundation
import os.log

/// Persistent WebSocket connection to a Nostr relay.
/// Supports publishing events and subscribing with filters.
/// M-8: Includes configurable timeout, heartbeat ping, and exponential backoff reconnection.
actor NostrRelayConnection {
    let url: URL
    private var webSocketTask: URLSessionWebSocketTask?
    private let session: URLSession
    private var subscriptions: [String: ([String: Any], (NostrEvent) -> Void)] = [:]
    private var isConnected = false
    private var continuations: [AsyncStream<NostrEvent>.Continuation] = []
    private var reconnectAttempts = 0
    private var pingTask: Task<Void, Never>?

    /// Maximum reconnect delay in seconds (caps exponential backoff).
    private static let maxReconnectDelay: TimeInterval = 120
    /// Base reconnect delay in seconds.
    private static let baseReconnectDelay: TimeInterval = 1
    /// Heartbeat ping interval in seconds.
    private static let pingInterval: TimeInterval = 30
    /// Connection timeout in seconds.
    private static let connectionTimeout: TimeInterval = 15

    init(url: URL) {
        self.url = url
        let config = URLSessionConfiguration.default
        config.waitsForConnectivity = true
        config.timeoutIntervalForRequest = Self.connectionTimeout
        config.timeoutIntervalForResource = Self.connectionTimeout * 4
        self.session = URLSession(configuration: config)
    }

    func connect() {
        guard webSocketTask == nil else { return }
        let task = session.webSocketTask(with: url)
        self.webSocketTask = task
        task.resume()
        isConnected = true
        reconnectAttempts = 0
        Task { await receiveLoop() }
        startHeartbeat()

        // Resubscribe existing subscriptions
        for (subID, (filter, _)) in subscriptions {
            sendREQ(subscriptionID: subID, filter: filter)
        }
    }

    func disconnect() {
        pingTask?.cancel()
        pingTask = nil
        webSocketTask?.cancel(with: .normalClosure, reason: nil)
        webSocketTask = nil
        isConnected = false
        reconnectAttempts = 0
    }

    /// Publish an event to the relay.
    func publish(event: NostrEvent) async throws {
        let frame: [Any] = ["EVENT", event.jsonObject]
        let data = try JSONSerialization.data(withJSONObject: frame)
        let string = String(data: data, encoding: .utf8)!
        try await webSocketTask?.send(.string(string))
    }

    /// Subscribe to events matching a filter. Returns an AsyncStream of events.
    func subscribe(
        subscriptionID: String,
        filter: [String: Any]
    ) -> AsyncStream<NostrEvent> {
        let stream = AsyncStream<NostrEvent> { continuation in
            subscriptions[subscriptionID] = (filter, { event in
                continuation.yield(event)
            })
            continuation.onTermination = { @Sendable _ in
                Task { [weak self] in
                    await self?.unsubscribe(subscriptionID: subscriptionID)
                }
            }
        }
        sendREQ(subscriptionID: subscriptionID, filter: filter)
        return stream
    }

    func unsubscribe(subscriptionID: String) {
        subscriptions.removeValue(forKey: subscriptionID)
        let frame: [Any] = ["CLOSE", subscriptionID]
        if let data = try? JSONSerialization.data(withJSONObject: frame),
           let string = String(data: data, encoding: .utf8)
        {
            Task { try? await webSocketTask?.send(.string(string)) }
        }
    }

    // MARK: - Private

    private func sendREQ(subscriptionID: String, filter: [String: Any]) {
        let frame: [Any] = ["REQ", subscriptionID, filter]
        guard let data = try? JSONSerialization.data(withJSONObject: frame),
              let string = String(data: data, encoding: .utf8)
        else { return }
        Task { try? await webSocketTask?.send(.string(string)) }
    }

    private func receiveLoop() async {
        while isConnected {
            guard let task = webSocketTask else { break }
            do {
                let message = try await task.receive()
                let text: String
                switch message {
                case .string(let s): text = s
                case .data(let d): text = String(data: d, encoding: .utf8) ?? ""
                @unknown default: continue
                }
                handleMessage(text)
            } catch {
                // Connection lost — reconnect with exponential backoff (M-8)
                // Cancel heartbeat first to prevent pings on a dead task.
                pingTask?.cancel()
                pingTask = nil
                isConnected = false
                webSocketTask?.cancel(with: .abnormalClosure, reason: nil)
                webSocketTask = nil
                reconnectAttempts += 1
                let delay = min(
                    Self.maxReconnectDelay,
                    Self.baseReconnectDelay * pow(2.0, Double(min(reconnectAttempts - 1, 6)))
                )
                try? await Task.sleep(for: .seconds(delay))
                connect()
                return
            }
        }
    }

    /// Periodic heartbeat to detect stale connections (M-8).
    ///
    /// Uses a no-op Nostr CLOSE frame instead of `sendPing` because
    /// `URLSessionWebSocketTask.sendPing` has a CFNetwork bug: the pong
    /// handler fires on CFNetwork's internal dispatch queue after the
    /// task is cancelled, dereferencing a nil `nw_connection` (SEGFAULT).
    /// A regular `.send()` doesn't have this issue — if the connection
    /// is dead, the send throws and the receive loop handles reconnection.
    private func startHeartbeat() {
        pingTask?.cancel()
        pingTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(Self.pingInterval))
                guard !Task.isCancelled else { break }
                guard let task = await self?.webSocketTask,
                      task.state == .running else { break }
                // CLOSE for a non-existent subscription is a harmless no-op on any relay.
                try? await task.send(.string("[\"CLOSE\",\"__hb\"]"))
            }
        }
    }

    /// N-18: Maximum incoming WebSocket message size (1 MB).
    private static let maxMessageSize = 1_048_576
    private static let securityLogger = Logger(subsystem: "com.stellarmls.chat", category: "Security")

    private func handleMessage(_ text: String) {
        // N-18: Reject oversized messages to prevent memory exhaustion from malicious relays
        guard text.utf8.count <= Self.maxMessageSize else {
            Self.securityLogger.warning("Relay oversized message rejected (\(text.utf8.count) bytes): \(self.url.absoluteString, privacy: .public)")
            return
        }
        guard let data = text.data(using: .utf8),
              let array = try? JSONSerialization.jsonObject(with: data) as? [Any],
              let kind = array.first as? String
        else { return }

        switch kind {
        case "EVENT":
            guard array.count >= 3,
                  let subID = array[1] as? String,
                  let eventObj = array[2] as? [String: Any]
            else { return }
            if let event = parseEvent(eventObj),
               let (_, callback) = subscriptions[subID]
            {
                callback(event)
            }
        case "EOSE":
            break // End of stored events, no action needed
        case "OK":
            break // Publish acknowledgement
        case "NOTICE":
            break // Relay notice
        default:
            break
        }
    }

    private func parseEvent(_ obj: [String: Any]) -> NostrEvent? {
        guard let id = obj["id"] as? String,
              let pubkey = obj["pubkey"] as? String,
              let createdAt = obj["created_at"] as? Int64,
              let kind = obj["kind"] as? Int,
              let tags = obj["tags"] as? [[String]],
              let content = obj["content"] as? String,
              let sig = obj["sig"] as? String
        else { return nil }

        let event = NostrEvent(
            id: id,
            pubkey: pubkey,
            createdAt: createdAt,
            kind: kind,
            tags: tags,
            content: content,
            sig: sig
        )

        // N-7: Verify event ID integrity before processing.
        if !event.verifyEventID() {
            Self.securityLogger.warning("Relay invalid event ID: \(self.url.absoluteString, privacy: .public)")
            return nil
        }

        return event
    }
}
