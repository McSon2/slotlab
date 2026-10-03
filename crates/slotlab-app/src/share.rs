use crate::state::AppState;
use anyhow::{Context, Result};
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};

/// State of the current share (one at a time).
#[derive(Default, Clone)]
pub struct ShareManager {
    inner: Arc<Mutex<Option<ShareState>>>,
}

struct ShareState {
    game: String,
    url: String,
    play_url: String,
    config: ShareConfig,
    tunnel: tokio::process::Child,
    server: tokio::task::JoinHandle<()>,
}

/// Settings handed to every visitor of the public link.
#[derive(Clone)]
pub struct ShareConfig {
    pub game: String,
    pub balance_micro: u64,
    pub currency: String,
    pub language: String,
    pub social: bool,
    /// Hostname to put in the visitor's `rgs_url`. None → the running quick
    /// tunnel, then the request's Host header. Set it when a reverse proxy
    /// or a named tunnel fronts the server.
    pub public_host: Option<String>,
}

impl ShareManager {
    /// Public hostname of the running tunnel, without the scheme.
    fn public_host(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.url.trim_start_matches("https://").to_string())
    }

    fn config(&self) -> Option<ShareConfig> {
        self.inner.lock().unwrap().as_ref().map(|s| s.config.clone())
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/ui/share/start", post(start))
        .route("/api/ui/share/stop", post(stop))
        .route("/api/ui/share/status", get(status))
        .route("/api/ui/share/sessions", get(sessions))
        .route("/api/ui/share/sessions/reset", post(reset_session))
}

/// Local port of the share server. Overridable with SLOTLAB_SHARE_PORT so a
/// second instance can share without colliding with a running one.
fn share_port() -> u16 {
    std::env::var("SLOTLAB_SHARE_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3602)
}
/// Default balance handed to a visitor: $1,000 in micro-units.
pub const DEFAULT_SHARE_BALANCE: u64 = 1_000 * 1_000_000;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartReq {
    game: String,
    /// Static build directory. Empty → game's auto-detected build.
    #[serde(default)]
    build_dir: String,
    /// Starting balance of each visitor, in micro-units (1,000,000 = $1).
    #[serde(default)]
    balance: Option<u64>,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    social: Option<bool>,
}

fn find_cloudflared() -> Option<PathBuf> {
    for p in [
        "/opt/homebrew/bin/cloudflared",
        "/usr/local/bin/cloudflared",
        "/usr/bin/cloudflared",
    ] {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    which_in_path("cloudflared")
}

fn which_in_path(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(bin))
            .find(|p| p.exists())
    })
}

async fn start(
    State(state): State<AppState>,
    Json(req): Json<StartReq>,
) -> Json<serde_json::Value> {
    match start_impl(&state, req).await {
        Ok(v) => Json(v),
        Err(e) => Json(json!({ "error": format!("{e:#}") })),
    }
}

