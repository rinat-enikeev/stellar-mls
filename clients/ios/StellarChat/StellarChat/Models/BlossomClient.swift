import CryptoKit
import Foundation

/// Client for Blossom (BL-01) content-addressable blob storage.
/// Files are addressed by SHA-256 hash of their content.
/// Upload authorization uses Nostr-signed kind 24242 events.
enum BlossomClient {
    /// Extended session for large uploads (video). 300s request timeout.
    private static let extendedSession: URLSession = {
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 300
        config.timeoutIntervalForResource = 600
        return URLSession(configuration: config)
    }()

    /// Upload an encrypted blob to Blossom server(s).
    /// Returns the SHA-256 hex hash of the uploaded blob.
    static func upload(
        _ data: Data,
        servers: [URL],
        keyManager: KeyManager
    ) async throws -> String {
        let hash = SHA256.hash(data: data)
        let blobHash = Data(hash).map { String(format: "%02x", $0) }.joined()

        let authEvent = try buildAuthEvent(
            action: "upload",
            blobHash: blobHash,
            keyManager: keyManager
        )
        let authJSON = try JSONSerialization.data(withJSONObject: authEvent.jsonObject)
        let authToken = "Nostr \(authJSON.base64EncodedString())"

        var lastError: Error?
        for server in servers {
            do {
                let url = server.appendingPathComponent("upload")
                var request = URLRequest(url: url)
                request.httpMethod = "PUT"
                request.httpBody = data
                request.setValue(authToken, forHTTPHeaderField: "Authorization")
                request.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")

                let session = data.count > 2_000_000 ? extendedSession : URLSession.shared
                let (responseData, response) = try await session.data(for: request)
                if let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) {
                    return blobHash
                }
                let http = response as? HTTPURLResponse
                let statusCode = http?.statusCode ?? -1
                let reason = http?.value(forHTTPHeaderField: "x-reason") ?? ""
                let body = String(data: responseData, encoding: .utf8) ?? ""
                let detail = reason.isEmpty ? body : reason
                lastError = BlossomError.serverError(server: server.absoluteString, statusCode: statusCode, detail: detail)
            } catch {
                lastError = error
            }
        }

        throw lastError ?? BlossomError.uploadFailed
    }

    /// Download an encrypted blob from Blossom server(s) by its SHA-256 hash.
    static func download(
        blobHash: String,
        servers: [URL]
    ) async throws -> Data {
        var lastError: Error?
        for server in servers {
            do {
                let url = server.appendingPathComponent(blobHash)
                let (data, response) = try await URLSession.shared.data(from: url)
                if let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) {
                    // Verify content hash matches
                    let downloadHash = SHA256.hash(data: data)
                    let downloadHashHex = Data(downloadHash).map { String(format: "%02x", $0) }.joined()
                    guard downloadHashHex == blobHash else {
                        throw BlossomError.hashMismatch
                    }
                    return data
                }
            } catch {
                lastError = error
            }
        }

        throw lastError ?? BlossomError.downloadFailed
    }

    /// Build a BL-01 authorization event (kind 24242).
    private static func buildAuthEvent(
        action: String,
        blobHash: String,
        keyManager: KeyManager
    ) throws -> NostrEvent {
        let expiration = Int64(Date().timeIntervalSince1970) + 300 // 5 minutes

        let tags: [[String]] = [
            ["t", action],
            ["x", blobHash],
            ["expiration", String(expiration)],
        ]

        return try NostrEvent.build(
            kind: 24242,
            tags: tags,
            content: "Upload encrypted media",
            keyManager: keyManager
        )
    }
}

enum BlossomError: LocalizedError {
    case uploadFailed
    case downloadFailed
    case hashMismatch
    case serverError(server: String, statusCode: Int, detail: String)

    var errorDescription: String? {
        switch self {
        case .uploadFailed: return "Failed to upload to any Blossom server"
        case .downloadFailed: return "Failed to download from any Blossom server"
        case .hashMismatch: return "Downloaded blob hash does not match expected hash"
        case .serverError(let server, let statusCode, let detail):
            return "Blossom upload to \(server) failed: HTTP \(statusCode) \(detail)"
        }
    }
}
