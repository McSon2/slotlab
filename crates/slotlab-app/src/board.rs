//! Entry board editor — the Board tab.
//!
//! Gathers what the page needs to edit a game's `INITIAL_BOARD` visually: the
//! board itself (parsed from `src/game/constants.ts`), the symbols and their
//! pays from the frontend config the math SDK exports (`config_fe_<game>.json`),
//! the reel strips (to know which symbol can land on which reel), and the
//! symbol images the frontend ships in `static/`. Writing replaces only the
//! declaration in the file — see `slotlab_core::board`.
//!
//! The check of what the board SHOWS (paying connections, placement) runs in
//! the page, live, while the user paints; this module only states the facts.

use crate::api::routes::ApiError;
use crate::registry::LoadedGame;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use slotlab_core::board::{self, Cell};
use std::collections::BTreeMap;
use std::path::{Path as FsPath, PathBuf};
use xxhash_rust::xxh3::xxh3_64;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/ui/games/{name}/board", get(get_board).put(put_board))
        .route("/api/ui/games/{name}/board/preview", post(preview_board))
        .route("/api/ui/games/{name}/board/asset/{*path}", get(board_asset))
}

#[derive(Serialize)]
struct Pay {
    min: u32,
    max: u32,
    pay: f64,
}

#[derive(Serialize)]
struct SymbolInfo {
    name: String,
    pays: Vec<Pay>,
    special: Vec<String>,
}

#[derive(Serialize)]
struct ConstInfo {
    name: String,
    value: Map<String, Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BoardInfo {
    game: String,
    constants_path: String,
    /// Hash of the declaration as read. Sent back on write: a board changed on
    /// disk meanwhile (by hand, by another tool) is not silently overwritten.
    base_hash: String,
    board: Vec<Vec<Map<String, Value>>>,
    consts: Vec<ConstInfo>,
    num_reels: usize,
    /// Visible rows per reel (the board has one padding row above and below).
    num_rows: Vec<usize>,
    symbols: Vec<SymbolInfo>,
    /// Lines games only — `paylines[i][reel]` = visible row.
    paylines: Option<Vec<Vec<usize>>>,
    /// Per reel, every symbol any reel strip carries there. None = no strips.
    strip_symbols: Option<Vec<Vec<String>>>,
    strip_files: Vec<String>,
    images: BTreeMap<String, String>,
    notes: Vec<String>,
}

fn game_of(state: &AppState, name: &str) -> Result<std::sync::Arc<LoadedGame>, ApiError> {
    state
        .registry
        .get(name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))
}

fn app_dir(game: &LoadedGame) -> Result<PathBuf, ApiError> {
    game.entry.app_dir.clone().ok_or_else(|| {
        ApiError::not_found(
            "no frontend app detected for this game (expected <repo>/frontend/apps/<game>)",
        )
    })
}

/// `src/game/constants.ts`, or the first source file declaring the board.
fn constants_path(app: &FsPath) -> Result<PathBuf, ApiError> {
    let default = app.join("src/game/constants.ts");
    if std::fs::read_to_string(&default).is_ok_and(|s| s.contains("INITIAL_BOARD")) {
        return Ok(default);
    }
    let mut files = Vec::new();
    walk(&app.join("src"), &mut files, 6);
    files
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "ts"))
        .find(|p| {
            std::fs::read_to_string(p)
                .is_ok_and(|s| s.contains("export const INITIAL_BOARD"))
        })
        .ok_or_else(|| ApiError::not_found("no `export const INITIAL_BOARD` in the frontend sources"))
}

fn walk(dir: &FsPath, out: &mut Vec<PathBuf>, depth: usize) {
    if depth == 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out, depth - 1),
            Ok(t) if t.is_file() => out.push(p),
            _ => {}
        }
    }
}

/// `<game>/library/configs/config_fe_*.json`, next to `publish_files`.
fn fe_config(game: &LoadedGame) -> Option<Value> {
    let configs = game.entry.publish_dir.parent()?.join("configs");
    let file = std::fs::read_dir(&configs)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("config_fe_") && n.ends_with(".json"))
        })?;
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

