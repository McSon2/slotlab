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

/// State of the Share tab's share (one at a time), plus the shares pinned to
/// their own port.
#[derive(Default, Clone)]
pub struct ShareManager {
    inner: Arc<Mutex<Option<ShareState>>>,
    /// Shares started with their own `port` (≠ the Share tab's): each is
    /// fronted by a stable hostname (`publicHost`, a named tunnel) and lives
    /// beside the tab's share — starting or stopping one never touches the
    /// other. Keyed by port.
    pinned: Arc<Mutex<HashMap<u16, ShareState>>>,
}

struct ShareState {
    game: String,
    url: String,
    play_url: String,
    config: ShareConfig,
    port: u16,
    build_dir: PathBuf,
    /// Quick tunnel, when one was asked for (`tunnel: false` serves the
    /// share through `publicHost` only).
    tunnel: Option<tokio::process::Child>,
    server: tokio::task::JoinHandle<()>,
}

impl ShareState {
    fn stop(mut self) {
        if let Some(t) = self.tunnel.as_mut() {
            let _ = t.start_kill();
        }
        self.server.abort();
        tracing::info!("share stopped ({} on :{})", self.game, self.port);
    }

    fn status(&self) -> serde_json::Value {
        json!({
            "active": true,
            "game": self.game,
            "url": self.url,
            "playUrl": self.play_url,
            "port": self.port,
            "buildDir": self.build_dir,
            "tunnel": self.tunnel.is_some(),
            "publicHost": self.config.public_host,
            "balance": self.config.balance_micro,
            "currency": self.config.currency,
            "language": self.config.language,
            "social": self.config.social,
        })
    }
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
    /// Stable hostname fronting the share server (a named tunnel such as
    /// `mg-rush.mcberry.fr`, or a reverse proxy). Put in every visitor's
    /// `rgs_url` instead of the quick tunnel's host.
    #[serde(default)]
    public_host: Option<String>,
    /// `false` → no quick tunnel: the server is only reached through
    /// `publicHost` (required then). Default: a tunnel, except for a pinned
    /// share.
    #[serde(default)]
    tunnel: Option<bool>,
    /// Local port of the share server. Absent (or the Share tab's port) →
    /// the tab's share, which replaces the previous one. Any other port → a
    /// PINNED share living beside it (requires `publicHost`): a stable public
    /// name must never serve whatever the tab happens to share.
    #[serde(default)]
    port: Option<u16>,
    /// Demo console injected in the page (force a result). Default: on.
    #[serde(default)]
    console: Option<bool>,
}

/// `mg-rush.mcberry.fr` from `https://mg-rush.mcberry.fr/`, None when empty.
/// Refuses anything that is not a bare host (it lands in a URL).
fn normalize_host(raw: Option<&str>) -> Result<Option<String>> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let host = raw
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    anyhow::ensure!(
        !host.is_empty()
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':')),
        "publicHost must be a bare hostname (got {raw:?})"
    );
    Ok(Some(host.to_ascii_lowercase()))
}

