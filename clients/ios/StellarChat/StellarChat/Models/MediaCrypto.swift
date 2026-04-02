import CryptoKit
import Foundation
import UIKit

/// Per-file AES-256-GCM encryption for media attachments.
/// Each file gets a fresh random key to avoid reusing the group key
/// for large binary payloads, minimizing AES-GCM nonce-collision risk.
enum MediaCrypto {
    /// Maximum compressed image size in bytes (2 MB).
    static let maxImageBytes = 2_000_000
    /// Maximum thumbnail dimension in points.
    static let thumbnailMaxDimension: CGFloat = 200
    /// Thumbnail JPEG compression quality.
    static let thumbnailQuality: CGFloat = 0.4

    // MARK: - Encrypt / Decrypt

    /// Encrypt data with a fresh random AES-256-GCM key.
    /// Returns `(encryptedBlob, key)` where encryptedBlob = nonce(12) || ciphertext || tag(16).
    static func encryptMedia(_ data: Data) throws -> (encryptedBlob: Data, key: Data) {
        var keyBytes = [UInt8](repeating: 0, count: 32)
        guard SecRandomCopyBytes(kSecRandomDefault, 32, &keyBytes) == errSecSuccess else {
            throw MediaCryptoError.keyGenerationFailed
        }
        let key = SymmetricKey(data: keyBytes)
        let encrypted = try encryptMedia(data, key: key)
        return (encrypted, Data(keyBytes))
    }

    /// Encrypt data with a provided AES-256-GCM key (used for thumbnails sharing the file key).
    /// Returns combined format: nonce(12) || ciphertext || tag(16).
    static func encryptMedia(_ data: Data, key: SymmetricKey) throws -> Data {
        let sealed = try AES.GCM.seal(data, using: key)
        guard let combined = sealed.combined else {
            throw MediaCryptoError.encryptionFailed
        }
        return combined
    }

    /// Convenience overload accepting raw key bytes.
    static func encryptMedia(_ data: Data, key keyData: Data) throws -> Data {
        try encryptMedia(data, key: SymmetricKey(data: keyData))
    }

    /// Decrypt combined-format blob: nonce(12) || ciphertext || tag(16).
    static func decryptMedia(_ encryptedBlob: Data, key keyData: Data) throws -> Data {
        let symmetricKey = SymmetricKey(data: keyData)
        let box = try AES.GCM.SealedBox(combined: encryptedBlob)
        return try AES.GCM.open(box, using: symmetricKey)
    }

    // MARK: - Image Processing

    /// Compress an image to JPEG within `maxBytes`.
    /// Progressively reduces quality until the size limit is met.
    static func compressImage(_ imageData: Data, maxBytes: Int = maxImageBytes) -> Data? {
        guard let image = UIImage(data: imageData) else { return nil }
        var quality: CGFloat = 0.7
        while quality >= 0.1 {
            if let jpeg = image.jpegData(compressionQuality: quality),
               jpeg.count <= maxBytes {
                return jpeg
            }
            quality -= 0.1
        }
        // Last resort: lowest quality
        return image.jpegData(compressionQuality: 0.1)
    }

    /// Generate a small JPEG thumbnail.
    static func generateThumbnail(_ imageData: Data, maxDimension: CGFloat = thumbnailMaxDimension) -> Data? {
        guard let image = UIImage(data: imageData) else { return nil }
        let size = image.size
        let scale: CGFloat
        if size.width > size.height {
            scale = maxDimension / size.width
        } else {
            scale = maxDimension / size.height
        }
        let newSize = CGSize(width: size.width * scale, height: size.height * scale)
        let renderer = UIGraphicsImageRenderer(size: newSize)
        let thumbnail = renderer.image { _ in
            image.draw(in: CGRect(origin: .zero, size: newSize))
        }
        return thumbnail.jpegData(compressionQuality: thumbnailQuality)
    }

    /// Get pixel dimensions of an image.
    static func imageDimensions(_ imageData: Data) -> (width: Int, height: Int)? {
        guard let image = UIImage(data: imageData) else { return nil }
        return (Int(image.size.width * image.scale), Int(image.size.height * image.scale))
    }
}

enum MediaCryptoError: LocalizedError {
    case keyGenerationFailed
    case encryptionFailed

    var errorDescription: String? {
        switch self {
        case .keyGenerationFailed: return "Failed to generate encryption key"
        case .encryptionFailed: return "Failed to encrypt media"
        }
    }
}
