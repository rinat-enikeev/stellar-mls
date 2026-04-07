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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_endpoint_url_https_accepted() {
        assert!(validate_endpoint_url("https://push.example.com/up/abc123").is_ok());
    }

    #[test]
    fn test_validate_endpoint_url_http_accepted() {
        assert!(validate_endpoint_url("http://push.example.com/up/abc123").is_ok());
    }

    #[test]
    fn test_validate_endpoint_url_ftp_rejected() {
        let result = validate_endpoint_url("ftp://push.example.com/file");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("http"));
    }

    #[test]
    fn test_validate_endpoint_url_localhost_rejected() {
        let result = validate_endpoint_url("https://localhost/up");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("localhost"));
    }

    #[test]
    fn test_validate_endpoint_url_127_0_0_1_rejected() {
        let result = validate_endpoint_url("https://127.0.0.1/up");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("localhost"));
    }

    #[test]
    fn test_validate_endpoint_url_ipv6_loopback_rejected() {
        let result = validate_endpoint_url("https://[::1]/up");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_endpoint_url_private_10_rejected() {
        let result = validate_endpoint_url("https://10.0.0.1/up");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("private"));
    }

    #[test]
    fn test_validate_endpoint_url_private_172_16_rejected() {
        let result = validate_endpoint_url("https://172.16.0.1/up");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("private"));
    }

    #[test]
    fn test_validate_endpoint_url_private_192_168_rejected() {
        let result = validate_endpoint_url("https://192.168.1.1/up");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("private"));
    }

    #[test]
    fn test_validate_endpoint_url_link_local_rejected() {
        let result = validate_endpoint_url("https://169.254.1.1/up");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("private"));
    }

    #[test]
    fn test_validate_endpoint_url_cloud_metadata_rejected() {
        // 169.254.169.254 is the IMDS endpoint — it's link-local, so blocked
        let result = validate_endpoint_url("https://169.254.169.254/latest/meta-data/");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_endpoint_url_azure_imds_rejected() {
        let result = validate_endpoint_url("https://168.63.129.16/metadata");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("private"));
    }

    #[test]
    fn test_validate_endpoint_url_cgnat_rejected() {
        let result = validate_endpoint_url("https://100.64.0.1/up");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("private"));
    }

    #[test]
    fn test_validate_endpoint_url_valid_public_ip() {
        assert!(validate_endpoint_url("https://8.8.8.8/up").is_ok());
        assert!(validate_endpoint_url("https://1.1.1.1/up").is_ok());
        assert!(validate_endpoint_url("https://203.0.113.1/up").is_ok());
    }
}
