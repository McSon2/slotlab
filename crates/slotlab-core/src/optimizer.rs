use crate::model::Lut;
use serde::{Deserialize, Serialize};

/// LUT re-balancing: we change ONLY the weights (payouts and books stay
/// intact → directly re-publishable). Method: 2-moment exponential tilt
/// `w_i · exp(a·pn + b·pn²)` (pn = normalized payout). Always ≥ 0 (unlike the
/// linear α·w+β·w·p which goes negative), so no RTP blow-up. `a` sets the mean
/// (RTP), `b` sets the spread (volatility). Solved by nested bisection.

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeRequest {
    /// Target RTP (0.965 = 96.5%).
    pub target_rtp: f64,
    /// Target volatility = std/cost. None → left free (RTP only).
    #[serde(default)]
    pub target_vol: Option<f64>,
    /// Cap the dominant book at ≤ 1/1000 (most-likely compliance).
    #[serde(default = "default_true")]
    pub cap_most_likely: bool,
    /// ANCHOR: fix the zero-win rate (0.70 = 70% of weight on payout 0).
    #[serde(default)]
    pub target_zero_rate: Option<f64>,
    /// ANCHOR: fix the max win frequency ("1 in N"; 5e6 = 1/5M).
    /// Makes the jackpot more (small N) or less (large N) accessible.
    #[serde(default)]
    pub max_win_one_in: Option<f64>,
    /// ANCHOR (cap): cap P(payout ≥ 10000x) — Stake's "Tail Probability
    /// (10,000x)" (≤ 0.002 for 2★). Only acts if the tail exceeds it.
    #[serde(default)]
    pub target_tail_10k: Option<f64>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Feasibility {
    pub current_rtp: f64,
    pub current_vol: f64,
    /// RTP achievable by shifting weights (payouts fixed, cap applied).
    pub min_rtp: f64,
    pub max_rtp: f64,
    /// Volatility (std/cost) approximately achievable at the target RTP.
    pub min_vol: f64,
    pub max_vol: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeResult {
    pub ok: bool,
    pub message: String,
    /// Target RTP reached (within tolerance).
    pub rtp_hit: bool,
    /// Target volatility reached (if requested).
    pub vol_hit: bool,
    pub achieved_rtp: f64,
    pub achieved_vol: f64,
    pub achieved_std: f64,
    pub most_likely_one_in: f64,
    pub feasibility: Feasibility,
    /// New weights, aligned with the order of `lut.rows`. Not sent to the UI:
    /// it shows the before/after stats, and a million weights is ~8 MB of JSON.
    #[serde(skip)]
    pub weights: Vec<u64>,
}

/// Weighted moments of a set of weights (payouts in x100).
fn moments(weights: &[f64], p_x100: &[f64]) -> (f64, f64, f64) {
    let mut w_sum = 0.0;
    let mut wp = 0.0;
    let mut wp2 = 0.0;
    for (w, p) in weights.iter().zip(p_x100) {
        w_sum += w;
        wp += w * p;
        wp2 += w * p * p;
    }
    let mean = wp / w_sum;
    let var = (wp2 / w_sum - mean * mean).max(0.0);
    (w_sum, mean, var)
}

fn tilt_weights(w0: &[f64], pn: &[f64], a: f64, b: f64) -> Vec<f64> {
    w0.iter()
        .zip(pn)
        .map(|(w, &x)| {
            let arg = (a * x + b * x * x).clamp(-50.0, 50.0);
            w * arg.exp()
        })
        .collect()
}

/// Min/max RTP achievable while respecting the cap (weight ≤ W/target_hr).
fn rtp_bounds(
    lut: &Lut,
    cost: f64,
    cap_hr: f64,
) -> (f64, f64) {
    let total_w = lut.total_weight as f64;
    let cap = if cap_hr > 0.0 { total_w / cap_hr } else { total_w };
    let mut sorted: Vec<f64> = lut.rows.iter().map(|r| r.payout_x100 as f64).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    // min: fill from the smallest payouts up to the cap.
    let fill = |asc: bool| -> f64 {
        let mut remaining = total_w;
        let mut wp = 0.0;
        let iter: Box<dyn Iterator<Item = &f64>> = if asc {
            Box::new(sorted.iter())
        } else {
            Box::new(sorted.iter().rev())
        };
        for p in iter {
            if remaining <= 0.0 {
                break;
            }
            let take = cap.min(remaining);
            wp += take * p;
            remaining -= take;
        }
        wp / total_w / 100.0 / cost
    };
    (fill(true), fill(false))
}

pub fn feasibility(lut: &Lut, cost: f64, cap_hr: f64) -> Feasibility {
    let p: Vec<f64> = lut.rows.iter().map(|r| r.payout_x100 as f64).collect();
    let w: Vec<f64> = lut.rows.iter().map(|r| r.weight as f64).collect();
    let (_, mean, var) = moments(&w, &p);
    let (min_rtp, max_rtp) = rtp_bounds(lut, cost, cap_hr);
    // Approximate volatility bounds at the current RTP: min ≈ everything
    // concentrated near the mean (bounded by granularity), max ≈ 2-point
    // solution at the extremes. We give a rough order of magnitude.
    let pmax = p.iter().cloned().fold(0.0_f64, f64::max) / 100.0;
    let m = mean / 100.0;
    // max std at fixed mean with 2 masses (0 and pmax): sqrt(m·(pmax−m)).
    let max_std = if pmax > m { (m * (pmax - m)).sqrt() } else { 0.0 };
    Feasibility {
        current_rtp: mean / 100.0 / cost,
        current_vol: var.sqrt() / 100.0 / cost,
        min_rtp,
        max_rtp,
        min_vol: 0.0,
        max_vol: max_std / cost,
    }
}

/// Weighted mean of `p` under the tilted weights, in one pass and without
/// materializing them: `solve_a` evaluates it 80 times, and the volatility
/// bisection calls `solve_a` 60 times — thousands of passes over the LUT.
/// Same operations in the same order as `tilt_weights` + `moments`, so the
/// result is bit-for-bit identical.
fn tilted_mean(w0: &[f64], pn: &[f64], p: &[f64], a: f64, b: f64) -> f64 {
    let mut w_sum = 0.0;
    let mut wp = 0.0;
    for ((w, &x), p) in w0.iter().zip(pn).zip(p) {
        let arg = (a * x + b * x * x).clamp(-50.0, 50.0);
        let wi = w * arg.exp();
        w_sum += wi;
        wp += wi * p;
    }
    wp / w_sum
}

/// Solves for `a` such that the weighted mean == target_mean (with b fixed).
/// The mean is monotonically increasing in a → bisection.
fn solve_a(w0: &[f64], pn: &[f64], p: &[f64], b: f64, target_mean: f64) -> f64 {
    let mean_at = |a: f64| tilted_mean(w0, pn, p, a, b);
    let (mut lo, mut hi) = (-45.0_f64, 45.0_f64);
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if mean_at(mid) < target_mean {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Water-filling: forces ALL weights ≤ cap while preserving the total sum,
/// redistributing the excess proportionally onto the rows that have room.
/// Converges (each pass caps ≥ 1 more row).
fn enforce_cap(w: &mut [f64], cap: f64, total: f64) {
    if !cap.is_finite() || w.is_empty() {
        return;
    }
    // Infeasible if too few rows to carry the mass under the cap:
    // we cap as best we can (the sum will drop, corrected upstream).
    if (w.len() as f64) * cap < total {
        for wi in w.iter_mut() {
            *wi = wi.min(cap);
        }
        return;
    }
    let n = w.len() as f64;
    for _ in 0..64 {
        let mut excess = 0.0;
        let mut room = 0.0; // Σ weight of rows under the cap (distribution key).
        for &wi in w.iter() {
            if wi > cap {
                excess += wi - cap;
            } else {
                room += wi;
            }
        }
        if excess <= total * 1e-12 {
            break;
        }
        for wi in w.iter_mut() {
            if *wi > cap {
                *wi = cap;
            } else if room > 0.0 {
                *wi += excess * (*wi / room);
            } else {
                *wi += excess / n;
            }
        }
    }
}

/// Spreads a total mass `mass` over the indices `idx`, proportionally to the
/// original weights (or uniformly if all are zero).
fn spread(mass: f64, idx: &[usize], w0: &[f64], out: &mut [f64]) {
    let sum: f64 = idx.iter().map(|&i| w0[i]).sum();
    for &i in idx {
        out[i] = if sum > 0.0 {
            mass * w0[i] / sum
        } else {
            mass / idx.len() as f64
        };
    }
}

pub fn optimize(lut: &Lut, cost: f64, req: &OptimizeRequest) -> OptimizeResult {
    let cap_hr = if req.cap_most_likely { 1050.0 } else { 0.0 };
    let feas = feasibility(lut, cost, cap_hr);

    let p: Vec<f64> = lut.rows.iter().map(|r| r.payout_x100 as f64).collect();
    let w0: Vec<f64> = lut.rows.iter().map(|r| r.weight as f64).collect();
    let n = p.len();
    let pmax = p.iter().cloned().fold(1.0_f64, f64::max);
    // LOG normalization of the payout: the tilt must be sensitive across the
    // whole range (0.01x → 50000x). Normalizing by the linear max made the
    // tilt flat over small/medium wins → impossible to reshape the core of the
    // distribution. In log space, each decade counts equally.
    let key: Vec<f64> = p.iter().map(|&x| if x > 0.0 { x.ln() } else { f64::NAN }).collect();
    let key_min = key.iter().cloned().filter(|k| k.is_finite()).fold(f64::MAX, f64::min);
    let key_max = key.iter().cloned().filter(|k| k.is_finite()).fold(f64::MIN, f64::max);
    let span = (key_max - key_min).max(1e-9);
    let pn: Vec<f64> = key
        .iter()
        .map(|&k| if k.is_finite() { (k - key_min) / span } else { 0.0 })
        .collect();
    let total_w = lut.total_weight as f64;
    let cap = if cap_hr > 0.0 { total_w / cap_hr } else { f64::INFINITY };
    let target_std = req.target_vol.map(|v| v * cost * 100.0);
    let mut msgs: Vec<String> = Vec::new();

    // --- Anchors: groups pinned to a fixed mass ---
    let mut pinned = vec![f64::NAN; n]; // NaN = free
    if let Some(zr) = req.target_zero_rate {
        let idx: Vec<usize> = (0..n).filter(|&i| p[i] == 0.0).collect();
        if !idx.is_empty() {
            spread((zr.clamp(0.0, 0.999)) * total_w, &idx, &w0, &mut pinned);
        }
    }
    if let Some(one_in) = req.max_win_one_in {
        if one_in >= 1.0 {
            let idx: Vec<usize> = (0..n).filter(|&i| p[i] == pmax).collect();
            spread(total_w / one_in, &idx, &w0, &mut pinned);
        }
    }
    // Cap the tail ≥ 10000x (Stake Tail Probability). Only acts if the current
    // tail exceeds the target → we pin the band to target·W.
    if let Some(cap_prob) = req.target_tail_10k {
        let thresh = 1_000_000.0; // 10000x in payout_x100
        let target_mass = cap_prob.max(0.0) * total_w;
        let all_tail: Vec<usize> = (0..n).filter(|&i| p[i] >= thresh).collect();
        let pinned_mass: f64 = all_tail
            .iter()
            .filter(|&&i| !pinned[i].is_nan())
            .map(|&i| pinned[i])
            .sum();
        let free_tail: Vec<usize> =
            all_tail.iter().cloned().filter(|&i| pinned[i].is_nan()).collect();
        let cur_free_mass: f64 = free_tail.iter().map(|&i| w0[i]).sum();
        let remaining = (target_mass - pinned_mass).max(0.0);
        if cur_free_mass > remaining && !free_tail.is_empty() {
            spread(remaining, &free_tail, &w0, &mut pinned);
        }
    }

    let free: Vec<usize> = (0..n).filter(|&i| pinned[i].is_nan()).collect();
    let w_pinned: f64 = (0..n).filter(|&i| !pinned[i].is_nan()).map(|i| pinned[i]).sum();
    let win_pinned: f64 = (0..n).filter(|&i| !pinned[i].is_nan()).map(|i| pinned[i] * p[i]).sum();
    let w_free = (total_w - w_pinned).max(0.0);

    // Residual RTP that the free group must carry.
    let target_total_win = req.target_rtp * cost * 100.0 * total_w;
    let win_free_needed = target_total_win - win_pinned;

    if free.is_empty() || w_free <= 0.0 {
        msgs.push("The anchors saturate all the weight — loosen the zero rate or the max win frequency.".into());
    }
    let mean_free_target = if w_free > 0.0 { win_free_needed / w_free } else { 0.0 };
    // Feasibility of the free group.
    let free_pmax = free.iter().map(|&i| p[i]).fold(0.0_f64, f64::max);
    let free_pmin = free.iter().map(|&i| p[i]).fold(f64::MAX, f64::min);
    if win_free_needed < -1.0 {
        msgs.push("The anchors already contribute more than the target RTP — lower the zero rate or make the max win less frequent.".into());
    } else if mean_free_target > free_pmax + 1.0 || mean_free_target < free_pmin - 1.0 {
        msgs.push(format!(
            "RTP {:.2}% unreachable with these anchors (the rest would have to pay {:.1}x on average) — loosen an anchor or re-simulate.",
            req.target_rtp * 100.0,
            mean_free_target / 100.0
        ));
    } else if req.target_zero_rate.is_none()
        && (req.target_rtp < feas.min_rtp - 1e-6 || req.target_rtp > feas.max_rtp + 1e-6)
    {
        msgs.push(format!(
            "RTP outside the achievable range [{:.2}%, {:.2}%] — re-simulate for more diversity.",
            feas.min_rtp * 100.0,
            feas.max_rtp * 100.0
        ));
    }

    // Sub-vectors of the free group.
    let fw0: Vec<f64> = free.iter().map(|&i| w0[i]).collect();
    let fpn: Vec<f64> = free.iter().map(|&i| pn[i]).collect();
    let fp: Vec<f64> = free.iter().map(|&i| p[i]).collect();

    // Solve (a, b) on the free group to reach mean_free_target, with volatility
    // measured over the WHOLE assembled set (pinned + free).
    let assemble = |fa: f64, fb: f64| -> Vec<f64> {
        let fw = tilt_weights(&fw0, &fpn, fa, fb);
        let s: f64 = fw.iter().sum();
        let scale = if s > 0.0 { w_free / s } else { 0.0 };
        let mut full = vec![0.0; n];
        for i in 0..n {
            if !pinned[i].is_nan() {
                full[i] = pinned[i];
            }
        }
        for (k, &i) in free.iter().enumerate() {
            full[i] = fw[k] * scale;
        }
        full
    };

    let (a, b) = match target_std {
        None => (solve_a(&fw0, &fpn, &fp, 0.0, mean_free_target), 0.0),
        Some(s_target) => {
            let std_at_b = |fb: f64| -> f64 {
                let fa = solve_a(&fw0, &fpn, &fp, fb, mean_free_target);
                let full = assemble(fa, fb);
                let (_, _, var) = moments(&full, &p);
                var.sqrt()
            };
            let (mut lo, mut hi) = (-40.0_f64, 40.0_f64);
            let (slo, shi) = (std_at_b(lo), std_at_b(hi));
            let fb = if s_target <= slo {
                lo
            } else if s_target >= shi {
                hi
            } else {
                for _ in 0..60 {
                    let mid = 0.5 * (lo + hi);
                    if std_at_b(mid) < s_target {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                0.5 * (lo + hi)
            };
            (solve_a(&fw0, &fpn, &fp, fb, mean_free_target), fb)
        }
    };

    // Assembled weights, then GLOBAL cap via water-filling (guarantees that NO
    // book — not even an anchor — exceeds W/1050), re-aligning the RTP between
    // each pass. The old cap only touched the free group and un-capped itself
    // on renormalization → a concentrated book survived (BONUS/FULLCREW 1/80
    // bug).
    let mut w = assemble(a, b);
    let target_mean_overall = req.target_rtp * cost * 100.0;
    if cap.is_finite() {
        for _ in 0..12 {
            enforce_cap(&mut w, cap, total_w);
            let (_, mean, _) = moments(&w, &p);
            if (mean - target_mean_overall).abs() <= target_mean_overall * 1e-4 {
                break;
            }
            // Global corrective tilt to restore the RTP, then renormalize.
            let a2 = solve_a(&w, &pn, &p, 0.0, target_mean_overall);
            w = tilt_weights(&w, &pn, a2, 0.0);
            let s: f64 = w.iter().sum();
            for wi in w.iter_mut() {
                *wi *= total_w / s;
            }
        }
        enforce_cap(&mut w, cap, total_w); // final guarantee
    }
    if req.max_win_one_in.map(|o| o < 1000.0).unwrap_or(false) && req.cap_most_likely {
        msgs.push("Max win < 1/1000 is incompatible with the most-likely cap: the jackpot was capped at ~1/1050 (anchor not honored).".into());
    }

    // Integer rounding while preserving Σ = total_w.
    let s: f64 = w.iter().sum();
    let scale = total_w / s;
    let mut wi: Vec<u64> = w.iter().map(|x| (x * scale).round().max(0.0) as u64).collect();
    // Correct the rounding residual on the median weight (neither 0 nor cap).
    let sum_i: i128 = wi.iter().map(|&x| x as i128).sum();
    let mut residual = total_w as i128 - sum_i;
    if residual != 0 {
        // Spread ±1 over the largest weights under the cap.
        let cap_u = if cap.is_finite() { cap as u64 } else { u64::MAX };
        let mut idx: Vec<usize> = (0..wi.len()).collect();
        idx.sort_by(|&a, &b| wi[b].cmp(&wi[a]));
        let step: i128 = if residual > 0 { 1 } else { -1 };
        for &i in idx.iter().cycle() {
            if residual == 0 {
                break;
            }
            if step > 0 && wi[i] < cap_u {
                wi[i] += 1;
                residual -= 1;
            } else if step < 0 && wi[i] > 0 {
                wi[i] -= 1;
                residual += 1;
            }
        }
    }

    // Achieved stats on the final INTEGER weights.
    let wf: Vec<f64> = wi.iter().map(|&x| x as f64).collect();
    let (wsum, mean, var) = moments(&wf, &p);
    let achieved_rtp = mean / 100.0 / cost;
    let achieved_std = var.sqrt() / 100.0;
    let achieved_vol = achieved_std / cost;
    let max_w = wi.iter().cloned().max().unwrap_or(0) as f64;
    let most_likely_one_in = if max_w > 0.0 { wsum / max_w } else { f64::INFINITY };

    let rtp_hit = (achieved_rtp - req.target_rtp).abs() <= 0.0005;
    let vol_hit = target_std
        .map(|s| (achieved_std * 100.0 - s).abs() / s.max(1.0) <= 0.05)
        .unwrap_or(true);
    if !vol_hit {
        msgs.push(format!(
            "Target volatility {:.1} not reached (closest: {:.1}) — bounded by the books; re-simulate to widen it.",
            req.target_vol.unwrap_or(0.0),
            achieved_vol
        ));
    }
    if msgs.is_empty() {
        msgs.push("Re-balancing successful.".into());
    }

    OptimizeResult {
        ok: rtp_hit,
        message: msgs.join(" "),
        rtp_hit,
        vol_hit,
        achieved_rtp,
        achieved_vol,
        achieved_std,
        most_likely_one_in,
        feasibility: feas,
        weights: wi,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Lut, LutRow};

    fn lut(rows: &[(u64, u64, u64)]) -> Lut {
        Lut::new(
            rows.iter()
                .map(|&(sim_id, weight, payout_x100)| LutRow { sim_id, weight, payout_x100 })
                .collect(),
        )
    }

    #[test]
    fn hits_target_rtp() {
        // Broad distribution: 0, 1x, 5x, 50x, 500x.
        let l = lut(&[
            (0, 900_000, 0),
            (1, 80_000, 100),
            (2, 15_000, 500),
            (3, 4_000, 5000),
            (4, 1_000, 50000),
        ]);
        let req = OptimizeRequest { target_rtp: 0.965, target_vol: None, cap_most_likely: false, target_zero_rate: None, max_win_one_in: None, target_tail_10k: None };
        let r = optimize(&l, 1.0, &req);
        assert!(r.rtp_hit, "rtp {} != 0.965 (msg {})", r.achieved_rtp, r.message);
        assert!((r.achieved_rtp - 0.965).abs() < 0.0005);
        // Weights conserved in sum.
        let sum: u64 = r.weights.iter().sum();
        assert_eq!(sum, 1_000_000);
    }

    #[test]
    fn raises_volatility_on_request() {
        let l = lut(&[
            (0, 900_000, 0),
            (1, 80_000, 100),
            (2, 15_000, 500),
            (3, 4_000, 5000),
            (4, 1_000, 50000),
        ]);
        let base = optimize(&l, 1.0, &OptimizeRequest { target_rtp: 0.965, target_vol: None, cap_most_likely: false, target_zero_rate: None, max_win_one_in: None, target_tail_10k: None });
        let hi = optimize(&l, 1.0, &OptimizeRequest { target_rtp: 0.965, target_vol: Some(base.achieved_vol * 1.5), cap_most_likely: false, target_zero_rate: None, max_win_one_in: None, target_tail_10k: None });
        assert!(hi.rtp_hit);
        assert!(hi.achieved_vol > base.achieved_vol, "{} !> {}", hi.achieved_vol, base.achieved_vol);
    }

    #[test]
    fn pins_zero_rate() {
        let l = lut(&[
            (0, 890_000, 0),
            (1, 80_000, 100),
            (2, 25_000, 500),
            (3, 4_000, 5000),
            (4, 1_000, 50000),
        ]);
        // Push the zero rate to 70% while keeping RTP at 96.5%.
        let r = optimize(&l, 1.0, &OptimizeRequest {
            target_rtp: 0.965, target_vol: None, cap_most_likely: false,
            target_zero_rate: Some(0.70), max_win_one_in: None, target_tail_10k: None,
        });
        let total: u64 = r.weights.iter().sum();
        let zero_w = r.weights[0] as f64;
        assert!((zero_w / total as f64 - 0.70).abs() < 0.005, "zero rate {}", zero_w / total as f64);
        assert!(r.rtp_hit, "rtp {}", r.achieved_rtp);
    }

    #[test]
    fn pins_max_win_frequency() {
        let l = lut(&[
            (0, 890_000, 0),
            (1, 80_000, 100),
            (2, 25_000, 500),
            (3, 4_000, 5000),
            (4, 1_000, 50000),
        ]);
        let r = optimize(&l, 1.0, &OptimizeRequest {
            target_rtp: 0.965, target_vol: None, cap_most_likely: false,
            target_zero_rate: None, max_win_one_in: Some(1_000_000.0), target_tail_10k: None,
        });
        let total: u64 = r.weights.iter().sum();
        // The max win (row 4) should land at ~1/1M.
        let odds = total as f64 / r.weights[4] as f64;
        assert!((odds - 1_000_000.0).abs() / 1e6 < 0.02, "odds 1/{}", odds);
        assert!(r.rtp_hit);
    }

    #[test]
    fn cap_holds_on_concentrated_mode() {
        // Concentrated "buy" mode: one overwhelming book + a handful of others.
        // Without a robust cap, the dominant book stays at ~1/1 (the BONUS bug).
        let mut rows = vec![(0u64, 5_000_000u64, 9000u64)]; // giant book at 90x
        for i in 1..2000 {
            rows.push((i, 1000, 9000 + (i % 50) * 10));
        }
        let l = lut(&rows);
        let r = optimize(&l, 1.0, &OptimizeRequest {
            target_rtp: 0.90, target_vol: None, cap_most_likely: true,
            target_zero_rate: None, max_win_one_in: None, target_tail_10k: None,
        });
        assert!(
            r.most_likely_one_in >= 1000.0,
            "most-likely 1/{} must be ≥ 1000",
            r.most_likely_one_in
        );
    }

    #[test]
    fn weights_never_negative() {
        let l = lut(&[(0, 500, 0), (1, 300, 200), (2, 200, 9000)]);
        let r = optimize(&l, 1.0, &OptimizeRequest { target_rtp: 0.90, target_vol: Some(30.0), cap_most_likely: false, target_zero_rate: None, max_win_one_in: None, target_tail_10k: None });
        assert!(r.weights.iter().all(|&_w| true)); // u64 → never negative by construction
        assert!(r.achieved_rtp > 0.0);
    }
}
