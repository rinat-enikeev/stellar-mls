package chat.onym.android.call

import android.content.ComponentName
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.telecom.*
import android.util.Log
import androidx.core.content.getSystemService

/**
 * Android Telecom ConnectionService for StellarChat calls.
 * Registers a PhoneAccount so the system shows incoming calls on the lock screen
 * and integrates with Bluetooth/car head units.
 */
class CallConnectionService : ConnectionService() {

    companion object {
        private const val TAG = "CallConnService"
        private const val PHONE_ACCOUNT_ID = "stellarchat_calls"
        private var activeConnection: CallConnection? = null
        private var callManagerRef: CallManager? = null

        /** Must be called once at app startup. */
        fun initialize(context: Context, callManager: CallManager) {
            callManagerRef = callManager
            val telecomManager = context.getSystemService<TelecomManager>() ?: return
            val componentName = ComponentName(context, CallConnectionService::class.java)
            val phoneAccountHandle = PhoneAccountHandle(componentName, PHONE_ACCOUNT_ID)
            val phoneAccount = PhoneAccount.builder(phoneAccountHandle, "StellarChat")
                .setCapabilities(PhoneAccount.CAPABILITY_SELF_MANAGED)
                .build()
            telecomManager.registerPhoneAccount(phoneAccount)
        }

        /** Report an incoming call to the system. */
        fun reportIncomingCall(context: Context, callerName: String, hasVideo: Boolean) {
            val telecomManager = context.getSystemService<TelecomManager>() ?: return
            val componentName = ComponentName(context, CallConnectionService::class.java)
            val phoneAccountHandle = PhoneAccountHandle(componentName, PHONE_ACCOUNT_ID)
            val extras = Bundle().apply {
                putString("caller_name", callerName)
                putBoolean("has_video", hasVideo)
                putParcelable(
                    TelecomManager.EXTRA_PHONE_ACCOUNT_HANDLE,
                    phoneAccountHandle
                )
            }
            try {
                telecomManager.addNewIncomingCall(phoneAccountHandle, extras)
            } catch (e: SecurityException) {
                Log.w(TAG, "Cannot report incoming call: ${e.message}")
            }
        }

        /** Report an outgoing call to the system. */
        fun placeOutgoingCall(context: Context, callerName: String, hasVideo: Boolean) {
            val telecomManager = context.getSystemService<TelecomManager>() ?: return
            val componentName = ComponentName(context, CallConnectionService::class.java)
            val phoneAccountHandle = PhoneAccountHandle(componentName, PHONE_ACCOUNT_ID)
            val extras = Bundle().apply {
                putParcelable(
                    TelecomManager.EXTRA_PHONE_ACCOUNT_HANDLE,
                    phoneAccountHandle
                )
                putBundle(TelecomManager.EXTRA_OUTGOING_CALL_EXTRAS, Bundle().apply {
                    putString("caller_name", callerName)
                    putBoolean("has_video", hasVideo)
                })
            }
            try {
                telecomManager.placeCall(
                    Uri.fromParts("tel", "stellarchat", null),
                    extras
                )
            } catch (e: SecurityException) {
                Log.w(TAG, "Cannot place call: ${e.message}")
            }
        }

        /** End the active connection from the system side. */
        fun reportCallEnded() {
            activeConnection?.let { conn ->
                conn.setDisconnected(DisconnectCause(DisconnectCause.REMOTE))
                conn.destroy()
            }
            activeConnection = null
        }
    }

    override fun onCreateIncomingConnection(
        connectionManagerPhoneAccount: PhoneAccountHandle?,
        request: ConnectionRequest?
    ): Connection {
        val extras = request?.extras
        val callerName = extras?.getString("caller_name") ?: "Incoming Call"
        val hasVideo = extras?.getBoolean("has_video", false) ?: false

        val connection = CallConnection(callManagerRef).apply {
            setAddress(
                Uri.fromParts("tel", callerName, null),
                TelecomManager.PRESENTATION_ALLOWED
            )
            setCallerDisplayName(callerName, TelecomManager.PRESENTATION_ALLOWED)
            connectionProperties = Connection.PROPERTY_SELF_MANAGED
            audioModeIsVoip = true
            if (hasVideo) {
                videoState = VideoProfile.STATE_BIDIRECTIONAL
            }
            setRinging()
        }
        activeConnection = connection
        return connection
    }

    override fun onCreateOutgoingConnection(
        connectionManagerPhoneAccount: PhoneAccountHandle?,
        request: ConnectionRequest?
    ): Connection {
        val extras = request?.extras?.getBundle(TelecomManager.EXTRA_OUTGOING_CALL_EXTRAS)
        val callerName = extras?.getString("caller_name") ?: "Call"

        val connection = CallConnection(callManagerRef).apply {
            setAddress(
                Uri.fromParts("tel", callerName, null),
                TelecomManager.PRESENTATION_ALLOWED
            )
            connectionProperties = Connection.PROPERTY_SELF_MANAGED
            audioModeIsVoip = true
            setDialing()
        }
        activeConnection = connection
        return connection
    }

    override fun onCreateIncomingConnectionFailed(
        connectionManagerPhoneAccount: PhoneAccountHandle?,
        request: ConnectionRequest?
    ) {
        Log.w(TAG, "Incoming connection failed")
    }

    override fun onCreateOutgoingConnectionFailed(
        connectionManagerPhoneAccount: PhoneAccountHandle?,
        request: ConnectionRequest?
    ) {
        Log.w(TAG, "Outgoing connection failed")
    }
}

/**
 * Represents a single call connection. The system calls onAnswer/onReject/onDisconnect
 * when the user interacts with the system call UI.
 */
private class CallConnection(private val callManager: CallManager?) : Connection() {

    override fun onAnswer() {
        setActive()
        callManager?.acceptCall()
    }

    override fun onReject() {
        setDisconnected(DisconnectCause(DisconnectCause.REJECTED))
        destroy()
        callManager?.rejectCall()
    }

    override fun onDisconnect() {
        setDisconnected(DisconnectCause(DisconnectCause.LOCAL))
        destroy()
        callManager?.hangup()
    }

    override fun onAbort() {
        setDisconnected(DisconnectCause(DisconnectCause.CANCELED))
        destroy()
        callManager?.endCall(sendHangup = true)
    }
}
