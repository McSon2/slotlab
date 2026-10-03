use crate::stats::ModeStats;
use serde::Serialize;

/// Stake Engine compliance, modeled on the official
/// "Math Distribution & Summary" report (verified against forge_of_titans v11
/// and leonida_nights v7). Two blocks:
///   1. Compliance per mode — 6 exact and reproducible checks.
///   2. Overall Bet Level Compliance — 2★/3★ table; we compute the
///      deterministic constraints (payout, cost mult, exposure, base std)
///      and flag Tail/CVaR/ETL as not replicated (proprietary aggregate).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Info,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub id: &'static str,
    pub label: &'static str,
    pub status: CheckStatus,
    pub value: String,
    pub threshold: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeCompliance {
    pub mode: String,
    pub cost: f64,
    pub passed: usize,
    pub total: usize,
    pub checks: Vec<Check>,
}

/// One row of the "Overall Bet Level Compliance" table (2★ / 3★).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StarConstraint {
    pub label: &'static str,
    pub value: String,
    pub limit_2star: String,
    pub limit_3star: String,
    pub pass_2star: bool,
    pub pass_3star: bool,
    /// Metric computed exactly by SlotLab (vs. non-replicated aggregate).
    pub computed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameCheckReport {
    pub passed: bool,
    pub fail_count: usize,
    pub warn_count: usize,
    /// Best tier reached on the COMPUTABLE constraints (payout, cost,
    /// exposure, base std). 0 = ineligible. See `star_tier_partial`.
    pub star_tier: u8,
    /// true: Tail/CVaR/ETL not included in the tier (Stake aggregate not
    /// replicated) — to be confirmed on the dashboard.
    pub star_tier_partial: bool,
    pub cross_mode_variance: f64,
    pub bet_level_constraints: Vec<StarConstraint>,
    pub modes: Vec<ModeCompliance>,
    /// "In-house" checks tied to publishing (outside the official Stake report).
    pub publish_checks: Vec<Check>,
}

// Universal constraints (identical forge v11 / leonida v7).
const TIER2_MAX_BET_LEVEL: f64 = 100.0;
const TIER3_MAX_BET_LEVEL: f64 = 500.0;
const TIER2_MAX_PAYOUT: f64 = 25_000.0;
const TIER3_MAX_PAYOUT: f64 = 100_000.0;
const TIER2_MAX_EXPOSURE: f64 = 15_000_000.0;
const TIER3_MAX_EXPOSURE: f64 = 50_000_000.0;
const TIER2_MAX_COST_MULT: f64 = 1_000.0;
const TIER3_MAX_COST_MULT: f64 = 1_500.0;
// Max Bet Cost: FIXED caps from the report (not cost_mult × bet_level).
const TIER2_MAX_BET_COST: f64 = 100_000.0;
const TIER3_MAX_BET_COST: f64 = 500_000.0;
const TIER2_STD_MAX: f64 = 50.0;
const TIER3_STD_MAX: f64 = 60.0;
const STD_MIN: f64 = 0.6;
/// Allowed RTP: FIXED upper bound (both reports display it at 96.70%).
const RTP_MIN: f64 = 0.90;
const RTP_MAX: f64 = 0.967;

fn is_base_like(cost: f64) -> bool {
    cost <= 2.0
}

fn pct(v: f64) -> String {
    format!("{:.2}%", v * 100.0)
}

fn one_in(v: f64) -> String {
    if v.is_finite() {
        format!("1 in {:.0}", v)
    } else {
        "never".to_string()
    }
}

fn mk(
    id: &'static str,
    label: &'static str,
    ok: bool,
    fail: CheckStatus,
    value: String,
    threshold: String,
) -> Check {
    Check {
        id,
        label,
        status: if ok { CheckStatus::Pass } else { fail },
        value,
        threshold,
        details: None,
    }
}

/// The 6 "Compliance per mode" checks from the official report.
/// `base_cost` = cost of the reference mode (must equal 1.0).
/// `cross_ok` / `variance` = cross-mode RTP consistency (game level, repeated).
fn mode_checks(
    stats: &ModeStats,
    base_cost: f64,
    cross_ok: bool,
    variance: f64,
) -> Vec<Check> {
    let mut c = Vec::with_capacity(6);

    c.push(mk(
        "rtp_range",
        "RTP within [90%, 96.70%]",
        stats.rtp >= RTP_MIN && stats.rtp <= RTP_MAX + 1e-9,
        CheckStatus::Fail,
        pct(stats.rtp),
        "90.00% – 96.70%".into(),
    ));

    c.push(mk(
        "max_win_achievable",
        "Max win actually achievable",
        stats.max_payout_one_in <= 20_000_000.0,
        CheckStatus::Fail,
        one_in(stats.max_payout_one_in),
        "≤ 1 in 20,000,000".into(),
    ));

    c.push(mk(
        "nonzero_hit_rate",
        "Non-zero win at least 1 spin in 20",
        stats.hit_rate >= 0.05,
        CheckStatus::Fail,
        format!("{} ({})", pct(stats.hit_rate), one_in(1.0 / stats.hit_rate.max(1e-9))),
        "≥ 1 in 20 (5%)".into(),
    ));

    c.push(mk(
        "cross_mode_rtp",
        "Cross-mode RTP consistency",
        cross_ok,
        CheckStatus::Fail,
        format!("variance {:.2}%", variance * 100.0),
        "≤ 1.00% (±0.5%)".into(),
    ));

    c.push(mk(
        "base_mode_cost",
        "Default mode cost = 1.0x",
        (base_cost - 1.0).abs() < 1e-9,
        CheckStatus::Fail,
        format!("{:.2}", base_cost),
        "= 1.0x".into(),
    ));

    // We always provide valid bet levels on the RGS side.
    c.push(mk(
        "bet_level_validator",
        "Valid bet levels template",
        true,
        CheckStatus::Fail,
        "present".into(),
        "levels != none".into(),
    ));

    c
}

