//! Per-tier queue serialisation.
//!
//! The slot state machine is stored in SQLite, but *mutations* — claim,
//! commit, expire — funnel through one async mutex per tier so we never
//! double-assign a slot during races.

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::model::Tier;

#[derive(Clone)]
pub struct QueueSet {
    small: Arc<Mutex<()>>,
    medium: Arc<Mutex<()>>,
    large: Arc<Mutex<()>>,
}

impl QueueSet {
    pub fn new() -> Self {
        Self {
            small: Arc::new(Mutex::new(())),
            medium: Arc::new(Mutex::new(())),
            large: Arc::new(Mutex::new(())),
        }
    }

    pub fn lock_for(&self, tier: Tier) -> Arc<Mutex<()>> {
        match tier {
            Tier::Small => self.small.clone(),
            Tier::Medium => self.medium.clone(),
            Tier::Large => self.large.clone(),
        }
    }
}

impl Default for QueueSet {
    fn default() -> Self {
        Self::new()
    }
}
