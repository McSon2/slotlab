//! Full scan of a mode's books, weighted by its LUT.
//!
//! ── Why it is a job and not a query ─────────────────────────────────────────
//! Answering "how often does the anticipation actually fire" means reading
//! every event of every book — 1.8 million of them for a big game, a few
//! gigabytes once decompressed. That is a minute of CPU, not a request: so it
//! runs in the background, reports progress, and the result is kept against the
//! game's fingerprint. A re-run after a `run.py prod` re-reads everything,
//! because the fingerprint changed; a re-run before it returns instantly.
//!
//! The blocks are decompressed straight from the mmap and NOT put through the
//! shared cache: a scan reads each block exactly once, and pushing them all
//! through the LRU would evict what the RGS is serving spins from.

use crate::api::routes::ApiError;
use crate::registry::LoadedGame;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use slotlab_core::invariants::{self, Accumulator, ScanParams, ScanReport};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/ui/games/{name}/scan", get(status).post(start))
}

/// Books read to work out the shape of the game before the real pass.
const SHAPE_SAMPLE: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanState {
    Running,
    Done,
    Failed,
}

pub struct Job {
    state: ScanState,
    mode: String,
    error: Option<String>,
    books_done: Arc<AtomicU64>,
    books_total: u64,
}

/// Version de ce qu'un rapport compte. Un rapport d'une autre version est
/// ignoré — donc refait au prochain scan — même pour un build inchangé : le
/// fingerprint dit « la math n'a pas bougé », pas « le scan compte pareil ».
///   1 : retriggers selon la règle générique de `slotlab_core::freespins`
///       (avant : les seuls `freeSpinRetrigger`, soit 0 partout sur BLOOD WATER).
const SCAN_VERSION: u32 = 1;

/// A result as it is kept on disk, next to the config.
#[derive(Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredScan {
    /// Absent des rapports d'avant `SCAN_VERSION` → 0 → périmé.
    #[serde(default)]
    version: u32,
    fingerprint: u64,
    /// The report, kept as raw JSON: it is only ever handed back to the UI,
    /// and re-parsing it into its structs would buy nothing.
    report: serde_json::Value,
}

impl StoredScan {
    /// Même build ET même façon de compter.
    fn is_current(&self, fingerprint: u64) -> bool {
        self.version == SCAN_VERSION && self.fingerprint == fingerprint
    }
}

fn scan_path(game: &str, mode: &str) -> std::path::PathBuf {
    let safe = |s: &str| s.replace(['/', '\\', ':'], "_");
    crate::persist::app_dir()
        .join("scans")
        .join(format!("{}__{}.json", safe(game), safe(mode)))
}

#[derive(Default)]
pub struct ScanManager {
    /// One job per game — a scan saturates the machine, two would only be
    /// slower.
    jobs: Mutex<HashMap<String, Job>>,
    /// (game, mode) → (fingerprint, report).
    results: Mutex<HashMap<(String, String), (u64, ScanReport)>>,
}

impl ScanManager {
    /// Results for the CURRENT build, in mode order — from memory, or from
    /// disk when the process has been restarted since (a scan costs a minute
    /// of CPU; losing it to a restart would make the panel unusable).
    fn results_for(&self, game: &LoadedGame) -> Vec<serde_json::Value> {
        let results = self.results.lock().unwrap();
        game.modes
            .iter()
            .filter_map(|m| {
                let key = (game.entry.name.clone(), m.config.name.clone());
                if let Some((fp, r)) = results.get(&key) {
                    if *fp == game.fingerprint {
                        return serde_json::to_value(r).ok();
                    }
                }
                let bytes = std::fs::read(scan_path(&key.0, &key.1)).ok()?;
                let stored: StoredScan = serde_json::from_slice(&bytes).ok()?;
                stored.is_current(game.fingerprint).then_some(stored.report)
            })
            .collect()
    }

    /// True when a result for this exact build is already held.
    fn has_result(&self, game: &str, mode: &str, fingerprint: u64) -> bool {
        if self
            .results
            .lock()
            .unwrap()
            .get(&(game.to_string(), mode.to_string()))
            .is_some_and(|(fp, _)| *fp == fingerprint)
        {
            return true;
        }
        std::fs::read(scan_path(game, mode))
            .ok()
            .and_then(|b| serde_json::from_slice::<StoredScan>(&b).ok())
            .is_some_and(|s| s.is_current(fingerprint))
    }

