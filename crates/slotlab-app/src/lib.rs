//! SlotLab server — reusable as a lib (CLI binary + Tauri desktop app).

mod api;
mod books;
mod fs;
mod optimize;
mod persist;
mod registry;
mod rgs;
mod search;
mod share;
mod state;
mod tls;

use axum::Router;
use state::AppState;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};

/// Listening ports for the SlotLab server.
#[derive(Clone, Copy, Debug)]
pub struct ServerOptions {
    /// UI + API (HTTP).
    pub port: u16,
    /// Emulated RGS (HTTPS — the games hardcode `https://`).
    pub rgs_port: u16,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self { port: 3600, rgs_port: 3601 }
    }
}

/// Starts both listeners (UI :3600, RGS :3601) and the books auto-import,
/// then serves until the process stops. Returns an error if a port is
/// already taken.
pub async fn run_server(opts: ServerOptions) -> anyhow::Result<()> {
    let (registry, errors) = registry::Registry::from_disk();
    for (game, err) in &errors {
        tracing::warn!("game `{game}` not loaded: {err}");
    }
    let (events_tx, _) = tokio::sync::broadcast::channel(256);
    let state = AppState {
        registry: Arc::new(registry),
        load_errors: Arc::new(errors.into_iter().collect()),
        sessions: Arc::new(rgs::session::Sessions::default()),
        books: Arc::new(books::BooksManager::default()),
        events_tx,
        share: share::ShareManager::default(),
    };

    // RGS HTTPS listener :3601 — localhost cert signed by the SlotLab CA.
    let material = tls::ensure_material()?;
    let rustls_config = axum_server::tls_rustls::RustlsConfig::from_pem(
        material.leaf_cert_pem.clone().into_bytes(),
        material.leaf_key_pem.clone().into_bytes(),
    )
    .await?;
    let rgs_app = Router::new()
        .merge(rgs::routes::router())
        .layer(CorsLayer::permissive())
        .with_state(state.clone());
    let rgs_addr = std::net::SocketAddr::from(([127, 0, 0, 1], opts.rgs_port));
    // Synchronous bind BEFORE spawning: a taken port = immediate error
    // propagated to the caller (the desktop app relies on this).
    let rgs_listener = std::net::TcpListener::bind(rgs_addr)?;
    tokio::spawn(async move {
        tracing::info!("emulated RGS: https://localhost:{}/api/rgs/<game>", rgs_addr.port());
        if let Err(e) = axum_server::from_tcp_rustls(rgs_listener, rustls_config)
            .serve(rgs_app.into_make_service())
            .await
        {
            tracing::error!("RGS listener: {e}");
        }
    });
    if !tls::is_trusted() {
        tracing::warn!(
            "SlotLab CA not installed in the trust store — games will show \
             a TLS error. UI → \"Install CA\" banner (or POST /api/ui/tls/install)."
        );
    }

    // Background auto-import at startup: all games whose books cache is
    // missing or stale (fingerprint), one at a time.
    {
        let state = state.clone();
        tokio::spawn(async move {
            for entry in state.registry.list_entries() {
                let Some(game) = state.registry.get(&entry.name) else {
                    continue;
                };
                if state.books.is_imported(&game) {
                    continue;
                }
                tracing::info!("books auto-import: {}", entry.name);
                state.books.start_import(game);
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(700))
                        .await;
                    match state
                        .books
                        .import_status(&entry.name)
                        .map(|s| s.state)
                    {
                        Some(books::ImportState::Running) => continue,
                        _ => break,
                    }
                }
            }
        });
    }

    // UI API CORS: restricted to SlotLab's local origins (the embedded
    // UI :3600 and the Vite dev server :5173). Prevents a third-party site
    // open in the browser from driving the tool (register/remove/share).
    let ui_cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            origin
                .to_str()
                .map(|o| {
                    o.starts_with("http://localhost:")
                        || o.starts_with("http://127.0.0.1:")
                })
                .unwrap_or(false)
        }))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let app = Router::new()
        .merge(api::routes::router())
        .merge(search::router())
        .merge(share::router())
        .merge(fs::router())
        .merge(optimize::router())
        .merge(games_router())
        .merge(ui_router())
        .layer(ui_cors)
        .with_state(state);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], opts.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("SlotLab UI: http://localhost:{}", opts.port);
    axum::serve(listener, app).await?;
    Ok(())
}

/// Serves the games' static builds under /games/<name>/ (auto-detected).
/// The games' SvelteKit builds use RELATIVE paths (./assets/…)
/// → the trailing slash is mandatory, hence the redirect.
fn games_router() -> Router<AppState> {
    use axum::extract::{Path as AxPath, State};
    use axum::http::{header, StatusCode};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::get;

    async fn serve_file(
        state: &AppState,
        name: &str,
        rel: &str,
    ) -> Response {
        let Some(game) = state.registry.get(name) else {
            return (StatusCode::NOT_FOUND, "unknown game").into_response();
        };
        let Some(build) = game.entry.build_dir.clone() else {
            return (
                StatusCode::NOT_FOUND,
                "no build detected for this game — build the frontend \
                 (pnpm --filter <game> build) then Reload",
            )
                .into_response();
        };
        let rel = rel.trim_start_matches('/');
        if rel.split('/').any(|c| c == "..") {
            return StatusCode::FORBIDDEN.into_response();
        }
        let mut path = build.join(rel);
        if rel.is_empty() || !path.is_file() {
            path = build.join("index.html");
        }
        match tokio::fs::read(&path).await {
            Ok(bytes) => {
                let mime = mime_guess::from_path(&path).first_or_octet_stream();
                (
                    [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                    bytes,
                )
                    .into_response()
            }
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        }
    }

    Router::new()
        .route(
            "/games/{name}",
            get(|AxPath(name): AxPath<String>| async move {
                Redirect::permanent(&format!("/games/{name}/"))
            }),
        )
        .route(
            "/games/{name}/",
            get(
                |State(state): State<AppState>, AxPath(name): AxPath<String>| async move {
                    serve_file(&state, &name, "").await
                },
            ),
        )
        .route(
            "/games/{name}/{*path}",
            get(
                |State(state): State<AppState>,
                 AxPath((name, path)): AxPath<(String, String)>| async move {
                    serve_file(&state, &name, &path).await
                },
            ),
        )
}

/// Embedded UI (release, embed-ui feature) or a pointer message (dev).
#[cfg(feature = "embed-ui")]
fn ui_router() -> Router<AppState> {
    use axum::http::{header, StatusCode, Uri};
    use axum::response::IntoResponse;
    use axum::routing::get;

    #[derive(rust_embed::RustEmbed)]
    #[folder = "../../frontend/dist"]
    struct UiAssets;

    async fn serve(uri: Uri) -> impl IntoResponse {
        let path = uri.path().trim_start_matches('/');
        let path = if path.is_empty() { "index.html" } else { path };
        let file = UiAssets::get(path).or_else(|| UiAssets::get("index.html"));
        match file {
            Some(f) => {
                let mime = mime_guess::from_path(path).first_or_octet_stream();
                (
                    [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                    f.data.into_owned(),
                )
                    .into_response()
            }
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }

    Router::new().fallback(get(serve))
}

#[cfg(not(feature = "embed-ui"))]
fn ui_router() -> Router<AppState> {
    use axum::routing::get;
    Router::new().route(
        "/",
        get(|| async {
            "SlotLab backend (dev). UI: run `pnpm dev` in slotlab/frontend \
             then open http://localhost:5173"
        }),
    )
}
