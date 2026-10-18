//! Market reference — every mode of the slots the market actually plays: the
//! studios that make the turnover, and those of their slots that are played
//! (`scripts/market/fetch_market.py`, from the certified lookup tables
//! StakeCruncher serves). Embedded: SlotLab compares without the network.
//!
//! A mode is compared with its PEERS, modes of the same kind: a base mode
//! with the base modes whose max win is close to its own (a 20 000x game is
//! not read against a 500 000x one); an ante (« double chance ») with the
//! antes of the SAME cost — a 3x ante against the slots that have a 3x mode,
//! the closest max wins first when there are plenty; a buy or an
//! all-or-nothing mode with those of a close cost. A slot is ONE peer, however
//! many modes of the kind it has (`one_per_slot`), and a burst game is no slot
//! at all (`is_burst`). Every figure is in multiples of the mode's own stake
//! (its cost), computed the same way here (`figures`) and in the script.
//!
//! The embedded reference is only the fallback: SlotLab rebuilds the market
//! every day from the 30-day ranking (`slotlab-app/src/market_refresh.rs`)
//! and swaps it in with `install` — a market frozen at build time ages
//! unseen (2026-10-03: half of the 30-day top 50 was missing from the
//! reference of 2026-09-14).

use crate::model::Lut;
use crate::session;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};

/// The figures of a mode — the script writes them, `figures` computes them
/// for a lookup table under study. Probabilities are per round; `q*` are
/// quantiles of the return, in stakes; `vs*` the share of the RTP carried by
/// wins ≥ 10 / 100 / 1 000 stakes.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Fig {
    pub rtp: f64,
    pub hit: f64,
    pub zero: f64,
    pub vol: f64,
    /// Max win in base bets, and in stakes.
    pub max: f64,
    pub max_mult: f64,
    pub pmax: f64,
    pub q25: f64,
    pub median: f64,
    pub q75: f64,
    pub q90: f64,
    pub wins_under_stake: f64,
    pub crumbs: f64,
    pub near_zero: f64,
    pub vs10: f64,
    pub vs100: f64,
    pub vs1000: f64,
    pub p1: f64,
    pub p2: f64,
    pub p5: f64,
    pub p10: f64,
    pub p50: f64,
    pub p100: f64,
    pub p500: f64,
    pub p1000: f64,
    pub p5000: f64,
    pub p10000: f64,
    /// Share of the value carried by the top 1 % of the rounds (what SlotLab
    /// prints as « the top 1 % carries … »). NaN in a dataset built before it.
    #[serde(default = "nan", deserialize_with = "nan_or_f64", skip_serializing_if = "is_nan")]
    pub top1: f64,
    /// A 100-round session (`session`): median return, share of winning
    /// sessions, share giving back under 20 % — in stakes. NaN for buys and
    /// in a dataset built before them.
    #[serde(default = "nan", deserialize_with = "nan_or_f64", skip_serializing_if = "is_nan")]
    pub s100_median: f64,
    #[serde(default = "nan", deserialize_with = "nan_or_f64", skip_serializing_if = "is_nan")]
    pub s100_win: f64,
    #[serde(default = "nan", deserialize_with = "nan_or_f64", skip_serializing_if = "is_nan")]
    pub s100_bust: f64,
}

fn nan() -> f64 {
    f64::NAN
}

/// serde_json writes a NaN as `null`: read it back as the NaN it was.
fn nan_or_f64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(Option::<f64>::deserialize(d)?.unwrap_or(f64::NAN))
}

fn is_nan(v: &f64) -> bool {
    v.is_nan()
}

/// One mode of the reference.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketMode {
    pub rank: u32,
    pub slug: String,
    pub name: String,
    pub studio: String,
    /// The game's slug on stake.com, what Stake's burst list is keyed by
    /// (empty in a dataset built before it).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stake: String,
    /// Share of the 30-day turnover (the slot's).
    #[serde(default)]
    pub share: f64,
    #[serde(default)]
    pub mode: String,
    #[serde(default = "one")]
    pub cost: f64,
    /// `base` | `ante` | `buy` | `bust`.
    #[serde(default = "base_kind")]
    pub kind: String,
    #[serde(flatten)]
    pub fig: Fig,
}

fn one() -> f64 {
    1.0
}

fn base_kind() -> String {
    "base".into()
}

