package chat.onym.android.blossom

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.LruCache
import chat.onym.android.crypto.StorageEncryption
import java.io.ByteArrayOutputStream
import java.io.File

/**
 * In-memory + encrypted disk cache for decrypted images.
 * Key = blobHash (SHA-256 hex of encrypted blob).
 * Disk cache uses StorageEncryption for at-rest protection.
 */
class ImageCache private constructor(context: Context) {
    private val memoryCache: LruCache<String, Bitmap>
    private val cacheDir: File

    init {
        val maxMemory = (Runtime.getRuntime().maxMemory() / 1024).toInt()
        val cacheSize = maxMemory / 8 // 1/8 of available memory
        memoryCache = object : LruCache<String, Bitmap>(cacheSize) {
            override fun sizeOf(key: String, bitmap: Bitmap): Int {
                return bitmap.byteCount / 1024
            }
        }
        cacheDir = File(context.cacheDir, "StellarImageCache").also { it.mkdirs() }
    }

    /** Get a cached bitmap by blob hash. */
    fun getBitmap(blobHash: String): Bitmap? {
        // Check memory
        memoryCache.get(blobHash)?.let { return it }

        // Check disk
        val file = File(cacheDir, blobHash)
        if (!file.exists()) return null
        return try {
            val encrypted = file.readBytes()
            val decrypted = StorageEncryption.decrypt(encrypted)
            val bitmap = BitmapFactory.decodeByteArray(decrypted, 0, decrypted.size) ?: return null
            memoryCache.put(blobHash, bitmap)
            bitmap
        } catch (_: Exception) {
            null
        }
    }

    /** Store a decrypted image in both memory and encrypted disk cache. */
    fun store(blobHash: String, bitmap: Bitmap, imageData: ByteArray) {
        memoryCache.put(blobHash, bitmap)
        try {
            val encrypted = StorageEncryption.encrypt(imageData)
            File(cacheDir, blobHash).writeBytes(encrypted)
        } catch (_: Exception) { }
    }

    companion object {
        @Volatile
        private var instance: ImageCache? = null

        fun getInstance(context: Context): ImageCache {
            return instance ?: synchronized(this) {
                instance ?: ImageCache(context.applicationContext).also { instance = it }
            }
        }
    }
}
