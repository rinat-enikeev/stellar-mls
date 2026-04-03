# Postmortem: Secure Member Removal — From Accidental Discovery to Cryptographic Fix

**Date:** 2026-04-04
**Severity:** CRITICAL
**Duration of vulnerability:** From initial group chat implementation through 2026-04-04
**Platforms affected:** iOS and Android
**Status:** Resolved

---

## Timeline

| When | What |
|------|------|
| Pre-2026-04-03 | Groups use `HKDF(groupSecret ‖ epoch ‖ salt)` for traffic key derivation. Removal broadcasts next `epoch` + `salt` encrypted under old traffic key. `groupSecret` never rotates. |
| 2026-04-03 | **Security audit v4** traces the full removal path on both platforms and confirms: removed members can derive the next traffic key. The poisoned-salt + `SEPRekey` mitigation is timing-based, not cryptographic. |
| 2026-04-03 | Design doc (`secure-member-removal-design.md`) and implementation plan written. Core insight: need asymmetric per-recipient delivery of a *fresh* `groupSecret`. |
| 2026-04-03 | Phase 1 (authenticated `SEPMemberTransportBundle`) and Phase 2 (inbox `SEPRekeyEnvelope`) implemented on both platforms. |
| 2026-04-04 | Cross-platform testing begins: 3-member group (AX=Android1, I=iPhone, AE=Android2). AX creates group, invites I, invites AE. I removes AX. |
| 2026-04-04 | **Accidental discovery:** After removal, AE can still send messages — but AX (the removed member) receives them. I (the remover) can't send or receive. The "secure" removal made things *worse* for the remaining members while the removed member stayed in the loop. |
| 2026-04-04 | 6-hour debugging session. 5 distinct bugs found and fixed across both platforms. |
| 2026-04-04 | Final fix (concurrent relay publishing) applied. End-to-end rekey delivery confirmed. |

---

## 1. The Accidental Discovery

The secure member removal feature was designed to fix a critical vulnerability: removed members could derive the next traffic key and continue reading group messages. The fix was implemented, the code looked correct, the design was sound. Time to test.

**Test scenario:** 3-member group — AX (Android), I (iPhone), AE (Android). I removes AX.

**Expected outcome:** AX loses access. I and AE continue chatting on a new encrypted topic with a fresh `groupSecret` that AX never receives.

**Actual outcome:** The exact opposite of every security property we designed for.

- AX (removed member) **continued receiving messages** from AE
- I (the remover) **couldn't send or receive anything**
- AE (innocent bystander) **was stuck on the old epoch** sending messages that only AX could read

The system had created a situation where member removal *improved* the removed member's position (they still had access) while *degrading* the remaining members' ability to communicate. This was worse than doing nothing.

---

## 2. Root Cause Analysis

What appeared to be a single bug was actually 5 independent failures that conspired to produce the observed behavior. Each bug alone would have been manageable. Together, they made the secure rekey flow completely non-functional while leaving the old channel partially operational — the worst possible combination.

### Bug 1: `applyMemberChanges` didn't handle `SEPRemovalNotice`

**Severity:** HIGH
**Impact:** BLS sender authentication continued accepting messages from the removed member

**The problem:**

Both platforms maintain a `currentMembers` list in `NostrMessageTransport` for BLS aggregate signature verification (the H-4 security control). When a message arrives, the transport checks whether the sender's BLS public key is in `currentMembers`. If not, the message is rejected.

The `applyMemberChanges` method is called on every incoming protocol message to keep `currentMembers` synchronized. It handled `SEPMemberJoined` and `SEPGroupStateUpdate` — but not the newly introduced `SEPRemovalNotice`.

When AX was removed, I broadcast an `SEPRemovalNotice` on the old group topic. AE's transport received it — but `applyMemberChanges` didn't strip AX from `currentMembers`. So AE's transport continued accepting messages signed by AX as if they were from a valid member.