/// "3" → (3, 3), "8-9" → (8, 9).
fn kind_range(k: &str) -> Option<(u32, u32)> {
    match k.split_once('-') {
        Some((a, b)) => Some((a.trim().parse().ok()?, b.trim().parse().ok()?)),
        None => {
            let n = k.trim().parse().ok()?;
            Some((n, n))
        }
    }
}

fn symbols_of(fe: &Value) -> Vec<SymbolInfo> {
    let mut out = Vec::new();
    let Some(list) = fe.get("symbols").and_then(Value::as_array) else {
        return out;
    };
    for entry in list {
        let Some(obj) = entry.as_object() else {
            continue;
        };
        for (name, v) in obj {
            let mut pays = Vec::new();
            for p in v.get("paytable").and_then(Value::as_array).into_iter().flatten() {
                for (k, pay) in p.as_object().into_iter().flatten() {
                    if let (Some((min, max)), Some(pay)) = (kind_range(k), pay.as_f64()) {
                        pays.push(Pay { min, max, pay });
                    }
                }
            }
            pays.sort_by_key(|p| p.min);
            let special = v
                .get("special_properties")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect();
            out.push(SymbolInfo {
                name: name.clone(),
                pays,
                special,
            });
        }
    }
    out
}

/// Union, per reel, of the symbols every strip of `<game>/reels/*.csv` carries.
fn strips_of(game: &LoadedGame, num_reels: usize) -> (Option<Vec<Vec<String>>>, Vec<String>) {
    let Some(reels_dir) = game
        .entry
        .publish_dir
        .parent()
        .and_then(|l| l.parent())
        .map(|g| g.join("reels"))
    else {
        return (None, Vec::new());
    };
    let Ok(rd) = std::fs::read_dir(&reels_dir) else {
        return (None, Vec::new());
    };
    let mut per_reel: Vec<std::collections::BTreeSet<String>> = vec![Default::default(); num_reels];
    let mut files = Vec::new();
    for p in rd.flatten().map(|e| e.path()) {
        if p.extension().is_none_or(|e| e != "csv") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let rows: Vec<Vec<&str>> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split(',').map(str::trim).collect())
            .collect();
        if rows.is_empty() || rows.iter().any(|r| r.len() != num_reels) {
            continue;
        }
        for row in rows {
            for (reel, sym) in row.into_iter().enumerate() {
                if !sym.is_empty() {
                    per_reel[reel].insert(sym.to_string());
                }
            }
        }
        if let Some(n) = p.file_name().and_then(|n| n.to_str()) {
            files.push(n.to_string());
        }
    }
    files.sort();
    if files.is_empty() {
        return (None, files);
    }
    (
        Some(per_reel.into_iter().map(|s| s.into_iter().collect()).collect()),
        files,
    )
}

fn is_image(p: &FsPath) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "webp" | "jpg" | "jpeg" | "avif" | "gif" | "svg"))
}

fn url_segment(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// A static image per symbol, found by file name (`H1.webp`), preferring a
/// folder that says "symbol" — the paytable images most frontends ship.
fn images_of(game_name: &str, app: &FsPath, names: &[String]) -> BTreeMap<String, String> {
    let root = app.join("static");
    let mut files = Vec::new();
    walk(&root, &mut files, 8);
    let files: Vec<PathBuf> = files.into_iter().filter(|p| is_image(p)).collect();
    let mut out = BTreeMap::new();
    for name in names {
        let best = files
            .iter()
            .filter(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case(name))
            })
            .min_by_key(|p| {
                let s = p.to_string_lossy().to_ascii_lowercase();
                (!s.contains("symbol"), s.len())
            });
        if let Some(p) = best {
            if let Ok(rel) = p.strip_prefix(&root) {
                let rel: Vec<String> = rel
                    .components()
                    .map(|c| url_segment(&c.as_os_str().to_string_lossy()))
                    .collect();
                out.insert(
                    name.clone(),
                    format!(
                        "/api/ui/games/{}/board/asset/{}",
                        url_segment(game_name),
                        rel.join("/")
                    ),
                );
            }
        }
    }
    out
}

