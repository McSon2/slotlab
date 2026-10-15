//! Multi-game public server: one process serves every game published under
//! a data directory, and the request's `Host` decides which one.
//!
//! The alternative — one container per game — meant editing a compose file
//! and picking a free port by hand for each new game. Here a game exists
//! as soon as its directory does, and the caches (decompressed blocks,
//! parsed books) stay shared, so ten games cost ten times the disk but not
//! ten times the memory.

use crate::gate::Gate;
use crate::persist::GameEntry;
use crate::share::ShareConfig;
use crate::state::AppState;
use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::{Extension, State};
use axum::http::{header, Request, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// `game.json`, written by the deployer next to the game's data. It is the
/// only thing that decides how a game is served.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Slug used in the RGS URLs.
    pub name: String,
    /// Public hostname, e.g. `skullperch.mcberry.fr`.
    pub host: String,
    /// Starting balance of a visitor, in whole currency units.
    #[serde(default = "default_balance")]
    pub balance: u64,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub social: bool,
    /// Shared access code. Empty → the link is open.
    #[serde(default)]
    pub access_code: String,
    /// Console de démonstration dans la page du jeu (choix du prochain
    /// spin). Réservée aux liens de présentation.
    #[serde(default)]
    pub console: bool,
    /// ISO date, informational (shown in the dashboard).
    #[serde(default)]
    pub published_at: String,
}

fn default_balance() -> u64 {
    1_000
}
fn default_currency() -> String {
    "USD".into()
}
fn default_language() -> String {
    "en".into()
}

pub struct HubGame {
    pub manifest: Manifest,
    pub build_dir: PathBuf,
    pub gate: Arc<Gate>,
    pub config: ShareConfig,
}

/// Dashboard hostname + the token the control API demands.
#[derive(Default, Clone)]
pub struct AdminConfig {
    pub host: Option<String>,
    pub token: Option<String>,
}

#[derive(Default)]
pub struct AdminSlot(RwLock<AdminConfig>);

impl AdminSlot {
    pub fn set(&self, cfg: AdminConfig) {
        *self.0.write().unwrap() = cfg;
    }
    fn get(&self) -> AdminConfig {
        self.0.read().unwrap().clone()
    }
}

#[derive(Default)]
pub struct Hub {
    /// Lowercased hostname → game.
    by_host: RwLock<HashMap<String, Arc<HubGame>>>,
    pub data_dir: RwLock<PathBuf>,
    /// Games whose directory exists but which failed to load.
    pub errors: RwLock<Vec<(String, String)>>,
    /// Directories without a `game.json`: games taken offline whose data
    /// was kept on purpose. Not an error — they only await a purge or a
    /// republish.
    pub retired: RwLock<Vec<String>>,
    pub admin: AdminSlot,
}

impl Hub {
    pub fn new(data_dir: PathBuf, admin: AdminConfig) -> Self {
        let hub = Self {
            data_dir: RwLock::new(data_dir),
            ..Default::default()
        };
        hub.admin.set(admin);
        hub
    }

    pub fn resolve(&self, host: &str) -> Option<Arc<HubGame>> {
        // The Host header may carry a port; the routing never does.
        let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
        self.by_host.read().unwrap().get(&host).cloned()
    }

    pub fn list(&self) -> Vec<Arc<HubGame>> {
        let mut v: Vec<_> = self.by_host.read().unwrap().values().cloned().collect();
        v.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
        v
    }