Meanwhile, I (the remover) had already switched to a new topic derived from the fresh `groupSecret`. Messages sent by AE on the old topic never reached I, and I's messages on the new topic never reached AE.

**Files:**

- iOS: `clients/ios/StellarChat/StellarChat/Nostr/NostrMessageTransport.swift` — `applyMemberChanges`
- Android: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/nostr/NostrMessageTransport.kt` — `applyMemberChanges`

**Fix:**

Added `SEPRemovalNotice.messageType` case to `applyMemberChanges` on both platforms:

```swift
// SEPRemovalNotice — strip removed members immediately so BLS rejects their messages
if let notice = try? decoder.decode(SEPRemovalNotice.self, from: data) {
    for removed in notice.removedMemberKeys {
        currentMembers.removeAll { $0.publicKeyCompressed == removed }
    }
}
```

### Bug 2: Rekey envelopes published through wrong transport

**Severity:** CRITICAL
**Impact:** Rekey envelopes never reached recipient inboxes

**The problem:**

The codebase has two separate transport layers:

1. `NostrMessageTransport` (`chatTransport`) — handles group messages on topic-based channels
2. `InvitationTransport` (`invitationTransport`) — handles per-recipient inbox delivery (kind 34113)

These are separate classes with separate WebSocket connection pools. `chatTransport` subscribes to group topics. `invitationTransport` subscribes to inbox tags and publishes invitation events.

The rekey envelope is a per-recipient inbox-delivered payload — it uses kind 34113 with hidden inbox tags, just like initial group invitations. It *must* go through `invitationTransport`.

But on both platforms, three call sites were using `chatTransport` to publish kind 34113 events:

1. Initial rekey envelope send during `performEpochTransition`
2. Resend to a specific requester (on `SEPRekeyResendRequest`)
3. Periodic resend to unacked members

These events were published to the group topic's relay connections, but with kind 34113 tags that no group-topic subscription would match. The events went into the void.

**Files:**

- iOS: `clients/ios/StellarChat/StellarChat/StellarChatApp.swift` — 3 call sites in `performEpochTransition` and rekey resend logic
- Android: `clients/android/StellarChat/app/src/main/java/com/stellarmls/chat/viewmodel/GroupListViewModel.kt` — 3 equivalent call sites

**Fix:**

Changed all 3 call sites on each platform from `chatTransport.publishToRelays(event)` to `invitationTransport.publishToRelays(event, relayURLs: relayURLs)`.

### Bug 3: `invitationTransport.publishToRelays` had no connections and failed silently

**Severity:** HIGH
**Impact:** Even after routing to the correct transport, events were never sent

**The problem:**

After fixing Bug 2, the rekey envelopes were routed to `invitationTransport` — but its connection pool was empty. The `invitationTransport` connects to relays during initial inbox subscription setup at app launch. But `publishToRelays` was a new method that could be called at any time during the app lifecycle.

On iOS, the original method:

```swift
func publishToRelays(_ event: NostrEvent, relayURLs: [URL] = []) async throws {
    // connections was empty — this loop executed zero times
    var published = false
    for conn in connections.values {
        try await conn.publish(event: event)
        published = true
    }
    // !published was true, but !connections.isEmpty was false
    // so this guard didn't throw — silent success with zero publishes
    if !published && !connections.isEmpty {
        throw ChatError.relayPublishFailed
    }
}
```

The condition `!published && !connections.isEmpty` was designed to only throw when connections existed but all failed. When connections were empty, it returned successfully — silently doing nothing.

On Android, the `publishToRelays` method had the same empty-connections problem.

**Fix:**

Added auto-reconnect when connections are empty and `relayURLs` are provided, and changed the guard to always throw when nothing was published:

```swift
if connections.isEmpty && !relayURLs.isEmpty {
    await connect(to: relayURLs)
}
// ...
if !published {
    throw ChatError.relayPublishFailed
}
```

### Bug 4: Removal notice `sendProtocolMessage` blocked rekey delivery

**Severity:** CRITICAL
**Impact:** The rekey envelope loop never executed at all

**The problem:**

This was the most insidious bug. In `performEpochTransition`, the removal flow was structured as:

```swift
// Step 1: Send removal notice on old topic
try await chatTransport.sendProtocolMessage(
    notice, topic: currentGroup.topicTag, key: previousKey, keyManager: keyManager
)

