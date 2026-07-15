use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/ui/games/{name}/search-meta", get(search_meta))
        .route("/api/ui/search", post(search))
}

/// Search metadata per mode: criteria and event types — all from the
/// .idx sidecars (never from the .zst files). Trigger stats live in
/// the Analyze bonus-stats endpoint (weighted by the LUT).
async fn search_meta(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<serde_json::Value> {
    let Some(game) = state.registry.get(&name) else {
        return Json(json!({ "error": "unknown game" }));
    };
    let mut modes = Vec::new();
    for m in &game.modes {
        let Ok(books) = state.books.mode_books(&game, &m.config.name) else {
            modes.push(json!({
                "mode": m.config.name,
                "imported": false,
            }));
            continue;
        };
        let idx = &books.index;
        modes.push(json!({
            "mode": m.config.name,
            "imported": true,
            "criteria": idx.criteria_names,
            "eventTypes": idx.event_type_names,
            "totalBooks": idx.books.len(),
        }));
    }
    Json(json!({ "game": name, "modes": modes }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub game: String,
    /// Empty → all modes.
    #[serde(default)]
    pub mode: Option<String>,
    /// Bounds in bet multiplier (inclusive).
    #[serde(default)]
    pub payout_min: Option<f64>,
    #[serde(default)]
    pub payout_max: Option<f64>,
    #[serde(default)]
    pub criteria: Option<String>,
    /// The book must contain ALL of these event types.
    #[serde(default)]
    pub event_types: Vec<String>,
    #[serde(default)]
    pub min_fs_triggers: Option<u16>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchHit {
    mode: String,
    sim_id: u64,
    payout_x100: u64,
    criteria: Option<String>,
    fs_triggers: u16,
    one_in: f64,
}

async fn search(
    State(state): State<AppState>,
    Json(q): Json<SearchQuery>,
) -> Json<serde_json::Value> {
    let Some(game) = state.registry.get(&q.game) else {
        return Json(json!({ "error": "unknown game" }));
    };
    let min_x100 = q.payout_min.map(|v| (v * 100.0).round() as u64);
    let max_x100 = q.payout_max.map(|v| (v * 100.0).round() as u64);
    let limit = q.limit.min(500);

    let mut total = 0usize;
    let mut hits: Vec<SearchHit> = Vec::new();
    let mut not_imported: Vec<String> = Vec::new();

    for m in &game.modes {
        if let Some(want) = &q.mode {
            if !m.config.name.eq_ignore_ascii_case(want) {
                continue;
            }
        }
        let Ok(books) = state.books.mode_books(&game, &m.config.name) else {
            not_imported.push(m.config.name.clone());
            continue;
        };
        let idx = &books.index;
        // Weight per sim_id for the "1 in N" frequency of hits.
        let lut = &m.lut;
        let total_weight = lut.total_weight as f64;

        // Required bits for the requested event types (in THIS mode).
        let mut required_bits: u64 = 0;
        let mut unknown_type = false;
        for t in &q.event_types {
            match idx
                .event_type_names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(t))
            {
                Some(i) => required_bits |= 1u64 << i,
                None => unknown_type = true,
            }
        }
        if unknown_type {
            continue; // this mode doesn't have that event type → no hits
        }
        let criteria_idx = q.criteria.as_ref().map(|c| {
            idx.criteria_names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(c))
                .map(|i| i as u16)
        });
        if matches!(criteria_idx, Some(None)) {
            continue;
        }
        let criteria_idx = criteria_idx.flatten();

        for b in &idx.books {
            if let Some(min) = min_x100 {
                if b.payout_x100 < min {
                    continue;
                }
            }
            if let Some(max) = max_x100 {
                if b.payout_x100 > max {
                    continue;
                }
            }
            if let Some(ci) = criteria_idx {
                if b.criteria != ci {
                    continue;
                }
            }
            if required_bits != 0 && (b.event_bits & required_bits) != required_bits {
                continue;
            }
            if let Some(minfs) = q.min_fs_triggers {
                if b.fs_triggers < minfs {
                    continue;
                }
            }
            if total >= q.offset && hits.len() < limit {
                let weight = lut
                    .rows
                    .iter()
                    .find(|r| r.sim_id == b.sim_id)
                    .map(|r| r.weight)
                    .unwrap_or(0);
                hits.push(SearchHit {
                    mode: m.config.name.clone(),
                    sim_id: b.sim_id,
                    payout_x100: b.payout_x100,
                    criteria: idx
                        .criteria_names
                        .get(b.criteria as usize)
                        .cloned(),
                    fs_triggers: b.fs_triggers,
                    one_in: if weight > 0 {
                        total_weight / weight as f64
                    } else {
                        f64::INFINITY
                    },
                });
            }
            total += 1;
        }
    }

    Json(json!({
        "total": total,
        "hits": hits,
        "notImported": not_imported,
    }))
}
