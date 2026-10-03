//! What changed between two builds of the math.
//!
//! ── Why a tool has to remember ──────────────────────────────────────────────
//! A `run.py prod` replaces every file in publish_files at once. The RTP may
//! have moved by a thousandth, or a mode's hit rate may have halved, or a max
//! win may have become ten times harder to reach — and nothing on screen says
//! so, because every panel shows the CURRENT numbers and has nothing to compare
//! them to. The regressions that cost the most were all of this shape: silent,
//! and only visible against the previous state.
//!
//! So every time a game is loaded with a fingerprint never seen before, its
//! headline numbers are written down. The list is kept next to the config, one
//! file per game, and the diff is computed against the entry before.
//!
//! Deliberately a few numbers per mode rather than the full report: the point
//! is to notice that something moved, and every panel of the app is there to
//! explain what.

use crate::api::routes::ApiError;
use crate::persist;
use crate::registry::LoadedGame;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;


pub fn router() -> Router<AppState> {
    Router::new().route("/api/ui/games/{name}/history", get(history))
}

/// Snapshots kept per game. Twenty builds is several weeks of work, and the
/// file stays a few kilobytes.
const MAX_SNAPSHOTS: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeSnapshot {
    pub name: String,
    pub cost: f64,
    pub rtp: f64,
    pub hit_rate: f64,
    pub zero_rate: f64,
    /// std ÷ cost — the Stake definition of volatility.
    pub volatility: f64,
    pub max_payout: f64,
    pub max_payout_one_in: f64,
    pub most_likely_one_in: f64,
    pub outcomes: usize,
    pub total_weight: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub fingerprint: u64,
    /// Unix milliseconds — formatted by whoever displays it.
    pub taken_at: u64,
    pub modes: Vec<ModeSnapshot>,
}

fn path_for(game: &str) -> std::path::PathBuf {
    persist::app_dir()
        .join("history")
        .join(format!("{}.json", game.replace(['/', '\\'], "_")))
}

fn load(game: &str) -> Vec<Snapshot> {
    std::fs::read(path_for(game))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn snapshot_of(game: &LoadedGame) -> Snapshot {
    let modes = game
        .modes
        .iter()
        .map(|m| {
            let s = m.stats();
            ModeSnapshot {
                name: m.config.name.clone(),
                cost: m.config.cost,
                rtp: s.rtp,
                hit_rate: s.hit_rate,
                zero_rate: s.zero_rate,
                volatility: s.cost_adj_volatility,
                max_payout: s.max_payout,
                max_payout_one_in: s.max_payout_one_in,
                most_likely_one_in: s.most_likely_one_in,
                outcomes: s.outcome_count,
                total_weight: s.total_weight,
            }
        })
        .collect();
    Snapshot {
        fingerprint: game.fingerprint,
        taken_at: now_ms(),
        modes,
    }
}

/// Records the game's current state, unless the last entry already describes
/// this exact build. Cheap enough to call on every load.
pub fn record(game: &LoadedGame) {
    let name = &game.entry.name;
    let mut list = load(name);
    if list
        .last()
        .is_some_and(|s| s.fingerprint == game.fingerprint)
    {
        return;
    }
    list.push(snapshot_of(game));
    if list.len() > MAX_SNAPSHOTS {
        let excess = list.len() - MAX_SNAPSHOTS;
        list.drain(0..excess);
    }
    let path = path_for(name);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&list) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
    tracing::info!("history: new build recorded for {name}");
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Change {
    field: &'static str,
    label: &'static str,
    before: f64,
    after: f64,
    /// Relative move, or the absolute one for a rate that is already a share.
    delta: f64,
    /// Worth stopping for: the move is large enough not to be rounding.
    notable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModeDiff {
    mode: String,
    status: &'static str,
    changes: Vec<Change>,
}

fn diff_modes(before: &Snapshot, after: &Snapshot) -> Vec<ModeDiff> {
    let mut out = Vec::new();
    for a in &after.modes {
        let Some(b) = before.modes.iter().find(|m| m.name == a.name) else {
            out.push(ModeDiff {
                mode: a.name.clone(),
                status: "added",
                changes: Vec::new(),
            });
            continue;
        };
        // (field, label, before, after, threshold on the relative move)
        let fields: [(&'static str, &'static str, f64, f64, f64); 7] = [
            ("rtp", "RTP", b.rtp, a.rtp, 0.0005),
            ("hitRate", "Hit rate", b.hit_rate, a.hit_rate, 0.01),
            ("volatility", "Volatility (std÷cost)", b.volatility, a.volatility, 0.02),
            ("maxPayout", "Max win", b.max_payout, a.max_payout, 0.001),
            (
                "maxPayoutOneIn",
                "Max win frequency",
                b.max_payout_one_in,
                a.max_payout_one_in,
                0.05,
            ),
            (
                "mostLikelyOneIn",
                "Most likely book",
                b.most_likely_one_in,
                a.most_likely_one_in,
                0.05,
            ),
            (
                "outcomes",
                "Books",
                b.outcomes as f64,
                a.outcomes as f64,
                0.0001,
            ),
        ];
        let changes: Vec<Change> = fields
            .iter()
            .filter_map(|(field, label, bv, av, threshold)| {
                if (av - bv).abs() < f64::EPSILON {
                    return None;
                }
                let rel = if bv.abs() > 0.0 && bv.is_finite() && av.is_finite() {
                    (av - bv) / bv.abs()
                } else {
                    1.0
                };
                Some(Change {
                    field,
                    label,
                    before: *bv,
                    after: *av,
                    delta: av - bv,
                    notable: rel.abs() >= *threshold,
                })
            })
            .collect();
        if !changes.is_empty() {
            out.push(ModeDiff {
                mode: a.name.clone(),
                status: "changed",
                changes,
            });
        }
    }
    for b in &before.modes {
        if !after.modes.iter().any(|m| m.name == b.name) {
            out.push(ModeDiff {
                mode: b.name.clone(),
                status: "removed",
                changes: Vec::new(),
            });
        }
    }
    out
}

async fn history(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    // A build loaded before this feature existed still deserves a baseline.
    record(&game);
    let list = load(&name);
    let diff = match list.len() {
        0 | 1 => Vec::new(),
        n => diff_modes(&list[n - 2], &list[n - 1]),
    };
    Ok(Json(json!({
        "game": name,
        "snapshots": list,
        "diff": diff,
        "current": game.fingerprint,
    })))
}
