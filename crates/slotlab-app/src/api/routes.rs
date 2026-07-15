use crate::persist::GameEntry;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use slotlab_core::{checks, review, stats};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/ui/health", get(health))
        .route("/api/ui/games", get(list_games).post(register_game))
        .route("/api/ui/games/{name}", delete(remove_game).patch(update_game))
        .route("/api/ui/games/{name}/reload", post(reload_game))
        .route("/api/ui/games/{name}/stats", get(game_stats))
        .route("/api/ui/games/{name}/bonus-stats", get(bonus_stats))
        .route("/api/ui/games/{name}/checks", get(game_checks))
        .route("/api/ui/games/{name}/review-ids", get(game_review_ids))
        .route(
            "/api/ui/games/{name}/import",
            post(start_import).get(import_status),
        )
        .route("/api/ui/sessions/prepare", post(prepare_session))
        .route("/api/ui/force", post(set_force))
        .route("/api/ui/sessions/reset", post(reset_sessions))
        .route("/api/ui/tls/status", get(tls_status))
        .route("/api/ui/tls/install", post(tls_install))
        .route("/api/ui/events", get(sse_events))
        .route("/api/ui/crowdsim", post(run_crowdsim))
        .route("/api/ui/convergence", post(run_convergence))
        .route("/api/ui/bookmarks", get(list_bookmarks).post(add_bookmark))
        .route("/api/ui/bookmarks/{id}", delete(remove_bookmark))
}

/// Identity + readiness — probed by the desktop app (splash → UI) and to
/// recognize a SlotLab instance already running on the port.
async fn health() -> Json<serde_json::Value> {
    Json(json!({ "app": "slotlab", "version": env!("CARGO_PKG_VERSION") }))
}

/// Uniform API error: { "error": "..." } + status.
pub struct ApiError(StatusCode, String);

