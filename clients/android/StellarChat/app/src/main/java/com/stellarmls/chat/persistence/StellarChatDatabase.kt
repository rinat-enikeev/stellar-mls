package com.stellarmls.chat.persistence

import android.content.Context
import androidx.room.Database
import androidx.room.Room
import androidx.room.RoomDatabase

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
