//! Staging invariants, read from the books and WEIGHTED BY THE LUT.
//!
//! ── Why nothing else sees these ─────────────────────────────────────────────
//! A rule of staging — "the anticipation starts on the reel that can complete
//! the trigger", "the bonus grants the tier its scatters earned", "a wild only
//! ever moves onto the visible grid" — is invisible to every other check in the
//! toolbox. The RTP does not move when it breaks. The compliance report stays
//! green. `tier_audit` passes. The game runs, and the player is simply told
//! something that is not happening.
//!
//! The studio's own worst case: an anticipation that fired on 1 spin in 137
//! instead of 1 in 16.6 — eight times too rare, unnoticed for weeks, and only
//! found by re-reading the EVENTS of the published books.
//!
//! Two things make that reading trustworthy, and both are easy to get wrong:
//!
//!   · **weights, never counts.** A book is not a spin. The LUT decides how
//!     often each book is played, and the ratio between the rarest and the most
//!     common is routinely a factor of a million — a raw scan over the file
//!     describes a game nobody plays;
//!   · **the visible grid, never the raw board.** Boards ship with padding
//!     rows above and below the window. Counting scatters over the whole column
//!     silently inflates every number that matters.
//!
//! ── What "auto" means ───────────────────────────────────────────────────────
//! The trigger size and the height of the window are DERIVED from the books
//! (§32: measure, don't estimate): the trigger is the smallest scatter count
//! the game ever turns into a bonus, and the window is the tallest row index
//! the events ever address, plus one. Both can be overridden when a game does
//! something unusual — and the report always says which values it used.

use crate::freespins::{FsAnnounce, FsKey, RetriggerCounter};
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

// ─── The slice of a book this module reads ──────────────────────────────────

#[derive(Deserialize)]
pub struct Cell {
    #[serde(default)]
    pub scatter: bool,
    #[serde(default)]
    pub wild: bool,
}

#[derive(Deserialize, Clone, Copy)]
pub struct Pos {
    #[serde(default)]
    pub reel: i64,
    #[serde(default)]
    pub row: i64,
}

#[derive(Deserialize)]
pub struct Move {
    #[serde(default)]
    pub to: Option<Pos>,
}

/// Un événement tel que le scan le lit.
///
/// ⚠️ Désérialisé À LA MAIN (`impl Deserialize for Ev`, juste en dessous) : un
/// champ ajouté ici doit l'être aussi à `EvKey`, sinon il reste à sa valeur par
/// défaut sans la moindre erreur. La raison : toute clé qu'`Ev` ne lit pas
/// passe par `FsAnnounce::read`, qui y relève au passage les annonces de total
/// de free spins (`sharkFeed.hunts[].totalFs`…) — la règle de retriggers de
/// l'index de recherche, sans relire le book. Mesuré sur les books de BLOOD
/// WATER : un `#[serde(flatten)]` ferait la même chose en mettant chaque champ
/// inconnu en tampon (~17 % de scan en plus), une seconde lecture du book
/// coûterait ~60 %.
#[derive(Default)]
pub struct Ev<'a> {
    pub typ: &'a str,
    pub game_type: Option<&'a str>,
    pub board: Option<Vec<Vec<Cell>>>,
    pub anticipation: Option<Vec<i64>>,
    pub anticipation_row: Option<i64>,
    pub scatter_count: Option<i64>,
    pub positions: Option<Vec<Pos>>,
    pub moves: Option<Vec<Move>>,
    pub wins: Option<Vec<serde_json::Value>>,
    pub exploding_symbols: Option<Vec<Pos>>,
    /// Ce que l'événement annonce du total de free spins.
    pub fs: FsAnnounce,
}

/// Les clés d'un événement, reconnues sans allocation.
enum EvKey {
    Type,
    GameType,
    Board,
    Anticipation,
    AnticipationRow,
    ScatterCount,
    Positions,
    Moves,
    Wins,
    ExplodingSymbols,
    Fs(FsKey),
}

impl<'de> Deserialize<'de> for EvKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = EvKey;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an event field name")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<EvKey, E> {
                Ok(match v {
                    "type" => EvKey::Type,
                    "gameType" => EvKey::GameType,
                    "board" => EvKey::Board,
                    "anticipation" => EvKey::Anticipation,
                    "anticipationRow" => EvKey::AnticipationRow,
                    // BLOOD WATER écrit le palier du bonus `sharkCount` (le requin est
                    // son scatter) : sans l'alias, chaque bonus lisait un palier 0.
                    "scatterCount" | "sharkCount" => EvKey::ScatterCount,
                    "positions" => EvKey::Positions,
                    "moves" => EvKey::Moves,
                    "wins" => EvKey::Wins,
                    "explodingSymbols" => EvKey::ExplodingSymbols,
                    other => EvKey::Fs(FsKey::of(other)),
                })
            }
        }
        d.deserialize_identifier(V)
    }
}

impl<'de: 'a, 'a> Deserialize<'de> for Ev<'a> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<'a>(PhantomData<Ev<'a>>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = Ev<'a>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a book event")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ev<'a>, A::Error> {
                // Champ absent → valeur par défaut, comme `#[serde(default)]`.
                let mut ev = Ev::default();
                while let Some(key) = map.next_key::<EvKey>()? {
                    match key {
                        EvKey::Type => ev.typ = map.next_value()?,
                        EvKey::GameType => ev.game_type = map.next_value()?,
                        EvKey::Board => ev.board = map.next_value()?,
                        EvKey::Anticipation => ev.anticipation = map.next_value()?,
                        EvKey::AnticipationRow => ev.anticipation_row = map.next_value()?,
                        EvKey::ScatterCount => ev.scatter_count = map.next_value()?,
                        EvKey::Positions => ev.positions = map.next_value()?,
                        EvKey::Moves => ev.moves = map.next_value()?,
                        EvKey::Wins => ev.wins = map.next_value()?,
                        EvKey::ExplodingSymbols => ev.exploding_symbols = map.next_value()?,
                        EvKey::Fs(k) => ev.fs.read(k, &mut map)?,
                    }
                }
                Ok(ev)
            }
        }
        d.deserialize_map(V(PhantomData))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Book<'a> {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub payout_multiplier: f64,
    #[serde(default, borrow)]
    pub criteria: Option<&'a str>,
    #[serde(default, borrow)]
    pub events: Vec<Ev<'a>>,
}

