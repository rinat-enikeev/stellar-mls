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

/// Validate that an endpoint URL is safe to POST to.
///
/// Rejects non-HTTP(S) schemes, localhost, private/link-local IP ranges,
/// and cloud metadata service endpoints to prevent Server-Side Request Forgery (SSRF).
pub fn validate_endpoint_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| format!("invalid endpoint URL: {e}"))?;

    // Only allow HTTP and HTTPS (UnifiedPush distributors may use plain HTTP)
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err(format!(
            "endpoint URL must use http or https scheme, got '{}'",
            parsed.scheme()
        ));
    }

    let host = parsed
        .host_str()
        .ok_or("endpoint URL has no host")?;

    // Reject localhost and loopback
    if host == "localhost"
        || host == "127.0.0.1"
        || host == "::1"
        || host == "[::1]"
        || host == "0.0.0.0"
    {
        return Err("endpoint URL must not target localhost".to_string());
    }

    // Reject cloud metadata service hostnames
    if host == "metadata.google.internal" {
        return Err("endpoint URL must not target cloud metadata services".to_string());
    }

    // Reject private, link-local, and metadata IP ranges
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let is_blocked = match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback()             // 127.0.0.0/8
                || v4.is_private()           // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local()        // 169.254.0.0/16 (includes 169.254.169.254 IMDS)
                || v4.is_broadcast()         // 255.255.255.255
                || v4.is_unspecified()       // 0.0.0.0
                || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64  // 100.64.0.0/10 (CGNAT)
                || v4.octets()[0] == 168 && v4.octets()[1] == 63
                    && v4.octets()[2] == 129 && v4.octets()[3] == 16  // 168.63.129.16 (Azure IMDS)
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()
                || v6.is_unspecified()
            }
        };
        if is_blocked {
            return Err(format!(
                "endpoint URL must not target private/link-local/metadata addresses: {host}"
            ));
        }
    }

    Ok(())
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
    ///
    /// The endpoint URL is validated to prevent SSRF: private/link-local/metadata
    /// IP ranges and non-HTTP(S) schemes are rejected.
    pub async fn send_push(
        &self,
        endpoint_url: &str,
        payload: &[u8],
    ) -> Result<PushResult, String> {
        validate_endpoint_url(endpoint_url)?;

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
