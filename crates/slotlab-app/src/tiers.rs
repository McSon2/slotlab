//! Celebration tiers, per mode, measured on the published LUT.
//!
//! The ladder is READ FROM THE GAME whenever the game has one
//! (`src/game/winLevelThresholds.ts` for the steps, `winLevelMap.ts` for the
//! hold durations) — the point of the panel is to tell the truth about the
//! build that is about to be published, not about a plausible default. When no
//! frontend is registered, the studio defaults stand in, and the report says
//! so.

use crate::api::routes::ApiError;
use crate::registry::LoadedGame;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use slotlab_core::tiers::{self, TierSpec};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/ui/games/{name}/tiers",
        get(game_tiers).post(custom_tiers),
    )
}

/// Ladders declared by the game, by upper-case mode name, plus the durations.
struct Ladders {
    by_mode: Vec<(String, Vec<f64>)>,
    durations: Option<Vec<f64>>,
    source: String,
}

fn read_ladders(game: &LoadedGame) -> Ladders {
    let mut out = Ladders {
        by_mode: Vec::new(),
        durations: None,
        source: "SlotLab defaults".to_string(),
    };
    let Some(app) = game.entry.app_dir.as_ref() else {
        return out;
    };
    let thresholds_file = app.join("src/game/winLevelThresholds.ts");
    if let Ok(src) = std::fs::read_to_string(&thresholds_file) {
        let rows = tiers::parse_basegame_steps(&src);
        if !rows.is_empty() {
            out.by_mode = rows;
            out.source = "winLevelThresholds.ts".to_string();
        }
    }
    if let Ok(src) = std::fs::read_to_string(app.join("src/game/winLevelMap.ts")) {
        let d = tiers::parse_present_durations(&src);
        if d.len() >= 2 {
            out.durations = Some(d);
        }
    }
    out
}

/// The game's max win, as the frontend uses it: an absolute fact of the game,
/// taken from the BASE mode and never scaled per mode.
fn max_win_of(game: &LoadedGame) -> f64 {
    game.base_mode()
        .map(|m| {
            m.lut
                .rows
                .iter()
                .map(|r| r.payout_x100)
                .max()
                .unwrap_or(0) as f64
                / 100.0
        })
        .unwrap_or(0.0)
}

fn spec_for(ladders: &Ladders, mode: &str, max_win: f64) -> (TierSpec, String) {
    let upper = mode.to_uppercase();
    let steps = ladders
        .by_mode
        .iter()
        .find(|(k, _)| *k == upper)
        .or_else(|| ladders.by_mode.iter().find(|(k, _)| k == "DEFAULT"))
        .map(|(_, v)| v.clone());
    let source = match (&steps, ladders.by_mode.iter().any(|(k, _)| *k == upper)) {
        (Some(_), true) => format!("{} · {upper}", ladders.source),
        (Some(_), false) => format!("{} · DEFAULT", ladders.source),
        (None, _) => "SlotLab defaults".to_string(),
    };
    let mut spec = match steps {
        Some(s) => TierSpec::from_steps(&s, max_win),
        None => TierSpec::default_for(max_win),
    };
    if let Some(d) = &ladders.durations {
        if d.len() == spec.thresholds.len() {
            spec.durations = d.clone();
        }
    }
    (spec, source)
}

async fn game_tiers(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let ladders = read_ladders(&game);
    let max_win = max_win_of(&game);
    let modes: Vec<serde_json::Value> = game
        .modes
        .iter()
        .map(|m| {
            let (spec, source) = spec_for(&ladders, &m.config.name, max_win);
            let report = tiers::analyze(&m.lut, &m.config.name, m.config.cost, &spec, &source);
            json!({ "report": report, "spec": spec })
        })
        .collect();
    Ok(Json(json!({
        "game": name,
        "maxWin": max_win,
        "ladderSource": ladders.source,
        "modes": modes,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomTiers {
    mode: String,
    /// Low bounds of tiers 2..n-1, in multiples of the base bet — exactly the
    /// eight numbers of a `winLevelThresholds.ts` row.
    steps: Vec<f64>,
    /// Optional hold durations, one per tier (tier 1 included).
    #[serde(default)]
    durations: Option<Vec<f64>>,
}

/// Same measurement on a ladder typed in the UI: the answer to "what does this
/// mode look like if BIG WIN moves to 2 500x?" without touching the game.
async fn custom_tiers(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<CustomTiers>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;
    let mode = game
        .resolve_mode(&body.mode)
        .ok_or_else(|| ApiError::not_found(format!("unknown mode: {}", body.mode)))?;
    let steps: Vec<f64> = body
        .steps
        .into_iter()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .collect();
    if steps.len() < 2 {
        return Err(ApiError::bad_request("a ladder needs at least two steps"));
    }
    let mut spec = TierSpec::from_steps(&steps, max_win_of(&game));
    if let Some(d) = body.durations {
        if d.len() == spec.thresholds.len() {
            spec.durations = d;
        }
    }
    let report = tiers::analyze(
        &mode.lut,
        &mode.config.name,
        mode.config.cost,
        &spec,
        "custom",
    );
    Ok(Json(json!({ "report": report, "spec": spec })))
}
