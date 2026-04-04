# Privacy-Preserving Push Notifications for StellarChat (Android)

## Preamble

```
Document: Push Notification Design (Android)
Title: Privacy-Preserving Push Notifications with Encrypted FCM and UnifiedPush Delivery
Author: @rinat-enikeev
Status: Draft
Created: 2026-04-04
Requires: SEP-XXXX, NIP-XX (Private Group Relay Transport)
Platform: Android / Firebase Cloud Messaging (FCM) + UnifiedPush
Companion: push-notification-design.md (iOS)
```

---

## Simple Summary

The Android counterpart to the iOS push notification design. Uses the same PN relay, the same ephemeral X25519 registration protocol, and the same privacy guarantees. On-device decryption happens in `FirebaseMessagingService.onMessageReceived()` using data-only messages (no notification field), giving the app full control over decryption and display. For users without Google Play Services or who prefer Google-free operation, UnifiedPush is supported as an alternative delivery transport with identical privacy properties.

---

## Motivation

Same as the iOS design: persistent WebSocket connections disconnect when the app is backgrounded or killed. Android is more aggressive than iOS about killing background processes — Doze mode, App Standby Buckets, and OEM battery optimizations (Samsung, Xiaomi, Huawei) frequently terminate WebSocket connections within minutes.

FCM is the standard Android push delivery mechanism, but it routes all notifications through Google infrastructure, where Google can observe:
- device registration tokens and associated device identifiers
- message delivery timing and frequency
- unencrypted payload content (if not encrypted by the app)

This design eliminates content exposure by sending only encrypted payloads through FCM, and offers UnifiedPush as a fully Google-free alternative.

---

## Design Principles

Shared with the iOS design:

1. **Identity separation.** The PN relay never learns the Nostr pubkey, BLS identity, or Stellar address of any subscriber.
2. **Content opacity.** The PN relay forwards encrypted SealedEnvelopes it cannot decrypt.
3. **Reuse existing cryptography.** Registration encryption reuses `x25519-aes-256-gcm-v1` from `GroupCrypto.encryptInvitation()`.
4. **Relays remain dumb.** The Nostr relay is unaware of push notifications.
5. **Graceful degradation.** If decryption fails, a generic "New message" notification is shown.

Android-specific principles:

6. **Data-only messages.** All FCM messages use only the `data` field (no `notification` field), ensuring `onMessageReceived()` is always called — even when the app is in the background. This gives the app full control over decryption before display.
7. **Google-free option.** UnifiedPush support ensures the app works on de-Googled devices (GrapheneOS, CalyxOS, LineageOS without GApps, F-Droid distribution).
8. **No Firebase Analytics.** FCM auto-initialization and analytics collection are disabled. The app uses FCM as a dumb push pipe only.

---

## Threat Model

### What each party learns

The PN relay operator threat model is identical to the iOS design. This section covers Android-specific differences.

**Google (FCM) learns (accepted):**
- A device registration token received a data message at a given time
- The sender ID (Firebase project identifier — shared across all users of the app)
- The encrypted payload size
- Device configuration data uploaded during token registration