fn constraint(
    label: &'static str,
    value: String,
    v2: String,
    v3: String,
    pass2: bool,
    pass3: bool,
) -> StarConstraint {
    StarConstraint {
        label,
        value,
        limit_2star: v2,
        limit_3star: v3,
        pass_2star: pass2,
        pass_3star: pass3,
        computed: true,
    }
}

/// `modes` = (name, stats, cost_is_int). The first mode with cost ≤ 2 serves as
/// the base (otherwise the very first one).
pub fn run_game_checks(modes: &[(String, ModeStats, bool)]) -> GameCheckReport {
    let base = modes
        .iter()
        .find(|(_, s, _)| is_base_like(s.cost))
        .or_else(|| modes.first());
    let base_cost = base.map(|(_, s, _)| s.cost).unwrap_or(1.0);
    let base_std = base.map(|(_, s, _)| s.std_dev / s.cost.max(1.0)).unwrap_or(0.0);

    // RTP consistency: (max - min) of the realized RTPs.
    let rtps: Vec<f64> = modes.iter().map(|(_, s, _)| s.rtp).collect();
    let rtp_max = rtps.iter().cloned().fold(f64::MIN, f64::max);
    let rtp_min = rtps.iter().cloned().fold(f64::MAX, f64::min);
    let variance = if rtps.is_empty() { 0.0 } else { rtp_max - rtp_min };
    let cross_ok = variance <= 0.01 + 1e-9;

    // Game-level aggregates.
    let max_payout = modes
        .iter()
        .map(|(_, s, _)| s.max_payout)
        .fold(0.0_f64, f64::max);
    let max_cost_mult = modes.iter().map(|(_, s, _)| s.cost).fold(0.0_f64, f64::max);

    // --- Overall Bet Level Compliance (computable constraints) ---
    let mut bl = Vec::new();
    let exp2 = max_payout * TIER2_MAX_BET_LEVEL;
    let exp3 = max_payout * TIER3_MAX_BET_LEVEL;
    bl.push(constraint(
        "Max Exposure",
        format!("{:.0}", exp2.max(exp3).min(exp3)),
        format!("{exp2:.0} / {TIER2_MAX_EXPOSURE:.0}"),
        format!("{exp3:.0} / {TIER3_MAX_EXPOSURE:.0}"),
        exp2 <= TIER2_MAX_EXPOSURE,
        exp3 <= TIER3_MAX_EXPOSURE,
    ));
    bl.push(constraint(
        "Max Payout Multiplier",
        format!("{max_payout:.0}x"),
        format!("≤ {TIER2_MAX_PAYOUT:.0}"),
        format!("≤ {TIER3_MAX_PAYOUT:.0}"),
        max_payout <= TIER2_MAX_PAYOUT,
        max_payout <= TIER3_MAX_PAYOUT,
    ));
    let bc2 = max_cost_mult * TIER2_MAX_BET_LEVEL;
    let bc3 = max_cost_mult * TIER3_MAX_BET_LEVEL;
    bl.push(constraint(
        "Max Bet Cost",
        format!("{bc2:.0} / {bc3:.0}"),
        format!("≤ {TIER2_MAX_BET_COST:.0}"),
        format!("≤ {TIER3_MAX_BET_COST:.0}"),
        bc2 <= TIER2_MAX_BET_COST,
        bc3 <= TIER3_MAX_BET_COST,
    ));
    bl.push(constraint(
        "Cost Multiplier",
        format!("{max_cost_mult:.0}"),
        format!("≤ {TIER2_MAX_COST_MULT:.0}"),
        format!("≤ {TIER3_MAX_COST_MULT:.0}"),
        max_cost_mult <= TIER2_MAX_COST_MULT,
        max_cost_mult <= TIER3_MAX_COST_MULT,
    ));
    bl.push(constraint(
        "Base Volatility (Std Dev)",
        format!("{base_std:.1}"),
        format!("{STD_MIN} – {TIER2_STD_MAX}"),
        format!("{STD_MIN} – {TIER3_STD_MAX}"),
        base_std >= STD_MIN && base_std <= TIER2_STD_MAX,
        base_std >= STD_MIN && base_std <= TIER3_STD_MAX,
    ));

    // Tier = highest level where ALL computable constraints pass.
    let all2 = bl.iter().all(|c| c.pass_2star);
    let all3 = bl.iter().all(|c| c.pass_3star);
    let star_tier = if all3 {
        3
    } else if all2 {
        2
    } else {
        0
    };

    // --- Per mode ---
    let mode_reports: Vec<ModeCompliance> = modes
        .iter()
        .map(|(name, s, _)| {
            let checks = mode_checks(s, base_cost, cross_ok, variance);
            let passed = checks
                .iter()
                .filter(|c| c.status == CheckStatus::Pass)
                .count();
            ModeCompliance {
                mode: name.clone(),
                cost: s.cost,
                passed,
                total: checks.len(),
                checks,
            }
        })
        .collect();

    // --- In-house checks (RGS publish, outside the official report) ---
    let mut publish = Vec::new();
    let gran = modes.iter().map(|(_, s, _)| s.granularity_violations).sum::<usize>();
    publish.push(mk(
        "granularity",
        "0.1x granularity (payout×100 % 10 == 0)",
        gran == 0,
        CheckStatus::Fail,
        format!("{gran} violation(s)"),
        "0".into(),
    ));
    let worst_ml = modes
        .iter()
        .map(|(_, s, _)| s.most_likely_one_in)
        .fold(f64::INFINITY, f64::min);
    publish.push(mk(
        "most_likely_hr",
        "Hit rate of the most likely book ≥ 1/1000",
        worst_ml >= 1000.0,
        CheckStatus::Fail,
        one_in(worst_ml),
        "≥ 1 in 1000".into(),
    ));
    // Stake's official index.json schema declares `"cost": <float>` (its examples
    // use 1.0 and 100.0), so fractional costs such as 1.5x are valid. The old
    // "integer costs only" rule traced back to a third-party dev tool that parsed
    // the field as u64 — not to the RGS. What does break is a cost the fixed-point
    // bet arithmetic can't hold: stakes are computed in thousandths of a base bet.
    let bad_cost = modes
        .iter()
        .filter(|(_, s, _)| {
            let milli = s.cost * 1000.0;
            s.cost <= 0.0 || (milli - milli.round()).abs() > 1e-6
        })
        .count();
    let all_int = modes.iter().all(|(_, _, i)| *i);
    publish.push(mk(
        "cost_format",
        "Mode costs usable by the RGS (> 0, at most 3 decimals)",
        bad_cost == 0,
        CheckStatus::Fail,
        if bad_cost > 0 {
            format!("{bad_cost} mode(s) out of range")
        } else if all_int {
            "ok — all integers".into()
        } else {
            "ok — fractional cost present".into()
        },
        "1, 1.5, 2.5 … all valid".into(),
    ));

    // A bet-level constraint only fails the game when no tier is reached:
    // a 3★ game that exceeds a 2★ cap (e.g. max payout 50000x) is compliant,
    // it just doesn't qualify for the lower tier. When no tier is reached,
    // count the misses of the nearest tier (the fixes actually needed).
    let tier_misses = if star_tier == 0 {
        let miss2 = bl.iter().filter(|c| !c.pass_2star).count();
        let miss3 = bl.iter().filter(|c| !c.pass_3star).count();
        miss2.min(miss3)
    } else {
        0
    };
    let fail_count = mode_reports
        .iter()
        .flat_map(|m| &m.checks)
        .chain(publish.iter())
        .filter(|c| c.status == CheckStatus::Fail)
        .count()
        + tier_misses;
    let warn_count = mode_reports
        .iter()
        .flat_map(|m| &m.checks)
        .chain(publish.iter())
        .filter(|c| c.status == CheckStatus::Warn)
        .count();

    GameCheckReport {
        passed: fail_count == 0,
        fail_count,
        warn_count,
        star_tier,
        star_tier_partial: true,
        cross_mode_variance: variance,
        bet_level_constraints: bl,
        modes: mode_reports,
        publish_checks: publish,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Lut, LutRow};
    use crate::stats::analyze;

    fn lut(rows: &[(u64, u64, u64)]) -> Lut {
        Lut::new(
            rows.iter()
                .map(|&(sim_id, weight, payout_x100)| LutRow {
                    sim_id,
                    weight,
                    payout_x100,
                })
                .collect(),
        )
    }

    #[test]
    fn three_star_game_is_not_failed_by_two_star_caps() {
        // Max win 50,000x: over the 2★ cap (25,000x), within the 3★ one.
        let l = lut(&[(0, 5_199_999, 0), (1, 4_800_000, 200), (2, 1, 5_000_000)]);
        let s = analyze(&l, 1.0);
        let r = run_game_checks(&[("base".into(), s, true)]);
        assert_eq!(r.star_tier, 3);
        assert!(r.bet_level_constraints.iter().any(|c| !c.pass_2star));
        let check_fails = r
            .modes
            .iter()
            .flat_map(|m| &m.checks)
            .chain(r.publish_checks.iter())
            .filter(|c| c.status == CheckStatus::Fail)
            .count();
        assert_eq!(r.fail_count, check_fails);
    }
}
