use crate::persist::GameEntry;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use slotlab_core::{assets, checks, provenance, review, stats};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/ui/health", get(health))
        .route("/api/ui/games", get(list_games).post(register_game))
        .route(
            "/api/ui/games/{name}",
            delete(remove_game).patch(update_game),
        )
        .route("/api/ui/games/{name}/reload", post(reload_game))
        .route("/api/ui/games/{name}/version", get(game_version))
        .route("/api/ui/games/{name}/stats", get(game_stats))
        .route("/api/ui/games/{name}/bonus-stats", get(bonus_stats))
        .route("/api/ui/games/{name}/mechanics", get(game_mechanics))
        .route("/api/ui/games/{name}/populations", get(populations))
        .route("/api/ui/games/{name}/reach", get(game_reach))
        .route("/api/ui/games/{name}/market", get(game_market))
        .route("/api/ui/market", get(market_status))
        .route("/api/ui/market/refresh", post(market_refresh_now))
        .route("/api/ui/games/{name}/checks", get(game_checks))
        .route("/api/ui/games/{name}/assets", get(game_assets))
        .route("/api/ui/games/{name}/challenge", post(challenge_calc))
        .route("/api/ui/games/{name}/review-ids", get(game_review_ids))
        .route(
            "/api/ui/games/{name}/import",
            post(start_import).get(import_status),
        )
        .route("/api/ui/sessions/prepare", post(prepare_session))
        .route("/api/ui/force", post(set_force))
        .route("/api/ui/pool", get(get_pool).post(set_pool))
        .route("/api/ui/sessions/reset", post(reset_sessions))
        .route("/api/ui/sessions/balance", post(set_balance))
        .route("/api/ui/faults", get(get_faults).post(set_faults))
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

/// Runs a handler body on the blocking pool: LUT walks (a million rows),
/// books index loads and the simulations take from tens of ms to seconds,
/// and would otherwise hold a runtime worker — stalling the other UI requests
/// and the RGS bets for as long.
pub async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
}

/// Uniform API error: { "error": "..." } + status.
#[derive(Debug)]
pub struct ApiError(StatusCode, String);

impl ApiError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self(StatusCode::NOT_FOUND, msg.into())
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, msg.into())
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, msg.into())
    }
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self(StatusCode::CONFLICT, msg.into())
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

async fn list_games(State(state): State<AppState>) -> Result<Json<Vec<GameSummary>>, ApiError> {
    blocking(move || Ok(Json(list_games_sync(&state)))).await
}

