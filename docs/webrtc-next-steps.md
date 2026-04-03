# WebRTC Next Steps: Configurable TURN & Diagnostics

## Context

The initial WebRTC implementation had several bugs preventing calls from connecting:

1. **`sendSignal` not configured for incoming calls** — the callee could never send the
   answer SDP back. Fixed by wiring `sendSignal` in `setupCallSignalHandler()` on both
   platforms using the groupID from the incoming event.
2. **No first-answer-wins protection** — in groups with 3+ members, multiple answers could
   corrupt the peer connection. Fixed by tracking `answerReceived` and dismissing late
   answerers with `hangup(reason: "answered")`.
3. **iOS deprecated track delivery** — `didAdd stream:` unreliable with unified plan SDP.
   Fixed by adding `peerConnection(_:didAdd:rtpReceiver:streams:)`.
4. **iOS missing video codec factories** — bare `RTCPeerConnectionFactory()` without
   encoder/decoder config. Fixed by using `RTCDefaultVideoEncoderFactory` /
   `RTCDefaultVideoDecoderFactory`.
5. **STUN-only ICE config** — no TURN servers for restrictive NATs. Fixed by adding EU-based
   Metered TURN servers (hardcoded credentials for MVP).

This document covers the follow-up work.

---

## 1. Configurable TURN Settings

### Current State

TURN credentials are hardcoded in `CallManager` on both platforms. This is fine for MVP but
needs to be configurable for:

- Self-hosted TURN deployments (privacy-conscious users)
- Credential rotation
- Multiple TURN providers

### Design

Add a TURN config model on each platform:

```
turnURLs: [String]         // e.g. ["turn:my-server.eu:443?transport=tcp"]
username: String
password: String
enabled: Bool
```

**Persistence:**
- iOS: `UserDefaults` for URLs and enabled flag, Keychain for credentials (matching existing
  relay auth patterns)
- Android: `SharedPreferences` for URLs and enabled flag, `EncryptedSharedPreferences` or
  KeyStore for credentials

**UI:** Add a "TURN Servers" section in Advanced Settings on both platforms:
- Toggle to enable/disable TURN
- Editable URL list
- Username / password fields
- Validation: `turn:` and `turns:` schemes required, non-empty credentials when enabled

**ICE server construction:** At call start, build the ICE server list dynamically:
- Always include public STUN servers
- Append configured TURN servers when enabled and valid

### Recommended Providers (EU)

- **Metered** (metered.ca) — EU nodes, usage-based pricing, simple API
- **ExpressTURN** (expressturn.com) — EU infrastructure, not US-hosted
- **Self-hosted coturn** — full privacy, requires server management

---

## 2. ICE Candidate Queuing

### Problem

ICE candidates can arrive (via the Nostr signaling channel) before `setRemoteDescription`
completes on the receiving side. When this happens, `addIceCandidate` silently fails because
there's no remote description to associate the candidate with.

### Fix

Add a `pendingCandidates: [RTCIceCandidate]` buffer on both platforms.

In the `"ice"` handler:
- If `peerConnection?.remoteDescription != nil` → add candidate immediately
- Otherwise → append to `pendingCandidates`

After `setRemoteDescription` succeeds (in both `"offer"` and `"answer"` handlers):
- Drain `pendingCandidates` by calling `addIceCandidate` for each
- Clear the buffer

---

## 3. Connection Diagnostics

### Visible State

Expose to the call UI:

- **ICE connection status**: gathering / checking / connected / disconnected / failed
- **Call failure reason**: timeout, ICE failed, remote busy, remote rejected, answered
  elsewhere
- **TURN active**: whether relay candidates are present (indicates TURN is working)

### Implementation

- Add an `iceConnectionStatus` observable property to `CallManager`
- Update it from the `didChange newState: RTCIceConnectionState` delegate callback
- Add a `callEndReason` enum/string set on `endCall` with the specific cause
- Display connection status on the call screen (subtle indicator, not intrusive)

### Debug Logging

Add `#if DEBUG` / `BuildConfig.DEBUG` logging for:
- Selected ICE server set at call start
- ICE connection state transitions
- Each ICE candidate type discovered (host / srflx / relay)
- SDP offer/answer exchange timing
- Call end reason

---

## 4. Self-Hosted TURN for Privacy

The existing doc (`sep-voice-video-calls.md` §5.1) notes that TURN servers can observe peer
IP addresses and call duration. For privacy-sensitive deployments:

- Document a recommended coturn configuration
- Support long-term credential mechanism (RFC 5389) or time-limited credentials via HMAC
- Consider distributing TURN credentials via the group channel (encrypted, so relays can't
  see them)

This is a longer-term item and separate from the client-side configuration work.
