package com.stellarmls.chat.crypto

import android.content.Context
import com.stellarmls.chat.model.toHex
import com.stellarmls.mls.RustBackedNostrSigner
import com.stellarmls.mls.SEPCommitmentBuilder
import com.stellarmls.mls.SEPGroupMemberLeaf
import org.bouncycastle.crypto.params.Ed25519PrivateKeyParameters
import org.bouncycastle.crypto.params.Ed25519PublicKeyParameters
import org.bouncycastle.crypto.params.X25519PrivateKeyParameters
import org.bouncycastle.crypto.params.X25519PublicKeyParameters
import org.bouncycastle.crypto.signers.Ed25519Signer
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import java.security.SecureRandom

class KeyManager(context: Context) {
    val secretKey: ByteArray
    val publicKey: ByteArray
    val publicKeyHex: String
    private val signer: RustBackedNostrSigner

    /** Independent BLS12-381 secret key (separate from Nostr key per SEP-XXXX §1.1). */
    val blsSecretKey: ByteArray

    /** Ed25519 Stellar identity key (HKDF-derived from Nostr key). */
    val stellarPublicKey: ByteArray
    private val stellarPrivateKeyParams: Ed25519PrivateKeyParameters

    /** X25519 key agreement key (HKDF-derived from Nostr key). */
    val keyAgreementPublicKey: ByteArray
    val keyAgreementPublicKeyHex: String
    val keyAgreementPrivateKey: X25519PrivateKeyParameters

    /** Hidden inbox tag derived from X25519 public key. */
    val inboxTag: String

    init {
        val masterKey = MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()
        val prefs = EncryptedSharedPreferences.create(
            context,
            "stellar_keys_encrypted",
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
        )

        // Nostr secp256k1 key
        val storedNostr = prefs.getString("nostr_secret_key", null)
        secretKey = if (storedNostr != null) {
            storedNostr.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
        } else {
            val key = ByteArray(32)
            SecureRandom().nextBytes(key)
            prefs.edit().putString("nostr_secret_key", key.toHex()).apply()
            key
        }

        signer = RustBackedNostrSigner(secretKey)
        publicKey = signer.publicKey()
        publicKeyHex = publicKey.toHex()

        // Independent BLS12-381 key
        val storedBls = prefs.getString("bls_secret_key", null)
        blsSecretKey = if (storedBls != null) {
            storedBls.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
        } else {
            val key = ByteArray(32)
            SecureRandom().nextBytes(key)
            prefs.edit().putString("bls_secret_key", key.toHex()).apply()
            key
        }

        // Ed25519 Stellar key (HKDF-derived from Nostr secret)
        val stellarSeed = GroupCrypto.hkdf(
            ikm = secretKey,
            salt = "com.stellarmls.chat".toByteArray(),
            info = "stellar-ed25519-v1".toByteArray(),
            length = 32
        )
        stellarPrivateKeyParams = Ed25519PrivateKeyParameters(stellarSeed, 0)
        stellarPublicKey = stellarPrivateKeyParams.generatePublicKey().encoded

        // X25519 key agreement key (HKDF-derived from Nostr secret)
        val x25519Seed = GroupCrypto.hkdf(
            ikm = secretKey,
            salt = "com.stellarmls.chat".toByteArray(),
            info = "x25519-key-agreement-v1".toByteArray(),
            length = 32
        )
        keyAgreementPrivateKey = X25519PrivateKeyParameters(x25519Seed, 0)
        keyAgreementPublicKey = keyAgreementPrivateKey.generatePublicKey().encoded
        keyAgreementPublicKeyHex = keyAgreementPublicKey.toHex()
        inboxTag = GroupCrypto.hiddenInboxTag(keyAgreementPublicKey)
    }

    /** Stellar account ID (G... StrKey encoding of Ed25519 public key). */
    val stellarAccountID: String = StellarStrKey.encodeAccountID(stellarPublicKey)

    /** Create a key attestation binding BLS ↔ Stellar Ed25519 (SEP-XXXX §1.1). */
    fun createAttestation(): KeyAttestation = KeyAttestation.create(this)

    /** Sign a 32-byte event ID with secp256k1 Schnorr. Returns 64-byte signature. */
    fun signEventID(eventID: ByteArray): ByteArray = signer.signEventId(eventID)

    /** Sign a message with Ed25519 (Stellar key). Returns 64-byte signature. */
    fun stellarSign(message: ByteArray): ByteArray {
        val signer = Ed25519Signer()
        signer.init(true, stellarPrivateKeyParams)
        signer.update(message, 0, message.size)
        return signer.generateSignature()
    }

    /** BLS12-381 leaf hash for SEP Merkle tree (uses independent BLS key). */
    fun leafHash(): ByteArray = SEPCommitmentBuilder.computeLeafHash(blsSecretKey)

    /** BLS12-381 compressed public key (48 bytes, uses independent BLS key). */
    fun blsPublicKey(): ByteArray = SEPCommitmentBuilder.computePublicKey(blsSecretKey)

    /** SEP group member leaf for Merkle tree construction. */
    fun memberLeaf(): SEPGroupMemberLeaf = SEPGroupMemberLeaf(
        publicKeyCompressed = blsPublicKey(),
        leafHash = leafHash()
    )

    companion object {
        /** Verify an Ed25519 signature against a public key. */
        fun verifyEd25519(publicKey: ByteArray, message: ByteArray, signature: ByteArray): Boolean {
            val verifier = Ed25519Signer()
            verifier.init(false, Ed25519PublicKeyParameters(publicKey, 0))
            verifier.update(message, 0, message.size)
            return verifier.verifySignature(signature)
        }
    }
}
