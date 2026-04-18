//! Minimal Blossom client.
//!
//! Blossom stores blobs by SHA-256. We only need two operations: PUT a blob
//! (idempotent by hash) and construct a public URL for GET redirects. The
//! hzrd149 blossom-server enforces BUD-02 auth on `/upload`, so if a
//! signing key is provided we attach a signed kind-24242 event as the
//! `Authorization: Nostr <b64>` header.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use k256::schnorr::SigningKey;
use sha2::{Digest, Sha256};

use crate::model::now_unix;
use crate::nostr::{sign, UnsignedEvent};

#[derive(Clone)]
pub struct BlossomClient {
    http: reqwest::Client,
    internal_url: String,
    public_url: String,
    signing_key: Option<SigningKey>,
}

pub struct PutResult {
    pub sha256_hex: String,
    pub public_url: String,
    pub size: u64,
}

impl BlossomClient {
    pub fn new(internal_url: String, public_url: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            internal_url: internal_url.trim_end_matches('/').to_string(),
            public_url: public_url.trim_end_matches('/').to_string(),
            signing_key: None,
        }
    }

    pub fn with_signing_key(mut self, key: Option<SigningKey>) -> Self {
        self.signing_key = key;
        self
    }

    pub fn public_blob_url(&self, sha256_hex: &str) -> String {
        format!("{}/{}", self.public_url, sha256_hex)
    }

    pub async fn put(&self, data: Vec<u8>, content_type: &str) -> Result<PutResult> {
        let sha256_hex = hex::encode(Sha256::digest(&data));
        let size = data.len() as u64;
        let url = format!("{}/upload", self.internal_url);

        let mut req = self
            .http
            .put(&url)
            .header("Content-Type", content_type);

        if let Some(key) = &self.signing_key {
            let header = build_upload_auth(key, &sha256_hex)?;
            req = req.header("Authorization", header);
        }

        req.body(data)
            .send()
            .await
            .context("blossom PUT")?
            .error_for_status()
            .context("blossom PUT status")?;
        let public_url = self.public_blob_url(&sha256_hex);
        Ok(PutResult {
            sha256_hex,
            public_url,
            size,
        })
    }

    pub async fn get(&self, sha256_hex: &str) -> Result<Vec<u8>> {
        let url = format!("{}/{}", self.internal_url, sha256_hex);
        let bytes = self
            .http
            .get(&url)
            .send()
            .await
            .context("blossom GET")?
            .error_for_status()
            .context("blossom GET status")?
            .bytes()
            .await
            .context("blossom GET body")?;
        Ok(bytes.to_vec())
    }
}

/// BUD-02 upload authorization event — kind 24242, one-minute expiry.
fn build_upload_auth(key: &SigningKey, sha256_hex: &str) -> Result<String> {
    let now = now_unix();
    let exp = now + 60;
    let tags = vec![
        vec!["t".into(), "upload".into()],
        vec!["x".into(), sha256_hex.into()],
        vec!["expiration".into(), exp.to_string()],
    ];
    let signed = sign(
        key,
        UnsignedEvent {
            kind: 24242,
            tags,
            content: "Upload blob".to_string(),
        },
    )?;
    let json = serde_json::to_string(&signed).context("serialize blossom auth event")?;
    Ok(format!("Nostr {}", B64.encode(json.as_bytes())))
}