    /// Rereads the data directory and swaps the whole set of games. Called
    /// at startup and by the deployer once its files are in place — which
    /// is what lets a game be published without restarting the process and
    /// dropping everyone's session.
    pub fn reload(&self, state: &AppState) -> Result<usize> {
        let data_dir = self.data_dir.read().unwrap().clone();
        let mut entries = Vec::new();
        let mut games = Vec::new();
        let mut errors = Vec::new();
        let mut retired = Vec::new();

        let dir = std::fs::read_dir(&data_dir)
            .with_context(|| format!("reading {}", data_dir.display()))?;
        for e in dir.flatten() {
            let path = e.path();
            if !path.is_dir() {
                continue;
            }
            // `unpublish` without purge removes game.json and nothing else:
            // a missing manifest is a withdrawal, not a broken game.
            if !path.join("game.json").exists() {
                retired.push(e.file_name().to_string_lossy().to_string());
                continue;
            }
            match load_manifest(&path) {
                Ok(m) => {
                    entries.push(GameEntry {
                        name: m.name.clone(),
                        publish_dir: path.join("publish"),
                        front_url: String::new(),
                        build_dir: Some(path.join("build")),
                        // No sources on the hub: it serves a build, it does
                        // not audit one.
                        app_dir: None,
                    });
                    games.push((m, path));
                }
                Err(err) => {
                    let label = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    errors.push((label, format!("{err:#}")));
                }
            }
        }

        // Two directories claiming the same slug would share one registry
        // entry and one books mapping — the last loaded silently winning.
        // Publishing the same game under two hostnames is legitimate, but
        // they must then carry different slugs.
        let mut seen: HashMap<String, PathBuf> = HashMap::new();
        for (m, path) in &games {
            if let Some(first) = seen.insert(m.name.clone(), path.clone()) {
                errors.push((
                    m.name.clone(),
                    format!(
                        "slug déjà utilisé par {} — donne un `name` distinct à chaque jeu publié",
                        first.display()
                    ),
                ));
            }
        }

        // Books first: the registry is what the RGS looks up, and it must
        // never point at a game whose books are not mapped yet.
        let caches: HashMap<String, PathBuf> = games
            .iter()
            .map(|(m, p)| (m.name.clone(), p.join("cache")))
            .collect();
        state.books.set_cache_dirs(caches);
        errors.extend(state.registry.load_all(entries));

        let mut by_host = HashMap::new();
        for (m, path) in games {
            if errors.iter().any(|(n, _)| n == &m.name) {
                continue;
            }
            let gate = Arc::new(if m.access_code.trim().is_empty() {
                Gate::open()
            } else {
                Gate::new(m.access_code.trim())
            });
            let config = ShareConfig {
                game: m.name.clone(),
                balance_micro: m.balance.saturating_mul(1_000_000).max(1),
                currency: m.currency.clone(),
                language: m.language.clone(),
                social: m.social,
                public_host: Some(m.host.clone()),
            };
            by_host.insert(
                m.host.to_ascii_lowercase(),
                Arc::new(HubGame {
                    manifest: m,
                    build_dir: path.join("build"),
                    gate,
                    config,
                }),
            );
        }
        let n = by_host.len();
        *self.by_host.write().unwrap() = by_host;
        for (name, err) in &errors {
            tracing::warn!("game `{name}` not served: {err}");
        }
        *self.errors.write().unwrap() = errors;
        retired.sort();
        *self.retired.write().unwrap() = retired;
        // A published game keeps the balance of visitors whose session was
        // evicted; with several games the largest wins, which is harmless.
        if let Some(max) = self
            .by_host
            .read()
            .unwrap()
            .values()
            .map(|g| g.config.balance_micro)
            .max()
        {
            state.sessions.set_default_balance(max);
        }
        Ok(n)
    }
}

fn load_manifest(dir: &Path) -> Result<Manifest> {
    let raw = std::fs::read(dir.join("game.json")).context("game.json missing")?;
    let m: Manifest = serde_json::from_slice(&raw).context("parsing game.json")?;
    anyhow::ensure!(!m.name.trim().is_empty(), "game.json: empty name");
    anyhow::ensure!(!m.host.trim().is_empty(), "game.json: empty host");
    anyhow::ensure!(
        dir.join("publish/index.json").is_file(),
        "publish/index.json missing"
    );
    anyhow::ensure!(
        dir.join("build/index.html").is_file(),
        "build/index.html missing"
    );
    anyhow::ensure!(dir.join("cache").is_dir(), "cache/ missing");
    Ok(m)
}

