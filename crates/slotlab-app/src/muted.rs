//! Findings a studio chose to stop seeing, per game.
//!
//! The rules of « Worth a look » (`frontend/src/lib/findings.ts`), the
//! pre-upload checklist and the crowd simulation are written for every slot;
//! some of them make no sense for a given one (a symbol mix rule on a game
//! whose free spins are meant to be loaded with highs, a celebration rule on a
//! mode the client decided to celebrate anyway). Muting one keeps it out of
//! the counters without losing it: the UI lists what is muted and can bring
//! it back.
//!
//! A mute is `{ rule, mode }`: `rule` is the stable key of the finding (never
//! its numbers), `mode` one mode or `null` for every mode of the game. Kept on
//! the server (`<app dir>/muted/<game>.json`) so the Mac, the remote UI and a
//! shared link all see the same list.
//!
//! What Stake itself refuses — compliance checks, publish checks, star
//! limits, gaps, the most likely book, the bonus ladder, the checklist items
//! that already cost a refusal — can never be muted: the API turns them away.

use crate::api::routes::ApiError;
use crate::persist;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Mutex;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/ui/games/{name}/muted",
        get(list).put(replace).post(add).delete(remove),
    )
}

/// One muted finding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Mute {
    /// Stable key of the finding (`plaque-loss`, `symbols-fs`, `preflight:unused_assets`, `sim:dry-spell`…).
    pub rule: String,
    /// One mode, or `None` for every mode of the game.
    #[serde(default)]
    pub mode: Option<String>,
    /// Unix milliseconds, set by the server.
    #[serde(default)]
    pub at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MuteFile {
    #[serde(default)]
    muted: Vec<Mute>,
}

/// Keys of `lib/findings.ts` that are always a Stake rule (blocking whatever the numbers).
const UNMUTABLE_PREFIXES: &[&str] = &["check:", "publish:", "star:"];
const UNMUTABLE_RULES: &[&str] = &["gap", "most-likely", "ladder-freq", "ladder-mean", "ante-tier"];
/// Checklist items computed by `crate::preflight` with `Severity::Fail`.
const UNMUTABLE_PREFLIGHT: &[&str] = &["ghost_assets", "lut_book_match", "event_coverage"];

/// A blocking finding — the API refuses to mute it.
pub fn is_unmutable(rule: &str) -> bool {
    if UNMUTABLE_PREFIXES.iter().any(|p| rule.starts_with(p)) || UNMUTABLE_RULES.contains(&rule) {
        return true;
    }
    if let Some(id) = rule.strip_prefix("preflight:") {
        return UNMUTABLE_PREFLIGHT.contains(&id)
            || slotlab_core::preflight::RULES
                .iter()
                .any(|r| r.id == id && r.severity == slotlab_core::preflight::Severity::Fail);
    }
    false
}

/// Checks and normalises one mute (trimmed, `""` mode = every mode).
fn validate(mut m: Mute) -> Result<Mute, String> {
    m.rule = m.rule.trim().to_string();
    if m.rule.is_empty() || m.rule.len() > 200 {
        return Err("rule: a non-empty key of at most 200 characters".into());
    }
    if is_unmutable(&m.rule) {
        return Err(format!("{}: a blocking (Stake) finding cannot be muted", m.rule));
    }
    m.mode = m.mode.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if m.mode.as_ref().is_some_and(|s| s.len() > 100) {
        return Err("mode: at most 100 characters".into());
    }
    Ok(m)
}

/// Adds `m` unless the same `{ rule, mode }` is already there.
fn insert(list: &mut Vec<Mute>, m: Mute) {
    if !list.iter().any(|x| x.rule == m.rule && x.mode == m.mode) {
        list.push(m);
    }
}

fn dir() -> PathBuf {
    persist::app_dir().join("muted")
}

fn file_in(dir: &FsPath, game: &str) -> PathBuf {
    dir.join(format!("{}.json", game.replace(['/', '\\', '.'], "_")))
}

fn load_in(dir: &FsPath, game: &str) -> Vec<Mute> {
    std::fs::read(file_in(dir, game))
        .ok()
        .and_then(|b| serde_json::from_slice::<MuteFile>(&b).ok())
        .map(|f| f.muted)
        .unwrap_or_default()
}

/// Atomic write (tmp + rename); an empty list removes the file.
fn save_in(dir: &FsPath, game: &str, list: &[Mute]) -> std::io::Result<()> {
    let path = file_in(dir, game);
    if list.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(&MuteFile { muted: list.to_vec() })?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &path)
}

/// Read-modify-write of the files, one at a time (the Mac and a remote UI may click together).
static LOCK: Mutex<()> = Mutex::new(());

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn known(state: &AppState, name: &str) -> Result<(), ApiError> {
    state
        .registry
        .get(name)
        .map(|_| ())
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))
}

