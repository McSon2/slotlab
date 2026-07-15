use crate::state::AppState;
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/ui/pick-folder", post(pick_folder))
        .route("/api/ui/resolve", post(resolve))
}

/// Finds `publish_files` directories under (or equal to) `root`.
/// Accepts: a direct publish_files, a game directory
/// (`<game>/math/games/<id>/library/publish_files`), or the repo root
/// (several games). Search bounded to a few levels, not a full walk.
pub fn resolve_publish_dirs(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if is_publish_dir(root) {
        found.push(root.to_path_buf());
        return found;
    }
    // <root>/library/publish_files
    let direct = root.join("library/publish_files");
    if is_publish_dir(&direct) {
        found.push(direct);
    }
    // <root>/math/games/*/library/publish_files (a game directory)
    collect_under(&root.join("math/games"), &mut found);
    // <root>/*/math/games/*/library/publish_files (repo root)
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                collect_under(&e.path().join("math/games"), &mut found);
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

fn is_publish_dir(p: &Path) -> bool {
    p.join("index.json").is_file()
}

fn collect_under(games_dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(games_dir) else {
        return;
    };
    for e in entries.flatten() {
        let pf = e.path().join("library/publish_files");
        if is_publish_dir(&pf) {
            out.push(pf);
        }
    }
}

/// game_id from `.../math/games/<id>/library/publish_files`.
pub fn game_id_from_path(p: &Path) -> Option<String> {
    let s = p.to_string_lossy();
    s.split("/math/games/")
        .nth(1)?
        .split('/')
        .next()
        .map(|s| s.to_string())
}

fn describe(paths: Vec<PathBuf>) -> Value {
    let items: Vec<Value> = paths
        .iter()
        .map(|p| {
            json!({
                "path": p.display().to_string(),
                "name": game_id_from_path(p).unwrap_or_default(),
            })
        })
        .collect();
    json!({ "games": items })
}

/// Opens the native macOS folder picker (osascript) and resolves the
/// selection into publish_files. Returns `{ cancelled: true }` if cancelled.
async fn pick_folder(State(_): State<AppState>) -> Json<Value> {
    let out = tokio::task::spawn_blocking(|| {
        Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "POSIX path of (choose folder with prompt \"Choose the game folder (or its publish_files)\")",
            ])
            .output()
    })
    .await;

    let output = match out {
        Ok(Ok(o)) => o,
        _ => return Json(json!({ "error": "unable to open the macOS picker" })),
    };
    if !output.status.success() {
        // User cancellation = non-zero exit "User canceled".
        return Json(json!({ "cancelled": true }));
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return Json(json!({ "cancelled": true }));
    }
    let paths = resolve_publish_dirs(Path::new(&raw));
    let mut resp = describe(paths);
    resp["picked"] = json!(raw);
    Json(resp)
}

#[derive(Deserialize)]
struct ResolveReq {
    path: String,
}

/// Resolves a hand-typed path into publish_files (same rules).
async fn resolve(Json(req): Json<ResolveReq>) -> Json<Value> {
    let paths = resolve_publish_dirs(Path::new(req.path.trim()));
    Json(describe(paths))
}
