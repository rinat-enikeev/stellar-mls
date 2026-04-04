# Prevention Plan: Secure Member Removal Incident Classes

**Date:** 2026-04-04
**Based on:** [postmortem-secure-member-removal.md](postmortem-secure-member-removal.md)
**Status:** Active

---

## Background

The secure member removal postmortem revealed 5 compounding bugs that made the feature
worse than not having it. Each bug was simple in isolation — the damage came from their
interaction and the fact that each was invisible until the one before it was fixed.

This document defines prevention phases to eliminate the *classes* of failure, not just
the individual bugs.

---

## Phase 0 — Immediate: Close Remaining Gaps (2026-04-04)

### 0.1 Fix iOS `sendInvitation` silent failure

**Status:** DONE

`InvitationTransport.sendInvitation` on iOS still had the old guard `if !published && !conns.isEmpty`
which silently succeeds when connections are empty. Changed to `if !published`.

**File:** `clients/ios/StellarChat/StellarChat/Nostr/InvitationTransport.swift`

### 0.2 Fix Android `publishToRelays` silent failure

**Status:** DONE

Android `InvitationTransport.publishToRelays` had no error indication at all — events
published into the void with no exception or return value. Now throws
`IllegalStateException` when no connected relay accepts the event.

**File:** `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/nostr/InvitationTransport.kt`

### 0.3 Audit all publish call sites for error handling

**Status:** TODO

Every `publish` or `publishToRelays` call site across both platforms must be checked:

- Does the caller handle the error?
- If the publish fails, does the user or retry logic know about it?
- Are fire-and-forget publishes intentional and documented?

**Files to audit:**

| Platform | File | Methods |
|----------|------|---------|
| iOS | `InvitationTransport.swift` | `sendInvitation`, `publishToRelays` |
| iOS | `NostrMessageTransport.swift` | `publishToRelays`, `sendProtocolMessage` |
| iOS | `StellarChatApp.swift` | All call sites that publish events |
| Android | `InvitationTransport.kt` | `sendInvitation`, `publishToRelays` |
| Android | `NostrMessageTransport.kt` | `send`, `sendProtocolMessage`, `publish` |
| Android | `GroupListViewModel.kt` | All call sites that publish events |

---

## Phase 1 — Transport Routing Safety

**Problem class:** Kind 34113 (inbox) events published through the wrong transport
(chatTransport instead of invitationTransport). Events are well-formed but reach zero
subscribers because the transport's subscriptions don't match the event kind/tags.

### 1.1 Centralize event publishing behind kind-aware routing

Instead of requiring every call site to choose the correct transport, create a single
publish entry point that routes based on event kind:

```swift
// iOS
func publishEvent(_ event: NostrEvent, relayURLs: [URL]) async throws {
    switch event.kind {
    case 34113:
        try await invitationTransport.publishToRelays(event, relayURLs: relayURLs)
    case 44114:
        try await chatTransport.publishToRelays(event)
    default:
        assertionFailure("Unknown event kind \(event.kind)")
    }
}
```

```kotlin
// Android
fun publishEvent(event: NostrEvent, relayURLs: List<String>) {
    when (event.kind) {
        34113 -> invitationTransport.publishToRelays(event, relayURLs)
        44114 -> transport.publish(event)
        else -> error("Unknown event kind ${event.kind}")
    }
}
```

**Benefit:** Eliminates an entire class of routing errors. New event kinds must be
explicitly registered. Wrong-transport bugs become compilation or assertion failures
instead of silent message loss.

### 1.2 Add debug assertion for transport/kind mismatch

Until 1.1 is implemented, add runtime checks:

```swift
// In InvitationTransport.publishToRelays
assert(event.kind == 34113, "InvitationTransport received non-inbox event kind \(event.kind)")

// In NostrMessageTransport.publishToRelays
assert(event.kind == 44114, "MessageTransport received non-chat event kind \(event.kind)")
```

These fire only in DEBUG but catch routing mistakes during development.

### 1.3 Document transport boundaries

Add a comment block at the top of each transport class:

```
// InvitationTransport: handles kind 34113 (inbox-addressed, per-recipient)
// NostrMessageTransport: handles kind 44114 (topic-addressed, group broadcast)
// Do NOT cross-publish between these transports.
```