// ─── Parameters ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timings {
    /// A board coming to rest, reels included, at the normal speed.
    pub reveal: f64,
    /// Each anticipated reel, on top of its normal fall.
    pub anticipation_step: f64,
    /// One cascade: explosion + refill.
    pub tumble: f64,
    /// Presenting one win line / cluster.
    pub win: f64,
    /// Entering the bonus.
    pub bonus_intro: f64,
    /// Leaving it.
    pub bonus_outro: f64,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            reveal: 1.2,
            anticipation_step: 1.1,
            tumble: 0.45,
            win: 0.3,
            bonus_intro: 2.0,
            bonus_outro: 2.5,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanParams {
    /// Scatters that trigger the bonus. 0 = derive it from the books.
    pub trigger_scatters: i64,
    /// Scatters that re-trigger inside the bonus. 0 = derive it.
    pub retrigger_scatters: i64,
    /// Height of the visible window. 0 = derive it from the events.
    pub visible_rows: i64,
    /// Scatters a base-game board is allowed to show. 0 = no ceiling checked.
    pub max_base_scatters: i64,
    pub timings: Timings,
}

impl Default for ScanParams {
    fn default() -> Self {
        Self {
            trigger_scatters: 0,
            retrigger_scatters: 0,
            visible_rows: 0,
            max_base_scatters: 0,
            timings: Timings::default(),
        }
    }
}

/// What a sample of books says about the shape of the game.
#[derive(Debug, Clone, Copy, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    pub trigger_scatters: i64,
    /// Scatters a board must show for the BONUS to announce a re-trigger.
    /// 0 — the default — means the game does not anticipate inside the bonus
    /// at all (`anticipation_triggers[freegame] = 100`, §18.3): on a tumbling
    /// slot where one board in four already shows a scatter, suspense would
    /// fire constantly for one extra free spin. Set it only for a game that
    /// really does announce re-triggers.
    pub retrigger_scatters: i64,
    pub visible_rows: i64,
    pub board_rows: i64,
    pub reels: i64,
    /// Rows of padding taken off each end of a column.
    pub pad: i64,
    /// The last reel a scatter can land on, when the sample shows one — `None`
    /// means every reel. It is the last reel that can COMPLETE a trigger: on a game
    /// whose scatters stop before the last column (BLOOD WATER: columns 2-5 of
    /// six), announcing the last column would promise what it cannot bring, and
    /// the intra-column announcement belongs to the last SCATTER reel.
    pub last_scatter_reel: Option<i64>,
    /// The most scatters a base-game board ever shows in the sample. A board that
    /// reaches it on the threshold reel has nothing left to announce: no reel can
    /// bring another scatter (BLOOD WATER's HIDDEN: the 4th shark stacked on the
    /// 3rd completes the bonus and the cap in the same column).
    pub max_base_scatters: Option<i64>,
    /// True when the values came from the books rather than the request.
    pub derived: bool,
}

/// Derives the trigger size and the padding from a sample of books.
///
/// ⚠️ Event coordinates are written in the PADDED frame: with
/// `include_padding = True` the math-sdk ships one row above and one below the
/// window (8 rows per column for a 6-row grid) and shifts every emitted `row`
/// by +1. So the padding is read from the row indices the events NEVER use:
/// the lowest row seen, and the distance from the highest row seen to the
/// bottom of the column. Taking the highest row alone would miss it entirely —
/// and counting scatters over the padding is what makes an anticipation scan
/// report failures that the player could never have seen.
pub fn detect_shape(sample: &[String]) -> Shape {
    let mut trigger = i64::MAX;
    let mut max_row = -1i64;
    let mut min_row = i64::MAX;
    let mut board_rows = 0i64;
    let mut reels = 0i64;
    let mut last_scatter_reel = -1i64;
    // Scatter rows of every base-game reveal, counted once the padding is known.
    let mut base_scatter_rows: Vec<Vec<i64>> = Vec::new();
    for line in sample {
        let Ok(book) = serde_json::from_str::<Book>(line) else {
            continue;
        };
        for ev in &book.events {
            if let Some(b) = &ev.board {
                reels = reels.max(b.len() as i64);
                board_rows = board_rows.max(b.iter().map(|c| c.len() as i64).max().unwrap_or(0));
                for (i, col) in b.iter().enumerate() {
                    if col.iter().any(|c| c.scatter) {
                        last_scatter_reel = last_scatter_reel.max(i as i64);
                    }
                }
                if ev.typ == "reveal" && ev.game_type.map(|g| g == "basegame").unwrap_or(true) {
                    base_scatter_rows.push(
                        b.iter()
                            .flat_map(|col| {
                                col.iter().enumerate().filter(|(_, c)| c.scatter).map(|(r, _)| r as i64)
                            })
                            .collect(),
                    );
                }
            }
            if let Some(p) = &ev.positions {
                for pos in p {
                    max_row = max_row.max(pos.row);
                    min_row = min_row.min(pos.row);
                }
                if ev.typ.eq_ignore_ascii_case("freeSpinTrigger") && !p.is_empty() {
                    trigger = trigger.min(p.len() as i64);
                }
            }
            if let Some(sc) = ev.scatter_count {
                if ev.typ.eq_ignore_ascii_case("bonusSetup") && sc > 0 {
                    trigger = trigger.min(sc);
                }
            }
            if let Some(m) = &ev.moves {
                for mv in m {
                    if let Some(to) = mv.to {
                        max_row = max_row.max(to.row);
                        min_row = min_row.min(to.row);
                    }
                }
            }
            if let Some(x) = &ev.exploding_symbols {
                for pos in x {
                    max_row = max_row.max(pos.row);
                    min_row = min_row.min(pos.row);
                }
            }
        }
    }
    // Padding is symmetric, so both ends must agree: the smaller of "rows the
    // events never address at the top" and "rows they never address at the
    // bottom" is the padding, and a game with none lands on 0 on its own.
    let pad = if max_row >= 0 && min_row != i64::MAX && board_rows > 0 {
        min_row.min(board_rows - 1 - max_row).max(0)
    } else {
        0
    };
    let visible_rows = board_rows - 2 * pad;
    Shape {
        trigger_scatters: if trigger == i64::MAX { 0 } else { trigger },
        // Deliberately 0: the bonus announces nothing unless told otherwise.
        retrigger_scatters: 0,
        visible_rows,
        board_rows,
        reels,
        pad,
        // A sample that never shows a scatter says nothing: every reel, then.
        last_scatter_reel: (last_scatter_reel >= 0 && last_scatter_reel < reels - 1)
            .then_some(last_scatter_reel),
        max_base_scatters: base_scatter_rows
            .iter()
            .map(|rows| rows.iter().filter(|r| **r >= pad && **r < board_rows - pad).count() as i64)
            .max()
            .filter(|m| *m > 0),
        derived: true,
    }
}

// ─── Accumulator ────────────────────────────────────────────────────────────

