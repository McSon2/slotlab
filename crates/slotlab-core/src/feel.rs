//! Player feel — what a cohort resents, measured exactly on the published
//! lookup table, and what fixing each frustration costs.
//!
//! The frustrations of a base game, in the player's words:
//!   · drought — « 1 000 spins and one bonus »: the share of `D`-spin sessions
//!               that bring `m` bonuses or fewer;
//!   · letdown — « three bonuses in my 300 spins: 5x, 10x, 15x »: when an
//!               `S`-spin session brings `k` bonuses or more, the chance that
//!               they ALL pay under `T` × the stake;
//!   · dry     — the longest run without any win over the `N` spins played,
//!               for the median player;
//!   · broke   — « I deposited 20 € and never saw the bonus »: the players who
//!               lose a deposit before their first bonus;
//!   · reach   — « 6 000 spins and never a 500x »: the sessions that never win
//!               `H` × the stake;
//!   · shape   — the typical bonus against the average one (median ÷ mean):
//!               too low, the value hides in rare huge bonuses the typical
//!               session never meets; too high, « always 15-25x ».
//! And of a bought (or costly) round:
//!   · crumbs  — « I buy at 100x and get 9x back »: the purchases returning
//!               under a quarter of their price.
//! Rounds are independent (the RGS draws each one in the LUT), so the figures
//! are exact — binomial sums for the sessions, a run-length recurrence for the
//! dry spells, (1 − p)^N for the reach — except `broke`, a first passage of
//! the balance, estimated on 12 000 deposits played on common random numbers.
//! The cohort (`crowdsim`) counts the same sessions — what the players lived —
//! next to what the math says.
//!
//! ── A fix costs RTP ─────────────────────────────────────────────────────────
//! The RTP is a fixed budget: a fix moves RTP points from one line of it to
//! another. Each fix is a re-weighting of the LUT (books untouched, so the
//! result stays publishable) that keeps the RTP, and comes in three versions,
//! one per payer:
//!   · `base`  — the base game: fewer base wins (hit rate ↓, dry spells ↑) —
//!               or, to fight dry spells, more but smaller base wins;
//!   · `big`   — the big wins (≥ `H` × the stake): rarer;
//!   · `bonus` — the bonus itself: each bonus smaller (for more bonuses), or
//!               rarer (for better bonuses).
//! (`broke`, `reach` and `crumbs` name their payers in `payers`.)
//! Every version is measured with the same exact figures, so its cost reads
//! as before → after, and the one that hurts the rest of the game least is
//! recommended.

use crate::market::{self, Figures, Peers};
use crate::model::Lut;
use crate::provenance::BookFacts;
use rand::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;
#[cfg(feature = "parallel")]
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

// ── Parameters and references ────────────────────────────────────────────────

/// What a frustration is, in spins and multiples of the stake.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FeelParams {
    /// Drought: a session of `drought_spins` spins…
    pub drought_spins: u32,
    /// …that brings at most this many bonuses.
    pub drought_bonuses: u32,
    /// Letdown: a session of `session_spins` spins…
    pub session_spins: u32,
    /// …that brings at least this many bonuses…
    pub letdown_bonuses: u32,
    /// …all paying under this multiple of the stake.
    pub weak_under: f64,
    /// A big win, in multiples of the stake.
    pub big_at: f64,
    /// Broke: a deposit of this many stakes.
    pub deposit: f64,
    /// Reach: sessions of this many spins that never win `big_at`.
    pub reach_spins: u32,
    /// Crumbs: a purchase returning under this share of its price…
    pub crumbs_under: f64,
    /// …and one that pops, at this multiple of its price.
    pub pop_at: f64,
    /// Purchases in a row that never refund the price.
    pub buy_streak: u32,
}

impl Default for FeelParams {
    fn default() -> Self {
        FeelParams {
            drought_spins: 1_000,
            drought_bonuses: 1,
            session_spins: 300,
            letdown_bonuses: 3,
            weak_under: 20.0,
            big_at: 500.0,
            deposit: 100.0,
            reach_spins: 6_000,
            crumbs_under: 0.25,
            pop_at: 10.0,
            buy_streak: 5,
        }
    }
}

impl FeelParams {
    /// Out-of-range values brought back to something measurable.
    pub fn sanitized(self) -> Self {
        let d = FeelParams::default();
        let pos = |v: f64, dflt: f64| if v.is_finite() && v > 0.0 { v } else { dflt };
        let weak_under = pos(self.weak_under, d.weak_under);
        FeelParams {
            drought_spins: self.drought_spins.clamp(10, 100_000),
            drought_bonuses: self.drought_bonuses.min(50),
            session_spins: self.session_spins.clamp(10, 100_000),
            letdown_bonuses: self.letdown_bonuses.clamp(1, 50),
            weak_under,
            big_at: pos(self.big_at, d.big_at).max(weak_under * 2.0),
            deposit: pos(self.deposit, d.deposit).clamp(2.0, 1_000_000.0),
            reach_spins: self.reach_spins.clamp(100, 1_000_000),
            crumbs_under: pos(self.crumbs_under, d.crumbs_under).clamp(0.01, 0.95),
            pop_at: pos(self.pop_at, d.pop_at).clamp(1.5, 10_000.0),
            buy_streak: self.buy_streak.clamp(1, 100),
        }
    }
}

/// When a frustration deserves a look, and where a fix aims. SlotLab judgement
/// calls (not Stake rules), anchored on the market figures of
/// `stake-engine-math/references/player-feel-market.md`: natural bonus
/// 1/202-312, bonus median 31-56x, base hit rate ~19 %.
pub struct Reference {
    pub minor: f64,
    pub serious: f64,
    pub target: f64,
}

/// Share of drought sessions. 1 bonus in 300 spins → 15.5 %; 1 in 420 → 31 %.
pub const DROUGHT: Reference = Reference { minor: 0.15, serious: 0.30, target: 0.10 };
/// Letdown chance. ≈ (share of weak bonuses)^k: 37 % weak → 5 %, 49 % → 12 %.
pub const LETDOWN: Reference = Reference { minor: 0.05, serious: 0.12, target: 0.03 };
/// Median player's longest run without a win, in spins (same thresholds as the
/// CrowdSim reading).
pub const DRY: Reference = Reference { minor: 40.0, serious: 80.0, target: 36.0 };
/// Players who lose a 100-stake deposit before their first bonus. A
/// market-shaped base game (bonus 1 in 250 with a 40x median, hit rate 19 %,
/// 62.5 RTP points in the base game) loses 42 % of them
/// (`tests::market_shaped_broke`): the target sits there, the alarm ten
/// points above.
pub const BROKE: Reference = Reference { minor: 0.50, serious: 0.65, target: 0.40 };
/// Sessions of 6 000 spins that never win 500x. Market (StakeCruncher, 109
/// base modes): p25 20 %, median 31 %, p75 50 %.
pub const REACH: Reference = Reference { minor: 0.50, serious: 0.70, target: 0.31 };
/// Purchases returning under a quarter of their price. Market (107 main buys):
/// p25 25.5 %, median 37 %, p75 49 %.
pub const CRUMBS: Reference = Reference { minor: 0.49, serious: 0.60, target: 0.37 };
/// A costly mode where this share of the rounds pays nothing is all-or-nothing
/// by design (DEGEN, HIDDEN…): its crumbs are its identity, not a flaw.
pub const BUST_MODE: f64 = 0.25;
/// Median ÷ average of the natural bonus. Tracked spins of 142 volatile slots
/// (clashofslots): 0.42–0.60. Below, the average is carried by rare huge
/// bonuses; above, the bonus never surprises. Serious past 0.25 / 0.75.
pub const SHAPE_BAND: (f64, f64) = (0.42, 0.60);
pub const SHAPE_SERIOUS: (f64, f64) = (0.25, 0.75);

// ── A mode, ready to be measured under any weights ───────────────────────────

/// Row groups: where a re-weighting takes or gives. `Weak`/`Mid`/`BonusBig`
/// are bonus books, split at `weak_under` and `big_at`. `Max`: the books at
/// the max win, bonus or not — pinned: every fix keeps its odds (players and
/// the Stake review both read them; the market's median sits near 1 in 2M).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum G {
    Dead = 0,
    Base = 1,
    BaseBig = 2,
    Weak = 3,
    Mid = 4,
    BonusBig = 5,
    Max = 6,
}

const NG: usize = 7;
const GROUP_KEYS: [&str; NG] = ["dead", "base", "baseBig", "weakBonus", "bonus", "bigBonus", "maxWin"];

/// The bonus groups a fix may move (the max win is pinned).
fn is_bonus_group(g: usize) -> bool {
    (G::Weak as usize..=G::BonusBig as usize).contains(&g)
}

/// A base game (bonuses trigger among ordinary spins) or a bought round
/// (every round is the event; `Weak`/`Mid`/`BonusBig` then read crumbs /
/// middle / pops, split at `crumbs_under` and `pop_at` × the price).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Base,
    Buy,
}

/// One mode as this module reads it: payouts, groups, and the bonus (and
/// non-bonus) rows in payout order (weighted median, base-game walks).
pub struct Shape {
    kind: Kind,
    x: Vec<f64>,
    group: Vec<u8>,
    /// A bonus book (for the figures: the max win keeps its nature).
    bonus_flag: Vec<bool>,
    bonus_order: Vec<u32>,
    nb_order: Vec<u32>,
    cost: f64,
    /// Bonus flags come from imported books.
    has_bonus: bool,
    weak_x: f64,
    big_x: f64,
    params: FeelParams,
    /// Market band of each row: 0 nothing, 1 under 100x, 2 100-500x,
    /// 3 500-1 000x, 4 1 000-5 000x, 5 5 000x up to the max, 6 the max.
    mband: Vec<u8>,
    /// The market's games of a close max win (a base mode at a 1x stake).
    peers: Option<Peers>,
}

/// Tail thresholds of the market ladder, in multiples of the stake.
const LADDER: [f64; 4] = [100.0, 500.0, 1_000.0, 5_000.0];

impl Shape {
    /// A base game. `facts`: one per LUT row, same order (`bonus` stays false
    /// when the books are not imported — say it with `has_bonus`).
    pub fn new(lut: &Lut, facts: &[BookFacts], cost: f64, has_bonus: bool, params: FeelParams) -> Self {
        let params = params.sanitized();
        let cost = if cost > 0.0 { cost } else { 1.0 };
        Self::build(Kind::Base, lut, facts, cost, has_bonus, params.weak_under * cost, params.big_at * cost, params)
    }

    /// A bought (or costly) round: every row is the event, read by its payout
    /// against the price — the books are not needed.
    pub fn buy(lut: &Lut, cost: f64, params: FeelParams) -> Self {
        let params = params.sanitized();
        let cost = if cost > 0.0 { cost } else { 1.0 };
        Self::build(Kind::Buy, lut, &[], cost, true, params.crumbs_under * cost, params.pop_at * cost, params)
    }

    #[allow(clippy::too_many_arguments)]
    fn build(kind: Kind, lut: &Lut, facts: &[BookFacts], cost: f64, has_bonus: bool, weak_x: f64, big_x: f64, params: FeelParams) -> Self {
        let x: Vec<f64> = lut.rows.iter().map(|r| r.payout_x100 as f64 / 100.0).collect();
        let bonus = |i: usize| kind == Kind::Buy || (has_bonus && facts.get(i).is_some_and(|f| f.bonus));
        let bonus_flag: Vec<bool> = (0..x.len()).map(bonus).collect();
        // The max win as the LUT draws it.
        let top = x.iter().zip(&lut.rows).filter(|(_, r)| r.weight > 0).map(|(&v, _)| v).fold(0.0, f64::max);
        let group: Vec<u8> = x
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let g = if top > 0.0 && v >= top {
                    G::Max
                } else if bonus(i) {
                    if v < weak_x {
                        G::Weak
                    } else if v < big_x {
                        G::Mid
                    } else {
                        G::BonusBig
                    }
                } else if v <= 0.0 {
                    G::Dead
                } else if v < big_x {
                    G::Base
                } else {
                    G::BaseBig
                };
                g as u8
            })
            .collect();
        let order = |bonus: bool| {
            let mut v: Vec<u32> = (0..x.len()).filter(|&i| bonus_flag[i] == bonus).map(|i| i as u32).collect();
            v.sort_by(|&a, &b| x[a as usize].total_cmp(&x[b as usize]));
            v
        };
        let (bonus_order, nb_order) = (order(true), order(false));
        let mband: Vec<u8> = x
            .iter()
            .map(|&v| {
                if v <= 0.0 {
                    0
                } else if top > 0.0 && v >= top {
                    6
                } else {
                    1 + LADDER.iter().filter(|&&t| v >= t * cost).count() as u8
                }
            })
            .collect();
        // The market's peers: base modes of a close max win; an ante (« double
        // chance ») among the antes of the same cost.
        let peers = match kind {
            Kind::Base if (cost - 1.0).abs() < 1e-9 => market::peers(top),
            Kind::Base => market::peers_for("ante", cost, top),
            Kind::Buy => None,
        };
        Shape { kind, x, group, bonus_flag, bonus_order, nb_order, cost, has_bonus, weak_x, big_x, params, mband, peers }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn params(&self) -> FeelParams {
        self.params
    }

    fn sums(&self, w: &[f64]) -> Sums {
        let mut s = Sums::default();
        for (((&wi, &xi), &g), &b) in w.iter().zip(&self.x).zip(&self.group).zip(&self.bonus_flag) {
            let g = g as usize;
            s.mass[g] += wi;
            s.value[g] += wi * xi;
            if xi > 0.0 {
                s.hit[g] += wi;
            }
            if b {
                s.bmass[g] += wi;
                s.bvalue[g] += wi * xi;
            }
        }
        s
    }
}

/// Per group: weight, weight × payout, weight of the paying rows, and the
/// bonus books' weight and value.
#[derive(Debug, Clone, Default)]
struct Sums {
    mass: [f64; NG],
    value: [f64; NG],
    hit: [f64; NG],
    bmass: [f64; NG],
    bvalue: [f64; NG],
}

impl Sums {
    fn total(&self) -> f64 {
        self.mass.iter().sum()
    }
    fn total_value(&self) -> f64 {
        self.value.iter().sum()
    }
    fn bonus_mass(&self) -> f64 {
        self.bmass.iter().sum()
    }
    fn bonus_value(&self) -> f64 {
        self.bvalue.iter().sum()
    }
    /// The bonus a fix may move: every bonus book but the max win's.
    fn free_bonus(&self) -> (f64, f64) {
        let r = G::Weak as usize..=G::BonusBig as usize;
        (r.clone().map(|g| self.mass[g]).sum(), r.map(|g| self.value[g]).sum())
    }
    fn hit_mass(&self) -> f64 {
        self.hit.iter().sum()
    }
}

// ── Exact figures ────────────────────────────────────────────────────────────