/// Resolves the game from the Host header, then hands the request the
/// game's share config and gate so the shared handlers work unchanged.
pub async fn resolve_mw(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if path == "/healthz" {
        return next.run(req).await;
    }
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bare = host.split(':').next().unwrap_or(&host).to_ascii_lowercase();
    // The control API answers on any host — the deployer calls it over the
    // LAN, where the Host is an IP. It checks the token itself.
    if path.starts_with("/api/hub/") {
        return next.run(req).await;
    }
    let admin = state.hub.admin.get();
    if admin.host.as_deref().map(str::to_ascii_lowercase) == Some(bare.clone()) {
        return dashboard(&state).await.into_response();
    }
    let Some(game) = state.hub.resolve(&host) else {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            format!("Aucun jeu publié sur {host}."),
        )
            .into_response();
    };
    // A visitor on host A must not be able to play game B by hitting its
    // RGS path directly.
    if let Some(rest) = path.strip_prefix("/api/rgs/") {
        let asked = rest.split('/').next().unwrap_or("");
        if !asked.eq_ignore_ascii_case(&game.manifest.name) {
            return StatusCode::NOT_FOUND.into_response();
        }
    }
    req.extensions_mut().insert(game.config.clone());
    req.extensions_mut().insert(game.gate.clone());
    req.extensions_mut().insert(game.clone());
    next.run(req).await
}

/// ⚠️ CACHE — every static file goes out `no-cache` + a content ETag.
///
/// The hub used to send NO cache header at all. Cloudflare fills that gap on
/// images with `cache-control: max-age=14400`, while `.json` / `.atlas` /
/// `.js` pass uncached. After every redeploy the browser therefore assembled
/// the NEW Spine skeleton with the OLD atlas page of the same name (and the
/// old symbol vignettes), for four hours: symbols cut anywhere, previous
/// symbol still shown. `Cmd+Shift+R` can't help — Pixi loads its images from
/// a worker. Lived on BLOOD WATER (2026-09-15), TOXIC SPINS (2026-10-03),
/// BUBBLE HEX (2026-10-05).
///
/// `no-cache` = "store it, but ask before reusing it": Cloudflare neither
/// edge-caches it nor applies its Browser Cache TTL, and the browser
/// revalidates with `If-None-Match`. An unchanged file costs a body-less 304;
/// a changed one is downloaded. Fixed for every game at once, whatever its
/// file names.
const STATIC_CACHE_CONTROL: &str = "no-cache";

/// Content ETag (xxh3 of the bytes actually sent: the HTML WITH its injected
/// console). Strong, but compared weakly: Cloudflare turns it into `W/"…"`
/// when it recompresses the body.
fn static_response(req_headers: &axum::http::HeaderMap, mime: String, body: Vec<u8>) -> Response {
    let tag = format!("\"{:016x}\"", xxhash_rust::xxh3::xxh3_64(&body));
    let fresh = req_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .map(|t| t.trim().trim_start_matches("W/"))
                .any(|t| t == "*" || t == tag)
        });
    if fresh {
        return (
            StatusCode::NOT_MODIFIED,
            [(header::ETAG, tag), (header::CACHE_CONTROL, STATIC_CACHE_CONTROL.to_string())],
        )
            .into_response();
    }
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::ETAG, tag),
            (header::CACHE_CONTROL, STATIC_CACHE_CONTROL.to_string()),
        ],
        body,
    )
        .into_response()
}