#[derive(Debug, Deserialize)]
struct Dataset {
    source: String,
    rule: String,
    fetched: String,
    #[serde(default)]
    coverage: f64,
    #[serde(default)]
    studios: u32,
    #[serde(alias = "games")]
    modes: Vec<MarketMode>,
}

/// A market installed since the build (`install`), else the embedded one.
static LIVE: RwLock<Option<&'static Dataset>> = RwLock::new(None);

fn prepare(mut d: Dataset) -> Dataset {
    // A burst game is not a slot, and a spinning mode that never pays 100
    // stakes is not a slot to aim at.
    d.modes.retain(|m| !is_burst(&m.stake) && (!matches!(m.kind.as_str(), "base" | "ante") || (m.fig.max_mult.max(m.fig.max / m.cost) >= 100.0 && m.fig.hit > 0.0)));
    d
}

/// Stake's « Burst Games » group (`data/burst_games.json`): instant games, not
/// slots, however their lookup tables look (2026-10-16, client: « Angry Balls
/// n'est pas une slot, c'est un burst game » — with Minedrop 2 and Drop The
/// Boss, it sat among the slots). stake.com turns a plain HTTP client away,
/// so the list is read in a browser: `scripts/market/burst_games.js`.
#[derive(Debug, Deserialize)]
struct BurstList {
    fetched: String,
    slugs: HashSet<String>,
}

fn bursts() -> &'static BurstList {
    static LIST: OnceLock<BurstList> = OnceLock::new();
    LIST.get_or_init(|| serde_json::from_str(include_str!("../data/burst_games.json")).expect("burst_games.json is valid"))
}

/// A game Stake files as a burst game, by its stake.com slug.
pub fn is_burst(stake_slug: &str) -> bool {
    !stake_slug.is_empty() && bursts().slugs.contains(stake_slug)
}

/// Day of the burst list (YYYY-MM-DD).
pub fn bursts_fetched() -> &'static str {
    bursts().fetched.as_str()
}

fn embedded() -> &'static Dataset {
    static DATA: OnceLock<Dataset> = OnceLock::new();
    DATA.get_or_init(|| prepare(serde_json::from_str(include_str!("../data/market.json")).expect("market.json is valid")))
}

fn data() -> &'static Dataset {
    LIVE.read().ok().and_then(|g| *g).unwrap_or_else(embedded)
}

/// What the market reference in use is made of.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketInfo {
    pub source: &'static str,
    pub rule: &'static str,
    /// Day of the ranking (YYYY-MM-DD).
    pub fetched: &'static str,
    pub coverage: f64,
    pub studios: u32,
    pub slots: usize,
    pub modes: usize,
    /// The reference built into SlotLab, not a refreshed one.
    pub embedded: bool,
}

/// The market reference in use.
pub fn info() -> MarketInfo {
    let d = data();
    let mut slots: Vec<&str> = d.modes.iter().map(|m| m.slug.as_str()).collect();
    slots.sort_unstable();
    slots.dedup();
    MarketInfo {
        source: d.source.as_str(),
        rule: d.rule.as_str(),
        fetched: d.fetched.as_str(),
        coverage: d.coverage,
        studios: d.studios,
        slots: slots.len(),
        modes: d.modes.len(),
        embedded: std::ptr::eq(d, embedded()),
    }
}

/// Day of the embedded reference (YYYY-MM-DD).
pub fn embedded_fetched() -> &'static str {
    embedded().fetched.as_str()
}

/// Swaps in a market reference (same JSON as `data/market.json`). The one it
/// replaces stays allocated — views already handed out point into it — which
/// costs a few hundred KB per refresh, one refresh a day.
pub fn install(json: &str) -> Result<MarketInfo, String> {
    let d = prepare(serde_json::from_str::<Dataset>(json).map_err(|e| format!("market.json: {e}"))?);
    let bases = d.modes.iter().filter(|m| m.kind == "base").count();
    if bases < MIN_PEERS {
        return Err(format!("market.json: {bases} base modes, not a market"));
    }
    let leaked: &'static Dataset = Box::leak(Box::new(d));
    *LIVE.write().map_err(|_| "market lock poisoned".to_string())? = Some(leaked);
    Ok(info())
}

/// Sessions of this many spins, for the « never 500x / 1 000x » figures, and
/// purchases in a row for « never refunded ».
pub const SESSION: f64 = 6_000.0;
pub const BUY_STREAK: i32 = 5;
/// A costly mode where this share of the rounds pays nothing is all-or-nothing.
pub const BUST_ZERO: f64 = 0.25;
/// Fewest peers, in slots (`one_per_slot`).
const MIN_PEERS: usize = 12;