async fn start_impl(state: &AppState, req: StartReq) -> Result<serde_json::Value> {
    // One share at a time: stop the existing one.
    stop_current(state);

    let game = state
        .registry
        .get(&req.game)
        .context("unknown game — register it first in Games")?;
    // Books required (the share's RGS reads the same caches).
    anyhow::ensure!(
        state.books.is_imported(&game),
        "books not imported for {} — Games/Play tab → Import books",
        req.game
    );
    let build_dir = if req.build_dir.trim().is_empty() {
        game.entry.build_dir.clone().context(
            "no build detected for this game — build the frontend \
             (pnpm --filter <game> build) then Reload in Games",
        )?
    } else {
        PathBuf::from(req.build_dir.trim())
    };
    anyhow::ensure!(
        build_dir.join("index.html").is_file(),
        "index.html not found in {} — build the game first \
         (pnpm --filter <game> build)",
        build_dir.display()
    );
    let cloudflared = find_cloudflared()
        .context("cloudflared not found — install it: brew install cloudflared")?;

    let config = ShareConfig {
        game: req.game.clone(),
        balance_micro: req.balance.unwrap_or(DEFAULT_SHARE_BALANCE).max(1),
        currency: req.currency.unwrap_or_else(|| "USD".into()),
        language: req.language.unwrap_or_else(|| "en".into()),
        social: req.social.unwrap_or(false),
        public_host: None,
    };

    // Server :3602 — game statics + RGS contract (HTTP, the tunnel
    // provides the public TLS). The SlotLab UI is NOT exposed.
    // `/play` is the entry point handed out: it mints one session per
    // visitor before redirecting into the game.
    let share_app = Router::new()
        .route("/play", get(landing))
        .merge(crate::rgs::routes::router())
        .fallback_service(tower_http::services::ServeDir::new(build_dir))
        .layer(Extension(config.clone()))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state.clone());
    let port = share_port();
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("port {port} in use"))?;
    let server = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, share_app).await {
            tracing::error!("share server: {e}");
        }
    });

    // Cloudflared quick tunnel (free, no account).
    let mut child = tokio::process::Command::new(&cloudflared)
        .args([
            "tunnel",
            "--url",
            &format!("http://127.0.0.1:{port}"),
            "--no-autoupdate",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("launching cloudflared")?;

    // The *.trycloudflare.com URL appears on stderr in ~2-5 s.
    // WARNING: keep the pipes DRAINED afterwards: closing them would kill
    // cloudflared (EPIPE on the next log line).
    let stderr = child.stderr.take().context("cloudflared stderr")?;
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(_)) = lines.next_line().await {}
        });
    }
    let mut lines = BufReader::new(stderr).lines();
    let url = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(pos) = line.find("https://") {
                let rest = &line[pos..];
                if let Some(end) = rest.find(".trycloudflare.com") {
                    let url = &rest[..end + ".trycloudflare.com".len()];
                    return Some(url.to_string());
                }
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
    .context("cloudflared did not return a trycloudflare.com URL within 30 s")?;
    // Keep draining stderr in the background.
    tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });

    // The shared link points at /play, NOT at a baked sessionID: every
    // opening gets its own session (own balance, own active round).
    let play_url = format!("{url}/play");

    let mut inner = state.share.inner.lock().unwrap();
    *inner = Some(ShareState {
        game: req.game.clone(),
        url: url.clone(),
        play_url: play_url.clone(),
        config,
        tunnel: child,
        server,
    });
    tracing::info!("share active: {play_url}");
    Ok(json!({ "ok": true, "url": url, "playUrl": play_url, "game": req.game }))
}

/// GET /play — hands the visitor their session, then redirects into the
/// game with it.
///
/// A returning visitor gets the SAME session back (cookie), otherwise every
/// reload of this URL would mint one more and the dashboard would fill with
/// ghosts of a single person.
pub async fn landing(
    State(state): State<AppState>,
    Extension(cfg): Extension<ShareConfig>,
    gate: Option<Extension<Arc<crate::gate::Gate>>>,
    key: Option<Extension<crate::gate::PathKey>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    // Host of the public tunnel; the Host header is the fallback when the
    // share manager has already been swapped out.
    let host = cfg
        .public_host
        .clone()
        .or_else(|| state.share.public_host())
        .unwrap_or_else(|| {
            headers
                .get(axum::http::header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("127.0.0.1")
                .to_string()
        });

    // The link may be opened on a phone: pick the layout from the UA
    // unless the visitor forced it with ?device=.
    let ua = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let ua_low = ua.to_ascii_lowercase();
    let device = q.get("device").cloned().unwrap_or_else(|| {
        if ua_low.contains("mobi") || ua_low.contains("android") || ua_low.contains("iphone") {
            "mobile".into()
        } else {
            "desktop".into()
        }
    });

    // Rien n'est demandé au visiteur : on ne garde que ce que Cloudflare
    // transmet déjà, ce qui suffit à distinguer deux personnes.
    let who = crate::rgs::session::Identity {
        ip: header_str(&headers, "cf-connecting-ip"),
        country: header_str(&headers, "cf-ipcountry"),
        device: format!("{device} · {}", browser_of(&ua_low)),
    };

    // Same browser, same session: the id is remembered in a cookie and
    // reused as long as it is still alive server-side.
    // Dans une iframe le cookie ne revient pas : la page qui l'affiche
    // rappelle alors la session par `?sid=`.
    let existing = cookie(&headers, "slotlab_sid")
        .filter(|sid| state.sessions.is_live_shared(sid, &cfg.game))
        .or_else(|| {
            q.get("sid")
                .filter(|sid| state.sessions.is_live_shared(sid, &cfg.game))
                .cloned()
        });
    let session_id = match existing {
        Some(sid) => {
            state.sessions.touch_shared(&sid, &who);
            sid
        }
        None => {
            let sid = format!("share-{:032x}", rand::random::<u128>());
            state.sessions.create_shared(
                &sid,
                &cfg.game,
                cfg.balance_micro,
                &cfg.currency,
                q.get("lang").map(String::as_str).unwrap_or(&cfg.language),
                cfg.social,
                who.clone(),
            );
            tracing::info!("share visitor: {sid} ({}, {})", who.device, who.ip);
            sid
        }
    };

    let lang = q.get("lang").cloned().unwrap_or_else(|| cfg.language.clone());
    let currency = q
        .get("currency")
        .cloned()
        .unwrap_or_else(|| cfg.currency.clone());
    let secure = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|p| p.eq_ignore_ascii_case("https"));
    let sid_cookie = format!(
        "slotlab_sid={session_id}; Path=/; Max-Age=21600; HttpOnly; SameSite=Lax{}",
        if secure { "; Secure" } else { "" }
    );
    // Entré par une clé dans l'adresse (`/k/<code>/play`) : tout ce que la
    // page charge ensuite la reporte, sous forme de jeton — le code en clair
    // ne reste pas dans l'historique de l'iframe.
    let prefix = match (gate, key) {
        (Some(Extension(g)), Some(_)) if !g.is_open() => format!("/k/{}", g.token()),
        _ => String::new(),
    };
    // `embed=1` : la page est dans l'onglet Play de Reelhouse, qui tient sa
    // propre console à côté du jeu.
    let embed = if q.get("embed").is_some_and(|v| v == "1") { "&embed=1" } else { "" };
    (
        [(axum::http::header::SET_COOKIE, sid_cookie)],
        Redirect::to(&format!(
            "{prefix}/?sessionID={session_id}&rgs_url={host}{prefix}/api/rgs/{game}&lang={lang}\
             &currency={currency}&device={device}&social={social}&demo=true{embed}",
            game = cfg.game,
            social = cfg.social,
        )),
    )
        .into_response()
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())?
        .split(';')
        .find_map(|c| c.trim().strip_prefix(&format!("{name}=")))
        .map(str::to_string)
}