/// Static files of the game the Host resolved to.
pub async fn serve_static(
    Extension(game): Extension<Arc<HubGame>>,
    uri: Uri,
    req_headers: axum::http::HeaderMap,
) -> Response {
    let rel = uri.path().trim_start_matches('/');
    if rel.split('/').any(|c| c == "..") {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut path = game.build_dir.join(rel);
    if rel.is_empty() || !path.is_file() {
        path = game.build_dir.join("index.html");
    }
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            // La console est greffée sur le HTML servi, pas dans le build du
            // jeu : aucun jeu n'a à être modifié ni rebuildé pour l'avoir.
            // Dans l'onglet Play de Reelhouse (`embed=1`), la console est
            // dessinée par Reelhouse à côté du jeu : la page ne fait que lui
            // passer sa session.
            let embed = uri
                .query()
                .is_some_and(|q| q.split('&').any(|p| p == "embed=1"));
            let snippet = if embed {
                Some(crate::console::BRIDGE)
            } else if game.manifest.console {
                Some(crate::console::WIDGET)
            } else {
                None
            };
            if let Some(snippet) = snippet.filter(|_| mime.as_ref().starts_with("text/html")) {
                let html = String::from_utf8_lossy(&bytes);
                let injected = match html.rfind("</body>") {
                    Some(i) => format!("{}{}{}", &html[..i], snippet, &html[i..]),
                    None => format!("{html}{snippet}"),
                };
                return static_response(&req_headers, mime.as_ref().to_string(), injected.into_bytes());
            }
            static_response(&req_headers, mime.as_ref().to_string(), bytes)
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

// ─────────────────────────────── Control API ───────────────────────────────
//
// Called by the deployer (SlotLab, over the LAN) once its files are in
// place, and by the dashboard. Publishing this way — write game.json, then
// reload — is what avoids restarting the process, which would drop every
// visitor's session.

use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

fn authorized(state: &AppState, headers: &axum::http::HeaderMap) -> Result<(), Response> {
    let Some(expected) = state.hub.admin.get().token.filter(|t| !t.is_empty()) else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "control API disabled: no admin token configured",
        )
            .into_response());
    };
    let given = headers
        .get("x-slotlab-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    // Length-independent comparison is overkill here (LAN + Access), but
    // constant enough to not leak the token by timing a prefix.
    if given.len() == expected.len()
        && given
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "bad token").into_response())
    }
}

pub fn admin_router() -> Router<AppState> {
    Router::new()
        .route("/api/hub/games", get(api_games))
        .route("/api/hub/reload", post(api_reload))
        .route("/api/hub/publish", post(api_publish))
        .route("/api/hub/unpublish", post(api_unpublish))
        .route("/api/hub/sessions", get(api_sessions))
        .route("/api/hub/sessions/clear", post(api_sessions_clear))
        .route("/api/hub/sessions/remove", post(api_sessions_remove))
}

fn game_json(g: &Arc<HubGame>) -> serde_json::Value {
    json!({
        "name": g.manifest.name,
        "host": g.manifest.host,
        "url": format!("https://{}/play", g.manifest.host),
        "balance": g.manifest.balance,
        "currency": g.manifest.currency,
        "language": g.manifest.language,
        "social": g.manifest.social,
        "accessCode": g.manifest.access_code,
        "console": g.manifest.console,
        "publishedAt": g.manifest.published_at,
    })
}

async fn api_games(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    if let Err(r) = authorized(&state, &headers) {
        return r;
    }
    Json(json!({
        "games": state.hub.list().iter().map(game_json).collect::<Vec<_>>(),
        "errors": state.hub.errors.read().unwrap().clone(),
        "retired": state.hub.retired.read().unwrap().clone(),
        // Lu par Deploy : un `.idx` écrit par un SlotLab d'une autre version est
        // illisible ici, et chaque spin répondrait « books not imported ».
        "idxVersion": crate::books::format::IDX_VERSION,
    }))
    .into_response()
}