/// The felt figures of a mode under some weights. Bonus figures are `None`
/// when the books are not imported (or the mode has no bonus).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeelMetrics {
    pub rtp: f64,
    /// Standard deviation of a round ÷ the stake.
    pub volatility: f64,
    pub hit_rate: f64,
    /// Share of the wins paying less than the stake (« a win that loses »).
    pub wins_under_stake: f64,
    /// Spins per bonus.
    pub bonus_one_in: Option<f64>,
    /// Whole book (base spin included), in base bets.
    pub bonus_mean: Option<f64>,
    pub bonus_median: Option<f64>,
    /// Median ÷ average bonus — the volatility dial of Stake Engine: the
    /// lower, the more the typical bonus sits under what the math pays on
    /// average (market 0.42-0.60; « always 15-25x » bonuses sit far above).
    pub bonus_ratio: Option<f64>,
    /// Share of the bonuses paying at least `big_x`.
    pub big_bonus_share: Option<f64>,
    /// Share of the bonuses paying under `weak_x`.
    pub weak_share: Option<f64>,
    /// Share of drought sessions.
    pub drought: Option<f64>,
    /// P(every bonus < `weak_x` | the session brings ≥ k bonuses).
    pub letdown: Option<f64>,
    /// Share of ALL sessions that end that way.
    pub letdown_sessions: Option<f64>,
    /// Longest run without any win over the spins played: median, P95.
    pub dry_median: f64,
    pub dry_p95: f64,
    /// Players losing `deposit` stakes before their first bonus (estimated).
    pub broke: Option<f64>,
    /// Spins per win ≥ `big_x` (`None`: never).
    pub big_one_in: Option<f64>,
    /// Spins per win ≥ 100, 500, 1 000 and 5 000 × the stake — the
    /// market's ladder (`None`: never).
    pub one_in_100: Option<f64>,
    pub one_in_500: Option<f64>,
    pub one_in_1000: Option<f64>,
    pub one_in_5000: Option<f64>,
    /// Share of `reach_spins`-spin sessions that never win `big_x`.
    pub never_big: f64,
    pub max_win: f64,
    pub max_one_in: Option<f64>,
    /// Share of the rounds paying nothing.
    pub zero_share: f64,
    // ── A bought round (`Kind::Buy`), against its price ──
    /// Median payout ÷ price.
    pub buy_median: Option<f64>,
    /// Share of purchases under `crumbs_under` × the price, and under 10 %.
    pub crumbs: Option<f64>,
    pub near_zero: Option<f64>,
    /// Share of purchases paying back at least the price.
    pub refund: Option<f64>,
    /// Purchases per payout ≥ `pop_at` × the price, and ≥ 50 × the price.
    pub pop_one_in: Option<f64>,
    pub pop50_one_in: Option<f64>,
    /// Chance that `buy_streak` purchases in a row never refund the price.
    pub never_refunded: Option<f64>,
    /// RTP points carried by each group (`GROUP_KEYS` order), ×100.
    pub budget: Vec<f64>,
}

/// ln P(n) for n = 0..=cap of Binomial(trials, p), by recurrence (no factorial,
/// no underflow). `p` in (0, 1).
fn binom_log_pmf(trials: u32, p: f64, cap: u32) -> Vec<f64> {
    let cap = cap.min(trials);
    let mut out = Vec::with_capacity(cap as usize + 1);
    let mut lp = trials as f64 * (-p).ln_1p();
    let step = (p / (1.0 - p)).ln();
    out.push(lp);
    for n in 1..=cap {
        lp += ((trials - n + 1) as f64 / n as f64).ln() + step;
        out.push(lp);
    }
    out
}

fn log_sum_exp(v: impl Iterator<Item = f64>) -> f64 {
    let v: Vec<f64> = v.filter(|x| x.is_finite()).collect();
    let m = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !m.is_finite() {
        return f64::NEG_INFINITY;
    }
    m + v.iter().map(|x| (x - m).exp()).sum::<f64>().ln()
}

impl Figures for FeelMetrics {
    fn figure(&self, key: &str) -> Option<f64> {
        let p = |o: Option<f64>| o.map_or(0.0, |v| 1.0 / v);
        Some(match key {
            "hit" => self.hit_rate,
            "winsUnderStake" => self.wins_under_stake,
            "p100" => p(self.one_in_100),
            "p500" => p(self.one_in_500),
            "p1000" => p(self.one_in_1000),
            "p5000" => p(self.one_in_5000),
            "pmax" => p(self.max_one_in),
            "never500" => market::never(p(self.one_in_500)),
            "never1000" => market::never(p(self.one_in_1000)),
            "vol" => self.volatility,
            _ => return None,
        })
    }
}

/// P(Binomial(spins, p) ≤ at_most).
pub fn drought_share(spins: u32, p: f64, at_most: u32) -> f64 {
    if p <= 0.0 {
        return 1.0;
    }
    if p >= 1.0 {
        return if at_most >= spins { 1.0 } else { 0.0 };
    }
    binom_log_pmf(spins, p, at_most).into_iter().map(f64::exp).sum::<f64>().min(1.0)
}

/// (P(all k+ bonuses weak | ≥ k bonuses), P(≥ k bonuses and all weak)) for a
/// session of `spins` spins, bonus probability `p`, weak share `q`.
pub fn letdown(spins: u32, p: f64, q: f64, k: u32) -> (f64, f64) {
    if p <= 0.0 || k > spins {
        return (0.0, 0.0);
    }
    let q = q.clamp(0.0, 1.0);
    if p >= 1.0 {
        return (q.powi(spins as i32), q.powi(spins as i32));
    }
    let lp = binom_log_pmf(spins, p, spins);
    let lq = if q > 0.0 { q.ln() } else { f64::NEG_INFINITY };
    let den = log_sum_exp((k..=spins).map(|n| lp[n as usize]));
    let num = log_sum_exp((k..=spins).map(|n| lp[n as usize] + n as f64 * lq));
    if !den.is_finite() || !num.is_finite() {
        return (if q >= 1.0 { 1.0 } else { 0.0 }, 0.0);
    }
    ((num - den).exp().min(1.0), num.exp().min(1.0))
}

/// P(no run of `len` losing spins within `spins` spins), hit probability `h`:
/// S(n) = S(n−1) − h·r^len·S(n−len−1), S(n < len) = 1, S(len) = 1 − r^len.
fn no_run_of(spins: u32, h: f64, len: u32) -> f64 {
    if len == 0 {
        return 0.0;
    }
    if len > spins {
        return 1.0;
    }
    let r = 1.0 - h;
    let rl = r.powi(len as i32);
    let (n_all, l) = (spins as usize, len as usize);
    let mut s = vec![1.0_f64; n_all + 1];
    s[l] = 1.0 - rl;
    for n in l + 1..=n_all {
        s[n] = (s[n - 1] - h * rl * s[n - l - 1]).max(0.0);
    }
    s[n_all]
}

