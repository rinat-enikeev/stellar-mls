import AVFoundation
import CryptoKit
import Foundation
import UIKit

/// Per-file AES-256-GCM encryption for media attachments.
/// Each file gets a fresh random key to avoid reusing the group key
/// for large binary payloads, minimizing AES-GCM nonce-collision risk.
enum MediaCrypto {
    /// Maximum compressed image size in bytes (2 MB).
    static let maxImageBytes = 2_000_000
    /// Maximum video size in bytes before encryption (50 MB).
    static let maxVideoBytes = 50_000_000
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

    // MARK: - Video Processing

    /// Extract video metadata: dimensions and duration.
    static func videoMetadata(_ url: URL) async -> (width: Int, height: Int, duration: Int)? {
        let asset = AVURLAsset(url: url)
        guard let track = try? await asset.loadTracks(withMediaType: .video).first else { return nil }
        let size = try? await track.load(.naturalSize)
        let transform = try? await track.load(.preferredTransform)
        let duration = try? await asset.load(.duration)
        guard let size, let duration else { return nil }
        // Apply transform to handle rotated videos
        let transformed = size.applying(transform ?? .identity)
        let w = Int(abs(transformed.width))
        let h = Int(abs(transformed.height))
        let secs = Int(CMTimeGetSeconds(duration))
        return (w, h, secs)
    }

    /// Generate a thumbnail from the first frame of a video.
    static func generateVideoThumbnail(_ url: URL, maxDimension: CGFloat = thumbnailMaxDimension) async -> Data? {
        let asset = AVURLAsset(url: url)
        let generator = AVAssetImageGenerator(asset: asset)
        generator.appliesPreferredTrackTransform = true
        generator.maximumSize = CGSize(width: maxDimension * 2, height: maxDimension * 2)
        guard let cgImage = try? await generator.image(at: .zero).image else { return nil }
        let uiImage = UIImage(cgImage: cgImage)
        // Scale to thumbnail size
        let size = uiImage.size
        let scale: CGFloat = size.width > size.height
            ? maxDimension / size.width
            : maxDimension / size.height
        let newSize = CGSize(width: size.width * scale, height: size.height * scale)
        let renderer = UIGraphicsImageRenderer(size: newSize)
        let thumbnail = renderer.image { _ in
            uiImage.draw(in: CGRect(origin: .zero, size: newSize))
        }
        return thumbnail.jpegData(compressionQuality: thumbnailQuality)
    }

    /// Compress a video to fit within maxBytes using AVAssetExportSession.
    /// Returns the compressed video data, or nil if compression fails.
    static func compressVideo(_ url: URL, maxBytes: Int = maxVideoBytes) async throws -> Data? {
        // Check if original is already small enough
        let attrs = try FileManager.default.attributesOfItem(atPath: url.path)
        let fileSize = attrs[.size] as? Int ?? 0
        if fileSize <= maxBytes {
            return try Data(contentsOf: url)
        }

        // Transcode to medium quality
        let asset = AVURLAsset(url: url)
        guard let session = AVAssetExportSession(asset: asset, presetName: AVAssetExportPresetMediumQuality) else {
            return nil
        }
        let tempURL = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("mp4")
        session.outputURL = tempURL
        session.outputFileType = .mp4
        await session.export()

        defer { try? FileManager.default.removeItem(at: tempURL) }

        guard session.status == .completed else { return nil }
        let data = try Data(contentsOf: tempURL)
        if data.count > maxBytes {
            // Still too large — try low quality
            guard let lowSession = AVAssetExportSession(asset: asset, presetName: AVAssetExportPresetLowQuality) else {
                return nil
            }
            let lowURL = FileManager.default.temporaryDirectory
                .appendingPathComponent(UUID().uuidString)
                .appendingPathExtension("mp4")
            lowSession.outputURL = lowURL
            lowSession.outputFileType = .mp4
            await lowSession.export()
            defer { try? FileManager.default.removeItem(at: lowURL) }
            guard lowSession.status == .completed else { return nil }
            let lowData = try Data(contentsOf: lowURL)
            return lowData.count <= maxBytes ? lowData : nil
        }
        return data
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