    fn store(&self, game: &str, mode: &str, fingerprint: u64, report: ScanReport) {
        let path = scan_path(game, mode);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(value) = serde_json::to_value(&report) {
            let stored = StoredScan {
                version: SCAN_VERSION,
                fingerprint,
                report: value,
            };
            if let Ok(bytes) = serde_json::to_vec(&stored) {
                let tmp = path.with_extension("json.tmp");
                if std::fs::write(&tmp, bytes).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
        }
        self.results
            .lock()
            .unwrap()
            .insert((game.to_string(), mode.to_string()), (fingerprint, report));
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct StartScan {
    /// Absent → every mode of the game, cheapest first.
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    params: Option<ScanParams>,
    /// Re-scan even when a result for this fingerprint is already held.
    #[serde(default)]
    force: bool,
}

async fn status(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let job = {
        let jobs = state.scans.jobs.lock().unwrap();
        jobs.get(&name).map(|j| {
            json!({
                "state": j.state,
                "mode": j.mode,
                "booksDone": j.books_done.load(Ordering::Relaxed),
                "booksTotal": j.books_total,
                "error": j.error,
            })
        })
    };
    Ok(Json(json!({
        "game": name,
        "job": job,
        "reports": state.scans.results_for(&game),
    })))
}

async fn start(
    State(state): State<AppState>,
    Path(name): Path<String>,
    body: Option<Json<StartScan>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;

    let modes: Vec<String> = match &body.mode {
        Some(m) => vec![game
            .resolve_mode(m)
            .ok_or_else(|| ApiError::not_found(format!("unknown mode: {m}")))?
            .config
            .name
            .clone()],
        None => {
            let mut v: Vec<_> = game.modes.iter().collect();
            v.sort_by(|a, b| a.config.cost.total_cmp(&b.config.cost));
            v.into_iter().map(|m| m.config.name.clone()).collect()
        }
    };

    {
        let jobs = state.scans.jobs.lock().unwrap();
        if jobs.get(&name).is_some_and(|j| j.state == ScanState::Running) {
            return Ok(Json(json!({ "ok": true, "alreadyRunning": true })));
        }
    }

    let params = body.params.unwrap_or_default();
    let force = body.force;
    let state2 = state.clone();
    let name2 = name.clone();
    tokio::task::spawn_blocking(move || {
        for mode in modes {
            if !force && state2.scans.has_result(&name2, &mode, game.fingerprint) {
                continue;
            }
            let books_done = Arc::new(AtomicU64::new(0));
            {
                let mut jobs = state2.scans.jobs.lock().unwrap();
                jobs.insert(
                    name2.clone(),
                    Job {
                        state: ScanState::Running,
                        mode: mode.clone(),
                        error: None,
                        books_done: books_done.clone(),
                        books_total: 0,
                    },
                );
            }
            match scan_mode(&state2, &game, &mode, params, books_done) {
                Ok(report) => {
                    state2.scans.store(&name2, &mode, game.fingerprint, report);
                }
                Err(e) => {
                    let mut jobs = state2.scans.jobs.lock().unwrap();
                    if let Some(j) = jobs.get_mut(&name2) {
                        j.state = ScanState::Failed;
                        j.error = Some(format!("{mode}: {e:#}"));
                    }
                    tracing::error!("scan {name2}/{mode}: {e:#}");
                    return;
                }
            }
        }
        let mut jobs = state2.scans.jobs.lock().unwrap();
        if let Some(j) = jobs.get_mut(&name2) {
            j.state = ScanState::Done;
            j.mode.clear();
        }
    });

    Ok(Json(json!({ "ok": true })))
}

fn scan_mode(
    state: &AppState,
    game: &Arc<LoadedGame>,
    mode_name: &str,
    params: ScanParams,
    books_done: Arc<AtomicU64>,
) -> anyhow::Result<ScanReport> {
    let mode = game
        .resolve_mode(mode_name)
        .ok_or_else(|| anyhow::anyhow!("unknown mode"))?;
    let books = state.books.mode_books(game, mode_name)?;
    let index = &books.index;

    let weight_by_sim: HashMap<u64, u64> = mode
        .lut
        .rows
        .iter()
        .map(|r| (r.sim_id, r.weight))
        .collect();

    {
        let mut jobs = state.scans.jobs.lock().unwrap();
        if let Some(j) = jobs.get_mut(&game.entry.name) {
            j.books_total = index.books.len() as u64;
        }
    }

    // Records grouped by the block that holds them: one decompression per
    // block, whatever the order the ids come in.
    let mut by_block: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, rec) in index.books.iter().enumerate() {
        by_block.entry(rec.block).or_default().push(i);
    }
    let mut blocks: Vec<(u32, Vec<usize>)> = by_block.into_iter().collect();
    blocks.sort_by_key(|(b, _)| *b);

    // Shape first, on a sample: the trigger size and the height of the window
    // decide how every later number is counted.
    let mut sample: Vec<String> = Vec::with_capacity(SHAPE_SAMPLE);
    for (block, recs) in blocks.iter().take(4) {
        let raw = books.block_bytes(*block as usize)?;
        for i in recs.iter().take(SHAPE_SAMPLE / 4) {
            if let Some(line) = line_of(&raw, &index.books[*i]) {
                sample.push(line.to_string());
            }
        }
        if sample.len() >= SHAPE_SAMPLE {
            break;
        }
    }
    let mut shape = invariants::detect_shape(&sample);
    if params.trigger_scatters > 0 {
        shape.trigger_scatters = params.trigger_scatters;
        shape.derived = false;
    }
    if params.retrigger_scatters > 0 {
        shape.retrigger_scatters = params.retrigger_scatters;
        shape.derived = false;
    }
    if params.visible_rows > 0 {
        shape.visible_rows = params.visible_rows;
        shape.pad = ((shape.board_rows - shape.visible_rows) / 2).max(0);
        shape.derived = false;
    }

    let acc = blocks
        .par_iter()
        .fold(Accumulator::default, |mut acc, (block, recs)| {
            let Ok(raw) = books.block_bytes(*block as usize) else {
                return acc;
            };
            for i in recs {
                let rec = &index.books[*i];
                if let Some(line) = line_of(&raw, rec) {
                    let w = weight_by_sim.get(&rec.sim_id).copied().unwrap_or(0);
                    acc.push(line, w, &shape, &params);
                }
            }
            books_done.fetch_add(recs.len() as u64, Ordering::Relaxed);
            acc
        })
        .reduce(Accumulator::default, |mut a, b| {
            a.merge(&b);
            a
        });

    Ok(acc.finish(mode_name, mode.config.cost, shape, params))
}

fn line_of<'a>(raw: &'a [u8], rec: &crate::books::format::BookRecord) -> Option<&'a str> {
    let start = rec.offset_in_block as usize;
    let end = start + rec.len as usize;
    std::str::from_utf8(raw.get(start..end)?)
        .ok()
        .map(|s| s.trim_end())
}