/// The figures Player feel compares (spinning modes), in display order.
pub const KEYS: [&str; 10] = ["hit", "winsUnderStake", "p100", "p500", "p1000", "p5000", "pmax", "never500", "never1000", "vol"];

/// The figures worth comparing for each kind of mode, in display order.
pub fn keys_for(kind: &str) -> &'static [&'static str] {
    match kind {
        "buy" => &["rtp", "median", "q25", "q75", "q90", "crumbs", "nearZero", "p1", "neverRefunded", "p2", "p5", "p10", "p50", "pmax", "vs10", "top1", "vol"],
        "bust" => &["rtp", "zero", "p1", "p2", "p5", "p10", "p50", "pmax", "vol"],
        _ => &["rtp", "hit", "winsUnderStake", "s100Median", "s100Win", "s100Bust", "p1", "p10", "p100", "p500", "p1000", "p5000", "p10000", "pmax", "never500", "never1000", "vs100", "vs1000", "top1", "vol"],
    }
}

/// P(no win at probability `p` over a `SESSION`-spin session).
pub fn never(p: f64) -> f64 {
    if p >= 1.0 {
        0.0
    } else {
        (SESSION * (-p).ln_1p()).exp()
    }
}

/// `base` (a 1x stake), `ante` (under 10x), `buy`, or `bust` (a costly mode
/// where a quarter of the rounds or more pay nothing).
pub fn kind_of(cost: f64, zero: f64) -> &'static str {
    if (cost - 1.0).abs() < 1e-9 {
        "base"
    } else if cost < 10.0 {
        "ante"
    } else if zero >= BUST_ZERO {
        "bust"
    } else {
        "buy"
    }
}

/// A figure, by key.
pub trait Figures {
    fn figure(&self, key: &str) -> Option<f64>;
}

impl Figures for Fig {
    fn figure(&self, key: &str) -> Option<f64> {
        Some(match key {
            "rtp" => self.rtp,
            "hit" => self.hit,
            "zero" => self.zero,
            "vol" => self.vol,
            "pmax" => self.pmax,
            "q25" => self.q25,
            "median" => self.median,
            "q75" => self.q75,
            "q90" => self.q90,
            "winsUnderStake" => self.wins_under_stake,
            "crumbs" => self.crumbs,
            "nearZero" => self.near_zero,
            "vs10" => self.vs10,
            "vs100" => self.vs100,
            "vs1000" => self.vs1000,
            "p1" => self.p1,
            "p2" => self.p2,
            "p5" => self.p5,
            "p10" => self.p10,
            "p50" => self.p50,
            "p100" => self.p100,
            "p500" => self.p500,
            "p1000" => self.p1000,
            "p5000" => self.p5000,
            "p10000" => self.p10000,
            "never500" => never(self.p500),
            "never1000" => never(self.p1000),
            "neverRefunded" => (1.0 - self.p1).max(0.0).powi(BUY_STREAK),
            "top1" => self.top1,
            "s100Median" => self.s100_median,
            "s100Win" => self.s100_win,
            "s100Bust" => self.s100_bust,
            _ => return None,
        })
        // A figure the dataset does not hold (built before it) is no figure.
        .filter(|v| v.is_finite())
    }
}

impl Figures for MarketMode {
    fn figure(&self, key: &str) -> Option<f64> {
        self.fig.figure(key)
    }
}

/// The figures of a lookup table, as the script computes them.
pub fn figures(lut: &Lut, cost: f64) -> Fig {
    figures_of(lut.rows.iter().filter(|r| r.weight > 0).map(|r| (r.payout_x100 as f64 / 100.0, r.weight as f64)).collect(), cost)
}