/// The share's RGS answers for the shared game only: a visitor must not be
/// able to play another registered game by hitting its RGS path directly
/// (same rule as the hub's `resolve_mw`).
async fn only_shared_game(
    Extension(cfg): Extension<ShareConfig>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if let Some(rest) = req.uri().path().strip_prefix("/api/rgs/") {
        let asked = rest.split('/').next().unwrap_or("");
        if !asked.eq_ignore_ascii_case(&cfg.game) {
            return axum::http::StatusCode::NOT_FOUND.into_response();
        }
    }
    next.run(req).await
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
    let port = req.port.unwrap_or_else(share_port);
    // Pinned share: its own port, beside the Share tab's one.
    let pinned = port != share_port();
    let public_host = normalize_host(req.public_host.as_deref())?;
    let want_tunnel = req.tunnel.unwrap_or(!pinned);
    anyhow::ensure!(
        !pinned || public_host.is_some(),
        "a share on its own port (:{port}) needs publicHost — the stable name that fronts it"
    );
    anyhow::ensure!(
        want_tunnel || public_host.is_some(),
        "tunnel: false needs publicHost — without a quick tunnel nothing else reaches the share"
    );
    // One share per port: stop the one this start replaces.
    if pinned {
        stop_pinned(state, port);
    } else {
        stop_current(state);
    }

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
    let cloudflared = if want_tunnel {
        Some(
            find_cloudflared()
                .context("cloudflared not found — install it: brew install cloudflared")?,
        )
    } else {
        None
    };

    let config = ShareConfig {
        game: req.game.clone(),
        balance_micro: req.balance.unwrap_or(DEFAULT_SHARE_BALANCE).max(1),
        currency: req.currency.unwrap_or_else(|| "USD".into()),
        language: req.language.unwrap_or_else(|| "en".into()),
        social: req.social.unwrap_or(false),
        public_host: public_host.clone(),
    };

    // Server :3602 — game statics + RGS contract (HTTP, the tunnel
    // provides the public TLS). The SlotLab UI is NOT exposed.
    // `/play` is the entry point handed out: it mints one session per
    // visitor before redirecting into the game.
    // Served like a hub game with its console on: Reelhouse's Play tab finds and
    // forces books through /api/console/*, and its `/k/<code>/…` links open (the
    // share has no access code, the key is only dropped).
    let game_entry = Arc::new(crate::hub::HubGame {
        manifest: crate::hub::Manifest {
            name: req.game.clone(),
            host: String::new(),
            balance: config.balance_micro / 1_000_000,
            currency: config.currency.clone(),
            language: config.language.clone(),
            social: config.social,
            access_code: String::new(),
            console: req.console.unwrap_or(true),
            published_at: String::new(),
        },
        build_dir: build_dir.clone(),
        gate: Arc::new(crate::gate::Gate::open()),
        config: config.clone(),
    });
    // ⚠️ Layer order: the extensions are added OUTSIDE `only_shared_game`,
    // which reads the config.
    let share_app = Router::new()
        .route("/play", get(landing))
        .merge(crate::rgs::routes::router())
        .merge(crate::console::router())
        .fallback(crate::hub::serve_static)
        .layer(axum::middleware::from_fn(only_shared_game))
        .layer(Extension(game_entry))
        .layer(Extension(config.clone()))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state.clone());
    let share_app: Router = Router::new()
        .fallback_service(share_app)
        .layer(axum::middleware::from_fn(crate::gate::strip_key));
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("port {port} in use"))?;
    let server = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, share_app).await {
            tracing::error!("share server: {e}");
        }
    });

    let (tunnel, url) = match cloudflared {
        Some(cloudflared) => {
            let (child, url) = match quick_tunnel(&cloudflared, port).await {
                Ok(t) => t,
                Err(e) => {
                    server.abort();
                    return Err(e);
                }
            };
            (Some(child), url)
        }
        // No quick tunnel: the stable name is the public address.
        None => (
            None,
            format!("https://{}", public_host.as_deref().unwrap_or_default()),
        ),
    };

    // The shared link points at /play, NOT at a baked sessionID: every
    // opening gets its own session (own balance, own active round).
    let play_url = format!("{url}/play");

    let share = ShareState {
        game: req.game.clone(),
        url: url.clone(),
        play_url: play_url.clone(),
        config,
        port,
        build_dir,
        tunnel,
        server,
    };
    if pinned {
        state.share.pinned.lock().unwrap().insert(port, share);
    } else {
        *state.share.inner.lock().unwrap() = Some(share);
    }
    tracing::info!(
        "share active: {play_url} (:{port}{})",
        if pinned { ", pinned" } else { "" }
    );
    Ok(json!({
        "ok": true,
        "url": url,
        "playUrl": play_url,
        "game": req.game,
        "port": port,
        "pinned": pinned,
    }))
}

/// Cloudflared quick tunnel (free, no account) to the share port. Returns
/// the child and its `https://*.trycloudflare.com` URL.
async fn quick_tunnel(
    cloudflared: &std::path::Path,
    port: u16,
) -> Result<(tokio::process::Child, String)> {
    let mut child = tokio::process::Command::new(cloudflared)
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
    Ok((child, url))
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
    let taken = state.share.inner.lock().unwrap().take();
    if let Some(s) = taken {
        s.stop();
    }
}

fn stop_pinned(state: &AppState, port: u16) -> bool {
    let taken = state.share.pinned.lock().unwrap().remove(&port);
    taken.map(ShareState::stop).is_some()
}

#[derive(Deserialize, Default)]
struct StopReq {
    /// Pinned share to stop. Absent → the Share tab's share.
    #[serde(default)]
    port: Option<u16>,
}

/// POST /api/ui/share/stop — the Share tab's share, or `{"port": N}` for a
/// pinned one. The body is optional (the tab sends none).
async fn stop(State(state): State<AppState>, body: axum::body::Bytes) -> Json<serde_json::Value> {
    let req: StopReq = serde_json::from_slice(&body).unwrap_or_default();
    match req.port.filter(|p| *p != share_port()) {
        Some(port) => Json(json!({ "ok": stop_pinned(&state, port), "port": port })),
        None => {
            stop_current(&state);
            Json(json!({ "ok": true }))
        }
    }
}

/// GET /api/ui/share/status — the Share tab's share (same fields as
/// before), plus `pinned`: the shares living on their own port.
async fn status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut out = match &*state.share.inner.lock().unwrap() {
        Some(s) => s.status(),
        None => json!({ "active": false }),
    };
    let mut pinned: Vec<_> = state
        .share
        .pinned
        .lock()
        .unwrap()
        .values()
        .map(ShareState::status)
        .collect();
    pinned.sort_by_key(|v| v["port"].as_u64());
    out["pinned"] = json!(pinned);
    Json(out)
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

#[cfg(test)]
mod tests {
    use super::normalize_host;

    #[test]
    fn public_host_is_a_bare_host() {
        let host = |raw: Option<&str>| normalize_host(raw).unwrap();
        assert_eq!(
            host(Some(" https://MG-Rush.mcberry.fr/ ")).as_deref(),
            Some("mg-rush.mcberry.fr")
        );
        assert_eq!(host(Some("127.0.0.1:3603")).as_deref(), Some("127.0.0.1:3603"));
        assert_eq!(host(Some("  ")), None);
        assert_eq!(host(None), None);
        // it lands in the visitor's URL: nothing but a host
        assert!(normalize_host(Some("evil.com/x?y")).is_err());
        assert!(normalize_host(Some("a b.com")).is_err());
        assert!(normalize_host(Some("https://")).is_err());
    }
}
