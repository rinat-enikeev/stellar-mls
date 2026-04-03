package com.stellarmls.chat.persistence

import android.content.Context
import androidx.room.Database
import androidx.room.Room
import androidx.room.RoomDatabase
import androidx.room.migration.Migration
import androidx.sqlite.db.SupportSQLiteDatabase

/**
 * N-9: Room database without full-database encryption. Sensitive fields (group secret,
 * member keys, message content) are encrypted field-by-field via [StorageEncryption].
 * Metadata columns (id, createdAt, epoch, relayHintsJSON, isPublishedOnChain) remain
 * in cleartext. For production deployments requiring full metadata protection,
 * integrate SQLCipher via `net.zetetic:android-database-sqlcipher`.
 */
@Database(entities = [PersistedGroup::class, PersistedMessage::class, PersistedContactAlias::class], version = 4)
abstract class StellarChatDatabase : RoomDatabase() {
    abstract fun dao(): StellarChatDao

    companion object {
        @Volatile
        private var instance: StellarChatDatabase? = null

        private val MIGRATION_3_4 = object : Migration(3, 4) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE messages ADD COLUMN isSystemMessage INTEGER NOT NULL DEFAULT 0")
            }
        }

        fun getInstance(context: Context): StellarChatDatabase {
            return instance ?: synchronized(this) {
                instance ?: Room.databaseBuilder(
                    context.applicationContext,
                    StellarChatDatabase::class.java,
                    "stellar_chat.db"
                ).addMigrations(MIGRATION_3_4)
                .fallbackToDestructiveMigration()
                .build().also { instance = it }
            }
        }
    }
}
