use crate::model::Lut;
use serde::Serialize;

/// Selection of the Event IDs to provide to the Stake reviewer (convention:
/// `event` = simulation id = book `id`, 0-indexed).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewIds {
    pub mode: String,
    pub average: EventPick,
    pub high: EventPick,
    pub max_win: EventPick,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPick {
    pub sim_id: u64,
    /// Bet multiplier (payout_x100 / 100).
    pub payout: f64,
    pub one_in: f64,
}

pub fn pick_review_ids(mode: &str, lut: &Lut) -> ReviewIds {
    let t = lut.total_weight as f64;
    let mut sum_wp = 0.0_f64;
    for r in &lut.rows {
        sum_wp += r.weight as f64 * (r.payout_x100 as f64 / 100.0);
    }
    let mean = sum_wp / t;

    // average = WINNING book at the payout closest to the mean.
    let mut avg_pick = None;
    let mut best_diff = f64::INFINITY;
    // high = payout at the weighted 99.9th percentile of non-zero wins.
    let mut nonzero: Vec<(u64, u64, u64)> = lut
        .rows
        .iter()
        .filter(|r| r.payout_x100 > 0)
        .map(|r| (r.payout_x100, r.weight, r.sim_id))
        .collect();
    nonzero.sort_unstable_by_key(|&(p, _, _)| p);
    let nz_weight: u64 = nonzero.iter().map(|&(_, w, _)| w).sum();

    for &(p, w, sim_id) in &nonzero {
        let diff = (p as f64 / 100.0 - mean).abs();
        if diff < best_diff {
            best_diff = diff;
            avg_pick = Some(EventPick {
                sim_id,
                payout: p as f64 / 100.0,
                one_in: t / w as f64,
            });
        }
    }

    let high_target = nz_weight as f64 * 0.999;
    let mut acc = 0.0_f64;
    let mut high_pick = None;
    for &(p, w, sim_id) in &nonzero {
        acc += w as f64;
        if acc >= high_target {
            high_pick = Some(EventPick {
                sim_id,
                payout: p as f64 / 100.0,
                one_in: t / w as f64,
            });
            break;
        }
    }

    let max = nonzero.last().map(|&(p, w, sim_id)| EventPick {
        sim_id,
        payout: p as f64 / 100.0,
        one_in: t / w as f64,
    });

    let fallback = EventPick { sim_id: 0, payout: 0.0, one_in: f64::INFINITY };
    ReviewIds {
        mode: mode.to_string(),
        average: avg_pick.unwrap_or_else(|| fallback.clone()),
        high: high_pick.unwrap_or_else(|| fallback.clone()),
        max_win: max.unwrap_or(fallback),
    }
}

/// Ready-to-paste text block for submission (one game = all its modes).
pub fn format_review_block(game: &str, picks: &[ReviewIds]) -> String {
    let mut out = format!("Event IDs for review — {game}\n");
    for p in picks {
        out.push_str(&format!(
            "\n[{}]\n  average win : event {}  ({:.2}x, ~1/{:.0})\n  high win    : event {}  ({:.2}x, ~1/{:.0})\n  max win     : event {}  ({:.2}x, ~1/{:.0})\n",
            p.mode,
            p.average.sim_id, p.average.payout, p.average.one_in,
            p.high.sim_id, p.high.payout, p.high.one_in,
            p.max_win.sim_id, p.max_win.payout, p.max_win.one_in,
        ));
    }
    out
}
