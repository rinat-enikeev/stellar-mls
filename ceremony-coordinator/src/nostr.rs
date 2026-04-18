//! Coordinator-side Nostr publisher.
//!
//! Publishes transcript events (kind 30078 addressable) to the configured
//! strfry relay. Runs as a background tokio task; callers enqueue events
//! through [`NostrPublisher::publish`].

use anyhow::{Context, Result};
use k256::schnorr::{signature::hazmat::PrehashSigner, Signature as SchnorrSig, SigningKey};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::model::now_unix;

#[derive(Debug, Clone, Serialize)]
pub struct UnsignedEvent {
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignedEvent {
    pub id: String,
    pub pubkey: String,
    pub created_at: i64,
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,
}

#[derive(Clone)]
pub struct NostrPublisher {
    tx: Option<mpsc::UnboundedSender<SignedEvent>>,
    coordinator_pk: Option<String>,
    signing_key: Option<SigningKey>,
}

impl NostrPublisher {
    pub fn new(relay_url: String, nsec_hex: Option<String>) -> Self {
        let signing_key = nsec_hex.as_deref().and_then(|h| parse_signing_key(h).ok());
        let coordinator_pk = signing_key.as_ref().map(|sk| {
            let vk = sk.verifying_key();
            let pk_bytes = vk.to_bytes();
            hex::encode(pk_bytes)
        });

        let tx = if signing_key.is_some() {
            let (tx, rx) = mpsc::unbounded_channel::<SignedEvent>();
            tokio::spawn(relay_loop(relay_url, rx));
            Some(tx)
        } else {
            warn!("CEREMONY_COORDINATOR_NSEC unset — Nostr publishing disabled");
            None
        };

        Self {
            tx,
            coordinator_pk,
            signing_key,
        }
    }

    pub fn coordinator_pubkey(&self) -> Option<&str> {
        self.coordinator_pk.as_deref()
    }

    /// Returns a clone of the signing key, if one is configured.
    pub fn signing_key(&self) -> Option<SigningKey> {
        self.signing_key.clone()
    }

    pub fn publish(&self, event: UnsignedEvent) -> Result<Option<String>> {
        let Some(key) = &self.signing_key else {
            return Ok(None);
        };
        let signed = sign(key, event)?;
        let id = signed.id.clone();
        if let Some(tx) = &self.tx {
            let _ = tx.send(signed);
        }
        Ok(Some(id))
    }
}

pub fn parse_signing_key(nsec_hex: &str) -> Result<SigningKey> {
    let bytes = hex::decode(nsec_hex).context("decode nsec hex")?;
    let arr: [u8; 32] = bytes.try_into().map_err(|_| anyhow::anyhow!("nsec must be 32 bytes"))?;
    SigningKey::from_bytes(&arr).context("parse signing key")
}

pub fn pubkey_hex(key: &SigningKey) -> String {
    hex::encode(key.verifying_key().to_bytes())
}

/// Build a signed Nostr event (without sending it to the relay).
pub fn sign(key: &SigningKey, event: UnsignedEvent) -> Result<SignedEvent> {
    let pubkey = pubkey_hex(key);
    let created_at = now_unix();
    let id = compute_event_id(&pubkey, created_at, &event);
    let sig = sign_event_id(key, &id)?;
    Ok(SignedEvent {
        id,
        pubkey,
        created_at,
        kind: event.kind,
        tags: event.tags,
        content: event.content,
        sig,
    })
}

fn compute_event_id(pubkey: &str, created_at: i64, event: &UnsignedEvent) -> String {
    let serialized = serde_json::json!([
        0,
        pubkey,
        created_at,
        event.kind,
        event.tags,
        event.content,
    ])
    .to_string();
    hex::encode(Sha256::digest(serialized.as_bytes()))
}

fn sign_event_id(key: &SigningKey, id_hex: &str) -> Result<String> {
    let msg = hex::decode(id_hex).context("decode id")?;
    let sig: SchnorrSig = key.sign_prehash(&msg).context("schnorr sign")?;
    Ok(hex::encode(sig.to_bytes()))
}

async fn relay_loop(relay_url: String, mut rx: mpsc::UnboundedReceiver<SignedEvent>) {
    use async_tungstenite::tokio::connect_async;
    use async_tungstenite::tungstenite::protocol::Message;
    use futures::{SinkExt, StreamExt};

    loop {
        match connect_async(&relay_url).await {
            Ok((mut ws, _resp)) => {
                info!(relay = %relay_url, "nostr connected");
                while let Some(event) = rx.recv().await {
                    let payload: Value = serde_json::json!(["EVENT", event]);
                    if let Err(e) = ws.send(Message::Text(payload.to_string())).await {
                        warn!(error = %e, "nostr send failed, reconnecting");
                        break;
                    }
                    // Drain OK responses non-blockingly.
                    if let Some(Ok(_msg)) = ws.next().await {
                        // Intentionally ignored; we only care that the send returned.
                    }
                }
                let _ = ws.close(None).await;
                return;
            }
            Err(e) => {
                warn!(error = %e, relay = %relay_url, "nostr connect failed, retrying in 5s");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}
