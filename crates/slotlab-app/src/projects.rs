//! Projects tab: every Stake Engine slot project on the VPS, from the `slot`
//! CLI (`~/.local/bin/slot`), with its archive / restore / clean actions.
//!
//! The CLI owns the logic (what is regenerable, the Drive remote, the
//! verification before deleting anything); this module only lists, matches
//! each project with the SlotLab game that reads its publish files, and runs
//! one CLI job at a time per project in the background.
//!
//! The key sent by the UI is never handed to a shell: it must be one of the
//! keys `slot list` itself returned, and is passed as a single argv entry.

use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/ui/projects", get(list))
        .route("/api/ui/projects/jobs", get(jobs))
        .route("/api/ui/projects/action", post(action))
        .route("/api/ui/projects/drive", get(drive))
}

/// Reading the whole Drive archive takes ~20 s: one answer serves the Drive tab for a while.
const DRIVE_TTL: Duration = Duration::from_secs(120);

fn drive_cache() -> &'static tokio::sync::Mutex<Option<(Instant, Value)>> {
    static C: OnceLock<tokio::sync::Mutex<Option<(Instant, Value)>>> = OnceLock::new();
    C.get_or_init(|| tokio::sync::Mutex::new(None))
}

/// GET /api/ui/projects/drive — `slot drive --json`: every game stored on Google Drive.
async fn drive(Query(q): Query<ListQuery>) -> Response {
    let force = q.refresh.is_some_and(|r| r != "0" && r != "false");
    let mut cache = drive_cache().lock().await;
    if !force {
        if let Some((at, v)) = cache.as_ref() {
            if at.elapsed() < DRIVE_TTL {
                return Json(v.clone()).into_response();
            }
        }
    }
    let out = match Command::new(slot_bin()).args(["drive", "--json"]).stdin(Stdio::null()).output().await {
        Ok(o) => o,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, format!("cannot run `slot`: {e}")),
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return err(StatusCode::BAD_GATEWAY, format!("`slot drive` failed: {}", stderr.trim()));
    }
    match serde_json::from_slice::<Value>(&out.stdout) {
        Ok(v) => {
            *cache = Some((Instant::now(), v.clone()));
            Json(v).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("`slot drive` returned invalid JSON: {e}")),
    }
}

/// `slot list` takes about a second: one answer serves every poll for a while.
const CACHE_TTL: Duration = Duration::from_secs(15);

const STATUSES: [&str; 3] = ["en_cours", "pause", "livre"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Archive,
    Restore,
    Clean,
    Status,
}

impl Action {
    fn as_str(self) -> &'static str {
        match self {
            Action::Archive => "archive",
            Action::Restore => "restore",
            Action::Clean => "clean",
            Action::Status => "status",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Running,
    Done,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub key: String,
    pub action: Action,
    pub state: JobState,
    pub last_progress: Option<String>,
    pub message: Option<String>,
    /// Unix milliseconds.
    pub started_at: u64,
    pub finished_at: Option<u64>,
}

/// Last job of each project. A running one blocks any other on that key.
#[derive(Default)]
pub struct Jobs {
    map: Mutex<HashMap<String, Job>>,
}

impl Jobs {
    /// Records a new running job, or refuses when one already runs on `key`.
    pub fn try_start(&self, key: &str, action: Action) -> Result<(), Job> {
        let mut map = self.map.lock().unwrap();
        if let Some(j) = map.get(key) {
            if j.state == JobState::Running {
                return Err(j.clone());
            }
        }
        map.insert(
            key.to_string(),
            Job {
                key: key.to_string(),
                action,
                state: JobState::Running,
                last_progress: None,
                message: None,
                started_at: now_ms(),
                finished_at: None,
            },
        );
        Ok(())
    }

    fn progress(&self, key: &str, line: &str) {
        if let Some(j) = self.map.lock().unwrap().get_mut(key) {
            j.last_progress = Some(line.to_string());
        }
    }

    pub fn finish(&self, key: &str, ok: bool, message: Option<String>) {
        if let Some(j) = self.map.lock().unwrap().get_mut(key) {
            j.state = if ok { JobState::Done } else { JobState::Error };
            j.message = message;
            j.finished_at = Some(now_ms());
        }
    }

    pub fn get(&self, key: &str) -> Option<Job> {
        self.map.lock().unwrap().get(key).cloned()
    }