// Step 2: Send rekey envelopes to remaining members' inboxes
for member in remainingMembers { ... }
```

`sendProtocolMessage` publishes to relays **sequentially**. The publish operation on `NostrRelayConnection` uses a timeout of 5 seconds per relay. With multiple relays and some being slow or unreachable, this await could block for 15-30+ seconds.

But the real issue was worse: some relays would accept the WebSocket write but never send back an OK response, causing the publish to hang *indefinitely* (no timeout on the OK response, only on the `.send()` call).

The removal notice was a best-effort courtesy message — it tells the removed member "you've been removed" and tells remaining members to stop accepting messages from the removed member. It is NOT on the critical path. But because it was `await`ed synchronously, it blocked the *entire* rekey envelope delivery loop.

In testing, the last log line visible was `>>> SENDING REMOVAL NOTICE on old topic` — followed by silence. The rekey loop never printed `>>> REKEY LOOP`.

**Fix:**

Made the removal notice fire-and-forget:

```swift
let noticeTopic = currentGroup.topicTag
let noticeKey = previousKey
Task { [weak self] in
    guard let self else { return }
    do {
        try await self.chatTransport.sendProtocolMessage(
            notice, topic: noticeTopic, key: noticeKey, keyManager: self.keyManager
        )
    } catch {
        #if DEBUG
        print("[EpochTransition] Removal notice failed (non-critical): \(error)")
        #endif
    }
}
// Rekey loop now executes immediately
```

### Bug 5: Sequential relay publishing in `InvitationTransport` hung on slow relays

**Severity:** HIGH
**Impact:** Even after all other fixes, the rekey event never reached any relay

**The problem:**

After fixing Bugs 1-4, the iOS logs finally showed the rekey loop executing:

```
>>> REKEY LOOP: 2 members, envelopeLen=1543
>>> REKEY MEMBER b1921d68: hasBundle=true isSelf=false
[EpochTransition] Sending inbox rekey to b1921d68 inboxTag=a606b08f25d6f959 eventID=8ca673a28cf9
```

But the log line that should have followed — `[Invite] publishToRelays: published=true` — never appeared. And Android AE never received the event.

The `publishToRelays` method was publishing to relays **sequentially**:

```swift
for conn in connections.values {
    do {
        try await conn.publish(event: event)   // blocks on each relay
        published = true
    } catch { ... }
}
```

If the first relay in the dictionary iteration order was slow or hanging (DNS resolution failure, TLS handshake stall, overloaded relay), every subsequent relay would be blocked. The `NostrRelayConnection.publish` method has a 5-second timeout on the WebSocket `.send()`, but network-level hangs (TCP connect timeout, TLS negotiation) can exceed this and block the entire chain.

**Fix:**

Changed to concurrent publishing with `withTaskGroup`:

```swift
let conns = Array(connections.values)
let published = await withTaskGroup(of: Bool.self) { group in
    for conn in conns {
        group.addTask {
            do {
                try await conn.publish(event: event)
                return true
            } catch {
                return false
            }
        }
    }
    var any = false
    for await result in group {
        if result { any = true }
    }
    return any
}
```

Now all relays are attempted concurrently. One hanging relay doesn't block others. Applied to both `publishToRelays` and `sendInvitation` on iOS.

---

## 3. The Self-Removal Handler Gap

**Severity:** MEDIUM
**Impact:** Removed member's local state was inconsistent

In addition to the 5 critical-path bugs, the `SEPRemovalNotice` self-removal handler (when a member processes a notice that removes *themselves*) was incomplete on both platforms. It only inserted a system message and optionally unsubscribed from the topic.

Missing from the original handler:

- Updating the local epoch to match the removal notice epoch
- Removing self from the local member list
- Persisting the updated group state
- Rebuilding `chatTransport.currentMembers` for BLS validation

This meant the removed member's local state was stale — their epoch counter didn't advance, their member list still included themselves, and subsequent protocol processing used incorrect state.

**Fix (both platforms):**

```swift
if selfRemoved {
    if let idx = self.groups.firstIndex(where: { $0.id == groupID }) {
        var g = self.groups[idx]
        g.epoch = notice.epoch
        if let myBls = try? self.keyManager.blsPublicKey {
            g.members.removeAll { $0.publicKeyCompressed == myBls }
        }
        self.groups[idx] = g
        self.store.saveGroup(g)
        self.chatTransport.currentMembers = self.groups.flatMap(\.members)
    }
    self.insertSystemMessage(
        groupID: groupID,
        text: "You were removed from this group",
        event: "self-removed",
        epoch: notice.epoch
    )
}
```

---

## 4. How the Bugs Interacted

The devastating user-visible behavior — removed member keeps access, remaining members lose communication — was the result of all 5 bugs composing:

```
Bug 4: Removal notice blocks     ──→ Rekey loop never runs
Bug 2: Wrong transport            ──→ Even if loop ran, events go nowhere
Bug 3: Empty connections          ──→ Even if right transport, no relays connected
Bug 5: Sequential publish hangs   ──→ Even if connected, one slow relay blocks all
Bug 1: No BLS membership update   ──→ Removed member's messages still accepted
```

**The result:**

1. I sends `SEPRemovalNotice` on old topic (Bug 4 blocks here)
2. Rekey loop never executes (Bugs 2, 3, 5 are irrelevant — never reached)
3. AE receives `SEPRemovalNotice` but doesn't update `currentMembers` (Bug 1)
4. AE stays on old topic with old key, still accepting AX's messages
5. I switches to new topic with new key — alone
6. AX receives the removal notice, but since no rekey happened, AE is still on the old topic sending messages AX can read

If only Bug 4 existed, the rekey would have eventually delivered (assuming Bugs 2/3/5 didn't exist), and the system would have self-healed. If only Bug 1 existed, at least AX's messages would have been rejected after the notice. The combination of all five created the worst possible outcome.

---

## 5. The Security Vulnerability That Started Everything

### What the audit found

The original vulnerability, documented in `docs/audit-4.md`, was fundamental:

> The repository does not currently provide cryptographic exclusion of removed members. On both clients, the traffic key is deterministically derived from a long-lived `groupSecret`, `epoch`, and `salt`, while the state update that announces the next epoch is encrypted with the previous epoch key and includes the new `epoch` and `salt`. A removed insider who still knows `groupSecret` can decrypt that update, derive the next traffic key, and continue reading or writing on the group channel.

The key derivation was: `HKDF(groupSecret ‖ epoch ‖ salt)`

When a member was removed:
1. `epoch` incremented, new `salt` generated
2. State update broadcast on old channel, encrypted with previous traffic key
3. Removed member still knew `groupSecret` (never rotated)
4. Removed member could decrypt the update (they had the previous key)
5. Removed member now had all three inputs: `groupSecret`, `epoch'`, `salt'`
6. Removed member derived the new traffic key

**The poisoned-salt mitigation** tried to fix this by:
1. Broadcasting with a *fake* salt
2. Sending the *real* salt in a follow-up `SEPRekey` message
3. Hoping the removed member would unsubscribe before receiving the rekey

This failed because (per `docs/secure-member-removal-design.md` §3.2):

> A malicious client can ignore the unsubscribe. Any custom client that keeps listening to the old topic can: decrypt the poisoned-salt state update, derive the poisoned-key, decrypt the `sep_rekey`, recover the real salt, continue deriving the new traffic key.

### What the fix achieved

The secure member removal redesign introduced:

1. **`SEPMemberTransportBundle`** — an Ed25519-signed binding of each member's BLS group identity to their X25519 inbox encryption key, giving the remover an authenticated asymmetric delivery target

2. **`groupSecret` rotation** — removals now generate a completely fresh `groupSecret'` (not just a new salt), so the removed member loses the base secret entirely

