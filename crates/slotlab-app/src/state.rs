use crate::books::BooksManager;
use crate::registry::Registry;
use crate::rgs::faults::FaultManager;
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
    /// Deliberate RGS misbehaviour, per game: the states a reviewer reaches
    /// by accident and a developer cannot reach at all.
    pub faults: Arc<FaultManager>,
    pub books: Arc<BooksManager>,
    /// Live feed: each spin published as JSON to the UI (SSE).
    pub events_tx: broadcast::Sender<String>,
    pub share: crate::share::ShareManager,
    /// Games published by the multi-game server (empty in dev mode).
    pub hub: Arc<crate::hub::Hub>,
    /// Book scans: staging invariants weighted by the LUT.
    pub scans: Arc<crate::scan::ScanManager>,
    /// One-click publishing to the hub (dev machine only).
    pub deploy: Arc<crate::deploy::DeployManager>,
}