/// The figures of a distribution: (payout in base bets, weight).
pub fn figures_of(mut dist: Vec<(f64, f64)>, cost: f64) -> Fig {
    let c = if cost > 0.0 { cost } else { 1.0 };
    dist.retain(|d| d.1 > 0.0);
    dist.sort_by(|a, b| a.0.total_cmp(&b.0));
    let w: f64 = dist.iter().map(|d| d.1).sum();
    if dist.is_empty() || w <= 0.0 {
        return Fig::default();
    }
    // From +0: an empty `sum()` of floats is −0, which would print « -0 ».
    let add = |it: &mut dyn Iterator<Item = f64>| it.fold(0.0, |a, b| a + b);
    let v: f64 = dist.iter().map(|&(x, ww)| x * ww).sum();
    let mean = v / w;
    let var = dist.iter().map(|&(x, ww)| ww * (x - mean).powi(2)).sum::<f64>() / w;
    let top = dist.last().map_or(0.0, |d| d.0);
    let at = |t: f64| add(&mut dist.iter().filter(|d| d.0 >= t * c - 1e-9).map(|d| d.1)) / w;
    let under = |t: f64| add(&mut dist.iter().filter(|d| d.0 < t * c - 1e-9).map(|d| d.1)) / w;
    let vfrom = |t: f64| if v > 0.0 { add(&mut dist.iter().filter(|d| d.0 >= t * c - 1e-9).map(|&(x, ww)| x * ww)) / v } else { 0.0 };
    let q = |p: f64| {
        let mut cum = 0.0;
        for &(x, ww) in &dist {
            cum += ww;
            if cum >= p * w - 1e-9 * w {
                return x / c;
            }
        }
        top / c
    };
    let hit = add(&mut dist.iter().filter(|d| d.0 > 0.0).map(|d| d.1)) / w;
    let under_stake = add(&mut dist.iter().filter(|d| d.0 > 0.0 && d.0 < c - 1e-9).map(|d| d.1)) / w;
    // Value in the top 1 % of the weight, from the biggest win down, the
    // straddling row taken in part (`population::top_slice`).
    let top1 = {
        let target = 0.01 * w;
        let (mut mass, mut top) = (0.0, 0.0);
        for &(x, ww) in dist.iter().rev() {
            if mass >= target {
                break;
            }
            let take = ww.min(target - mass);
            top += take * x;
            mass += take;
        }
        if v > 0.0 { top / v } else { 0.0 }
    };
    let s100 = if c < 10.0 {
        session::short_session(&dist.iter().map(|&(x, ww)| (x / c, ww)).collect::<Vec<_>>(), session::SPINS)
    } else {
        None
    };
    Fig {
        rtp: mean / c,
        hit,
        zero: 1.0 - hit,
        vol: var.sqrt() / c,
        max: top,
        max_mult: top / c,
        pmax: at(top / c),
        q25: q(0.25),
        median: q(0.5),
        q75: q(0.75),
        q90: q(0.9),
        wins_under_stake: if hit > 0.0 { under_stake / hit } else { 0.0 },
        crumbs: under(0.25),
        near_zero: under(0.1),
        vs10: vfrom(10.0),
        vs100: vfrom(100.0),
        vs1000: vfrom(1000.0),
        p1: at(1.0),
        p2: at(2.0),
        p5: at(5.0),
        p10: at(10.0),
        p50: at(50.0),
        p100: at(100.0),
        p500: at(500.0),
        p1000: at(1000.0),
        p5000: at(5000.0),
        p10000: at(10000.0),
        top1,
        s100_median: s100.map_or(f64::NAN, |s| s.median),
        s100_win: s100.map_or(f64::NAN, |s| s.win),
        s100_bust: s100.map_or(f64::NAN, |s| s.bust),
    }
}

/// The peers of a mode, and the band they span (of max win, or of cost).
#[derive(Debug, Clone)]
pub struct Peers {
    pub games: Vec<&'static MarketMode>,
    /// `max` | `cost`.
    pub by: &'static str,
    pub lo: f64,
    pub hi: f64,
    /// Max win of the mode compared: the peers' max-win odds are read at the
    /// same share of RTP (see `values`).
    pub anchor: f64,
}

/// Base modes whose max win is close to `max_win`.
pub fn peers(max_win: f64) -> Option<Peers> {
    peers_for("base", 1.0, max_win)
}

/// One mode per slot: of a slot's modes, the one closest to the mode compared
/// (`key`, smallest first) — a slot is one peer, however many modes of the
/// kind it has (2026-10-16: the six 1x modes of one game were 6 of the 17
/// peers of a 5 000x base game, and moved the hit-rate median from 20 % to
/// 29 %). Kept in the reference's order. The closest mode is in a window
/// around the anchor whenever any of the slot's modes is, so the windows
/// then count slots.
fn one_per_slot(pool: Vec<&'static MarketMode>, key: impl Fn(&MarketMode) -> (f64, f64)) -> Vec<&'static MarketMode> {
    let mut at: HashMap<&str, usize> = HashMap::new();
    let mut out: Vec<&'static MarketMode> = Vec::new();
    for m in pool {
        match at.get(m.slug.as_str()) {
            Some(&i) => {
                let (a, b) = (key(m), key(out[i]));
                if a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).is_lt() {
                    out[i] = m;
                }
            }
            None => {
                at.insert(m.slug.as_str(), out.len());
                out.push(m);
            }
        }
    }
    out
}

