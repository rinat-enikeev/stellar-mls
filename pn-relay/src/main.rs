mod apns;
mod config;
mod crypto;
mod db;
mod fcm;
mod handler;
mod nostr_watcher;
mod unified_push;

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use tower_http::limit::RequestBodyLimitLayer;

use config::Config;
use handler::AppState;

#[tokio::main]
async fn main() {
    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Configuration error: {e}");
            eprintln!();
            eprintln!("Required environment variables: (none)");
            eprintln!();
            eprintln!("Optional:");
            eprintln!("  PN_BIND                    Listen address (default: 0.0.0.0:8090)");
            eprintln!("  PN_NOSTR_RELAY             Nostr relay WebSocket URL (default: ws://nostr-relay:7777)");
            eprintln!("  PN_DATA_DIR                Data directory (default: /app/data)");
            eprintln!("  APNS_KEY_PATH              APNs .p8 key file path");
            eprintln!("  APNS_KEY_ID                APNs key ID");
            eprintln!("  APNS_TEAM_ID               Apple team ID");
            eprintln!("  APNS_BUNDLE_ID             APNs bundle ID (default: com.stellarmls.chat)");
            eprintln!("  APNS_ENVIRONMENT           APNs environment (default: production)");
            eprintln!("  FCM_SERVICE_ACCOUNT_PATH   FCM service account JSON path");
            eprintln!("  FCM_PROJECT_ID             Firebase project ID");
            eprintln!("  PN_RATE_LIMIT_SECONDS      Min seconds between pushes per subscription (default: 5)");
            std::process::exit(1);
        }
    };

    let data_dir = Path::new(&config.data_dir);

    // Load or create X25519 keypair
    let (x25519_private, x25519_public) = match crypto::load_or_create_keypair(data_dir) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("Failed to load/create X25519 keypair: {e}");
            std::process::exit(1);
        }
    };

    // Load or create storage key
    let storage_key = match crypto::load_or_create_storage_key(data_dir) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("Failed to load/create storage key: {e}");
            std::process::exit(1);
        }
    };

    // Initialize database
    let database = match db::Database::new(data_dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Failed to initialize database: {e}");
            std::process::exit(1);
        }
    };

    // Create watch channel for signaling the watcher to refresh subscriptions
    let (watcher_tx, watcher_rx) = tokio::sync::watch::channel(());

    let bind_address: SocketAddr = config
        .bind_address
        .parse()
        .expect("PN_BIND must be a valid socket address");

    eprintln!("PN Relay configuration:");
    eprintln!("  Bind:          {}", config.bind_address);
    eprintln!("  Nostr relay:   {}", config.nostr_relay_url);
    eprintln!("  Data dir:      {}", config.data_dir);
    eprintln!("  APNs:          {}", if config.apns_configured() { "configured" } else { "not configured" });
    eprintln!("  FCM:           {}", if config.fcm_configured() { "configured" } else { "not configured" });
    eprintln!("  UnifiedPush:   always available");
    eprintln!("  Rate limit:    {}s per subscription", config.rate_limit_seconds);

    // Create a second database handle for the watcher (SQLite with WAL supports concurrent reads)
    let watcher_db = match db::Database::new(data_dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Failed to create watcher database handle: {e}");
            std::process::exit(1);
        }
    };

    let watcher_storage_key = storage_key;
    let watcher_config = config.clone();
    let nostr_relay_url = config.nostr_relay_url.clone();

    // Spawn Nostr watcher background task
    tokio::spawn(async move {
        nostr_watcher::run_watcher(
            nostr_relay_url,
            watcher_db,
            watcher_storage_key,
            watcher_config,
            watcher_rx,
        )
        .await;
    });

    // Spawn daily cleanup task (delete stale subscriptions older than 90 days)
    let cleanup_db = match db::Database::new(data_dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Failed to create cleanup database handle: {e}");
            std::process::exit(1);
        }
    };
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(86400));
        loop {
            interval.tick().await;
            match cleanup_db.delete_stale_subscriptions(90) {
                Ok(count) => {
                    if count > 0 {
                        eprintln!("Cleanup: deleted {count} stale subscriptions");
                    }
                }
                Err(e) => {
                    eprintln!("Cleanup: failed to delete stale subscriptions: {e}");
                }
            }
        }
    });

    let state = Arc::new(AppState {
        config,
        db: database,
        x25519_private,
        x25519_public,
        storage_key,
        watcher_tx,
    });

    let app = Router::new()
        .route("/v1/info", get(handler::handle_info))
        .route("/v1/subscription", post(handler::handle_subscription))
        .route("/v1/health", get(handler::handle_health))
        .layer(RequestBodyLimitLayer::new(65536)) // 64KB max body
        .with_state(state);

    eprintln!("Listening on {bind_address}");

    let listener = tokio::net::TcpListener::bind(bind_address)
        .await
        .expect("failed to bind");
    axum::serve(listener, app).await.expect("server error");
}
