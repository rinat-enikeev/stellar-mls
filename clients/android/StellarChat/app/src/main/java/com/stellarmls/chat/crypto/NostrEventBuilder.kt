package com.stellarmls.chat.crypto

import com.stellarmls.chat.model.toHex
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
    /** Build a NIP-01 event with computed ID and real Schnorr signature. */
    fun build(
        kind: Int,
        tags: List<List<String>>,
        content: String,
        keyManager: KeyManager
    ): NostrEvent {
        val pubkeyHex = keyManager.publicKeyHex
        val createdAt = System.currentTimeMillis() / 1000

        // NIP-01: event ID = SHA256([0, pubkey, created_at, kind, tags, content])
        val canonical = JSONArray().apply {
            put(0)
            put(pubkeyHex)
            put(createdAt)
            put(kind)
            put(JSONArray(tags.map { JSONArray(it) }))
            put(content)
        }
        val serialized = canonical.toString().toByteArray()
        val hash = MessageDigest.getInstance("SHA-256").digest(serialized)
        val eventIDHex = hash.toHex()

        // Real secp256k1 Schnorr signature via Rust FFI
        val signature = keyManager.signEventID(hash)
        val sigHex = signature.toHex()

        return NostrEvent(
            id = eventIDHex,
            pubkey = pubkeyHex,
            createdAt = createdAt,
            kind = kind,
            tags = tags,
            content = content,
            sig = sigHex
        )
    }
}
