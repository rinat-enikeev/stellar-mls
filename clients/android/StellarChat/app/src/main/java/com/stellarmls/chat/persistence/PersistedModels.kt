package com.stellarmls.chat.persistence

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
    val isPublishedOnChain: Boolean = false
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
    @androidx.room.ColumnInfo(defaultValue = "0")
    val isSystemMessage: Boolean = false
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is PersistedMessage) return false
        return id == other.id
    }

    override fun hashCode(): Int = id.hashCode()
}
