package chat.onym.android.persistence

import androidx.room.ColumnInfo
import androidx.room.Entity
import androidx.room.PrimaryKey

@Entity(tableName = "groups")
data class PersistedGroup(
    @PrimaryKey val id: String,
    val encryptedName: ByteArray,
    val encryptedGroupSecret: ByteArray,
    val createdAt: Long,
    val relayHintsJSON: String,       // cleartext (relay URLs are not sensitive)
    val encryptedMembers: ByteArray,
    val epoch: Int,                   // cleartext (needed for queries)
    val encryptedSalt: ByteArray,
    val encryptedCommitment: ByteArray?,
    val tierRawValue: Int,            // cleartext
    val isPublishedOnChain: Boolean = false,
    val pinnedEpoch: Int? = null,
    val forkedFromGroupID: String? = null,
    val forkedAtEpoch: Int? = null,
    val removedByPubkeyHex: String? = null,
    @ColumnInfo(defaultValue = "0")
    val pushNotificationsEnabled: Boolean = false
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is PersistedGroup) return false
        return id == other.id
    }

    override fun hashCode(): Int = id.hashCode()
}

@Entity(tableName = "contact_aliases")
data class PersistedContactAlias(
    @PrimaryKey val pubkey: String,
    val encryptedName: ByteArray,
    val updatedAt: Long
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is PersistedContactAlias) return false
        return pubkey == other.pubkey
    }

    override fun hashCode(): Int = pubkey.hashCode()
}

@Entity(tableName = "messages")
data class PersistedMessage(
    @PrimaryKey val id: String,
    val groupID: String,              // cleartext (needed for queries)
    val senderPubkey: String,         // cleartext
    val encryptedText: ByteArray,
    val timestamp: Long,              // cleartext (needed for sorting)
    val isMine: Boolean,              // cleartext
    val encryptedMediaAttachment: ByteArray? = null,
    @ColumnInfo(defaultValue = "0")
    val isSystemMessage: Boolean = false,
    val epoch: Int? = null,
    val replyToID: String? = null
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is PersistedMessage) return false
        return id == other.id
    }

    override fun hashCode(): Int = id.hashCode()
}

@Entity(tableName = "transport_bundles", primaryKeys = ["groupID", "blsPubkeyHex"])
data class PersistedTransportBundle(
    val groupID: String,
    val blsPubkeyHex: String,
    val encryptedBundle: ByteArray
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is PersistedTransportBundle) return false
        return groupID == other.groupID && blsPubkeyHex == other.blsPubkeyHex
    }

    override fun hashCode(): Int = 31 * groupID.hashCode() + blsPubkeyHex.hashCode()
}

@Entity(tableName = "pending_rekeys")
data class PersistedPendingRekey(
    @PrimaryKey val id: String,       // groupID-epoch
    val groupID: String,
    val epoch: Int,
    val encryptedEnvelope: ByteArray,
    val unackedMemberKeysJSON: String,
    val retryCount: Int = 0,
    val createdAt: Long = System.currentTimeMillis(),
    @ColumnInfo(defaultValue = "0")
    val isRemovalEpoch: Boolean = false
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is PersistedPendingRekey) return false
        return id == other.id
    }

    override fun hashCode(): Int = id.hashCode()
}

@Entity(tableName = "epoch_snapshots", primaryKeys = ["groupID", "epoch"])
data class PersistedEpochSnapshot(
    val groupID: String,
    val epoch: Int,
    val encryptedMembers: ByteArray,
    val encryptedSalt: ByteArray,
    val encryptedGroupSecret: ByteArray,
    val changeDescription: String
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is PersistedEpochSnapshot) return false
        return groupID == other.groupID && epoch == other.epoch
    }

    override fun hashCode(): Int = 31 * groupID.hashCode() + epoch
}
