//! CrowdSim — what players LIVE, simulated on the published lookup tables.
//!
//! A cohort of players each plays `spins` rounds (seeded Monte Carlo, bet = 1
//! base bet). A scenario is one mode, or a mix of modes (each round draws its
//! mode by share: « 95 % BASE, 5 % BONUS buys »). Everything is measured twice:
//!
//!   · per PLAYER — net result, session RTP, best win, bonuses, dry spells… —
//!     then summarised across players (mean, P5…P95, share of players);
//!   · per ROUND — every lookup-table row drawn is counted, so the OBSERVED
//!     distribution (bonuses by tier, their median, bands…) is exact for the
//!     sample, and comparable to the theoretical one (`population`).
//!
//! « Bonus » = a book with a `freeSpinTrigger`, as everywhere in SlotLab; the
//! books index must be imported for bonus figures (else they are absent).
//! Amounts are in multiples of the BASE bet; a round of a mode costs its cost.
//!
//! Player feel (`feel`): each player's run is also cut into sessions — droughts
//! (« 1 000 spins, one bonus »), letdowns (« three bonuses, all under 20x ») —
//! counted here, then read next to the exact figures, with what a fix costs.

use crate::feel::{self, FeelParams, FeelReport};
use crate::model::Lut;
use crate::population::{self, Population};
use crate::provenance::BookFacts;
use rand::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;
#[cfg(feature = "parallel")]
use rayon::prelude::*;
use serde::Serialize;

/// A mode as the cohort plays it.
pub struct SimMode<'a> {
    pub name: String,
    pub cost: f64,
    pub lut: &'a Lut,
    /// One entry per `lut.rows` row, same order. `bonus` stays false when the
    /// books are not imported — `imported` says so.
    pub facts: &'a [BookFacts],
    pub criteria_names: &'a [String],
    pub imported: bool,
}

/// One cohort: a mode, or a mix of modes by share of rounds.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub name: String,
    /// (index in the modes slice, share of rounds); shares need not sum to 1.
    pub mix: Vec<(usize, f64)>,
    /// The cohort plays a fixed version of a mode (`feel` fix), not the
    /// published one.
    pub fix: Option<FixRef>,
}

/// Which fix a cohort plays: frustration, payer, and the mode it re-weights.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixRef {
    pub mode: String,
    pub frustration: String,
    pub payer: String,
}

#[derive(Debug, Clone)]
pub struct CrowdParams {
    pub players: u32,
    pub spins: u32,
    /// Starting balance in base bets; `None`: unlimited, everyone plays every round.
    pub bankroll: Option<f64>,
    pub seed: u64,
    /// What a drought and a letdown are (`feel`).
    pub feel: FeelParams,
}

// ── Output ───────────────────────────────────────────────────────────────────

/// A per-player metric summarised over the cohort.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metric {
    pub key: &'static str,
    pub label: &'static str,
    /// `bets` | `x` | `ratio` | `spins` | `count`
    pub unit: &'static str,
    /// Players the metric exists for (e.g. those who triggered a bonus).
    pub players: u32,
    pub mean: f64,
    pub min: f64,
    pub p5: f64,
    pub p25: f64,
    pub p50: f64,
    pub p75: f64,
    pub p95: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Share {
    pub key: &'static str,
    pub label: &'static str,
    pub value: f64,
}

