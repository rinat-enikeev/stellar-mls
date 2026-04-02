import Foundation
import UIKit

/// In-memory + encrypted disk cache for decrypted images.
/// Key = blobHash (SHA-256 hex of encrypted blob).
/// Disk cache uses StorageEncryption for at-rest protection.
final class ImageCache {
    static let shared = ImageCache()

    private let memoryCache = NSCache<NSString, UIImage>()
    private let cacheDir: URL

    private init() {
        let appSupport = FileManager.default.urls(
            for: .cachesDirectory, in: .userDomainMask
        )[0]
        cacheDir = appSupport.appendingPathComponent("StellarImageCache", isDirectory: true)
        try? FileManager.default.createDirectory(
            at: cacheDir,
            withIntermediateDirectories: true
        )

        memoryCache.countLimit = 50
        memoryCache.totalCostLimit = 50 * 1024 * 1024 // 50MB
    }

    /// Get a cached image by blob hash.
    func image(for blobHash: String) -> UIImage? {
        // Check memory
        if let cached = memoryCache.object(forKey: blobHash as NSString) {
            return cached
        }

        // Check disk
        let fileURL = cacheDir.appendingPathComponent(blobHash)
        guard let encryptedData = try? Data(contentsOf: fileURL),
              let decryptedData = try? StorageEncryption.decrypt(encryptedData),
              let image = UIImage(data: decryptedData) else {
            return nil
        }

        memoryCache.setObject(image, forKey: blobHash as NSString, cost: decryptedData.count)
        return image
    }

    /// Store a decrypted image in both memory and encrypted disk cache.
    func store(_ image: UIImage, imageData: Data, for blobHash: String) {
        memoryCache.setObject(image, forKey: blobHash as NSString, cost: imageData.count)

        // Encrypt and write to disk
        if let encrypted = try? StorageEncryption.encrypt(imageData) {
            let fileURL = cacheDir.appendingPathComponent(blobHash)
            try? encrypted.write(to: fileURL)
        }
    }
}
