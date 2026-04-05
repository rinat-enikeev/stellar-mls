package chat.onym.android.blossom

/**
 * In-memory LRU cache for decrypted voice message audio data.
 * Voice messages are small (< 10 MB) so disk caching is unnecessary.
 */
object AudioCache {
    private val cache = object : LinkedHashMap<String, ByteArray>(16, 0.75f, true) {
        override fun removeEldestEntry(eldest: Map.Entry<String, ByteArray>): Boolean {
            return size > 30
        }
    }

    @Synchronized
    fun get(blobHash: String): ByteArray? = cache[blobHash]

    @Synchronized
    fun put(blobHash: String, data: ByteArray) {
        cache[blobHash] = data
    }
}
