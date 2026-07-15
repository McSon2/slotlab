use super::session::ActiveRound;
use super::types::*;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use rand::Rng;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/rgs/{game}/wallet/authenticate", post(authenticate))
        .route("/api/rgs/{game}/wallet/balance", post(balance))
        .route("/api/rgs/{game}/wallet/play", post(play))
        .route("/api/rgs/{game}/wallet/end-round", post(end_round))
        .route("/api/rgs/{game}/bet/event", post(bet_event))
        .route(
            "/api/rgs/{game}/bet/replay/{rgame}/{version}/{mode}/{event}",
            get(replay),
        )
}

fn err_response(code: &'static str, msg: impl Into<String>) -> Json<Value> {
    Json(json!({ "status": StatusObject::err(code, msg) }))
}

fn round_json(r: &ActiveRound) -> Value {
    json!({
        "roundID": r.round_id,
        "amount": r.amount,
        "payout": r.payout,
        "payoutMultiplier": r.payout_multiplier,
        "active": r.active,
        "mode": r.mode,
        "event": r.event,
        "state": &*r.state,
    })
}

async fn authenticate(
    State(state): State<AppState>,
    Path(game_name): Path<String>,
    Json(req): Json<ReqAuthenticate>,
) -> Json<Value> {
    let Some(game) = state.registry.get(&game_name) else {
        return err_response("ERR_ATE", format!("unknown game: {game_name}"));
    };
    let session = state.sessions.update(&req.session_id, &game_name, |s| {
        if let Some(lang) = &req.language {
            s.language = lang.clone();
        }
    });

    let mut bet_modes = serde_json::Map::new();
    for m in &game.modes {
        bet_modes.insert(
            m.config.name.clone(),
            serde_json::to_value(BetModeConfig {
                mode: m.config.name.clone(),
                cost_multiplier: m.config.cost,
                feature: m.config.cost > 2.0,
            })
            .unwrap(),
        );
    }
    let bet_levels = default_bet_levels();
    let config = ConfigObject {
        min_bet: *bet_levels.first().unwrap(),
        max_bet: *bet_levels.last().unwrap(),
        step_bet: 10_000,
        default_bet_level: 1_000_000,
        bet_levels,
        bet_modes,
        jurisdiction: Jurisdiction::new(session.social),
    };

    Json(json!({
        "status": StatusObject::success(),
        "balance": BalanceObject {
            amount: session.balance_micro,
            currency: session.currency.clone(),
        },
        "round": session.round.as_ref().filter(|r| r.active).map(round_json),
        "config": config,
    }))
}

async fn balance(
    State(state): State<AppState>,
    Path(game_name): Path<String>,
    Json(req): Json<ReqBalance>,
) -> Json<Value> {
    let session = state.sessions.get_or_create(&req.session_id, &game_name);
    Json(json!({
        "status": StatusObject::success(),
        "balance": BalanceObject {
            amount: session.balance_micro,
            currency: session.currency,
        },
    }))
}

async fn play(
    State(state): State<AppState>,
    Path(game_name): Path<String>,
    Json(req): Json<ReqPlay>,
) -> Json<Value> {
    let Some(game) = state.registry.get(&game_name) else {
        return err_response("ERR_GE", format!("unknown game: {game_name}"));
    };
    // Case-insensitive mode resolution (BASE vs base gotcha).
    let Some(mode) = game.resolve_mode(&req.mode) else {
        return err_response(
            "ERR_GLE",
            format!("unknown mode: {} (modes: {})", req.mode,
                game.modes.iter().map(|m| m.config.name.as_str())
                    .collect::<Vec<_>>().join(", ")),
        );
    };
    if req.amount == 0 {
        return err_response("ERR_IS", "amount is zero");
    }

    // Actual spin cost = bet × the mode's costMultiplier.
    let cost_micro = ((req.amount as u128 * (mode.config.cost * 100.0).round() as u128)
        / 100) as u64;

    let session = state.sessions.get_or_create(&req.session_id, &game_name);
    if session.balance_micro < cost_micro {
        return err_response("ERR_IPB", "insufficient balance");
    }

    // Forced book (session > game) if the mode matches, otherwise weighted draw.
    let forced = session
        .force
        .clone()
        .or_else(|| state.sessions.game_force(&game_name))
        .filter(|(m, _)| mode.config.name.eq_ignore_ascii_case(m)
            || mode.config.aliases.iter().any(|a| a.eq_ignore_ascii_case(m)));
    let was_forced = forced.is_some();
    let (sim_id, payout_x100) = match forced {
        Some((_, sim)) => {
            match mode.lut.rows.iter().find(|r| r.sim_id == sim) {
                Some(row) => (row.sim_id, row.payout_x100),
                None => {
                    return err_response(
                        "ERR_GLE",
                        format!("forced sim_id {sim} missing from LUT {}", mode.config.name),
                    )
                }
            }
        }
        None => {
            let roll = rand::thread_rng().gen_range(0..mode.lut.total_weight);
            let row = mode.lut.pick(roll);
            (row.sim_id, row.payout_x100)
        }
    };

    // Book: requires the import (block cache). Actionable message otherwise.
    let books = match state.books.mode_books(&game, &mode.config.name) {
        Ok(b) => b,
        Err(_) => {
            return err_response(
                "ERR_GE",
                format!(
                    "books for {game_name} not imported — SlotLab Games tab → Import books"
                ),
            )
        }
    };
    let book_state = match state.books.store.book_value(
        &game_name,
        &mode.config.name,
        &books,
        sim_id,
    ) {
        Ok(v) => v,
        Err(e) => return err_response("ERR_GE", format!("reading book {sim_id}: {e:#}")),
    };
    // `state` = the book's events array.
    let events: Arc<Value> = match book_state.get("events") {
        Some(ev) => Arc::new(ev.clone()),
        None => return err_response("ERR_GE", format!("book {sim_id} has no events")),
    };

    let payout_micro = ((req.amount as u128 * payout_x100 as u128) / 100) as u64;
    let payout_multiplier = payout_x100 as f64 / 100.0;

    let updated = state.sessions.update(&req.session_id, &game_name, |s| {
        if let Some(c) = &req.currency {
            s.currency = c.clone();
        }
        s.balance_micro = s.balance_micro.saturating_sub(cost_micro);
        s.round_counter += 1;
        s.round = Some(ActiveRound {
            round_id: s.round_counter,
            amount: req.amount,
            payout: payout_micro,
            payout_multiplier,
            mode: mode.config.name.clone(),
            sim_id,
            state: events,
            event: "0".into(),
            active: true,
        });
    });

    let round = updated.round.as_ref().unwrap();
    // Live feed UI (non-blocking, ignored if no one is listening).
    let _ = state.events_tx.send(
        json!({
            "type": "spin",
            "game": game_name,
            "sessionID": req.session_id,
            "mode": mode.config.name,
            "simId": sim_id,
            "payoutX100": payout_x100,
            "amount": req.amount,
            "forced": was_forced,
        })
        .to_string(),
    );
    Json(json!({
        "status": StatusObject::success(),
        "balance": BalanceObject {
            amount: updated.balance_micro,
            currency: updated.currency.clone(),
        },
        "round": round_json(round),
    }))
}

