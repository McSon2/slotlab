use crate::state::AppState;
use anyhow::{Context, Result};
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
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
    tunnel: tokio::process::Child,
    server: tokio::task::JoinHandle<()>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/ui/share/start", post(start))
        .route("/api/ui/share/stop", post(stop))
        .route("/api/ui/share/status", get(status))
}

const SHARE_PORT: u16 = 3602;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartReq {
    game: String,
    /// Static build directory. Empty → game's auto-detected build.
    #[serde(default)]
    build_dir: String,
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

async fn start_impl(
    state: &AppState,
    req: StartReq,
) -> Result<serde_json::Value> {
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
    let cloudflared = find_cloudflared().context(
        "cloudflared not found — install it: brew install cloudflared",
    )?;

    // Server :3602 — game statics + RGS contract (HTTP, the tunnel
    // provides the public TLS). The SlotLab UI is NOT exposed.
    let share_app = Router::new()
        .merge(crate::rgs::routes::router())
        .fallback_service(tower_http::services::ServeDir::new(build_dir))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state.clone());
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], SHARE_PORT));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("port 3602 in use")?;
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
            &format!("http://127.0.0.1:{SHARE_PORT}"),
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
    tokio::spawn(async move {
        while let Ok(Some(_)) = lines.next_line().await {}
    });

    let host = url.trim_start_matches("https://");
    let session = format!("share-{}", rand::random::<u32>());
    let play_url = format!(
        "{url}/?sessionID={session}&rgs_url={host}/api/rgs/{game}&lang=en&currency=USD&device=desktop&social=false&demo=true",
        game = req.game,
    );

    let mut inner = state.share.inner.lock().unwrap();
    *inner = Some(ShareState {
        game: req.game.clone(),
        url: url.clone(),
        play_url: play_url.clone(),
        tunnel: child,
        server,
    });
    tracing::info!("share active: {play_url}");
    Ok(json!({ "ok": true, "url": url, "playUrl": play_url, "game": req.game }))
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
        })),
        None => Json(json!({ "active": false })),
    }
}
