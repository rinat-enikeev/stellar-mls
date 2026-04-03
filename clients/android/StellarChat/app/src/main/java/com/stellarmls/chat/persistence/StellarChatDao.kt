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

    // Transport bundles
    @Query("SELECT * FROM transport_bundles WHERE groupID = :groupID")
    suspend fun loadTransportBundles(groupID: String): List<PersistedTransportBundle>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun saveTransportBundle(bundle: PersistedTransportBundle)

    @Query("DELETE FROM transport_bundles WHERE groupID = :groupID")
    suspend fun deleteTransportBundles(groupID: String)

    @Query("DELETE FROM transport_bundles WHERE groupID = :groupID AND blsPubkeyHex = :blsPubkeyHex")
    suspend fun deleteTransportBundle(groupID: String, blsPubkeyHex: String)

    // Pending rekeys
    @Query("SELECT * FROM pending_rekeys WHERE groupID = :groupID")
    suspend fun loadPendingRekeys(groupID: String): List<PersistedPendingRekey>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun savePendingRekey(rekey: PersistedPendingRekey)

    @Query("DELETE FROM pending_rekeys WHERE groupID = :groupID AND epoch = :epoch")
    suspend fun deletePendingRekey(groupID: String, epoch: Int)

    @Query("UPDATE pending_rekeys SET unackedMemberKeysJSON = :unackedJSON WHERE groupID = :groupID AND epoch = :epoch")
    suspend fun updatePendingRekeyUnacked(groupID: String, epoch: Int, unackedJSON: String)

    @Query("UPDATE pending_rekeys SET retryCount = retryCount + 1 WHERE groupID = :groupID AND epoch = :epoch")
    suspend fun incrementPendingRekeyRetry(groupID: String, epoch: Int)

    @Query("SELECT * FROM pending_rekeys")
    suspend fun loadAllPendingRekeys(): List<PersistedPendingRekey>

    @Query("SELECT COUNT(*) FROM pending_rekeys WHERE groupID = :groupID AND epoch = :epoch AND isRemovalEpoch = 1")
    suspend fun isRemovalEpoch(groupID: String, epoch: Int): Int
}