fn answer(name: &str, list: Vec<Mute>) -> Json<serde_json::Value> {
    Json(json!({ "game": name, "muted": list }))
}

fn update(name: &str, f: impl FnOnce(&mut Vec<Mute>)) -> Result<Vec<Mute>, ApiError> {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let d = dir();
    let mut list = load_in(&d, name);
    f(&mut list);
    save_in(&d, name, &list).map_err(|e| ApiError::internal(format!("saving muted findings: {e}")))?;
    Ok(list)
}

/// GET /api/ui/games/{name}/muted → `{ game, muted: [{ rule, mode, at }] }`.
async fn list(State(state): State<AppState>, Path(name): Path<String>) -> Result<Json<serde_json::Value>, ApiError> {
    known(&state, &name)?;
    Ok(answer(&name, load_in(&dir(), &name)))
}

/// POST /api/ui/games/{name}/muted `{ rule, mode? }` — mutes one finding (400 if blocking).
async fn add(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(m): Json<Mute>,
) -> Result<Json<serde_json::Value>, ApiError> {
    known(&state, &name)?;
    let m = Mute { at: now_ms(), ..validate(m).map_err(ApiError::bad_request)? };
    Ok(answer(&name, update(&name, |l| insert(l, m))?))
}

/// PUT /api/ui/games/{name}/muted `{ muted: [...] }` — replaces the whole list (400 if one is blocking).
async fn replace(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(f): Json<MuteFile>,
) -> Result<Json<serde_json::Value>, ApiError> {
    known(&state, &name)?;
    let mut next = Vec::new();
    for m in f.muted {
        let at = if m.at > 0 { m.at } else { now_ms() };
        insert(&mut next, Mute { at, ..validate(m).map_err(ApiError::bad_request)? });
    }
    Ok(answer(&name, update(&name, |l| *l = next)?))
}

#[derive(Deserialize)]
struct RemoveQuery {
    rule: String,
    /// Absent or empty: the every-mode mute.
    #[serde(default)]
    mode: Option<String>,
}

/// DELETE /api/ui/games/{name}/muted?rule=…&mode=… — brings one finding back.
async fn remove(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(q): Query<RemoveQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    known(&state, &name)?;
    let mode = q.mode.filter(|s| !s.is_empty());
    Ok(answer(&name, update(&name, |l| l.retain(|x| !(x.rule == q.rule && x.mode == mode)))?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mute(rule: &str, mode: Option<&str>) -> Mute {
        Mute { rule: rule.into(), mode: mode.map(Into::into), at: 0 }
    }

    #[test]
    fn blocking_findings_cannot_be_muted() {
        for r in ["check:rtp_range", "publish:most_likely_hr", "star:CVaR", "gap", "most-likely", "ladder-freq", "ladder-mean", "ante-tier", "preflight:lut_book_match", "preflight:renderer_webgpu_app"] {
            assert!(is_unmutable(r), "{r}");
            assert!(validate(mute(r, None)).is_err(), "{r}");
        }
        for r in ["plaque-loss", "symbols-fs", "hit-low", "preflight:unused_assets", "sim:dry-spell"] {
            assert!(!is_unmutable(r), "{r}");
        }
    }

    #[test]
    fn validate_trims_and_empty_mode_means_every_mode() {
        let m = validate(mute("  plaque-loss ", Some(" "))).unwrap();
        assert_eq!(m.rule, "plaque-loss");
        assert_eq!(m.mode, None);
        assert!(validate(mute("", None)).is_err());
        assert_eq!(validate(mute("hit-low", Some("MAXWIN"))).unwrap().mode.as_deref(), Some("MAXWIN"));
    }

    #[test]
    fn insert_dedupes_rule_and_mode() {
        let mut l = Vec::new();
        insert(&mut l, mute("symbols-fs", Some("MYSTERY")));
        insert(&mut l, mute("symbols-fs", Some("MYSTERY")));
        insert(&mut l, mute("symbols-fs", None));
        assert_eq!(l.len(), 2);
    }

    #[test]
    fn round_trip_on_disk_and_empty_list_removes_the_file() {
        let d = std::env::temp_dir().join(format!("slotlab-muted-{}", std::process::id()));
        let list = vec![mute("plaque-loss", Some("HIDDEN")), mute("hit-low", None)];
        save_in(&d, "meat/grinder", &list).unwrap();
        assert_eq!(load_in(&d, "meat/grinder"), list);
        assert!(load_in(&d, "other").is_empty());
        save_in(&d, "meat/grinder", &[]).unwrap();
        assert!(!file_in(&d, "meat/grinder").exists());
        save_in(&d, "meat/grinder", &[]).unwrap();
        std::fs::remove_dir_all(&d).ok();
    }
}
