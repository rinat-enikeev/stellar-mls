package com.stellarmls.chat.nostr

import com.stellarmls.chat.crypto.NostrEvent
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

class NostrRelayConnection(
    private val url: String,
    private val client: OkHttpClient = OkHttpClient.Builder()
        .readTimeout(0, TimeUnit.MILLISECONDS)
        .build()
) {
    private var webSocket: WebSocket? = null
    private val subscriptionCallbacks = ConcurrentHashMap<String, (NostrEvent) -> Unit>()

    fun connect() {
        val request = Request.Builder().url(url).build()
        webSocket = client.newWebSocket(request, object : WebSocketListener() {
            override fun onMessage(webSocket: WebSocket, text: String) {
                handleMessage(text)
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                // Auto-reconnect after delay
                Thread.sleep(3000)
                connect()
            }
        })
    }

    fun disconnect() {
        webSocket?.close(1000, "Bye")
        webSocket = null
        subscriptionCallbacks.clear()
    }

    fun publish(event: NostrEvent) {
        val frame = JSONArray().apply {
            put("EVENT")
            put(event.toJson())
        }
        webSocket?.send(frame.toString())
    }

    fun subscribe(subscriptionID: String, filter: JSONObject): Flow<NostrEvent> = callbackFlow {
        subscriptionCallbacks[subscriptionID] = { event ->
            trySend(event)
        }

        val frame = JSONArray().apply {
            put("REQ")
            put(subscriptionID)
            put(filter)
        }
        webSocket?.send(frame.toString())

        awaitClose {
            subscriptionCallbacks.remove(subscriptionID)
            val closeFrame = JSONArray().apply {
                put("CLOSE")
                put(subscriptionID)
            }
            webSocket?.send(closeFrame.toString())
        }
    }

    private fun handleMessage(text: String) {
        try {
            val array = JSONArray(text)
            when (array.getString(0)) {
                "EVENT" -> {
                    if (array.length() >= 3) {
                        val subID = array.getString(1)
                        val eventJson = array.getJSONObject(2)
                        val event = parseEvent(eventJson) ?: return
                        subscriptionCallbacks[subID]?.invoke(event)
                    }
                }
                "EOSE", "OK", "NOTICE" -> { /* ignored */ }
            }
        } catch (_: Exception) { }
    }

    private fun parseEvent(json: JSONObject): NostrEvent? {
        return try {
            val tagsArray = json.getJSONArray("tags")
            val tags = (0 until tagsArray.length()).map { i ->
                val inner = tagsArray.getJSONArray(i)
                (0 until inner.length()).map { j -> inner.getString(j) }
            }
            NostrEvent(
                id = json.getString("id"),
                pubkey = json.getString("pubkey"),
                createdAt = json.getLong("created_at"),
                kind = json.getInt("kind"),
                tags = tags,
                content = json.getString("content"),
                sig = json.getString("sig")
            )
        } catch (_: Exception) { null }
    }
}
