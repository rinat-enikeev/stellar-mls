mod auth;
mod blossom;
mod ceremony_exec;
mod config;
mod handlers;
mod model;
mod nostr;
mod queue;
mod reaper;
mod seed;
mod sse;
mod store;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::config::Config;
use crate::sse::StatusBroadcaster;
use crate::store::Store;

pub struct AppState {
    pub config: Config,
    pub store: Store,
    pub blossom: blossom::BlossomClient,
    pub nostr: nostr::NostrPublisher,
    pub ceremony_tool: ceremony_exec::CeremonyTool,
    pub queues: queue::QueueSet,
    pub status_tx: StatusBroadcaster,
}

pub type SharedState = Arc<AppState>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ceremony_coordinator=info,tower_http=info".into()),
        )
        .init();

    let mut args = std::env::args().skip(1);
    if let Some(sub) = args.next() {
        if sub == "seed" {
            let tier = args.next().ok_or_else(|| {
                anyhow::anyhow!("usage: ceremony-coordinator seed <small|medium|large>")
            })?;
            return seed::run(&tier).await;
        }
        anyhow::bail!("unknown subcommand: {sub}");
    }

    let config = Config::from_env()?;
    info!(bind = %config.bind, db = %config.db_path.display(), "starting coordinator");

    let store = Store::open(&config.db_path)?;
    store.run_migrations()?;

    let nostr = nostr::NostrPublisher::new(
        config.nostr_relay.clone(),
        config.coordinator_nsec.clone(),
    );
    let blossom = blossom::BlossomClient::new(
        config.blossom_url.clone(),
        config.blossom_public_url.clone(),
    )
    .with_signing_key(nostr.signing_key());
    let ceremony_tool = ceremony_exec::CeremonyTool::new(
        config.ceremony_tool_bin.clone(),
        config.work_dir.clone(),
    );
    let queues = queue::QueueSet::new();
    let status_tx = sse::StatusBroadcaster::new();

    let state: SharedState = Arc::new(AppState {
        config: config.clone(),
        store,
        blossom,
        nostr,
        ceremony_tool,
        queues,
        status_tx,
    });

    reaper::spawn(state.clone());

    let app = Router::new()
        .route("/api/v1/healthz", get(healthz))
        .route("/api/v1/status", get(handlers::transcript::status))
        .route("/api/v1/status/stream", get(handlers::transcript::status_stream))
        .route("/api/v1/tiers/:tier/queue", get(handlers::transcript::queue))
        .route("/api/v1/tiers/:tier/rounds", get(handlers::transcript::rounds))
        .route(
            "/api/v1/tiers/:tier/rounds/:round",
            get(handlers::transcript::round),
        )
        .route(
            "/api/v1/tiers/:tier/rounds/:round/artifacts/:name",
            get(handlers::transcript::round_artifact),
        )
        .route(
            "/api/v1/tiers/:tier/rounds/:round/prev.zip",
            get(handlers::transcript::round_prev_zip),
        )
        .route("/api/v1/signup", post(handlers::signup::signup))
        .route("/api/v1/tiers/:tier/claim", post(handlers::slot::claim))
        .route(
            "/api/v1/tiers/:tier/contribute",
            post(handlers::upload::contribute),
        )
        .route("/api/v1/verify/state", post(handlers::verify::verify_state))
        .route("/api/v1/phase2/summary", get(handlers::phase2::summary))
        .route(
            "/api/v1/phase2/rounds",
            get(handlers::phase2::rounds).post(handlers::phase2::publish_round),
        )
        .route(
            "/api/v1/phase2/rounds/upload",
            post(handlers::phase2::upload_round),
        )
        .route("/api/v1/phase2/freeze", post(handlers::phase2::freeze))
        .route("/api/v1/phase2/beacon", post(handlers::phase2::set_beacon))
        .route(
            "/api/v1/tiers/:tier/phase2/rounds/:round/artifacts/:name",
            get(handlers::transcript::phase2_round_artifact),
        )
        .route("/api/v1/downloads", get(handlers::transcript::downloads))
        .route(
            "/api/v1/participants/:pubkey",
            get(handlers::transcript::participant),
        )
        .layer(RequestBodyLimitLayer::new(210 * 1024 * 1024))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = config.bind.parse()?;
    let listener = TcpListener::bind(addr).await?;
    info!(%addr, "listening");
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}
