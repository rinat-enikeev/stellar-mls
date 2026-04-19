package chat.onym.android.persistence

import androidx.compose.runtime.mutableStateListOf
import chat.onym.android.crypto.StorageEncryption
import java.util.Date

enum class InvitedContactStatus(val raw: String) {
    PENDING("pending"),
    SENT("sent"),
    JOINED("joined");

    companion object {
        fun fromRaw(raw: String): InvitedContactStatus =
            entries.find { it.raw == raw } ?: PENDING
    }
}

enum class InvitedContactKind(val raw: String) {
    GROUP("group"),
    ONBOARD("onboard");

    companion object {
        fun fromRaw(raw: String): InvitedContactKind =
            entries.find { it.raw == raw } ?: GROUP
    }
}

data class InvitedContact(
    val phone: String,
    val displayName: String,
    val kind: InvitedContactKind,
    val inviteURL: String,
    val handshakeNonce: String?,
    val groupID: String?,
    val invitedAt: Date,
    val status: InvitedContactStatus,
    val resolvedPubkey: String?
)

/**
 * In-memory cache of invited contacts, backed by encrypted Room persistence.
 * Compose State list triggers recomposition on changes.
 */
class InvitedContactStore {
    val contacts = mutableStateListOf<InvitedContact>()

    suspend fun load(dao: StellarChatDao) {
        val persisted = dao.loadAllInvitedContacts()
        contacts.clear()
        for (p in persisted) {
            try {
                val name = StorageEncryption.decryptString(p.encryptedDisplayName)
                val url = StorageEncryption.decryptString(p.encryptedInviteURL)
                contacts.add(
                    InvitedContact(
                        phone = p.phone,
                        displayName = name,
                        kind = InvitedContactKind.fromRaw(p.inviteKind),
                        inviteURL = url,
                        handshakeNonce = p.handshakeNonce,
                        groupID = p.groupID,
                        invitedAt = Date(p.invitedAt),
                        status = InvitedContactStatus.fromRaw(p.status),
                        resolvedPubkey = p.resolvedPubkey
                    )
                )
            } catch (_: Exception) { /* skip corrupted entries */ }
        }
    }

    suspend fun upsert(contact: InvitedContact, dao: StellarChatDao) {
        val existing = contacts.indexOfFirst { it.phone == contact.phone }
        if (existing >= 0) {
            contacts[existing] = contact
        } else {
            contacts.add(0, contact)
        }
        dao.saveInvitedContact(
            PersistedInvitedContact(
                phone = contact.phone,
                encryptedDisplayName = StorageEncryption.encrypt(contact.displayName),
                inviteKind = contact.kind.raw,
                encryptedInviteURL = StorageEncryption.encrypt(contact.inviteURL),
                handshakeNonce = contact.handshakeNonce,
                groupID = contact.groupID,
                invitedAt = contact.invitedAt.time,
                status = contact.status.raw,
                resolvedPubkey = contact.resolvedPubkey
            )
        )
    }

    suspend fun setStatus(phone: String, status: InvitedContactStatus, dao: StellarChatDao) {
        val index = contacts.indexOfFirst { it.phone == phone }
        if (index >= 0) {
            contacts[index] = contacts[index].copy(status = status)
        }
        dao.updateInvitedContactStatus(phone, status.raw)
    }

    /** Match any pending contact by handshake nonce and flip to joined. Returns true if matched. */
    suspend fun reconcile(nonce: String, resolvedPubkey: String, dao: StellarChatDao): Boolean {
        val index = contacts.indexOfFirst { it.handshakeNonce == nonce }
        if (index >= 0) {
            contacts[index] = contacts[index].copy(
                status = InvitedContactStatus.JOINED,
                resolvedPubkey = resolvedPubkey
            )
        }
        val rows = dao.reconcileInvitedContact(nonce, resolvedPubkey)
        return rows > 0
    }

    suspend fun remove(phone: String, dao: StellarChatDao) {
        contacts.removeAll { it.phone == phone }
        dao.deleteInvitedContact(phone)
    }
}