/// How far `x` is from `anchor`, as a ratio either way (the windows' ×/÷).
fn apart(x: f64, anchor: f64) -> f64 {
    (x / anchor).ln().abs()
}

/// Keeps the modes within ×/÷ f of `anchor` (by `of`), f widening until
/// there are at least `MIN_PEERS` of them.
fn widen(pool: Vec<&'static MarketMode>, anchor: f64, first: f64, of: impl Fn(&MarketMode) -> f64) -> Vec<&'static MarketMode> {
    for f in [first, 1.3, 1.6, 2.0, 3.0, 5.0].into_iter().filter(|&f| f >= first) {
        let got: Vec<&MarketMode> = pool.iter().copied().filter(|m| of(m) >= anchor / f && of(m) <= anchor * f).collect();
        if got.len() >= MIN_PEERS || f >= 5.0 {
            return got;
        }
    }
    Vec::new()
}

/// Peers of a mode, of the same kind, one mode per slot:
///   · base — the max win within ×/÷ 1.3, widened until there are 12 slots;
///   · ante, buy, bust — the SAME cost (±5 %: a 3x ante among the slots with
///     a 3x mode, a 100x buy among the 100x buys), then, when there are
///     plenty, the closest max wins (×/÷ 1.3 → …); widened on the cost only
///     when too few slots share it.
pub fn peers_for(kind: &str, cost: f64, max_win: f64) -> Option<Peers> {
    if cost.is_nan() || cost <= 0.0 || max_win.is_nan() || max_win <= 0.0 {
        return None;
    }
    let pool: Vec<&'static MarketMode> = data().modes.iter().filter(|m| m.kind == kind).collect();
    let near_max = |m: &MarketMode| apart(m.fig.max, max_win);
    let (games, by): (Vec<&MarketMode>, &'static str) = if kind == "base" {
        (widen(one_per_slot(pool, |m| (near_max(m), 0.0)), max_win, 1.3, |m| m.fig.max), "max")
    } else {
        // Narrowed by max win only among the SAME cost: across costs, the
        // commonest price (100x) would crowd out the closest ones.
        let same = one_per_slot(pool.iter().copied().filter(|m| (m.cost / cost - 1.0).abs() <= 0.05).collect(), |m| (near_max(m), 0.0));
        let near = if same.len() >= 2 * MIN_PEERS { widen(same.clone(), max_win, 1.3, |m| m.fig.max) } else { Vec::new() };
        if near.len() >= MIN_PEERS {
            (near, "cost+max")
        } else if same.len() >= MIN_PEERS {
            (same, "cost")
        } else {
            (widen(one_per_slot(pool, |m| (apart(m.cost, cost), near_max(m))), cost, 1.05, |m| m.cost), "cost")
        }
    };
    if games.is_empty() {
        return None;
    }
    let of = |m: &MarketMode| if by == "max" { m.fig.max } else { m.cost };
    let lo = games.iter().map(|m| of(m)).fold(f64::MAX, f64::min);
    let hi = games.iter().map(|m| of(m)).fold(0.0, f64::max);
    Some(Peers { games, by, lo, hi, anchor: max_win })
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let k = (sorted.len() - 1) as f64 * q;
    let (f, c) = (k.floor() as usize, (k.ceil() as usize).min(sorted.len() - 1));
    sorted[f] + (sorted[c] - sorted[f]) * (k - f as f64)
}

impl Peers {
    /// A figure of every peer. The max-win odds are rescaled to the compared
    /// max win at the SAME share of RTP (p × max kept): with few slots at the
    /// same max win, the peers span ×/÷ 2, and a 50,000× max at 1 in 2M is the
    /// weight of a 100,000× one at 1 in 4M — comparing raw odds made every
    /// bigger max win look « rarer than most ».
    fn values(&self, key: &str) -> Vec<f64> {
        let scale = |g: &MarketMode, v: f64| if key == "pmax" && self.anchor > 0.0 { v * g.fig.max / self.anchor } else { v };
        let mut v: Vec<f64> = self.games.iter().filter_map(|g| g.figure(key).map(|x| scale(g, x))).filter(|x| x.is_finite()).collect();
        v.sort_by(f64::total_cmp);
        v
    }

    pub fn median(&self, key: &str) -> f64 {
        quantile(&self.values(key), 0.5)
    }

    /// (p25, median, p75) of a figure.
    pub fn quartiles(&self, key: &str) -> (f64, f64, f64) {
        let v = self.values(key);
        (quantile(&v, 0.25), quantile(&v, 0.5), quantile(&v, 0.75))
    }

    /// Share of the peers whose figure is lower than `value`.
    pub fn rank(&self, key: &str, value: f64) -> f64 {
        let v = self.values(key);
        if v.is_empty() {
            return 0.0;
        }
        v.iter().filter(|&&x| x < value).count() as f64 / v.len() as f64
    }

    /// Their max wins (from, to), whatever chose them.
    fn max_window(&self) -> [f64; 2] {
        let lo = self.games.iter().map(|m| m.fig.max).fold(f64::MAX, f64::min);
        let hi = self.games.iter().map(|m| m.fig.max).fold(0.0, f64::max);
        [lo, hi]
    }

    fn cost_window(&self) -> [f64; 2] {
        let lo = self.games.iter().map(|m| m.cost).fold(f64::MAX, f64::min);
        let hi = self.games.iter().map(|m| m.cost).fold(0.0, f64::max);
        [lo, hi]
    }
}

/// One figure of the comparison table.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketStat {
    pub key: &'static str,
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
    /// This game, and after the global fix.
    pub value: f64,
    pub after: Option<f64>,
    /// Share of the peers under the value.
    pub rank: f64,
    pub rank_after: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketPeer {
    pub rank: u32,
    pub name: String,
    pub studio: String,
    pub mode: String,
    pub cost: f64,
    pub max: f64,
    pub hit: f64,
    pub p500: f64,
    pub pmax: f64,
    pub vol: f64,
    pub median: f64,
    pub p1: f64,
    pub share: f64,
}

impl From<&MarketMode> for MarketPeer {
    fn from(g: &MarketMode) -> Self {
        MarketPeer {
            rank: g.rank,
            name: g.name.clone(),
            studio: g.studio.clone(),
            mode: g.mode.clone(),
            cost: g.cost,
            max: g.fig.max,
            hit: g.fig.hit,
            p500: g.fig.p500,
            pmax: g.fig.pmax,
            vol: g.fig.vol,
            median: g.fig.median,
            p1: g.fig.p1,
            share: g.share,
        }
    }
}

/// Where a mode stands among its peers (Player feel).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketView {
    pub source: &'static str,
    pub rule: &'static str,
    pub fetched: &'static str,
    /// Share of the turnover the reference covers.
    pub coverage: f64,
    /// The peers' kind, their max wins and their costs (from, to), and how
    /// many.
    pub kind: String,
    pub window: [f64; 2],
    pub costs: [f64; 2],
    pub count: usize,
    pub stats: Vec<MarketStat>,
    /// The most played peers.
    pub peers: Vec<MarketPeer>,
}