fn board_err(e: board::BoardError) -> ApiError {
    ApiError::bad_request(e.to_string())
}

fn build_info(game: &LoadedGame) -> Result<BoardInfo, ApiError> {
    let app = app_dir(game)?;
    let path = constants_path(&app)?;
    let src = std::fs::read_to_string(&path)
        .map_err(|e| ApiError::internal(format!("reading {}: {e}", path.display())))?;
    let parsed = board::parse(&src).map_err(board_err)?;
    let mut notes = Vec::new();

    let fe = fe_config(game);
    if fe.is_none() {
        notes.push(
            "config_fe_<game>.json not found next to publish_files — no pays: the win check is off"
                .to_string(),
        );
    }
    let num_reels = fe
        .as_ref()
        .and_then(|f| f.get("numReels"))
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(parsed.board.len());
    let num_rows: Vec<usize> = fe
        .as_ref()
        .and_then(|f| f.get("numRows"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).map(|n| n as usize).collect())
        .filter(|v: &Vec<usize>| v.len() == num_reels)
        .unwrap_or_else(|| parsed.board.iter().map(|r| r.len().saturating_sub(2)).collect());

    let mut symbols = fe.as_ref().map(symbols_of).unwrap_or_default();
    // Symbols the board uses but the config does not list (frontend-only).
    for cell in parsed.board.iter().flatten() {
        if let Some(n) = cell.iter().find(|(k, _)| k == "name").and_then(|(_, v)| v.as_str()) {
            if !symbols.iter().any(|s| s.name == n) {
                symbols.push(SymbolInfo {
                    name: n.to_string(),
                    pays: Vec::new(),
                    special: Vec::new(),
                });
            }
        }
    }
    let paylines = fe
        .as_ref()
        .and_then(|f| f.get("paylines"))
        .and_then(Value::as_object)
        .map(|m| {
            let mut lines: Vec<(u64, Vec<usize>)> = m
                .iter()
                .filter_map(|(k, v)| {
                    let rows = v
                        .as_array()?
                        .iter()
                        .map(|x| x.as_u64().map(|n| n as usize))
                        .collect::<Option<Vec<_>>>()?;
                    Some((k.parse().unwrap_or(u64::MAX), rows))
                })
                .collect();
            lines.sort_by_key(|(k, _)| *k);
            lines.into_iter().map(|(_, r)| r).collect::<Vec<_>>()
        })
        .filter(|l| !l.is_empty());

    let (strip_symbols, strip_files) = strips_of(game, num_reels);
    if strip_symbols.is_none() {
        notes.push("no reel strips found (<game>/reels/*.csv) — placement is not checked".to_string());
    }
    let names: Vec<String> = symbols.iter().map(|s| s.name.clone()).collect();
    let images = images_of(&game.entry.name, &app, &names);

    let consts = parsed
        .consts
        .iter()
        .map(|(n, c)| ConstInfo {
            name: n.clone(),
            value: c.iter().cloned().collect(),
        })
        .collect();

    Ok(BoardInfo {
        game: game.entry.name.clone(),
        constants_path: path.display().to_string(),
        base_hash: format!("{:016x}", xxh3_64(src[parsed.start..parsed.end].as_bytes())),
        board: board::board_to_json(&parsed.board),
        consts,
        num_reels,
        num_rows,
        symbols,
        paylines,
        strip_symbols,
        strip_files,
        images,
        notes,
    })
}