async fn api_reload(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Response {
    if let Err(r) = authorized(&state, &headers) {
        return r;
    }
    match state.hub.reload(&state) {
        Ok(n) => Json(json!({ "ok": true, "games": n })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
    }
}

/// The name becomes a path: a slug with a slash or a `..` would let a caller
/// write game.json anywhere the process can reach — and `.` would make a
/// purge wipe every game at once.
fn valid_slug(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Writes/overwrites a game's manifest, then reloads. The data files are
/// expected to be in place already (the deployer rsyncs them first).
async fn api_publish(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(m): Json<Manifest>,
) -> Response {
    if let Err(r) = authorized(&state, &headers) {
        return r;
    }
    if !valid_slug(&m.name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "nom de jeu invalide (a-z, 0-9, - et _ seulement)" })),
        )
            .into_response();
    }
    let dir = state.hub.data_dir.read().unwrap().join(&m.name);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("creating {}: {e}", dir.display()) })),
        )
            .into_response();
    }
    let raw = serde_json::to_vec_pretty(&m).unwrap_or_default();
    if let Err(e) = std::fs::write(dir.join("game.json"), raw) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("writing game.json: {e}") })),
        )
            .into_response();
    }
    match state.hub.reload(&state) {
        Ok(n) => {
            let served = state.hub.resolve(&m.host).is_some();
            let errors = state.hub.errors.read().unwrap().clone();
            Json(json!({
                "ok": served, "games": n,
                "url": format!("https://{}/play", m.host),
                "errors": errors,
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Unpublish {
    name: String,
    /// Also delete the data (books included). Off by default: taking a
    /// game offline must not destroy the 2 GB it took to upload.
    #[serde(default)]
    purge: bool,
}

async fn api_unpublish(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<Unpublish>,
) -> Response {
    if let Err(r) = authorized(&state, &headers) {
        return r;
    }
    if !valid_slug(&req.name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "nom de jeu invalide (a-z, 0-9, - et _ seulement)" })),
        )
            .into_response();
    }
    let dir = state.hub.data_dir.read().unwrap().join(&req.name);
    let res = if req.purge {
        std::fs::remove_dir_all(&dir)
    } else {
        std::fs::remove_file(dir.join("game.json"))
    };
    if let Err(e) = res {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e}") })),
        )
            .into_response();
    }
    let n = state.hub.reload(&state).unwrap_or(0);
    Json(json!({ "ok": true, "games": n, "purged": req.purge })).into_response()
}

async fn api_sessions(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Response {
    if let Err(r) = authorized(&state, &headers) {
        return r;
    }
    let out: Vec<_> = state
        .hub
        .list()
        .iter()
        .map(|g| {
            let sessions: Vec<_> = state
                .sessions
                .list_shared(&g.manifest.name)
                .into_iter()
                .map(|(id, s)| {
                    let idle = s.last_seen.elapsed().as_secs();
                    json!({
                        "sessionID": id,
                        "ip": s.ip,
                        "country": s.country,
                        "device": s.device,
                        "idleSecs": idle,
                        "balance": s.balance_micro,
                        "currency": s.currency,
                        "spins": s.round_counter,
                        "wagered": s.wagered_micro,
                        "won": s.won_micro,
                        // Un round resté « actif » depuis dix minutes n'est
                        // pas un spin en cours, c'est un onglet fermé.
                        "inRound": s.round.as_ref().is_some_and(|r| r.active) && idle < 60,
                    })
                })
                .collect();
            json!({ "game": g.manifest.name, "sessions": sessions })
        })
        .collect();
    Json(json!({ "games": out })).into_response()
}

// ──────────────────────────────── Dashboard ────────────────────────────────
//
// Served only on the admin hostname, which sits behind Cloudflare Access.
// The control token is embedded in the page: everyone able to load it has
// already been authenticated by Access, and a browser has no other way to
// carry it.

async fn dashboard(state: &AppState) -> axum::response::Html<String> {
    let admin = state.hub.admin.get();
    let token = admin.token.unwrap_or_default();
    let games: Vec<_> = state.hub.list().iter().map(game_json).collect();
    let errors = state.hub.errors.read().unwrap().clone();
    let retired = state.hub.retired.read().unwrap().clone();
    let data = json!({ "games": games, "errors": errors, "retired": retired });
    axum::response::Html(dashboard_html(
        &token,
        &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into()),
    ))
}

