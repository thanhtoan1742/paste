use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use crate::config::Config;

pub struct PasteEntry {
    pub content: String,
    pub expires_at: Instant,
}

pub struct AppState {
    pub pastes: RwLock<HashMap<String, PasteEntry>>,
    pub config: Config,
}

pub fn new_app_state(config: Config) -> Arc<AppState> {
    Arc::new(AppState {
        pastes: RwLock::new(HashMap::new()),
        config,
    })
}