async fn end_round(
    State(state): State<AppState>,
    Path(game_name): Path<String>,
    Json(req): Json<ReqEndRound>,
) -> Json<Value> {
    let updated = state.sessions.update(&req.session_id, &game_name, |s| {
        if let Some(r) = &mut s.round {
            if r.active {
                s.balance_micro = s.balance_micro.saturating_add(r.payout);
                r.active = false;
            }
        }
    });
    Json(json!({
        "status": StatusObject::success(),
        "balance": BalanceObject {
            amount: updated.balance_micro,
            currency: updated.currency,
        },
    }))
}

async fn bet_event(
    State(state): State<AppState>,
    Path(game_name): Path<String>,
    Json(req): Json<ReqEvent>,
) -> Json<Value> {
    let event = req.event.clone();
    state.sessions.update(&req.session_id, &game_name, |s| {
        if let Some(r) = &mut s.round {
            r.event = req.event.clone();
        }
    });
    Json(json!({ "status": StatusObject::success(), "event": event }))
}

/// GET /api/rgs/{game}/bet/replay/{rgame}/{version}/{mode}/{event}
/// The response is spread by the game into `betToResume`: flat round fields.
async fn replay(
    State(state): State<AppState>,
    Path((game_name, _rgame, _version, mode_name, event)): Path<(
        String,
        String,
        String,
        String,
        String,
    )>,
) -> Json<Value> {
    let Some(game) = state.registry.get(&game_name) else {
        return err_response("ERR_GE", format!("unknown game: {game_name}"));
    };
    let Some(mode) = game.resolve_mode(&mode_name) else {
        return err_response("ERR_GLE", format!("unknown mode: {mode_name}"));
    };
    let Ok(sim_id) = event.parse::<u64>() else {
        return err_response("ERR_GLE", format!("invalid event id: {event}"));
    };
    let Some(row) = mode.lut.rows.iter().find(|r| r.sim_id == sim_id) else {
        return err_response("ERR_GLE", format!("event {sim_id} missing from LUT"));
    };
    let books = match state.books.mode_books(&game, &mode.config.name) {
        Ok(b) => b,
        Err(_) => {
            return err_response("ERR_GE", "books not imported — Games → Import books")
        }
    };
    let book = match state.books.store.book_value(
        &game_name,
        &mode.config.name,
        &books,
        sim_id,
    ) {
        Ok(v) => v,
        Err(e) => return err_response("ERR_GE", format!("{e:#}")),
    };
    let events = book.get("events").cloned().unwrap_or(Value::Array(vec![]));
    let amount: u64 = 1_000_000; // the real bet comes from the game's `amount` URL param
    let payout_multiplier = row.payout_x100 as f64 / 100.0;
    Json(json!({
        "roundID": sim_id,
        "amount": amount,
        "payout": ((amount as u128 * row.payout_x100 as u128) / 100) as u64,
        "payoutMultiplier": payout_multiplier,
        "mode": mode.config.name,
        "state": events,
    }))
}
