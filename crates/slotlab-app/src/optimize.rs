use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use slotlab_core::optimizer::{self, OptimizeRequest};
use slotlab_core::stats;
use std::io::Write;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/ui/games/{name}/optimize/{mode}", get(feasibility).post(preview))
        .route("/api/ui/games/{name}/optimize/{mode}/apply", post(apply))
}

/// Feasibility only (current RTP, reachable range) — to bootstrap the UI.
async fn feasibility(
    State(state): State<AppState>,
    Path((name, mode)): Path<(String, String)>,
) -> Json<Value> {
    let Some(game) = state.registry.get(&name) else {
        return Json(json!({ "error": "unknown game" }));
    };
    let Some(m) = game.resolve_mode(&mode) else {
        return Json(json!({ "error": "unknown mode" }));
    };
    let feas = optimizer::feasibility(&m.lut, m.config.cost, 1050.0);
    Json(json!({ "feasibility": feas, "cost": m.config.cost }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewResponse {
    result: optimizer::OptimizeResult,
    /// Full stats BEFORE (current LUT).
    before: stats::ModeStats,
    /// Full stats AFTER (rebalanced LUT), for the live diff.
    after: stats::ModeStats,
}

async fn preview(
    State(state): State<AppState>,
    Path((name, mode)): Path<(String, String)>,
    Json(req): Json<OptimizeRequest>,
) -> Json<Value> {
    let Some(game) = state.registry.get(&name) else {
        return Json(json!({ "error": "unknown game" }));
    };
    let Some(m) = game.resolve_mode(&mode) else {
        return Json(json!({ "error": "unknown mode" }));
    };
    let result = optimizer::optimize(&m.lut, m.config.cost, &req);
    let before = stats::analyze(&m.lut, m.config.cost);
    // LUT rebuilt with the new weights for the "after" analysis.
    let after_lut = rebuilt_lut(&m.lut, &result.weights);
    let after = stats::analyze(&after_lut, m.config.cost);
    Json(serde_json::to_value(PreviewResponse { result, before, after }).unwrap())
}

fn rebuilt_lut(lut: &slotlab_core::model::Lut, weights: &[u64]) -> slotlab_core::model::Lut {
    let rows = lut
        .rows
        .iter()
        .zip(weights)
        .map(|(r, &w)| slotlab_core::model::LutRow { weight: w, ..*r })
        .collect();
    slotlab_core::model::Lut::new(rows)
}

#[derive(Deserialize)]
struct ApplyRequest {
    #[serde(flatten)]
    opt: OptimizeRequest,
}

/// Writes the new lookUpTable into publish_files (backup in _prebalance/),
/// then reloads the game (the LUT fingerprint changes → stats/RGS updated).
async fn apply(
    State(state): State<AppState>,
    Path((name, mode)): Path<(String, String)>,
    Json(req): Json<ApplyRequest>,
) -> Json<Value> {
    let Some(game) = state.registry.get(&name) else {
        return Json(json!({ "error": "unknown game" }));
    };
    let Some(m) = game.resolve_mode(&mode) else {
        return Json(json!({ "error": "unknown mode" }));
    };
    let result = optimizer::optimize(&m.lut, m.config.cost, &req.opt);
    if result.weights.len() != m.lut.rows.len() {
        return Json(json!({ "error": "weight misalignment" }));
    }

    let lut_path = game.entry.publish_dir.join(&m.config.weights);
    // Backup.
    let backup_dir = game.entry.publish_dir.join("_prebalance");
    if let Err(e) = std::fs::create_dir_all(&backup_dir) {
        return Json(json!({ "error": format!("backup dir: {e}") }));
    }
    let backup = backup_dir.join(&m.config.weights);
    if lut_path.exists() && !backup.exists() {
        // Don't overwrite an existing backup (keeps the pristine original).
        if let Err(e) = std::fs::copy(&lut_path, &backup) {
            return Json(json!({ "error": format!("backup: {e}") }));
        }
    }

    // Atomic write of the new CSV (same row order, weights replaced).
    let tmp = lut_path.with_extension("csv.tmp");
    let write = || -> std::io::Result<()> {
        let f = std::fs::File::create(&tmp)?;
        let mut w = std::io::BufWriter::new(f);
        for (row, &weight) in m.lut.rows.iter().zip(&result.weights) {
            writeln!(w, "{},{},{}", row.sim_id, weight, row.payout_x100)?;
        }
        w.flush()
    };
    if let Err(e) = write().and_then(|_| std::fs::rename(&tmp, &lut_path)) {
        return Json(json!({ "error": format!("CSV write: {e}") }));
    }

    // Reload (re-parse the LUT; books intact, no re-import).
    match state.registry.reload(&name) {
        Ok(_) => {}
        Err(e) => return Json(json!({ "error": format!("reload: {e:#}") })),
    }
    state.books.invalidate_game(&name);

    Json(json!({
        "ok": true,
        "achievedRtp": result.achieved_rtp,
        "achievedVol": result.achieved_vol,
        "backup": backup.display().to_string(),
        "lut": lut_path.display().to_string(),
    }))
}
