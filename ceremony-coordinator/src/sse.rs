//! Server-Sent Events broadcaster for live queue status.
//!
//! The coordinator publishes `StatusSnapshot` JSON on every meaningful state
//! change (signup, claim, commit, expire). Clients connect via
//! `GET /api/v1/status/stream` and receive one JSON line per change.

use tokio::sync::broadcast;

#[derive(Clone)]
pub struct StatusBroadcaster {
    tx: broadcast::Sender<String>,
}

impl StatusBroadcaster {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(64);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    pub fn publish(&self, json_payload: String) {
        let _ = self.tx.send(json_payload);
    }
}

impl Default for StatusBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}
