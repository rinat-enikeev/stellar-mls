package chat.onym.android.persistence

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
        PersistedTransportBundle::class, PersistedPendingRekey::class,
        PersistedEpochSnapshot::class, PersistedInvitedContact::class
    ],
    version = 16
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

        private val MIGRATION_5_6 = object : Migration(5, 6) {
            override fun migrate(db: SupportSQLiteDatabase) {
                // Add pinnedEpoch column to groups
                db.execSQL("ALTER TABLE groups ADD COLUMN pinnedEpoch INTEGER")
                // Add epoch column to messages
                db.execSQL("ALTER TABLE messages ADD COLUMN epoch INTEGER")
                // Create epoch_snapshots table
                db.execSQL("""
                    CREATE TABLE IF NOT EXISTS epoch_snapshots (
                        groupID TEXT NOT NULL,
                        epoch INTEGER NOT NULL,
                        encryptedMembers BLOB NOT NULL,
                        encryptedSalt BLOB NOT NULL,
                        encryptedGroupSecret BLOB NOT NULL,
                        changeDescription TEXT NOT NULL,
                        PRIMARY KEY(groupID, epoch)
                    )
                """.trimIndent())
            }
        }

        private val MIGRATION_6_7 = object : Migration(6, 7) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE groups ADD COLUMN forkedFromGroupID TEXT")
                db.execSQL("ALTER TABLE groups ADD COLUMN forkedAtEpoch INTEGER")
            }
        }

        private val MIGRATION_7_8 = object : Migration(7, 8) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE groups ADD COLUMN removedByPubkeyHex TEXT")
            }
        }

        private val MIGRATION_8_9 = object : Migration(8, 9) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE groups ADD COLUMN pushNotificationsEnabled INTEGER NOT NULL DEFAULT 0")
            }
        }

        private val MIGRATION_9_10 = object : Migration(9, 10) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE messages ADD COLUMN replyToID TEXT")
            }
        }

        private val MIGRATION_10_11 = object : Migration(10, 11) {
            override fun migrate(db: SupportSQLiteDatabase) {
                // Existing chats default to 0 ("distant past") so they sort to the bottom
                // and are ordered among themselves by createdAt DESC (see DAO query).
                // A new message in any chat will immediately promote it to the top.
                db.execSQL("ALTER TABLE groups ADD COLUMN lastMessageAt INTEGER NOT NULL DEFAULT 0")
            }
        }

        private val MIGRATION_11_12 = object : Migration(11, 12) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE messages ADD COLUMN status TEXT NOT NULL DEFAULT 'SENT'")
            }
        }

        private val MIGRATION_12_13 = object : Migration(12, 13) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE groups ADD COLUMN isPinned INTEGER NOT NULL DEFAULT 0")
            }
        }

        private val MIGRATION_13_14 = object : Migration(13, 14) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("""
                    CREATE TABLE IF NOT EXISTS invited_contacts (
                        phone TEXT NOT NULL PRIMARY KEY,
                        encryptedDisplayName BLOB NOT NULL,
                        inviteKind TEXT NOT NULL,
                        encryptedInviteURL BLOB NOT NULL,
                        handshakeNonce TEXT,
                        groupID TEXT,
                        invitedAt INTEGER NOT NULL,
                        status TEXT NOT NULL,
                        resolvedPubkey TEXT
                    )
                """.trimIndent())
            }
        }

        private val MIGRATION_14_15 = object : Migration(14, 15) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE groups ADD COLUMN groupTypeRawValue INTEGER NOT NULL DEFAULT 0")
            }
        }

        private val MIGRATION_15_16 = object : Migration(15, 16) {
            override fun migrate(db: SupportSQLiteDatabase) {
                // Oligarchy admin set: encrypted JSON list of BLS pubkey hex strings.
                // Null for non-oligarchy groups and pre-existing rows.
                db.execSQL("ALTER TABLE groups ADD COLUMN encryptedAdminPubkeys BLOB")
            }
        }

        fun getInstance(context: Context): StellarChatDatabase {
            return instance ?: synchronized(this) {
                instance ?: Room.databaseBuilder(
                    context.applicationContext,
                    StellarChatDatabase::class.java,
                    "stellar_chat.db"
                ).addMigrations(MIGRATION_3_4, MIGRATION_4_5, MIGRATION_5_6, MIGRATION_6_7, MIGRATION_7_8, MIGRATION_8_9, MIGRATION_9_10, MIGRATION_10_11, MIGRATION_11_12, MIGRATION_12_13, MIGRATION_13_14, MIGRATION_14_15, MIGRATION_15_16)
                .fallbackToDestructiveMigration()
                .build().also { instance = it }
            }
        }
    }
}