impl Peers {
    fn top(&self, n: usize) -> Vec<MarketPeer> {
        let mut peers: Vec<&MarketMode> = self.games.clone();
        peers.sort_by_key(|g| g.rank);
        peers.iter().take(n).map(|&g| MarketPeer::from(g)).collect()
    }

    pub fn view(&self, now: &impl Figures, after: Option<&dyn Figures>) -> MarketView {
        let d = data();
        let stats = KEYS
            .iter()
            .filter_map(|&key| {
                let value = now.figure(key)?;
                let (p25, median, p75) = self.quartiles(key);
                let after_v = after.and_then(|a| a.figure(key));
                Some(MarketStat { key, p25, median, p75, value, after: after_v, rank: self.rank(key, value), rank_after: after_v.map(|v| self.rank(key, v)) })
            })
            .collect();
        MarketView {
            source: d.source.as_str(),
            rule: d.rule.as_str(),
            fetched: d.fetched.as_str(),
            coverage: d.coverage,
            kind: self.games.first().map_or_else(|| "base".into(), |g| g.kind.clone()),
            window: self.max_window(),
            costs: self.cost_window(),
            count: self.games.len(),
            stats,
            peers: self.top(15),
        }
    }
}

// ── The Analyze view: every mode of a game against its peers ────────────────

