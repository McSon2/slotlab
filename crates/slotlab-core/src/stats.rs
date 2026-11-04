use crate::model::Lut;
use crate::session::{self, ShortSession};
use serde::Serialize;

/// Statistics of a mode, computed solely from its LUT.
/// Convention: `payout_x100 / 100` = BET multiplier (not cost); the RTP divides
/// by the mode cost (a 250x buy returning 241x the bet has a 96.4% RTP).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeStats {
    pub cost: f64,
    pub outcome_count: usize,
    pub total_weight: u64,
    pub rtp: f64,
    pub hit_rate: f64,
    pub zero_rate: f64,
    pub mean_payout: f64,
    pub median_payout: f64,
    pub min_nonzero_payout: f64,
    pub max_payout: f64,
    pub variance: f64,
    pub std_dev: f64,
    /// Coefficient of variation: std / mean.
    pub volatility: f64,
    /// std / cost — the definition of the Stake volatility label.
    pub cost_adj_volatility: f64,
    /// P(payout ≥ cost): probability of returning at least the mode's bet.
    pub breakeven_rate: f64,
    /// Weighted average of payouts over the top 0.1% slice of weight.
    pub cvar_001: f64,
    /// RTP contribution of wins ≥ 40x (÷ cost).
    pub etl_40x: f64,
    /// RTP contribution of wins ≥ 10,000x (÷ cost).
    pub etl_10kx: f64,
    pub prob_win_5k: f64,
    pub prob_win_10k: f64,
    pub unique_payout_count: usize,
    /// "1 in N" of the most likely outcome (T / max_weight) —
    /// the Stake requirement is ≥ 1000.
    pub most_likely_one_in: f64,
    /// "1 in N" of the maximum payout (T / cumulative weight of books at max).
    pub max_payout_one_in: f64,
    pub buckets: Vec<Bucket>,
    pub top_payouts: Vec<TopPayout>,
    /// Payouts outside the Studio format. Studio (« Math file format ») only asks for whole hundredths of the
    /// stake (uint64) — 0.01x steps are legal; the old 0.1x rule was the Python SDK's own check (2026-11-03).
    /// A lookup table parsed as integers can't break it: kept at 0 for the report's shape.
    pub granularity_violations: usize,
    /// What 100 rounds of this mode give back (`session`): spinning modes only
    /// (under 10x) — a run of purchases is not a session.
    pub short_session: Option<ShortSession>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    /// Bounds in bet multiplier, half-open [start, end).
    pub start: f64,
    pub end: f64,
    pub count: usize,
    pub weight: u64,
    pub probability: f64,
    /// "1 in N"
    pub one_in: f64,
    pub rtp_contribution: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopPayout {
    pub payout: f64,
    pub weight: u64,
    pub one_in: f64,
    /// Number of distinct books at this exact payout.
    pub count: usize,
    /// A few representative sim_ids (≤ 5) for the play↔analysis link.
    pub sim_ids: Vec<u64>,
}