impl ApiError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self(StatusCode::NOT_FOUND, msg.into())
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, msg.into())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self(StatusCode::UNPROCESSABLE_ENTITY, format!("{e:#}"))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameSummary {
    name: String,
    publish_dir: String,
    front_url: String,
    /// Auto-detected build, served by SlotLab under /games/<name>/.
    #[serde(skip_serializing_if = "Option::is_none")]
    build_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    served_url: Option<String>,
    loaded: bool,
    imported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    modes: Vec<ModeSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModeSummary {
    name: String,
    cost: f64,
    aliases: Vec<String>,
    rtp: f64,
    hit_rate: f64,
    max_payout: f64,
    outcome_count: usize,
}

async fn list_games(State(state): State<AppState>) -> Json<Vec<GameSummary>> {
    let mut out = Vec::new();
    for entry in state.registry.list_entries() {
        match state.registry.get(&entry.name) {
            Some(game) => {
                let modes = game
                    .modes
                    .iter()
                    .map(|m| {
                        let s = stats::analyze(&m.lut, m.config.cost);
                        ModeSummary {
                            name: m.config.name.clone(),
                            cost: m.config.cost,
                            aliases: m.config.aliases.clone(),
                            rtp: s.rtp,
                            hit_rate: s.hit_rate,
                            max_payout: s.max_payout,
                            outcome_count: s.outcome_count,
                        }
                    })
                    .collect();
                out.push(GameSummary {
                    name: entry.name.clone(),
                    publish_dir: entry.publish_dir.display().to_string(),
                    front_url: entry.front_url,
                    build_dir: game
                        .entry
                        .build_dir
                        .as_ref()
                        .map(|p| p.display().to_string()),
                    served_url: game
                        .entry
                        .build_dir
                        .as_ref()
                        .map(|_| format!("/games/{}", entry.name)),
                    loaded: true,
                    imported: state.books.is_imported(&game),
                    error: None,
                    modes,
                });
            }
            None => out.push(GameSummary {
                name: entry.name.clone(),
                publish_dir: entry.publish_dir.display().to_string(),
                front_url: entry.front_url,
                build_dir: None,
                served_url: None,
                loaded: false,
                imported: false,
                error: state.load_errors.get(&entry.name).cloned(),
                modes: Vec::new(),
            }),
        }
    }
    Json(out)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterGame {
    name: String,
    publish_dir: String,
    #[serde(default)]
    front_url: String,
}

async fn register_game(
    State(state): State<AppState>,
    Json(body): Json<RegisterGame>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let entry = GameEntry {
        name: body.name.trim().to_string(),
        publish_dir: std::path::PathBuf::from(body.publish_dir.trim()),
        front_url: body.front_url.trim().to_string(),
        build_dir: None, // auto-detected by register()
    };
    let game = state.registry.register(entry)?;
    // Books import started immediately in the background.
    if !state.books.is_imported(&game) {
        state.books.start_import(game.clone());
    }
    Ok(Json(json!({
        "ok": true,
        "modes": game.modes.iter().map(|m| m.config.name.clone()).collect::<Vec<_>>(),
        "servedUrl": game.entry.build_dir.as_ref().map(|_| format!("/games/{}", game.entry.name)),
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateGame {
    front_url: Option<String>,
}

async fn update_game(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<UpdateGame>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Some(url) = body.front_url {
        state.registry.set_front_url(&name, url.trim())?;
    }
    Ok(Json(json!({ "ok": true })))
}

async fn remove_game(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let removed = state.registry.remove(&name)?;
    if !removed {
        return Err(ApiError::not_found(format!("unknown game: {name}")));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn reload_game(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state.registry.reload(&name)?;
    state.books.invalidate_game(&name);
    // Math changed → automatic re-import.
    if !state.books.is_imported(&game) {
        state.books.start_import(game);
    }
    Ok(Json(json!({ "ok": true })))
}

async fn game_stats(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let modes: Vec<serde_json::Value> = game
        .modes
        .iter()
        .map(|m| {
            let s = stats::analyze(&m.lut, m.config.cost);
            json!({ "mode": m.config.name, "stats": s })
        })
        .collect();
    Ok(Json(json!({ "game": name, "modes": modes })))
}

/// Bonus (freespin) trigger stats per mode, weighted by the LUT: the real
/// per-spin probabilities, unlike the raw book counts shown in Search.
/// Joins the books .idx sidecar (fs_triggers, criteria) with the LUT weights.
async fn bonus_stats(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use std::collections::{BTreeMap, HashMap};

    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let mut modes = Vec::new();
    for m in &game.modes {
        let Ok(books) = state.books.mode_books(&game, &m.config.name) else {
            modes.push(json!({ "mode": m.config.name, "imported": false }));
            continue;
        };
        let idx = &books.index;
        let lut = &m.lut;
        let t = lut.total_weight as f64;
        let cost = if m.config.cost > 0.0 { m.config.cost } else { 1.0 };
        let weight_by_sim: HashMap<u64, u64> =
            lut.rows.iter().map(|r| (r.sim_id, r.weight)).collect();

        // (count, weight, Σ w·p) accumulators.
        let mut total_wp = 0.0_f64;
        let mut trig = (0usize, 0u64, 0.0_f64);
        let mut by_count: BTreeMap<u16, (usize, u64, f64)> = BTreeMap::new();
        // criteria index → (count, weight, Σ w·p, books with trigger);
        // last slot = books without criteria.
        let none_slot = idx.criteria_names.len();
        let mut by_crit = vec![(0usize, 0u64, 0.0_f64, 0usize); none_slot + 1];

        for b in &idx.books {
            let w = weight_by_sim.get(&b.sim_id).copied().unwrap_or(0);
            let wp = w as f64 * (b.payout_x100 as f64 / 100.0);
            total_wp += wp;
            if b.fs_triggers > 0 {
                trig.0 += 1;
                trig.1 += w;
                trig.2 += wp;
                let e = by_count.entry(b.fs_triggers).or_insert((0, 0, 0.0));
                e.0 += 1;
                e.1 += w;
                e.2 += wp;
            }
            let ci = (b.criteria as usize).min(none_slot);
            let c = &mut by_crit[ci];
            c.0 += 1;
            c.1 += w;
            c.2 += wp;
            if b.fs_triggers > 0 {
                c.3 += 1;
            }
        }

        let rtp = total_wp / t / cost;
        let group = |count: usize, w: u64, wp: f64| {
            json!({
                "count": count,
                "probability": w as f64 / t,
                "oneIn": if w > 0 { t / w as f64 } else { f64::INFINITY },
                "avgWin": if w > 0 { wp / w as f64 } else { 0.0 },
                "rtpContribution": wp / t / cost,
            })
        };

        let by_trigger_count: Vec<_> = by_count
            .iter()
            .map(|(&k, &(count, w, wp))| {
                let mut g = group(count, w, wp);
                g["triggers"] = json!(k);
                g
            })
            .collect();
        let mut by_criteria: Vec<_> = by_crit
            .iter()
            .enumerate()
            .filter(|(_, &(count, ..))| count > 0)
            .map(|(i, &(count, w, wp, fs))| {
                let mut g = group(count, w, wp);
                g["criteria"] = json!(idx
                    .criteria_names
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| "(none)".to_string()));
                g["booksWithTrigger"] = json!(fs);
                g
            })
            .collect();
        // Most likely first — reads as "what you usually get".
        by_criteria.sort_by(|a, b| {
            b["probability"]
                .as_f64()
                .unwrap_or(0.0)
                .total_cmp(&a["probability"].as_f64().unwrap_or(0.0))
        });

        let (tb, tw, twp) = trig;
        modes.push(json!({
            "mode": m.config.name,
            "imported": true,
            "cost": m.config.cost,
            "totalBooks": idx.books.len(),
            "booksWithTrigger": tb,
            "triggerProbability": tw as f64 / t,
            "triggerOneIn": if tw > 0 { t / tw as f64 } else { f64::INFINITY },
            "avgTriggerWin": if tw > 0 { twp / tw as f64 } else { 0.0 },
            "triggerRtpContribution": twp / t / cost,
            "triggerRtpShare": if rtp > 0.0 { (twp / t / cost) / rtp } else { 0.0 },
            "byTriggerCount": by_trigger_count,
            "byCriteria": by_criteria,
        }));
    }
    Ok(Json(json!({ "game": name, "modes": modes })))
}

async fn game_checks(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<checks::GameCheckReport>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let modes: Vec<(String, stats::ModeStats, bool)> = game
        .modes
        .iter()
        .map(|m| {
            (
                m.config.name.clone(),
                stats::analyze(&m.lut, m.config.cost),
                m.config.cost_is_int,
            )
        })
        .collect();
    Ok(Json(checks::run_game_checks(&modes)))
}

async fn start_import(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    state.books.start_import(game);
    Ok(Json(json!({ "ok": true })))
}

async fn import_status(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let status = state.books.import_status(&name);
    let imported = state.books.is_imported(&game);
    Ok(Json(json!({ "imported": imported, "job": status })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrepareSession {
    session_id: String,
    game: String,
    /// Balance in micro-units (1,000,000 = $1).
    balance: Option<u64>,
    currency: Option<String>,
    social: Option<bool>,
    language: Option<String>,
}

async fn prepare_session(
    State(state): State<AppState>,
    Json(body): Json<PrepareSession>,
) -> Json<serde_json::Value> {
    state.sessions.prepare(
        &body.session_id,
        &body.game,
        body.balance,
        body.currency,
        body.social,
        body.language,
    );
    Json(json!({ "ok": true }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetForce {
    game: String,
    /// null → clear.
    mode: Option<String>,
    sim_id: Option<u64>,
    /// If present: force limited to this session, otherwise the whole game.
    session_id: Option<String>,
}

async fn set_force(
    State(state): State<AppState>,
    Json(body): Json<SetForce>,
) -> Json<serde_json::Value> {
    let force = match (body.mode, body.sim_id) {
        (Some(m), Some(s)) => Some((m, s)),
        _ => None,
    };
    match body.session_id {
        Some(sid) => state.sessions.set_session_force(&sid, &body.game, force),
        None => state.sessions.set_game_force(&body.game, force),
    }
    Json(json!({ "ok": true }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResetSessions {
    game: Option<String>,
}

async fn reset_sessions(
    State(state): State<AppState>,
    Json(body): Json<ResetSessions>,
) -> Json<serde_json::Value> {
    state.sessions.reset(body.game.as_deref());
    Json(json!({ "ok": true }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrowdSimReq {
    game: String,
    mode: String,
    #[serde(default = "default_players")]
    players: u32,
    #[serde(default = "default_spins")]
    spins: u32,
    /// Starting bankroll in bets.
    #[serde(default = "default_bankroll")]
    start_balance: f64,
    #[serde(default = "default_seed")]
    seed: u64,
}

fn default_players() -> u32 {
    10_000
}
fn default_spins() -> u32 {
    1_000
}
fn default_bankroll() -> f64 {
    100.0
}
fn default_seed() -> u64 {
    42
}

async fn run_crowdsim(
    State(state): State<AppState>,
    Json(req): Json<CrowdSimReq>,
) -> Result<Json<slotlab_core::crowdsim::CrowdResult>, ApiError> {
    let game = state
        .registry
        .get(&req.game)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {}", req.game)))?;
    let mode = game
        .resolve_mode(&req.mode)
        .ok_or_else(|| ApiError::not_found(format!("unknown mode: {}", req.mode)))?;
    let players = req.players.min(1_000_000);
    let spins = req.spins.min(100_000);
    let lut = mode.lut.clone();
    let cost = mode.config.cost;
    let params = slotlab_core::crowdsim::CrowdParams {
        players,
        spins,
        start_balance: req.start_balance.max(cost),
        seed: req.seed,
    };
    let result = tokio::task::spawn_blocking(move || {
        slotlab_core::crowdsim::run(&lut, cost, &params)
    })
    .await
    .map_err(|e| ApiError::bad_request(format!("crowdsim: {e}")))?;
    Ok(Json(result))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConvergenceReq {
    game: String,
    mode: String,
    #[serde(default = "default_conv_spins")]
    spins: u64,
    #[serde(default = "default_seed")]
    seed: u64,
}

fn default_conv_spins() -> u64 {
    10_000_000
}

async fn run_convergence(
    State(state): State<AppState>,
    Json(req): Json<ConvergenceReq>,
) -> Result<Json<slotlab_core::crowdsim::ConvergenceResult>, ApiError> {
    let game = state
        .registry
        .get(&req.game)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {}", req.game)))?;
    let mode = game
        .resolve_mode(&req.mode)
        .ok_or_else(|| ApiError::not_found(format!("unknown mode: {}", req.mode)))?;
    let spins = req.spins.min(100_000_000);
    let lut = mode.lut.clone();
    let cost = mode.config.cost;
    let seed = req.seed;
    let result = tokio::task::spawn_blocking(move || {
        slotlab_core::crowdsim::rtp_convergence(&lut, cost, spins, seed)
    })
    .await
    .map_err(|e| ApiError::bad_request(format!("convergence: {e}")))?;
    Ok(Json(result))
}

async fn sse_events(
    State(state): State<AppState>,
) -> axum::response::sse::Sse<
    impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    use axum::response::sse::{Event, KeepAlive, Sse};
    let rx = state.events_tx.subscribe();
    let stream = async_stream::stream! {
        let mut rx = rx;
        loop {
            match rx.recv().await {
                Ok(msg) => yield Ok(Event::default().data(msg)),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn list_bookmarks(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<Vec<crate::persist::Bookmark>> {
    Json(state.registry.list_bookmarks(q.get("game").map(|s| s.as_str())))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddBookmark {
    game: String,
    mode: String,
    sim_id: u64,
    #[serde(default)]
    description: String,
    #[serde(default)]
    payout_x100: u64,
}

async fn add_bookmark(
    State(state): State<AppState>,
    Json(body): Json<AddBookmark>,
) -> Result<Json<crate::persist::Bookmark>, ApiError> {
    let bm = state.registry.add_bookmark(
        body.game,
        body.mode,
        body.sim_id,
        body.description,
        body.payout_x100,
    )?;
    Ok(Json(bm))
}

async fn remove_bookmark(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let removed = state.registry.remove_bookmark(id)?;
    if !removed {
        return Err(ApiError::not_found("unknown bookmark"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn tls_status() -> Json<serde_json::Value> {
    Json(json!({ "trusted": crate::tls::is_trusted() }))
}

async fn tls_install() -> Result<Json<serde_json::Value>, ApiError> {
    crate::tls::install_ca().map_err(ApiError::from)?;
    Ok(Json(json!({ "ok": true, "trusted": crate::tls::is_trusted() })))
}

async fn game_review_ids(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let picks: Vec<review::ReviewIds> = game
        .modes
        .iter()
        .map(|m| review::pick_review_ids(&m.config.name, &m.lut))
        .collect();
    let text = review::format_review_block(&name, &picks);
    Ok(Json(json!({ "picks": picks, "text": text })))
}
