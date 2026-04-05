/// Result of a UnifiedPush dispatch attempt.
pub enum PushResult {
    /// Successfully delivered.
    Success,
    /// Endpoint is no longer valid; subscription should be deleted.
    Gone,
    /// An error occurred.
    Error(String),
}

/// UnifiedPush HTTP POST client.
pub struct UnifiedPushClient {
    client: reqwest::Client,
}

impl UnifiedPushClient {
    /// Create a new UnifiedPush client.
    pub fn new() -> Self {
        UnifiedPushClient {
            client: reqwest::Client::new(),
        }
    }

    /// Send a push notification via UnifiedPush.
    ///
    /// The payload is the raw encrypted bytes: nonce(12) || ciphertext || tag(16).
    /// It is POSTed to the endpoint URL with Content-Type: application/octet-stream.
    pub async fn send_push(
        &self,
        endpoint_url: &str,
        payload: &[u8],
    ) -> Result<PushResult, String> {
        let response = self
            .client
            .post(endpoint_url)
            .header("Content-Type", "application/octet-stream")
            .body(payload.to_vec())
            .send()
            .await
            .map_err(|e| format!("UnifiedPush request failed: {e}"))?;

        let status = response.status().as_u16();

        match status {
            200..=299 => Ok(PushResult::Success),
            404 | 410 => {
                eprintln!("UnifiedPush: endpoint gone ({status}) for {endpoint_url}");
                Ok(PushResult::Gone)
            }
            _ => {
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "unknown".to_string());
                eprintln!("UnifiedPush error {status}: {body}");
                Ok(PushResult::Error(format!(
                    "UnifiedPush status {status}: {body}"
                )))
            }
        }
    }
}

/// Build the UnifiedPush raw payload.
///
/// Format: nonce(12) || ciphertext || tag(16)
pub fn build_up_payload(encrypted: &[u8], nonce: &[u8], tag: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(nonce.len() + encrypted.len() + tag.len());
    payload.extend_from_slice(nonce);
    payload.extend_from_slice(encrypted);
    payload.extend_from_slice(tag);
    payload
}