/// A rare result: how many the sample saw vs how many the math expects.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RareCheck {
    pub at_least: f64,
    pub expected: f64,
    pub observed: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeObserved {
    pub mode: String,
    pub cost: f64,
    pub imported: bool,
    /// Share of the scenario's rounds played on this mode.
    pub round_share: f64,
    pub rounds: u64,
    pub wagered: f64,
    pub won: f64,
    pub rtp: f64,
    pub theoretical_rtp: f64,
    /// Observed distribution (weights = draws): all spins, bonus, tiers, variants.
    pub populations: Vec<Population>,
    pub rare: Vec<RareCheck>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Curves {
    pub checkpoints: Vec<u32>,
    pub net_p5: Vec<f64>,
    pub net_p25: Vec<f64>,
    pub net_p50: Vec<f64>,
    pub net_p75: Vec<f64>,
    pub net_p95: Vec<f64>,
    pub net_mean: Vec<f64>,
    /// Session RTP (won ÷ wagered so far) across players.
    pub rtp_p5: Vec<f64>,
    pub rtp_p50: Vec<f64>,
    pub rtp_p95: Vec<f64>,
    /// Players in profit.
    pub pop: Vec<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioResult {
    pub name: String,
    pub mix: Vec<(String, f64)>,
    pub players: u32,
    pub spins: u32,
    pub bankroll: Option<f64>,
    pub rounds: u64,
    pub wagered: f64,
    pub won: f64,
    pub rtp: f64,
    pub theoretical_rtp: f64,
    /// Standard error of the empirical RTP expected at this sample size.
    pub rtp_std_error: f64,
    /// Bonus figures exist (books imported for every mode of the scenario).
    pub has_bonus: bool,
    pub bonuses: u64,
    pub max_wins: u64,
    pub metrics: Vec<Metric>,
    pub shares: Vec<Share>,
    /// Share of players by number of bonuses (index = count, last = « or more »).
    pub bonus_hist: Vec<f64>,
    pub curves: Curves,
    pub modes: Vec<ModeObserved>,
    /// Player feel of a single base-like mode: frustrations, exact and lived,
    /// and what fixing each one costs.
    pub feel: Option<FeelReport>,
    pub fix: Option<FixRef>,
    pub elapsed_ms: u64,
}

// ── Simulation ───────────────────────────────────────────────────────────────

const CHECKPOINTS: usize = 50;
const BONUS_HIST: usize = 21;

#[derive(Default, Clone)]
struct Player {
    wagered: f64,
    won: f64,
    rounds: u32,
    busted: bool,
    hits: u32,
    best_x: f64,
    dead: u32,
    losing: u32,
    first_10x: Option<u32>,
    first_100x: Option<u32>,
    bonuses: u32,
    first_bonus: Option<u32>,
    best_bonus: f64,
    bonus_won: f64,
    max_wins: u32,
    min_net: f64,
    max_net: f64,
    // Player feel: the sessions under way, and what the finished ones brought.
    d_bonuses: u32,
    drought_sessions: u32,
    droughts: u32,
    s_bonuses: u32,
    s_strong: bool,
    sessions: u32,
    multi: u32,
    letdowns: u32,
    /// Lowest balance change before a spin, until the first bonus.
    pre_low: f64,
    r_big: bool,
    reach_sessions: u32,
    reach_misses: u32,
}

/// Everything a chunk of players produces; chunks are merged at the end.
struct Acc {
    players: Vec<Player>,
    /// Per player, CHECKPOINTS × (net, wagered).
    cp: Vec<[f32; 2]>,
    /// Draws per lookup-table row, per mode of the scenario.
    rows: Vec<Vec<u32>>,
    rounds: Vec<u64>,
}

struct Prepared<'a> {
    mode: &'a SimMode<'a>,
    /// Payout (× base bet) and bonus flag per row, for the hot loop.
    x: Vec<f64>,
    bonus: Vec<bool>,
    max_x: f64,
    /// A bonus paying less than this is a letdown (`weak_under` × the cost).
    weak_x: f64,
    /// A big win (`big_at` × the cost).
    big_x: f64,
}

fn simulate_chunk(
    prepared: &[Prepared],
    mix_cum: &[(usize, f64)],
    params: &CrowdParams,
    feel: &FeelParams,
    checkpoints: &[u32],
    players: std::ops::Range<u32>,
) -> Acc {
    let mut acc = Acc {
        players: Vec::with_capacity(players.len()),
        cp: Vec::with_capacity(players.len() * checkpoints.len()),
        rows: prepared.iter().map(|p| vec![0u32; p.mode.lut.rows.len()]).collect(),
        rounds: vec![0; prepared.len()],
    };
    let total_share = mix_cum.last().map_or(1.0, |m| m.1);
    for i in players {
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(params.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(i as u64));
        let mut p = Player::default();
        let (mut dead, mut losing) = (0u32, 0u32);
        let mut next_cp = 0usize;
        for round in 1..=params.spins {
            // Mode of this round.
            let k = if mix_cum.len() == 1 {
                mix_cum[0].0
            } else {
                let r = rng.gen::<f64>() * total_share;
                mix_cum.iter().find(|m| r < m.1).map_or(mix_cum[mix_cum.len() - 1].0, |m| m.0)
            };
            let pm = &prepared[k];
            let cost = pm.mode.cost;
            if p.first_bonus.is_none() {
                p.pre_low = p.pre_low.min(p.won - p.wagered);
            }
            if let Some(bank) = params.bankroll {
                if bank + p.won - p.wagered < cost {
                    p.busted = true;
                    break;
                }
            }
            let roll = rng.gen_range(0..pm.mode.lut.total_weight);
            let row = pm.mode.lut.prefix.partition_point(|&c| c <= roll).min(pm.x.len() - 1);
            let x = pm.x[row];
            acc.rows[k][row] += 1;
            acc.rounds[k] += 1;

            p.wagered += cost;
            p.won += x;
            p.rounds = round;
            if x > 0.0 {
                p.hits += 1;
                dead = 0;
            } else {
                dead += 1;
                p.dead = p.dead.max(dead);
            }
            if x >= cost {
                losing = 0;
            } else {
                losing += 1;
                p.losing = p.losing.max(losing);
            }
            if x > p.best_x {
                p.best_x = x;
            }
            if p.first_10x.is_none() && x >= 10.0 {
                p.first_10x = Some(round);
            }
            if p.first_100x.is_none() && x >= 100.0 {
                p.first_100x = Some(round);
            }
            if x >= pm.max_x {
                p.max_wins += 1;
            }
            if pm.bonus[row] {
                p.bonuses += 1;
                p.first_bonus.get_or_insert(round);
                p.bonus_won += x;
                p.best_bonus = p.best_bonus.max(x);
                p.d_bonuses += 1;
                p.s_bonuses += 1;
                p.s_strong |= x >= pm.weak_x;
            }
            if round % feel.drought_spins == 0 {
                p.drought_sessions += 1;
                p.droughts += u32::from(p.d_bonuses <= feel.drought_bonuses);
                p.d_bonuses = 0;
            }
            p.r_big |= x >= pm.big_x;
            if round % feel.reach_spins == 0 {
                p.reach_sessions += 1;
                p.reach_misses += u32::from(!p.r_big);
                p.r_big = false;
            }
            if round % feel.session_spins == 0 {
                p.sessions += 1;
                if p.s_bonuses >= feel.letdown_bonuses {
                    p.multi += 1;
                    p.letdowns += u32::from(!p.s_strong);
                }
                p.s_bonuses = 0;
                p.s_strong = false;
            }
            let net = p.won - p.wagered;
            p.min_net = p.min_net.min(net);
            p.max_net = p.max_net.max(net);
            while next_cp < checkpoints.len() && checkpoints[next_cp] == round {
                acc.cp.push([net as f32, p.wagered as f32]);
                next_cp += 1;
            }
        }
        if p.first_bonus.is_none() {
            p.pre_low = p.pre_low.min(p.won - p.wagered);
        }
        // A busted player's balance stays where it stopped.
        let net = (p.won - p.wagered) as f32;
        while next_cp < checkpoints.len() {
            acc.cp.push([net, p.wagered as f32]);
            next_cp += 1;
        }
        acc.players.push(p);
    }
    acc
}

fn merge(mut a: Acc, b: Acc) -> Acc {
    a.players.extend(b.players);
    a.cp.extend(b.cp);
    for (ra, rb) in a.rows.iter_mut().zip(b.rows) {
        for (x, y) in ra.iter_mut().zip(rb) {
            *x += y;
        }
    }
    for (x, y) in a.rounds.iter_mut().zip(b.rounds) {
        *x += y;
    }
    a
}

/// Runs every scenario. Errors on an empty or out-of-range mix.
pub fn run(modes: &[SimMode], scenarios: &[Scenario], params: &CrowdParams) -> Result<Vec<ScenarioResult>, String> {
    scenarios.iter().map(|sc| run_scenario(modes, sc, params)).collect()
}

fn run_scenario(modes: &[SimMode], sc: &Scenario, params: &CrowdParams) -> Result<ScenarioResult, String> {
    let t0 = std::time::Instant::now();
    let mix: Vec<(usize, f64)> = sc.mix.iter().copied().filter(|m| m.1 > 0.0).collect();
    if mix.is_empty() {
        return Err(format!("scenario {}: no mode", sc.name));
    }
    if mix.iter().any(|m| m.0 >= modes.len()) {
        return Err(format!("scenario {}: unknown mode", sc.name));
    }
    let feel = params.feel.sanitized();
    // Only the scenario's modes are prepared (and counted).
    let used: Vec<usize> = {
        let mut u: Vec<usize> = mix.iter().map(|m| m.0).collect();
        u.sort_unstable();
        u.dedup();
        u
    };
    let prepared: Vec<Prepared> = used
        .iter()
        .map(|&i| {
            let m = &modes[i];
            let x: Vec<f64> = m.lut.rows.iter().map(|r| r.payout_x100 as f64 / 100.0).collect();
            let bonus = (0..m.lut.rows.len()).map(|j| m.facts.get(j).is_some_and(|f| f.bonus)).collect();
            let max_x = x.iter().copied().fold(0.0, f64::max);
            Prepared { mode: m, x, bonus, max_x, weak_x: feel.weak_under * m.cost, big_x: feel.big_at * m.cost }
        })
        .collect();
    let mut cum = 0.0;
    let mix_cum: Vec<(usize, f64)> = mix
        .iter()
        .map(|&(i, s)| {
            cum += s;
            (used.iter().position(|&u| u == i).unwrap(), cum)
        })
        .collect();

    let n_cp = CHECKPOINTS.min(params.spins as usize).max(1);
    let checkpoints: Vec<u32> = (1..=n_cp).map(|i| ((i as u64 * params.spins as u64) / n_cp as u64) as u32).collect();

    let players = params.players.max(1);
    #[cfg(feature = "parallel")]
    let chunks = rayon::current_num_threads().max(1) as u32;
    #[cfg(not(feature = "parallel"))]
    let chunks = 1u32;
    let chunks = chunks.min(players);
    let range = |c: u32| (c as u64 * players as u64 / chunks as u64) as u32..((c as u64 + 1) * players as u64 / chunks as u64) as u32;
    let sim = |c: u32| simulate_chunk(&prepared, &mix_cum, params, &feel, &checkpoints, range(c));
    #[cfg(feature = "parallel")]
    let acc = (0..chunks).into_par_iter().map(sim).reduce_with(merge).unwrap();
    #[cfg(not(feature = "parallel"))]
    let acc = (0..chunks).map(sim).reduce(merge).unwrap();

    let total_share: f64 = mix.iter().map(|m| m.1).sum();
    let named: Vec<(String, f64)> = mix.iter().map(|&(i, sh)| (modes[i].name.clone(), sh / total_share)).collect();
    Ok(summarise(sc, named, &prepared, &acc, params, &checkpoints, t0))
}

// ── Summary ──────────────────────────────────────────────────────────────────

fn quantile_sorted(v: &[f64], q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let i = ((v.len() - 1) as f64 * q).round() as usize;
    v[i.min(v.len() - 1)]
}

fn metric(key: &'static str, label: &'static str, unit: &'static str, mut v: Vec<f64>) -> Metric {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    Metric {
        key,
        label,
        unit,
        players: n as u32,
        mean: if n > 0 { v.iter().sum::<f64>() / n as f64 } else { 0.0 },
        min: v.first().copied().unwrap_or(0.0),
        p5: quantile_sorted(&v, 0.05),
        p25: quantile_sorted(&v, 0.25),
        p50: quantile_sorted(&v, 0.50),
        p75: quantile_sorted(&v, 0.75),
        p95: quantile_sorted(&v, 0.95),
        max: v.last().copied().unwrap_or(0.0),
    }
}