---

## Phase 2 — Publish Reliability

**Problem class:** Events published but never actually delivered — empty connections,
silent failures, sequential blocking, no delivery confirmation.

### 2.1 Fail loud on zero publishes (all platforms, all transports)

Every publish method must throw/return error when zero relays accepted the event.
The pattern `if !published && !connections.isEmpty` is banned — it hides total
delivery failure when connections are empty.

**Rule:** `if !published { throw }` — always, unconditionally.

Audit checklist:

| Method | iOS | Android |
|--------|-----|---------|
| `InvitationTransport.publishToRelays` | DONE | DONE |
| `InvitationTransport.sendInvitation` | DONE | TODO |
| `NostrMessageTransport.publishToRelays` | CHECK | N/A (different pattern) |
| `NostrMessageTransport.sendProtocolMessage` | CHECK | TODO |

### 2.2 Auto-reconnect on publish

All `publishToRelays` methods must accept relay URLs and auto-reconnect when
connections are empty, rather than silently failing.

**Current status:**
- iOS `InvitationTransport.publishToRelays`: DONE
- Android `InvitationTransport.publishToRelays`: DONE
- iOS `NostrMessageTransport.publishToRelays`: Does NOT auto-reconnect — relies on
  existing connections. Should be reviewed.
- Android `NostrMessageTransport`: No `publishToRelays` equivalent — publishes are
  inline in `send`/`sendProtocolMessage`. No auto-reconnect.

### 2.3 Concurrent relay fan-out (iOS)

All iOS publish operations must use `withTaskGroup` for concurrent relay publishing.
Sequential `for conn in connections` awaited publish is banned on iOS because
`URLSessionWebSocketTask.send` can block on slow relays.

**Current status:**
- `InvitationTransport.publishToRelays`: DONE
- `InvitationTransport.sendInvitation`: DONE
- `NostrMessageTransport.publishToRelays`: CHECK

**Not required on Android:** OkHttp's `WebSocket.send()` is non-blocking (enqueues
and returns immediately). Sequential iteration is safe.

### 2.4 Publish confirmation logging

Every publish path should log success/failure in DEBUG mode with:
- Event ID (first 12 chars)
- Event kind
- Number of relays attempted
- Number of relays that accepted

This makes silent delivery failures immediately visible in logs.

---

## Phase 3 — Protocol Message Coverage

**Problem class:** New protocol message types introduced without updating all handlers
that need to know about them (`applyMemberChanges`, dispatch switches, etc.).

### 3.1 Protocol message type registry

Create a single source of truth for all SEP protocol message types and their
properties:

```swift
enum SEPMessageType: String, CaseIterable {
    case memberJoined = "sep_member_joined"
    case stateUpdate = "sep_group_state_update"
    case removalNotice = "sep_removal_notice"
    case rekeyEnvelope = "sep_rekey_envelope"
    case rekeyAck = "sep_rekey_ack"
    case rekeyResendRequest = "sep_rekey_resend_request"
    case saltRequest = "sep_salt_request"
    case saltResponse = "sep_salt_response"
    case groupRenamed = "sep_group_renamed"
    case messageAck = "sep_message_ack"
    // ... all types

    /// Whether this message type modifies group membership.
    var modifiesMembership: Bool {
        switch self {
        case .memberJoined, .stateUpdate, .removalNotice: return true
        default: return false
        }
    }
}
```

### 3.2 Exhaustive switch enforcement

On iOS, use Swift's exhaustive `switch` on the enum so adding a new message type
produces a compile error until all handlers are updated.

On Android, add a unit test that verifies every `MESSAGE_TYPE` constant in the
kotlin-mls SDK is handled in `applyMemberChanges` (for membership-modifying types)
and in the main protocol message dispatch.

### 3.3 Checklist for adding new protocol messages

When adding a new protocol message type:

1. Define in both `swift-mls/GroupStateUpdate.swift` and `kotlin-mls/GroupStateUpdate.kt`
2. Add to `isProtocolMessage` check on both platforms
3. If it modifies membership: add to `applyMemberChanges` on both platforms
4. Add dispatch case in the main protocol message handler on both platforms
5. Add to the type registry (3.1)
6. Add cross-platform test vector in `docs/cross-platform-test-vectors.json`

---