fn dashboard_html(token: &str, data: &str) -> String {
    format!(
        r##"<!doctype html>
<html lang="fr"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex,nofollow">
<title>SlotLab — jeux publiés</title>
<style>
  :root {{ color-scheme: dark; }}
  * {{ box-sizing: border-box; }}
  body {{ margin:0; background:#0d0b0f; color:#e8e2d9; padding:28px 20px 64px;
    font:15px/1.55 ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif; }}
  .wrap {{ max-width:1080px; margin:0 auto; }}
  header {{ display:flex; align-items:baseline; gap:14px; flex-wrap:wrap; margin-bottom:6px; }}
  h1 {{ font-size:20px; margin:0; letter-spacing:.01em; }}
  .dim {{ color:#8d8593; font-size:13px; }}
  button {{ font:inherit; font-size:13px; padding:7px 12px; border-radius:8px;
    border:1px solid #35303b; background:#1b171f; color:#e8e2d9; cursor:pointer; }}
  button:hover {{ background:#241f29; }}
  button.primary {{ background:#d2a03c; border-color:#d2a03c; color:#150f05; font-weight:600; }}
  button.primary:hover {{ background:#e0ae47; }}
  button.danger {{ color:#e2757c; border-color:#4a2b30; }}
  .card {{ background:#17141a; border:1px solid #2c2731; border-radius:13px;
    padding:18px 20px; margin-top:16px; }}
  .row {{ display:flex; align-items:center; gap:12px; flex-wrap:wrap; }}
  .row h2 {{ font-size:17px; margin:0; }}
  a {{ color:#d2a03c; }}
  code {{ font-family:ui-monospace,SFMono-Regular,Menlo,monospace; font-size:13px;
    background:#100e13; border:1px solid #2c2731; border-radius:6px; padding:2px 7px; }}
  table {{ width:100%; border-collapse:collapse; margin-top:12px; font-size:13px; }}
  th,td {{ text-align:left; padding:7px 10px; border-bottom:1px solid #241f29; }}
  th {{ color:#8d8593; font-weight:500; font-size:11px; text-transform:uppercase;
    letter-spacing:.08em; }}
  td.num, th.num {{ text-align:right; font-variant-numeric:tabular-nums; }}
  .pill {{ font-size:11px; padding:2px 8px; border-radius:20px; border:1px solid;
    letter-spacing:.05em; }}
  .pill.live {{ color:#7fd6a1; border-color:#2c5c41; background:#122019; }}
  .pill.idle {{ color:#8d8593; border-color:#2c2731; }}
  .empty {{ color:#8d8593; font-size:13px; padding:10px 0; }}
  .sub {{ color:#8d8593; font-size:11px; margin-top:2px; }}
  .err {{ color:#e2757c; }}
  .spacer {{ flex:1; }}
</style>
</head><body><div class="wrap">
  <header>
    <h1>Jeux publiés</h1>
    <span class="dim" id="count"></span>
    <span class="spacer"></span>
    <button onclick="reload()">Relire le disque</button>
  </header>
  <p class="dim">Un visiteur = une session, retenue dans son navigateur&nbsp;:
    recharger la page ne le dédouble pas. Les sessions inactives depuis plus de
    6&nbsp;h disparaissent d'elles-mêmes. Retirer un jeu le rend injoignable sans
    effacer ses données&nbsp;: republier le remet en ligne aussitôt.</p>
  <div id="games"></div>
  <div id="retired"></div>
  <div id="errors"></div>
</div>
<script>
const TOKEN = {token};
const DATA = {data};
const H = {{ 'x-slotlab-token': TOKEN, 'Content-Type': 'application/json' }};
const esc = s => String(s).replace(/[&<>"]/g, c => ({{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}}[c]));
let sessions = {{}};

function render() {{
  const g = DATA.games || [];
  document.getElementById('count').textContent =
    g.length ? g.length + (g.length > 1 ? ' jeux en ligne' : ' jeu en ligne') : '';
  document.getElementById('games').innerHTML = g.length ? g.map(card).join('') :
    '<div class="card empty">Aucun jeu publié. Utilise l\'onglet Deploy de SlotLab.</div>';
  const errs = DATA.errors || [];
  document.getElementById('errors').innerHTML = errs.length
    ? '<div class="card"><h2>Non servis</h2>' + errs.map(e =>
        `<p class="err">${{esc(e[0])}} — ${{esc(e[1])}}</p>`).join('') + '</div>' : '';
  const ret = DATA.retired || [];
  document.getElementById('retired').innerHTML = ret.length
    ? '<div class="card"><h2>Retirés</h2><p class="dim">Hors ligne, données conservées sur le disque&nbsp;: '
      + 'republier depuis l\'onglet Deploy les remet en ligne sans rien renvoyer.</p>'
      + ret.map(n => `<div class="row" style="margin-top:8px"><code>${{esc(n)}}</code>
          <span class="spacer"></span>
          <button class="danger" onclick="purge('${{esc(n)}}')">Supprimer les données</button></div>`).join('')
      + '</div>' : '';
}}

function card(x) {{
  const list = (sessions[x.name] || []);
  const live = list.filter(s => s.inRound).length;
  const rows = list.length ? list.map(s => `<tr>
      <td>
        <strong>${{esc(s.device || 'inconnu')}}</strong>${{s.country ? ' <span class="pill idle">' + esc(s.country) + '</span>' : ''}}
        ${{s.inRound ? '<span class="pill live">en spin</span>' : ''}}
        <div class="sub">${{esc(s.ip || 'IP inconnue')}} · <code>${{esc(s.sessionID.replace(/^share-/, '').slice(0, 6))}}</code></div>
      </td>
      <td class="num">${{ago(s.idleSecs)}}</td>
      <td class="num">${{s.spins}}</td>
      <td class="num">${{money(s.wagered, s.currency)}}</td>
      <td class="num">${{money(s.won, s.currency)}}</td>
      <td class="num">${{money(s.balance, s.currency)}}</td>
      <td class="num"><button onclick="dropSession('${{esc(s.sessionID)}}')">×</button></td>
    </tr>`).join('')
    : '<tr><td colspan="7" class="empty">Personne n\'a encore ouvert le lien.</td></tr>';
  return `<div class="card">
    <div class="row">
      <h2>${{esc(x.name)}}</h2>
      <span class="pill ${{live ? 'live' : 'idle'}}">${{list.length}} visiteur(s)</span>
      <span class="spacer"></span>
      <button onclick="setCode('${{esc(x.name)}}')">Changer le code</button>
      <button onclick="clearSessions('${{esc(x.name)}}')">Vider les visiteurs</button>
      <button onclick="toggleConsole('${{esc(x.name)}}')">${{x.console ? 'Désactiver' : 'Activer'}} la console</button>
      <button class="danger" onclick="unpublish('${{esc(x.name)}}')">Retirer</button>
    </div>
    <p><a href="${{esc(x.url)}}" target="_blank" rel="noreferrer">${{esc(x.url)}}</a></p>
    <p class="dim">Code&nbsp;: ${{x.accessCode
        ? '<code>' + esc(x.accessCode) + '</code>'
        : '<span class="err">aucun — lien ouvert à tous</span>'}}
      &nbsp;·&nbsp; Solde ${{x.balance.toLocaleString('fr-FR')}} ${{esc(x.currency)}}
      &nbsp;·&nbsp; ${{esc(x.language)}}${{x.social ? ' · social' : ''}}${{x.console ? ' · <b>console de démo active</b>' : ''}}</p>
    <table><thead><tr><th>Visiteur</th><th class="num">Vu</th><th class="num">Spins</th>
      <th class="num">Misé</th><th class="num">Gagné</th><th class="num">Solde</th><th></th>
    </tr></thead><tbody>${{rows}}</tbody></table>
  </div>`;
}}

const money = (micro, cur) =>
  (micro / 1e6).toLocaleString('fr-FR', {{ maximumFractionDigits: 2 }}) + ' ' + cur;

const ago = s => s < 45 ? 'à l\'instant'
  : s < 3600 ? Math.round(s / 60) + ' min'
  : Math.round(s / 3600) + ' h';

async function refresh() {{
  try {{
    const r = await fetch('/api/hub/sessions', {{ headers: H }}).then(r => r.json());
    sessions = {{}};
    (r.games || []).forEach(g => sessions[g.game] = g.sessions);
    render();
  }} catch (e) {{ /* le serveur redémarre : on retentera au prochain tick */ }}
}}

async function reload() {{
  const r = await fetch('/api/hub/reload', {{ method: 'POST', headers: H }}).then(r => r.json());
  if (r.error) return alert(r.error);
  location.reload();
}}

async function setCode(name) {{
  const g = DATA.games.find(x => x.name === name);
  const code = prompt('Nouveau code pour ' + name + " (vide = lien ouvert à tous) :", g.accessCode);
  if (code === null) return;
  const body = {{ ...g, accessCode: code.trim() }};
  delete body.url;
  const r = await fetch('/api/hub/publish', {{ method: 'POST', headers: H,
    body: JSON.stringify(body) }}).then(r => r.json());
  if (r.error) return alert(r.error);
  location.reload();
}}

async function toggleConsole(name) {{
  const g = DATA.games.find(x => x.name === name);
  const body = {{ ...g, console: !g.console }};
  delete body.url;
  const r = await fetch('/api/hub/publish', {{ method: 'POST', headers: H,
    body: JSON.stringify(body) }}).then(r => r.json());
  if (r.error) return alert(r.error);
  location.reload();
}}

async function clearSessions(name) {{
  if (!confirm('Vider la liste des visiteurs de ' + name + " ? Ceux qui jouent encore repartiront d'une session neuve.")) return;
  const r = await fetch('/api/hub/sessions/clear', {{ method: 'POST', headers: H,
    body: JSON.stringify({{ game: name }}) }}).then(r => r.json());
  if (r.error) return alert(r.error);
  refresh();
}}

async function dropSession(sessionID) {{
  await fetch('/api/hub/sessions/remove', {{ method: 'POST', headers: H,
    body: JSON.stringify({{ sessionID }}) }});
  refresh();
}}

async function unpublish(name) {{
  if (!confirm('Retirer ' + name + " ? Le lien cesse de répondre. Les données restent sur le disque.")) return;
  const r = await fetch('/api/hub/unpublish', {{ method: 'POST', headers: H,
    body: JSON.stringify({{ name }}) }}).then(r => r.json());
  if (r.error) return alert(r.error);
  location.reload();
}}

async function purge(name) {{
  if (!confirm('Supprimer définitivement les données de ' + name + " (books, LUT, build) ? Republier demandera de tout renvoyer.")) return;
  const r = await fetch('/api/hub/unpublish', {{ method: 'POST', headers: H,
    body: JSON.stringify({{ name, purge: true }}) }}).then(r => r.json());
  if (r.error) return alert(r.error);
  location.reload();
}}

render();
refresh();
setInterval(refresh, 5000);
</script></body></html>"##,
        token = serde_json::to_string(token).unwrap_or_else(|_| "\"\"".into()),
        data = data,
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClearReq {
    /// None → toutes les sessions de tous les jeux.
    #[serde(default)]
    game: Option<String>,
}

async fn api_sessions_clear(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ClearReq>,
) -> Response {
    if let Err(r) = authorized(&state, &headers) {
        return r;
    }
    let n = state.sessions.clear_shared(req.game.as_deref());
    Json(json!({ "ok": true, "removed": n })).into_response()
}

#[derive(Deserialize)]
struct RemoveReq {
    #[serde(rename = "sessionID")]
    session_id: String,
}

async fn api_sessions_remove(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<RemoveReq>,
) -> Response {
    if let Err(r) = authorized(&state, &headers) {
        return r;
    }
    Json(json!({ "ok": state.sessions.remove(&req.session_id) })).into_response()
}
