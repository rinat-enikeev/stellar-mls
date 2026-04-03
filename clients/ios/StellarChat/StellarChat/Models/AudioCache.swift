import Foundation

/// In-memory cache for decrypted voice message audio data.
/// Voice messages are small (< 10 MB) so disk caching is unnecessary.
final class AudioCache {
    static let shared = AudioCache()

    private let cache = NSCache<NSString, NSData>()

    private init() {
        cache.countLimit = 30
        cache.totalCostLimit = 30 * 1024 * 1024 // 30 MB
    }

    func data(for blobHash: String) -> Data? {
        cache.object(forKey: blobHash as NSString) as Data?
    }

    func store(_ data: Data, for blobHash: String) {
        cache.setObject(data as NSData, forKey: blobHash as NSString, cost: data.count)
    }
}