/// One figure of a mode against its peers: the spread (p10…p90) and where
/// the mode falls in it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub key: &'static str,
    pub value: f64,
    pub p10: f64,
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
    pub p90: f64,
    /// Share of the peers under the value.
    pub rank: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeMarket {
    pub mode: String,
    pub cost: f64,
    pub kind: &'static str,
    pub max: f64,
    /// How the peers were chosen (`max` | `cost` | `cost+max`), their max
    /// wins and costs (from, to), how many.
    pub by: &'static str,
    pub window: [f64; 2],
    pub costs: [f64; 2],
    pub count: usize,
    pub rows: Vec<Row>,
    pub peers: Vec<MarketPeer>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameMarket {
    pub source: &'static str,
    pub rule: &'static str,
    pub fetched: &'static str,
    pub coverage: f64,
    /// Modes of the reference, and slots.
    pub size: usize,
    pub slots: usize,
    pub modes: Vec<ModeMarket>,
    /// Modes with no peer (a kind or a cost the market does not play).
    pub alone: Vec<String>,
}

/// A mode of a game against its peers; `None` when it has none.
pub fn compare_mode(mode: &str, lut: &Lut, cost: f64) -> Option<ModeMarket> {
    let fig = figures(lut, cost);
    let kind = kind_of(cost, fig.zero);
    let peers = peers_for(kind, cost, fig.max)?;
    let rows = keys_for(kind)
        .iter()
        .filter_map(|&key| {
            let value = fig.figure(key)?;
            let v = peers.values(key);
            // A figure the reference does not hold yet (its next refresh will).
            if v.len() < MIN_PEERS / 2 {
                return None;
            }
            // Nothing to read where no one reaches it (10 000 stakes of a 3x ante).
            (value > 0.0 || v.last().is_some_and(|&x| x > 0.0)).then(|| Row {
                key,
                value,
                p10: quantile(&v, 0.1),
                p25: quantile(&v, 0.25),
                median: quantile(&v, 0.5),
                p75: quantile(&v, 0.75),
                p90: quantile(&v, 0.9),
                rank: peers.rank(key, value),
            })
        })
        .collect();
    Some(ModeMarket {
        mode: mode.to_string(),
        cost,
        kind,
        max: fig.max,
        by: peers.by,
        window: peers.max_window(),
        costs: peers.cost_window(),
        count: peers.games.len(),
        rows,
        peers: peers.top(20),
    })
}

/// Every mode of a game against its peers.
pub fn compare_game(modes: &[(String, &Lut, f64)]) -> GameMarket {
    let d = data();
    let mut out = Vec::new();
    let mut alone = Vec::new();
    for (name, lut, cost) in modes {
        match compare_mode(name, lut, *cost) {
            Some(m) => out.push(m),
            None => alone.push(name.clone()),
        }
    }
    let mut slots: Vec<&str> = d.modes.iter().map(|m| m.slug.as_str()).collect();
    slots.sort_unstable();
    slots.dedup();
    GameMarket { source: d.source.as_str(), rule: d.rule.as_str(), fetched: d.fetched.as_str(), coverage: d.coverage, size: d.modes.len(), slots: slots.len(), modes: out, alone }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LutRow;
    use serde_json::json;

    #[test]
    fn the_reference_loads_and_finds_peers_by_max_win() {
        assert!(data().modes.iter().filter(|m| m.kind == "base").count() >= 50);
        let p = peers(20_000.0).expect("20 000x is a common max win");
        assert!(p.games.len() >= MIN_PEERS);
        assert!(p.games.iter().all(|g| g.kind == "base" && g.fig.max >= 20_000.0 / 5.0 && g.fig.max <= 20_000.0 * 5.0));
        let (a, m, b) = p.quartiles("hit");
        assert!(a <= m && m <= b && m > 0.05 && m < 0.6, "{a} {m} {b}");
        // The ladder keeps its order in the medians: more 100x than 500x…
        assert!(p.median("p100") >= p.median("p500") && p.median("p500") >= p.median("p1000") && p.median("p1000") >= p.median("p5000"));
        assert!(peers(0.0).is_none());
    }

    #[test]
    fn a_double_chance_is_read_against_the_modes_of_its_cost() {
        // A 3x ante: among the slots that have a 3x mode — never a 1.25x one.
        let p = peers_for("ante", 3.0, 25_000.0).expect("3x antes are common");
        assert!(p.games.len() >= MIN_PEERS, "{}", p.games.len());
        assert!(p.games.iter().all(|g| g.kind == "ante" && (g.cost / 3.0 - 1.0).abs() <= 0.05));
        assert!(p.by.starts_with("cost"));
        // A cost few slots charge: the closest costs, still antes.
        let rare = peers_for("ante", 4.5, 25_000.0).expect("antes exist");
        assert!(rare.games.iter().all(|g| g.kind == "ante") && rare.games.len() >= MIN_PEERS.min(data().modes.iter().filter(|m| m.kind == "ante").count()));
        // Buys by their price too: a 100x buy among the 100x buys.
        let b = peers_for("buy", 100.0, 25_000.0).expect("100x buys are common");
        assert!(b.games.len() >= MIN_PEERS && b.games.iter().all(|g| g.kind == "buy" && g.cost == 100.0));
    }

    #[test]
    fn a_slot_is_one_peer_and_a_burst_game_none() {
        // However many modes of the kind a slot has, it is one peer…
        let groups = [peers(2_000.0), peers(5_000.0), peers(20_000.0), peers(100_000.0), peers_for("ante", 3.0, 25_000.0), peers_for("buy", 100.0, 25_000.0), peers_for("bust", 200.0, 10_000.0)];
        for p in groups.iter().flatten() {
            let mut slugs: Vec<&str> = p.games.iter().map(|g| g.slug.as_str()).collect();
            let n = slugs.len();
            slugs.sort_unstable();
            slugs.dedup();
            assert_eq!(slugs.len(), n, "a slot counted twice among the peers ({})", p.by);
        }
        // … through the mode closest to the one compared.
        let mode = |slug: &str, max: f64| -> &'static MarketMode { Box::leak(Box::new(serde_json::from_value(json!({ "rank": 1, "slug": slug, "name": slug, "studio": "S", "max": max, "hit": 0.3 })).unwrap())) };
        let kept = one_per_slot(vec![mode("a", 3_000.0), mode("b", 9_000.0), mode("a", 4_500.0), mode("a", 6_500.0)], |m| (apart(m.fig.max, 5_000.0), 0.0));
        assert_eq!(kept.iter().map(|m| (m.slug.as_str(), m.fig.max)).collect::<Vec<_>>(), [("a", 4_500.0), ("b", 9_000.0)]);
        // A game Stake files as a burst game leaves the reference.
        assert!(is_burst("coreffectinteractive-angry-balls") && !is_burst("") && !is_burst("no-such-game"));
        let row = |slug: &str, stake: &str| json!({ "rank": 1, "slug": slug, "name": slug, "studio": "S", "stake": stake, "mode": "base", "max": 5_000.0, "maxMult": 5_000.0, "hit": 0.3 });
        let d: Dataset = serde_json::from_value(json!({ "source": "", "rule": "", "fetched": "", "modes": [row("angry-balls", "coreffectinteractive-angry-balls"), row("a-slot", "studio-a-slot"), row("old-row", "")] })).unwrap();
        assert_eq!(prepare(d).modes.iter().map(|m| m.slug.as_str()).collect::<Vec<_>>(), ["a-slot", "old-row"]);
    }

    #[test]
    fn figures_read_a_lut_in_stakes() {
        // A 100x buy: half the purchases return 20x, a quarter 100x, a quarter 300x.
        let l = Lut::new(vec![
            LutRow { sim_id: 0, weight: 2, payout_x100: 2_000 },
            LutRow { sim_id: 1, weight: 1, payout_x100: 10_000 },
            LutRow { sim_id: 2, weight: 1, payout_x100: 30_000 },
        ]);
        let f = figures(&l, 100.0);
        assert!((f.rtp - 1.1).abs() < 1e-12);
        assert!((f.median - 0.2).abs() < 1e-12 && (f.q75 - 1.0).abs() < 1e-12);
        assert!((f.crumbs - 0.5).abs() < 1e-12 && (f.p1 - 0.5).abs() < 1e-12 && (f.p2 - 0.25).abs() < 1e-12);
        assert!((f.pmax - 0.25).abs() < 1e-12 && f.max_mult == 3.0);
        assert_eq!(kind_of(100.0, f.zero), "buy");
        assert!((f.figure("neverRefunded").unwrap() - 0.5f64.powi(5)).abs() < 1e-12);
    }
}
