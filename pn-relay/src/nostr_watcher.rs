use std::collections::{HashSet, VecDeque};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use crate::apns::{self, ApnsClient};
use crate::config::Config;
use crate::crypto;
use crate::db::{self, Database};
use crate::fcm::{self, FcmClient};
use crate::unified_push::{self, UnifiedPushClient};

/// Run the Nostr relay watcher in a background task.
///
/// This connects to the Nostr relay, subscribes to events matching registered
/// subscription filters, and dispatches encrypted push notifications.
pub async fn run_watcher(
    nostr_relay_url: String,
    db: Database,
    storage_key: [u8; 32],
    config: Config,
    mut refresh_rx: tokio::sync::watch::Receiver<()>,
) {
    let mut apns_client = match ApnsClient::new(&config) {
        Ok(Some(c)) => Some(c),
        Ok(None) => {
            eprintln!("Watcher: APNs not configured, iOS pushes disabled");
            None
        }
        Err(e) => {
            eprintln!("Watcher: failed to initialize APNs client: {e}");
            None
        }
    };

    let mut fcm_client = match FcmClient::new(&config) {
        Ok(Some(c)) => Some(c),
        Ok(None) => {
            eprintln!("Watcher: FCM not configured, Android FCM pushes disabled");
            None
        }
        Err(e) => {
            eprintln!("Watcher: failed to initialize FCM client: {e}");
            None
        }
    };

    let up_client = UnifiedPushClient::new();

    let mut backoff_secs: u64 = 1;
    let max_backoff: u64 = 120;

    // Event deduplication: keep last 1000 event IDs
    let mut seen_events: VecDeque<String> = VecDeque::new();
    let mut seen_set: HashSet<String> = HashSet::new();

    loop {
        eprintln!("Watcher: connecting to {nostr_relay_url}");

        let ws_result = tokio_tungstenite::connect_async(&nostr_relay_url).await;

        let (mut ws_stream, _) = match ws_result {
            Ok(pair) => {
                backoff_secs = 1; // Reset backoff on successful connection
                eprintln!("Watcher: connected to Nostr relay");
                pair
            }
            Err(e) => {
                eprintln!(
                    "Watcher: connection failed: {e}, retrying in {backoff_secs}s"
                );
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(max_backoff);
                continue;
            }
        };

        // Subscribe to all registered tags
        if let Err(e) = send_subscriptions(&db, &mut ws_stream).await {
            eprintln!("Watcher: failed to send subscriptions: {e}");
            let _ = ws_stream.close(None).await;
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(max_backoff);
            continue;
        }

        // Main event loop
        loop {
            tokio::select! {
                msg = ws_stream.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            handle_message(
                                &text,
                                &db,
                                &storage_key,
                                &config,
                                &mut apns_client,
                                &mut fcm_client,
                                &up_client,
                                &mut seen_events,
                                &mut seen_set,
                            ).await;
                        }
                        Some(Ok(Message::Ping(data))) => {
                            let _ = ws_stream.send(Message::Pong(data)).await;
                        }
                        Some(Ok(Message::Close(_))) => {
                            eprintln!("Watcher: relay closed connection");
                            break;
                        }
                        Some(Err(e)) => {
                            eprintln!("Watcher: WebSocket error: {e}");
                            break;
                        }
                        None => {
                            eprintln!("Watcher: WebSocket stream ended");
                            break;
                        }
                        _ => {} // Ignore binary, pong, etc.
                    }
                }
                _ = refresh_rx.changed() => {
                    eprintln!("Watcher: subscription refresh signal received");
                    // Close current subscription and re-subscribe
                    let close_msg = json!(["CLOSE", "pn"]).to_string();
                    let _ = ws_stream.send(Message::Text(close_msg)).await;
                    let close_msg2 = json!(["CLOSE", "pn2"]).to_string();
                    let _ = ws_stream.send(Message::Text(close_msg2)).await;

                    if let Err(e) = send_subscriptions(&db, &mut ws_stream).await {
                        eprintln!("Watcher: failed to re-subscribe: {e}");
                        break;
                    }
                }
            }
        }

        let _ = ws_stream.close(None).await;

        eprintln!(
            "Watcher: disconnected, reconnecting in {backoff_secs}s"
        );
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(max_backoff);
    }
}

