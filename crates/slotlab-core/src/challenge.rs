//! Challenge calculator — sizing Stake "challenges" (reach a target win
//! multiplier at a minimum bet, fixed award, claimMax=1) from the mode's
//! real published LUT distribution.
//!
//! Model (per mode):
//! - Challenge multiplier of a round = payout / round stake. LUT payouts are
//!   in base-bet multiples, a round of this mode stakes `cost` base bets →
//!   multiplier = payout / cost. (This is also why bonus buys can't snipe a
//!   base-game challenge: their stake includes the buy cost.)
//! - A hunter grinding at `min_bet` needs on average `1/p` rounds; their
//!   expected loss ("grind cost") = turnover × house edge.
//! - Boost = award / expected turnover: the RTP points the award adds to the
//!   hunter's grind (StakeCruncher's "boost"). The challenge is +EV for them
//!   iff boost > house edge.
//! - The operator collects edge on ALL turnover and pays the award once →
//!   break-even turnover = award / edge.

use crate::model::Lut;
use serde::Serialize;

/// Hunter attractiveness zone, from `ev = boost / house edge` (= award /
/// expected grind cost). Sweet spot = attractive enough to be hunted, hard
/// enough that the grind volume funds a large share of the award.
pub fn verdict(ev: f64) -> &'static str {
    if !ev.is_finite() || ev < 1.0 {
        "dead" // -EV: hunters skip it (pure showcase)
    } else if ev < 1.5 {
        "thin" // barely +EV: only some hunters bite
    } else if ev <= 3.0 {
        "sweet" // the zone: hunted AND largely self-funded
    } else if ev <= 5.0 {
        "generous" // hunted hard, claimed cheap: operator subsidizes
    } else {
        "giveaway" // sniped almost instantly, award ≈ pure cost
    }
}

/// Survival function of the challenge multiplier for one mode:
/// distinct payouts sorted ascending + suffix weight sums → P(mult ≥ x)
/// in O(log n).
pub struct Survival {
    /// Distinct payout_x100 values (base-bet multiples ×100), ascending.
    payouts: Vec<u64>,
    /// suffix[i] = Σ weight of rows with payout ≥ payouts[i].
    suffix: Vec<u64>,
    total: u64,
    cost: f64,
    rtp: f64,
}

impl Survival {
    pub fn build(lut: &Lut, cost: f64) -> Self {
        let cost = if cost > 0.0 { cost } else { 1.0 };
        let mut by_payout: std::collections::BTreeMap<u64, u64> = Default::default();
        let mut total: u64 = 0;
        let mut wp: f64 = 0.0;
        for r in &lut.rows {
            *by_payout.entry(r.payout_x100).or_insert(0) += r.weight;
            total += r.weight;
            wp += r.weight as f64 * (r.payout_x100 as f64 / 100.0);
        }
        let payouts: Vec<u64> = by_payout.keys().copied().collect();
        let weights: Vec<u64> = by_payout.values().copied().collect();
        let mut suffix = vec![0u64; payouts.len()];
        let mut acc = 0u64;
        for i in (0..payouts.len()).rev() {
            acc += weights[i];
            suffix[i] = acc;
        }
        let rtp = if total > 0 { wp / total as f64 / cost } else { 0.0 };
        Self { payouts, suffix, total, cost, rtp }
    }

    pub fn rtp(&self) -> f64 {
        self.rtp
    }

    pub fn house_edge(&self) -> f64 {
        (1.0 - self.rtp).max(0.0)
    }

    /// Highest reachable challenge multiplier (payout / cost).
    pub fn max_mult(&self) -> f64 {
        self.payouts
            .last()
            .map(|&p| p as f64 / 100.0 / self.cost)
            .unwrap_or(0.0)
    }

