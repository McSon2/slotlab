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
    // Only books the published LUT can actually draw (weight > 0): a
    // weight-0 book exists in the books but a reviewer would never meet it.
    // Falls back to every winning book if none is drawable.
    let winning = |drawable_only: bool| -> Vec<(u64, u64, u64)> {
        lut.rows
            .iter()
            .filter(|r| r.payout_x100 > 0 && (!drawable_only || r.weight > 0))
            .map(|r| (r.payout_x100, r.weight, r.sim_id))
            .collect()
    };
    let mut nonzero = winning(true);
    if nonzero.is_empty() {
        nonzero = winning(false);
    }
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

/// One line of the block. `one_in` is infinite when the picked book carries
/// a zero weight in the published LUT (it exists in the books, the LUT just
/// never draws it) or when the mode has no winning book at all — neither is
/// worth printing as "~1/inf".
fn format_pick(label: &str, p: &EventPick) -> String {
    if p.payout == 0.0 && !p.one_in.is_finite() {
        return format!("  {label:<11} : no winning book in this mode\n");
    }
    let odds = if p.one_in.is_finite() {
        format!("~1/{:.0}", p.one_in)
    } else {
        "never drawn, LUT weight 0".to_string()
    };
    format!("  {label:<11} : event {}  ({:.2}x, {odds})\n", p.sim_id, p.payout)
}

/// Ready-to-paste text block for submission (one game = all its modes).
pub fn format_review_block(game: &str, picks: &[ReviewIds]) -> String {
    let mut out = format!("Event IDs for review — {game}\n");
    for p in picks {
        out.push_str(&format!("\n[{}]\n", p.mode));
        out.push_str(&format_pick("average win", &p.average));
        out.push_str(&format_pick("high win", &p.high));
        out.push_str(&format_pick("max win", &p.max_win));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(sim_id: u64, payout: f64, one_in: f64) -> EventPick {
        EventPick { sim_id, payout, one_in }
    }

    #[test]
    fn block_never_prints_inf() {
        let ids = ReviewIds {
            mode: "base".into(),
            average: pick(74417, 1.0, f64::INFINITY),
            high: pick(12, 250.0, 3816.2),
            max_win: pick(0, 0.0, f64::INFINITY),
        };
        let text = format_review_block("demo", &[ids]);
        assert!(!text.contains("inf"), "{text}");
        assert!(text.contains("  average win : event 74417  (1.00x, never drawn, LUT weight 0)\n"));
        assert!(text.contains("  high win    : event 12  (250.00x, ~1/3816)\n"));
        assert!(text.contains("  max win     : no winning book in this mode\n"));
    }
}
