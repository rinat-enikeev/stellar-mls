package chat.onym.android.persistence

import androidx.compose.runtime.mutableStateMapOf
import chat.onym.android.crypto.StorageEncryption

/**
 * In-memory cache of contact aliases (pubkey → display name), backed by
 * encrypted Room persistence. Compose State map triggers recomposition on changes.
 */
class ContactAliasStore {
    val aliases = mutableStateMapOf<String, String>()

    suspend fun load(dao: StellarChatDao) {
        val persisted = dao.loadAllAliases()
        for (alias in persisted) {
            try {
                val name = StorageEncryption.decryptString(alias.encryptedName)
                aliases[alias.pubkey] = name
            } catch (_: Exception) { /* skip corrupted entries */ }
        }
    }

    fun displayName(pubkey: String): String? = aliases[pubkey]

    suspend fun setAlias(pubkey: String, name: String, dao: StellarChatDao) {
        val trimmed = name.trim()
        if (trimmed.isEmpty()) {
            removeAlias(pubkey, dao)
            return
        }
        aliases[pubkey] = trimmed
        dao.saveContactAlias(
            PersistedContactAlias(
                pubkey = pubkey,
                encryptedName = StorageEncryption.encrypt(trimmed),
                updatedAt = System.currentTimeMillis()
            )
        )
    }

    suspend fun removeAlias(pubkey: String, dao: StellarChatDao) {
        aliases.remove(pubkey)
        dao.deleteContactAlias(pubkey)
    }
}