    /// P(round multiplier ≥ mult), mult in round-stake multiples.
    pub fn prob_at_least(&self, mult: f64) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        // payout_x100 ≥ mult × cost × 100 (ceil to stay conservative on
        // non-representable floats).
        let threshold = (mult * self.cost * 100.0).ceil() as u64;
        let i = self.payouts.partition_point(|&p| p < threshold);
        if i >= self.payouts.len() {
            return 0.0;
        }
        self.suffix[i] as f64 / self.total as f64
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetInfo {
    pub target: f64,
    pub probability: f64,
    /// 1 / probability (∞ serialized as null via Option below is avoided:
    /// unreachable targets get probability 0 and one_in = f64::MAX-ish; the
    /// frontend treats prob 0 as "never").
    pub one_in: f64,
    /// Expected rounds for one lone hunter to hit the target once.
    pub expected_rounds: f64,
    /// expected_rounds × min_bet.
    pub expected_turnover: f64,
    /// Expected loss of that lone grind = expected_turnover × edge.
    /// Symmetric: it is also the operator's expected income from the grind.
    pub hunter_grind_cost: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedTarget {
    pub target: f64,
    pub probability: f64,
    pub one_in: f64,
    /// award / expected turnover (0.004 = +0.4 % RTP).
    pub boost: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwardInfo {
    pub award: f64,
    /// Turnover at which the operator's edge income equals the award.
    pub break_even_turnover: f64,
    /// break_even_turnover / min_bet.
    pub break_even_rounds: f64,
    /// Largest "nice" target whose boost is still ≥ 1.5 × edge — the hardest
    /// still-attractive target (max grind per claim → best for the operator).
    pub suggested: Option<SuggestedTarget>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChallengeReport {
    pub mode: String,
    pub cost: f64,
    pub rtp: f64,
    pub house_edge: f64,
    pub min_bet: f64,
    pub max_mult: f64,
    pub max_mult_one_in: f64,
    pub targets: Vec<TargetInfo>,
    pub awards: Vec<AwardInfo>,
    /// boost_matrix[t][a] = award[a] / expected_turnover(target[t]) — the RTP
    /// the award adds to the grind (0.004 = +0.4 %); 0 if unreachable.
    pub boost_matrix: Vec<Vec<f64>>,
    /// Same shape: "dead" | "thin" | "sweet" | "generous" | "giveaway".
    pub verdict_matrix: Vec<Vec<&'static str>>,
}

pub const DEFAULT_MIN_BET: f64 = 0.05;
pub const DEFAULT_AWARDS: [f64; 5] = [15.0, 35.0, 50.0, 88.0, 100.0];

/// Standard target ladder (100x → 25000x) incl. the 888 brand number.
pub fn default_targets(max_mult: f64) -> Vec<f64> {
    [
        100.0, 250.0, 300.0, 400.0, 500.0, 600.0, 750.0, 888.0, 1000.0,
        1200.0, 1500.0, 2000.0, 2500.0, 3000.0, 5000.0, 10000.0, 25000.0,
    ]
    .into_iter()
    .filter(|&t| t <= max_mult)
    .collect()
}

/// "Nice" candidates the suggestion scans (marketing-friendly numbers).
fn nice_targets(max_mult: f64) -> Vec<f64> {
    let mut v = Vec::new();
    let mut push = |x: f64| {
        if x >= 100.0 && x <= max_mult {
            v.push(x);
        }
    };
    let mut x = 100.0;
    while x <= 1000.0 {
        push(x);
        x += 50.0;
    }
    let mut x = 1100.0;
    while x <= 3000.0 {
        push(x);
        x += 100.0;
    }
    let mut x = 3500.0;
    while x <= 10000.0 {
        push(x);
        x += 500.0;
    }
    let mut x = 12500.0;
    while x <= 50000.0 {
        push(x);
        x += 2500.0;
    }
    push(888.0);
    push(1888.0);
    push(8888.0);
    v.sort_by(|a, b| a.total_cmp(b));
    v.dedup();
    v
}

pub fn analyze(
    lut: &Lut,
    mode: &str,
    cost: f64,
    min_bet: f64,
    awards: &[f64],
    targets: Option<&[f64]>,
) -> ChallengeReport {
    let s = Survival::build(lut, cost);
    let edge = s.house_edge().max(1e-9);
    let min_bet = if min_bet > 0.0 { min_bet } else { DEFAULT_MIN_BET };
    let max_mult = s.max_mult();

    let targets: Vec<f64> = match targets {
        Some(t) if !t.is_empty() => t.to_vec(),
        _ => default_targets(max_mult),
    };

    let target_infos: Vec<TargetInfo> = targets
        .iter()
        .map(|&t| {
            let p = s.prob_at_least(t);
            let rounds = if p > 0.0 { 1.0 / p } else { f64::INFINITY };
            let turnover = rounds * min_bet;
            TargetInfo {
                target: t,
                probability: p,
                one_in: rounds,
                expected_rounds: rounds,
                expected_turnover: turnover,
                hunter_grind_cost: turnover * edge,
            }
        })
        .collect();

    let boost_of = |award: f64, turnover: f64| {
        if turnover.is_finite() && turnover > 0.0 { award / turnover } else { 0.0 }
    };

    let boost_matrix: Vec<Vec<f64>> = target_infos
        .iter()
        .map(|t| awards.iter().map(|&a| boost_of(a, t.expected_turnover)).collect())
        .collect();
    let verdict_matrix: Vec<Vec<&'static str>> = boost_matrix
        .iter()
        .map(|row| row.iter().map(|&b| verdict(b / edge)).collect())
        .collect();

    let nice = nice_targets(max_mult);
    let award_infos: Vec<AwardInfo> = awards
        .iter()
        .map(|&award| {
            // Boost decreases as the target grows → the last nice target with
            // boost ≥ 1.5 × edge is the hardest still-attractive one.
            let mut suggested: Option<SuggestedTarget> = None;
            for &t in &nice {
                let p = s.prob_at_least(t);
                if p <= 0.0 {
                    continue;
                }
                let boost = boost_of(award, (1.0 / p) * min_bet);
                if boost / edge >= 1.5 {
                    suggested = Some(SuggestedTarget {
                        target: t,
                        probability: p,
                        one_in: 1.0 / p,
                        boost,
                    });
                }
            }
            AwardInfo {
                award,
                break_even_turnover: award / edge,
                break_even_rounds: award / edge / min_bet,
                suggested,
            }
        })
        .collect();

    let p_max = s.prob_at_least(max_mult);
    ChallengeReport {
        mode: mode.to_string(),
        cost,
        rtp: s.rtp(),
        house_edge: s.house_edge(),
        min_bet,
        max_mult,
        max_mult_one_in: if p_max > 0.0 { 1.0 / p_max } else { f64::INFINITY },
        targets: target_infos,
        awards: award_infos,
        boost_matrix,
        verdict_matrix,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LutRow;

    fn lut(rows: &[(u64, u64)]) -> Lut {
        Lut::new(
            rows.iter()
                .enumerate()
                .map(|(i, &(w, p))| LutRow { sim_id: i as u64, weight: w, payout_x100: p })
                .collect(),
        )
    }

    #[test]
    fn survival_basic() {
        // 90% → 0, 9% → 1x, 1% → 100x ⇒ RTP = 0.09 + 1.0 = 1.09? no:
        // payouts: 0, 100 (=1x), 10000 (=100x); weights 90, 9, 1.
        let l = lut(&[(90, 0), (9, 100), (1, 10000)]);
        let s = Survival::build(&l, 1.0);
        assert!((s.prob_at_least(1.0) - 0.10).abs() < 1e-12);
        assert!((s.prob_at_least(100.0) - 0.01).abs() < 1e-12);
        assert_eq!(s.prob_at_least(100.01), 0.0);
        assert!((s.max_mult() - 100.0).abs() < 1e-12);
        // RTP = (9·1 + 1·100) / 100 = 1.09
        assert!((s.rtp() - 1.09).abs() < 1e-12);
    }

    #[test]
    fn cost_scales_multiplier() {
        // A 100x-cost mode paying 96x of base bet is a 0.96 round multiplier.
        let l = lut(&[(1, 9600)]);
        let s = Survival::build(&l, 100.0);
        assert!((s.max_mult() - 0.96).abs() < 1e-12);
        assert_eq!(s.prob_at_least(1.0), 0.0);
        assert!((s.prob_at_least(0.96) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn ev_and_verdicts() {
        // p(≥500x) = 1/10000, edge forced via zero-heavy LUT.
        // weights: 9999 → 0x, 1 → 500x ⇒ RTP = 500/10000 = 0.05, edge 0.95.
        let l = lut(&[(9999, 0), (1, 50000)]);
        let r = analyze(&l, "BASE", 1.0, 0.05, &[100.0], Some(&[500.0]));
        let t = &r.targets[0];
        assert!((t.probability - 1e-4).abs() < 1e-12);
        // grind = 10000 × 0.05 × 0.95 = 475; boost = 100 / 500 = 20 % RTP,
        // below the 95 % edge → dead.
        assert!((t.hunter_grind_cost - 475.0).abs() < 1e-9);
        assert!((r.boost_matrix[0][0] - 0.2).abs() < 1e-12);
        assert_eq!(r.verdict_matrix[0][0], "dead");
        assert_eq!(verdict(2.0), "sweet");
        assert_eq!(verdict(1.2), "thin");
        assert_eq!(verdict(4.0), "generous");
        assert_eq!(verdict(9.0), "giveaway");
    }

    #[test]
    fn suggestion_is_hardest_attractive() {
        // Rich distribution: p(≥t) = c/t for nice targets (synthetic).
        // 1000 rows won't be needed — craft: 100x:1/100, 500x:1/1000,
        // 1000x:1/10000 with mostly zeros, RTP small → edge ~1.
        let l = lut(&[(989_899, 0), (10_000, 10_000), (100, 50_000), (1, 100_000)]);
        let r = analyze(&l, "BASE", 1.0, 0.01, &[50.0], Some(&[100.0]));
        let sug = r.awards[0].suggested.as_ref().expect("has suggestion");
        // boost at the suggested target must be ≥ 1.5 × edge (hardest
        // still-attractive).
        assert!(sug.boost / r.house_edge >= 1.5);
    }
}