## Phase 4 — Critical Path Protection

**Problem class:** Non-critical operations (courtesy notices, logging, UI updates)
blocking security-critical operations (rekey delivery).

### 4.1 Identify and document the critical path

For secure member removal, the critical path is:

```
generate fresh groupSecret' + salt'
    → encrypt rekey envelope per recipient
        → publish to each recipient's inbox
            → recipient decrypts and installs new epoch
```

Everything else is non-critical:
- SEPRemovalNotice broadcast (courtesy)
- System message insertion (UI)
- Pending rekey persistence (reliability, not correctness)
- Rekey ack (optimization)

### 4.2 Rule: never await non-critical work on the critical path

Non-critical operations on the critical path must be either:
- Fire-and-forget (`Task { }` on iOS, `launch { }` on Android)
- Deferred to after the critical path completes

**Specific instances:**

| Operation | Platform | Current | Correct |
|-----------|----------|---------|---------|
| SEPRemovalNotice broadcast | iOS | Fire-and-forget `Task { }` | DONE |
| SEPRemovalNotice broadcast | Android | Synchronous (but non-blocking due to OkHttp) | OK for now, but should be explicit fire-and-forget for clarity |
| System message insertion | Both | Synchronous but fast | OK |
| Pending rekey persistence | Both | After rekey loop | OK |

### 4.3 Add timeout to the rekey delivery loop

The entire rekey delivery loop (all members) should have a bounded timeout.
If it takes more than 30 seconds, log an error and proceed — the retry mechanism
will handle undelivered envelopes.

```swift
// iOS
try await withThrowingTaskGroup(of: Void.self) { group in
    group.addTask {
        try await Task.sleep(for: .seconds(30))
        throw CancellationError()
    }
    group.addTask {
        for member in remainingMembers {
            // ... encrypt and publish rekey envelope
        }
    }
    try await group.next()
    group.cancelAll()
}
```

---

## Phase 5 — Cross-Platform Integration Testing

**Problem class:** Each component works in isolation but the composition fails
across platforms.

### 5.1 Automated cross-platform removal test

Build a test harness that runs the following scenario:

1. Device A (Android) creates group
2. Device B (iOS) joins
3. Device C (Android) joins
4. Device B removes Device A
5. **Assert:** Device A cannot decrypt messages after removal
6. **Assert:** Device B and C can exchange messages on new topic
7. **Assert:** Device B and C are on the same epoch
8. **Assert:** Device A's epoch did not advance to the new epoch's key material

### 5.2 Malicious client removal test

1. Device A joins group, records `groupSecret`
2. Device A is removed
3. Device A stays subscribed to old topic (does NOT unsubscribe)
4. Device A intercepts all Nostr events on old topic
5. **Assert:** Device A sees only `SEPRemovalNotice` (no secret material)
6. **Assert:** Device A cannot compute new `hiddenGroupTopic`
7. **Assert:** Device A cannot derive new traffic key from any intercepted data

### 5.3 Relay failure resilience test

1. 3-member group, 3 relays configured
2. Kill 2 of 3 relays
3. Remove a member
4. **Assert:** Rekey envelope delivered via the 1 surviving relay
5. **Assert:** Remaining members reach consensus on new epoch
6. Bring relays back
7. **Assert:** Pending rekey resends complete for any unacked members

### 5.4 Inbox delivery verification test

1. Device A sends rekey envelope to Device B's inbox
2. **Assert:** Event appears on at least 1 relay (query relay directly)
3. **Assert:** Event has correct kind (34113), correct inbox tags
4. **Assert:** Device B's inbox subscription receives the event
5. **Assert:** Device B can decrypt the envelope
6. **Assert:** Device B installs the new epoch

---

## Phase 6 — Operational Observability

**Problem class:** Failures are silent — no logs, no metrics, no alerts. The only
way to detect a failed rekey is when users report they can't communicate.

### 6.1 Structured logging for rekey flow

Replace ad-hoc `print` / `Log.d` with structured log events:

```
[Rekey] START groupID=<8hex> epoch=<n> members=<count> secure=<bool>
[Rekey] NOTICE_SENT groupID=<8hex> epoch=<n> (fire-and-forget)
[Rekey] ENVELOPE_SENT groupID=<8hex> epoch=<n> recipient=<8hex> eventID=<12hex> relays=<count>
[Rekey] ENVELOPE_FAILED groupID=<8hex> epoch=<n> recipient=<8hex> error=<msg>
[Rekey] COMPLETE groupID=<8hex> epoch=<n> sent=<count>/<total>
[Rekey] RECEIVED groupID=<8hex> epoch=<n> sender=<8hex>
[Rekey] INSTALLED groupID=<8hex> epoch=<n> newTopic=<8hex>
[Rekey] ACK_SENT groupID=<8hex> epoch=<n>
```

### 6.2 Rekey health check

On app foreground, check for groups where:
- Local epoch < any received message epoch (missed a rekey)
- Pending outgoing rekey with unacked members > 5 minutes old
- Group has no transport bundles for some members

Log warnings and trigger recovery (resend request) automatically.

### 6.3 Debug diagnostics command

Add a developer-facing diagnostic that dumps:
- All groups with epoch, member count, bundle count
- Pending rekeys with unacked member list
- Transport connection status (which relays connected, which not)
- Last successful publish timestamp per transport

---

## Phase 7 — Defensive Coding Standards

### 7.1 Ban silent-success patterns

The following patterns are banned across the codebase:

```
// BANNED: silent success when nothing happened
if !result && !collection.isEmpty { throw }

// CORRECT: always fail when nothing happened
if !result { throw }
```

```
// BANNED: empty loop that does nothing
for item in possiblyEmptyCollection { doWork(item) }
// (no check that collection was non-empty)

// CORRECT: guard non-empty
guard !collection.isEmpty else { throw/log/return error }
for item in collection { doWork(item) }
```

### 7.2 Require error paths for all publish operations

Every function that publishes to relays must have one of:
- A `throws` annotation (Swift) or `throws` declaration (Kotlin)
- A return value indicating success/failure
- An explicit `// fire-and-forget: failure is non-critical` comment explaining why

Functions that silently drop publish failures without documentation are considered bugs.

### 7.3 Require `applyMemberChanges` coverage for membership-modifying messages

Any PR that introduces a new protocol message type with `removedMemberKeys`,
`addedMembers`, or equivalent fields MUST include corresponding `applyMemberChanges`
updates on both platforms. CI should enforce this via a grep-based check:

```bash
# In CI: if a new MESSAGE_TYPE is added to GroupStateUpdate, check applyMemberChanges
new_types=$(git diff --name-only | xargs grep -l 'MESSAGE_TYPE.*=.*"sep_')
if [ -n "$new_types" ]; then
    # Verify applyMemberChanges is also modified
    grep -l 'applyMemberChanges' $(git diff --name-only) || \
        echo "WARNING: New message type added but applyMemberChanges not updated"
fi
```

---

## Implementation Priority

```
Phase 0 ━━━━━━━━━━━ DONE (immediate fixes)
Phase 1 ━━━━━━━━━━━ DONE (transport routing safety — publishEvent router + assertions)
Phase 2 ━━━━━━━━━━━ DONE (publish reliability — fail loud + logging on all transports)
Phase 3 ━━━━━━━━━━━ DONE (protocol coverage — SEPMessageType enum + CI script)
Phase 4 ━━━━━━━━━━━ DONE (critical path — fire-and-forget notice + 30s rekey timeout)
Phase 5 ━━━━━━━━━━━ SPEC (integration testing — test scenarios documented, manual for now)
Phase 6 ━━━━━━━━━━━ DONE (observability — structured [Rekey] logging + health check + diagnostics)
Phase 7 ━━━━━━━━━━━ DONE (coding standards — CI script + transport boundary docs)
```

All phases implemented on 2026-04-04. Phase 5 tests are specified but require
manual cross-device testing until automated test infrastructure is built.

---

## Success Criteria

This prevention plan is successful when:

1. A member removal on any platform reliably delivers rekey envelopes to all remaining
   members within 10 seconds
2. A removed member provably cannot derive the new traffic key under any subscription
   behavior
3. Adding a new protocol message type that affects membership produces a compile error
   or CI failure until all handlers are updated
4. Every publish operation either succeeds with confirmation or fails with an error —
   no silent drops
5. Cross-platform integration tests run on every PR that touches transport, crypto,
   or epoch transition code
