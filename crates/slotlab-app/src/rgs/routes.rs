use super::faults::{Endpoint, FailureKind};
use super::pool::Pool;
use super::session::ActiveRound;
use super::types::*;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rand::Rng;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

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

fn json_ok(v: Value) -> Response {
    Json(v).into_response()
}

fn err_response(code: &'static str, msg: impl Into<String>) -> Response {
    json_ok(json!({ "status": StatusObject::err(code, msg) }))
}

/// Applies the game's deliberate faults to this call: the configured latency
/// first (it applies to answers that succeed too), then the failure rule, if
/// one is armed for this endpoint.
///
/// Returns the response to send INSTEAD of doing the work.
async fn faulted(state: &AppState, game: &str, endpoint: Endpoint) -> Option<Response> {
    let latency = state.faults.get(game).latency_ms;
    if latency > 0 {
        tokio::time::sleep(Duration::from_millis(latency.min(30_000))).await;
    }
    let rule = state.faults.take(game, endpoint)?;
    tracing::info!("fault: {endpoint:?} → {:?} {}", rule.kind, rule.code);
    Some(match rule.kind {
        FailureKind::RgsError => json_ok(json!({
            "status": StatusObject::err(rule.code.clone(), "SlotLab fault injection"),
        })),
        FailureKind::Http500 => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "SlotLab fault injection",
        )
            .into_response(),
        // Long enough that any watchdog worth having has fired.
        FailureKind::Hang => {
            tokio::time::sleep(Duration::from_secs(120)).await;
            (StatusCode::GATEWAY_TIMEOUT, "SlotLab fault injection").into_response()
        }
    })
}

/// A round as the RGS sends it. Borrows the book's events instead of copying
/// them into a `Value` tree: a bonus book is hundreds of KB of JSON.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RoundOut<'a> {
    #[serde(rename = "roundID")]
    round_id: u64,
    /// The drawn book (simulation id). SlotLab-only extension, ignored by the games:
    /// SSP keeps it to replay the round (`?replay=true&mode=…&event=<bookID>`).
    #[serde(rename = "bookID")]
    book_id: u64,
    amount: u64,
    payout: u64,
    payout_multiplier: f64,
    active: bool,
    mode: &'a str,
    event: &'a str,
    state: &'a Value,
}

fn round_json(r: &ActiveRound) -> RoundOut<'_> {
    RoundOut {
        round_id: r.round_id,
        book_id: r.sim_id,
        amount: r.amount,
        payout: r.payout,
        payout_multiplier: r.payout_multiplier,
        active: r.active,
        mode: &r.mode,
        event: &r.event,
        state: &r.state,
    }
}