/// Send subscription filters to the Nostr relay.
async fn send_subscriptions<S>(
    db: &Database,
    ws: &mut S,
) -> Result<(), String>
where
    S: SinkExt<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    let all_tags = db.get_all_tags()?;

    if all_tags.is_empty() {
        eprintln!("Watcher: no subscriptions registered, sending minimal filter");
        // Subscribe with a filter that won't match anything to keep connection alive
        let req = json!(["REQ", "pn", {"kinds": [44114, 24114, 24113, 34113], "#t": ["__none__"]}]);
        ws.send(Message::Text(req.to_string()))
            .await
            .map_err(|e| format!("failed to send REQ: {e}"))?;
        return Ok(());
    }

    // Separate tags into #t tags and #sep_inbox tags by inspecting all filters
    let mut t_tags: HashSet<String> = HashSet::new();
    let mut sep_inbox_tags: HashSet<String> = HashSet::new();

    // We need to look at each subscription's filter to categorize tags properly
    // For simplicity, query all filters and extract by tag name
    let all_tag_values: Vec<String> = all_tags;

    // Get all filters to categorize
    if let Ok(filters) = get_all_filters(db) {
        for filter in &filters {
            let tags_by_name = db::extract_tags_by_name(filter);
            for (name, values) in tags_by_name {
                for v in values {
                    if name == "sep_inbox" {
                        sep_inbox_tags.insert(v);
                    } else {
                        t_tags.insert(v);
                    }
                }
            }
        }
    }

    // If we couldn't categorize, put all tags into #t
    if t_tags.is_empty() && sep_inbox_tags.is_empty() {
        for tag in &all_tag_values {
            t_tags.insert(tag.clone());
        }
    }

    // Send #t subscription
    if !t_tags.is_empty() {
        let t_tag_list: Vec<&str> = t_tags.iter().map(|s| s.as_str()).collect();
        let req = json!(["REQ", "pn", {
            "kinds": [44114, 24114, 24113, 34113],
            "#t": t_tag_list,
        }]);
        eprintln!(
            "Watcher: subscribing to {} #t tags",
            t_tag_list.len()
        );
        ws.send(Message::Text(req.to_string()))
            .await
            .map_err(|e| format!("failed to send #t REQ: {e}"))?;
    }

    // Send #sep_inbox subscription
    if !sep_inbox_tags.is_empty() {
        let inbox_list: Vec<&str> = sep_inbox_tags.iter().map(|s| s.as_str()).collect();
        let req = json!(["REQ", "pn2", {
            "kinds": [44114, 24114, 24113, 34113],
            "#sep_inbox": inbox_list,
        }]);
        eprintln!(
            "Watcher: subscribing to {} #sep_inbox tags",
            inbox_list.len()
        );
        ws.send(Message::Text(req.to_string()))
            .await
            .map_err(|e| format!("failed to send #sep_inbox REQ: {e}"))?;
    }

    Ok(())
}

/// Get all filter JSON values from the database.
fn get_all_filters(db: &Database) -> Result<Vec<Value>, String> {
    // We use get_all_tags to find subscriptions, then for each tag, get subscriptions
    // Instead, let's use a simpler approach: scan for common tag values
    // Actually, we need to query all filters. Let's use a helper.
    let tags = db.get_all_tags()?;
    let mut filters = Vec::new();
    let mut seen_ids = HashSet::new();

    for tag in &tags {
        if let Ok(subs) = db.get_subscriptions_for_tag(tag) {
            for sub in subs {
                if seen_ids.insert(sub.subscription_id.clone()) {
                    if let Ok(filter) = serde_json::from_str::<Value>(&sub.filter_json) {
                        filters.push(filter);
                    }
                }
            }
        }
    }

    Ok(filters)
}