fn list_games_sync(state: &AppState) -> Vec<GameSummary> {
    let mut out = Vec::new();
    for entry in state.registry.list_entries() {
        match state.registry.get(&entry.name) {
            Some(game) => {
                let modes = game
                    .modes
                    .iter()
                    .map(|m| {
                        let s = m.stats();
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
    out
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
        app_dir: None,
    };
    let game = state.registry.register(entry)?;
    // Books import started immediately in the background.
    let (books, g) = (state.books.clone(), game.clone());
    if !blocking(move || Ok(books.is_imported(&g))).await? {
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
    state.books.invalidate_game(&name);
    let entries = state.registry.list_entries();
    tokio::task::spawn_blocking(move || crate::books::prune_orphan_caches(&entries));
    Ok(Json(json!({ "ok": true })))
}

async fn reload_game(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    reload_and_import(&state, &name).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Reads the game's files again, drops the books cached in memory and starts
/// the import when the books on disk changed too. Shared by the Reload
/// button and the source watcher (`crate::watch_sources`).
pub async fn reload_and_import(state: &AppState, name: &str) -> Result<(), ApiError> {
    let (registry, n) = (state.registry.clone(), name.to_string());
    let game = blocking(move || registry.reload(&n).map_err(ApiError::from)).await?;
    state.books.invalidate_game(name);
    let (books, g) = (state.books.clone(), game.clone());
    if !blocking(move || Ok(books.is_imported(&g))).await? {
        state.books.start_import(game);
    }
    let _ = state.events_tx.send(json!({ "type": "reload", "game": name }).to_string());
    Ok(())
}

/// What build of the math the panels show: when it was read, whether its
/// files moved on disk since, whether the books are imported. Polled by
/// Analyze (cheap: a few `stat`).
async fn game_version(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let ms = |t: std::time::SystemTime| t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let (books, g) = (state.books.clone(), game.clone());
    let imported = blocking(move || Ok(books.is_imported(&g))).await?;
    let importing = matches!(state.books.import_status(&name).map(|s| s.state), Some(crate::books::ImportState::Running));
    Ok(Json(json!({
        "game": name,
        "fingerprint": game.fingerprint.to_string(),
        "loadedAt": ms(game.loaded_at),
        "sourceMtime": game.source_mtime().map(ms),
        "sourceChanged": game.source_changed(),
        "imported": imported,
        "importing": importing,
    })))
}

async fn game_stats(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    blocking(move || game_stats_sync(&state, name)).await
}

fn game_stats_sync(state: &AppState, name: String) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let modes: Vec<serde_json::Value> = game
        .modes
        .iter()
        .map(|m| {
            let s = m.stats();
            json!({ "mode": m.config.name, "stats": s })
        })
        .collect();
    Ok(Json(json!({ "game": name, "modes": modes })))
}

#[derive(Deserialize)]
struct MechanicsQuery {
    mode: String,
    /// `all` (défaut), `bonus` (books qui ouvrent des free spins), `nobonus`.
    #[serde(default)]
    scope: Option<String>,
}

/// Mécaniques d'un mode, pondérées par la LUT : pour chaque champ des
/// événements (type de poisson, symbole gagnant, niveau…), la répartition que
/// le joueur voit, à côté de celle que la simulation a produite
/// (`slotlab_core::mechanics`). Générique : aucune configuration par jeu.
async fn game_mechanics(
    State(state): State<AppState>,
    Path(name): Path<String>,
    axum::extract::Query(q): axum::extract::Query<MechanicsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let mode = game
        .modes
        .iter()
        .find(|m| m.config.name == q.mode)
        .ok_or_else(|| ApiError::not_found(format!("unknown mode: {}", q.mode)))?;
    let books = {
        let (manager, g, m) = (state.books.clone(), game.clone(), q.mode.clone());
        blocking(move || Ok(manager.mode_books(&g, &m)?)).await?
    };
    let path = state
        .books
        .cache_dir_for(&game)
        .join(format!("books_{}.mech", q.mode));
    let scope = q.scope.unwrap_or_else(|| "all".into());
    let lut = mode.lut.clone();
    let total = lut.total_weight as f64;

    // Stats clés déclarées par le jeu, à côté de sa math (`<jeu>/slotlab.mechanics.json`,
    // publish_dir = `<jeu>/library/publish_files`). Lues à chaque requête : une
    // modification se voit sans ré-import.
    let defs_path = [2usize, 1, 3].iter().find_map(|&up| {
        let mut p = game.entry.publish_dir.clone();
        for _ in 0..up {
            p = p.parent()?.to_path_buf();
        }
        let f = p.join("slotlab.mechanics.json");
        f.exists().then_some(f)
    });
    let mode_name = q.mode.clone();

    let report = tokio::task::spawn_blocking(move || -> anyhow::Result<serde_json::Value> {
        use slotlab_core::mechanics::{aggregate, key_stat, KeyStatsFile};
        let weight_by_sim: std::collections::HashMap<u64, u64> =
            lut.rows.iter().map(|r| (r.sim_id, r.weight)).collect();
        let file = crate::books::format::MechFile::read_from(&path)?;
        anyhow::ensure!(
            file.source_fingerprint == books.index.source_fingerprint
                && file.mech.offsets.len() == books.index.books.len() + 1,
            "mechanics cache does not match the books — SlotLab Games tab → Import books"
        );
        let idx = &books.index;
        let weights: Vec<f64> = idx
            .books
            .iter()
            .map(|b| weight_by_sim.get(&b.sim_id).copied().unwrap_or(0) as f64)
            .collect();
        let include_for = |sc: &str| -> Vec<bool> {
            idx.books
                .iter()
                .map(|b| match sc {
                    "bonus" => b.fs_triggers > 0,
                    "nobonus" => b.fs_triggers == 0,
                    _ => true,
                })
                .collect()
        };
        let mass = |inc: &[bool]| -> f64 {
            weights.iter().zip(inc).filter(|(_, &i)| i).map(|(w, _)| *w).sum()
        };
        let include = include_for(&scope);
        let in_scope = mass(&include);
        let facets = aggregate(&file.mech, &weights, &include);

        // Stats clés : un relevé par périmètre, au plus trois.
        let (key_stats, key_error) = match &defs_path {
            None => (Vec::new(), None),
            Some(p) => match std::fs::read_to_string(p)
                .map_err(anyhow::Error::from)
                .and_then(|t| Ok(serde_json::from_str::<KeyStatsFile>(&t)?))
            {
                Err(e) => (Vec::new(), Some(format!("{}: {e:#}", p.display()))),
                Ok(defs) => {
                    let bonus_mass = mass(&include_for("bonus"));
                    let mut by_scope: std::collections::HashMap<String, Vec<_>> = Default::default();
                    let mut out = Vec::new();
                    for d in &defs.stats {
                        let mut sc = d.scope.clone().unwrap_or_else(|| "bonus".into());
                        if sc == "bonus" && bonus_mass <= 0.0 {
                            sc = "all".into(); // mode sans bonus (achat de spins, Degen…)
                        }
                        let f = by_scope
                            .entry(sc.clone())
                            .or_insert_with(|| aggregate(&file.mech, &weights, &include_for(&sc)));
                        if let Some(k) = key_stat(d, &mode_name, &sc, f) {
                            out.push(k);
                        }
                    }
                    (out, None)
                }
            },
        };
        Ok(json!({
            "scope": scope,
            "books": include.iter().filter(|&&i| i).count(),
            "probability": if total > 0.0 { in_scope / total } else { 0.0 },
            "facets": facets,
            "keyStats": key_stats,
            "keyStatsFile": defs_path.as_ref().map(|p| p.display().to_string()),
            "keyStatsError": key_error,
        }))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))??;
    let mut out = report;
    out["mode"] = json!(q.mode);
    Ok(Json(out))
}

/// Bonus (freespin) trigger stats per mode, weighted by the LUT: the real
/// per-spin probabilities, unlike the raw book counts shown in Search.
/// Joins the books .idx sidecar (fs_triggers, fs_retriggers, fs_tier,
/// criteria) with the LUT weights.
async fn bonus_stats(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    blocking(move || bonus_stats_sync(&state, name)).await
}

fn bonus_stats_sync(state: &AppState, name: String) -> Result<Json<serde_json::Value>, ApiError> {
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
        let cost = if m.config.cost > 0.0 {
            m.config.cost
        } else {
            1.0
        };
        let weight_by_sim: HashMap<u64, u64> =
            lut.rows.iter().map(|r| (r.sim_id, r.weight)).collect();

        // (count, weight, Σ w·p) accumulators.
        let mut total_wp = 0.0_f64;
        let mut trig = (0usize, 0u64, 0.0_f64);
        let mut by_count: BTreeMap<u16, (usize, u64, f64)> = BTreeMap::new();
        // Books à bonus (même population que `trig`), par nombre de retriggers.
        let mut by_retriggers: BTreeMap<u16, (usize, u64, f64)> = BTreeMap::new();

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
                let r = by_retriggers.entry(b.fs_retriggers).or_insert((0, 0, 0.0));
                r.0 += 1;
                r.1 += w;
                r.2 += wp;
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
        let by_retrigger_count: Vec<_> = by_retriggers
            .iter()
            .map(|(&k, &(count, w, wp))| {
                let mut g = group(count, w, wp);
                g["retriggers"] = json!(k);
                // Part des BONUS, pas des spins : sur un mode de base, « 1 bonus
                // sur 4 retrigger » se lit mieux qu'une probabilité par spin.
                g["shareOfBonuses"] = json!(if trig.1 > 0 {
                    w as f64 / trig.1 as f64
                } else {
                    0.0
                });
                g
            })
            .collect();
        // ── D'où viennent les gains : tranches × bonus/palier, paliers de
        // bonus avec leurs retriggers, criteria (`slotlab_core::provenance`).
        //
        // Une entrée par LIGNE DE LUT, gain de la LUT compris : les tranches
        // retombent ainsi exactement sur celles de l'onglet Distribution. Le
        // palier vient des positions du déclenchement, pas du nom du criteria :
        // les portes d'entrée d'un même bonus (`freegame_3`, `wave_3`, un
        // `wincap`…) s'additionnent d'elles-mêmes, et un `mystery_0` qui
        // n'ouvre rien n'y figure pas.
        let facts = book_facts(lut, idx);
        let buckets = &m.stats().buckets;
        let prov = provenance::analyze(&facts, m.config.cost, &buckets, &idx.criteria_names);

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
            "byRetriggerCount": by_retrigger_count,
            "byCriteria": prov.criteria,
            "byTier": prov.tiers,
            "byBucket": prov.buckets,
        }));
    }
    Ok(Json(json!({ "game": name, "modes": modes })))
}

/// Une entrée par LIGNE DE LUT (poids et gain de la LUT), enrichie de ce que
/// l'index des books sait du book : bonus, palier, retriggers, criteria,
/// spins, gain du bonus seul.
fn book_facts(
    lut: &slotlab_core::model::Lut,
    idx: &crate::books::format::ModeBookIndex,
) -> Vec<provenance::BookFacts> {
    lut.rows
        .iter()
        .map(|r| {
            let rec = idx.find(r.sim_id);
            provenance::BookFacts {
                sim_id: r.sim_id,
                weight: r.weight,
                payout_x100: r.payout_x100,
                bonus: rec.is_some_and(|b| b.fs_triggers > 0),
                tier: rec.map_or(0, |b| b.fs_tier),
                retriggers: rec.map_or(0, |b| b.fs_retriggers),
                criteria: rec
                    .filter(|b| b.criteria != u16::MAX)
                    .map(|b| b.criteria as usize),
                spins_initial: rec.map_or(0, |b| b.fs_spins_initial),
                spins_final: rec.map_or(0, |b| b.fs_spins_final),
                fg_win_x100: rec.and_then(|b| b.fg_win_x100),
                sym_low: rec.map_or(0, |b| b.sym_low),
                sym_high: rec.map_or(0, |b| b.sym_high),
                sym_low_fs: rec.map_or(0, |b| b.sym_low_fs),
                sym_high_fs: rec.map_or(0, |b| b.sym_high_fs),
                antic: rec.is_some_and(|b| b.antic),
            }
        })
        .collect()
}

/// « Sur N spins, combien de sessions n'atteignent jamais X ? » — exact, sur la LUT
/// publiée du mode (`slotlab_core::reach`). `?mode=BASE&spins=6000&x=500`.
async fn game_reach(
    State(state): State<AppState>,
    Path(name): Path<String>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<slotlab_core::reach::SessionReach>, ApiError> {
    blocking(move || game_reach_sync(&state, name, q)).await
}

fn game_reach_sync(
    state: &AppState,
    name: String,
    q: std::collections::HashMap<String, String>,
) -> Result<Json<slotlab_core::reach::SessionReach>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let mode_name = q.get("mode").ok_or_else(|| ApiError::bad_request("mode is required"))?;
    let mode = game
        .resolve_mode(mode_name)
        .ok_or_else(|| ApiError::not_found(format!("unknown mode: {mode_name}")))?;
    let spins: u32 = q
        .get("spins")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(1_000)
        .clamp(1, 100_000_000);
    let x: f64 = q
        .get("x")
        .and_then(|v| v.trim().parse().ok())
        .filter(|v: &f64| v.is_finite() && *v > 0.0)
        .ok_or_else(|| ApiError::bad_request("x must be a positive multiplier"))?;
    Ok(Json(slotlab_core::reach::session_reach(&mode.lut, spins, x)))
}

/// The market reference in use (day of the 30-day ranking, rule, size) and
/// where its daily refresh stands.
async fn market_status() -> Json<serde_json::Value> {
    Json(json!({
        "market": slotlab_core::market::info(),
        "embeddedFetched": slotlab_core::market::embedded_fetched(),
        "refresh": crate::market_refresh::status(),
    }))
}

/// Rebuilds the market reference now (background; `started: false` if a
/// refresh is already running).
async fn market_refresh_now() -> Json<serde_json::Value> {
    let started = crate::market_refresh::trigger();
    Json(json!({ "started": started, "refresh": crate::market_refresh::status() }))
}

/// Every mode of a game against the market's modes of the same kind — base
/// modes of a close max win, antes of the same cost, buys of a close cost
/// (`slotlab_core::market`).
async fn game_market(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<slotlab_core::market::GameMarket>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let result = tokio::task::spawn_blocking(move || {
        let modes: Vec<(String, &slotlab_core::model::Lut, f64)> =
            game.modes.iter().map(|m| (m.config.name.clone(), m.lut.as_ref(), m.config.cost)).collect();
        slotlab_core::market::compare_game(&modes)
    })
    .await
    .map_err(|e| ApiError::internal(format!("market: {e}")))?;
    Ok(Json(result))
}

/// Toutes les populations de chaque mode — mode entier, spins sans bonus,
/// bonus, paliers, variantes, criteria — avec le même bloc de statistiques
/// (`slotlab_core::population`). `?thresholds=7,15` ajoute des seuils mesurés
/// exactement (« part sous 7x »).
///
/// Un mode dont les books ne sont pas importés n'a que sa population `all`,
/// calculée sur la LUT seule : c'est l'index des books qui dit ce qu'est un
/// bonus.
async fn populations(
    State(state): State<AppState>,
    Path(name): Path<String>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    blocking(move || populations_sync(&state, name, q)).await
}

fn populations_sync(
    state: &AppState,
    name: String,
    q: std::collections::HashMap<String, String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use slotlab_core::population;

    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let thresholds: Vec<f64> = q
        .get("thresholds")
        .map(|t| t.split(',').filter_map(|v| v.trim().parse().ok()).collect())
        .unwrap_or_default();
    let mut modes = Vec::new();
    for m in &game.modes {
        let lut = &m.lut;
        let (imported, facts, criteria_names) = match state.books.mode_books(&game, &m.config.name) {
            Ok(books) => (true, book_facts(lut, &books.index), books.index.criteria_names.clone()),
            Err(_) => (
                false,
                lut.rows
                    .iter()
                    .map(|r| provenance::BookFacts {
                        sim_id: r.sim_id,
                        weight: r.weight,
                        payout_x100: r.payout_x100,
                        ..Default::default()
                    })
                    .collect(),
                Vec::new(),
            ),
        };
        let mut pops = population::analyze(&facts, m.config.cost, &criteria_names, &thresholds);
        if !imported {
            pops.retain(|p| p.key == "all");
        }
        modes.push(json!({
            "mode": m.config.name,
            "cost": m.config.cost,
            "imported": imported,
            "populations": pops,
            // Symboles payants low / high vus par round (LUT) ; null : aucun
            // plateau lisible ou noms non classés.
            "symbols": slotlab_core::symbols::seen(&facts),
            // Anticipations du spin payé (part des rounds, LUT) ; null : aucune.
            "anticipation": slotlab_core::symbols::anticipation(&facts),
        }));
    }
    Ok(Json(json!({ "game": name, "modes": modes })))
}

/// Asset budget of the game's frontend build.
///
/// ⚠️ Reads the build folder on every call, deliberately: it is what the player
/// downloads, it changes at every `vite build`, and a cached answer would show
/// the weight of a build that no longer exists. A full walk of a slot build is a
/// few hundred `stat` calls.
async fn game_assets(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<assets::AssetReport>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let build = game.entry.build_dir.clone().ok_or_else(|| {
        ApiError::not_found(
            "no frontend build detected for this game — build it              (pnpm --filter <game> build) then Reload"
                .to_string(),
        )
    })?;
    if !build.is_dir() {
        return Err(ApiError::not_found(format!(
            "build folder is gone: {}",
            build.display()
        )));
    }
    // Textures the game mipmaps itself pay a third more VRAM. Matched by file
    // name suffix so a game only has to name the file, not its full path.
    const MIPMAPPED: &[&str] = &["typo_gains_gold.webp"];
    let build_for_walk = build.clone();
    let report = tokio::task::spawn_blocking(move || assets::analyse(&build_for_walk, MIPMAPPED))
        .await
        .map_err(|e| ApiError::internal(format!("asset scan failed: {e}")))?;
    Ok(Json(report))
}

async fn game_checks(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<checks::GameCheckReport>, ApiError> {
    blocking(move || game_checks_sync(&state, name)).await
}

fn game_checks_sync(state: &AppState, name: String) -> Result<Json<checks::GameCheckReport>, ApiError> {
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
                m.stats().clone(),
                m.config.cost_is_int,
            )
        })
        .collect();
    Ok(Json(checks::run_game_checks(&modes)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChallengeReq {
    /// Default: the cheapest mode (usually BASE) — challenges are ground
    /// in the base game since buys dilute the round multiplier.
    mode: Option<String>,
    min_bet: Option<f64>,
    awards: Option<Vec<f64>>,
    /// Empty/absent → standard ladder capped at the mode's max multiplier.
    targets: Option<Vec<f64>>,
}

/// Challenge calculator: EV matrix (targets × awards), operator break-evens
/// and per-award suggested targets, from the mode's published LUT.
async fn challenge_calc(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<ChallengeReq>,
) -> Result<Json<slotlab_core::challenge::ChallengeReport>, ApiError> {
    blocking(move || challenge_calc_sync(&state, name, req)).await
}

fn challenge_calc_sync(
    state: &AppState,
    name: String,
    req: ChallengeReq,
) -> Result<Json<slotlab_core::challenge::ChallengeReport>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let mode = match &req.mode {
        Some(m) => game
            .resolve_mode(m)
            .ok_or_else(|| ApiError::not_found(format!("unknown mode: {m}")))?,
        None => game
            .modes
            .iter()
            .min_by(|a, b| a.config.cost.total_cmp(&b.config.cost))
            .ok_or_else(|| ApiError::bad_request("game has no modes"))?,
    };
    let min_bet = req
        .min_bet
        .filter(|b| *b > 0.0 && b.is_finite())
        .unwrap_or(slotlab_core::challenge::DEFAULT_MIN_BET);
    let awards: Vec<f64> = req
        .awards
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| slotlab_core::challenge::DEFAULT_AWARDS.to_vec())
        .into_iter()
        .filter(|a| *a > 0.0 && a.is_finite())
        .collect();
    let targets = req.targets.filter(|t| !t.is_empty()).map(|t| {
        t.into_iter()
            .filter(|x| *x > 0.0 && x.is_finite())
            .collect::<Vec<_>>()
    });
    let report = slotlab_core::challenge::analyze(
        &mode.lut,
        &mode.config.name,
        mode.config.cost,
        min_bet,
        &awards,
        targets.as_deref(),
    );
    Ok(Json(report))
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
    let imported = blocking(move || Ok(state.books.is_imported(&game))).await?;
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

/// Bonus-only draw, per spin mode (base, hunt…): what can be restricted, and
/// what is restricted right now.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PoolQuery {
    game: String,
}