fn summarise(sc: &Scenario, mix: Vec<(String, f64)>, prepared: &[Prepared], acc: &Acc, params: &CrowdParams, checkpoints: &[u32], t0: std::time::Instant) -> ScenarioResult {
    let ps = &acc.players;
    let n = ps.len().max(1) as f64;
    let share = |f: &dyn Fn(&Player) -> bool| ps.iter().filter(|p| f(p)).count() as f64 / n;
    let wagered: f64 = ps.iter().map(|p| p.wagered).sum();
    let won: f64 = ps.iter().map(|p| p.won).sum();
    let has_bonus = prepared.iter().all(|p| p.mode.imported);

    // Theoretical RTP and its standard error, for the rounds actually played.
    let (mut th_won, mut var) = (0.0, 0.0);
    let mut modes = Vec::new();
    for (k, pm) in prepared.iter().enumerate() {
        let m = pm.mode;
        let t = m.lut.total_weight as f64;
        let mean_x = m.lut.rows.iter().map(|r| r.weight as f64 * r.payout_x100 as f64 / 100.0).sum::<f64>() / t;
        let var_x = m.lut.rows.iter().map(|r| r.weight as f64 * (r.payout_x100 as f64 / 100.0 - mean_x).powi(2)).sum::<f64>() / t;
        let rounds = acc.rounds[k];
        th_won += rounds as f64 * mean_x;
        var += rounds as f64 * var_x;
        let m_wagered = rounds as f64 * m.cost;
        let m_won: f64 = acc.rows[k].iter().zip(&pm.x).map(|(&c, &x)| c as f64 * x).sum();

        // Observed distribution: the same statistics as Analyze, weighted by draws.
        let facts: Vec<BookFacts> = m
            .facts
            .iter()
            .zip(&acc.rows[k])
            .map(|(f, &c)| BookFacts { weight: c as u64, ..*f })
            .collect();
        let mut pops = population::analyze(&facts, m.cost, m.criteria_names, &[]);
        pops.retain(|p| matches!(p.kind, "all" | "nobonus" | "bonus" | "tier" | "variant"));

        let rare = [100.0, 1_000.0, 10_000.0, pm.max_x]
            .into_iter()
            .filter(|&t| t > 0.0 && t <= pm.max_x)
            .map(|at_least| {
                let p_th = m.lut.rows.iter().filter(|r| r.payout_x100 as f64 / 100.0 >= at_least).map(|r| r.weight as f64).sum::<f64>() / t;
                let observed = acc.rows[k].iter().zip(&pm.x).filter(|(_, &x)| x >= at_least).map(|(&c, _)| c as u64).sum();
                RareCheck { at_least, expected: p_th * rounds as f64, observed }
            })
            .collect();

        modes.push(ModeObserved {
            mode: m.name.clone(),
            cost: m.cost,
            imported: m.imported,
            round_share: 0.0,
            rounds,
            wagered: m_wagered,
            won: m_won,
            rtp: if m_wagered > 0.0 { m_won / m_wagered } else { 0.0 },
            theoretical_rtp: mean_x / m.cost,
            populations: pops,
            rare,
        });
    }
    let total_rounds: u64 = acc.rounds.iter().sum();
    for m in &mut modes {
        m.round_share = if total_rounds > 0 { m.rounds as f64 / total_rounds as f64 } else { 0.0 };
    }

    let net = |p: &Player| p.won - p.wagered;
    let mut metrics = vec![
        metric("net", "Net result", "bets", ps.iter().map(net).collect()),
        metric("rtp", "Session RTP", "ratio", ps.iter().map(|p| if p.wagered > 0.0 { p.won / p.wagered } else { 0.0 }).collect()),
        metric("best", "Best win", "x", ps.iter().map(|p| p.best_x).collect()),
        metric("hit", "Hit rate", "ratio", ps.iter().map(|p| p.hits as f64 / p.rounds.max(1) as f64).collect()),
        metric("dead", "Longest run without any win", "spins", ps.iter().map(|p| p.dead as f64).collect()),
        metric("losing", "Longest run under the bet", "spins", ps.iter().map(|p| p.losing as f64).collect()),
        metric("deepest", "Deepest point", "bets", ps.iter().map(|p| p.min_net).collect()),
        metric("peak", "Highest point", "bets", ps.iter().map(|p| p.max_net).collect()),
        metric("first10", "Spins to the first 10x", "spins", ps.iter().filter_map(|p| p.first_10x.map(f64::from)).collect()),
        metric("first100", "Spins to the first 100x", "spins", ps.iter().filter_map(|p| p.first_100x.map(f64::from)).collect()),
    ];
    if has_bonus {
        metrics.extend([
            metric("bonuses", "Bonuses", "count", ps.iter().map(|p| p.bonuses as f64).collect()),
            metric("firstBonus", "Spins to the first bonus", "spins", ps.iter().filter_map(|p| p.first_bonus.map(f64::from)).collect()),
            metric("bestBonus", "Best bonus", "x", ps.iter().filter(|p| p.bonuses > 0).map(|p| p.best_bonus).collect()),
            metric("bonusShare", "Share of the win from bonuses", "ratio", ps.iter().filter(|p| p.won > 0.0).map(|p| p.bonus_won / p.won).collect()),
        ]);
    }
    if params.bankroll.is_some() {
        metrics.push(metric("rounds", "Spins played", "spins", ps.iter().map(|p| p.rounds as f64).collect()));
    }

    let mut shares = vec![
        Share { key: "profit", label: "Finish in profit", value: share(&|p| net(p) > 0.0) },
        Share { key: "lostHalf", label: "Lose more than half of what they bet", value: share(&|p| p.won < 0.5 * p.wagered) },
        Share { key: "never10", label: "Never win 10x", value: share(&|p| p.first_10x.is_none()) },
        Share { key: "never100", label: "Never win 100x", value: share(&|p| p.first_100x.is_none()) },
        Share { key: "never1000", label: "Never win 1000x", value: share(&|p| p.best_x < 1000.0) },
        Share { key: "maxWin", label: "Hit the max win", value: share(&|p| p.max_wins > 0) },
    ];
    if has_bonus {
        shares.push(Share { key: "noBonus", label: "Never trigger a bonus", value: share(&|p| p.bonuses == 0) });
    }
    if params.bankroll.is_some() {
        shares.push(Share { key: "busted", label: "Run out of balance", value: share(&|p| p.busted) });
    }

    let mut bonus_hist = vec![0.0; BONUS_HIST + 1];
    if has_bonus {
        for p in ps {
            bonus_hist[(p.bonuses as usize).min(BONUS_HIST)] += 1.0 / n;
        }
    }

    // Curves across players at each checkpoint.
    let k = checkpoints.len();
    let mut curves = Curves {
        checkpoints: checkpoints.to_vec(),
        net_p5: vec![],
        net_p25: vec![],
        net_p50: vec![],
        net_p75: vec![],
        net_p95: vec![],
        net_mean: vec![],
        rtp_p5: vec![],
        rtp_p50: vec![],
        rtp_p95: vec![],
        pop: vec![],
    };
    let mut nets = Vec::with_capacity(ps.len());
    let mut rtps = Vec::with_capacity(ps.len());
    for c in 0..k {
        nets.clear();
        rtps.clear();
        for i in 0..ps.len() {
            let [net, wag] = acc.cp[i * k + c];
            nets.push(net as f64);
            rtps.push(if wag > 0.0 { 1.0 + net as f64 / wag as f64 } else { 0.0 });
        }
        nets.sort_by(f64::total_cmp);
        rtps.sort_by(f64::total_cmp);
        curves.net_p5.push(quantile_sorted(&nets, 0.05));
        curves.net_p25.push(quantile_sorted(&nets, 0.25));
        curves.net_p50.push(quantile_sorted(&nets, 0.5));
        curves.net_p75.push(quantile_sorted(&nets, 0.75));
        curves.net_p95.push(quantile_sorted(&nets, 0.95));
        curves.net_mean.push(nets.iter().sum::<f64>() / nets.len().max(1) as f64);
        curves.rtp_p5.push(quantile_sorted(&rtps, 0.05));
        curves.rtp_p50.push(quantile_sorted(&rtps, 0.5));
        curves.rtp_p95.push(quantile_sorted(&rtps, 0.95));
        curves.pop.push(nets.iter().filter(|&&v| v > 0.0).count() as f64 / nets.len().max(1) as f64);
    }

    // Player feel of a single mode: a base game's frustrations, or a bought
    // round's (`feel::shape_for` decides).
    let feel = match prepared {
        [pm] => {
            let m = pm.mode;
            let shape = feel::shape_for(m.lut, m.facts, m.cost, m.imported, params.feel);
            let fp = shape.params();
            let sum = |f: &dyn Fn(&Player) -> u32| ps.iter().map(|p| f(p) as u64).sum::<u64>();
            let count = |f: &dyn Fn(&Player) -> bool| ps.iter().filter(|p| f(p)).count() as u64;
            let ruin = -(fp.deposit - 1.0) * m.cost;
            let crumbs_x = fp.crumbs_under * m.cost;
            let observed = feel::Observed {
                drought_sessions: sum(&|p| p.drought_sessions),
                droughts: sum(&|p| p.droughts),
                sessions: sum(&|p| p.sessions),
                multi_bonus_sessions: sum(&|p| p.multi),
                letdowns: sum(&|p| p.letdowns),
                players_drought: share(&|p| p.droughts > 0),
                players_letdown: share(&|p| p.letdowns > 0),
                dry_median: metrics.iter().find(|m| m.key == "dead").map(|m| m.p50),
                players: ps.len() as u64,
                broke: count(&|p| p.pre_low < ruin),
                undecided: count(&|p| p.first_bonus.is_none() && p.pre_low >= ruin),
                reach_sessions: sum(&|p| p.reach_sessions),
                reach_misses: sum(&|p| p.reach_misses),
                rounds: acc.rounds[0],
                crumbs_rounds: acc.rows[0].iter().zip(&pm.x).filter(|(_, &x)| x < crumbs_x).map(|(&c, _)| c as u64).sum(),
                bonus_ratio: modes[0].populations.iter().find(|p| p.kind == "bonus").filter(|p| p.mean > 0.0).map(|p| p.median / p.mean),
                bonuses: acc.rows[0].iter().zip(&pm.bonus).filter(|(_, &b)| b).map(|(&c, _)| c as u64).sum(),
            };
            let w: Vec<f64> = m.lut.rows.iter().map(|r| r.weight as f64).collect();
            Some(shape.report(&w, params.spins, observed))
        }
        _ => None,
    };

    ScenarioResult {
        name: sc.name.clone(),
        mix,
        players: ps.len() as u32,
        spins: params.spins,
        bankroll: params.bankroll,
        rounds: total_rounds,
        wagered,
        won,
        rtp: if wagered > 0.0 { won / wagered } else { 0.0 },
        theoretical_rtp: if wagered > 0.0 { th_won / wagered } else { 0.0 },
        rtp_std_error: if wagered > 0.0 { var.sqrt() / wagered } else { 0.0 },
        has_bonus,
        bonuses: ps.iter().map(|p| p.bonuses as u64).sum(),
        max_wins: ps.iter().map(|p| p.max_wins as u64).sum(),
        metrics,
        shares,
        bonus_hist,
        curves,
        modes,
        feel,
        fix: sc.fix.clone(),
        elapsed_ms: t0.elapsed().as_millis() as u64,
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

    /// RTP 0.965 : 50 % à 0, 25 % à 1x, 25 % à 2,86x (bonus).
    fn lut() -> Lut {
        Lut::new(vec![
            LutRow { sim_id: 0, weight: 50, payout_x100: 0 },
            LutRow { sim_id: 1, weight: 25, payout_x100: 100 },
            LutRow { sim_id: 2, weight: 25, payout_x100: 286 },
        ])
    }

    fn facts(l: &Lut) -> Vec<BookFacts> {
        l.rows
            .iter()
            .map(|r| BookFacts { sim_id: r.sim_id, weight: r.weight, payout_x100: r.payout_x100, bonus: r.sim_id == 2, tier: if r.sim_id == 2 { 3 } else { 0 }, ..Default::default() })
            .collect()
    }

    fn one(bankroll: Option<f64>) -> ScenarioResult {
        let l = lut();
        let f = facts(&l);
        let modes = [SimMode { name: "BASE".into(), cost: 1.0, lut: &l, facts: &f, criteria_names: &[], imported: true }];
        let sc = [Scenario { name: "BASE".into(), mix: vec![(0, 1.0)], fix: None }];
        let p = CrowdParams { players: 2_000, spins: 500, bankroll, seed: 42, feel: FeelParams::default() };
        run(&modes, &sc, &p).unwrap().remove(0)
    }

    #[test]
    fn reproducible_and_close_to_the_math() {
        let a = one(None);
        let b = one(None);
        assert_eq!(a.won, b.won);
        assert_eq!(a.rounds, 1_000_000);
        // 1 000 000 rounds: the empirical RTP is within a few standard errors.
        assert!((a.rtp - 0.965).abs() < 4.0 * a.rtp_std_error, "{} ± {}", a.rtp, a.rtp_std_error);
        assert!((a.theoretical_rtp - 0.965).abs() < 1e-9);
        // A bonus a quarter of the time: ~125 per player.
        let bonuses = a.metrics.iter().find(|m| m.key == "bonuses").unwrap();
        assert!((bonuses.mean - 125.0).abs() < 3.0, "{}", bonuses.mean);
        assert_eq!(a.bonuses as f64 / a.rounds as f64 > 0.24, true);
        // Observed distribution: the bonus tier exists and pays exactly 2.86x.
        let t3 = a.modes[0].populations.iter().find(|p| p.key == "tier:3").unwrap();
        assert_eq!(t3.median, 2.86);
        assert!((a.bonus_hist.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert_eq!(a.curves.checkpoints.len(), 50);
        assert_eq!(*a.curves.checkpoints.last().unwrap(), 500);
    }

    #[test]
    fn a_bankroll_stops_busted_players() {
        let r = one(Some(5.0));
        let busted = r.shares.iter().find(|s| s.key == "busted").unwrap().value;
        assert!(busted > 0.0 && busted < 1.0, "{busted}");
        assert!(r.rounds < 1_000_000);
    }

    #[test]
    fn a_mixed_scenario_follows_its_shares() {
        let l = lut();
        let f = facts(&l);
        let buy = Lut::new(vec![LutRow { sim_id: 7, weight: 1, payout_x100: 9_650 }]);
        let fb = vec![BookFacts { sim_id: 7, weight: 1, payout_x100: 9_650, bonus: true, tier: 3, ..Default::default() }];
        let modes = [
            SimMode { name: "BASE".into(), cost: 1.0, lut: &l, facts: &f, criteria_names: &[], imported: true },
            SimMode { name: "BONUS".into(), cost: 100.0, lut: &buy, facts: &fb, criteria_names: &[], imported: true },
        ];
        let sc = [Scenario { name: "mix".into(), mix: vec![(0, 0.9), (1, 0.1)], fix: None }];
        let r = run(&modes, &sc, &CrowdParams { players: 500, spins: 400, bankroll: None, seed: 3, feel: FeelParams::default() }).unwrap().remove(0);
        let buy_share = r.modes.iter().find(|m| m.mode == "BONUS").unwrap().round_share;
        assert!((buy_share - 0.1).abs() < 0.01, "{buy_share}");
        assert_eq!(r.mix, vec![("BASE".to_string(), 0.9), ("BONUS".to_string(), 0.1)]);
        assert!((r.theoretical_rtp - 0.965).abs() < 1e-3);
    }

    /// The sessions the cohort lives match the exact binomial figures.
    #[test]
    fn lived_sessions_match_the_exact_feel() {
        // 1 bonus in 100 spins, 40 % of them at 5x (a letdown), the rest at 50x.
        let l = Lut::new(vec![
            LutRow { sim_id: 0, weight: 7_000, payout_x100: 0 },
            LutRow { sim_id: 1, weight: 2_900, payout_x100: 100 },
            LutRow { sim_id: 2, weight: 40, payout_x100: 500 },
            LutRow { sim_id: 3, weight: 60, payout_x100: 5_000 },
        ]);
        let f: Vec<BookFacts> = l.rows.iter().map(|r| BookFacts { sim_id: r.sim_id, weight: r.weight, payout_x100: r.payout_x100, bonus: r.sim_id >= 2, ..Default::default() }).collect();
        let modes = [SimMode { name: "BASE".into(), cost: 1.0, lut: &l, facts: &f, criteria_names: &[], imported: true }];
        let feel = FeelParams { drought_spins: 200, drought_bonuses: 1, session_spins: 300, letdown_bonuses: 3, ..FeelParams::default() };
        let sc = [Scenario { name: "BASE".into(), mix: vec![(0, 1.0)], fix: None }];
        let r = run(&modes, &sc, &CrowdParams { players: 4_000, spins: 3_000, bankroll: None, seed: 5, feel }).unwrap().remove(0);
        let fr = r.feel.expect("a base mode gets a feel report");
        let o = &fr.observed;
        assert_eq!(o.drought_sessions, 4_000 * 15);
        assert_eq!(o.sessions, 4_000 * 10);
        let drought = fr.frustrations.iter().find(|x| x.key == "drought").unwrap();
        let exact = drought.value;
        assert!((exact - 0.4046).abs() < 0.002, "{exact}");
        let se = (exact * (1.0 - exact) / o.drought_sessions as f64).sqrt();
        assert!((drought.observed.unwrap() - exact).abs() < 4.0 * se, "{:?} vs {exact}", drought.observed);
        let ld = fr.frustrations.iter().find(|x| x.key == "letdown").unwrap();
        let se = (ld.value * (1.0 - ld.value) / o.multi_bonus_sessions as f64).sqrt();
        assert!((ld.observed.unwrap() - ld.value).abs() < 4.0 * se, "{:?} vs {}", ld.observed, ld.value);
        // Broke before the first bonus: the deposits the cohort lived, next to
        // the estimate (12 000 deposits played apart).
        let broke = fr.frustrations.iter().find(|x| x.key == "broke").unwrap();
        let se = (broke.value * (1.0 - broke.value) / 4_000.0).sqrt();
        assert!(broke.value > 0.1 && broke.value < 0.5, "{}", broke.value);
        assert!((broke.observed.unwrap() - broke.value).abs() < 4.0 * se + 0.01, "{:?} vs {}", broke.observed, broke.value);
        assert_eq!(o.undecided, 0);
        // A drought past the serious line comes with its fix.
        assert_eq!(drought.severity, "serious");
        assert!(drought.fix.as_ref().is_some_and(|x| x.recommended.is_some()));
    }

    #[test]
    fn convergence_approaches_analytic_rtp() {
        let c = rtp_convergence(&lut(), 1.0, 2_000_000, 7);
        assert!((c.final_rtp - 0.965).abs() < 0.01, "rtp {}", c.final_rtp);
    }
}
