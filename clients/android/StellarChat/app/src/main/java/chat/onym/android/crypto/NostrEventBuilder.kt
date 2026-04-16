package chat.onym.android.crypto

import chat.onym.android.model.toHex
import com.stellarmls.mls.RustBackedNostrSigner
import org.json.JSONArray
import java.security.MessageDigest

data class NostrEvent(
    val id: String,
    val pubkey: String,
    val createdAt: Long,
    val kind: Int,
    val tags: List<List<String>>,
    val content: String,
    val sig: String
) {
    /** App-specific millisecond timestamp from `["ms", "..."]` tag.
     *  Falls back to `createdAt * 1000` if absent or invalid.
     *  This is an app-local convention — relays and third-party clients may ignore it. */
    val displayMilliseconds: Long
        get() {
            val msTag = tags.firstOrNull { it.size >= 2 && it[0] == "ms" }
            val ms = msTag?.get(1)?.toLongOrNull()
            return if (ms != null && ms >= 0) ms else createdAt * 1000
        }

    fun toJson(): org.json.JSONObject = org.json.JSONObject().apply {
        put("id", id)
        put("pubkey", pubkey)
        put("created_at", createdAt)
        put("kind", kind)
        put("tags", JSONArray(tags.map { JSONArray(it) }))
        put("content", content)
        put("sig", sig)
    }

    /** N-7: Verify event ID integrity by recomputing from canonical JSON. */
    fun verifyEventID(): Boolean {
        val canonical = JSONArray().apply {
            put(0)
            put(pubkey)
            put(createdAt)
            put(kind)
            put(JSONArray(tags.map { JSONArray(it) }))
            put(content)
        }
        val serialized = canonical.toString().toByteArray()
        val hash = java.security.MessageDigest.getInstance("SHA-256").digest(serialized)
        return hash.toHex() == id
    }
}

object NostrEventBuilder {
    /** Build a NIP-01 event with computed ID and real Schnorr signature.
     *  Appends an app-specific `["ms", "<unix_ms>"]` tag for sub-second ordering.
     *
     *  When [ephemeralSigner] is non-null, the outer pubkey and Schnorr signature come
     *  from that per-event key instead of the device identity. Used for kinds 44114/34113
     *  to prevent relays from clustering co-membership via stable sender pubkeys —
     *  inner sender authentication is proven by BLS inside ciphertext. */
    fun build(
        kind: Int,
        tags: List<List<String>>,
        content: String,
        keyManager: KeyManager,
        ephemeralSigner: RustBackedNostrSigner? = null
    ): NostrEvent {
        val pubkeyHex = ephemeralSigner?.publicKey()?.toHex() ?: keyManager.publicKeyHex
        val unixMs = System.currentTimeMillis()
        val createdAt = unixMs / 1000

        // Append app-specific ms tag for sub-second ordering
        val allTags = tags + listOf(listOf("ms", unixMs.toString()))

        // NIP-01: event ID = SHA256([0, pubkey, created_at, kind, tags, content])
        val canonical = JSONArray().apply {
            put(0)
            put(pubkeyHex)
            put(createdAt)
            put(kind)
            put(JSONArray(allTags.map { JSONArray(it) }))
            put(content)
        }
        val serialized = canonical.toString().toByteArray()
        val hash = MessageDigest.getInstance("SHA-256").digest(serialized)
        val eventIDHex = hash.toHex()

        // Real secp256k1 Schnorr signature via Rust FFI
        val signature = ephemeralSigner?.signEventId(hash) ?: keyManager.signEventID(hash)
        val sigHex = signature.toHex()

        return NostrEvent(
            id = eventIDHex,
            pubkey = pubkeyHex,
            createdAt = createdAt,
            kind = kind,
            tags = allTags,
            content = content,
            sig = sigHex
        )
    }
}
