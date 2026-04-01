package com.stellarmls.chat.persistence

import android.content.Context
import androidx.room.Database
import androidx.room.Room
import androidx.room.RoomDatabase

/**
 * N-9: Room database without full-database encryption. Sensitive fields (group secret,
 * member keys, message content) are encrypted field-by-field via [StorageEncryption].
 * Metadata columns (id, createdAt, epoch, relayHintsJSON, isPublishedOnChain) remain
 * in cleartext. For production deployments requiring full metadata protection,
 * integrate SQLCipher via `net.zetetic:android-database-sqlcipher`.
 */
@Database(entities = [PersistedGroup::class, PersistedMessage::class], version = 1)
abstract class StellarChatDatabase : RoomDatabase() {
    abstract fun dao(): StellarChatDao

    companion object {
        @Volatile
        private var instance: StellarChatDatabase? = null

        fun getInstance(context: Context): StellarChatDatabase {
            return instance ?: synchronized(this) {
                instance ?: Room.databaseBuilder(
                    context.applicationContext,
                    StellarChatDatabase::class.java,
                    "stellar_chat.db"
                ).build().also { instance = it }
            }
        }
    }
}