3. **`SEPRekeyEnvelope`** — the new secrets are delivered per-recipient to each remaining member's X25519 inbox, encrypted so only the intended recipient can decrypt

4. **`SEPRemovalNotice`** — the old group channel only carries a non-secret notice (group ID, epoch, removed members, commitment). No `groupSecret'`, no `salt'`, no derivation seeds

5. **Topic migration** — since `groupSecret` changes, `hiddenGroupTopic` (derived from `SHA-256("sep-topic-v1" ‖ groupSecret)`) changes automatically. Remaining members subscribe to the new topic. The removed member doesn't know the new `groupSecret` and can't compute the new topic.

This creates the asymmetry that the original design lacked: remaining members have X25519 private inbox keys that the removed member does not possess. The new epoch secret material is delivered *only* through those inboxes.

---

## 6. Debugging Methodology

The 6-hour debugging session progressed through several phases of increasing instrumentation:

### Phase 1: Behavioral observation (30 min)
- Identified that AE stayed on epoch 2 while I moved to epoch 3
- AX (removed) was receiving AE's messages
- I was completely isolated

### Phase 2: Log tracing on receiver side (1 hr)
- Added debug logging to Android's `applyRekeyEnvelope` — discovered it was never called
- Checked Android's `InvitationTransport` — no rekey events received
- Confirmed: the problem was sender-side, not receiver-side