/// Quantile `q` of the longest run without a win over `spins` spins: the
/// smallest M with P(longest ≤ M) ≥ q.
pub fn dry_quantile(spins: u32, h: f64, q: f64) -> f64 {
    if h <= 0.0 {
        return spins as f64;
    }
    if h >= 1.0 {
        return 0.0;
    }
    // P(longest ≤ M) = no_run_of(M + 1), increasing in M.
    let (mut lo, mut hi) = (0u32, spins);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if no_run_of(spins, h, mid + 1) >= q {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo as f64
}

/// Deposits played by `broke_share`, and their seed (common random numbers).
const DEPOSITS: u32 = 12_000;
const DEPOSIT_SEED: u64 = 0xB0B0_5EED_F00D;

impl Shape {
    /// Players who lose `deposit` stakes before their first bonus. The base
    /// game alone (the non-bonus spins) is played from the deposit until the
    /// balance cannot pay a spin; each spin paid escapes the bonus with
    /// probability 1 − p, so the chance to go broke first is E[(1 − p)^T],
    /// T = the spins paid. Every set of weights plays the same deposits — the
    /// same random draws, through its own distribution — so two versions of a
    /// mode differ by their weights, not by luck.
    fn broke_share(&self, w: &[f64], p: f64) -> f64 {
        // The base game's payouts, merged by value.
        let (mut vals, mut cum) = (Vec::new(), Vec::new());
        let mut acc = 0.0;
        for &i in &self.nb_order {
            let (wi, xi) = (w[i as usize], self.x[i as usize]);
            if wi <= 0.0 {
                continue;
            }
            acc += wi;
            if vals.last() == Some(&xi) {
                *cum.last_mut().unwrap() = acc;
            } else {
                vals.push(xi);
                cum.push(acc);
            }
        }
        if acc <= 0.0 || p >= 1.0 {
            return 0.0;
        }
        let c = self.cost;
        let start = self.params.deposit * c;
        let ln_q = (-p).ln_1p();
        // Past this, (1 − p)^T < 1e-6: the bonus has come.
        let cap = ((1e-6f64).ln() / ln_q).ceil().min(1e7) as u32;
        let walk = |i: u32| -> f64 {
            let mut rng = Xoshiro256PlusPlus::seed_from_u64(DEPOSIT_SEED ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let (mut bal, mut t) = (start, 0u32);
            while bal >= c - 1e-9 && t < cap {
                let u = rng.gen::<f64>() * acc;
                bal += vals[cum.partition_point(|&v| v <= u).min(vals.len() - 1)] - c;
                t += 1;
            }
            (t as f64 * ln_q).exp()
        };
        // Collected, then summed in order: the same figure on every run.
        #[cfg(feature = "parallel")]
        let v: Vec<f64> = (0..DEPOSITS).into_par_iter().map(walk).collect();
        #[cfg(not(feature = "parallel"))]
        let v: Vec<f64> = (0..DEPOSITS).map(walk).collect();
        v.iter().sum::<f64>() / DEPOSITS as f64
    }

    /// Every felt figure under the weights `w` (one per row), `spins` = the
    /// length of a player's run.
    pub fn measure(&self, w: &[f64], spins: u32) -> FeelMetrics {
        self.measure_opts(w, spins, true)
    }

    /// `measure` — without the deposits (`broke`) when `with_broke` is false:
    /// they are the one slow figure, and a search often does not need them.
    fn measure_opts(&self, w: &[f64], spins: u32, with_broke: bool) -> FeelMetrics {
        let p = &self.params;
        let s = self.sums(w);
        let total = s.total().max(f64::MIN_POSITIVE);
        let value = s.total_value();
        let mean = value / total;
        let c = self.cost;
        let (mut var, mut under, mut zero, mut tenth, mut refund, mut pop50) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let mut ladder = [0.0_f64; 4];
        for (&wi, &xi) in w.iter().zip(&self.x) {
            var += wi * (xi - mean).powi(2);
            for (l, t) in ladder.iter_mut().zip(LADDER) {
                if xi >= t * c {
                    *l += wi;
                }
            }
            if xi > 0.0 && xi < c {
                under += wi;
            }
            if xi <= 0.0 {
                zero += wi;
            }
            if xi < 0.1 * c {
                tenth += wi;
            }
            if xi >= c {
                refund += wi;
            }
            if xi >= 50.0 * c {
                pop50 += wi;
            }
        }
        let var = var / total;
        let hit_rate = s.hit_mass() / total;

        let bonus_mass = s.bonus_mass();
        let buy = self.kind == Kind::Buy;
        let bonus = !buy && self.has_bonus && bonus_mass > 0.0;
        let pb = bonus_mass / total;
        let q = if bonus { s.mass[G::Weak as usize] / bonus_mass } else { 0.0 };
        let (ld, ld_sessions) = letdown(p.session_spins, pb, q, p.letdown_bonuses);
        let median = || {
            let half = bonus_mass / 2.0;
            let mut cum = 0.0;
            for &i in &self.bonus_order {
                cum += w[i as usize];
                if cum >= half - 1e-9 * bonus_mass && w[i as usize] > 0.0 {
                    return self.x[i as usize];
                }
            }
            self.bonus_order.last().map_or(0.0, |&i| self.x[i as usize])
        };

        let max_g = G::Max as usize;
        let big_mass: f64 = s.mass[G::BaseBig as usize] + s.mass[G::BonusBig as usize] + s.mass[max_g];
        let p_big = big_mass / total;
        let bonus_mean = s.bonus_value() / bonus_mass.max(f64::MIN_POSITIVE);
        let bonus_median = bonus.then(median);
        let pops = s.mass[G::BonusBig as usize] + s.mass[max_g];
        // The max win as drawn: a book the weights never draw does not count.
        let max_x = w.iter().zip(&self.x).filter(|(&wi, _)| wi > 0.0).map(|(_, &xi)| xi).fold(0.0, f64::max);
        let max_mass: f64 = w.iter().zip(&self.x).filter(|(_, &xi)| max_x > 0.0 && xi >= max_x).map(|(&wi, _)| wi).sum();

        FeelMetrics {
            rtp: value / total / self.cost,
            volatility: var.sqrt() / self.cost,
            hit_rate,
            wins_under_stake: if s.hit_mass() > 0.0 { under / s.hit_mass() } else { 0.0 },
            bonus_one_in: bonus.then(|| 1.0 / pb),
            bonus_mean: bonus.then_some(bonus_mean),
            bonus_median,
            bonus_ratio: bonus_median.filter(|_| bonus_mean > 0.0).map(|m| m / bonus_mean),
            big_bonus_share: bonus.then(|| (s.mass[G::BonusBig as usize] + s.bmass[max_g]) / bonus_mass),
            weak_share: bonus.then_some(q),
            drought: bonus.then(|| drought_share(p.drought_spins, pb, p.drought_bonuses)),
            letdown: bonus.then_some(ld),
            letdown_sessions: bonus.then_some(ld_sessions),
            dry_median: dry_quantile(spins, hit_rate, 0.5),
            dry_p95: dry_quantile(spins, hit_rate, 0.95),
            broke: (bonus && with_broke).then(|| self.broke_share(w, pb)),
            big_one_in: (p_big > 0.0).then(|| 1.0 / p_big),
            one_in_100: (ladder[0] > 0.0).then(|| total / ladder[0]),
            one_in_500: (ladder[1] > 0.0).then(|| total / ladder[1]),
            one_in_1000: (ladder[2] > 0.0).then(|| total / ladder[2]),
            one_in_5000: (ladder[3] > 0.0).then(|| total / ladder[3]),
            never_big: if p_big >= 1.0 { 0.0 } else { (p.reach_spins as f64 * (-p_big).ln_1p()).exp() },
            max_win: max_x,
            max_one_in: (max_mass > 0.0).then(|| total / max_mass),
            zero_share: zero / total,
            buy_median: buy.then(|| median() / c),
            crumbs: buy.then(|| s.mass[G::Weak as usize] / total),
            near_zero: buy.then(|| tenth / total),
            refund: buy.then(|| refund / total),
            pop_one_in: (buy && pops > 0.0).then(|| total / pops),
            pop50_one_in: (buy && pop50 > 0.0).then(|| total / pop50),
            never_refunded: buy.then(|| (1.0 - refund / total).max(0.0).powi(p.buy_streak as i32)),
            budget: (0..NG).map(|g| s.value[g] / total / self.cost * 100.0).collect(),
        }
    }
}

// ── Re-weighting ─────────────────────────────────────────────────────────────

/// A group's factor: `scale` × (unknown `var`, or 1).
#[derive(Clone, Copy)]
struct Coef {
    var: Option<usize>,
    scale: f64,
}

const KEEP: Coef = Coef { var: None, scale: 1.0 };

fn fixed(scale: f64) -> Coef {
    Coef { var: None, scale }
}

fn unknown(k: usize) -> Coef {
    Coef { var: Some(k), scale: 1.0 }
}

fn scaled(k: usize, scale: f64) -> Coef {
    Coef { var: Some(k), scale }
}

#[derive(Clone, Copy)]
enum Sum {
    Mass,
    Value,
    Hit,
    Bonus,
}

impl Sums {
    fn of(&self, kind: Sum, g: usize) -> f64 {
        match kind {
            Sum::Mass => self.mass[g],
            Sum::Value => self.value[g],
            Sum::Hit => self.hit[g],
            Sum::Bonus => self.bmass[g],
        }
    }
}

/// Group factors meeting every constraint (Σ_g factor_g · sum_g = target), one
/// unknown per constraint. `None`: no solution — an unknown with nothing
/// under it (a payer that does not exist in this mode).
fn solve(s: &Sums, coefs: &[Coef; NG], constraints: &[(Sum, f64)]) -> Option<[f64; NG]> {
    let n = constraints.len();
    let mut a = vec![vec![0.0_f64; n + 1]; n];
    for (r, &(kind, target)) in constraints.iter().enumerate() {
        a[r][n] = target;
        for (g, c) in coefs.iter().enumerate() {
            let v = c.scale * s.of(kind, g);
            match c.var {
                Some(k) => a[r][k] += v,
                None => a[r][n] -= v,
            }
        }
    }
    // Columns are scaled apart (a weight, a value): the tolerance is per column.
    let col_max: Vec<f64> = (0..n).map(|c| a.iter().map(|row| row[c].abs()).fold(0.0, f64::max)).collect();
    for c in 0..n {
        let piv = (c..n).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        let p = a[piv][c].abs();
        if col_max[c] == 0.0 || p.is_nan() || p <= 1e-12 * col_max[c] {
            return None;
        }
        a.swap(c, piv);
        let pivot = a[c].clone();
        for (r, row) in a.iter_mut().enumerate() {
            if r != c {
                let f = row[c] / pivot[c];
                for (x, pv) in row.iter_mut().zip(&pivot).skip(c) {
                    *x -= f * pv;
                }
            }
        }
    }
    let x: Vec<f64> = (0..n).map(|r| a[r][n] / a[r][r]).collect();
    let mut out = [1.0; NG];
    for (g, c) in coefs.iter().enumerate() {
        out[g] = c.scale * c.var.map_or(1.0, |k| x[k]);
    }
    out.iter().all(|f| f.is_finite()).then_some(out)
}

/// Why a set of group factors cannot be played: a group asked to give more
/// than it has.
fn check_factors(s: &Sums, f: &[f64; NG]) -> Result<(), String> {
    for (g, &fg) in f.iter().enumerate() {
        if s.mass[g] > 0.0 && fg < -1e-9 {
            return Err(match g {
                0 => "not enough losing spins to make room for it".into(),
                1 => "the base game does not carry enough RTP to pay for it".into(),
                2 | 5 => "the big wins do not carry enough RTP to pay for it".into(),
                _ => "the bonus does not carry enough RTP to pay for it".into(),
            });
        }
    }
    Ok(())
}

/// Where a re-weighting lands on the payer's side.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    /// One of `dead`, `base`, `baseBig`, `weakBonus`, `bonus`, `bigBonus`.
    pub group: &'static str,
    /// New weight of the group ÷ old: how much more (or less) often it lands.
    pub mass: f64,
    /// New average win of the group ÷ old.
    pub mean: f64,
}

impl Shape {
    fn apply(&self, w: &[f64], f: &[f64; NG]) -> Vec<f64> {
        w.iter().zip(&self.group).map(|(&wi, &g)| wi * f[g as usize].max(0.0)).collect()
    }

    /// Re-weights the six movable groups (`coefs`); the max win stays.
    fn by_groups(&self, w: &[f64], coefs: &[Coef; 6], constraints: &[(Sum, f64)], missing: &str) -> Result<Vec<f64>, String> {
        let s = self.sums(w);
        let all = [coefs[0], coefs[1], coefs[2], coefs[3], coefs[4], coefs[5], KEEP];
        let f = solve(&s, &all, constraints).ok_or_else(|| missing.to_string())?;
        check_factors(&s, &f)?;
        Ok(self.apply(w, &f))
    }

    /// Weights of the rows `rows` tilted by e^{a·z} (z = log payout scaled to
    /// [0, 1] over the rows) and rescaled to `mass`, `a` solved so that their
    /// average payout is `mean`. Every other row keeps its weight.
    fn tilt(&self, w: &[f64], rows: &[usize], mass: f64, mean: f64) -> Result<Vec<f64>, String> {
        let live: Vec<usize> = rows.iter().copied().filter(|&i| w[i] > 0.0).collect();
        if live.is_empty() || mass <= 0.0 {
            return Err("nothing to re-shape".into());
        }
        let (lo_x, hi_x) = live.iter().fold((f64::MAX, f64::MIN), |(l, h), &i| (l.min(self.x[i]), h.max(self.x[i])));
        if mean < lo_x || mean > hi_x {
            return Err(format!("it would need an average of {mean:.2}x, outside what these books pay ({lo_x:.2}x – {hi_x:.2}x)"));
        }
        let min_pos = live.iter().map(|&i| self.x[i]).filter(|&v| v > 0.0).fold(f64::MAX, f64::min);
        let ln = |v: f64| if v > 0.0 { v.ln() } else if min_pos < f64::MAX { min_pos.ln() - 1.0 } else { 0.0 };
        let (zl, zh) = live.iter().fold((f64::MAX, f64::MIN), |(l, h), &i| (l.min(ln(self.x[i])), h.max(ln(self.x[i]))));
        let span = (zh - zl).max(1e-9);
        let z: Vec<f64> = live.iter().map(|&i| (ln(self.x[i]) - zl) / span).collect();
        // Σ w·e^{a·z} and Σ w·x·e^{a·z} in one pass (shifted: no overflow).
        let sums = |a: f64| -> (f64, f64) {
            let top = a.max(0.0);
            live.iter().zip(&z).fold((0.0, 0.0), |(m, v), (&i, &zi)| {
                let e = w[i] * (a * zi - top).exp();
                (m + e, v + e * self.x[i])
            })
        };
        let gap = |a: f64| {
            let (m, v) = sums(a);
            v / m - mean
        };
        // The tilted mean grows with `a`: a few halvings to close in, then
        // regula falsi (Illinois) — a handful of passes instead of hundreds.
        let (mut lo, mut hi) = (-400.0_f64, 400.0_f64);
        for _ in 0..12 {
            let mid = 0.5 * (lo + hi);
            if gap(mid) < 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let (mut f_lo, mut f_hi) = (gap(lo), gap(hi));
        let tol = 1e-11 * mean.abs().max(1e-9);
        let mut a = 0.5 * (lo + hi);
        let mut side = 0;
        for _ in 0..60 {
            if f_hi == f_lo {
                break;
            }
            a = (lo * f_hi - hi * f_lo) / (f_hi - f_lo);
            let fa = gap(a);
            if fa.abs() <= tol {
                break;
            }
            if fa > 0.0 {
                hi = a;
                f_hi = fa;
                if side == -1 {
                    f_lo *= 0.5;
                }
                side = -1;
            } else {
                lo = a;
                f_lo = fa;
                if side == 1 {
                    f_hi *= 0.5;
                }
                side = 1;
            }
        }
        let top = a.max(0.0);
        let t: Vec<f64> = live.iter().zip(&z).map(|(&i, &zi)| w[i] * (a * zi - top).exp()).collect();
        let sum: f64 = t.iter().sum();
        if sum.is_nan() || sum <= 0.0 || gap(a).abs() > 1e-6 * mean.max(1e-9) {
            return Err(format!("the books cannot average {mean:.2}x"));
        }
        let mut out = w.to_vec();
        for &i in rows {
            out[i] = 0.0;
        }
        for (&i, &ti) in live.iter().zip(&t) {
            out[i] = ti * mass / sum;
        }
        Ok(out)
    }

    fn rows_where(&self, f: impl Fn(usize) -> bool) -> Vec<usize> {
        (0..self.x.len()).filter(|&i| f(self.group[i] as usize)).collect()
    }

    fn moves(&self, before: &[f64], after: &[f64]) -> Vec<Move> {
        let (b, a) = (self.sums(before), self.sums(after));
        (0..NG)
            .filter(|&g| b.mass[g] > 0.0)
            .map(|g| {
                let mass = a.mass[g] / b.mass[g];
                let mean = if a.mass[g] > 0.0 && b.value[g] > 0.0 { (a.value[g] / a.mass[g]) / (b.value[g] / b.mass[g]) } else { 1.0 };
                Move { group: GROUP_KEYS[g], mass, mean }
            })
            .filter(|m| (m.mass - 1.0).abs() > 0.005 || (m.mean - 1.0).abs() > 0.005)
            .collect()
    }
}

// ── Frustrations and their fixes ─────────────────────────────────────────────

pub const FRUSTRATIONS: [&str; 7] = ["drought", "letdown", "dry", "broke", "reach", "crumbs", "shape"];

/// Who can pay for a frustration's fix, in display order:
///   · `base` / `big` / `bonus` — the base game, the big wins, the bonus;
///   · broke: `bonus` (smaller bonuses feed the base game), `big` (the big
///     wins feed it), `more` (more bonuses, each smaller);
///   · reach: `bonus` (all-or-nothing bonuses: the typical bonus pays),
///     `base`, `rarer` (rarer bonuses);
///   · crumbs: `top` (the purchases that pop pay), `middle` (the middle pays:
///     fewer refunds, the pops kept);
///   · shape: the biggest bonuses are the lever (rarer to raise the ratio,
///     more often to lower it) and the other side is `typical` (the typical
///     bonus), `base` (the base game) or `frequency` (how often a bonus comes).
pub fn payers(key: &str) -> &'static [&'static str] {
    match key {
        "broke" => &["bonus", "big", "more"],
        "reach" => &["bonus", "base", "rarer"],
        "crumbs" => &["top", "middle"],
        "shape" => &["typical", "base", "frequency"],
        _ => &["base", "big", "bonus"],
    }
}

/// One way to pay for a fix.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixOption {
    /// One of `payers(frustration)`, `blend` (every payer pays a share) or
    /// `global` (one fix for every frustration).
    pub payer: &'static str,
    pub feasible: bool,
    /// Brings its frustration(s) to the aim (a blend can fall short).
    pub reaches: bool,
    /// How a blend was made: « the average of 3 ways, pushed ×1.40 ».
    pub note: Option<String>,
    /// Why it cannot be played, when it cannot.
    pub reason: Option<String>,
    pub moves: Vec<Move>,
    pub after: Option<FeelMetrics>,
    /// Harm done to the rest of the game — the other frustrations, the big
    /// wins, the max win — less half the relief it brings to the other
    /// frustrations already past their line. The lowest is recommended.
    pub side_cost: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fix {
    /// What the fix moves: `bonusOneIn` (drought), `weakShare` (letdown),
    /// `hitRate` (dry), `broke` (the share itself: each payer pulls its own
    /// lever), `bigOneIn` (reach), `crumbs`.
    pub lever: &'static str,
    pub from: f64,
    pub to: f64,
    pub options: Vec<FixOption>,
    pub recommended: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frustration {
    /// One of `FRUSTRATIONS`.
    pub key: &'static str,
    /// `serious` | `minor` | `ok`.
    pub severity: &'static str,
    /// Read but not flagged: an all-or-nothing mode's crumbs are its design.
    pub by_design: bool,
    /// A two-sided reading (shape): the healthy band; `minor`/`serious`/
    /// `target` then describe the side the value is on.
    pub band: Option<[f64; 2]>,
    /// Exact, on the published weights.
    pub value: f64,
    /// Measured on the cohort (`None`: sessions longer than the run).
    pub observed: Option<f64>,
    /// Sessions (or players) behind `observed`.
    pub sample: u64,
    pub minor: f64,
    pub serious: f64,
    pub target: f64,
    /// Present when the frustration deserves one.
    pub fix: Option<Fix>,
}

/// What the cohort lived, counted by `crowdsim` on the same sessions.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observed {
    /// Complete `drought_spins` sessions, and those with ≤ m bonuses.
    pub drought_sessions: u64,
    pub droughts: u64,
    /// Complete `session_spins` sessions, those with ≥ k bonuses, and those
    /// among them where every bonus paid under `weak_x`.
    pub sessions: u64,
    pub multi_bonus_sessions: u64,
    pub letdowns: u64,
    /// Players who lived at least one drought / letdown session.
    pub players_drought: f64,
    pub players_letdown: f64,
    /// Median player's longest run without a win.
    pub dry_median: Option<f64>,
    pub players: u64,
    /// Players who lost `deposit` stakes before their first bonus, and those
    /// who neither saw a bonus nor went broke (a run too short to tell).
    pub broke: u64,
    pub undecided: u64,
    /// Complete `reach_spins` sessions, and those that never won `big_x`.
    pub reach_sessions: u64,
    pub reach_misses: u64,
    /// Rounds played, and those returning under `crumbs_under` × the price.
    pub rounds: u64,
    pub crumbs_rounds: u64,
    /// Median ÷ average of the bonuses the cohort opened, and how many.
    pub bonus_ratio: Option<f64>,
    pub bonuses: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeelReport {
    pub kind: Kind,
    pub params: FeelParams,
    /// Length of a player's run (the dry spells are measured over it).
    pub spins: u32,
    /// The thresholds in multiples of the BASE bet (× the stake).
    pub weak_x: f64,
    pub big_x: f64,
    pub has_bonus: bool,
    /// Keys of `metrics.budget`, in order.
    pub groups: Vec<&'static str>,
    pub metrics: FeelMetrics,
    pub observed: Observed,
    pub frustrations: Vec<Frustration>,
    /// One fix for everything (see `global_weights`).
    pub global: Option<GlobalFix>,
    /// Where the mode stands among the market's games of a close max win.
    pub market: Option<market::MarketView>,
}

fn severity(v: f64, r: &Reference) -> &'static str {
    if v >= r.serious {
        "serious"
    } else if v >= r.minor {
        "minor"
    } else {
        "ok"
    }
}

/// Pain of each frustration (`FRUSTRATIONS` order), 1 = its minor threshold.
fn pains(m: &FeelMetrics) -> [f64; 7] {
    [
        m.drought.unwrap_or(0.0) / DROUGHT.minor,
        m.letdown.unwrap_or(0.0) / LETDOWN.minor,
        m.dry_median / DRY.minor,
        m.broke.unwrap_or(0.0) / BROKE.minor,
        m.never_big / REACH.minor,
        m.crumbs.unwrap_or(0.0) / CRUMBS.minor,
        // 1 at either edge of the band, under 1 inside it.
        m.bonus_ratio.map_or(0.0, |r| (SHAPE_BAND.0 / r.max(1e-9)).max(r / SHAPE_BAND.1)),
    ]
}

fn side_cost(kind: Kind, fixing: &str, before: &FeelMetrics, after: &FeelMetrics) -> f64 {
    let (pb, pa) = (pains(before), pains(after));
    let felt = |k: &str| (kind == Kind::Buy) == (k == "crumbs");
    let others = || FRUSTRATIONS.iter().enumerate().filter(|(_, &k)| k != fixing && felt(k)).map(|(i, _)| i);
    let mut cost: f64 = others().map(|i| (pa[i] - pb[i]).max(0.0)).sum();
    // Relief on another frustration already past its line counts for half:
    // between two fixes of equal harm, the one that also helps wins.
    cost -= 0.5 * others().filter(|&i| pb[i] >= 1.0).map(|i| (pb[i] - pa[i]).max(0.0)).sum::<f64>();
    match kind {
        // The hit rate is felt on every spin: losing a fifth of it counts
        // one; so do 20 more points of wins that pay less than the stake.
        Kind::Base => {
            if before.hit_rate > 0.0 {
                cost += ((before.hit_rate - after.hit_rate) / before.hit_rate / 0.2).max(0.0);
            }
            cost += ((after.wins_under_stake - before.wins_under_stake) / 0.2).max(0.0);
        }
        // A purchase that pays its price back is what a buyer hopes for:
        // losing a fifth of them counts one.
        Kind::Buy => {
            if let (Some(b), Some(a)) = (before.refund, after.refund) {
                if b > 0.0 {
                    cost += ((b - a) / b / 0.2).max(0.0);
                }
            }
        }
    }
    // Each halving of a frequency counts one; the max win is the least felt
    // (a rare event weighs less lived than described).
    let rarer = |b: Option<f64>, a: Option<f64>| match (b, a) {
        (Some(b), Some(a)) if b > 0.0 => (a / b).log2().max(0.0),
        (Some(_), None) => 4.0,
        _ => 0.0,
    };
    cost += 0.5 * rarer(before.big_one_in, after.big_one_in) + 0.25 * rarer(before.max_one_in, after.max_one_in);
    cost
}

/// Smallest x in [lo, hi] with `ok(x)`, `ok` monotone (false → true).
fn bisect(mut lo: f64, mut hi: f64, iters: u32, ok: impl Fn(f64) -> bool) -> Option<f64> {
    if !ok(hi) {
        return None;
    }
    for _ in 0..iters {
        let mid = 0.5 * (lo + hi);
        if ok(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(hi)
}

impl Shape {
    /// The value the lever must reach for the frustration to fall to its
    /// target: (lever, from, to). `None`: nothing to fix, or out of reach.
    fn lever(&self, key: &str, m: &FeelMetrics, spins: u32) -> Option<(&'static str, f64, f64)> {
        let p = &self.params;
        match key {
            "drought" => {
                let pb = 1.0 / m.bonus_one_in?;
                let to = bisect(pb, 0.9, 80, |x| drought_share(p.drought_spins, x, p.drought_bonuses) <= DROUGHT.target)?;
                Some(("bonusOneIn", 1.0 / pb, 1.0 / to))
            }
            "letdown" => {
                let q = m.weak_share?;
                let to = self.weak_share_for(1.0 / m.bonus_one_in?, q);
                (to < q).then_some(("weakShare", q, to))
            }
            "dry" => {
                let h = m.hit_rate;
                let to = bisect(h, 0.95, 40, |x| dry_quantile(spins, x, 0.5) <= DRY.target)?;
                Some(("hitRate", h, to))
            }
            // Each payer pulls its own lever, down to the target share.
            "broke" => m.broke.filter(|&b| b > BROKE.target).map(|b| ("broke", b, BROKE.target)),
            // (1 − p)^N = target: the frequency a big win needs.
            "reach" => {
                let now = 1.0 / m.big_one_in?;
                let to = 1.0 - REACH.target.powf(1.0 / p.reach_spins as f64);
                (to > now).then(|| ("bigOneIn", 1.0 / now, 1.0 / to))
            }
            "crumbs" => m.crumbs.filter(|&q| q > CRUMBS.target).map(|q| ("crumbs", q, CRUMBS.target)),
            // Back to the nearest edge of the market band.
            "shape" => {
                let r = m.bonus_ratio?;
                let (lo, hi) = SHAPE_BAND;
                if r < lo {
                    Some(("bonusRatio", r, lo))
                } else if r > hi {
                    Some(("bonusRatio", r, hi))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Median ÷ average bonus under the weights `w` — cheap (no walks).
    fn ratio_of(&self, w: &[f64]) -> Option<f64> {
        let (mut m, mut v) = (0.0, 0.0);
        for &i in &self.bonus_order {
            m += w[i as usize];
            v += w[i as usize] * self.x[i as usize];
        }
        if m <= 0.0 || v <= 0.0 {
            return None;
        }
        let mut cum = 0.0;
        for &i in &self.bonus_order {
            cum += w[i as usize];
            if cum >= 0.5 * m - 1e-9 * m && w[i as usize] > 0.0 {
                return Some(self.x[i as usize] / (v / m));
            }
        }
        None
    }

    /// Moves the biggest bonuses' factor from 1 — down to raise the ratio,
    /// up (to ×20) to lower it — until the median ÷ average reaches `target`:
    /// the smallest move that gets there. The ratio is not monotone all the
    /// way (push the top far enough and the rest of the bonus vanishes), so the
    /// first crossing is found by stepping out, then pinned by halving. The
    /// median moves by steps: the ratio lands on the first step past the target.
    fn solve_ratio(&self, target: f64, raise: bool, weights_at: impl Fn(f64) -> Result<Vec<f64>, String>) -> Result<Vec<f64>, String> {
        let ratio = |nw: &[f64]| self.ratio_of(nw).unwrap_or(if raise { 0.0 } else { f64::INFINITY });
        let reached = |r: f64| if raise { r >= target - 1e-9 } else { r <= target + 1e-9 };
        let step = |b: f64| if raise { b * 0.9 } else { b * 1.1 };
        let (mut prev, mut beta) = (1.0, step(1.0));
        let mut found = None;
        for _ in 0..200 {
            if !raise && beta > 20.0 {
                break;
            }
            let Ok(nw) = weights_at(beta) else { break };
            if reached(ratio(&nw)) {
                found = Some(nw);
                break;
            }
            prev = beta;
            beta = step(beta);
            if raise && beta < 1e-6 {
                break;
            }
        }
        let Some(mut best) = found else {
            return Err(String::new());
        };
        let (mut a, mut b) = (prev, beta);
        for _ in 0..40 {
            let mid = 0.5 * (a + b);
            let nw = weights_at(mid)?;
            if reached(ratio(&nw)) {
                b = mid;
                best = nw;
            } else {
                a = mid;
            }
        }
        Ok(best)
    }

    /// Players broke before the bonus under the weights `w`.
    fn broke_of(&self, w: &[f64]) -> f64 {
        let s = self.sums(w);
        self.broke_share(w, s.bonus_mass() / s.total())
    }

    /// Pushes a payer's lever from 1 up to `hi` (or as far as its weights stay
    /// playable) until the broke share falls to the target: the smallest push
    /// that gets there.
    fn solve_broke(&self, hi: f64, weights_at: impl Fn(f64) -> Result<Vec<f64>, String>) -> Result<Vec<f64>, String> {
        let mut top = hi;
        if weights_at(top).is_err() {
            let (mut lo, mut h) = (1.0, hi);
            for _ in 0..20 {
                let mid = 0.5 * (lo + h);
                if weights_at(mid).is_ok() {
                    lo = mid;
                } else {
                    h = mid;
                }
            }
            top = lo;
        }
        let at_top = weights_at(top)?;
        let floor = self.broke_of(&at_top);
        if floor > BROKE.target {
            return Err(format!("pushed as far as it goes, it still leaves {:.0} % broke before the bonus", floor * 100.0));
        }
        let (mut lo, mut h, mut best) = (1.0, top, at_top);
        for _ in 0..16 {
            let mid = 0.5 * (lo + h);
            let nw = weights_at(mid)?;
            if self.broke_of(&nw) <= BROKE.target {
                h = mid;
                best = nw;
            } else {
                lo = mid;
            }
        }
        Ok(best)
    }

    /// More bonuses (× `f`), each one smaller: the bonus keeps what it
    /// carries, the spins without bonus make room.
    fn more_smaller_bonuses(&self, w: &[f64], f: f64) -> Result<Vec<f64>, String> {
        let s = self.sums(w);
        let (total, value) = (s.total(), s.total_value());
        let (mb, vb) = s.free_bonus();
        let max_g = G::Max as usize;
        let (m_max, v_max) = (s.mass[max_g], s.value[max_g]);
        let nb_mass = total - mb - m_max;
        let sigma = (total - m_max - f * mb) / nb_mass;
        if sigma.is_nan() || sigma < 0.0 || nb_mass <= 0.0 {
            return Err("not enough spins without bonus to make room for it".into());
        }
        let nb_value = value - vb - v_max;
        let rows = self.rows_where(is_bonus_group);
        let mut out = self.tilt(w, &rows, f * mb, (value - v_max - sigma * nb_value) / (f * mb))?;
        for (o, &g) in out.iter_mut().zip(&self.group) {
            let g = g as usize;
            if !is_bonus_group(g) && g != max_g {
                *o *= sigma;
            }
        }
        Ok(out)
    }

    /// Largest weak share (≤ `cap`) whose letdown chance meets the target at
    /// bonus probability `pb` — the chance grows with the weak share.
    fn weak_share_for(&self, pb: f64, cap: f64) -> f64 {
        let p = &self.params;
        let ok = |x: f64| letdown(p.session_spins, pb, x, p.letdown_bonuses).0 <= LETDOWN.target;
        if ok(cap) {
            return cap;
        }
        let (mut lo, mut hi) = (0.0, cap);
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if ok(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// The weights of one fix, paid by `payer`. Keeps the total weight and the
    /// RTP; errors in plain words when this payer cannot pay.
    pub fn fix_weights(&self, w: &[f64], spins: u32, key: &str, payer: &str) -> Result<Vec<f64>, String> {
        if key == "global" || payer == "blend" {
            return self.blend_weights(w, spins, key);
        }
        let m = self.measure(w, spins);
        let (_, _, to) = self.lever(key, &m, spins).ok_or("nothing to fix, or out of reach of these books")?;
        let s = self.sums(w);
        let (total, value, mb) = (s.total(), s.total_value(), s.bonus_mass());
        let (dead, base, weak, mid) = (G::Dead as usize, G::Base as usize, G::Weak as usize, G::Mid as usize);
        let no_big = format!("no win reaches {}x here", self.big_x);
        let fail = |msg: &str| Err::<Vec<f64>, String>(msg.to_string());
        match (key, payer) {
            // ── More bonuses: 1 in `from` → 1 in `to` spins.
            ("drought", _) => {
                // The bonus mass it needs, found among the bonuses a fix may move.
                let f = (total / to - s.bmass[G::Max as usize]) / s.free_bonus().0;
                match payer {
                    "base" => self.by_groups(
                        w,
                        &[unknown(0), unknown(1), KEEP, fixed(f), fixed(f), fixed(f)],
                        &[(Sum::Mass, total), (Sum::Value, value)],
                        "no base win to take from",
                    ),
                    "big" => self.by_groups(
                        w,
                        &[unknown(0), unknown(0), unknown(1), unknown(2), unknown(2), unknown(1)],
                        &[(Sum::Bonus, total / to), (Sum::Mass, total), (Sum::Value, value)],
                        &no_big,
                    ),
                    "bonus" => self.more_smaller_bonuses(w, f),
                    _ => fail("unknown payer"),
                }
            }
            // ── Fewer weak bonuses: weak share `from` → `to`, moved to the
            // bonuses between `weak_x` and `big_x`.
            ("letdown", _) => {
                if s.mass[mid] <= 0.0 {
                    return fail("no bonus pays between the letdown line and the big wins: nowhere to move the weak ones");
                }
                let alpha = to * mb / s.mass[weak];
                let gamma = 1.0 + (1.0 - alpha) * s.mass[weak] / s.mass[mid];
                match payer {
                    "base" => self.by_groups(
                        w,
                        &[unknown(0), unknown(1), KEEP, fixed(alpha), fixed(gamma), KEEP],
                        &[(Sum::Mass, total), (Sum::Value, value)],
                        "no base win to take from",
                    ),
                    "big" => self.by_groups(
                        w,
                        &[unknown(0), unknown(0), unknown(1), fixed(alpha), unknown(2), unknown(1)],
                        &[(Sum::Bonus, mb), (Sum::Mass, total), (Sum::Value, value)],
                        &no_big,
                    ),
                    // A rarer bonus brings fewer multi-bonus sessions, where a
                    // letdown is likelier: the weak share is solved again at
                    // the new frequency until both agree.
                    "bonus" => {
                        let q = s.mass[weak] / mb;
                        let mut to = to;
                        let mut out = Vec::new();
                        for _ in 0..30 {
                            let alpha = to * mb / s.mass[weak];
                            let gamma = 1.0 + (1.0 - alpha) * s.mass[weak] / s.mass[mid];
                            out = self.by_groups(
                                w,
                                &[unknown(0), unknown(0), unknown(0), scaled(1, alpha), scaled(1, gamma), unknown(1)],
                                &[(Sum::Mass, total), (Sum::Value, value)],
                                "no bonus to make rarer",
                            )?;
                            let pb = self.sums(&out).bonus_mass() / total;
                            let next = self.weak_share_for(pb, q);
                            if (next - to).abs() <= 1e-9 {
                                break;
                            }
                            to = next;
                        }
                        let m = self.measure(&out, spins);
                        if m.letdown.unwrap_or(1.0) > LETDOWN.target * 1.001 {
                            return fail("a rarer bonus cannot get there: each step back in frequency makes a letdown likelier");
                        }
                        Ok(out)
                    }
                    _ => fail("unknown payer"),
                }
            }
            // ── Fewer dry spells: hit rate `from` → `to`.
            ("dry", _) => {
                let hit = to * total;
                match payer {
                    // More base wins, smaller: the base game keeps its RTP.
                    "base" => {
                        if s.mass[base] <= 0.0 {
                            return fail("no base win to spread");
                        }
                        let other_hit = s.hit_mass() - s.hit[base];
                        let k = (hit - other_hit) / s.mass[base];
                        if s.mass[dead] - (k - 1.0) * s.mass[base] < 0.0 {
                            return fail("not enough losing spins to turn into wins");
                        }
                        let rows = self.rows_where(|g| g == base);
                        let mut out = self.tilt(w, &rows, k * s.mass[base], s.value[base] / (k * s.mass[base]))?;
                        let dead_f = (s.mass[dead] - (k - 1.0) * s.mass[base]) / s.mass[dead];
                        for (o, &g) in out.iter_mut().zip(&self.group) {
                            if g as usize == dead {
                                *o *= dead_f;
                            }
                        }
                        Ok(out)
                    }
                    "big" => self.by_groups(
                        w,
                        &[unknown(0), unknown(1), unknown(2), KEEP, KEEP, unknown(2)],
                        &[(Sum::Hit, hit), (Sum::Mass, total), (Sum::Value, value)],
                        &no_big,
                    ),
                    "bonus" => {
                        if !self.has_bonus || mb <= 0.0 {
                            return fail("no bonus to make rarer");
                        }
                        self.by_groups(
                            w,
                            &[unknown(0), unknown(1), KEEP, unknown(2), unknown(2), unknown(2)],
                            &[(Sum::Hit, hit), (Sum::Mass, total), (Sum::Value, value)],
                            "no bonus to make rarer",
                        )
                    }
                    _ => fail("unknown payer"),
                }
            }
            // ── Fewer players broke before the bonus: each payer pushes its
            // own lever until the share falls to the target.
            ("broke", _) => {
                if mb <= 0.0 {
                    return fail("no bonus in this mode");
                }
                let (m_dead, m_base) = (s.mass[dead], s.mass[base]);
                match payer {
                    // Base wins × k (the losing spins make room), paid by
                    // every bonus paying a little less.
                    "bonus" => {
                        if m_base <= 0.0 || m_dead <= 0.0 {
                            return fail("no base win to strengthen");
                        }
                        let bonus_rows = self.rows_where(is_bonus_group);
                        let (m_b, v_b) = s.free_bonus();
                        self.solve_broke(1.0 + m_dead / m_base, |k| {
                            let mut out = self.tilt(w, &bonus_rows, m_b, (v_b - (k - 1.0) * s.value[base]) / m_b)?;
                            let dead_f = (m_dead - (k - 1.0) * m_base) / m_dead;
                            if dead_f < 0.0 {
                                return Err("not enough losing spins".into());
                            }
                            for (o, &g) in out.iter_mut().zip(&self.group) {
                                match g as usize {
                                    x if x == base => *o *= k,
                                    x if x == dead => *o *= dead_f,
                                    _ => {}
                                }
                            }
                            Ok(out)
                        })
                    }
                    // Base wins × k, paid by the big wins.
                    "big" => {
                        if m_base <= 0.0 {
                            return fail("no base win to strengthen");
                        }
                        self.solve_broke(1.0 + m_dead / m_base, |k| {
                            self.by_groups(w, &[unknown(0), fixed(k), unknown(1), KEEP, KEEP, unknown(1)], &[(Sum::Mass, total), (Sum::Value, value)], &no_big)
                        })
                    }
                    // More bonuses (× f, up to 1 in 20 spins), each smaller.
                    "more" => self.solve_broke((0.05 * total / s.free_bonus().0).max(1.0), |f| self.more_smaller_bonuses(w, f)),
                    _ => fail("unknown payer"),
                }
            }
            // ── Big wins within reach: 1 in `from` → 1 in `to` spins.
            ("reach", _) => {
                let (bb, bigb) = (s.mass[G::BaseBig as usize], s.mass[G::BonusBig as usize]);
                // The max win counts among the big wins, but keeps its odds.
                let want = total / to - s.mass[G::Max as usize];
                match payer {
                    // All-or-nothing bonuses: more of them reach the big line,
                    // the typical bonus pays for it (and some fall weak).
                    "bonus" => {
                        if bigb <= 0.0 {
                            return fail(&format!("no bonus reaches {}x here", self.big_x));
                        }
                        let beta = (want - bb) / bigb;
                        self.by_groups(w, &[KEEP, KEEP, KEEP, unknown(0), unknown(1), fixed(beta)], &[(Sum::Bonus, mb), (Sum::Value, value)], "the bonus has no middle to take from")
                    }
                    "base" => {
                        if bb + bigb <= 0.0 {
                            return fail(&no_big);
                        }
                        let beta = want / (bb + bigb);
                        self.by_groups(w, &[unknown(0), unknown(1), fixed(beta), KEEP, KEEP, fixed(beta)], &[(Sum::Mass, total), (Sum::Value, value)], "no base win to take from")
                    }
                    "rarer" => {
                        if bb + bigb <= 0.0 {
                            return fail(&no_big);
                        }
                        let beta = want / (bb + bigb);
                        self.by_groups(w, &[unknown(0), unknown(0), fixed(beta), unknown(1), unknown(1), fixed(beta)], &[(Sum::Mass, total), (Sum::Value, value)], "no bonus to make rarer")
                    }
                    _ => fail("unknown payer"),
                }
            }
            // ── Bonus shape: the biggest bonuses are the lever; the other side
            // of the move is the typical bonus, the base game or the frequency.
            ("shape", _) => {
                let bigb = s.mass[G::BonusBig as usize];
                if mb <= 0.0 {
                    return fail("no bonus in this mode");
                }
                if bigb <= 0.0 {
                    return fail(&format!("no bonus reaches {}x: nothing to move", self.big_x));
                }
                let weights_at = |beta: f64| -> Result<Vec<f64>, String> {
                    match payer {
                        // The bonus keeps its RTP and its frequency: its top
                        // and its bottom trade places with the middle.
                        "typical" => self.by_groups(w, &[KEEP, KEEP, KEEP, unknown(0), unknown(1), fixed(beta)], &[(Sum::Bonus, mb), (Sum::Value, value)], "the bonus has no middle to trade with"),
                        // The bonus keeps its frequency; the RTP moves to (or
                        // from) the base wins.
                        "base" => self.by_groups(
                            w,
                            &[unknown(0), unknown(1), KEEP, unknown(2), unknown(2), fixed(beta)],
                            &[(Sum::Bonus, mb), (Sum::Mass, total), (Sum::Value, value)],
                            "no base win to trade with",
                        ),
                        // The RTP moves to (or from) the frequency of the bonus.
                        "frequency" => self.by_groups(
                            w,
                            &[unknown(0), unknown(0), unknown(0), unknown(1), unknown(1), fixed(beta)],
                            &[(Sum::Mass, total), (Sum::Value, value)],
                            "no spin without bonus to trade with",
                        ),
                        _ => Err("unknown payer".into()),
                    }
                };
                let raise = to > self.ratio_of(w).unwrap_or(0.0);
                self.solve_ratio(to, raise, weights_at).map_err(|e| {
                    if !e.is_empty() {
                        return e;
                    }
                    // Say how far it is: what the median (or the average) would need.
                    let (median, mean) = (m.bonus_median.unwrap_or(0.0), m.bonus_mean.unwrap_or(0.0));
                    match (raise, payer) {
                        (true, "typical") => format!(
                            "with the average held at {mean:.0}x, the typical bonus would have to reach {:.0}x (it is {median:.1}x): out of reach by moving the biggest bonuses alone — lift the weak bonuses first",
                            to * mean
                        ),
                        (true, _) => format!(
                            "with the typical bonus at {median:.1}x, the average would have to fall to {:.0}x (it is {mean:.0}x): the biggest bonuses do not carry that much",
                            median / to
                        ),
                        (false, _) => format!("even with 20 times more big bonuses, the typical bonus stays above {to:.2} × the average"),
                    }
                })
            }
            // ── Fewer crumbs: purchases under the crumbs line `from` → `to`,
            // lifted into the middle.
            ("crumbs", _) => {
                let (m_w, m_m) = (s.mass[weak], s.mass[mid]);
                if m_m <= 0.0 {
                    return fail("no purchase lands between the crumbs and the pops: nowhere to lift the crumbs");
                }
                let alpha = to * total / m_w;
                match payer {
                    "top" => self.by_groups(w, &[KEEP, KEEP, KEEP, fixed(alpha), unknown(0), unknown(1)], &[(Sum::Mass, total), (Sum::Value, value)], "no purchase pops"),
                    // The pops keep their mass; the middle takes the lifted
                    // crumbs and pays a little less to keep the price.
                    "middle" => {
                        let mid_mass = m_m + (1.0 - alpha) * m_w;
                        let mid_value = value - alpha * s.value[weak] - s.value[G::BonusBig as usize] - s.value[G::Max as usize];
                        let rows = self.rows_where(|g| g == mid);
                        let mut out = self.tilt(w, &rows, mid_mass, mid_value / mid_mass)?;
                        for (o, &g) in out.iter_mut().zip(&self.group) {
                            if g as usize == weak {
                                *o *= alpha;
                            }
                        }
                        Ok(out)
                    }
                    _ => fail("unknown payer"),
                }
            }
            _ => fail("unknown frustration"),
        }
    }

    /// A frustration's fix, and the direction a global fix takes for it (the
    /// average move of its playable ways).
    fn fix(&self, w: &[f64], spins: u32, key: &'static str, before: &FeelMetrics) -> Option<(Fix, Option<Vec<f64>>)> {
        let (lever, from, to) = self.lever(key, before, spins)?;
        // A mode without bonus books (or not imported) is not offered bonus payers.
        let bonused = self.has_bonus && self.sums(w).bonus_mass() > 0.0;
        let needs_bonus = |p: &str| matches!(p, "bonus" | "rarer" | "more" | "typical" | "frequency");
        let mut dirs: Vec<Vec<f64>> = Vec::new();
        let mut options: Vec<FixOption> = payers(key)
            .iter()
            .filter(|&&payer| self.kind == Kind::Buy || bonused || !needs_bonus(payer))
            .map(|&payer| match self.fix_weights(w, spins, key, payer) {
                Ok(nw) => {
                    let after = self.measure(&nw, spins);
                    dirs.push(nw.iter().zip(w).map(|(a, b)| a - b).collect());
                    FixOption { payer, feasible: true, reaches: true, note: None, reason: None, moves: self.moves(w, &nw), side_cost: Some(side_cost(self.kind, key, before, &after)), after: Some(after) }
                }
                Err(reason) => FixOption { payer, feasible: false, reaches: false, note: None, reason: Some(reason), moves: Vec::new(), after: None, side_cost: None },
            })
            .collect();
        // Every payer pays a share: the average of the playable ways, pushed
        // until the aim is reached.
        let dir = mean_dir(&dirs);
        if let (true, Some(d)) = (dirs.len() >= 2, &dir) {
            options.push(self.blend_option(w, spins, &[key], d, before, "blend", dirs.len()));
        }
        let recommended = options
            .iter()
            .enumerate()
            .filter(|(_, o)| o.reaches)
            .filter_map(|(i, o)| o.side_cost.map(|c| (i, c)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i);
        Some((Fix { lever, from, to, options, recommended }, dir))
    }

    /// Whether the frustration `key` has reached its aim under `m` (`before`:
    /// where it started — the shape aims at the band edge it left).
    fn aim_reached(&self, key: &str, m: &FeelMetrics, before: &FeelMetrics) -> bool {
        let under = |v: Option<f64>, t: f64| v.is_some_and(|v| v <= t + 1e-9);
        match key {
            "drought" => under(m.drought, DROUGHT.target),
            "letdown" => under(m.letdown, LETDOWN.target),
            "dry" => m.dry_median <= DRY.target,
            "broke" => under(m.broke, BROKE.target),
            "reach" => m.never_big <= REACH.target + 1e-9,
            "crumbs" => under(m.crumbs, CRUMBS.target),
            "shape" => match (before.bonus_ratio, m.bonus_ratio) {
                (Some(b), Some(a)) if b < SHAPE_BAND.0 => a >= SHAPE_BAND.0 - 1e-9,
                (Some(b), Some(a)) if b > SHAPE_BAND.1 => a <= SHAPE_BAND.1 + 1e-9,
                (Some(_), Some(_)) => true,
                _ => false,
            },
            _ => false,
        }
    }

    /// Pushes `w` along `d` until every frustration of `keys` reaches its
    /// aim: (weights, push, reached all). Steps out by quarters up to ×4 (or
    /// until a weight would go negative), then pins the first push that gets
    /// there by halving. When none does: the smallest push (from the plain
    /// average, ×1) that reaches the most aims — pushing further for nothing
    /// only piles up the cost. A `guard` stops the push where it would fail
    /// (the market's middle half): the push never crosses it, and may then
    /// stay under ×1.
    fn push_until(&self, w: &[f64], d: &[f64], spins: u32, keys: &[&str], before: &FeelMetrics, guard: Option<&dyn Fn(&FeelMetrics) -> bool>) -> Option<(Vec<f64>, f64, bool)> {
        let with_broke = keys.contains(&"broke");
        let at = |t: f64| -> Option<(Vec<f64>, FeelMetrics)> {
            let nw = along(w, d, t)?;
            let m = self.measure_opts(&nw, spins, with_broke);
            Some((nw, m))
        };
        let count = |m: &FeelMetrics| keys.iter().filter(|k| self.aim_reached(k, m, before)).count();
        let all = |m: &FeelMetrics| count(m) == keys.len();
        let allowed = |m: &FeelMetrics| guard.is_none_or(|g| g(m));
        let min_t = if guard.is_some() { 0.0 } else { 1.0 };
        let (mut prev, mut t) = (0.0, 0.25);
        let mut best: Option<(Vec<f64>, f64, usize)> = guard.map(|_| (w.to_vec(), 0.0, count(before)));
        while t <= 4.0 + 1e-9 {
            let Some((nw, m)) = at(t) else { break };
            if !allowed(&m) {
                // The farthest push the guard allows, between the last two steps.
                let (mut a, mut b) = (prev, t);
                for _ in 0..12 {
                    let mid = 0.5 * (a + b);
                    match at(mid) {
                        Some((_, m)) if allowed(&m) => a = mid,
                        _ => b = mid,
                    }
                }
                if let Some((nw, m)) = at(a) {
                    let n = count(&m);
                    if a > prev && best.as_ref().is_none_or(|b| n > b.2) {
                        best = Some((nw, a, n));
                    }
                }
                break;
            }
            if all(&m) {
                let (mut a, mut b, mut hit) = (prev, t, nw);
                for _ in 0..14 {
                    let mid = 0.5 * (a + b);
                    match at(mid) {
                        Some((nw, m)) if all(&m) => {
                            b = mid;
                            hit = nw;
                        }
                        _ => a = mid,
                    }
                }
                return Some((hit, b, true));
            }
            let n = count(&m);
            if t >= min_t - 1e-9 && best.as_ref().is_none_or(|b| n > b.2) {
                best = Some((nw, t, n));
            }
            prev = t;
            t += 0.25;
        }
        best.map(|(nw, t, _)| (nw, t, false))
    }

    /// The option card of a blend: pushed to the aim, or the best it can do.
    #[allow(clippy::too_many_arguments)]
    fn blend_option(&self, w: &[f64], spins: u32, keys: &[&str], d: &[f64], before: &FeelMetrics, payer: &'static str, n: usize) -> FixOption {
        let fixing = if keys.len() == 1 { keys[0] } else { "global" };
        match self.push_until(w, d, spins, keys, before, None) {
            Some((nw, t, reaches)) => {
                let after = self.measure(&nw, spins);
                let what = if keys.len() == 1 { format!("the average of the {n} ways") } else { format!("the average of the {n} fixes") };
                let note = if reaches {
                    format!("{what}, pushed ×{t:.2} to reach the aim")
                } else {
                    format!("{what} (×{t:.2}): as close as it gets, short of the aim")
                };
                FixOption { payer, feasible: true, reaches, note: Some(note), reason: None, moves: self.moves(w, &nw), side_cost: Some(side_cost(self.kind, fixing, before, &after)), after: Some(after) }
            }
            None => FixOption { payer, feasible: false, reaches: false, note: None, reason: Some("the average of the ways is not playable".into()), moves: Vec::new(), after: None, side_cost: None },
        }
    }

    /// The whole reading: exact figures, what the cohort lived, and a fix for
    /// every frustration past its minor threshold.
    pub fn report(&self, w: &[f64], spins: u32, observed: Observed) -> FeelReport {
        let m = self.measure(w, spins);
        let mut frustrations = Vec::new();
        let mut dirs: Vec<(&'static str, Vec<f64>)> = Vec::new();
        let ratio = |a: u64, b: u64| (b > 0).then(|| a as f64 / b as f64);
        let mut push = |key: &'static str, value: f64, obs: Option<f64>, sample: u64, r: &Reference, by_design: bool, dirs: &mut Vec<(&'static str, Vec<f64>)>| {
            let sev = if by_design { "ok" } else { severity(value, r) };
            let fix = if sev == "ok" { None } else { self.fix(w, spins, key, &m) };
            let fix = fix.map(|(f, d)| {
                if let Some(d) = d {
                    dirs.push((key, d));
                }
                f
            });
            frustrations.push(Frustration { key, severity: sev, by_design, band: None, value, observed: obs, sample, minor: r.minor, serious: r.serious, target: r.target, fix });
        };
        match self.kind {
            Kind::Base => {
                if let Some(d) = m.drought {
                    push("drought", d, ratio(observed.droughts, observed.drought_sessions), observed.drought_sessions, &DROUGHT, false, &mut dirs);
                }
                if let Some(l) = m.letdown {
                    push("letdown", l, ratio(observed.letdowns, observed.multi_bonus_sessions), observed.multi_bonus_sessions, &LETDOWN, false, &mut dirs);
                }
                push("dry", m.dry_median, observed.dry_median, observed.players, &DRY, false, &mut dirs);
                if let Some(b) = m.broke {
                    push("broke", b, ratio(observed.broke, observed.players), observed.players, &BROKE, false, &mut dirs);
                }
                push("reach", m.never_big, ratio(observed.reach_misses, observed.reach_sessions), observed.reach_sessions, &REACH, false, &mut dirs);
                // Two-sided: under the band (lopsided) or above it (flat).
                if let Some(r) = m.bonus_ratio {
                    let ((lo, hi), (s_lo, s_hi)) = (SHAPE_BAND, SHAPE_SERIOUS);
                    let sev = if r < s_lo || r > s_hi {
                        "serious"
                    } else if r < lo || r > hi {
                        "minor"
                    } else {
                        "ok"
                    };
                    let (minor, serious) = if r > hi { (hi, s_hi) } else { (lo, s_lo) };
                    let fix = if sev == "ok" { None } else { self.fix(w, spins, "shape", &m) };
                    let fix = fix.map(|(f, d)| {
                        if let Some(d) = d {
                            dirs.push(("shape", d));
                        }
                        f
                    });
                    frustrations.push(Frustration {
                        key: "shape",
                        severity: sev,
                        by_design: false,
                        band: Some([lo, hi]),
                        value: r,
                        observed: observed.bonus_ratio,
                        sample: observed.bonuses,
                        minor,
                        serious,
                        target: minor,
                        fix,
                    });
                }
            }
            Kind::Buy => {
                let crumbs = m.crumbs.unwrap_or(0.0);
                push("crumbs", crumbs, ratio(observed.crumbs_rounds, observed.rounds), observed.rounds, &CRUMBS, m.zero_share >= BUST_MODE, &mut dirs);
            }
        }
        // One fix for everything — at the market's median when the market has
        // peers for this max win; else the average of the frustrations' fixes.
        let flagged: Vec<&'static str> = frustrations.iter().filter(|f| f.fix.is_some()).map(|f| f.key).collect();
        let off_market = self.peers.as_ref().is_some_and(|p| {
            market::KEYS.iter().any(|&k| {
                let (a, _, b) = p.quartiles(k);
                m.figure(k).is_some_and(|v| v < a || v > b)
            })
        });
        let wanted = if self.peers.is_some() { !flagged.is_empty() || off_market } else { dirs.len() >= 2 };
        let global = wanted.then(|| self.global_fix(w, spins, &flagged, &m)).flatten();
        let market = self.peers.as_ref().map(|p| p.view(&m, global.as_ref().and_then(|g| g.option.after.as_ref()).map(|a| a as &dyn Figures)));
        FeelReport {
            kind: self.kind,
            params: self.params,
            spins,
            weak_x: self.weak_x,
            big_x: self.big_x,
            has_bonus: self.has_bonus && m.bonus_one_in.is_some(),
            groups: GROUP_KEYS.to_vec(),
            metrics: m,
            observed,
            frustrations,
            global,
            market,
        }
    }

    /// Past its minor line (outside the band, for the shape).
    fn past_line(&self, key: &str, m: &FeelMetrics) -> bool {
        match key {
            "drought" => m.drought.is_some_and(|v| v >= DROUGHT.minor),
            "letdown" => m.letdown.is_some_and(|v| v >= LETDOWN.minor),
            "dry" => m.dry_median >= DRY.minor,
            "broke" => m.broke.is_some_and(|v| v >= BROKE.minor),
            "reach" => m.never_big >= REACH.minor,
            "crumbs" => m.crumbs.is_some_and(|v| v >= CRUMBS.minor) && m.zero_share < BUST_MODE,
            "shape" => m.bonus_ratio.is_some_and(|r| r < SHAPE_BAND.0 || r > SHAPE_BAND.1),
            _ => false,
        }
    }

    /// The average move of a frustration's playable ways, from `w`.
    fn way_dir(&self, w: &[f64], spins: u32, key: &str) -> Option<Vec<f64>> {
        let dirs: Vec<Vec<f64>> = payers(key)
            .iter()
            .filter_map(|p| self.fix_weights(w, spins, key, p).ok())
            .map(|nw| nw.iter().zip(w).map(|(a, b)| a - b).collect())
            .collect();
        mean_dir(&dirs)
    }

    /// One fix for everything. With market peers: the base game brought to
    /// the peers' median (`market_fit`), then — for any frustration still
    /// short of its aim, or pushed past its line by the move — the smallest
    /// push of their fixes. Without peers: the average of the frustrations'
    /// fixes, pushed to their aims. (weights, frustrations taken on, how)
    fn global_weights(&self, w: &[f64], spins: u32, flagged: &[&'static str], before: &FeelMetrics) -> Result<(Vec<f64>, Vec<&'static str>, String), String> {
        let n = self.peers.as_ref().map_or(0, |p| p.games.len());
        let fitted = self.peers.as_ref().map(|p| self.market_fit(w, p));
        let (w1, m1, how) = match fitted {
            Some(Ok(w1)) => {
                let m1 = self.measure(&w1, spins);
                (w1, m1, format!("the market's median of {n} peers"))
            }
            _ => (w.to_vec(), before.clone(), String::new()),
        };
        let from_market = !how.is_empty();
        const BASE: [&str; 6] = ["drought", "letdown", "dry", "broke", "reach", "shape"];
        let keys: Vec<&'static str> = BASE
            .iter()
            .copied()
            .filter(|k| if flagged.contains(k) { !self.aim_reached(k, &m1, before) } else { from_market && self.past_line(k, &m1) })
            .collect();
        let mut parts: Vec<&'static str> = flagged.to_vec();
        parts.extend(keys.iter().filter(|k| !flagged.contains(k)));
        if keys.is_empty() {
            return if from_market { Ok((w1, parts, how)) } else { Err("nothing to fix".into()) };
        }
        if !from_market {
            let dirs: Vec<Vec<f64>> = keys.iter().filter_map(|k| self.way_dir(&w1, spins, k)).collect();
            let d = mean_dir(&dirs).ok_or("no playable fix")?;
            let (nw, t, _) = self.push_until(&w1, &d, spins, &keys, &m1, None).ok_or("the average of the fixes is not playable")?;
            return Ok((nw, parts, format!("the average of the {} fixes, pushed ×{t:.2}", keys.len())));
        }
        // At the market's median, each frustration left takes the way (payer)
        // that gets it furthest while every market figure stays in the peers'
        // middle half — or no further out than the median move left it.
        // (Volatility and wins under the stake follow the shape of each band:
        // shown, not guarded — the median move itself cannot pin them.)
        const GUARDED: [&str; 8] = ["hit", "p100", "p500", "p1000", "p5000", "pmax", "never500", "never1000"];
        let guard = |m: &FeelMetrics| {
            self.peers.as_ref().is_none_or(|p| {
                GUARDED.iter().all(|&k| {
                    let (a, _, b) = p.quartiles(k);
                    let (Some(v), Some(v1)) = (m.figure(k), m1.figure(k)) else { return true };
                    // How far out of the middle half, in middle-half widths.
                    let out = |x: f64| if x < a { (a - x) / (b - a).max(1e-12) } else if x > b { (x - b) / (b - a).max(1e-12) } else { 0.0 };
                    out(v) <= out(v1) + 1e-9
                })
            })
        };
        let allowed = |nw: &[f64]| guard(&self.measure_opts(nw, spins, false));
        // Shape and letdowns first (their ways leave the hit rate alone), the
        // deposits last (every way of theirs feeds the hit rate).
        const ORDER: [&str; 6] = ["shape", "letdown", "drought", "reach", "dry", "broke"];
        let (mut cur, mut steps, mut short) = (w1, Vec::new(), false);
        // Two passes: a later way can undo part of an earlier one.
        let queue: Vec<&'static str> = ORDER.iter().copied().filter(|k| keys.contains(k)).collect();
        for (pass, k) in queue.iter().chain(queue.iter()).copied().enumerate() {
            let m = self.measure_opts(&cur, spins, k == "broke");
            if self.aim_reached(k, &m, before) {
                continue;
            }
            short = short && pass < queue.len();
            let mut best: Option<(Vec<f64>, f64, &str)> = None;
            for &payer in payers(k) {
                let Ok(full) = self.fix_weights(&cur, spins, k, payer) else { continue };
                let d: Vec<f64> = full.iter().zip(&cur).map(|(a, b)| a - b).collect();
                // The furthest the guard lets this way go, up to its aim (×1).
                let t = if allowed(&full) {
                    1.0
                } else {
                    let (mut a, mut b) = (0.0, 1.0);
                    for _ in 0..10 {
                        let mid = 0.5 * (a + b);
                        if along(&cur, &d, mid).is_some_and(|nw| allowed(&nw)) {
                            a = mid;
                        } else {
                            b = mid;
                        }
                    }
                    a
                };
                // Under 1 % of the way, it is no step (and would read « ×0.00 »).
                if t >= 0.01 && best.as_ref().is_none_or(|b| t > b.1) {
                    best = along(&cur, &d, t).map(|nw| (nw, t, payer));
                }
            }
            match best {
                Some((nw, t, payer)) => {
                    short |= t < 1.0;
                    let step = if t < 1.0 { format!("{k} via {payer} (×{t:.2})") } else { format!("{k} via {payer}") };
                    if !steps.contains(&step) {
                        steps.push(step);
                    }
                    cur = nw;
                }
                None => short = true,
            }
        }
        let how = match (steps.is_empty(), short) {
            (true, _) => format!("{how} — any further push would leave the market's middle half"),
            (false, false) => format!("{how}, then {}", steps.join(", ")),
            (false, true) => format!("{how}, then {} — as far as the market's middle half allows", steps.join(", ")),
        };
        let nw = cur;
        Ok((nw, parts, how))
    }

    fn global_fix(&self, w: &[f64], spins: u32, flagged: &[&'static str], before: &FeelMetrics) -> Option<GlobalFix> {
        let market = self.peers.is_some();
        match self.global_weights(w, spins, flagged, before) {
            Ok((nw, parts, note)) => {
                let after = self.measure(&nw, spins);
                let reached: Vec<&'static str> =
                    parts.iter().copied().filter(|k| if flagged.contains(k) { self.aim_reached(k, &after, before) } else { !self.past_line(k, &after) }).collect();
                let option = FixOption {
                    payer: "global",
                    feasible: true,
                    reaches: reached.len() == parts.len(),
                    note: Some(note),
                    reason: None,
                    moves: self.moves(w, &nw),
                    side_cost: Some(side_cost(self.kind, "global", before, &after)),
                    after: Some(after),
                };
                Some(GlobalFix { parts, reached, option, market })
            }
            Err(reason) => (!flagged.is_empty()).then(|| GlobalFix {
                parts: flagged.to_vec(),
                reached: Vec::new(),
                option: FixOption { payer: "global", feasible: false, reaches: false, note: None, reason: Some(reason), moves: Vec::new(), after: None, side_cost: None },
                market,
            }),
        }
    }

    /// The base game at the market's median: each rung of the big-win ladder
    /// (≥ 100, 500, 1 000, 5 000x) and the max win's odds set to the peers'
    /// median, every band keeping its own shape; the wins under 100x then
    /// carry what the RTP needs — at the median hit rate when their shape can
    /// bend that far, else at the hit rate the RTP allows.
    fn market_fit(&self, w: &[f64], peers: &Peers) -> Result<Vec<f64>, String> {
        let c = self.cost;
        let (mut mass, mut val) = ([0.0_f64; 7], [0.0_f64; 7]);
        let mut top = 0.0_f64;
        for ((&wi, &xi), &b) in w.iter().zip(&self.x).zip(&self.mband) {
            mass[b as usize] += wi;
            val[b as usize] += wi * xi;
            if b == 6 && wi > 0.0 {
                top = top.max(xi);
            }
        }
        let (total, value) = (mass.iter().sum::<f64>(), val.iter().sum::<f64>());
        // Tails at 0+, 100, 500, 1 000, 5 000x and the max, never increasing.
        let pmax = peers.median("pmax");
        let mut g = [peers.median("hit"), peers.median("p100"), peers.median("p500"), peers.median("p1000"), peers.median("p5000"), pmax];
        for (i, t) in LADDER.iter().enumerate() {
            if t * c >= top {
                g[i + 1] = pmax;
            }
        }
        for i in (0..5).rev() {
            g[i] = g[i].max(g[i + 1]);
        }
        let mut tgt = [0.0_f64; 7];
        for (b, t) in tgt.iter_mut().enumerate().skip(1) {
            *t = (if b == 6 { g[5] } else { g[b - 1] - g[b] }) * total;
        }
        // A rung with no book: its mass goes down a rung, so the lower tails
        // (and the hit rate) still land on the median.
        for b in (2..=5).rev() {
            if mass[b] <= 0.0 {
                tgt[b - 1] += tgt[b];
                tgt[b] = 0.0;
            }
        }
        let target = |b: usize| tgt[b];
        let mut f = [1.0_f64; 7];
        for b in 2..=6 {
            f[b] = if mass[b] > 0.0 { target(b) / mass[b] } else { 0.0 };
        }
        let m_rest: f64 = (2..=6).map(|b| f[b] * mass[b]).sum();
        let v1 = value - (2..=6).map(|b| f[b] * val[b]).sum::<f64>();
        if v1 <= 0.0 {
            return Err("at the market's ladder, the big wins alone would carry more than the RTP".into());
        }
        if mass[0] <= 0.0 {
            return Err("no losing spin to make room".into());
        }
        let scale_rest = |out: &mut Vec<f64>, dead_f: f64| {
            for (o, &b) in out.iter_mut().zip(&self.mband) {
                match b as usize {
                    0 => *o *= dead_f,
                    1 => {}
                    b => *o *= f[b],
                }
            }
        };
        // The median hit rate, the small wins bent to carry the rest of the RTP.
        let rows1: Vec<usize> = (0..self.x.len()).filter(|&i| self.mband[i] == 1).collect();
        let m1 = target(1);
        let dead_f = (total - m1 - m_rest) / mass[0];
        if m1 > 0.0 && dead_f >= 0.0 {
            if let Ok(mut out) = self.tilt(w, &rows1, m1, v1 / m1) {
                scale_rest(&mut out, dead_f);
                return Ok(out);
            }
        }
        // Else the small wins keep their shape; the hit rate follows the RTP.
        if val[1] <= 0.0 {
            return Err("no small win to carry the RTP".into());
        }
        let f1 = v1 / val[1];
        let dead_f = (total - f1 * mass[1] - m_rest) / mass[0];
        if dead_f < 0.0 {
            return Err("at the market's ladder, the small wins cannot carry the RTP".into());
        }
        let mut out: Vec<f64> = w.iter().zip(&self.mband).map(|(&wi, &b)| if b == 1 { wi * f1 } else { wi }).collect();
        scale_rest(&mut out, dead_f);
        Ok(out)
    }

    /// The weights of a blend (`key`, every payer a share) or of the global
    /// fix (`key` = `global`) — recomputed from scratch, as a fix cohort needs.
    fn blend_weights(&self, w: &[f64], spins: u32, key: &str) -> Result<Vec<f64>, String> {
        let before = self.measure(w, spins);
        let dir_of = |k: &str| -> Option<Vec<f64>> {
            let dirs: Vec<Vec<f64>> = payers(k).iter().filter_map(|p| self.fix_weights(w, spins, k, p).ok()).map(|nw| nw.iter().zip(w).map(|(a, b)| a - b).collect()).collect();
            mean_dir(&dirs)
        };
        let (keys, d): (Vec<&str>, Vec<f64>) = if key == "global" {
            const ALL: [&str; 7] = ["drought", "letdown", "dry", "broke", "reach", "shape", "crumbs"];
            let flagged: Vec<&'static str> = ALL.iter().copied().filter(|k| self.past_line(k, &before)).collect();
            return self.global_weights(w, spins, &flagged, &before).map(|(nw, _, _)| nw);
        } else {
            (vec![key], dir_of(key).ok_or("no playable way to average")?)
        };
        self.push_until(w, &d, spins, &keys, &before, None).map(|(nw, _, _)| nw).ok_or_else(|| "the average of the fixes is not playable".into())
    }
}

/// One fix for every frustration past its line.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalFix {
    /// The frustrations it takes on, and those it brings to their aim (or
    /// keeps under their line).
    pub parts: Vec<&'static str>,
    pub reached: Vec<&'static str>,
    pub option: FixOption,
    /// Aimed at the market's median of the peers.
    pub market: bool,
}

/// The average of a set of moves (`None`: no move).
fn mean_dir(dirs: &[Vec<f64>]) -> Option<Vec<f64>> {
    let first = dirs.first()?;
    let n = dirs.len() as f64;
    let mut out = vec![0.0; first.len()];
    for d in dirs {
        for (o, v) in out.iter_mut().zip(d) {
            *o += v / n;
        }
    }
    Some(out)
}

/// `w + t·d`, or `None` when a weight would go negative.
fn along(w: &[f64], d: &[f64], t: f64) -> Option<Vec<f64>> {
    let mut out = Vec::with_capacity(w.len());
    for (&a, &b) in w.iter().zip(d) {
        let v = a + t * b;
        if v < -1e-9 * a.abs().max(1.0) {
            return None;
        }
        out.push(v.max(0.0));
    }
    Some(out)
}

/// The reading a mode gets: a base game (cheap, its bonuses among ordinary
/// spins), or a bought round (costly, or every drawable round is a bonus).
pub fn shape_for(lut: &Lut, facts: &[BookFacts], cost: f64, imported: bool, params: FeelParams) -> Shape {
    let all_bonus = imported && lut.rows.iter().zip(facts).all(|(r, f)| f.bonus || r.weight == 0);
    if cost < 10.0 && !all_bonus {
        Shape::new(lut, facts, cost, imported, params)
    } else {
        Shape::buy(lut, cost, params)
    }
}

/// Integer LUT weights for re-weighted `w`: same total (scaled up when the
/// original total is small, so a thin tail keeps its shape once rounded).
pub fn integer_weights(w: &[f64], original_total: u64) -> Vec<u64> {
    let sum: f64 = w.iter().sum();
    if sum.is_nan() || sum <= 0.0 {
        return vec![0; w.len()];
    }
    let target = (original_total as f64).clamp(1e15, 4e18);
    w.iter().map(|&v| (v / sum * target).round().max(0.0) as u64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LutRow;
    use rand::{Rng, SeedableRng};
    use rand_xoshiro::Xoshiro256PlusPlus;

    /// A base mode: 1 bonus in ~420 spins, 42 % of them under 20x, a hit rate
    /// of ~25 %, a few base big wins.
    fn mode() -> (Lut, Vec<BookFacts>) {
        let base: &[(u64, f64)] = &[(750_000, 0.0), (60_000, 0.2), (60_000, 0.5), (50_000, 1.0), (40_000, 2.0), (30_000, 5.0), (7_000, 20.0), (1_500, 100.0), (40, 600.0), (60, 1_500.0)];
        let bonus: &[(u64, f64)] = &[(400, 5.0), (350, 10.0), (250, 15.0), (400, 25.0), (350, 40.0), (300, 80.0), (200, 150.0), (60, 300.0), (30, 800.0), (6, 3_000.0)];
        let mut rows = Vec::new();
        let mut facts = Vec::new();
        for (i, (w, x, b)) in base.iter().map(|&(w, x)| (w, x, false)).chain(bonus.iter().map(|&(w, x)| (w, x, true))).enumerate() {
            let p = (x * 100.0).round() as u64;
            rows.push(LutRow { sim_id: i as u64, weight: w, payout_x100: p });
            facts.push(BookFacts { sim_id: i as u64, weight: w, payout_x100: p, bonus: b, ..Default::default() });
        }
        (Lut::new(rows), facts)
    }

    fn weights(l: &Lut) -> Vec<f64> {
        l.rows.iter().map(|r| r.weight as f64).collect()
    }

    #[test]
    fn session_odds_are_the_binomial_ones() {
        // 1 in 300: e^-3.33 (1 + 3.33) ≈ 15.5 % of 1 000-spin sessions bring ≤ 1 bonus.
        let d = drought_share(1_000, 1.0 / 300.0, 1);
        assert!((d - 0.1546).abs() < 0.002, "{d}");
        assert!(drought_share(1_000, 1.0 / 420.0, 1) > 0.30);
        // Letdown: a session of ≥ 3 bonuses mostly has exactly 3, so the
        // chance sits just under q³; the absolute share is P(≥ 3) × that.
        let (cond, abs) = letdown(300, 1.0 / 400.0, 0.5, 3);
        assert!(cond > 0.10 && cond < 0.125, "{cond}");
        assert!(abs < cond * 0.1);
        // Brute force on a tiny session: 4 spins, p = 0.5, q = 0.5, k = 2.
        let (mut num, mut den) = (0.0, 0.0);
        for n in 2..=4u32 {
            let c = [1.0, 4.0, 6.0, 4.0, 1.0][n as usize];
            let pn = c * 0.5f64.powi(4);
            den += pn;
            num += pn * 0.5f64.powi(n as i32);
        }
        let (cond, _) = letdown(4, 0.5, 0.5, 2);
        assert!((cond - num / den).abs() < 1e-12);
    }

    #[test]
    fn dry_spells_match_a_simulation() {
        let (h, n) = (0.2, 1_000u32);
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(9);
        let mut longest: Vec<u32> = (0..20_000)
            .map(|_| {
                let (mut best, mut cur) = (0, 0);
                for _ in 0..n {
                    if rng.gen::<f64>() < h {
                        cur = 0;
                    } else {
                        cur += 1;
                        best = best.max(cur);
                    }
                }
                best
            })
            .collect();
        longest.sort_unstable();
        let (m50, m95) = (longest[10_000] as f64, longest[19_000] as f64);
        assert!((dry_quantile(n, h, 0.5) - m50).abs() <= 1.0, "{} vs {m50}", dry_quantile(n, h, 0.5));
        assert!((dry_quantile(n, h, 0.95) - m95).abs() <= 1.0, "{} vs {m95}", dry_quantile(n, h, 0.95));
    }

    #[test]
    fn the_mode_reads_as_intended() {
        let (l, f) = mode();
        let s = Shape::new(&l, &f, 1.0, true, FeelParams::default());
        let m = s.measure(&weights(&l), 1_000);
        let one_in = m.bonus_one_in.unwrap();
        assert!((one_in - 1e6 / 2_346.0).abs() < 1.0, "{one_in}");
        assert!((m.weak_share.unwrap() - 1_000.0 / 2_346.0).abs() < 1e-9);
        assert_eq!(m.bonus_median, Some(25.0));
        assert!(m.drought.unwrap() > DROUGHT.serious);
        assert!(m.letdown.unwrap() > LETDOWN.minor);
        // The budget adds up to the RTP.
        assert!((m.budget.iter().sum::<f64>() - m.rtp * 100.0).abs() < 1e-9);
    }

    /// Every fix of every frustration keeps the weight and the RTP, and
    /// brings its frustration to the target.
    #[test]
    fn fixes_keep_the_rtp_and_reach_their_target() {
        let (l, f) = mode();
        let s = Shape::new(&l, &f, 1.0, true, FeelParams::default());
        let w = weights(&l);
        let before = s.measure(&w, 1_000);
        let total: f64 = w.iter().sum();
        let r = s.report(&w, 1_000, Observed::default());
        for fr in &r.frustrations {
            let Some(fix) = &fr.fix else { continue };
            assert!(fix.recommended.is_some(), "{}: no option", fr.key);
            for o in fix.options.iter().filter(|o| o.feasible) {
                let a = o.after.as_ref().unwrap();
                assert!((a.rtp - before.rtp).abs() < 1e-9, "{} / {}: rtp {} → {}", fr.key, o.payer, before.rtp, a.rtp);
                let nw = s.fix_weights(&w, 1_000, fr.key, o.payer).unwrap();
                assert!((nw.iter().sum::<f64>() - total).abs() < 1e-6 * total);
                assert!(nw.iter().all(|&v| v >= 0.0));
                let value = match fr.key {
                    "drought" => a.drought.unwrap(),
                    "letdown" => a.letdown.unwrap(),
                    "broke" => a.broke.unwrap(),
                    "reach" => a.never_big,
                    "crumbs" => a.crumbs.unwrap(),
                    "shape" => a.bonus_ratio.unwrap(),
                    _ => a.dry_median,
                };
                if fr.key == "shape" {
                    // Back inside the band, on the side it came from.
                    let r = a.bonus_ratio.unwrap();
                    assert!(r >= SHAPE_BAND.0 - 1e-9 || r <= SHAPE_BAND.1 + 1e-9, "{} / {}: {r}", fr.key, o.payer);
                    assert!((fr.value < SHAPE_BAND.0) == (r >= SHAPE_BAND.0 - 1e-9), "{} / {}: {r}", fr.key, o.payer);
                } else {
                    assert!(value <= fr.target * 1.001 + 1e-9, "{} / {}: {value} > {}", fr.key, o.payer, fr.target);
                }
            }
        }
        let keys: Vec<&str> = r.frustrations.iter().filter(|f| f.fix.is_some()).map(|f| f.key).collect();
        assert!(keys.contains(&"drought") && keys.contains(&"letdown"), "{keys:?}");
    }

    #[test]
    fn each_payer_pays_where_it_says() {
        let (l, f) = mode();
        let s = Shape::new(&l, &f, 1.0, true, FeelParams::default());
        let w = weights(&l);
        let before = s.measure(&w, 1_000);
        let after = |key, payer| s.measure(&s.fix_weights(&w, 1_000, key, payer).unwrap(), 1_000);
        // More bonuses paid by the base game: fewer base wins.
        assert!(after("drought", "base").hit_rate < before.hit_rate);
        // …by the big wins: rarer big wins, the bonus keeps its median.
        let big = after("drought", "big");
        assert!(big.big_one_in.unwrap() > before.big_one_in.unwrap());
        assert_eq!(big.bonus_median, before.bonus_median);
        // …by the bonus itself: each bonus pays less.
        assert!(after("drought", "bonus").bonus_mean.unwrap() < before.bonus_mean.unwrap());
        // Fewer weak bonuses paid by rarer bonuses.
        assert!(after("letdown", "bonus").bonus_one_in.unwrap() > before.bonus_one_in.unwrap());
        assert!(after("letdown", "big").bonus_median.unwrap() >= before.bonus_median.unwrap());
    }

    #[test]
    fn a_payer_that_does_not_exist_says_so() {
        // No win reaches 500x: the big wins cannot pay.
        let rows = vec![LutRow { sim_id: 0, weight: 700, payout_x100: 0 }, LutRow { sim_id: 1, weight: 280, payout_x100: 150 }, LutRow { sim_id: 2, weight: 20, payout_x100: 2_000 }];
        let f: Vec<BookFacts> = rows.iter().map(|r| BookFacts { sim_id: r.sim_id, weight: r.weight, payout_x100: r.payout_x100, bonus: r.sim_id == 2, ..Default::default() }).collect();
        let l = Lut::new(rows);
        let s = Shape::new(&l, &f, 1.0, true, FeelParams::default());
        let err = s.fix_weights(&weights(&l), 1_000, "drought", "big").unwrap_err();
        assert!(err.contains("500x"), "{err}");
    }

    /// A base game shaped like the market: bonus 1 in 250 (median 40x),
    /// hit rate 19 %, 62.5 RTP points in the base game.
    fn market_mode() -> (Lut, Vec<BookFacts>) {
        let base: &[(u64, f64)] = &[(810_900, 0.0), (60_000, 0.5), (50_000, 1.0), (35_000, 2.0), (25_000, 5.0), (10_000, 10.0), (4_000, 25.0), (1_000, 100.0), (100, 500.0)];
        let bonus: &[(u64, f64)] = &[(800, 10.0), (800, 20.0), (1_000, 40.0), (700, 80.0), (400, 150.0), (200, 300.0), (80, 700.0), (20, 2_000.0)];
        let mut rows = Vec::new();
        let mut facts = Vec::new();
        for (i, (w, x, b)) in base.iter().map(|&(w, x)| (w, x, false)).chain(bonus.iter().map(|&(w, x)| (w, x, true))).enumerate() {
            let p = (x * 100.0).round() as u64;
            rows.push(LutRow { sim_id: i as u64, weight: w, payout_x100: p });
            facts.push(BookFacts { sim_id: i as u64, weight: w, payout_x100: p, bonus: b, ..Default::default() });
        }
        (Lut::new(rows), facts)
    }

    /// The reference of `BROKE`: a market-shaped game loses ~42 % of its
    /// 100-stake deposits before the first bonus; the mode of these tests
    /// (1 bonus in 426) loses more.
    #[test]
    fn market_shaped_broke() {
        let (l, f) = market_mode();
        let s = Shape::new(&l, &f, 1.0, true, FeelParams::default());
        let m = s.measure(&weights(&l), 1_000);
        let b = m.broke.unwrap();
        assert!((b - 0.42).abs() < 0.02, "{b}");
        assert!(b < BROKE.minor && b > BROKE.target);
        let (l2, f2) = mode();
        let heavy = Shape::new(&l2, &f2, 1.0, true, FeelParams::default()).measure(&weights(&l2), 1_000).broke.unwrap();
        assert!(heavy > b, "{heavy} vs {b}");
        // A bigger deposit, fewer broke; and the figure is the same on every run.
        let rich = Shape::new(&l, &f, 1.0, true, FeelParams { deposit: 300.0, ..FeelParams::default() }).measure(&weights(&l), 1_000).broke.unwrap();
        assert!(rich < b);
        assert_eq!(s.measure(&weights(&l), 1_000).broke.unwrap(), b);
    }

    #[test]
    fn reach_fixes_bring_the_big_wins_within_reach() {
        let (l, f) = mode();
        let s = Shape::new(&l, &f, 1.0, true, FeelParams::default());
        let w = weights(&l);
        let before = s.measure(&w, 1_000);
        assert!(before.never_big > REACH.target);
        for payer in payers("reach") {
            let nw = s.fix_weights(&w, 1_000, "reach", payer).unwrap_or_else(|e| panic!("{payer}: {e}"));
            let a = s.measure(&nw, 1_000);
            assert!((a.rtp - before.rtp).abs() < 1e-9, "{payer}");
            assert!((a.never_big - REACH.target).abs() < 1e-6, "{payer}: {}", a.never_big);
        }
        // All-or-nothing: the typical bonus pays — more of them fall weak.
        let nw = s.fix_weights(&w, 1_000, "reach", "bonus").unwrap();
        assert!(s.measure(&nw, 1_000).weak_share.unwrap() > before.weak_share.unwrap());
    }

    /// A 100x buy: 60 % of the purchases return under a quarter of the price.
    fn buy_mode() -> Lut {
        let rows: &[(u64, f64)] = &[(50, 0.0), (250, 5.0), (150, 12.0), (150, 20.0), (130, 40.0), (100, 80.0), (80, 150.0), (50, 300.0), (25, 700.0), (10, 1_500.0), (4, 5_000.0), (1, 20_000.0)];
        Lut::new(rows.iter().enumerate().map(|(i, &(w, x))| LutRow { sim_id: i as u64, weight: w, payout_x100: (x * 100.0).round() as u64 }).collect())
    }

    #[test]
    fn a_buy_reads_against_its_price_and_its_crumbs_can_be_lifted() {
        let l = buy_mode();
        let s = shape_for(&l, &[], 100.0, false, FeelParams::default());
        assert_eq!(s.kind(), Kind::Buy);
        let w = weights(&l);
        let m = s.measure(&w, 1_000);
        assert!((m.crumbs.unwrap() - 0.6).abs() < 1e-9, "{:?}", m.crumbs);
        assert!((m.near_zero.unwrap() - 0.3).abs() < 1e-9);
        assert_eq!(m.buy_median, Some(0.2));
        let refund = (80 + 50 + 25 + 10 + 4 + 1) as f64 / 1_000.0;
        assert!((m.refund.unwrap() - refund).abs() < 1e-9);
        assert!((m.never_refunded.unwrap() - (1.0 - refund).powi(5)).abs() < 1e-9);
        let r = s.report(&w, 1_000, Observed::default());
        let crumbs = r.frustrations.iter().find(|f| f.key == "crumbs").unwrap();
        assert_eq!(crumbs.severity, "serious");
        let fix = crumbs.fix.as_ref().unwrap();
        for o in &fix.options {
            let a = o.after.as_ref().unwrap_or_else(|| panic!("{}: {:?}", o.payer, o.reason));
            assert!((a.rtp - m.rtp).abs() < 1e-9, "{}", o.payer);
            assert!((a.crumbs.unwrap() - CRUMBS.target).abs() < 1e-9, "{}", o.payer);
        }
        let by = |p: &str| fix.options.iter().find(|o| o.payer == p).unwrap().after.clone().unwrap();
        // The pops pay: rarer; the middle pays: the pops keep their odds, fewer refunds.
        assert!(by("top").pop_one_in.unwrap() > m.pop_one_in.unwrap());
        assert!((by("middle").pop_one_in.unwrap() - m.pop_one_in.unwrap()).abs() < 1e-6);
        assert!(by("middle").refund.unwrap() < m.refund.unwrap());
        // All-or-nothing by design: read, not flagged.
        let bust = Lut::new(vec![LutRow { sim_id: 0, weight: 70, payout_x100: 0 }, LutRow { sim_id: 1, weight: 30, payout_x100: 32_000 }]);
        let r = shape_for(&bust, &[], 100.0, false, FeelParams::default()).report(&[70.0, 30.0], 1_000, Observed::default());
        let c = &r.frustrations[0];
        assert!(c.by_design && c.severity == "ok" && c.fix.is_none());
    }

    /// The typical bonus of the test mode (25x) is 0.40 × its average: under
    /// the market band. Each way of raising it lands back on the band and
    /// says where the RTP of the biggest bonuses went.
    #[test]
    fn a_lopsided_bonus_is_brought_back_to_the_band() {
        let (l, f) = mode();
        let s = Shape::new(&l, &f, 1.0, true, FeelParams::default());
        let w = weights(&l);
        let before = s.measure(&w, 1_000);
        let r = before.bonus_ratio.unwrap();
        assert!(r < SHAPE_BAND.0 && r > SHAPE_SERIOUS.0, "{r}");
        let rep = s.report(&w, 1_000, Observed::default());
        let shape = rep.frustrations.iter().find(|x| x.key == "shape").unwrap();
        assert_eq!((shape.severity, shape.band), ("minor", Some([SHAPE_BAND.0, SHAPE_BAND.1])));
        let after = |payer| s.measure(&s.fix_weights(&w, 1_000, "shape", payer).unwrap(), 1_000);
        for payer in payers("shape") {
            let a = after(payer);
            assert!((a.rtp - before.rtp).abs() < 1e-9, "{payer}");
            assert!(a.bonus_ratio.unwrap() >= SHAPE_BAND.0 - 1e-9, "{payer}: {:?}", a.bonus_ratio);
            // Always paid by the biggest bonuses.
            assert!(a.big_bonus_share.unwrap() <= before.big_bonus_share.unwrap() + 1e-12, "{payer}");
        }
        // Into the typical bonus: same frequency; into the base game: more
        // base wins; into the frequency: more bonuses.
        assert!((after("typical").bonus_one_in.unwrap() - before.bonus_one_in.unwrap()).abs() < 1e-6);
        assert!(after("base").hit_rate > before.hit_rate);
        assert!(after("frequency").bonus_one_in.unwrap() < before.bonus_one_in.unwrap());
    }

    /// The max win keeps its odds, whatever the fix and whoever pays.
    #[test]
    fn every_fix_keeps_the_max_win_odds() {
        let (l, f) = mode();
        let s = Shape::new(&l, &f, 1.0, true, FeelParams::default());
        let w = weights(&l);
        let before = s.measure(&w, 1_000).max_one_in.unwrap();
        for key in ["drought", "letdown", "broke", "reach", "shape"] {
            for payer in payers(key) {
                if let Ok(nw) = s.fix_weights(&w, 1_000, key, payer) {
                    let after = s.measure(&nw, 1_000).max_one_in.unwrap();
                    assert!((after - before).abs() < 1e-6 * before, "{key} / {payer}: 1 in {before} → {after}");
                }
            }
        }
    }

    /// « Always 15-25x »: a flat bonus is let loose back to the band.
    #[test]
    fn a_flat_bonus_is_let_loose() {
        let base: &[(u64, f64)] = &[(800_000, 0.0), (120_000, 0.5), (60_000, 1.5), (16_000, 5.0)];
        // The max win (5 000x) is pinned: the 600x bonuses are the ones to move.
        let bonus: &[(u64, f64)] = &[(1_600, 15.0), (1_200, 20.0), (800, 25.0), (360, 30.0), (40, 600.0), (1, 5_000.0)];
        let mut rows = Vec::new();
        let mut facts = Vec::new();
        for (i, (w, x, b)) in base.iter().map(|&(w, x)| (w, x, false)).chain(bonus.iter().map(|&(w, x)| (w, x, true))).enumerate() {
            let p = (x * 100.0).round() as u64;
            rows.push(LutRow { sim_id: i as u64, weight: w, payout_x100: p });
            facts.push(BookFacts { sim_id: i as u64, weight: w, payout_x100: p, bonus: b, ..Default::default() });
        }
        let l = Lut::new(rows);
        let s = Shape::new(&l, &facts, 1.0, true, FeelParams::default());
        let w = weights(&l);
        let before = s.measure(&w, 1_000);
        assert!(before.bonus_ratio.unwrap() > SHAPE_BAND.1, "{:?}", before.bonus_ratio);
        for payer in payers("shape") {
            let a = s.measure(&s.fix_weights(&w, 1_000, "shape", payer).unwrap_or_else(|e| panic!("{payer}: {e}")), 1_000);
            assert!((a.rtp - before.rtp).abs() < 1e-9, "{payer}");
            assert!(a.bonus_ratio.unwrap() <= SHAPE_BAND.1 + 1e-9, "{payer}: {:?}", a.bonus_ratio);
            // More bonuses reach the big line.
            assert!(a.big_bonus_share.unwrap() > before.big_bonus_share.unwrap(), "{payer}");
        }
    }

    /// Every payer pays a share: the average of a frustration's ways, pushed
    /// to the aim; and one fix for all the frustrations at once. Both keep the
    /// RTP, and a fix cohort rebuilds the same weights.
    #[test]
    fn blends_share_the_cost_and_one_fix_takes_on_everything() {
        let (l, f) = mode();
        let s = Shape::new(&l, &f, 1.0, true, FeelParams::default());
        let w = weights(&l);
        let before = s.measure(&w, 1_000);
        let r = s.report(&w, 1_000, Observed::default());
        let drought = r.frustrations.iter().find(|x| x.key == "drought").unwrap().fix.as_ref().unwrap();
        let blend = drought.options.iter().find(|o| o.payer == "blend").expect("a shared way");
        assert!(blend.reaches, "{:?}", blend.note);
        let a = blend.after.as_ref().unwrap();
        assert!((a.rtp - before.rtp).abs() < 1e-9);
        assert!(a.drought.unwrap() <= DROUGHT.target + 1e-9);
        // Each payer carries part of it: the base wins and the big wins both move.
        assert!(a.hit_rate < before.hit_rate && a.big_one_in.unwrap() > before.big_one_in.unwrap());
        let rebuilt = s.measure(&s.fix_weights(&w, 1_000, "drought", "blend").unwrap(), 1_000);
        assert!((rebuilt.drought.unwrap() - a.drought.unwrap()).abs() < 1e-12);

        let g = r.global.as_ref().expect("several frustrations: one fix for all");
        assert!(g.parts.len() >= 2 && g.parts.contains(&"drought"), "{:?}", g.parts);
        let ga = g.option.after.as_ref().unwrap();
        assert!((ga.rtp - before.rtp).abs() < 1e-9);
        assert_eq!(g.option.reaches, g.reached.len() == g.parts.len());
        // A flagged frustration is reached at its aim; one the market move
        // took on only has to stay under its line.
        let flagged: Vec<&str> = r.frustrations.iter().filter(|f| f.fix.is_some()).map(|f| f.key).collect();
        for k in &g.reached {
            if flagged.contains(k) {
                assert!(s.aim_reached(k, ga, &before), "{k}");
            } else {
                assert!(!s.past_line(k, ga), "{k}");
            }
        }
        let rebuilt = s.measure(&s.fix_weights(&w, 1_000, "global", "global").unwrap(), 1_000);
        assert!((rebuilt.rtp - before.rtp).abs() < 1e-9);
        assert!((rebuilt.hit_rate - ga.hit_rate).abs() < 1e-12);
    }

    /// At the market's median: each rung of the ladder and the max win's odds
    /// land on the peers' median, the RTP and the total weight stay.
    #[test]
    fn the_market_fit_lands_on_the_peers_median() {
        let (l, f) = market_mode();
        let s = Shape::new(&l, &f, 1.0, true, FeelParams::default());
        let peers = s.peers.clone().expect("a 2 000x base mode has peers");
        let w = weights(&l);
        let before = s.measure(&w, 1_000);
        let nw = s.market_fit(&w, &peers).unwrap();
        let a = s.measure(&nw, 1_000);
        assert!((a.rtp - before.rtp).abs() < 1e-9);
        assert!((nw.iter().sum::<f64>() - w.iter().sum::<f64>()).abs() < 1e-6 * w.iter().sum::<f64>());
        for k in ["hit", "p100", "p500", "pmax"] {
            let (v, med) = (a.figure(k).unwrap(), peers.median(k));
            assert!((v - med).abs() <= 1e-6 * med.max(1e-12) + 1e-12, "{k}: {v} vs {med}");
        }
        // The report puts the mode among its peers, now and after the global fix.
        let r = s.report(&w, 1_000, Observed::default());
        let mv = r.market.as_ref().unwrap();
        assert!(mv.count >= 1 && mv.stats.iter().any(|x| x.key == "p500"));
    }

    #[test]
    fn integer_weights_keep_a_thin_tail() {
        let w = [1.0e6, 0.4, 2.6];
        let iw = integer_weights(&w, 1_000_003);
        assert!(iw[1] > 0 && iw[2] > iw[1]);
        let ratio = iw[2] as f64 / iw[1] as f64;
        assert!((ratio - 6.5).abs() < 1e-6, "{ratio}");
    }
}