/// Weighted count: how many books, and how much LUT weight they carry.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tally {
    pub books: u64,
    pub weight: f64,
}

impl Tally {
    fn add(&mut self, w: f64) {
        self.books += 1;
        self.weight += w;
    }
}

/// Mean payout of a group, in bet multiples, plus its share of the spins.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub books: u64,
    pub weight: f64,
    pub probability: f64,
    pub one_in: f64,
    pub avg_payout: f64,
}

#[derive(Debug, Clone, Default)]
struct RawGroup {
    books: u64,
    weight: f64,
    wp: f64,
}

impl RawGroup {
    fn add(&mut self, w: f64, payout: f64) {
        self.books += 1;
        self.weight += w;
        self.wp += w * payout;
    }
    fn finish(&self, total: f64) -> Group {
        Group {
            books: self.books,
            weight: self.weight,
            probability: if total > 0.0 { self.weight / total } else { 0.0 },
            one_in: if self.weight > 0.0 {
                total / self.weight
            } else {
                f64::INFINITY
            },
            avg_payout: if self.weight > 0.0 {
                self.wp / self.weight
            } else {
                0.0
            },
        }
    }
}

/// Accumulates one mode. Merge-able so the scan can run in parallel.
#[derive(Debug, Clone, Default)]
pub struct Accumulator {
    pub total_weight: f64,
    pub books: u64,
    /// Books whose id the LUT never draws (weight 0) — they exist in the file
    /// and are never played.
    pub unweighted_books: u64,

    // Events
    events: BTreeMap<String, RawGroup>,
    event_occurrences: BTreeMap<String, f64>,

    // Anticipation
    pub antic_emitted: Tally,
    pub antic_deserved: Tally,
    pub antic_missing: Tally,
    pub antic_early: Tally,
    pub antic_late: Tally,
    pub antic_spurious: Tally,
    antic_start_reel: BTreeMap<i64, f64>,

    // Scatters on the visible grid
    scatters_base: BTreeMap<i64, RawGroup>,
    scatters_free: BTreeMap<i64, RawGroup>,
    pub over_cap: Tally,

    // Bonus
    bonus_tiers: BTreeMap<i64, RawGroup>,
    pub tier_board_mismatch: Tally,
    pub tier_criteria_mismatch: Tally,
    retriggers: BTreeMap<i64, RawGroup>,

    // Wilds
    pub wild_moves_offgrid: Tally,
    wild_move_count: f64,

    // Board readability
    pub reveals_without_board: Tally,

    // Pacing, in half-second buckets
    duration_buckets: BTreeMap<u32, f64>,
    duration_wsum: f64,

    /// Books that could not be parsed — never silently dropped.
    pub parse_errors: u64,
}

const BUCKET_S: f64 = 0.5;

impl Accumulator {
    pub fn merge(&mut self, other: &Accumulator) {
        self.total_weight += other.total_weight;
        self.books += other.books;
        self.unweighted_books += other.unweighted_books;
        self.parse_errors += other.parse_errors;
        merge_map(&mut self.events, &other.events);
        for (k, v) in &other.event_occurrences {
            *self.event_occurrences.entry(k.clone()).or_default() += v;
        }
        for (a, b) in [
            (&mut self.antic_emitted, &other.antic_emitted),
            (&mut self.antic_deserved, &other.antic_deserved),
            (&mut self.antic_missing, &other.antic_missing),
            (&mut self.antic_early, &other.antic_early),
            (&mut self.antic_late, &other.antic_late),
            (&mut self.antic_spurious, &other.antic_spurious),
            (&mut self.over_cap, &other.over_cap),
            (&mut self.tier_board_mismatch, &other.tier_board_mismatch),
            (&mut self.tier_criteria_mismatch, &other.tier_criteria_mismatch),
            (&mut self.wild_moves_offgrid, &other.wild_moves_offgrid),
            (&mut self.reveals_without_board, &other.reveals_without_board),
        ] {
            a.books += b.books;
            a.weight += b.weight;
        }
        for (k, v) in &other.antic_start_reel {
            *self.antic_start_reel.entry(*k).or_default() += v;
        }
        merge_map_i(&mut self.scatters_base, &other.scatters_base);
        merge_map_i(&mut self.scatters_free, &other.scatters_free);
        merge_map_i(&mut self.bonus_tiers, &other.bonus_tiers);
        merge_map_i(&mut self.retriggers, &other.retriggers);
        self.wild_move_count += other.wild_move_count;
        for (k, v) in &other.duration_buckets {
            *self.duration_buckets.entry(*k).or_default() += v;
        }
        self.duration_wsum += other.duration_wsum;
    }