### Phase 3: Log tracing on sender side (2 hrs)
- Added `>>> EPOCH TRANSITION START` marker at top of `performEpochTransition`
- Added `>>> REKEY LOOP` and `>>> REKEY MEMBER` tracing
- Discovered: no epoch transition logs appeared at all despite being in `#if DEBUG`
- Rebuilt iOS — discovered the log stopped at `>>> SENDING REMOVAL NOTICE on old topic`
- Diagnosed Bug 4: `sendProtocolMessage` was hanging

### Phase 4: Transport layer diagnosis (1.5 hrs)
- After making removal notice fire-and-forget, rekey loop finally executed
- Logs showed `[EpochTransition] Sending inbox rekey to b1921d68`
- But `[Invite] publishToRelays:` log never appeared
- Hypothesized: `invitationTransport` connections were empty
- Confirmed: the rekey was being sent through `chatTransport` (Bug 2)
- After fixing transport routing, discovered empty connections (Bug 3)

### Phase 5: Relay publishing diagnosis (1 hr)
- After all fixes, iOS showed the full rekey send chain executing
- But Android still didn't receive the event
- iOS log ended at `Sending inbox rekey to...` — no `publishToRelays` confirmation
- Diagnosed Bug 5: sequential publish blocking on slow relay
- Fixed with concurrent `withTaskGroup` publishing

### Key debugging insight

The hardest bug to find was Bug 4 (removal notice blocking). It was hard because:

- The hang was *before* the interesting code, not *in* it
- The hanging function (`sendProtocolMessage`) worked fine in normal operations
- The hang was timing-dependent — some relays were slower than others
- The symptom (no rekey logs) looked like the rekey code wasn't being reached, which led to initially suspecting logic errors in the `allMembersHaveBundles` check

The breakthrough was adding a `>>> SENDING REMOVAL NOTICE` log *before* the `sendProtocolMessage` call and seeing it as the absolute last line in the log output.

---

## 7. What We Got Right

Despite the bugs, several design decisions proved correct and made the eventual fix possible:

1. **Reusing `InvitationTransport` for rekey delivery** — The existing X25519 inbox infrastructure (encryption, hidden tags, relay delivery, subscription) worked correctly once events were routed through it. No new crypto needed.

