//! Minimal Blossom client.
//!
//! Blossom stores blobs by SHA-256. We only need two operations: PUT a blob
//! (idempotent by hash) and construct a public URL for GET redirects.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct BlossomClient {
    http: reqwest::Client,
    internal_url: String,
    public_url: String,
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
        }
    }

    pub fn public_blob_url(&self, sha256_hex: &str) -> String {
        format!("{}/{}", self.public_url, sha256_hex)
    }

    pub async fn put(&self, data: Vec<u8>, content_type: &str) -> Result<PutResult> {
        let sha256_hex = hex::encode(Sha256::digest(&data));
        let size = data.len() as u64;
        let url = format!("{}/upload", self.internal_url);
        self.http
            .put(&url)
            .header("Content-Type", content_type)
            .body(data)
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

