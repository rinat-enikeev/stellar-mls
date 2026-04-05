package chat.onym.android.blossom

import android.content.Context
import chat.onym.android.crypto.StorageEncryption
import java.io.File

/**
 * Disk-only encrypted cache for decrypted video files.
 * Key = blobHash (SHA-256 hex of encrypted blob).
 * No in-memory cache — videos are too large for LruCache.
 */
class VideoCache private constructor(context: Context) {
    private val cacheDir: File = File(context.cacheDir, "StellarVideoCache").also { it.mkdirs() }
    private val tempDir: File = File(context.cacheDir, "StellarVideoTemp").also { it.mkdirs() }

    /** Check if a video is cached. Returns a decrypted temp file for playback, or null. */
    fun getCachedFile(blobHash: String): File? {
        val encFile = File(cacheDir, blobHash)
        if (!encFile.exists()) return null
        return try {
            val encrypted = encFile.readBytes()
            val decrypted = StorageEncryption.decrypt(encrypted)
            val playbackFile = File(tempDir, "$blobHash.mp4")
            playbackFile.writeBytes(decrypted)
            playbackFile
        } catch (_: Exception) {
            null
        }
    }

    /** Store decrypted video data to encrypted disk cache. Returns a temp file for immediate playback. */
    fun store(blobHash: String, videoData: ByteArray): File {
        try {
            val encrypted = StorageEncryption.encrypt(videoData)
            File(cacheDir, blobHash).writeBytes(encrypted)
        } catch (_: Exception) { }
        val playbackFile = File(tempDir, "$blobHash.mp4")
        playbackFile.writeBytes(videoData)
        return playbackFile
    }

    companion object {
        @Volatile
        private var instance: VideoCache? = null

        fun getInstance(context: Context): VideoCache {
            return instance ?: synchronized(this) {
                instance ?: VideoCache(context.applicationContext).also { instance = it }
            }
        }
    }
}
