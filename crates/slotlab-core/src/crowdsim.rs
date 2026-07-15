use crate::model::Lut;
use rand::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;
#[cfg(feature = "parallel")]
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// Simulation of player cohorts on a mode (Monte Carlo, seeded →
/// reproducible). All monetary values are in BETS (bet = 1).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrowdParams {
    pub players: u32,
    pub spins: u32,
    /// Starting bankroll in bets (e.g. 100 = 100 bets).
    pub start_balance: f64,
    pub seed: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrowdResult {
    pub params_echo: CrowdEcho,
    /// Probability of Profit: players finishing above their bankroll.
    pub pop: f64,
    /// Busted players (balance < cost of one spin before the end).
    pub bust_rate: f64,
    pub empirical_rtp: f64,
    /// Average spins played (busts stop before `spins`).
    pub avg_spins_played: f64,
    /// Checkpoints (spin no.) of the curves.
    pub checkpoints: Vec<u32>,
    pub pop_curve: Vec<f64>,
    pub balance_p5: Vec<f64>,
    pub balance_p50: Vec<f64>,
    pub balance_p95: Vec<f64>,
    pub balance_mean: Vec<f64>,
    /// Average max peak→trough drawdown, as a fraction of the bankroll.
    pub avg_max_drawdown: f64,
    /// Players who dropped below 50% / 10% of the bankroll at some point.
    pub pct_below_50: f64,
    pub pct_below_10: f64,
    pub max_win_streak: u32,
    pub max_lose_streak: u32,
    /// Median spin of the first win ≥ 10x (u32::MAX if never → excluded).
    pub median_first_big_win_spin: Option<u32>,
    /// Mnemoo-style classification: Low / Medium / High.
    pub player_volatility: &'static str,
    /// Average max peak reached (as a fraction of bankroll).
    pub avg_max_peak: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrowdEcho {
    pub players: u32,
    pub spins: u32,
    pub start_balance: f64,
    pub cost: f64,
    pub seed: u64,
}

struct PlayerOutcome {
    checkpoints_balance: Vec<f32>,
    final_balance: f64,
    busted: bool,
    spins_played: u32,
    total_bet: f64,
    total_won: f64,
    max_drawdown: f64,
    max_peak: f64,
    below_50: bool,
    below_10: bool,
    max_win_streak: u32,
    max_lose_streak: u32,
    first_big_win: Option<u32>,
}

fn simulate_player(
    lut: &Lut,
    cost: f64,
    params: &CrowdParams,
    checkpoints: &[u32],
    seed: u64,
) -> PlayerOutcome {
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    let mut balance = params.start_balance;
    let mut peak = balance;
    let mut max_dd = 0.0_f64;
    let mut max_peak = balance;
    let mut below_50 = false;
    let mut below_10 = false;
    let mut win_streak = 0u32;
    let mut lose_streak = 0u32;
    let mut max_ws = 0u32;
    let mut max_ls = 0u32;
    let mut first_big = None;
    let mut total_bet = 0.0;
    let mut total_won = 0.0;
    let mut spins_played = 0u32;
    let mut busted = false;

    let mut cp_iter = checkpoints.iter().copied().peekable();
    let mut cp_balances = Vec::with_capacity(checkpoints.len());

    for spin in 1..=params.spins {
        if balance < cost {
            busted = true;
            break;
        }
        balance -= cost;
        total_bet += cost;
        let roll = rng.gen_range(0..lut.total_weight);
        let payout = lut.pick(roll).payout_x100 as f64 / 100.0;
        balance += payout;
        total_won += payout;
        spins_played = spin;

        if payout >= cost {
            win_streak += 1;
            lose_streak = 0;
        } else {
            lose_streak += 1;
            win_streak = 0;
        }
        max_ws = max_ws.max(win_streak);
        max_ls = max_ls.max(lose_streak);
        if first_big.is_none() && payout >= 10.0 * cost {
            first_big = Some(spin);
        }
        if balance > peak {
            peak = balance;
        }
        max_peak = max_peak.max(balance);
        if peak > 0.0 {
            max_dd = max_dd.max((peak - balance) / params.start_balance);
        }
        if balance < params.start_balance * 0.5 {
            below_50 = true;
        }
        if balance < params.start_balance * 0.1 {
            below_10 = true;
        }
        while let Some(&cp) = cp_iter.peek() {
            if cp == spin {
                cp_balances.push(balance as f32);
                cp_iter.next();
            } else {
                break;
            }
        }
    }
    // Remaining checkpoints after bust/end: balance frozen.
    for _ in cp_balances.len()..checkpoints.len() {
        cp_balances.push(balance as f32);
    }

    PlayerOutcome {
        checkpoints_balance: cp_balances,
        final_balance: balance,
        busted,
        spins_played,
        total_bet,
        total_won,
        max_drawdown: max_dd,
        max_peak,
        below_50,
        below_10,
        max_win_streak: max_ws,
        max_lose_streak: max_ls,
        first_big_win: first_big,
    }
}

pub fn run(lut: &Lut, cost: f64, params: &CrowdParams) -> CrowdResult {
    let n_checkpoints = 100usize.min(params.spins as usize).max(1);
    let checkpoints: Vec<u32> = (1..=n_checkpoints)
        .map(|i| ((i as u64 * params.spins as u64) / n_checkpoints as u64) as u32)
        .collect();

    let sim = |i: u32| {
        simulate_player(
            lut,
            cost,
            params,
            &checkpoints,
            params.seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(i as u64),
        )
    };

    #[cfg(feature = "parallel")]
    let outcomes: Vec<PlayerOutcome> = (0..params.players).into_par_iter().map(sim).collect();
    #[cfg(not(feature = "parallel"))]
    let outcomes: Vec<PlayerOutcome> = (0..params.players).map(sim).collect();

    let n = outcomes.len() as f64;
    let pop = outcomes
        .iter()
        .filter(|o| o.final_balance > params.start_balance)
        .count() as f64
        / n;
    let bust_rate = outcomes.iter().filter(|o| o.busted).count() as f64 / n;
    let total_bet: f64 = outcomes.iter().map(|o| o.total_bet).sum();
    let total_won: f64 = outcomes.iter().map(|o| o.total_won).sum();

    // Curves per checkpoint.
    let mut pop_curve = Vec::with_capacity(checkpoints.len());
    let mut p5 = Vec::with_capacity(checkpoints.len());
    let mut p50 = Vec::with_capacity(checkpoints.len());
    let mut p95 = Vec::with_capacity(checkpoints.len());
    let mut mean_curve = Vec::with_capacity(checkpoints.len());
    let mut col: Vec<f32> = vec![0.0; outcomes.len()];
    for ci in 0..checkpoints.len() {
        for (pi, o) in outcomes.iter().enumerate() {
            col[pi] = o.checkpoints_balance[ci];
        }
        col.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = |q: f64| ((col.len() - 1) as f64 * q).round() as usize;
        p5.push(col[idx(0.05)] as f64);
        p50.push(col[idx(0.50)] as f64);
        p95.push(col[idx(0.95)] as f64);
        mean_curve.push(col.iter().map(|&v| v as f64).sum::<f64>() / n);
        pop_curve.push(
            col.iter().filter(|&&v| v as f64 > params.start_balance).count() as f64 / n,
        );
    }

    let mut first_bigs: Vec<u32> =
        outcomes.iter().filter_map(|o| o.first_big_win).collect();
    first_bigs.sort_unstable();
    let median_first_big = if first_bigs.is_empty() {
        None
    } else {
        Some(first_bigs[first_bigs.len() / 2])
    };

    let avg_max_drawdown =
        outcomes.iter().map(|o| o.max_drawdown).sum::<f64>() / n;
    let avg_max_peak = outcomes
        .iter()
        .map(|o| o.max_peak / params.start_balance)
        .sum::<f64>()
        / n;
    let pct_below_50 = outcomes.iter().filter(|o| o.below_50).count() as f64 / n;

    // Mnemoo-style classification.
    let final_std = {
        let mean_final =
            outcomes.iter().map(|o| o.final_balance).sum::<f64>() / n;
        (outcomes
            .iter()
            .map(|o| (o.final_balance - mean_final).powi(2))
            .sum::<f64>()
            / n)
            .sqrt()
    };
    let std_ratio = final_std / params.start_balance;
    let player_volatility = if pop >= 0.40 && std_ratio < 0.50 && avg_max_peak < 3.0 {
        "low"
    } else if pop < 0.20 || std_ratio > 1.0 || avg_max_peak > 5.0 {
        "high"
    } else {
        "medium"
    };

    CrowdResult {
        params_echo: CrowdEcho {
            players: params.players,
            spins: params.spins,
            start_balance: params.start_balance,
            cost,
            seed: params.seed,
        },
        pop,
        bust_rate,
        empirical_rtp: if total_bet > 0.0 { total_won / total_bet } else { 0.0 },
        avg_spins_played: outcomes.iter().map(|o| o.spins_played as f64).sum::<f64>() / n,
        checkpoints,
        pop_curve,
        balance_p5: p5,
        balance_p50: p50,
        balance_p95: p95,
        balance_mean: mean_curve,
        avg_max_drawdown,
        pct_below_50,
        pct_below_10: outcomes.iter().filter(|o| o.below_10).count() as f64 / n,
        max_win_streak: outcomes.iter().map(|o| o.max_win_streak).max().unwrap_or(0),
        max_lose_streak: outcomes.iter().map(|o| o.max_lose_streak).max().unwrap_or(0),
        median_first_big_win_spin: median_first_big,
        player_volatility,
        avg_max_peak,
    }
}

/// Convergence of the cumulative RTP over a long run (log-spaced checkpoints).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvergenceResult {
    pub checkpoints: Vec<u64>,
    pub rtp_curve: Vec<f64>,
    pub final_rtp: f64,
}

