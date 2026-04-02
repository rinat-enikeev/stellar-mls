mod config;
mod handler;
mod validation;

use std::net::SocketAddr;
use std::process::Command;
use std::sync::Arc;

use axum::routing::post;
use axum::Router;
use tower_http::limit::RequestBodyLimitLayer;

use config::Config;
use handler::AppState;
use validation::RateLimiter;

#[tokio::main]
async fn main() {
    let mut config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Configuration error: {e}");
            eprintln!();
            eprintln!("Required environment variables:");
            eprintln!("  RELAYER_SECRET_KEY     Stellar secret key (S...) for signing transactions");
            eprintln!("  RELAYER_CONTRACT_ID    Whitelisted contract ID (C...)");
            eprintln!();
            eprintln!("Optional:");
            eprintln!("  RELAYER_RPC_URL        Soroban RPC (default: https://soroban.stellar.org)");
            eprintln!("  RELAYER_NETWORK_PASSPHRASE  Network passphrase for the RPC endpoint");
            eprintln!("  RELAYER_NETWORK        Network name (default: mainnet)");
            eprintln!("  RELAYER_BIND           Listen address (default: 0.0.0.0:8080)");
            eprintln!("  RELAYER_AUTH_TOKENS    Comma-separated bearer tokens (default: none)");
            eprintln!("  RELAYER_RATE_LIMIT     Requests/minute per IP (default: 30)");
            eprintln!("  RELAYER_MAX_PAYLOAD_SIZE  Max body size in bytes (default: 8192)");
            std::process::exit(1);
        }
    };

    // Add the secret key into stellar CLI as a named identity.
    // Current CLI versions use `keys add --secret-key` instead of `keys import`.
    eprintln!("Adding relayer identity to stellar CLI...");
    let add_output = Command::new("stellar")
        .arg("keys")
        .arg("add")
        .arg(&config.identity_name)
        .arg("--secret-key")
        .arg("--overwrite")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(config.secret_key.as_bytes())?;
                stdin.write_all(b"\n")?;
            }
            child.wait_with_output()
        });

    match add_output {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            // "already exists" is fine — we asked for overwrite
            if !stderr.contains("already exists") {
                eprintln!("Warning: stellar keys add: {stderr}");
            }
        }
        Err(e) => {
            eprintln!("Failed to run 'stellar keys add --secret-key': {e}");
            eprintln!("Make sure the 'stellar' CLI is installed and in PATH.");
            std::process::exit(1);
        }
    }

    // Resolve the public address from the identity
    let pubkey_output = Command::new("stellar")
        .arg("keys")
        .arg("public-key")
        .arg(&config.identity_name)
        .output();

    match pubkey_output {
        Ok(o) if o.status.success() => {
            config.public_address = String::from_utf8_lossy(&o.stdout).trim().to_string();
        }
        _ => {
            eprintln!("Failed to resolve public key for relayer identity.");
            eprintln!("Check that RELAYER_SECRET_KEY is a valid Stellar secret key.");
            std::process::exit(1);
        }
    }

    eprintln!("Relayer address: {}", config.public_address);
    eprintln!("Contract ID:    {}", config.contract_id);
    eprintln!("Network:        {}", config.network);
    eprintln!("Auth required:  {}", config.auth_required());
    eprintln!("Rate limit:     {} req/min per IP", config.rate_limit_per_minute);

    let bind_address: SocketAddr = config
        .bind_address
        .parse()
        .expect("RELAYER_BIND must be a valid socket address");

    let max_payload = config.max_payload_size;
    let state = Arc::new(AppState {
        rate_limiter: RateLimiter::new(config.rate_limit_per_minute),
        config,
    });

    let app = Router::new()
        .route("/", post(handler::handle_invoke))
        .layer(RequestBodyLimitLayer::new(max_payload))
        .with_state(state)
        .into_make_service_with_connect_info::<SocketAddr>();

    eprintln!("Listening on {bind_address}");

    let listener = tokio::net::TcpListener::bind(bind_address)
        .await
        .expect("failed to bind");
    axum::serve(listener, app).await.expect("server error");
}