2. **`SEPMemberTransportBundle` design** — The authenticated binding of BLS identity to X25519 inbox key worked as designed. Signature verification caught test cases with mismatched keys.

3. **`allMembersHaveBundles` gate** — The backward-compatibility check correctly prevented secure rekey in groups where some members hadn't sent their transport bundle yet, falling back to the legacy flow instead of silently failing.

4. **Separation of `SEPRemovalNotice` from `SEPRekeyEnvelope`** — Keeping the removal notice non-secret (no new `groupSecret` or `salt`) meant that even when it was the only message that worked correctly (Bug 4 scenario), it didn't leak any secret material to the removed member.

5. **Topic migration via `groupSecret` rotation** — Changing `groupSecret` automatically changes the derived topic, creating a clean cryptographic boundary. No extra topic-management code needed.

---

## 8. Lessons Learned

### Transport routing is a critical design boundary

The existence of two transport layers (`chatTransport` for group messages, `invitationTransport` for inbox delivery) created a subtle but critical routing decision for every publish call. Using the wrong one produces no error — the event is successfully serialized, successfully sent to a WebSocket, and successfully *ignored* by every subscription because the kind/tag combination doesn't match any active filter.

**Action:** Review all `publish` call sites to ensure they use the correct transport for the event kind.

### Sequential relay publishing is an anti-pattern for real-time delivery

When N relays are queried sequentially, the worst-performing relay determines the latency for the entire operation. For time-sensitive operations (rekey delivery, invitation send), one hanging relay can block all others indefinitely.

**Action:** All relay fan-out operations should use concurrent publishing (`withTaskGroup` on iOS, coroutine-based on Android). This was already the pattern in `NostrMessageTransport.sendProtocolMessage` — the fact that `InvitationTransport` used sequential publishing was an inconsistency.

### Fire-and-forget for non-critical protocol messages

The removal notice is a courtesy — it tells participants about the removal but carries no secret material. Blocking the critical rekey delivery path on a courtesy message was an unnecessary coupling.

**Action:** Any protocol message that is advisory (not on the critical path for security) should be sent fire-and-forget. Log failures but don't block.

### Guard conditions that silently succeed are worse than crashes

The `if !published && !connections.isEmpty` guard in `publishToRelays` was designed to be lenient — don't throw if there are no connections (maybe they'll connect later). In practice, this silent success hid a total delivery failure.

**Action:** Prefer failing loudly. If a publish operation publishes to zero relays, that is always an error. The caller can decide how to handle it.

### BLS membership must be updated atomically with protocol state changes

`applyMemberChanges` is the single point where BLS sender validation state is synchronized with protocol events. If a new protocol message type is introduced (like `SEPRemovalNotice`) and `applyMemberChanges` isn't updated, the BLS membership list drifts from protocol reality.

**Action:** Any new protocol message type that modifies group membership MUST have a corresponding case in `applyMemberChanges`. Add this to the protocol message checklist.

### Test the composition, not just the components

Each bug was individually straightforward. The transport worked. The crypto worked. The protocol messages were well-formed. The relay connections functioned. But the composition — "route this encrypted envelope through this transport to these relays for this recipient's inbox" — had never been exercised end-to-end across platforms.

**Action:** Cross-platform integration tests for the full removal flow: remove a member, verify the removed member can't read new messages, verify remaining members can communicate on the new topic.

---

## 9. Prevention Measures

### Immediate

- [x] Concurrent relay publishing in `InvitationTransport` on iOS
- [x] Fire-and-forget removal notice on both platforms
- [x] `applyMemberChanges` handles `SEPRemovalNotice` on both platforms
- [x] Rekey envelopes routed through `invitationTransport` on both platforms
- [x] `publishToRelays` auto-reconnects and throws on zero publishes

### Short-term

