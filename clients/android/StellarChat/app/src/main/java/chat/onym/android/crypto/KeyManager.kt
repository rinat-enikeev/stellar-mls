package chat.onym.android.crypto

import android.content.Context
import chat.onym.android.model.toHex
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

class KeyManagerException(message: String, cause: Throwable? = null) : IllegalStateException(message, cause)

/**
 * M-12: KeyManager initialization (EncryptedSharedPreferences + MasterKey generation)
 * may block the calling thread. Use [KeyManager.createAsync] to initialize on a
 * background thread, or call the constructor from a coroutine on Dispatchers.IO.
 */
class KeyManager private constructor(private val prefs: android.content.SharedPreferences) {
    companion object {
        private const val NOSTR_SECRET_KEY = "nostr_secret_key"
        private const val BLS_SECRET_KEY = "bls_secret_key"
        private const val IDENTITY_INITIALIZED = "identity_initialized"
        suspend fun createAsync(context: Context): KeyManager =
            kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                create(context)
            }

        fun create(context: Context): KeyManager {
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
            return KeyManager(prefs)
        }

        /** Verify an Ed25519 signature against a public key. */
        fun verifyEd25519(publicKey: ByteArray, message: ByteArray, signature: ByteArray): Boolean {
            val verifier = Ed25519Signer()
            verifier.init(false, Ed25519PublicKeyParameters(publicKey, 0))
            verifier.update(message, 0, message.size)
            return verifier.verifySignature(signature)
        }

        private fun decodeHexKey(value: String, keyName: String): ByteArray {
            require(value.length == 64) {
                "Expected 64 hex chars for $keyName, got ${value.length}"
            }
            return value.chunked(2).map { chunk ->
                chunk.toIntOrNull(16)?.toByte()
                    ?: throw IllegalArgumentException("Invalid hex in $keyName")
            }.toByteArray()
        }
    }

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
        val storedNostr = prefs.getString(NOSTR_SECRET_KEY, null)
        val storedBls = prefs.getString(BLS_SECRET_KEY, null)
        val initialized = prefs.getBoolean(IDENTITY_INITIALIZED, false)

        if (storedNostr == null && storedBls == null && !initialized) {
            val key = ByteArray(32)
            SecureRandom().nextBytes(key)
            val blsKey = ByteArray(32)
            SecureRandom().nextBytes(blsKey)
            val saved = prefs.edit()
                .putString(NOSTR_SECRET_KEY, key.toHex())
                .putString(BLS_SECRET_KEY, blsKey.toHex())
                .putBoolean(IDENTITY_INITIALIZED, true)
                .commit()
            if (!saved) {
                throw KeyManagerException("Failed to persist newly generated identity keys")
            }
        } else if (storedNostr != null && storedBls != null) {
            if (!initialized) {
                val marked = prefs.edit().putBoolean(IDENTITY_INITIALIZED, true).commit()
                if (!marked) {
                    throw KeyManagerException("Failed to mark persisted identity as initialized")
                }
            }
        } else {
            throw KeyManagerException(
                "Identity storage is inconsistent: initialized=$initialized nostrPresent=${storedNostr != null} blsPresent=${storedBls != null}"
            )
        }

        val nostrHex = prefs.getString(NOSTR_SECRET_KEY, null)
            ?: throw KeyManagerException("Missing persisted Nostr secret key")
        val blsHex = prefs.getString(BLS_SECRET_KEY, null)
            ?: throw KeyManagerException("Missing persisted BLS secret key")

        try {
            secretKey = decodeHexKey(nostrHex, NOSTR_SECRET_KEY)
            blsSecretKey = decodeHexKey(blsHex, BLS_SECRET_KEY)
        } catch (e: IllegalArgumentException) {
            throw KeyManagerException("Persisted identity key is invalid", e)
        }

        try {
            signer = RustBackedNostrSigner(secretKey)
            publicKey = signer.publicKey()
            publicKeyHex = publicKey.toHex()
        } catch (e: Exception) {
            throw KeyManagerException("Failed to initialize persisted Nostr signer", e)
        }

        // Ed25519 Stellar key (HKDF-derived from Nostr secret)
        // SECURITY: Salt must match iOS ("chat.onym.ios") for cross-platform key derivation
        // parity. Both platforms must derive identical Stellar Ed25519 and X25519 keys from
        // the same Nostr secret to enable cross-platform key attestation and invitation delivery.
        val stellarSeed = GroupCrypto.hkdf(
            ikm = secretKey,
            salt = "chat.onym.ios".toByteArray(),
            info = "stellar-ed25519-v1".toByteArray(),
            length = 32
        )
        stellarPrivateKeyParams = Ed25519PrivateKeyParameters(stellarSeed, 0)
        stellarPublicKey = stellarPrivateKeyParams.generatePublicKey().encoded

        // X25519 key agreement key (HKDF-derived from Nostr secret)
        val x25519Seed = GroupCrypto.hkdf(
            ikm = secretKey,
            salt = "chat.onym.ios".toByteArray(),
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

    // -- Transport Bundle --

    /** Create an authenticated transport bundle binding BLS, Stellar Ed25519, and X25519 inbox keys. */
    fun createTransportBundle(): com.stellarmls.mls.SEPMemberTransportBundle {
        val blsPub = blsPublicKey()
        val bundle = com.stellarmls.mls.SEPMemberTransportBundle(
            blsPubkey = blsPub,
            stellarEd25519Pubkey = stellarPublicKey,
            x25519InboxPubkey = keyAgreementPublicKey,
            signature = ByteArray(0) // placeholder for message computation
        )
        val message = bundle.computeBindingMessage()
        val signature = stellarSign(message)
        return com.stellarmls.mls.SEPMemberTransportBundle(
            blsPubkey = blsPub,
            stellarEd25519Pubkey = stellarPublicKey,
            x25519InboxPubkey = keyAgreementPublicKey,
            signature = signature
        )
    }

    /** Verify another member's transport bundle signature. */
    fun verifyTransportBundle(bundle: com.stellarmls.mls.SEPMemberTransportBundle): Boolean {
        if (!bundle.hasValidStructure) return false
        val message = bundle.computeBindingMessage()
        return verifyEd25519(bundle.stellarEd25519Pubkey, message, bundle.signature)
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

    // N-5: Store relayer auth token in EncryptedSharedPreferences instead of plaintext.

    /** Save relayer auth token securely. */
    fun saveRelayerAuthToken(token: String) {
        prefs.edit().putString("relayer_auth_token", token).apply()
    }

    /** Load relayer auth token from encrypted storage. */
    fun loadRelayerAuthToken(): String {
        return prefs.getString("relayer_auth_token", "") ?: ""
    }

    /** Save TURN username securely. */
    fun saveTurnUsername(username: String) {
        prefs.edit().putString("turn_username", username).apply()
    }

    /** Load TURN username from encrypted storage. */
    fun loadTurnUsername(): String {
        return prefs.getString("turn_username", "") ?: ""
    }

    /** Save TURN password securely. */
    fun saveTurnPassword(password: String) {
        prefs.edit().putString("turn_password", password).apply()
    }

    /** Load TURN password from encrypted storage. */
    fun loadTurnPassword(): String {
        return prefs.getString("turn_password", "") ?: ""
    }

}
