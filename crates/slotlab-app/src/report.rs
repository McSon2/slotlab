//! The same math, written for the person paying for the game.
//!
//! ── Why this is not the Analyze tab ─────────────────────────────────────────
//! A client does not read in fences, neg_pigs or av_win, and an intermediate
//! RTP over 100 % is a tuning state, not a result. Explaining a game in those
//! terms has cost the studio entire meetings. What a client asks is always the
//! same four questions: how often does it pay, how much when it does, how long
//! before something big happens, and what is the ceiling.
//!
//! So this endpoint answers those, in whole sentences, from exactly the numbers
//! the other panels compute — no second source of truth, no rounding of its own.

use crate::api::routes::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use slotlab_core::stats::ModeStats;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/ui/games/{name}/report", get(client_report))
}

/// "1 spin in 240" reads better than "0.42 %", and it is the only form a
/// player ever thinks in.
fn one_in(p: f64) -> String {
    if p <= 0.0 {
        return "never".into();
    }
    if p >= 0.995 {
        return "on every spin".into();
    }
    if p >= 0.9 {
        return "on almost every spin".into();
    }
    let n = 1.0 / p;
    if n < 10.0 {
        format!("about 1 spin in {n:.1}")
    } else if n < 100_000.0 {
        format!("about 1 spin in {}", round_nice(n))
    } else if n < 1e9 {
        format!("about 1 spin in {:.1} million", n / 1e6)
    } else {
        format!("about 1 spin in {:.1} billion", n / 1e9)
    }
}

/// Rounds to two significant-ish digits: nobody needs "1 in 237".
fn round_nice(n: f64) -> u64 {
    let mag = 10f64.powf(n.log10().floor() - 1.0).max(1.0);
    ((n / mag).round() * mag) as u64
}

fn money(x: f64) -> String {
    if x >= 100.0 {
        format!("{:.0}×", x)
    } else if x >= 10.0 {
        format!("{:.1}×", x)
    } else {
        format!("{:.2}×", x)
    }
}

/// The feel of a mode is a RELATIVE thing: `std ÷ cost` is the dashboard's
/// label and it collapses on an expensive mode — a 500× buy whose swings are
/// 500× wide scores the same as a penny spin. What the player feels is the
/// spread against the average round (the coefficient of variation), corrected
/// by how rarely the round comes out ahead at all.
fn feel(stats: &ModeStats) -> &'static str {
    let cv = stats.volatility;
    let rarely_ahead = stats.breakeven_rate < 0.2;
    if cv >= 20.0 {
        "extremely swingy: long dry runs, and the wins that matter are rare and large"
    } else if cv >= 8.0 || (rarely_ahead && cv >= 3.0) {
        "swingy: dry runs are part of the design, and the big wins pay for them"
    } else if cv >= 3.0 || rarely_ahead {
        "uneven: most rounds give something back without covering the spin, and the \
         real wins are what the mode is bought for"
    } else {
        "steady: wins come often and stay close to what the round cost"
    }
}

fn mode_section(name: &str, s: &ModeStats, base_cost: f64) -> String {
    let mut out = String::new();
    let price = if (s.cost - 1.0).abs() < 1e-9 {
        "one bet".to_string()
    } else {
        format!("{}× the bet", trim(s.cost))
    };
    out.push_str(&format!("### {name} — costs {price}\n\n"));

    let win_of_spins = one_in(s.hit_rate);
    let avg_win = if s.hit_rate > 0.0 {
        s.mean_payout / s.hit_rate
    } else {
        0.0
    };
    out.push_str(&format!(
        "- **Something comes back** {win_of_spins}, and when it does it is worth \
         {} on average.\n",
        money(avg_win)
    ));
    out.push_str(&format!(
        "- **You come out ahead on the round** {}.\n",
        one_in(s.breakeven_rate)
    ));

    // Landmarks a player actually notices, relative to what the spin costs.
    for (label, mult) in [("10×", 10.0), ("100×", 100.0), ("1 000×", 1000.0)] {
        let p = share_at_least(s, mult);
        if p > 0.0 {
            out.push_str(&format!(
                "- **A win of {label} the bet or more**: {}.\n",
                one_in(p)
            ));
        }
    }
    out.push_str(&format!(
        "- **The ceiling** is {} the bet, reached {}.\n",
        money(s.max_payout),
        one_in(if s.max_payout_one_in.is_finite() {
            1.0 / s.max_payout_one_in
        } else {
            0.0
        })
    ));
    out.push_str(&format!("- **How it feels**: {}.\n", feel(s)));
    if s.cost > base_cost * 1.5 {
        out.push_str(
            "- Being a paid entry, this mode wins far more often than the base game — \
             it is the price that makes the difference, not the luck.\n",
        );
    }
    out.push('\n');
    out
}

/// Share of spins paying at least `mult` times the bet, read off the buckets.
fn share_at_least(s: &ModeStats, mult: f64) -> f64 {
    s.buckets
        .iter()
        .filter(|b| b.start >= mult)
        .map(|b| b.probability)
        .sum()
}

fn trim(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v}")
    }
}

async fn client_report(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;

    let base_cost = game
        .base_mode()
        .map(|m| m.config.cost)
        .unwrap_or(1.0);

    let mut md = format!("# {name} — what the game does\n\n");
    let base = game.base_mode().map(|m| m.stats());
    if let Some(b) = &base {
        md.push_str(&format!(
            "Over a long session the game gives back **{:.2} %** of what is staked. \
             {} pays something, and the game is {}.\n\n",
            b.rtp * 100.0,
            capitalise(&one_in(b.hit_rate)),
            feel(b)
        ));
    }

    md.push_str("## The ways to play\n\n");
    let mut modes: Vec<_> = game.modes.iter().collect();
    modes.sort_by(|a, b| a.config.cost.total_cmp(&b.config.cost));
    for m in &modes {
        md.push_str(&mode_section(&m.config.name, m.stats(), base_cost));
    }

    // The bonus, if the books have been read: how often it opens and what it
    // is worth — the two questions that always follow.
    let mut bonus_lines = Vec::new();
    for m in &modes {
        let Ok(books) = state.books.mode_books(&game, &m.config.name) else {
            continue;
        };
        let total: u64 = m.lut.total_weight;
        let mut w_trigger = 0u64;
        let mut wp_trigger = 0.0f64;
        let by_sim: std::collections::HashMap<u64, u64> =
            m.lut.rows.iter().map(|r| (r.sim_id, r.weight)).collect();
        for b in &books.index.books {
            if b.fs_triggers == 0 {
                continue;
            }
            let w = by_sim.get(&b.sim_id).copied().unwrap_or(0);
            w_trigger += w;
            wp_trigger += w as f64 * (b.payout_x100 as f64 / 100.0);
        }
        if w_trigger == 0 {
            continue;
        }
        bonus_lines.push(format!(
            "- **{}**: the feature opens {}, and pays {} on average when it does.\n",
            m.config.name,
            one_in(w_trigger as f64 / total as f64),
            money(wp_trigger / w_trigger as f64)
        ));
    }
    if !bonus_lines.is_empty() {
        md.push_str("## The feature\n\n");
        for l in bonus_lines {
            md.push_str(&l);
        }
        md.push('\n');
    }

    md.push_str(
        "---\n\nAll the figures above are read from the published files: they are what \
         the game will actually do, not an estimate. Amounts are written as multiples \
         of the bet.\n",
    );

    Ok(Json(json!({ "game": name, "markdown": md })))
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
