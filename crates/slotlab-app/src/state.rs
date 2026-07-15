use crate::books::BooksManager;
use crate::registry::Registry;
use crate::rgs::session::Sessions;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<Registry>,
    /// Startup load errors, by game name (displayed in the UI).
    pub load_errors: Arc<HashMap<String, String>>,
    pub sessions: Arc<Sessions>,
    pub books: Arc<BooksManager>,
    /// Live feed: each spin published as JSON to the UI (SSE).
    pub events_tx: broadcast::Sender<String>,
    pub share: crate::share::ShareManager,
}