/// Handle a single Nostr relay message.
#[allow(clippy::too_many_arguments)]
async fn handle_message(
    text: &str,
    db: &Database,
    storage_key: &[u8; 32],
    config: &Config,
    apns_client: &mut Option<ApnsClient>,
    fcm_client: &mut Option<FcmClient>,
    up_client: &UnifiedPushClient,
    seen_events: &mut VecDeque<String>,
    seen_set: &mut HashSet<String>,
) {
    let msg: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return,
    };

    let arr = match msg.as_array() {
        Some(a) => a,
        None => return,
    };

    // We only care about EVENT messages: ["EVENT", "subscription_id", {event}]
    if arr.len() < 3 {
        return;
    }

    let msg_type = match arr[0].as_str() {
        Some(t) => t,
        None => return,
    };

    if msg_type != "EVENT" {
        return;
    }

    let event = &arr[2];

    let event_id = match event.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return,
    };

    // Deduplication
    if seen_set.contains(&event_id) {
        return;
    }
    seen_set.insert(event_id.clone());
    seen_events.push_back(event_id.clone());
    while seen_events.len() > 1000 {
        if let Some(old) = seen_events.pop_front() {
            seen_set.remove(&old);
        }
    }

    let content = match event.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return,
    };

    // Extract all tag values from the event
    let event_tags = extract_event_tags(event);
    if event_tags.is_empty() {
        return;
    }

    // For each tag value, find matching subscriptions and dispatch
    let mut dispatched_subs: HashSet<String> = HashSet::new();

    for tag_value in &event_tags {
        let subs = match db.get_subscriptions_for_tag(tag_value) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Watcher: failed to query subscriptions for tag {tag_value}: {e}");
                continue;
            }
        };

        for sub in subs {
            // Don't dispatch to the same subscription twice for the same event
            if !dispatched_subs.insert(sub.subscription_id.clone()) {
                continue;
            }

            // Rate limit check
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            if now - sub.last_pushed_at < config.rate_limit_seconds {
                continue;
            }

            // Decrypt notification key and token from storage
            let notif_key = match crypto::decrypt_from_storage(&sub.encrypted_notif_key, storage_key) {
                Ok(k) => k,
                Err(e) => {
                    eprintln!(
                        "Watcher: failed to decrypt notification key for {}: {e}",
                        sub.subscription_id
                    );
                    continue;
                }
            };

            let token_bytes = match crypto::decrypt_from_storage(&sub.encrypted_token, storage_key) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!(
                        "Watcher: failed to decrypt token for {}: {e}",
                        sub.subscription_id
                    );
                    continue;
                }
            };

            let token = match String::from_utf8(token_bytes) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!(
                        "Watcher: token is not valid UTF-8 for {}: {e}",
                        sub.subscription_id
                    );
                    continue;
                }
            };

            // Encrypt event content under the notification key
            let (encrypted, nonce, tag) =
                match crypto::encrypt_notification(content.as_bytes(), &notif_key, &event_id) {
                    Ok(result) => result,
                    Err(e) => {
                        eprintln!(
                            "Watcher: failed to encrypt notification for {}: {e}",
                            sub.subscription_id
                        );
                        continue;
                    }
                };

            // Dispatch based on platform
            let push_result = match sub.platform.as_str() {
                "ios" => {
                    if let Some(ref mut client) = apns_client {
                        let payload = apns::build_apns_payload(
                            &encrypted,
                            &nonce,
                            &tag,
                            &event_id,
                            &sub.subscription_id,
                        );
                        match client.send_push(&token, &payload).await {
                            Ok(apns::PushResult::Success) => DispatchResult::Success,
                            Ok(apns::PushResult::Gone) => DispatchResult::Gone,
                            Ok(apns::PushResult::Error(e)) => DispatchResult::Error(e),
                            Err(e) => DispatchResult::Error(e),
                        }
                    } else {
                        eprintln!("Watcher: APNs not configured, skipping iOS push");
                        continue;
                    }
                }
                "android-fcm" => {
                    if let Some(ref mut client) = fcm_client {
                        let data = fcm::build_fcm_data(
                            &encrypted,
                            &nonce,
                            &tag,
                            &event_id,
                            &sub.subscription_id,
                        );
                        match client.send_push(&token, &data).await {
                            Ok(fcm::PushResult::Success) => DispatchResult::Success,
                            Ok(fcm::PushResult::Unregistered) => DispatchResult::Gone,
                            Ok(fcm::PushResult::Error(e)) => DispatchResult::Error(e),
                            Err(e) => DispatchResult::Error(e),
                        }
                    } else {
                        eprintln!("Watcher: FCM not configured, skipping Android FCM push");
                        continue;
                    }
                }
                "android-up" => {
                    let payload = unified_push::build_up_payload(&encrypted, &nonce, &tag);
                    match up_client.send_push(&token, &payload).await {
                        Ok(unified_push::PushResult::Success) => DispatchResult::Success,
                        Ok(unified_push::PushResult::Gone) => DispatchResult::Gone,
                        Ok(unified_push::PushResult::Error(e)) => DispatchResult::Error(e),
                        Err(e) => DispatchResult::Error(e),
                    }
                }
                other => {
                    eprintln!("Watcher: unknown platform {other} for {}", sub.subscription_id);
                    continue;
                }
            };

            match push_result {
                DispatchResult::Success => {
                    if let Err(e) = db.update_last_pushed(&sub.subscription_id, now) {
                        eprintln!(
                            "Watcher: failed to update last_pushed for {}: {e}",
                            sub.subscription_id
                        );
                    }
                }
                DispatchResult::Gone => {
                    eprintln!(
                        "Watcher: token gone for {}, deleting subscription",
                        sub.subscription_id
                    );
                    if let Err(e) = db.delete_subscription(&sub.subscription_id) {
                        eprintln!(
                            "Watcher: failed to delete gone subscription {}: {e}",
                            sub.subscription_id
                        );
                    }
                }
                DispatchResult::Error(e) => {
                    eprintln!(
                        "Watcher: push failed for {}: {e}",
                        sub.subscription_id
                    );
                }
            }
        }
    }
}

enum DispatchResult {
    Success,
    Gone,
    Error(String),
}

/// Extract all tag values from a Nostr event's tags array.
/// Looks at "t" and "sep_inbox" tags.
fn extract_event_tags(event: &Value) -> Vec<String> {
    let mut values = Vec::new();

    if let Some(tags) = event.get("tags").and_then(|v| v.as_array()) {
        for tag in tags {
            if let Some(tag_arr) = tag.as_array() {
                if tag_arr.len() >= 2 {
                    if let Some(tag_name) = tag_arr[0].as_str() {
                        if tag_name == "t" || tag_name == "sep_inbox" {
                            if let Some(tag_value) = tag_arr[1].as_str() {
                                values.push(tag_value.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    values
}