    /// Reads one book. `weight` is its LUT weight — 0 for a book the game
    /// can never draw.
    pub fn push(&mut self, line: &str, weight: u64, shape: &Shape, params: &ScanParams) {
        let w = weight as f64;
        let Ok(book) = serde_json::from_str::<Book>(line) else {
            self.parse_errors += 1;
            return;
        };
        self.books += 1;
        self.total_weight += w;
        if weight == 0 {
            self.unweighted_books += 1;
        }
        let payout = book.payout_multiplier / 100.0;
        let pad = shape.pad.max(0) as usize;

        let mut seen: BTreeMap<String, u32> = BTreeMap::new();
        let mut duration = 0.0f64;
        // Même règle que l'index de recherche : compter les seuls
        // `freeSpinRetrigger` lisait 0 sur tout jeu qui annonce ses
        // retriggers autrement (BLOOD WATER : un repas = un spin).
        let mut retriggers = RetriggerCounter::default();
        let mut emitted_any = false;
        let mut deserved_any = false;
        // Board of the last reveal, to check what the bonus was granted for.
        let mut last_visible_scatters = 0i64;

        for ev in &book.events {
            *seen.entry(ev.typ.to_string()).or_default() += 1;
            // Avant le `match` : ses bras sortent de l'itération par `continue`.
            retriggers.push(ev.typ, &ev.fs);

            match ev.typ {
                "reveal" | "tumbleBoard" => {
                    let is_base = ev.game_type.map(|g| g == "basegame").unwrap_or(true);
                    // Inside the bonus it is the RE-trigger that is being
                    // announced, and it is cheaper.
                    let trigger = if is_base {
                        shape.trigger_scatters
                    } else {
                        shape.retrigger_scatters
                    };
                    // Only a reveal drops reel by reel: a cascade refills in
                    // place and has nothing to announce.
                    let is_reveal = ev.typ == "reveal";
                    if ev.typ == "reveal" {
                        duration += params.timings.reveal;
                    } else {
                        duration += params.timings.tumble;
                    }
                    let Some(board) = &ev.board else {
                        if ev.typ == "reveal" {
                            self.reveals_without_board.add(w);
                        }
                        continue;
                    };
                    // Cumulative scatters over the visible window, reel by
                    // reel: the anticipation can only be justified from the
                    // reel where the count reaches trigger − 1.
                    let mut cum = 0i64;
                    let mut deserved_from: Option<i64> = None;
                    // Visible row (0 = top) of the scatter that reaches trigger − 1,
                    // in DROP order: a column fills from the bottom up.
                    let mut threshold_row: Option<usize> = None;
                    let max_scatters = if params.max_base_scatters > 0 {
                        Some(params.max_base_scatters)
                    } else {
                        shape.max_base_scatters
                    };
                    // `last` is the last reel that can still COMPLETE the trigger:
                    // the last reel a scatter can land on, not the last reel of the
                    // board. The threshold reached ON it is the one case where the
                    // announcement is intra-column: there is no next reel to slow
                    // down, so the reel falls at normal speed up to its scatter and
                    // only the rows above it slow (§18.4, the "three scatters must
                    // be on screen" fix) — which needs rows left above that scatter.
                    let last = shape
                        .last_scatter_reel
                        .unwrap_or(board.len() as i64 - 1)
                        .min(board.len() as i64 - 1);
                    let mut settled = false;
                    for (i, col) in board.iter().enumerate() {
                        let hi = col.len().saturating_sub(pad);
                        let visible = col.get(pad..hi).unwrap_or(&[]);
                        let here = visible.iter().filter(|c| c.scatter).count() as i64;
                        if !settled && trigger > 1 && cum + here >= trigger - 1 {
                            settled = true;
                            // A board that reaches the most scatters it ever can within
                            // the threshold reel has nothing left for a LATER reel to
                            // bring. (On the last scatter reel the rows above the
                            // threshold are what is announced — the completing scatter
                            // may be among them.)
                            let full = is_base
                                && (i as i64) < last
                                && max_scatters.is_some_and(|m| cum + here >= m);
                            if !full {
                                // The NEXT reel is the one worth announcing.
                                deserved_from = Some(i as i64 + 1);
                                let mut need = trigger - 1 - cum;
                                threshold_row = visible
                                    .iter()
                                    .enumerate()
                                    .rev()
                                    .filter(|(_, c)| c.scatter)
                                    .find_map(|(r, _)| {
                                        need -= 1;
                                        (need == 0).then_some(r)
                                    });
                            }
                        }
                        cum += here;
                    }
                    let total_visible = cum;
                    last_visible_scatters = total_visible;

                    let entry = if is_base {
                        self.scatters_base.entry(total_visible).or_default()
                    } else {
                        self.scatters_free.entry(total_visible).or_default()
                    };
                    entry.add(w, payout);

                    if is_base
                        && params.max_base_scatters > 0
                        && total_visible > params.max_base_scatters
                    {
                        self.over_cap.add(w);
                    }

                    if !is_reveal {
                        continue;
                    }
                    // Anticipation: emitted vs deserved, reel by reel.
                    let emitted_from = ev.anticipation.as_ref().and_then(|a| {
                        a.iter().position(|v| *v > 0).map(|i| i as i64)
                    });
                    if let Some(from) = emitted_from {
                        emitted_any = true;
                        duration += params.timings.anticipation_step
                            * ev.anticipation
                                .as_ref()
                                .map(|a| a.iter().filter(|v| **v > 0).count() as f64)
                                .unwrap_or(0.0);
                        *self.antic_start_reel.entry(from).or_default() += w;
                    }
                    let intra_deserved =
                        deserved_from == Some(last + 1) && threshold_row.is_some_and(|r| r > 0);
                    if deserved_from.is_some_and(|d| d <= last) || intra_deserved {
                        deserved_any = true;
                    }
                    match (emitted_from, deserved_from) {
                        // Announcing a reel no scatter can land on.
                        (Some(e), _) if e > last => self.antic_spurious.add(w),
                        (Some(e), Some(d)) => {
                            let intra_column = intra_deserved && e == last;
                            if !intra_column {
                                if e < d {
                                    self.antic_early.add(w);
                                } else if e > d {
                                    self.antic_late.add(w);
                                }
                            }
                        }
                        // Announced with no reel able to complete the trigger.
                        (Some(_), None) => self.antic_spurious.add(w),
                        // A board that could have completed, announced by
                        // nothing: the failure mode that hid for weeks.
                        (None, Some(d)) if d <= last || intra_deserved => {
                            self.antic_missing.add(w)
                        }
                        _ => {}
                    }
                }
                "winInfo" => {
                    duration += params.timings.win
                        * ev.wins.as_ref().map(|w| w.len() as f64).unwrap_or(1.0);
                }
                "freeSpinTrigger" => {
                    duration += params.timings.bonus_intro;
                }
                "freeSpinEnd" => {
                    duration += params.timings.bonus_outro;
                }
                "bonusSetup" => {
                    let tier = ev.scatter_count.unwrap_or(0);
                    self.bonus_tiers.entry(tier).or_default().add(w, payout);
                    if last_visible_scatters > 0 && tier != last_visible_scatters {
                        self.tier_board_mismatch.add(w);
                    }
                    // `freegame_4` names the tier the criteria promises.
                    if let Some(c) = book.criteria {
                        if let Some(n) = trailing_number(c) {
                            if n != tier {
                                self.tier_criteria_mismatch.add(w);
                            }
                        }
                    }
                }
                "wildMove" => {
                    if let Some(moves) = &ev.moves {
                        self.wild_move_count += w * moves.len() as f64;
                        for m in moves {
                            if let Some(to) = m.to {
                                // Padded frame: the window is [pad, pad + visible).
                                let lo = shape.pad;
                                let hi = shape.pad + shape.visible_rows;
                                if to.row < lo
                                    || to.row >= hi
                                    || to.reel < 0
                                    || (shape.reels > 0 && to.reel >= shape.reels)
                                {
                                    self.wild_moves_offgrid.add(w);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if emitted_any {
            self.antic_emitted.add(w);
        }
        if deserved_any {
            self.antic_deserved.add(w);
        }
        self.retriggers
            .entry(retriggers.count() as i64)
            .or_default()
            .add(w, payout);

        for (name, count) in seen {
            self.events.entry(name.clone()).or_default().add(w, payout);
            *self.event_occurrences.entry(name).or_default() += w * count as f64;
        }

        let bucket = (duration / BUCKET_S).floor().max(0.0) as u32;
        *self.duration_buckets.entry(bucket).or_default() += w;
        self.duration_wsum += w * duration;
    }

    pub fn finish(&self, mode: &str, cost: f64, shape: Shape, params: ScanParams) -> ScanReport {
        let t = self.total_weight;
        let p = |x: f64| if t > 0.0 { x / t } else { 0.0 };
        let one_in = |x: f64| if x > 0.0 { t / x } else { f64::INFINITY };

        let events: Vec<EventStat> = self
            .events
            .iter()
            .map(|(name, g)| EventStat {
                name: name.clone(),
                group: g.finish(t),
                per_spin: self
                    .event_occurrences
                    .get(name)
                    .map(|o| if t > 0.0 { o / t } else { 0.0 })
                    .unwrap_or(0.0),
            })
            .collect();

        let dist = |m: &BTreeMap<i64, RawGroup>| -> Vec<Bucketed> {
            m.iter()
                .map(|(k, g)| Bucketed {
                    value: *k,
                    group: g.finish(t),
                })
                .collect()
        };

        // Tier monotonicity: more scatters must never mean a worse bonus.
        let tiers = dist(&self.bonus_tiers);
        let mut monotonic = true;
        let mut previous = f64::NEG_INFINITY;
        for row in tiers.iter().filter(|r| r.group.weight > 0.0) {
            if row.group.avg_payout + 1e-9 < previous {
                monotonic = false;
            }
            previous = row.group.avg_payout;
        }

        let (p50, p95) = self.percentiles();

        // A bought mode announces nothing: the result is paid for, not
        // discovered, and its reels are anticipated from the first one (§18.5
        // measures exactly 100 % on the buys and on the 500x modes). Against
        // the base-game rule that reads as "too early" on nearly every spin —
        // a number that would bury the modes where the rule does apply.
        //
        // Two shapes qualify, and both are read off the scan itself rather
        // than declared: a bonus that always opens, or an anticipation that
        // always fires.
        let bonus_always = events
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case("freeSpinTrigger"))
            .map(|e| e.group.probability >= 0.999)
            .unwrap_or(false);
        let systematic_anticipation = bonus_always || p(self.antic_emitted.weight) >= 0.99;

        let mut warnings = Vec::new();
        if self.antic_missing.weight > 0.0 && !systematic_anticipation {
            warnings.push(format!(
                "anticipation missing on {:.3} % of spins that could have completed the trigger \
                 (1 in {:.0}) — the reel that can finish the bonus falls like any other",
                p(self.antic_missing.weight) * 100.0,
                one_in(self.antic_missing.weight)
            ));
        }
        if self.antic_early.weight > 0.0 && !systematic_anticipation {
            warnings.push(format!(
                "anticipation starts too early on {:.3} % of spins — it announces before a reel \
                 can complete the trigger",
                p(self.antic_early.weight) * 100.0
            ));
        }
        if self.antic_spurious.weight > 0.0 && !systematic_anticipation {
            warnings.push(format!(
                "anticipation on {:.3} % of spins that cannot trigger at all",
                p(self.antic_spurious.weight) * 100.0
            ));
        }
        if self.tier_board_mismatch.weight > 0.0 {
            warnings.push(format!(
                "bonusSetup grants a tier the board does not show on {:.3} % of spins",
                p(self.tier_board_mismatch.weight) * 100.0
            ));
        }
        if self.tier_criteria_mismatch.weight > 0.0 {
            warnings.push(format!(
                "the criteria promises a tier bonusSetup does not grant on {:.3} % of spins",
                p(self.tier_criteria_mismatch.weight) * 100.0
            ));
        }
        if self.wild_moves_offgrid.weight > 0.0 {
            warnings.push(format!(
                "a wild moves outside the visible grid on {:.3} % of spins",
                p(self.wild_moves_offgrid.weight) * 100.0
            ));
        }
        if self.reveals_without_board.weight > 0.0 {
            warnings.push(
                "some reveals carry no board (COMPRESSION=True): the boards cannot be read from \
                 the books, only the reels and winInfo can"
                    .to_string(),
            );
        }
        if !monotonic {
            warnings.push(
                "bonus tiers are NOT monotonic: more scatters buys a worse bonus on average — \
                 the studio rule is that a higher tier is never worth less"
                    .to_string(),
            );
        }
        if self.over_cap.weight > 0.0 {
            warnings.push(format!(
                "{:.3} % of base-game boards show more scatters than the declared ceiling",
                p(self.over_cap.weight) * 100.0
            ));
        }
        if self.unweighted_books > 0 {
            warnings.push(format!(
                "{} book(s) carry no LUT weight — they ship and are never played",
                self.unweighted_books
            ));
        }
        if self.parse_errors > 0 {
            warnings.push(format!("{} book(s) could not be parsed", self.parse_errors));
        }

        ScanReport {
            mode: mode.to_string(),
            cost,
            shape,
            params,
            books: self.books,
            total_weight: t,
            events,
            anticipation: Anticipation {
                emitted: p(self.antic_emitted.weight),
                emitted_one_in: one_in(self.antic_emitted.weight),
                deserved: p(self.antic_deserved.weight),
                deserved_one_in: one_in(self.antic_deserved.weight),
                missing: p(self.antic_missing.weight),
                early: p(self.antic_early.weight),
                late: p(self.antic_late.weight),
                spurious: p(self.antic_spurious.weight),
                by_start_reel: self
                    .antic_start_reel
                    .iter()
                    .map(|(k, v)| Bucketed {
                        value: *k,
                        group: Group {
                            books: 0,
                            weight: *v,
                            probability: p(*v),
                            one_in: one_in(*v),
                            avg_payout: 0.0,
                        },
                    })
                    .collect(),
            },
            scatters_base: dist(&self.scatters_base),
            scatters_free: dist(&self.scatters_free),
            bonus_tiers: tiers,
            tiers_monotonic: monotonic,
            systematic_anticipation,
            integrity: Integrity {
                tier_board_mismatch: p(self.tier_board_mismatch.weight),
                tier_criteria_mismatch: p(self.tier_criteria_mismatch.weight),
                wild_moves_offgrid: p(self.wild_moves_offgrid.weight),
                reveals_without_board: p(self.reveals_without_board.weight),
                over_cap: p(self.over_cap.weight),
                unweighted_books: self.unweighted_books,
                parse_errors: self.parse_errors,
            },
            retriggers: dist(&self.retriggers),
            wild_moves_per_spin: if t > 0.0 { self.wild_move_count / t } else { 0.0 },
            pacing: Pacing {
                mean: if t > 0.0 { self.duration_wsum / t } else { 0.0 },
                p50,
                p95,
            },
            warnings,
        }
    }

    fn percentiles(&self) -> (f64, f64) {
        let t: f64 = self.duration_buckets.values().sum();
        if t <= 0.0 {
            return (0.0, 0.0);
        }
        let mut acc = 0.0;
        let (mut p50, mut p95) = (0.0, 0.0);
        for (bucket, w) in &self.duration_buckets {
            let before = acc;
            acc += w;
            let seconds = *bucket as f64 * BUCKET_S;
            if before < t * 0.5 && acc >= t * 0.5 {
                p50 = seconds;
            }
            if before < t * 0.95 && acc >= t * 0.95 {
                p95 = seconds;
            }
        }
        (p50, p95)
    }
}

fn merge_map(dst: &mut BTreeMap<String, RawGroup>, src: &BTreeMap<String, RawGroup>) {
    for (k, v) in src {
        let e = dst.entry(k.clone()).or_default();
        e.books += v.books;
        e.weight += v.weight;
        e.wp += v.wp;
    }
}

fn merge_map_i(dst: &mut BTreeMap<i64, RawGroup>, src: &BTreeMap<i64, RawGroup>) {
    for (k, v) in src {
        let e = dst.entry(*k).or_default();
        e.books += v.books;
        e.weight += v.weight;
        e.wp += v.wp;
    }
}

/// `freegame_4` → 4. The math-sdk writes the tier a criteria promises as the
/// number at the end of its name.
pub fn trailing_number(s: &str) -> Option<i64> {
    let digits: String = s
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if digits.is_empty() || digits.len() == s.len() {
        return None;
    }
    digits.parse().ok()
}

// ─── Report ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventStat {
    pub name: String,
    #[serde(flatten)]
    pub group: Group,
    /// Average occurrences per spin (weighted).
    pub per_spin: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bucketed {
    pub value: i64,
    #[serde(flatten)]
    pub group: Group,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Anticipation {
    pub emitted: f64,
    pub emitted_one_in: f64,
    pub deserved: f64,
    pub deserved_one_in: f64,
    pub missing: f64,
    pub early: f64,
    pub late: f64,
    pub spurious: f64,
    pub by_start_reel: Vec<Bucketed>,
}

/// The invariants that are either respected or not — as a share of spins,
/// so a rare book breaking one is not read as a rare problem.
#[derive(Debug, Clone, Copy, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Integrity {
    pub tier_board_mismatch: f64,
    pub tier_criteria_mismatch: f64,
    pub wild_moves_offgrid: f64,
    pub reveals_without_board: f64,
    pub over_cap: f64,
    pub unweighted_books: u64,
    pub parse_errors: u64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pacing {
    /// Seconds of staging in an average round, celebrations excluded.
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub mode: String,
    pub cost: f64,
    pub shape: Shape,
    pub params: ScanParams,
    pub books: u64,
    pub total_weight: f64,
    pub events: Vec<EventStat>,
    pub anticipation: Anticipation,
    pub scatters_base: Vec<Bucketed>,
    pub scatters_free: Vec<Bucketed>,
    pub bonus_tiers: Vec<Bucketed>,
    pub tiers_monotonic: bool,
    /// This mode anticipates on (almost) every spin — either because the
    /// bonus always opens or because the staging is systematic. Its
    /// anticipation columns announce a result that is already paid for, so the
    /// base-game rule does not apply and the early/missing counters are
    /// context rather than defects.
    pub systematic_anticipation: bool,
    pub integrity: Integrity,
    /// Retriggers par tour (`slotlab_core::freespins`), pondérés par la LUT.
    pub retriggers: Vec<Bucketed>,
    pub wild_moves_per_spin: f64,
    pub pacing: Pacing,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 6 reels × 4 rows with one row of padding at each end (visible 2..? ) —
    /// kept small so the expected numbers can be counted by hand.
    fn board(scatters_per_reel: &[usize]) -> String {
        let cols: Vec<String> = scatters_per_reel
            .iter()
            .map(|n| {
                // pad, then 2 visible cells, then pad
                let mut cells = vec![r#"{"name":"L1"}"#.to_string()];
                for i in 0..2 {
                    cells.push(if i < *n {
                        r#"{"name":"S","scatter":true}"#.to_string()
                    } else {
                        r#"{"name":"L1"}"#.to_string()
                    });
                }
                cells.push(r#"{"name":"L1"}"#.to_string());
                format!("[{}]", cells.join(","))
            })
            .collect();
        format!("[{}]", cols.join(","))
    }

    fn book(events: &str) -> String {
        format!(r#"{{"id":1,"payoutMultiplier":100,"events":[{events}]}}"#)
    }

    fn shape() -> Shape {
        Shape {
            trigger_scatters: 3,
            retrigger_scatters: 3,
            visible_rows: 2,
            board_rows: 4,
            reels: 3,
            pad: 1,
            last_scatter_reel: None,
            max_base_scatters: None,
            derived: false,
        }
    }

    /// BLOOD WATER's frame: 6 reels × 4 visible rows (+1 padding each end), a
    /// 2-scatter trigger (so a single scatter is the threshold), scatters only on
    /// reels 1-4. `scatters` are (reel, visible row), row 0 at the top.
    fn bw_board(scatters: &[(usize, usize)]) -> String {
        let cols: Vec<String> = (0..6)
            .map(|reel| {
                let mut cells = vec![r#"{"name":"L1"}"#.to_string()];
                for row in 0..4 {
                    cells.push(if scatters.contains(&(reel, row)) {
                        r#"{"name":"W","scatter":true,"wild":true}"#.to_string()
                    } else {
                        r#"{"name":"L1"}"#.to_string()
                    });
                }
                cells.push(r#"{"name":"L1"}"#.to_string());
                format!("[{}]", cells.join(","))
            })
            .collect();
        format!("[{}]", cols.join(","))
    }

    fn bw_shape() -> Shape {
        Shape {
            trigger_scatters: 2,
            retrigger_scatters: 0,
            visible_rows: 4,
            board_rows: 6,
            reels: 6,
            pad: 1,
            last_scatter_reel: Some(4),
            max_base_scatters: Some(4),
            derived: true,
        }
    }

    fn bw_scan(board: &[(usize, usize)], anticipation: &str) -> ScanReport {
        let line = book(&format!(
            r#"{{"index":0,"type":"reveal","gameType":"basegame","board":{},"anticipation":{anticipation}}}"#,
            bw_board(board)
        ));
        let mut acc = Accumulator::default();
        acc.push(&line, 5, &bw_shape(), &ScanParams::default());
        acc.finish("BASE", 1.0, bw_shape(), ScanParams::default())
    }

    #[test]
    fn detect_shape_finds_the_last_scatter_reel() {
        let line = book(&format!(
            r#"{{"index":0,"type":"reveal","gameType":"basegame","board":{}}}"#,
            bw_board(&[(1, 2), (4, 3)])
        ));
        assert_eq!(detect_shape(&[line]).last_scatter_reel, Some(4));
    }

    #[test]
    fn intra_column_on_the_last_scatter_reel_passes() {
        // Threshold scatter on reel 4 (the last that can hold one), row 2: only
        // the rows above it can still complete the trigger.
        let r = bw_scan(&[(4, 2)], "[0,0,0,0,1,0]");
        assert_eq!(r.anticipation.early, 0.0);
        assert_eq!(r.anticipation.missing, 0.0);
        assert_eq!(r.anticipation.spurious, 0.0);
    }

    #[test]
    fn top_row_of_the_last_scatter_reel_needs_nothing() {
        // Nothing falls after a scatter on the top row of reel 4, and reel 5 can
        // hold none: no announcement is owed.
        let r = bw_scan(&[(4, 0)], "[0,0,0,0,0,0]");
        assert_eq!(r.anticipation.missing, 0.0);
        let announced = bw_scan(&[(4, 0)], "[0,0,0,0,0,1]");
        assert!(announced.anticipation.spurious > 0.0);
    }

    #[test]
    fn missing_intra_column_anticipation_is_caught() {
        let r = bw_scan(&[(4, 3)], "[0,0,0,0,0,0]");
        assert!((r.anticipation.missing - 1.0).abs() < 1e-12);
    }

    #[test]
    fn nothing_is_owed_once_the_board_is_full() {
        // HIDDEN: a 4-shark trigger, and 4 is the cap. The 4th shark stacked on the
        // 3rd completes the bonus AND the cap in reel 3: no reel can bring more.
        let hidden = Shape { trigger_scatters: 4, ..bw_shape() };
        let scan = |board: &[(usize, usize)], anticipation: &str| {
            let line = book(&format!(
                r#"{{"index":0,"type":"reveal","gameType":"basegame","board":{},"anticipation":{anticipation}}}"#,
                bw_board(board)
            ));
            let mut acc = Accumulator::default();
            acc.push(&line, 5, &hidden, &ScanParams::default());
            acc.finish("HIDDEN", 500.0, hidden, ScanParams::default())
        };
        let full = scan(&[(1, 3), (2, 3), (3, 3), (3, 1)], "[0,0,0,0,0,0]");
        assert_eq!(full.anticipation.missing, 0.0);
        // The 3rd shark on the last scatter reel, the 4th above it: the rows above
        // the 3rd are announced, and the 4th lands in them.
        let intra = scan(&[(1, 1), (3, 1), (4, 3), (4, 2)], "[0,0,0,0,1,0]");
        assert_eq!(intra.anticipation.spurious, 0.0);
        assert_eq!(intra.anticipation.early, 0.0);
        // Three sharks by reel 3, the 4th still to come: reel 4 is owed.
        let open = scan(&[(1, 3), (2, 3), (3, 3)], "[0,0,0,0,0,0]");
        assert!((open.anticipation.missing - 1.0).abs() < 1e-12);
    }

    #[test]
    fn shark_count_reads_as_the_bonus_tier() {
        let line = format!(
            r#"{{"id":1,"payoutMultiplier":100,"criteria":"freegame_2","events":[{},{}]}}"#,
            format!(
                r#"{{"index":0,"type":"reveal","gameType":"basegame","board":{},"anticipation":[0,0,1,2,3,0]}}"#,
                bw_board(&[(1, 3), (2, 1)])
            ),
            r#"{"index":1,"type":"bonusSetup","sharkCount":2}"#
        );
        let mut acc = Accumulator::default();
        acc.push(&line, 5, &bw_shape(), &ScanParams::default());
        let r = acc.finish("BASE", 1.0, bw_shape(), ScanParams::default());
        assert_eq!(r.integrity.tier_board_mismatch, 0.0);
        assert_eq!(r.integrity.tier_criteria_mismatch, 0.0);
    }

    #[test]
    fn counts_scatters_on_the_visible_window_only() {
        // Padding rows are never scatters here, so a column with 2 visible
        // scatters must count 2, not 4.
        let line = book(&format!(
            r#"{{"index":0,"type":"reveal","gameType":"basegame","board":{}}}"#,
            board(&[2, 1, 0])
        ));
        let mut acc = Accumulator::default();
        acc.push(&line, 10, &shape(), &ScanParams::default());
        let r = acc.finish("BASE", 1.0, shape(), ScanParams::default());
        assert_eq!(r.scatters_base.len(), 1);
        assert_eq!(r.scatters_base[0].value, 3);
    }

    #[test]
    fn missing_anticipation_is_caught() {
        // Reels 0 and 1 carry 2 scatters: reel 2 can complete a 3-scatter
        // trigger, and nothing announces it.
        let line = book(&format!(
            r#"{{"index":0,"type":"reveal","gameType":"basegame","board":{},"anticipation":[0,0,0]}}"#,
            board(&[1, 1, 0])
        ));
        let mut acc = Accumulator::default();
        acc.push(&line, 5, &shape(), &ScanParams::default());
        let r = acc.finish("BASE", 1.0, shape(), ScanParams::default());
        assert!((r.anticipation.missing - 1.0).abs() < 1e-12);
        assert!(r.warnings.iter().any(|w| w.contains("anticipation missing")));
    }

    #[test]
    fn anticipation_on_the_right_reel_passes() {
        let line = book(&format!(
            r#"{{"index":0,"type":"reveal","gameType":"basegame","board":{},"anticipation":[0,0,1]}}"#,
            board(&[1, 1, 0])
        ));
        let mut acc = Accumulator::default();
        acc.push(&line, 5, &shape(), &ScanParams::default());
        let r = acc.finish("BASE", 1.0, shape(), ScanParams::default());
        assert_eq!(r.anticipation.missing, 0.0);
        assert_eq!(r.anticipation.early, 0.0);
        assert_eq!(r.anticipation.late, 0.0);
        assert!((r.anticipation.emitted - 1.0).abs() < 1e-12);
    }

    #[test]
    fn early_anticipation_is_caught() {
        // Only one scatter on reel 0: announcing from reel 1 is too early.
        let line = book(&format!(
            r#"{{"index":0,"type":"reveal","gameType":"basegame","board":{},"anticipation":[0,1,2]}}"#,
            board(&[1, 0, 0])
        ));
        let mut acc = Accumulator::default();
        acc.push(&line, 1, &shape(), &ScanParams::default());
        let r = acc.finish("BASE", 1.0, shape(), ScanParams::default());
        assert!(r.anticipation.spurious > 0.0 || r.anticipation.early > 0.0);
    }

    #[test]
    fn weights_decide_the_rates() {
        // Two books: the one with the flaw carries a thousandth of the weight.
        let clean = book(&format!(
            r#"{{"index":0,"type":"reveal","gameType":"basegame","board":{},"anticipation":[0,0,1]}}"#,
            board(&[1, 1, 0])
        ));
        let broken = book(&format!(
            r#"{{"index":0,"type":"reveal","gameType":"basegame","board":{},"anticipation":[0,0,0]}}"#,
            board(&[1, 1, 0])
        ));
        let mut acc = Accumulator::default();
        acc.push(&clean, 999, &shape(), &ScanParams::default());
        acc.push(&broken, 1, &shape(), &ScanParams::default());
        let r = acc.finish("BASE", 1.0, shape(), ScanParams::default());
        assert!((r.anticipation.missing - 0.001).abs() < 1e-12, "{}", r.anticipation.missing);
    }

    #[test]
    fn tier_mismatch_and_monotonicity() {
        let with_tier = |payout: f64, sc: i64, crit: &str| {
            format!(
                r#"{{"id":1,"payoutMultiplier":{payout},"criteria":"{crit}","events":[
                    {{"index":0,"type":"reveal","gameType":"basegame","board":{}}},
                    {{"index":1,"type":"bonusSetup","scatterCount":{sc}}}]}}"#,
                board(&[1, 1, 1])
            )
        };
        let mut acc = Accumulator::default();
        // Board shows 3 scatters; the setup grants 4 → mismatch, and the
        // criteria promises 4 → agrees with the setup.
        acc.push(&with_tier(100.0, 4, "freegame_4"), 1, &shape(), &ScanParams::default());
        let r = acc.finish("BASE", 1.0, shape(), ScanParams::default());
        assert!(r.integrity.tier_board_mismatch > 0.0);
    }

    #[test]
    fn shape_is_derived_from_the_events() {
        let line = format!(
            r#"{{"id":1,"payoutMultiplier":0,"events":[
                {{"index":0,"type":"reveal","board":{}}},
                {{"index":1,"type":"freeSpinTrigger","totalFs":10,
                  "positions":[{{"reel":0,"row":1}},{{"reel":1,"row":2}},{{"reel":2,"row":1}}]}}]}}"#,
            board(&[1, 1, 1])
        );
        // Rows 1..2 addressed on a 4-row column: one padding row at each end,
        // exactly what `include_padding = True` ships.
        let s = detect_shape(&[line]);
        assert_eq!(s.trigger_scatters, 3);
        assert_eq!(s.board_rows, 4);
        assert_eq!(s.pad, 1);
        assert_eq!(s.visible_rows, 2);
    }

    #[test]
    fn retriggers_follow_the_generic_rule() {
        // SDK : un `freeSpinRetrigger`, répété par `updateFreeSpin` → 1.
        let sdk = book(
            r#"{"type":"freeSpinTrigger","totalFs":10},
               {"type":"updateFreeSpin","amount":1,"total":10},
               {"type":"freeSpinRetrigger","totalFs":15},
               {"type":"updateFreeSpin","amount":2,"total":15},
               {"type":"freeSpinEnd","amount":0}"#,
        );
        // BLOOD WATER : trois repas qui montent le total, un au plafond → 3.
        let bloodwater = book(
            r#"{"type":"freeSpinTrigger","totalFs":6},
               {"type":"sharkFeed","hunts":[{"fish":"F","totalFs":7},{"fish":"F","totalFs":8}]},
               {"type":"updateFreeSpin","amount":2,"total":8},
               {"type":"sharkFeed","hunts":[{"fish":"M","totalFs":9}]},
               {"type":"sharkFeed","hunts":[{"fish":"F","totalFs":9}]},
               {"type":"freeSpinEnd","amount":0}"#,
        );
        let no_bonus = book(&format!(
            r#"{{"index":0,"type":"reveal","gameType":"basegame","board":{}}}"#,
            board(&[0, 0, 0])
        ));
        let mut acc = Accumulator::default();
        acc.push(&sdk, 3, &shape(), &ScanParams::default());
        acc.push(&bloodwater, 1, &shape(), &ScanParams::default());
        acc.push(&no_bonus, 6, &shape(), &ScanParams::default());
        let r = acc.finish("BASE", 1.0, shape(), ScanParams::default());
        let by: Vec<(i64, f64)> = r
            .retriggers
            .iter()
            .map(|b| (b.value, b.group.weight))
            .collect();
        assert_eq!(by, [(0, 6.0), (1, 3.0), (3, 1.0)]);
    }

    #[test]
    fn hand_written_event_reads_every_field() {
        // Garde-fou du désérialiseur écrit à la main : un champ oublié dans
        // `EvKey` resterait à `None` sans erreur.
        let ev: Ev = serde_json::from_str(
            r#"{"index":3,"type":"reveal","gameType":"freegame",
                "board":[[{"name":"S","scatter":true},{"name":"W","wild":true}]],
                "anticipation":[0,1],"anticipationRow":2,"scatterCount":4,
                "positions":[{"reel":1,"row":2}],"moves":[{"to":{"reel":0,"row":1}}],
                "wins":[{"win":5}],"explodingSymbols":[{"reel":2,"row":3}],
                "paddingPositions":[1,2],"totalFs":9,"hunts":[{"fish":"F","totalFs":10}]}"#,
        )
        .unwrap();
        assert_eq!(ev.typ, "reveal");
        assert_eq!(ev.game_type, Some("freegame"));
        let board = ev.board.as_ref().unwrap();
        assert!(board[0][0].scatter && board[0][1].wild);
        assert_eq!(ev.anticipation, Some(vec![0, 1]));
        assert_eq!(ev.anticipation_row, Some(2));
        assert_eq!(ev.scatter_count, Some(4));
        assert_eq!(ev.positions.as_ref().unwrap()[0].row, 2);
        assert_eq!(ev.moves.as_ref().unwrap()[0].to.unwrap().reel, 0);
        assert_eq!(ev.wins.as_ref().unwrap().len(), 1);
        assert_eq!(ev.exploding_symbols.as_ref().unwrap()[0].row, 3);
        assert_eq!(ev.fs.total_fs, Some(9.0));
        assert_eq!(ev.fs.nested, vec![10.0]);
    }

    #[test]
    fn trailing_numbers() {
        assert_eq!(trailing_number("freegame_4"), Some(4));
        assert_eq!(trailing_number("basegame"), None);
        assert_eq!(trailing_number("0"), None);
    }
}