pub fn analyze(lut: &Lut, cost: f64) -> ModeStats {
    let t = lut.total_weight as f64;
    let cost = if cost > 0.0 { cost } else { 1.0 };

    let mut sum_wp = 0.0_f64; // Σ w·p (p in x bet)
    let mut zero_w: u64 = 0;
    let mut max_weight: u64 = 0;
    let mut max_payout_x100: u64 = 0;
    let mut min_nonzero_x100: u64 = u64::MAX;
    let mut breakeven_w: u64 = 0;
    let mut w_5k: u64 = 0;
    let mut w_10k: u64 = 0;
    let mut etl40_wp = 0.0_f64;
    let mut etl10k_wp = 0.0_f64;
    let granularity_violations = 0usize;

    let cost_x100 = (cost * 100.0).round() as u64;

    for r in &lut.rows {
        let p = r.payout_x100 as f64 / 100.0;
        sum_wp += r.weight as f64 * p;
        if r.payout_x100 == 0 {
            zero_w += r.weight;
        } else {
            min_nonzero_x100 = min_nonzero_x100.min(r.payout_x100);
        }
        max_weight = max_weight.max(r.weight);
        max_payout_x100 = max_payout_x100.max(r.payout_x100);
        if r.payout_x100 >= cost_x100 {
            breakeven_w += r.weight;
        }
        if r.payout_x100 >= 500_000 {
            w_5k += r.weight;
        }
        if r.payout_x100 >= 1_000_000 {
            w_10k += r.weight;
        }
        if r.payout_x100 >= 4_000 {
            etl40_wp += r.weight as f64 * p;
        }
        if r.payout_x100 >= 1_000_000 {
            etl10k_wp += r.weight as f64 * p;
        }
    }

    let mean = sum_wp / t;
    let rtp = mean / cost;
    let zero_rate = zero_w as f64 / t;
    let hit_rate = 1.0 - zero_rate;

    // Weighted variance (second pass, exact mean).
    let mut variance = 0.0_f64;
    for r in &lut.rows {
        let p = r.payout_x100 as f64 / 100.0;
        variance += (p - mean) * (p - mean) * (r.weight as f64 / t);
    }
    let std_dev = variance.sqrt();

    // Sort by ascending payout: weighted median, CVaR, buckets, tops.
    let mut sorted: Vec<(u64, u64, u64)> = lut
        .rows
        .iter()
        .map(|r| (r.payout_x100, r.weight, r.sim_id))
        .collect();
    sorted.sort_unstable_by_key(|&(p, _, _)| p);

    // Weighted median.
    let half = lut.total_weight / 2;
    let mut acc: u64 = 0;
    let mut median = 0.0;
    for &(p, w, _) in &sorted {
        acc += w;
        if acc > half {
            median = p as f64 / 100.0;
            break;
        }
    }

    // CVaR@0.1%: from the top, target mass 0.001·T, partial inclusion.
    let target = t * 0.001;
    let mut mass = 0.0_f64;
    let mut cvar_wp = 0.0_f64;
    for &(p, w, _) in sorted.iter().rev() {
        if mass >= target {
            break;
        }
        let take = (w as f64).min(target - mass);
        cvar_wp += take * (p as f64 / 100.0);
        mass += take;
    }
    let cvar_001 = if mass > 0.0 { cvar_wp / mass } else { 0.0 };

    // Cumulative weight of books at the max payout (the "achievable max win").
    let max_w: u64 = sorted
        .iter()
        .rev()
        .take_while(|&&(p, _, _)| p == max_payout_x100)
        .map(|&(_, w, _)| w)
        .sum();

    // Distinct payout values + grouped top payouts.
    let mut unique = 0usize;
    let mut tops: Vec<TopPayout> = Vec::new();
    {
        let mut i = sorted.len();
        let mut last: Option<u64> = None;
        for &(p, _, _) in &sorted {
            if last != Some(p) {
                unique += 1;
                last = Some(p);
            }
        }
        // Groups from the top, 10 distinct payouts max.
        while i > 0 && tops.len() < 10 {
            let p = sorted[i - 1].0;
            let mut w: u64 = 0;
            let mut count = 0usize;
            let mut sim_ids = Vec::new();
            while i > 0 && sorted[i - 1].0 == p {
                w += sorted[i - 1].1;
                count += 1;
                if sim_ids.len() < 5 {
                    sim_ids.push(sorted[i - 1].2);
                }
                i -= 1;
            }
            tops.push(TopPayout {
                payout: p as f64 / 100.0,
                weight: w,
                one_in: t / w as f64,
                count,
                sim_ids,
            });
        }
    }

    let max_payout = max_payout_x100 as f64 / 100.0;
    let buckets = build_buckets(&sorted, t, cost, max_payout);
    let short_session = (cost < 10.0)
        .then(|| {
            let dist: Vec<(f64, f64)> =
                sorted.iter().map(|&(p, w, _)| (p as f64 / 100.0 / cost, w as f64)).collect();
            session::short_session(&dist, session::SPINS)
        })
        .flatten();

    ModeStats {
        cost,
        outcome_count: lut.rows.len(),
        total_weight: lut.total_weight,
        rtp,
        hit_rate,
        zero_rate,
        mean_payout: mean,
        median_payout: median,
        min_nonzero_payout: if min_nonzero_x100 == u64::MAX {
            0.0
        } else {
            min_nonzero_x100 as f64 / 100.0
        },
        max_payout,
        variance,
        std_dev,
        volatility: if mean > 0.0 { std_dev / mean } else { 0.0 },
        cost_adj_volatility: std_dev / cost,
        breakeven_rate: breakeven_w as f64 / t,
        cvar_001,
        etl_40x: etl40_wp / t / cost,
        etl_10kx: etl10k_wp / t / cost,
        prob_win_5k: w_5k as f64 / t,
        prob_win_10k: w_10k as f64 / t,
        unique_payout_count: unique,
        most_likely_one_in: t / max_weight as f64,
        max_payout_one_in: if max_w > 0 { t / max_w as f64 } else { f64::INFINITY },
        buckets,
        top_payouts: tops,
        granularity_violations,
        short_session,
    }
}