    pub fn all(&self) -> Vec<Job> {
        let mut v: Vec<Job> = self.map.lock().unwrap().values().cloned().collect();
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        v
    }
}

struct Manager {
    jobs: Jobs,
    cache: tokio::sync::Mutex<Option<(Instant, Value)>>,
}

fn manager() -> &'static Manager {
    static M: OnceLock<Manager> = OnceLock::new();
    M.get_or_init(|| Manager {
        jobs: Jobs::default(),
        cache: tokio::sync::Mutex::new(None),
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn slot_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("SLOTLAB_SLOT_BIN") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/home/ubuntu".into());
    home.join(".local/bin/slot")
}

fn err(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(json!({ "error": msg.into() }))).into_response()
}

/// `slot list --json`, from the cache unless stale or `force`.
async fn slot_list(force: bool) -> Result<Value, String> {
    let m = manager();
    let mut cache = m.cache.lock().await;
    if !force {
        if let Some((at, v)) = cache.as_ref() {
            if at.elapsed() < CACHE_TTL {
                return Ok(v.clone());
            }
        }
    }
    let out = Command::new(slot_bin())
        .args(["list", "--json"])
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("cannot run `slot`: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("`slot list` failed: {}", stderr.trim()));
    }
    let v: Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("`slot list` returned invalid JSON: {e}"))?;
    *cache = Some((Instant::now(), v.clone()));
    Ok(v)
}

async fn invalidate() {
    *manager().cache.lock().await = None;
    *drive_cache().lock().await = None;
}

fn norm(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// SlotLab game reading this publish_files directory (or one under it).
fn match_game(publish_files: &str, games: &[(String, PathBuf, PathBuf)]) -> Option<String> {
    if publish_files.is_empty() {
        // Drive-only project: no folder on the VPS, `""` would prefix every path.
        return None;
    }
    let raw = Path::new(publish_files);
    let canon = norm(raw);
    games
        .iter()
        .find(|(_, dir, dir_c)| dir.starts_with(raw) || dir_c.starts_with(&canon))
        .map(|(name, _, _)| name.clone())
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    refresh: Option<String>,
}

async fn list(State(state): State<AppState>, Query(q): Query<ListQuery>) -> Response {
    let force = q.refresh.is_some_and(|r| r != "0" && r != "false");
    let mut v = match slot_list(force).await {
        Ok(v) => v,
        Err(e) => return err(StatusCode::BAD_GATEWAY, e),
    };
    let games: Vec<(String, PathBuf, PathBuf)> = state
        .registry
        .list_entries()
        .into_iter()
        .map(|e| {
            let c = norm(&e.publish_dir);
            (e.name, e.publish_dir, c)
        })
        .collect();
    let jobs = &manager().jobs;
    if let Some(projects) = v.get_mut("projects").and_then(Value::as_array_mut) {
        for p in projects {
            let game = p
                .get("publish_files")
                .and_then(Value::as_str)
                .and_then(|pf| match_game(pf, &games));
            let job = p
                .get("key")
                .and_then(Value::as_str)
                .and_then(|k| jobs.get(k));
            if let Some(obj) = p.as_object_mut() {
                obj.insert("slotlab_game".into(), json!(game));
                obj.insert("job".into(), json!(job));
            }
        }
    }
    Json(v).into_response()
}

async fn jobs() -> Json<Value> {
    Json(json!({ "jobs": manager().jobs.all() }))
}

#[derive(Deserialize)]
struct ActionBody {
    key: String,
    action: Action,
    #[serde(default)]
    status: Option<String>,
}

async fn action(Json(body): Json<ActionBody>) -> Response {
    let list = match slot_list(false).await {
        Ok(v) => v,
        Err(e) => return err(StatusCode::BAD_GATEWAY, e),
    };
    let project = list
        .get("projects")
        .and_then(Value::as_array)
        .and_then(|ps| ps.iter().find(|p| p.get("key").and_then(Value::as_str) == Some(body.key.as_str())));
    let Some(project) = project else {
        return err(StatusCode::BAD_REQUEST, format!("unknown project `{}`", body.key));
    };
    // From here on `key` is a string `slot list` produced itself.
    let key = body.key.clone();
    let mut args: Vec<String> = vec![body.action.as_str().into(), key.clone()];
    match body.action {
        Action::Status => {
            let Some(s) = body.status.as_deref().filter(|s| STATUSES.contains(s)) else {
                return err(
                    StatusCode::BAD_REQUEST,
                    "status must be one of en_cours, pause, livre",
                );
            };
            args.push(s.into());
        }
        Action::Clean => args.push("--yes".into()),
        Action::Archive | Action::Restore => {}
    }
    if body.action != Action::Status
        && project.get("running").and_then(Value::as_bool).unwrap_or(false)
    {
        return err(
            StatusCode::CONFLICT,
            format!("a simulation or transfer is running on {key}: try again once it ends"),
        );
    }
    let jobs = &manager().jobs;
    if let Err(j) = jobs.try_start(&key, body.action) {
        return err(
            StatusCode::CONFLICT,
            format!("{} already running on {key}", j.action.as_str()),
        );
    }

    if body.action == Action::Status {
        let res = run_cli(&key, &args).await;
        let ok = res.is_ok();
        let msg = match res {
            Ok(m) | Err(m) => m,
        };
        jobs.finish(&key, ok, Some(msg.clone()));
        invalidate().await;
        return if ok {
            Json(json!({ "job": jobs.get(&key) })).into_response()
        } else {
            err(StatusCode::INTERNAL_SERVER_ERROR, msg)
        };
    }

    let k = key.clone();
    tokio::spawn(async move {
        let res = run_cli(&k, &args).await;
        let ok = res.is_ok();
        let msg = match res {
            Ok(m) | Err(m) => m,
        };
        if ok {
            tracing::info!("projects: {} {k} done", args[0]);
        } else {
            tracing::warn!("projects: {} {k} failed: {msg}", args[0]);
        }
        manager().jobs.finish(&k, ok, Some(msg));
        invalidate().await;
    });
    (StatusCode::ACCEPTED, Json(json!({ "job": jobs.get(&key) }))).into_response()
}

/// Splits on `\n` and `\r` (rclone redraws its stats line with `\r`).
fn take_lines(buf: &mut Vec<u8>) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(i) = buf.iter().position(|&b| b == b'\n' || b == b'\r') {
        let line: Vec<u8> = buf.drain(..=i).collect();
        let s = String::from_utf8_lossy(&line[..line.len() - 1]).trim().to_string();
        if !s.is_empty() {
            out.push(s);
        }
    }
    out
}

