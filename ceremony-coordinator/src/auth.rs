//! NIP-98 HTTP Auth (https://github.com/nostr-protocol/nips/blob/master/98.md).
//!
//! Clients send `Authorization: Nostr <base64-encoded-kind-27235-event>`.
//! The event signature is verified as BIP-340 Schnorr over secp256k1.
//! The request URL, method, and body hash are bound via tags.

use std::str::FromStr;

use axum::extract::FromRequestParts;
use axum::http::{request::Parts, StatusCode};
use base64::Engine;
use k256::schnorr::{signature::hazmat::PrehashVerifier, Signature as SchnorrSig, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::now_unix;
use crate::SharedState;

const NIP98_KIND: u64 = 27235;
const MAX_AGE_SECS: i64 = 60;
const REPLAY_WINDOW_SECS: i64 = 600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrEvent {
    pub id: String,
    pub pubkey: String,
    pub created_at: i64,
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,
}

impl NostrEvent {
    pub fn tag(&self, name: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|t| t.first().map(|s| s.as_str()) == Some(name))
            .and_then(|t| t.get(1).map(|s| s.as_str()))
    }

    /// Recompute the event id per NIP-01.
    pub fn compute_id(&self) -> String {
        let serialized = serde_json::json!([
            0,
            self.pubkey,
            self.created_at,
            self.kind,
            self.tags,
            self.content,
        ])
        .to_string();
        let hash = Sha256::digest(serialized.as_bytes());
        hex::encode(hash)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("missing Authorization header")]
    Missing,
    #[error("malformed Authorization header")]
    Malformed,
    #[error("event signature invalid")]
    InvalidSignature,
    #[error("event id mismatch")]
    BadId,
    #[error("event too old or from the future")]
    StaleEvent,
    #[error("wrong kind (expected 27235)")]
    WrongKind,
    #[error("URL tag does not match request")]
    UrlMismatch,
    #[error("method tag does not match request")]
    MethodMismatch,
    #[error("payload hash mismatch")]
    PayloadMismatch,
    #[error("event replayed")]
    Replay,
    #[error("internal: {0}")]
    Internal(String),
}

impl AuthError {
    pub fn status(&self) -> StatusCode {
        match self {
            AuthError::Missing | AuthError::Malformed => StatusCode::UNAUTHORIZED,
            AuthError::InvalidSignature
            | AuthError::BadId
            | AuthError::StaleEvent
            | AuthError::WrongKind
            | AuthError::UrlMismatch
            | AuthError::MethodMismatch
            | AuthError::PayloadMismatch
            | AuthError::Replay => StatusCode::UNAUTHORIZED,
            AuthError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// Parsed NIP-98 auth result. Attach `pubkey` to downstream handlers.
#[derive(Debug, Clone)]
pub struct NostrAuth {
    pub pubkey: String,
    pub event_id: String,
}

/// Verify a NIP-98 event. `reconstructed_url` must be the absolute URL of
/// the incoming request, and `body_sha256_hex` should be the SHA-256 of the
/// body (lowercase hex), or `None` if the request has no body.
pub fn verify_nip98(
    state: &SharedState,
    header_value: &str,
    method: &str,
    reconstructed_url: &str,
    body_sha256_hex: Option<&str>,
) -> Result<NostrAuth, AuthError> {
    let rest = header_value.strip_prefix("Nostr ").ok_or(AuthError::Malformed)?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(rest.trim())
        .map_err(|_| AuthError::Malformed)?;
    let event: NostrEvent = serde_json::from_slice(&decoded).map_err(|_| AuthError::Malformed)?;

    if event.kind != NIP98_KIND {
        return Err(AuthError::WrongKind);
    }

    let now = now_unix();
    if (now - event.created_at).abs() > MAX_AGE_SECS {
        return Err(AuthError::StaleEvent);
    }

    if event.compute_id() != event.id {
        return Err(AuthError::BadId);
    }

    verify_schnorr(&event.pubkey, &event.id, &event.sig)?;

    match event.tag("u") {
        Some(u) if u == reconstructed_url => {}
        _ => return Err(AuthError::UrlMismatch),
    }

    match event.tag("method") {
        Some(m) if m.eq_ignore_ascii_case(method) => {}
        _ => return Err(AuthError::MethodMismatch),
    }

    if let Some(expected) = body_sha256_hex {
        match event.tag("payload") {
            Some(p) if p.eq_ignore_ascii_case(expected) => {}
            _ => return Err(AuthError::PayloadMismatch),
        }
    }

    let fresh = state
        .store
        .mark_nip98_seen(&event.id)
        .map_err(|e| AuthError::Internal(format!("replay store: {e}")))?;
    if !fresh {
        return Err(AuthError::Replay);
    }

    Ok(NostrAuth {
        pubkey: event.pubkey,
        event_id: event.id,
    })
}

fn verify_schnorr(pubkey_hex: &str, msg_hex: &str, sig_hex: &str) -> Result<(), AuthError> {
    let pk_bytes = hex::decode(pubkey_hex).map_err(|_| AuthError::InvalidSignature)?;
    let msg_bytes = hex::decode(msg_hex).map_err(|_| AuthError::InvalidSignature)?;
    let sig_bytes = hex::decode(sig_hex).map_err(|_| AuthError::InvalidSignature)?;

    if pk_bytes.len() != 32 || msg_bytes.len() != 32 || sig_bytes.len() != 64 {
        return Err(AuthError::InvalidSignature);
    }

    let vk =
        VerifyingKey::from_bytes(&pk_bytes).map_err(|_| AuthError::InvalidSignature)?;
    let sig = SchnorrSig::try_from(sig_bytes.as_slice()).map_err(|_| AuthError::InvalidSignature)?;
    vk.verify_prehash(&msg_bytes, &sig)
        .map_err(|_| AuthError::InvalidSignature)?;
    Ok(())
}

/// Compute the absolute URL for NIP-98 verification. Honours the
/// `X-Forwarded-Proto` / `X-Forwarded-Host` headers that nginx adds.
pub fn reconstruct_url(parts: &Parts) -> String {
    let scheme = parts
        .headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("https");
    let host = parts
        .headers
        .get("x-forwarded-host")
        .or_else(|| parts.headers.get("host"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("ceremony.onym.chat");
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    format!("{scheme}://{host}{path_and_query}")
}

pub fn body_sha256_hex(body: &[u8]) -> Option<String> {
    if body.is_empty() {
        None
    } else {
        Some(hex::encode(Sha256::digest(body)))
    }
}

/// Axum extractor. Only verifies the Nostr event envelope and signature; the
/// caller is responsible for supplying the body hash binding by checking
/// `NostrAuth` against a recomputed body digest after reading the body.
pub struct NostrAuthHeaderOnly(pub String);

#[async_trait::async_trait]
impl FromRequestParts<SharedState> for NostrAuthHeaderOnly {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or((StatusCode::UNAUTHORIZED, "missing Authorization".into()))?
            .to_string();
        Ok(NostrAuthHeaderOnly(header))
    }
}

/// Helper string parse to keep other modules terse.
pub fn parse_tier(tier: &str) -> Result<crate::model::Tier, String> {
    crate::model::Tier::from_str(tier)
}
