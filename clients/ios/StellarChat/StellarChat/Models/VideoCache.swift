import Foundation

/// Disk-only encrypted cache for decrypted video files.
/// Key = blobHash (SHA-256 hex of encrypted blob).
/// No in-memory cache — videos are too large for NSCache.
final class VideoCache {
    static let shared = VideoCache()

    private let cacheDir: URL
    private let tempDir: URL

    private init() {
        let caches = FileManager.default.urls(
            for: .cachesDirectory, in: .userDomainMask
        )[0]
        cacheDir = caches.appendingPathComponent("StellarVideoCache", isDirectory: true)
        tempDir = caches.appendingPathComponent("StellarVideoTemp", isDirectory: true)
        try? FileManager.default.createDirectory(at: cacheDir, withIntermediateDirectories: true)
        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
    }

    /// Check if a video is cached. Returns a decrypted temp file URL for playback, or nil.
    func cachedFileURL(for blobHash: String) -> URL? {
        let encFile = cacheDir.appendingPathComponent(blobHash)
        guard FileManager.default.fileExists(atPath: encFile.path) else { return nil }
        // Decrypt to a temp file for playback
        guard let encrypted = try? Data(contentsOf: encFile),
              let decrypted = try? StorageEncryption.decrypt(encrypted) else { return nil }
        let playbackURL = tempDir.appendingPathComponent(blobHash).appendingPathExtension("mp4")
        try? decrypted.write(to: playbackURL)
        return playbackURL
    }

    /// Store decrypted video data to encrypted disk cache. Returns a temp file URL for immediate playback.
    func store(_ videoData: Data, for blobHash: String) -> URL {
        // Encrypt and save to disk cache
        if let encrypted = try? StorageEncryption.encrypt(videoData) {
            let encFile = cacheDir.appendingPathComponent(blobHash)
            try? encrypted.write(to: encFile)
        }
        // Write decrypted to temp for immediate playback
        let playbackURL = tempDir.appendingPathComponent(blobHash).appendingPathExtension("mp4")
        try? videoData.write(to: playbackURL)
        return playbackURL
    }
}