/// Dynamic bounds: {0, 0.01, 1} then a 1-2-5 pattern per decade up to
/// maxPayout×1.1. Exact-zero bucket kept separate, intervals [start, end).
fn bucket_boundaries(max_payout: f64) -> Vec<f64> {
    let mut b = vec![0.0, 0.01, 1.0];
    let limit = max_payout * 1.1;
    let mut decade = 1.0_f64;
    'outer: loop {
        for m in [2.0, 5.0, 10.0] {
            let v = decade * m;
            if *b.last().unwrap() >= limit {
                break 'outer;
            }
            b.push(v);
        }
        decade *= 10.0;
        if decade > 1e9 {
            break;
        }
    }
    b
}

fn build_buckets(
    sorted: &[(u64, u64, u64)],
    total_weight: f64,
    cost: f64,
    max_payout: f64,
) -> Vec<Bucket> {
    let mut bounds = bucket_boundaries(max_payout);
    // Last bucket: up to and including maxPayout.
    if *bounds.last().unwrap() <= max_payout {
        bounds.push(max_payout + 1.0);
    }

    let mut buckets: Vec<Bucket> = Vec::new();
    // Exact-zero bucket.
    let mut i = 0usize;
    let mut zero_w: u64 = 0;
    let mut zero_c = 0usize;
    while i < sorted.len() && sorted[i].0 == 0 {
        zero_w += sorted[i].1;
        zero_c += 1;
        i += 1;
    }
    if zero_c > 0 {
        buckets.push(Bucket {
            start: 0.0,
            end: 0.0,
            count: zero_c,
            weight: zero_w,
            probability: zero_w as f64 / total_weight,
            one_in: total_weight / zero_w as f64,
            rtp_contribution: 0.0,
        });
    }

    for win in bounds.windows(2) {
        let (start, end) = (win[0], win[1]);
        if end <= 0.01 && start == 0.0 {
            // [0, 0.01) excluding exact zero: sub-0.01x payouts don't exist
            // (granularity), but we keep the bound for the bucket shape.
        }
        let start_x100 = (start * 100.0).round() as u64;
        let end_x100 = (end * 100.0).round() as u64;
        let mut w: u64 = 0;
        let mut c = 0usize;
        let mut wp = 0.0_f64;
        while i < sorted.len() && sorted[i].0 < end_x100 {
            if sorted[i].0 >= start_x100.max(1) {
                w += sorted[i].1;
                c += 1;
                wp += sorted[i].1 as f64 * (sorted[i].0 as f64 / 100.0);
            }
            i += 1;
        }
        if c > 0 {
            buckets.push(Bucket {
                start,
                end,
                count: c,
                weight: w,
                probability: w as f64 / total_weight,
                one_in: total_weight / w as f64,
                rtp_contribution: wp / total_weight / cost,
            });
        }
    }
    buckets
}

/// Retrouve la tranche d'un gain avec la règle même de `build_buckets` —
/// gain nul → tranche zéro, sinon `max(début, 0.01) ≤ gain < fin`, bornes
/// arrondies au centième — pour que tout ce qui se ventile par tranche
/// ailleurs (provenance bonus/palier) retombe exactement sur ces tranches-ci.
pub struct BucketLookup {
    zero: Option<usize>,
    /// (début effectif ×100, fin ×100, index dans la liste), croissants.
    ranges: Vec<(u64, u64, usize)>,
}

impl BucketLookup {
    pub fn new(buckets: &[Bucket]) -> Self {
        let mut zero = None;
        let mut ranges = Vec::with_capacity(buckets.len());
        for (i, b) in buckets.iter().enumerate() {
            if b.start == 0.0 && b.end == 0.0 {
                zero = Some(i);
            } else {
                let start = ((b.start * 100.0).round() as u64).max(1);
                ranges.push((start, (b.end * 100.0).round() as u64, i));
            }
        }
        Self { zero, ranges }
    }

