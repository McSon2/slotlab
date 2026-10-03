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

/// Finds every `publish_files` folder under (or equal to) `root`, whatever
/// the layout: a publish_files itself, a game folder, a whole repo, a parent
/// of several repos… Bounded breadth-first walk (depth + visited caps) that
/// skips hidden/heavy directories, so picking a huge folder stays instant.
pub fn resolve_publish_dirs(root: &Path) -> Vec<PathBuf> {
    const MAX_DEPTH: u8 = 7;
    const MAX_VISITED: usize = 30_000;
    // Heavy or irrelevant directories. Note: macOS "Library" (capital L) is
    // skipped, but the math-sdk "library" (lowercase, parent of
    // publish_files) is NOT.
    const SKIP: &[&str] = &[
        "node_modules",
        "target",
        "dist",
        "build",
        "out",
        "Library",
        "__pycache__",
        "venv",
        "env",
    ];

    let mut found = Vec::new();
    if is_publish_dir(root) {
        found.push(root.to_path_buf());
        return found;
    }
    let mut visited = 0usize;
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0u8)]);
    while let Some((dir, depth)) = queue.pop_front() {
        visited += 1;
        if visited > MAX_VISITED {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            // Don't follow symlinks (cycles, escapes outside the picked tree).
            if e.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
                continue;
            }
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let name = e.file_name();
            let name = name.to_string_lossy();
            // '_' prefix = backup/scratch convention (math-sdk backups,
            // SlotLab's own _prebalance/…) — never offer those as games.
            if name.starts_with('.') || name.starts_with('_') || SKIP.contains(&name.as_ref()) {
                continue;
            }
            if is_publish_dir(&p) {
                found.push(p); // don't descend inside a publish_files
            } else if depth + 1 < MAX_DEPTH {
                queue.push_back((p, depth + 1));
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

/// Game name from a publish_files path. Tries, in order: the math-sdk
/// pattern `…/math/games/<id>/library/publish_files`, the folder above
/// `library/`, then the folder holding publish_files.
pub fn game_id_from_path(p: &Path) -> Option<String> {
    let s = p.to_string_lossy();
    if let Some(id) = s
        .split("/math/games/")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
    {
        return Some(id.to_string());
    }
    let comps: Vec<String> = p
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let n = comps.len();
    if n >= 3 && comps[n - 1] == "publish_files" && comps[n - 2] == "library" {
        return Some(comps[n - 3].clone());
    }
    if n >= 2 && comps[n - 1] == "publish_files" {
        return Some(comps[n - 2].clone());
    }
    None
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
