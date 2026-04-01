package com.stellarmls.chat.crypto

import android.content.Context
import com.stellarmls.chat.model.toHex
import com.stellarmls.mls.RustBackedNostrSigner
import com.stellarmls.mls.SEPCommitmentBuilder
import com.stellarmls.mls.SEPGroupMemberLeaf
import java.security.SecureRandom

class KeyManager(context: Context) {
    val secretKey: ByteArray
    val publicKey: ByteArray
    val publicKeyHex: String
    private val signer: RustBackedNostrSigner

    init {
        val prefs = context.getSharedPreferences("stellar_keys", Context.MODE_PRIVATE)
        val stored = prefs.getString("nostr_secret_key", null)

        secretKey = if (stored != null) {
            stored.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
        } else {
            val key = ByteArray(32)
            SecureRandom().nextBytes(key)
            prefs.edit().putString("nostr_secret_key", key.toHex()).apply()
            key
        }

        signer = RustBackedNostrSigner(secretKey)
        publicKey = signer.publicKey()
        publicKeyHex = publicKey.toHex()
    }

    /** Sign a 32-byte event ID with secp256k1 Schnorr. Returns 64-byte signature. */
    fun signEventID(eventID: ByteArray): ByteArray = signer.signEventId(eventID)

    /** BLS12-381 leaf hash for SEP Merkle tree. */
    fun leafHash(): ByteArray = SEPCommitmentBuilder.computeLeafHash(secretKey)

    /** BLS12-381 compressed public key (48 bytes). */
    fun blsPublicKey(): ByteArray = SEPCommitmentBuilder.computePublicKey(secretKey)

    /** SEP group member leaf for Merkle tree construction. */
    fun memberLeaf(): SEPGroupMemberLeaf = SEPGroupMemberLeaf(
        publicKeyCompressed = blsPublicKey(),
        leafHash = leafHash()
    )
}