/// Runs `slot <args>`, streaming `PROGRESS …` lines into the job. Ok = the
/// last plain stdout line; Err = the `ERREUR` line (or last stderr line).
async fn run_cli(key: &str, args: &[String]) -> Result<String, String> {
    let mut child = Command::new(slot_bin())
        .args(args)
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run `slot`: {e}"))?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");

    let err_task = tokio::spawn(async move {
        let mut s = Vec::new();
        let _ = stderr.read_to_end(&mut s).await;
        String::from_utf8_lossy(&s).into_owned()
    });

    let jobs = &manager().jobs;
    let mut last_plain: Option<String> = None;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stdout.read(&mut chunk).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        for line in take_lines(&mut buf) {
            if let Some(p) = line.strip_prefix("PROGRESS ") {
                jobs.progress(key, p.trim());
            } else {
                last_plain = Some(line);
            }
        }
    }
    buf.push(b'\n');
    for line in take_lines(&mut buf) {
        if let Some(p) = line.strip_prefix("PROGRESS ") {
            jobs.progress(key, p.trim());
        } else {
            last_plain = Some(line);
        }
    }

    let status = child.wait().await.map_err(|e| format!("`slot` did not finish: {e}"))?;
    let stderr = err_task.await.unwrap_or_default();
    if status.success() {
        Ok(last_plain.unwrap_or_else(|| "done".into()))
    } else {
        let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let msg = lines
            .iter()
            .rev()
            .find(|l| l.contains("ERREUR"))
            .or(lines.last())
            .map(|l| l.to_string())
            .unwrap_or_else(|| format!("`slot {}` exited with {status}", args[0]));
        Err(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_running_job_per_key() {
        let jobs = Jobs::default();
        assert!(jobs.try_start("A/x", Action::Archive).is_ok());
        let conflict = jobs.try_start("A/x", Action::Clean).unwrap_err();
        assert_eq!(conflict.action, Action::Archive);
        // Another project is independent.
        assert!(jobs.try_start("B/y", Action::Restore).is_ok());
        jobs.finish("A/x", true, Some("ok".into()));
        assert_eq!(jobs.get("A/x").unwrap().state, JobState::Done);
        // Finished: a new job may start.
        assert!(jobs.try_start("A/x", Action::Clean).is_ok());
        assert_eq!(jobs.get("A/x").unwrap().state, JobState::Running);
    }

    #[test]
    fn splits_cr_and_lf() {
        let mut b = b"PROGRESS 1%\rPROGRESS 2%\nfoo".to_vec();
        assert_eq!(take_lines(&mut b), vec!["PROGRESS 1%", "PROGRESS 2%"]);
        assert_eq!(b, b"foo");
    }

    #[test]
    fn matches_game_under_publish_dir() {
        let games = vec![
            ("a".to_string(), PathBuf::from("/x/a/publish_files"), PathBuf::from("/x/a/publish_files")),
            ("b".to_string(), PathBuf::from("/x/b/publish_files/sub"), PathBuf::from("/x/b/publish_files/sub")),
        ];
        assert_eq!(match_game("/x/a/publish_files", &games).as_deref(), Some("a"));
        assert_eq!(match_game("/x/b/publish_files", &games).as_deref(), Some("b"));
        assert_eq!(match_game("/x/c/publish_files", &games), None);
        assert_eq!(match_game("/x/a/publish", &games), None);
    }
}