async fn get_board(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<BoardInfo>, ApiError> {
    let game = game_of(&state, &name)?;
    let info = tokio::task::spawn_blocking(move || build_info(&game))
        .await
        .map_err(|e| ApiError::internal(format!("board scan failed: {e}")))??;
    Ok(Json(info))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteBody {
    board: Vec<Vec<Map<String, Value>>>,
    #[serde(default)]
    base_hash: Option<String>,
    #[serde(default)]
    force: bool,
}

/// The JSON board as cells in the file's own key order, after checking its shape
/// against the board already in the file (same reels, same rows).
fn cells_for(
    parsed: &board::ParsedBoard,
    board_json: &[Vec<Map<String, Value>>],
) -> Result<Vec<Vec<Cell>>, ApiError> {
    if board_json.len() != parsed.board.len() {
        return Err(ApiError::bad_request(format!(
            "{} reels sent, the game has {}",
            board_json.len(),
            parsed.board.len()
        )));
    }
    let hints = board::key_order_hints(parsed);
    let mut out = Vec::with_capacity(board_json.len());
    for (r, (reel, current)) in board_json.iter().zip(&parsed.board).enumerate() {
        if reel.len() != current.len() {
            return Err(ApiError::bad_request(format!(
                "reel {}: {} rows sent, the file has {}",
                r + 1,
                reel.len(),
                current.len()
            )));
        }
        let mut cells = Vec::with_capacity(reel.len());
        for (row, m) in reel.iter().enumerate() {
            if !m.get("name").is_some_and(Value::is_string) {
                return Err(ApiError::bad_request(format!(
                    "reel {}, row {row}: symbol without a name",
                    r + 1
                )));
            }
            cells.push(board::cell_from_map(m, &hints));
        }
        out.push(cells);
    }
    Ok(out)
}

async fn put_board(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<WriteBody>,
) -> Result<Json<BoardInfo>, ApiError> {
    let game = game_of(&state, &name)?;
    let info = tokio::task::spawn_blocking(move || -> Result<BoardInfo, ApiError> {
        let app = app_dir(&game)?;
        let path = constants_path(&app)?;
        let src = std::fs::read_to_string(&path)
            .map_err(|e| ApiError::internal(format!("reading {}: {e}", path.display())))?;
        let parsed = board::parse(&src).map_err(board_err)?;
        let current = format!("{:016x}", xxh3_64(src[parsed.start..parsed.end].as_bytes()));
        if !body.force && body.base_hash.as_deref().is_some_and(|h| h != current) {
            return Err(ApiError::conflict(
                "INITIAL_BOARD changed on disk since this board was loaded",
            ));
        }
        let cells = cells_for(&parsed, &body.board)?;
        let out = board::replace(&src, &cells).map_err(board_err)?;
        // Write-then-rename: an editor or a dev server watching the file never
        // reads it half written.
        let tmp = path.with_extension("ts.slotlab-tmp");
        std::fs::write(&tmp, &out)
            .and_then(|_| std::fs::rename(&tmp, &path))
            .map_err(|e| ApiError::internal(format!("writing {}: {e}", path.display())))?;
        build_info(&game)
    })
    .await
    .map_err(|e| ApiError::internal(format!("board write failed: {e}")))??;
    Ok(Json(info))
}

/// The declaration as it would be written — what the page shows and copies.
async fn preview_board(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<WriteBody>,
) -> Result<Json<Value>, ApiError> {
    let game = game_of(&state, &name)?;
    let app = app_dir(&game)?;
    let path = constants_path(&app)?;
    let src = std::fs::read_to_string(&path)
        .map_err(|e| ApiError::internal(format!("reading {}: {e}", path.display())))?;
    let parsed = board::parse(&src).map_err(board_err)?;
    let cells = cells_for(&parsed, &body.board)?;
    Ok(Json(json!({ "text": board::render(&cells, &parsed) })))
}

/// Symbol images, served from the app's `static/` (they exist before any build).
async fn board_asset(
    State(state): State<AppState>,
    Path((name, rel)): Path<(String, String)>,
) -> Response {
    let Ok(game) = game_of(&state, &name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(app) = game.entry.app_dir.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if rel.split('/').any(|c| c == ".." || c.is_empty()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let path = app.join("static").join(&rel);
    if !is_image(&path) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            (
                [
                    (header::CONTENT_TYPE, mime.as_ref().to_string()),
                    (header::CACHE_CONTROL, "no-cache".to_string()),
                ],
                bytes,
            )
                .into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