fn header_str(headers: &HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

/// Coarse browser name — enough to tell two anonymous visitors apart.
fn browser_of(ua: &str) -> &'static str {
    if ua.contains("edg/") {
        "Edge"
    } else if ua.contains("chrome") && !ua.contains("chromium") {
        "Chrome"
    } else if ua.contains("firefox") {
        "Firefox"
    } else if ua.contains("safari") {
        "Safari"
    } else if ua.contains("curl") {
        "curl"
    } else {
        "?"
    }
}

fn stop_current(state: &AppState) {
    let mut inner = state.share.inner.lock().unwrap();
    if let Some(mut s) = inner.take() {
        let _ = s.tunnel.start_kill();
        s.server.abort();
        tracing::info!("share stopped ({})", s.game);
    }
}

async fn stop(State(state): State<AppState>) -> Json<serde_json::Value> {
    stop_current(&state);
    Json(json!({ "ok": true }))
}

async fn status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let inner = state.share.inner.lock().unwrap();
    match &*inner {
        Some(s) => Json(json!({
            "active": true,
            "game": s.game,
            "url": s.url,
            "playUrl": s.play_url,
            "balance": s.config.balance_micro,
            "currency": s.config.currency,
            "language": s.config.language,
            "social": s.config.social,
        })),
        None => Json(json!({ "active": false })),
    }
}

/// GET /api/ui/share/sessions — who is playing behind the public link.
async fn sessions(State(state): State<AppState>) -> Json<serde_json::Value> {
    let Some(cfg) = state.share.config() else {
        return Json(json!({ "active": false, "sessions": [] }));
    };
    let list: Vec<_> = state
        .sessions
        .list_shared(&cfg.game)
        .into_iter()
        .map(|(id, s)| {
            json!({
                "sessionID": id,
                "balance": s.balance_micro,
                "currency": s.currency,
                "spins": s.round_counter,
                "wagered": s.wagered_micro,
                "won": s.won_micro,
                "inRound": s.round.as_ref().is_some_and(|r| r.active),
            })
        })
        .collect();
    Json(json!({ "active": true, "game": cfg.game, "sessions": list }))
}

#[derive(Deserialize)]
struct ResetReq {
    /// `sessionID`, not `sessionId`: the same spelling as the RGS contract
    /// and as the list this endpoint is fed from.
    #[serde(rename = "sessionID")]
    session_id: String,
}

/// POST /api/ui/share/sessions/reset — back to the starting balance.
async fn reset_session(
    State(state): State<AppState>,
    Json(req): Json<ResetReq>,
) -> Json<serde_json::Value> {
    let balance = state
        .share
        .config()
        .map(|c| c.balance_micro)
        .unwrap_or(DEFAULT_SHARE_BALANCE);
    let ok = state.sessions.reset_one(&req.session_id, balance);
    Json(json!({ "ok": ok }))
}
