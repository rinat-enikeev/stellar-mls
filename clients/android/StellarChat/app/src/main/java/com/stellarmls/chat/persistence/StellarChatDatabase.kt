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
@Database(
    entities = [
        PersistedGroup::class, PersistedMessage::class, PersistedContactAlias::class,
        PersistedTransportBundle::class, PersistedPendingRekey::class
    ],
    version = 5
)
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

        private val MIGRATION_4_5 = object : Migration(4, 5) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("""
                    CREATE TABLE IF NOT EXISTS transport_bundles (
                        groupID TEXT NOT NULL,
                        blsPubkeyHex TEXT NOT NULL,
                        encryptedBundle BLOB NOT NULL,
                        PRIMARY KEY(groupID, blsPubkeyHex)
                    )
                """.trimIndent())
                db.execSQL("""
                    CREATE TABLE IF NOT EXISTS pending_rekeys (
                        id TEXT NOT NULL PRIMARY KEY,
                        groupID TEXT NOT NULL,
                        epoch INTEGER NOT NULL,
                        encryptedEnvelope BLOB NOT NULL,
                        unackedMemberKeysJSON TEXT NOT NULL,
                        retryCount INTEGER NOT NULL DEFAULT 0,
                        createdAt INTEGER NOT NULL DEFAULT 0,
                        isRemovalEpoch INTEGER NOT NULL DEFAULT 0
                    )
                """.trimIndent())
            }
        }

        fun getInstance(context: Context): StellarChatDatabase {
            return instance ?: synchronized(this) {
                instance ?: Room.databaseBuilder(
                    context.applicationContext,
                    StellarChatDatabase::class.java,
                    "stellar_chat.db"
                ).addMigrations(MIGRATION_3_4, MIGRATION_4_5)
                .fallbackToDestructiveMigration()
                .build().also { instance = it }
            }
        }
    }
}