    /// `None` si aucune tranche listée ne contient ce gain (les tranches vides
    /// sont omises) — jamais pour une ligne de la LUT qui les a construites.
    pub fn find(&self, payout_x100: u64) -> Option<usize> {
        if payout_x100 == 0 {
            return self.zero;
        }
        let k = self.ranges.partition_point(|&(_, end, _)| end <= payout_x100);
        self.ranges
            .get(k)
            .filter(|&&(start, _, _)| payout_x100 >= start)
            .map(|&(_, _, i)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Lut, LutRow};

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
    fn basic_rtp_and_rates() {
        // 50% zero, 25% 1x, 25% 2.86x → mean = 0.965, cost 1.
        let l = lut(&[(0, 50, 0), (1, 25, 100), (2, 25, 286)]);
        let s = analyze(&l, 1.0);
        assert!((s.rtp - 0.965).abs() < 1e-12);
        assert!((s.hit_rate - 0.5).abs() < 1e-12);
        assert!((s.zero_rate - 0.5).abs() < 1e-12);
        assert!((s.breakeven_rate - 0.5).abs() < 1e-12);
        assert_eq!(s.unique_payout_count, 3);
        assert_eq!(s.granularity_violations, 0); // 2.86x: whole hundredths, legal for Studio
    }

    #[test]
    fn cost_normalizes_rtp() {
        // 100x buy always returning 96.5x → RTP 0.965.
        let l = lut(&[(0, 1, 9650)]);
        let s = analyze(&l, 100.0);
        assert!((s.rtp - 0.965).abs() < 1e-12);
        assert!((s.breakeven_rate - 0.0).abs() < 1e-12); // 96.5 < 100
    }

    #[test]
    fn buckets_rtp_contribution_sums_to_rtp() {
        let l = lut(&[
            (0, 700, 0),
            (1, 200, 50),
            (2, 60, 500),
            (3, 30, 2000),
            (4, 9, 10000),
            (5, 1, 500000),
        ]);
        let s = analyze(&l, 1.0);
        let sum: f64 = s.buckets.iter().map(|b| b.rtp_contribution).sum();
        assert!((sum - s.rtp).abs() < 1e-9, "Σcontrib {} != rtp {}", sum, s.rtp);
    }

    #[test]
    fn bucket_lookup_lands_every_lut_row_in_its_own_bucket() {
        // Chaque ligne de la LUT doit retomber dans la tranche qui l'a comptée :
        // on refait les poids par tranche via le lookup et on compare.
        let l = lut(&[
            (0, 700, 0),
            (1, 200, 50),
            (2, 60, 100),
            (3, 30, 199),
            (4, 9, 200),
            (5, 5, 99_999),
            (6, 1, 2_000_000),
        ]);
        let s = analyze(&l, 1.0);
        let lookup = BucketLookup::new(&s.buckets);
        let mut w = vec![0u64; s.buckets.len()];
        for r in &l.rows {
            w[lookup.find(r.payout_x100).expect("every row has a bucket")] += r.weight;
        }
        let expected: Vec<u64> = s.buckets.iter().map(|b| b.weight).collect();
        assert_eq!(w, expected);
        // 2 000 000 ×100 = 20 000x, le max : dans la dernière tranche.
        assert_eq!(lookup.find(2_000_000), Some(s.buckets.len() - 1));
    }

    #[test]
    fn weighted_median() {
        let l = lut(&[(0, 60, 0), (1, 30, 100), (2, 10, 1000)]);
        let s = analyze(&l, 1.0);
        assert_eq!(s.median_payout, 0.0); // 60% of the mass at 0
        let l2 = lut(&[(0, 40, 0), (1, 40, 100), (2, 20, 1000)]);
        let s2 = analyze(&l2, 1.0);
        assert_eq!(s2.median_payout, 1.0);
    }

    #[test]
    fn cvar_top_slice() {
        // 999 books weight 1 payout 0; 1 book weight 1 payout 5000x.
        // 0.1% slice = weight 1 = exactly the top book.
        let mut rows: Vec<(u64, u64, u64)> =
            (0..999).map(|i| (i, 1, 0)).collect();
        rows.push((999, 1, 500_000));
        let s = analyze(&lut(&rows), 1.0);
        assert!((s.cvar_001 - 5000.0).abs() < 1e-9);
    }
}