pub fn rtp_convergence(lut: &Lut, cost: f64, spins: u64, seed: u64) -> ConvergenceResult {
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    let mut checkpoints = Vec::new();
    let mut cp = 100u64;
    while cp < spins {
        checkpoints.push(cp);
        // ~10 points per decade.
        cp = (cp as f64 * 1.26).ceil() as u64;
    }
    checkpoints.push(spins);

    let mut total_won = 0.0_f64;
    let mut curve = Vec::with_capacity(checkpoints.len());
    let mut next_cp = 0usize;
    for spin in 1..=spins {
        let roll = rng.gen_range(0..lut.total_weight);
        total_won += lut.pick(roll).payout_x100 as f64 / 100.0;
        if next_cp < checkpoints.len() && spin == checkpoints[next_cp] {
            curve.push(total_won / (spin as f64 * cost));
            next_cp += 1;
        }
    }
    let final_rtp = *curve.last().unwrap_or(&0.0);
    ConvergenceResult { checkpoints, rtp_curve: curve, final_rtp }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Lut, LutRow};

    fn simple_lut() -> Lut {
        Lut::new(vec![
            LutRow { sim_id: 0, weight: 50, payout_x100: 0 },
            LutRow { sim_id: 1, weight: 25, payout_x100: 100 },
            LutRow { sim_id: 2, weight: 25, payout_x100: 286 },
        ])
    }

    #[test]
    fn reproducible_by_seed() {
        let lut = simple_lut();
        let p = CrowdParams { players: 100, spins: 200, start_balance: 100.0, seed: 42 };
        let a = run(&lut, 1.0, &p);
        let b = run(&lut, 1.0, &p);
        assert_eq!(a.pop, b.pop);
        assert_eq!(a.balance_p50, b.balance_p50);
    }

    #[test]
    fn convergence_approaches_analytic_rtp() {
        let lut = simple_lut(); // analytic RTP 0.965
        let c = rtp_convergence(&lut, 1.0, 2_000_000, 7);
        assert!((c.final_rtp - 0.965).abs() < 0.01, "rtp {}", c.final_rtp);
    }
}
