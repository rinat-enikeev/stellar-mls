package chat.onym.android.push

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import chat.onym.android.MainActivity
import java.util.concurrent.atomic.AtomicInteger

object NotificationHelper {
    const val CHANNEL_MESSAGES = "stellarchat_messages"
    const val CHANNEL_INVITATIONS = "stellarchat_invitations"

    private val notificationIdCounter = AtomicInteger(1000)

    fun createChannels(context: Context) {
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

        val messages = NotificationChannel(
            CHANNEL_MESSAGES,
            "Messages",
            NotificationManager.IMPORTANCE_HIGH
        ).apply {
            description = "Encrypted group messages"
        }

        val invitations = NotificationChannel(
            CHANNEL_INVITATIONS,
            "Invitations",
            NotificationManager.IMPORTANCE_DEFAULT
        ).apply {
            description = "Group invitations"
        }

        manager.createNotificationChannels(listOf(messages, invitations))
    }

    fun showMessageNotification(
        context: Context,
        groupName: String,
        senderAlias: String,
        text: String,
        groupID: String?
    ) {
        val intent = Intent(context, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
            if (groupID != null) putExtra("navigate_group_id", groupID)
        }
        val requestCode = groupID?.hashCode() ?: 0
        val pendingIntent = PendingIntent.getActivity(
            context, requestCode, intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val notification = NotificationCompat.Builder(context, CHANNEL_MESSAGES)
            .setSmallIcon(android.R.drawable.ic_dialog_email)
            .setContentTitle(groupName)
            .setContentText("$senderAlias: $text")
            .setAutoCancel(true)
            .setContentIntent(pendingIntent)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .build()

        try {
            NotificationManagerCompat.from(context).notify(notificationIdCounter.getAndIncrement(), notification)
        } catch (_: SecurityException) {
            // POST_NOTIFICATIONS permission not granted
        }
    }

    fun showGenericNotification(context: Context) {
        val intent = Intent(context, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val pendingIntent = PendingIntent.getActivity(
            context, 0, intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val notification = NotificationCompat.Builder(context, CHANNEL_MESSAGES)
            .setSmallIcon(android.R.drawable.ic_dialog_email)
            .setContentTitle("StellarChat")
            .setContentText("New message")
            .setAutoCancel(true)
            .setContentIntent(pendingIntent)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .build()

        try {
            NotificationManagerCompat.from(context).notify(notificationIdCounter.getAndIncrement(), notification)
        } catch (_: SecurityException) { }
    }

}