fn pool_mode(game: &crate::registry::LoadedGame, mode: Option<&str>) -> Result<String, ApiError> {
    match mode {
        Some(m) => game
            .resolve_mode(m)
            .map(|m| m.config.name.clone())
            .ok_or_else(|| ApiError::bad_request(format!("unknown mode: {m}"))),
        None => game
            .base_mode()
            .map(|m| m.config.name.clone())
            .ok_or_else(|| ApiError::bad_request("game has no mode")),
    }
}

async fn get_pool(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<PoolQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state
        .registry
        .get(&q.game)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {}", q.game)))?;
    let books = state.books.clone();
    let modes = tokio::task::spawn_blocking(move || crate::rgs::pool::all_options(&books, &game))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))??;
    let active: Vec<_> = state
        .sessions
        .game_pools(&q.game)
        .iter()
        .map(|p| p.summary())
        .collect();
    Ok(Json(json!({ "modes": modes, "active": active })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetPool {
    game: String,
    /// false → back to the full LUT.
    enabled: bool,
    mode: Option<String>,
    /// Scatters of the trigger; absent = any bonus.
    tier: Option<u8>,
}

async fn set_pool(
    State(state): State<AppState>,
    Json(body): Json<SetPool>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state
        .registry
        .get(&body.game)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {}", body.game)))?;
    let mode = pool_mode(&game, body.mode.as_deref())?;
    if !body.enabled {
        state.sessions.set_game_pool(&body.game, &mode, None);
        return Ok(Json(json!({ "ok": true, "active": null })));
    }
    let filter = crate::rgs::pool::PoolFilter { mode: mode.clone(), tier: body.tier };
    let books = state.books.clone();
    let pool = tokio::task::spawn_blocking(move || crate::rgs::pool::Pool::build(&books, &game, filter))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))??;
    let summary = pool.summary();
    state
        .sessions
        .set_game_pool(&body.game, &mode, Some(std::sync::Arc::new(pool)));
    Ok(Json(json!({ "ok": true, "active": summary })))
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
    // A fault surviving a reset is a bug that looks like the game's.
    state.faults.clear(body.game.as_deref());
    Json(json!({ "ok": true }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetBalance {
    session_id: String,
    game: String,
    /// Micro-units: 1,000,000 = $1.
    balance: u64,
}

async fn set_balance(
    State(state): State<AppState>,
    Json(body): Json<SetBalance>,
) -> Json<serde_json::Value> {
    let balance = state
        .sessions
        .set_balance(&body.session_id, &body.game, body.balance);
    Json(json!({ "ok": true, "balance": balance }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FaultsBody {
    game: String,
    #[serde(flatten)]
    faults: crate::rgs::faults::Faults,
}

async fn get_faults(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let game = q.get("game").cloned().unwrap_or_default();
    Json(json!({ "game": game, "faults": state.faults.get(&game) }))
}

/// Replaces a game's fault set in one call — the panel always sends the whole
/// state, so there is never a half-applied set of switches.
async fn set_faults(
    State(state): State<AppState>,
    Json(body): Json<FaultsBody>,
) -> Json<serde_json::Value> {
    state.faults.set(&body.game, body.faults);
    Json(json!({ "ok": true, "faults": state.faults.get(&body.game) }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrowdSimReq {
    game: String,
    /// One cohort per mode.
    #[serde(default)]
    modes: Vec<String>,
    /// Optional extra cohort playing several modes by share of rounds.
    #[serde(default)]
    mix: Vec<MixEntry>,
    #[serde(default = "default_players")]
    players: u32,
    #[serde(default = "default_spins")]
    spins: u32,
    /// Starting balance in base bets; absent = unlimited.
    #[serde(default)]
    bankroll: Option<f64>,
    #[serde(default = "default_seed")]
    seed: u64,
    /// What a drought and a letdown are (player feel).
    #[serde(default)]
    feel: slotlab_core::feel::FeelParams,
    /// Extra cohorts playing a fixed version of a mode.
    #[serde(default)]
    fixes: Vec<FixEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MixEntry {
    mode: String,
    share: f64,
}

/// A player-feel fix to play: `frustration` fixed on `mode`, paid by `payer`
/// (`slotlab_core::feel::payers`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixEntry {
    mode: String,
    frustration: String,
    payer: String,
}

fn default_players() -> u32 {
    10_000
}
fn default_spins() -> u32 {
    1_000
}
fn default_seed() -> u64 {
    42
}

/// Player cohorts on the published lookup tables — one per requested mode,
/// plus a mixed one when `mix` names two modes or more, plus one per `fixes`
/// entry, playing that mode re-weighted by a player-feel fix
/// (`slotlab_core::crowdsim`, `slotlab_core::feel`). Bonus figures come from
/// the books index.
async fn run_crowdsim(
    State(state): State<AppState>,
    Json(req): Json<CrowdSimReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    blocking(move || run_crowdsim_sync(&state, req)).await
}

fn run_crowdsim_sync(state: &AppState, req: CrowdSimReq) -> Result<Json<serde_json::Value>, ApiError> {
    use slotlab_core::crowdsim::{self, CrowdParams, FixRef, Scenario, SimMode};
    use slotlab_core::feel;

    let game = state
        .registry
        .get(&req.game)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {}", req.game)))?;
    let players = req.players.clamp(1, 200_000);
    let spins = req.spins.clamp(1, 100_000);

    // Every mode involved, resolved once.
    let mut names: Vec<String> = req.modes.clone();
    for m in req.mix.iter().map(|m| &m.mode).chain(req.fixes.iter().map(|f| &f.mode)) {
        if !names.contains(m) {
            names.push(m.clone());
        }
    }
    let mix: Vec<&MixEntry> = req.mix.iter().filter(|m| m.share > 0.0).collect();
    let scenario_count = req.modes.len() + usize::from(mix.len() >= 2) + req.fixes.len();
    if scenario_count == 0 {
        return Err(ApiError::bad_request("pick at least one mode"));
    }
    let total = players as u64 * spins as u64 * scenario_count as u64;
    if total > 2_000_000_000 {
        return Err(ApiError::bad_request(format!(
            "{total} rounds requested — keep players × spins × cohorts under 2 billion"
        )));
    }

    struct Loaded {
        name: String,
        cost: f64,
        lut: std::sync::Arc<slotlab_core::model::Lut>,
        facts: Vec<provenance::BookFacts>,
        criteria: Vec<String>,
        imported: bool,
    }
    let mut loaded = Vec::new();
    for name in &names {
        let mode = game
            .resolve_mode(name)
            .ok_or_else(|| ApiError::not_found(format!("unknown mode: {name}")))?;
        let lut = mode.lut.clone();
        let (facts, criteria, imported) = match state.books.mode_books(&game, &mode.config.name) {
            Ok(books) => (book_facts(&lut, &books.index), books.index.criteria_names.clone(), true),
            Err(_) => (
                lut.rows
                    .iter()
                    .map(|r| provenance::BookFacts { sim_id: r.sim_id, weight: r.weight, payout_x100: r.payout_x100, ..Default::default() })
                    .collect(),
                Vec::new(),
                false,
            ),
        };
        loaded.push(Loaded { name: mode.config.name.clone(), cost: mode.config.cost, lut, facts, criteria, imported });
    }

    let mut scenarios: Vec<Scenario> = req
        .modes
        .iter()
        .map(|m| Scenario { name: m.clone(), mix: vec![(names.iter().position(|n| n == m).unwrap(), 1.0)], fix: None })
        .collect();
    if mix.len() >= 2 {
        scenarios.push(Scenario {
            name: "Mixed".into(),
            mix: mix.iter().map(|m| (names.iter().position(|n| n == &m.mode).unwrap(), m.share)).collect(),
            fix: None,
        });
    }
    let feel_params = req.feel.sanitized();
    let params = CrowdParams { players, spins, bankroll: req.bankroll.filter(|b| *b > 0.0), seed: req.seed, feel: feel_params };
    let fixes = req.fixes;

    let result = (move || {
        // Each fix: the same books, re-weighted — one more (virtual) mode.
        for fx in &fixes {
            let i = names.iter().position(|n| n == &fx.mode).expect("resolved above");
            let l = &loaded[i];
            let shape = feel::shape_for(&l.lut, &l.facts, l.cost, l.imported, feel_params);
            let w: Vec<f64> = l.lut.rows.iter().map(|r| r.weight as f64).collect();
            let fixed = shape
                .fix_weights(&w, spins, &fx.frustration, &fx.payer)
                .map_err(|e| format!("{} — {} fix paid by {}: {e}", fx.mode, fx.frustration, fx.payer))?;
            let weights = feel::integer_weights(&fixed, l.lut.total_weight);
            let rows = l.lut.rows.iter().zip(weights).map(|(r, weight)| slotlab_core::model::LutRow { weight, ..*r }).collect();
            let fixed = Loaded {
                name: l.name.clone(),
                cost: l.cost,
                lut: std::sync::Arc::new(slotlab_core::model::Lut::new(rows)),
                facts: l.facts.clone(),
                criteria: l.criteria.clone(),
                imported: l.imported,
            };
            scenarios.push(Scenario {
                name: format!("{} · {} fix · {} pays", fx.mode, fx.frustration, fx.payer),
                mix: vec![(loaded.len(), 1.0)],
                fix: Some(FixRef { mode: fx.mode.clone(), frustration: fx.frustration.clone(), payer: fx.payer.clone() }),
            });
            loaded.push(fixed);
        }
        let modes: Vec<SimMode> = loaded
            .iter()
            .map(|l| SimMode { name: l.name.clone(), cost: l.cost, lut: &l.lut, facts: &l.facts, criteria_names: &l.criteria, imported: l.imported })
            .collect();
        crowdsim::run(&modes, &scenarios, &params)
    })()
    .map_err(ApiError::bad_request)?;
    Ok(Json(json!({ "game": req.game, "players": players, "spins": spins, "scenarios": result })))
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
    Json(
        state
            .registry
            .list_bookmarks(q.get("game").map(|s| s.as_str())),
    )
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
    Ok(Json(
        json!({ "ok": true, "trusted": crate::tls::is_trusted() }),
    ))
}

async fn game_review_ids(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    blocking(move || game_review_ids_sync(&state, name)).await
}

fn game_review_ids_sync(state: &AppState, name: String) -> Result<Json<serde_json::Value>, ApiError> {
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
