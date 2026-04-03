package com.stellarmls.chat.persistence

import androidx.room.Dao
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query

@Dao
interface StellarChatDao {
    @Query("SELECT * FROM groups ORDER BY createdAt ASC")
    suspend fun loadGroups(): List<PersistedGroup>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun saveGroup(group: PersistedGroup)

    @Query("DELETE FROM groups WHERE id = :id")
    suspend fun deleteGroup(id: String)

    @Query("SELECT * FROM messages WHERE groupID = :groupID ORDER BY timestamp ASC")
    suspend fun loadMessages(groupID: String): List<PersistedMessage>

    @Insert(onConflict = OnConflictStrategy.IGNORE)
    suspend fun saveMessage(message: PersistedMessage)

    @Query("DELETE FROM messages WHERE groupID = :groupID")
    suspend fun deleteMessages(groupID: String)

    @Query("SELECT * FROM contact_aliases")
    suspend fun loadAllAliases(): List<PersistedContactAlias>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun saveContactAlias(alias: PersistedContactAlias)

    @Query("DELETE FROM contact_aliases WHERE pubkey = :pubkey")
    suspend fun deleteContactAlias(pubkey: String)
}