**Google does NOT learn:**
- Message content (encrypted with `notification_key`, decrypted on-device)
- Group identity or name
- The user's Nostr pubkey, BLS key, or Stellar address
- Which PN relay sent the message (only the Firebase project's sender ID is visible)

**Comparison: Google (FCM) vs Apple (APNs):**

| Aspect | Google (FCM) | Apple (APNs) |
|--------|-------------|--------------|
| Token linked to user account | Yes (Google account) | Yes (Apple ID) |
| Analytics collection | Yes (can be disabled) | Minimal |
| Payload visibility | Opaque if encrypted | Opaque if encrypted |
| Required infrastructure | Google Play Services | iOS system service |
| Alternative available | UnifiedPush | None |

**UnifiedPush distributor learns:**
- That a push was received at a given time
- The encrypted payload size
- Nothing else (if self-hosted, the user controls the distributor entirely)

### Accepted residual risks

Same as iOS, plus:

1. **Google Play Services dependency.** FCM requires Google Play Services. On devices without it, only UnifiedPush works.
2. **FCM token as device fingerprint.** FCM tokens are long-lived (expire after 270 days of inactivity). If the PN relay's storage is compromised, past token-to-topic mappings are recoverable. Same mitigation as iOS: tokens stored encrypted at rest.
3. **Background execution limits.** Android may delay or batch data messages in Doze mode. High-priority FCM messages bypass Doze but require the app to be properly configured.

---

## Architecture

### Shared PN Relay

The Android client uses the **same PN relay** as iOS. The relay is platform-agnostic — it stores a `platform` field per subscription and dispatches to either APNs or FCM accordingly.

```
+---------------------+        +------------------+        +------------------+
|   Android Client    |        |   PN Relay (DO)  |        | Nostr Relay      |
|                     |        |                  |        | (strfry)         |
|  +-----------------+|  HTTPS |  +-------------+ |   WS   |                  |
|  | GroupListVM     |--------->| HTTP API    |<--------->|                  |
|  +-----------------+|        |  +-------------+ |        |                  |
|                     |        |  | Event       | |        |                  |
|  +-----------------+|  FCM   |  | Watcher     | |        |                  |
|  | FCM Service     |<---------|             | |        |                  |
|  | (decrypt)       ||        |  +-------------+ |        |                  |
|  +-----------------+|        |  | FCM Client  | |        |                  |
|                     |        |  +-------------+ |        |                  |
|  +-----------------+|        |  | APNs Client | |        |                  |
|  | Encrypted       ||        |  +-------------+ |        |                  |
|  | SharedPrefs     ||        |  | SQLite DB   | |        |                  |
|  +-----------------+|        |  +-------------+ |        |                  |
+---------------------+        +------------------+        +------------------+

  -- OR --

+---------------------+
|   Android Client    |
|                     |
|  +-----------------+|  UnifiedPush
|  | UP Receiver     |<--------- (ntfy / self-hosted distributor)
|  | (decrypt)       ||
|  +-----------------+|
+---------------------+
```

### Data Flow

```
1. REGISTRATION (app launch)
   Client ---[encrypted envelope]--> PN Relay
   Payload includes platform: "android-fcm" or "android-up"
   PN Relay stores: subscription_id, filter, encrypted_token, encrypted_notification_key

2. EVENT ARRIVAL (message sent to group)
   Sender --> Nostr Relay --[kind 44114 event]--> PN Relay (Event Watcher)

3. PUSH DISPATCH (FCM path)
   PN Relay: match event topic -> subscriptions
   PN Relay: decrypt FCM token (transiently)
   PN Relay ---[FCM HTTP v1 API]--> Google ---[data message]--> Device

   PUSH DISPATCH (UnifiedPush path)
   PN Relay: decrypt UP endpoint URL (transiently)
   PN Relay ---[HTTP POST]--> UP distributor ---[broadcast]--> Device

4. ON-DEVICE DECRYPTION
   FCM: StellarChatMessagingService.onMessageReceived()
   UP:  StellarChatUnifiedPushReceiver.onMessage()
   Both: decrypt notification_key layer -> SealedEnvelope
         load group key from EncryptedSharedPreferences
         decrypt SealedEnvelope -> plaintext message
         display notification via NotificationManager
```

---

## Registration Protocol

### Shared Protocol

The registration protocol is identical to the iOS design (see `push-notification-design.md` §Registration Protocol). The only difference is the `platform` field and the token format:

| Field | iOS | Android (FCM) | Android (UnifiedPush) |
|-------|-----|---------------|----------------------|
| `platform` | `"ios"` | `"android-fcm"` | `"android-up"` |
| `apns_token` / `fcm_token` / `up_endpoint` | APNs device token (hex) | FCM registration token (string) | UnifiedPush endpoint URL |

### FCM Registration Payload

```json
{
  "action": "subscribe",
  "subscription_id": "<32-byte random hex>",
  "filter": {
    "kinds": [44114],
    "#t": ["<hidden_group_topic>"]
  },
  "push_token": "<FCM registration token>",
  "notification_key": "<base64: 32-byte AES-256-GCM key>",
  "platform": "android-fcm"
}
```

### UnifiedPush Registration Payload

```json
{
  "action": "subscribe",
  "subscription_id": "<32-byte random hex>",
  "filter": {
    "kinds": [44114],
    "#t": ["<hidden_group_topic>"]
  },
  "push_token": "<UnifiedPush endpoint URL>",
  "notification_key": "<base64: 32-byte AES-256-GCM key>",
  "platform": "android-up"
}
```

Both payloads are encrypted and POSTed as `x25519-aes-256-gcm-v1` sealed envelopes, identical to iOS.

### Token Lifecycle

**FCM tokens:**
- Obtained via `FirebaseMessaging.getInstance().token`
- Refresh detected in `onNewToken()` callback
- Long-lived (expire after 270 days of inactivity)
- Change on: app reinstall, data clear, device restoration

**UnifiedPush endpoints:**
- Obtained from the UnifiedPush distributor via `registerApp()` callback
- Endpoint URL format depends on the distributor (e.g., `https://ntfy.example.com/topic123`)
- Change when: distributor changes, user re-registers

On token/endpoint change, the app re-registers all subscriptions with the PN relay using the new token.

---

## Push Delivery

### FCM: Data-Only Messages

The PN relay sends data-only messages (no `notification` field) via the FCM HTTP v1 API:

```json
POST https://fcm.googleapis.com/v1/projects/{project_id}/messages:send
Authorization: Bearer {oauth2_token}

{
  "message": {
    "token": "<FCM registration token>",
    "android": {
      "priority": "high"
    },
    "data": {
      "enc": "<base64: AES-256-GCM encrypted event content>",
      "nonce": "<base64: 12 bytes>",
      "tag": "<base64: 16 bytes>",
      "event_id": "<nostr event id hex>",
      "sub_id": "<subscription_id first 8 chars>"
    }
  }
}
```

**Why data-only:**
- `onMessageReceived()` is always called, even when the app is in the background
- If a `notification` field is present, Android auto-displays it when the app is backgrounded — bypassing decryption
- Data-only messages give the app full control over decryption and notification display

**Priority: `high`:**
- Bypasses Doze mode battery optimizations
- Immediate delivery even when the device is idle
- Required for time-sensitive chat notifications

### UnifiedPush: HTTP POST

For UnifiedPush subscriptions, the PN relay sends an HTTP POST to the distributor endpoint:

```
POST <unified_push_endpoint_url>
Content-Type: application/octet-stream

<binary: nonce(12) || AES-256-GCM(notification_key, event_content) || tag(16)>
```

The UnifiedPush distributor forwards the raw bytes to the app via a local broadcast. The app decrypts using the `notification_key` for this subscription.

### Payload Budget

FCM data messages are limited to 4,096 bytes. Budget:

| Component | Size |
|-----------|------|
| `enc` (typical chat SealedEnvelope) | ~300-700 bytes base64 |
| `nonce` + `tag` | ~40 bytes |
| `event_id` + `sub_id` | ~80 bytes |
| JSON key overhead | ~60 bytes |
| **Total typical** | **~480-880 bytes** |
| **Available headroom** | **~3,200 bytes** |

For events exceeding 3 KB, the PN relay sends a wake-only message:

```json
{
  "data": {
    "event_id": "<nostr event id hex>",
    "sub_id": "<subscription_id first 8 chars>",
    "fetch": "true"
  }
}
```

The app fetches the full event from the Nostr relay on wake.

---

## Android Implementation

### Firebase Configuration

**No `google-services.json` committed to the repo.** The Firebase project configuration is provided at build time via environment variables or a local file excluded from version control.

**Disable auto-initialization** in `AndroidManifest.xml`:

```xml
<meta-data
    android:name="firebase_messaging_auto_init_enabled"
    android:value="false" />
<meta-data
    android:name="firebase_analytics_collection_enabled"
    android:value="false" />
```

FCM is initialized programmatically only when the user opts in to push notifications, using:

```kotlin
FirebaseMessaging.getInstance().isAutoInitEnabled = true
```

This ensures no Google telemetry is sent until the user explicitly enables push.

### StellarChatMessagingService

New file: `app/src/main/java/com/stellarmls/chat/push/StellarChatMessagingService.kt`

```kotlin
class StellarChatMessagingService : FirebaseMessagingService() {

    override fun onMessageReceived(message: RemoteMessage) {
        val data = message.data
        val subIdHint = data["sub_id"] ?: return
        val encBase64 = data["enc"]
        val eventId = data["event_id"]

        if (encBase64 == null) {
            // Wake-only message — show generic notification
            showGenericNotification()
            return
        }

        try {
            // 1. Load notification_key for this subscription
            val store = PushSubscriptionStore.load(applicationContext)
            val subscription = store.findByHint(subIdHint) ?: return
            val notificationKey = subscription.decryptNotificationKey()

            // 2. Decrypt notification_key layer
            val sealedEnvelopeBase64 = decryptNotificationPayload(
                encBase64, data["nonce"]!!, data["tag"]!!, notificationKey
            )

            // 3. Decrypt SealedEnvelope with group key
            val groupKey = subscription.decryptGroupKey()
            val plaintext = GroupCrypto.decrypt(sealedEnvelopeBase64, groupKey)

            // 4. Parse v2 message
            val json = JSONObject(plaintext)
            val text = json.optString("text", "")
            val senderPubkey = json.optString("senderBlsPubkey", "")
            val type = json.optString("type", "chat")

            // 5. Resolve sender alias and group name
            val groupName = subscription.decryptGroupName()
            val senderAlias = store.resolveAlias(senderPubkey)

            // 6. Show notification
            showMessageNotification(groupName, senderAlias, text, type, eventId)

        } catch (e: Exception) {
            // Decryption failed — show generic notification
            showGenericNotification()
        }
    }

    override fun onNewToken(token: String) {
        // Re-register all subscriptions with new FCM token
        PNRelayClient.reregisterAll(applicationContext, token)
    }
}
```

**Execution window:** `onMessageReceived()` has ~20 seconds. Decryption (AES-256-GCM + HKDF) completes in <10ms. SharedPreferences access is <50ms. Well within limits.

### Notification Display

Android requires explicit notification channels (API 26+) and runtime permission (API 33+).

New file: `app/src/main/java/com/stellarmls/chat/push/NotificationHelper.kt`

**Notification channels:**

| Channel ID | Name | Importance |
|------------|------|-----------|
| `stellarchat_messages` | Messages | HIGH (heads-up) |
| `stellarchat_invitations` | Invitations | DEFAULT |
| `stellarchat_calls` | Calls | MAX |

**Notification construction:**

```kotlin
fun showMessageNotification(
    groupName: String,
    senderAlias: String,
    text: String,
    type: String,
    eventId: String?
) {
    val notification = NotificationCompat.Builder(context, "stellarchat_messages")
        .setSmallIcon(R.drawable.ic_notification)
        .setContentTitle(groupName)
        .setContentText("$senderAlias: $text")
        .setAutoCancel(true)
        .setContentIntent(pendingIntentForGroup(groupId))
        .setGroup("stellarchat_group_$groupId")
        .build()

    NotificationManagerCompat.from(context).notify(notificationId, notification)
}
```

**Generic fallback** (decryption failure):

```kotlin
fun showGenericNotification() {
    val notification = NotificationCompat.Builder(context, "stellarchat_messages")
        .setSmallIcon(R.drawable.ic_notification)
        .setContentTitle("StellarChat")
        .setContentText("New message")
        .setAutoCancel(true)
        .build()

    NotificationManagerCompat.from(context).notify(GENERIC_ID, notification)
}
```

### UnifiedPush Integration

New file: `app/src/main/java/com/stellarmls/chat/push/StellarChatUnifiedPushReceiver.kt`

UnifiedPush uses a broadcast receiver pattern:

```kotlin
class StellarChatUnifiedPushReceiver : MessagingReceiver() {

    override fun onNewEndpoint(context: Context, endpoint: String, instance: String) {
        // New endpoint assigned — register with PN relay
        PNRelayClient.reregisterAllUnifiedPush(context, endpoint)
    }

    override fun onMessage(context: Context, message: ByteArray, instance: String) {
        // Decrypt and display — same logic as FCM onMessageReceived
        // message is raw bytes: nonce(12) || ciphertext || tag(16)
        decryptAndDisplay(context, message, instance)
    }

    override fun onUnregistered(context: Context, instance: String) {
        // Distributor removed — clean up subscriptions
        PNRelayClient.unregisterAll(context)
    }
}
```

**Dependency:** `org.unifiedpush.android:connector:2.x.x`

The app detects whether UnifiedPush distributors are available at runtime:

```kotlin
val distributors = UnifiedPush.getDistributors(context)
if (distributors.isNotEmpty()) {
    // Offer UnifiedPush option in settings
}
```

### Push Subscription Store

New file: `app/src/main/java/com/stellarmls/chat/push/PushSubscriptionStore.kt`

Unlike iOS (which uses a shared App Group container for NSE access), Android's `FirebaseMessagingService` runs in the app's process and has full access to the app's storage. No special sharing mechanism is needed.

The store uses `EncryptedSharedPreferences` (same as `KeyManager`):

```kotlin
class PushSubscriptionStore private constructor(
    private val prefs: SharedPreferences
) {
    data class Subscription(
        val subscriptionId: String,
        val subIdHint: String,
        val encryptedNotificationKey: ByteArray,
        val encryptedGroupName: ByteArray,
        val encryptedGroupSecret: ByteArray,
        val encryptedSalt: ByteArray,
        val epoch: Long,
        val groupId: String
    )

    fun findByHint(subIdHint: String): Subscription?
    fun save(subscription: Subscription)
    fun delete(subscriptionId: String)
    fun all(): List<Subscription>

    fun resolveAlias(blsPubkeyBase64: String): String

    companion object {
        fun load(context: Context): PushSubscriptionStore
    }
}
```

All sensitive fields (`notification_key`, `group_name`, `group_secret`, `salt`) are encrypted under `StorageEncryption` before storage, consistent with the existing field-level encryption pattern used by `PersistenceStore`.

### PNRelayClient

New file: `app/src/main/java/com/stellarmls/chat/push/PNRelayClient.kt`

HTTP client for the PN relay, using OkHttp (already a project dependency):

```kotlin
object PNRelayClient {
    suspend fun fetchInfo(relayURL: String): PNRelayInfo
    suspend fun subscribe(
        relayURL: String,
        subscriptionId: String,
        filter: JSONObject,
        pushToken: String,
        notificationKey: ByteArray,
        platform: String,
        relayX25519Pubkey: ByteArray
    )
    suspend fun update(
        relayURL: String,
        subscriptionId: String,
        filter: JSONObject,
        relayX25519Pubkey: ByteArray
    )
    suspend fun unsubscribe(
        relayURL: String,
        subscriptionId: String,
        relayX25519Pubkey: ByteArray
    )
    suspend fun reregisterAll(context: Context, newToken: String)
    suspend fun reregisterAllUnifiedPush(context: Context, newEndpoint: String)
    suspend fun unregisterAll(context: Context)
}
```

Envelope encryption uses `GroupCrypto.encryptInvitation()` with the PN relay's X25519 public key as the recipient — identical to the iOS implementation.

### AndroidManifest.xml Changes

```xml
<!-- Push notification permission (Android 13+) -->
<uses-permission android:name="android.permission.POST_NOTIFICATIONS" />

<!-- FCM service -->
<service
    android:name=".push.StellarChatMessagingService"
    android:exported="false">
    <intent-filter>
        <action android:name="com.google.firebase.MESSAGING_EVENT" />
    </intent-filter>
</service>

<!-- UnifiedPush receiver -->
<receiver
    android:name=".push.StellarChatUnifiedPushReceiver"
    android:exported="true">
    <intent-filter>
        <action android:name="org.unifiedpush.android.connector.MESSAGE" />
        <action android:name="org.unifiedpush.android.connector.NEW_ENDPOINT" />
        <action android:name="org.unifiedpush.android.connector.UNREGISTERED" />
    </intent-filter>
</receiver>

<!-- Disable Firebase auto-init and analytics -->
<meta-data
    android:name="firebase_messaging_auto_init_enabled"
    android:value="false" />
<meta-data
    android:name="firebase_analytics_collection_enabled"
    android:value="false" />
```

### build.gradle.kts Changes

```kotlin
dependencies {
    // FCM
    implementation(platform("com.google.firebase:firebase-bom:33.x.x"))
    implementation("com.google.firebase:firebase-messaging")

    // UnifiedPush
    implementation("org.unifiedpush.android:connector:2.x.x")
}

// At bottom of file
apply(plugin = "com.google.gms.google-services")
```

Root `build.gradle.kts`:
```kotlin
plugins {
    id("com.google.gms.google-services") version "4.x.x" apply false
}
```

### Registration Lifecycle

```
App Launch (GroupListViewModel.init)
  ├── Check push notification settings
  ├── If FCM enabled:
  │     ├── FirebaseMessaging.getInstance().token → FCM token
  │     └── For each group: register subscription (platform: "android-fcm")
  ├── If UnifiedPush enabled:
  │     ├── UnifiedPush.registerApp(context) → endpoint via callback
  │     └── For each group: register subscription (platform: "android-up")
  └── Store subscription state in PushSubscriptionStore

Group Join
  └── Register new subscription with PN relay

Group Leave
  └── Unsubscribe

Rekey / Epoch Change
  ├── Update subscription filter with new hidden topic
  ├── Update PushSubscriptionStore with new group key material
  └── POST /v1/subscription with action: "update"

FCM Token Refresh (onNewToken)
  └── Re-register all subscriptions with new token

UnifiedPush Endpoint Change (onNewEndpoint)
  └── Re-register all subscriptions with new endpoint

Notification Permission
  ├── Request at runtime (Android 13+): POST_NOTIFICATIONS
  └── Create notification channels on first grant
```

### Settings UI

Push notification settings in the existing Settings screen:

```
Push Notifications
  ├── Enable push notifications          [toggle]
  ├── Delivery method
  │     ├── ○ Google (FCM)               [default if Play Services available]
  │     └── ○ UnifiedPush                [if distributor installed]
  ├── PN Relay URL                       [text field, default from BuildConfig]
  └── Show message preview               [toggle — controls whether NSE decrypts or shows generic]
```

When the user switches delivery method, all subscriptions are unregistered from the old method and re-registered with the new method.

---

## PN Relay Server Changes

The PN relay (described in the iOS design doc) needs the following additions for Android support:

### FCM Dispatch

The PN relay authenticates with FCM using a service account key (OAuth 2.0):

```
FCM_SERVICE_ACCOUNT_PATH=/app/fcm-service-account.json
FCM_PROJECT_ID=stellarchat-xxxxx
```

FCM HTTP v1 API dispatch:
1. Load service account credentials
2. Generate OAuth 2.0 access token (cached, refreshed before expiry)
3. POST data message to `https://fcm.googleapis.com/v1/projects/{project_id}/messages:send`
4. Handle error responses: `UNREGISTERED` (delete subscription), `QUOTA_EXCEEDED` (backoff)

### UnifiedPush Dispatch

For `android-up` subscriptions, the PN relay sends an HTTP POST directly to the UnifiedPush endpoint URL:

```
POST <endpoint_url>
Content-Type: application/octet-stream

<binary payload>
```

No authentication needed — the endpoint URL is a capability URL (knowledge of the URL is authorization). The endpoint URL is stored encrypted in the PN relay's database, same as APNs/FCM tokens.

### Platform Routing

```
Event arrives on topic
  └── For each matching subscription:
        ├── platform == "ios"        → APNs HTTP/2 dispatch
        ├── platform == "android-fcm" → FCM HTTP v1 dispatch
        └── platform == "android-up"  → UnifiedPush HTTP POST dispatch
```

### Updated Database Schema

```sql
CREATE TABLE subscriptions (
  subscription_id      TEXT PRIMARY KEY,
  filter_json          TEXT NOT NULL,
  encrypted_token      BLOB NOT NULL,       -- APNs token / FCM token / UP endpoint
  encrypted_notif_key  BLOB NOT NULL,
  platform             TEXT NOT NULL,        -- 'ios', 'android-fcm', 'android-up'
  created_at           INTEGER NOT NULL,
  last_pushed_at       INTEGER DEFAULT 0
);
```

### Docker Changes

Add FCM service account key volume:

```yaml
pn-relay:
  volumes:
    - pn-relay-data:/app/data
    - ./pn-relay/apns-key.p8:/app/apns-key.p8:ro
    - ./pn-relay/fcm-service-account.json:/app/fcm-service-account.json:ro
```

---

## Privacy Comparison: FCM vs UnifiedPush

| Property | FCM | UnifiedPush (self-hosted) |
|----------|-----|--------------------------|
| Google sees push timing | Yes | No |
| Google sees device token | Yes | No |
| Google sees payload | Encrypted (opaque) | N/A |
| Requires Google Play Services | Yes | No |
| Distributor operator sees push timing | N/A | Yes (but self-hosted = you) |
| Token/endpoint stability | ~270 days | Depends on distributor |
| Battery efficiency | Excellent (shared connection) | Good (single distributor connection) |
| Works on de-Googled devices | No | Yes |
| Setup complexity for user | None | Install distributor app |

**Recommendation:** Default to FCM for ease of use. Offer UnifiedPush in settings for privacy-conscious users. Both paths provide identical content privacy (encrypted payloads, on-device decryption).

---

## Security Considerations

### Shared with iOS

All security considerations from the iOS design doc apply equally:
- Subscription ID as bearer token (256-bit random, transmitted only encrypted)
- Server key compromise (forward secrecy via ephemeral keys)
- Replay protection
- Token re-encryption on rotation

### Android-Specific

**1. FirebaseMessagingService process model.**
Unlike iOS's NSE (separate process with memory/time limits), Android's `FirebaseMessagingService` runs in the app's main process. This means:
- Full access to app storage — no App Group sharing needed
- No memory limit beyond the app's allocation
- 20-second execution window (sufficient for decryption)
- If the app is killed by the OS, FCM will restart the service to deliver the message

**2. Notification permission (Android 13+).**
Starting with API 33 (Android 13), `POST_NOTIFICATIONS` is a runtime permission. The app must request it explicitly. If denied, the app can still receive and process data messages in `onMessageReceived()` — it just cannot display notifications. The app should handle this gracefully (e.g., badge count, in-app indicator on next launch).

**3. Doze mode and high-priority messages.**
Android's Doze mode defers background work to maintenance windows. FCM high-priority data messages bypass Doze and are delivered immediately. The PN relay MUST set `"priority": "high"` for all chat notifications. Low-priority messages may be delayed by minutes or hours.

**4. OEM battery optimizations.**
Samsung, Xiaomi, Huawei, and other OEMs add aggressive battery optimization that can kill background services and delay FCM delivery. Users may need to exempt StellarChat from battery optimization. The app should detect restricted battery settings and guide the user to allow unrestricted background activity.

**5. EncryptedSharedPreferences thread safety.**
`EncryptedSharedPreferences` initialization blocks the calling thread (M-12 audit finding). `PushSubscriptionStore` must be initialized on `Dispatchers.IO`, not the main thread. In `onMessageReceived()`, initialization is safe because the callback runs on a background thread.

**6. UnifiedPush endpoint as capability URL.**
The UnifiedPush endpoint URL is a capability URL — anyone who knows it can send pushes to the device. It must be:
- Stored encrypted (same as FCM tokens)
- Transmitted only inside encrypted registration envelopes
- Never logged or exposed in error messages

**7. No google-services.json in version control.**
The Firebase project configuration file contains the project ID, API key, and other identifiers. It must be excluded from the repository (`.gitignore`) and provided at build time.

---

## Differences from iOS Implementation

| Aspect | iOS | Android |
|--------|-----|---------|
| Push service | APNs | FCM + UnifiedPush |
| On-device decryption | Notification Service Extension (separate process) | FirebaseMessagingService (app process) |
| Shared key storage | Shared Keychain (App Group) | EncryptedSharedPreferences (app process, no sharing needed) |
| Shared data store | App Group container JSON file | EncryptedSharedPreferences (same process) |
| Keychain accessibility change | Required (WhenUnlocked → AfterFirstUnlock) | Not needed |
| New Xcode/build target | Yes (NSE target) | No (service declared in manifest) |
| Notification display | NSE modifies system notification | App creates notification via NotificationManager |
| Google-free option | None | UnifiedPush |
| Runtime permission | Not needed (iOS asks automatically) | POST_NOTIFICATIONS (Android 13+) |
| Battery optimization | Not an issue (APNs is system-level) | Doze mode bypass via high-priority, OEM exemptions |

---

## Implementation Phases

### Phase 1: FCM Infrastructure

- Add Firebase dependencies to `build.gradle.kts`
- Add `google-services` plugin (with `.gitignore` for `google-services.json`)
- Disable FCM auto-initialization and analytics in manifest
- Create notification channels in `StellarChatApp.onCreate()`
- Request `POST_NOTIFICATIONS` permission in settings or on first group join
- Implement `StellarChatMessagingService` with `onMessageReceived()` and `onNewToken()`
- Implement `NotificationHelper` for notification display

### Phase 2: Push Subscription Store and PN Relay Client

- Implement `PushSubscriptionStore` using `EncryptedSharedPreferences`
- Implement `PNRelayClient` using OkHttp + `GroupCrypto.encryptInvitation()` for envelope encryption
- Wire registration into `GroupListViewModel` lifecycle (launch, join, leave, rekey, token refresh)
- Implement FCM token retrieval and refresh handling

### Phase 3: On-Device Decryption

- Implement full decryption pipeline in `onMessageReceived()`:
  notification_key layer → SealedEnvelope layer → v2 message parsing → notification display
- Handle edge cases: large payloads (wake-only), decryption failure (generic fallback), epoch transitions
- Test with APNs sandbox equivalent (FCM test messages)

### Phase 4: UnifiedPush Support

- Add UnifiedPush connector dependency
- Implement `StellarChatUnifiedPushReceiver`
- Add distributor detection and selection in Settings UI
- Implement registration/re-registration for UnifiedPush endpoints
- Test with ntfy as distributor

### Phase 5: PN Relay FCM + UP Dispatch

- Add FCM HTTP v1 dispatch to the PN relay (OAuth 2.0 service account auth)
- Add UnifiedPush HTTP POST dispatch to the PN relay
- Add platform routing logic
- Add `fcm-service-account.json` volume to Docker config
- Test end-to-end: Android FCM + Android UP + iOS APNs simultaneously

### Phase 6: Hardening

- End-to-end testing across all delivery paths
- Doze mode testing (high-priority bypass)
- OEM battery optimization guidance (Samsung, Xiaomi)
- Privacy audit (no plaintext tokens, no Firebase analytics, ephemeral keys discarded)
- Subscription garbage collection (FCM `UNREGISTERED`, UP endpoint failure)

---

## Files to Create

```
clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/push/
  StellarChatMessagingService.kt     -- FCM service, decrypt + display
  StellarChatUnifiedPushReceiver.kt  -- UnifiedPush receiver, decrypt + display
  PushSubscriptionStore.kt           -- Encrypted subscription state
  PNRelayClient.kt                   -- PN relay HTTP client
  NotificationHelper.kt              -- Notification channels and display
```

## Files to Modify

```
clients/android/StellarChat/app/src/main/AndroidManifest.xml
  -- Add POST_NOTIFICATIONS permission, FCM service, UP receiver, meta-data

clients/android/StellarChat/app/build.gradle.kts
  -- Add Firebase BOM, firebase-messaging, unifiedpush-connector

clients/android/StellarChat/build.gradle.kts
  -- Add google-services plugin

clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/StellarChatApp.kt
  -- Create notification channels in onCreate()

clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt
  -- Wire push registration into group lifecycle (join, leave, rekey)

pn-relay/src/
  -- Add FCM dispatch (OAuth 2.0 + HTTP v1 API)
  -- Add UnifiedPush dispatch (HTTP POST)
  -- Add platform routing in event handler
```

---

## References

- [Firebase Cloud Messaging — Data Messages](https://firebase.google.com/docs/cloud-messaging/concept-options#data_messages)
- [FCM HTTP v1 API](https://firebase.google.com/docs/cloud-messaging/send-message)
- [FCM Token Management Best Practices](https://firebase.google.com/docs/cloud-messaging/manage-tokens)
- [UnifiedPush Specification](https://unifiedpush.org/spec/)
- [UnifiedPush Android Connector](https://github.com/UnifiedPush/android-connector)
- [ntfy — Open Source Push Notifications](https://ntfy.sh/)
- [Android Doze Mode and FCM](https://developer.android.com/training/monitoring-device-state/doze-standby)
- [Android Notification Channels](https://developer.android.com/develop/ui/views/notifications/channels)
- [Element Android — UnifiedPush Integration](https://github.com/element-hq/element-android)
- Companion document: `push-notification-design.md` (iOS)