- [ ] Add integration test: 3 members, remove one, verify remaining members' epoch advances
- [ ] Add integration test: removed member stays subscribed, verify they cannot derive new key
- [ ] Add `publishToRelays` concurrent pattern to Android `InvitationTransport` (currently fire-and-forget but synchronous)
- [ ] Add timeout to `NostrRelayConnection.publish` at the TCP/TLS level, not just WebSocket send

### Long-term

- [ ] Protocol message type registry that enforces `applyMemberChanges` coverage
- [ ] Transport routing lint: kind 34113 events must go through `invitationTransport`
- [ ] Relay health monitoring: detect and skip persistently hanging relays

---

## 10. Related Documents

- [Security Audit v4](audit-4.md) — the original finding that removal is not cryptographic eviction
- [Secure Member Removal Design](secure-member-removal-design.md) — the correct end-state design
- [Implementation Plan](implementation_plan.md) — phased implementation with file-level detail

---

## Appendix A: Affected Files

| File | Bugs Fixed |
|------|------------|
| `clients/ios/StellarChat/StellarChat/StellarChatApp.swift` | #2 (transport routing), #4 (fire-and-forget notice), self-removal handler |
| `clients/ios/StellarChat/StellarChat/Nostr/InvitationTransport.swift` | #3 (auto-reconnect), #5 (concurrent publish) |
| `clients/ios/StellarChat/StellarChat/Nostr/NostrMessageTransport.swift` | #1 (applyMemberChanges) |
| `clients/android/.../viewmodel/GroupListViewModel.kt` | #2 (transport routing), #4 (fire-and-forget notice), self-removal handler |
| `clients/android/.../nostr/InvitationTransport.kt` | #3 (auto-reconnect, publishToRelays method) |
| `clients/android/.../nostr/NostrMessageTransport.kt` | #1 (applyMemberChanges) |

## Appendix B: The Five Bugs in Dependency Order

```
                    ┌─────────────────────────────────┐
                    │ performEpochTransition           │
                    │ (member removal)                 │
                    └─────────────┬───────────────────┘
                                  │
                    ┌─────────────▼───────────────────┐
                    │ Bug 4: Removal notice hangs      │
                    │ sendProtocolMessage blocks       │
                    │ indefinitely on slow relay        │
                    └─────────────┬───────────────────┘
                                  │ (fixed: fire-and-forget Task{})
                                  │
                    ┌─────────────▼───────────────────┐
                    │ Rekey envelope loop              │
                    │ for each remaining member...     │
                    └─────────────┬───────────────────┘
                                  │
                    ┌─────────────▼───────────────────┐
                    │ Bug 2: Wrong transport           │
                    │ chatTransport.publishToRelays    │
                    │ instead of invitationTransport   │
                    └─────────────┬───────────────────┘
                                  │ (fixed: route to invitationTransport)
                                  │
                    ┌─────────────▼───────────────────┐
                    │ Bug 3: Empty connections          │
                    │ invitationTransport had no        │
                    │ active relay connections           │
                    └─────────────┬───────────────────┘
                                  │ (fixed: auto-reconnect with relayURLs)
                                  │
                    ┌─────────────▼───────────────────┐
                    │ Bug 5: Sequential publish         │
                    │ One slow relay blocks all others  │
                    └─────────────┬───────────────────┘
                                  │ (fixed: concurrent withTaskGroup)
                                  │
                    ┌─────────────▼───────────────────┐
                    │ Rekey event arrives at Android    │
                    │ InvitationTransport               │
                    └─────────────┬───────────────────┘
                                  │
                    ┌─────────────▼───────────────────┐
                    │ Bug 1: BLS membership stale      │
                    │ applyMemberChanges ignores        │
                    │ SEPRemovalNotice                   │
                    └─────────────────────────────────┘
                      (fixed: added SEPRemovalNotice case)
```

Every bug in the chain had to be fixed before the next one could even be observed. This is why the debugging took 6 hours for what turned out to be 5 individually simple fixes.