async fn authenticate(
    State(state): State<AppState>,
    Path(game_name): Path<String>,
    Json(req): Json<ReqAuthenticate>,
) -> Response {
    if let Some(r) = faulted(&state, &game_name, Endpoint::Authenticate).await {
        return r;
    }
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

    json_ok(json!({
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
) -> Response {
    if let Some(r) = faulted(&state, &game_name, Endpoint::Balance).await {
        return r;
    }
    let session = state.sessions.get_or_create(&req.session_id, &game_name);
    json_ok(json!({
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
) -> Response {
    if let Some(r) = faulted(&state, &game_name, Endpoint::Play).await {
        return r;
    }
    let Some(game) = state.registry.get(&game_name) else {
        return err_response("ERR_GE", format!("unknown game: {game_name}"));
    };
    // Case-insensitive mode resolution (BASE vs base gotcha).
    let Some(mode) = game.resolve_mode(&req.mode) else {
        return err_response(
            "ERR_GLE",
            format!(
                "unknown mode: {} (modes: {})",
                req.mode,
                game.modes
                    .iter()
                    .map(|m| m.config.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    };
    if req.amount == 0 {
        return err_response("ERR_IS", "amount is zero");
    }

    // Actual spin cost = bet × the mode's costMultiplier.
    let cost_micro =
        ((req.amount as u128 * (mode.config.cost * 100.0).round() as u128) / 100) as u64;

    let session = state.sessions.get_or_create(&req.session_id, &game_name);
    if session.balance_micro < cost_micro {
        return err_response("ERR_IPB", "insufficient balance");
    }

    // Forced book (session > game) if the mode matches, otherwise weighted
    // draw. A Share session never inherits the GAME-level force: a book
    // pinned in the Play tab must not land in a client's spins — nor in the
    // stats of an external driver (SSP in dev, `EXTERNAL_SESSION_PREFIX`).
    let forced = session
        .force
        .clone()
        .or_else(|| {
            if session.plain_draw() {
                None
            } else {
                state.sessions.game_force(&game_name)
            }
        })
        .filter(|(m, _)| {
            mode.config.name.eq_ignore_ascii_case(m)
                || mode
                    .config
                    .aliases
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(m))
        });
    let was_forced = forced.is_some();
    // Un forçage de console ne vaut que pour CE tour.
    if was_forced && session.force.is_some() {
        state.sessions.consume_force(&req.session_id);
    }
    // No book pinned: the Play tab may still restrict the draw to the mode's
    // bonus books. Same Share exclusion as the pinned book.
    let pool = if forced.is_some() || session.plain_draw() {
        None
    } else {
        match state.sessions.game_pool(&game_name, &mode.config.name) {
            Some(p) => {
                if p.fingerprint == game.fingerprint {
                    Some(p)
                } else {
                    // The math was reloaded: rebuild on the new LUT and books.
                    match Pool::build(&state.books, &game, p.filter.clone()) {
                        Ok(fresh) => {
                            let fresh = Arc::new(fresh);
                            state.sessions.set_game_pool(
                                &game_name,
                                &mode.config.name,
                                Some(fresh.clone()),
                            );
                            Some(fresh)
                        }
                        Err(e) => {
                            return err_response(
                                "ERR_GE",
                                format!("bonus-only draw on {}: {e:#}", mode.config.name),
                            )
                        }
                    }
                }
            }
            None => None,
        }
    };
    let was_pooled = pool.is_some();
    let (sim_id, payout_x100) = match forced {
        Some((_, sim)) => match mode.lut.row(sim) {
            Some(row) => (row.sim_id, row.payout_x100),
            None => {
                return err_response(
                    "ERR_GLE",
                    format!("forced sim_id {sim} missing from LUT {}", mode.config.name),
                )
            }
        },
        None => {
            let lut = pool.as_ref().map_or(mode.lut.as_ref(), |p| &p.lut);
            let roll = rand::thread_rng().gen_range(0..lut.total_weight);
            let row = lut.pick(roll);
            (row.sim_id, row.payout_x100)
        }
    };

    // Book: requires the import (block cache). Actionable message otherwise.
    // Off the runtime: the first bet of a mode reads its whole index, and a
    // block miss decompresses 2 MiB.
    let (manager, g, m, gn) = (
        state.books.clone(),
        game.clone(),
        mode.config.name.clone(),
        game_name.clone(),
    );
    let events = tokio::task::spawn_blocking(move || {
        // ⚠️ La cause est jointe : « not imported » seul a masqué deux fois une
        // autre panne sur le hub (source `.zst` absente, `.idx` d'une autre version).
        let books = manager.mode_books(&g, &m).map_err(|e| {
            format!("books for {gn} not imported — SlotLab Games tab → Import books ({e:#})")
        })?;
        manager
            .store
            .book_events(&gn, &m, &books, sim_id)
            .map_err(|e| format!("reading book {sim_id}: {e:#}"))
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    // `state` = the book's events array.
    let events: Arc<Value> = match events {
        Ok(ev) => ev,
        Err(e) => return err_response("ERR_GE", e),
    };

    let payout_micro = ((req.amount as u128 * payout_x100 as u128) / 100) as u64;
    let payout_multiplier = payout_x100 as f64 / 100.0;

    let updated = state.sessions.update(&req.session_id, &game_name, |s| {
        if let Some(c) = &req.currency {
            s.currency = c.clone();
        }
        s.balance_micro = s.balance_micro.saturating_sub(cost_micro);
        s.wagered_micro = s.wagered_micro.saturating_add(cost_micro);
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
            "pooled": was_pooled,
        })
        .to_string(),
    );
    #[derive(serde::Serialize)]
    struct PlayOut<'a> {
        status: StatusObject,
        balance: BalanceObject,
        round: RoundOut<'a>,
    }
    Json(PlayOut {
        status: StatusObject::success(),
        balance: BalanceObject {
            amount: updated.balance_micro,
            currency: updated.currency.clone(),
        },
        round: round_json(round),
    })
    .into_response()
}

async fn end_round(
    State(state): State<AppState>,
    Path(game_name): Path<String>,
    Json(req): Json<ReqEndRound>,
) -> Response {
    if let Some(r) = faulted(&state, &game_name, Endpoint::EndRound).await {
        return r;
    }
    // The round a closed tab leaves behind: answered as if it had been
    // settled, still open and unpaid on the server. The next authenticate
    // hands it back, and the game must offer to watch or skip it — with the
    // bet of THAT round, not the one currently selected.
    let keep_open = state.faults.take_keep_round_open(&game_name);
    let updated = state.sessions.update(&req.session_id, &game_name, |s| {
        if let Some(r) = &mut s.round {
            if r.active && !keep_open {
                s.balance_micro = s.balance_micro.saturating_add(r.payout);
                s.won_micro = s.won_micro.saturating_add(r.payout);
                r.active = false;
            }
        }
    });
    json_ok(json!({
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
) -> Response {
    if let Some(r) = faulted(&state, &game_name, Endpoint::Event).await {
        return r;
    }
    let event = req.event.clone();
    state.sessions.update(&req.session_id, &game_name, |s| {
        if let Some(r) = &mut s.round {
            r.event = req.event.clone();
        }
    });
    json_ok(json!({ "status": StatusObject::success(), "event": event }))
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
) -> Response {
    if let Some(r) = faulted(&state, &game_name, Endpoint::Replay).await {
        return r;
    }
    let Some(game) = state.registry.get(&game_name) else {
        return err_response("ERR_GE", format!("unknown game: {game_name}"));
    };
    let Some(mode) = game.resolve_mode(&mode_name) else {
        return err_response("ERR_GLE", format!("unknown mode: {mode_name}"));
    };
    let Ok(sim_id) = event.parse::<u64>() else {
        return err_response("ERR_GLE", format!("invalid event id: {event}"));
    };
    let Some(row) = mode.lut.row(sim_id) else {
        return err_response("ERR_GLE", format!("event {sim_id} missing from LUT"));
    };
    let (manager, g, m, gn) = (
        state.books.clone(),
        game.clone(),
        mode.config.name.clone(),
        game_name.clone(),
    );
    let events = tokio::task::spawn_blocking(move || {
        let books = manager
            .mode_books(&g, &m)
            .map_err(|_| "books not imported — Games → Import books".to_string())?;
        manager.store.book_events(&gn, &m, &books, sim_id).map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    let events = match events {
        Ok(ev) => ev,
        Err(e) => return err_response("ERR_GE", e),
    };
    let amount: u64 = 1_000_000; // the real bet comes from the game's `amount` URL param
    let payout_multiplier = row.payout_x100 as f64 / 100.0;
    json_ok(json!({
        "roundID": sim_id,
        "amount": amount,
        "payout": ((amount as u128 * row.payout_x100 as u128) / 100) as u64,
        "payoutMultiplier": payout_multiplier,
        "mode": mode.config.name,
        "state": &*events,
    }))
}
