//! Celebration tiers measured on the PUBLISHED lookup table, mode by mode.
//!
//! ── Why this cannot be eyeballed ────────────────────────────────────────────
//! A win level is a screen decision: a plaque, a piece of music, and six to
//! thirteen seconds during which the player cannot spin. The math knows none of
//! that — it only ever speaks in multiples of the BASE bet, and it ignores what
//! the spin cost. So a single ladder lies the moment a mode costs more than 1x:
//! on a 500x buy, a 100x win used to earn the top plaque and its 11.5 seconds
//! for a 0.2x return on the money actually staked.
//!
//! The frequency of a tier is therefore a property of the LUT, read per mode:
//!
//! ```text
//! awk -F, -v t=1000 '{w=$2;W+=w; if($3>=t*100) k+=w} END{print 100*k/W}' \
//!   lookUpTable_DEGEN_0.csv
//! ```
//!
//! This module is that awk, for every threshold and every mode at once, plus
//! the thing the awk cannot give: the SECONDS the ladder costs per spin
//! (Σ probability × presentDuration). A BIG WIN more often than one spin in ten
//! is a rhythm choice — lowering a threshold does not make a game more
//! generous, it makes the mode slower.
//!
//! ⚠️ Thresholds are in multiples of the BASE BET, never of the spin's cost:
//! `payout_x100` (LUT column 3) is already in hundredths of the base bet, which
//! is what makes the comparison a plain integer one.

use crate::model::Lut;
use serde::{Deserialize, Serialize};

/// Ladder of a mode: the low bound of each tier and how long the screen holds
/// it. Both vectors are the same length, tier 1 first (the "nothing happens"
/// tier, threshold 0).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TierSpec {
    pub names: Vec<String>,
    /// Low bound of each tier, in multiples of the base bet.
    pub thresholds: Vec<f64>,
    /// How long the celebration holds the screen, in seconds.
    pub durations: Vec<f64>,
    /// Index of the first tier that shows a plaque (0-based). Tiers below it
    /// are a pause, not a screen.
    pub first_plaque: usize,
}

/// The studio default, straight out of `winLevelThresholds.ts` /
/// `winLevelMap.ts`: ten tiers, the last one being the game's max win.
pub const DEFAULT_NAMES: [&str; 10] = [
    "zero",
    "standard",
    "small",
    "nice",
    "substantial",
    "BIG WIN",
    "SUPER WIN",
    "MEGA WIN",
    "TOP WIN",
    "MAX WIN",
];
pub const DEFAULT_STEPS: [f64; 8] = [0.1, 1.0, 2.0, 5.0, 25.0, 50.0, 100.0, 500.0];
pub const DEFAULT_DURATIONS: [f64; 10] =
    [0.0, 0.4, 0.6, 0.7, 0.9, 6.5, 8.5, 10.0, 11.5, 13.0];
/// Tiers 6..10 (1-based) are the ones with a plaque.
pub const DEFAULT_FIRST_PLAQUE: usize = 5;

impl TierSpec {
    /// Ladder built from the eight steps of a `winLevelThresholds.ts` row plus
    /// the game's max win, which is a fact of the game and never scaled.
    pub fn from_steps(steps: &[f64], max_win: f64) -> Self {
        let mut thresholds = Vec::with_capacity(steps.len() + 2);
        thresholds.push(0.0);
        thresholds.extend_from_slice(steps);
        thresholds.push(max_win);
        let n = thresholds.len();
        let names: Vec<String> = (0..n)
            .map(|i| {
                DEFAULT_NAMES
                    .get(i)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("tier {}", i + 1))
            })
            .collect();
        let durations: Vec<f64> = (0..n)
            .map(|i| DEFAULT_DURATIONS.get(i).copied().unwrap_or(0.0))
            .collect();
        Self {
            names,
            thresholds,
            durations,
            first_plaque: DEFAULT_FIRST_PLAQUE.min(n.saturating_sub(1)),
        }
    }

    pub fn default_for(max_win: f64) -> Self {
        Self::from_steps(&DEFAULT_STEPS, max_win)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierRow {
    /// 1-based, as the frontend numbers its win levels.
    pub level: usize,
    pub name: String,
    /// Low bound, in multiples of the base bet.
    pub threshold: f64,
    /// P(payout ≥ threshold) — the number the awk one-liner prints.
    pub at_least: f64,
    pub at_least_one_in: f64,
    /// P(this exact tier) = P(≥ this) − P(≥ next).
    pub exactly: f64,
    pub exactly_one_in: f64,
    /// Seconds this tier alone adds to the average spin.
    pub seconds_per_spin: f64,
    pub duration: f64,
    /// Does this tier put a plaque on screen?
    pub plaque: bool,
    /// Share of the mode's RTP paid by rounds landing in this tier.
    pub rtp_contribution: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeTiers {
    pub mode: String,
    pub cost: f64,
    /// Where the ladder came from: the game's own file, or the defaults.
    pub source: String,
    pub rows: Vec<TierRow>,
    /// P(any plaque at all).
    pub plaque_rate: f64,
    pub plaque_one_in: f64,
    /// Σ p·duration over every tier — what the ladder costs the average spin.
    pub seconds_per_spin: f64,
    /// Same, counting only tiers that stop the game with a plaque.
    pub plaque_seconds_per_spin: f64,
    pub warnings: Vec<String>,
}

/// One spin in this many is the point where a plaque stops being an event and
/// becomes the rhythm of the mode (§16.6 of the studio doc).
const PLAQUE_FREQUENCY_FLOOR: f64 = 10.0;

pub fn analyze(lut: &Lut, mode: &str, cost: f64, spec: &TierSpec, source: &str) -> ModeTiers {
    let t = lut.total_weight as f64;
    let cost = if cost > 0.0 { cost } else { 1.0 };
    let n = spec.thresholds.len();

    // Weight and Σ w·p per tier, walking the rows once.
    let mut weight = vec![0u64; n];
    let mut wp = vec![0.0f64; n];
    let bounds_x100: Vec<u64> = spec
        .thresholds
        .iter()
        .map(|v| (v * 100.0).round().max(0.0) as u64)
        .collect();
    for r in &lut.rows {
        // Highest tier whose low bound the payout reaches.
        let mut level = 0usize;
        for (i, &b) in bounds_x100.iter().enumerate() {
            // Tier 1 (bound 0) always matches; a zero payout stays in it.
            if r.payout_x100 >= b && (i == 0 || b > 0) {
                level = i;
            }
        }
        weight[level] += r.weight;
        wp[level] += r.weight as f64 * (r.payout_x100 as f64 / 100.0);
    }

    // Cumulative from the top: P(≥ threshold).
    let mut cum = vec![0u64; n];
    let mut acc = 0u64;
    for i in (0..n).rev() {
        acc += weight[i];
        cum[i] = acc;
    }

    let one_in = |w: u64| if w > 0 { t / w as f64 } else { f64::INFINITY };
    let rows: Vec<TierRow> = (0..n)
        .map(|i| {
            let p_exact = weight[i] as f64 / t;
            let plaque = i >= spec.first_plaque;
            let duration = spec.durations.get(i).copied().unwrap_or(0.0);
            TierRow {
                level: i + 1,
                name: spec.names.get(i).cloned().unwrap_or_default(),
                threshold: spec.thresholds[i],
                at_least: cum[i] as f64 / t,
                at_least_one_in: one_in(cum[i]),
                exactly: p_exact,
                exactly_one_in: one_in(weight[i]),
                seconds_per_spin: p_exact * duration,
                duration,
                plaque,
                rtp_contribution: wp[i] / t / cost,
            }
        })
        .collect();

    let plaque_weight: u64 = (spec.first_plaque..n).map(|i| weight[i]).sum();
    let plaque_rate = plaque_weight as f64 / t;
    let seconds_per_spin: f64 = rows.iter().map(|r| r.seconds_per_spin).sum();
    let plaque_seconds_per_spin: f64 = rows
        .iter()
        .filter(|r| r.plaque)
        .map(|r| r.seconds_per_spin)
        .sum();

    let mut warnings = Vec::new();
    for r in rows.iter().filter(|r| r.plaque) {
        if r.at_least_one_in.is_finite() && r.at_least_one_in <= PLAQUE_FREQUENCY_FLOOR {
            warnings.push(format!(
                "{} lands 1 spin in {:.1} — that is the rhythm of the mode, not an event \
                 (raise the threshold to slow the plaque down)",
                r.name, r.at_least_one_in
            ));
        }
    }
    // A tier nobody ever reaches is a dead branch of the frontend: its plaque,
    // its music and its timings are shipped and never played.
    for r in rows.iter().filter(|r| r.plaque) {
        if r.at_least <= 0.0 {
            warnings.push(format!(
                "{} is unreachable on this mode (max win {:.0}x < threshold {:.0}x)",
                r.name,
                lut.rows.iter().map(|r| r.payout_x100).max().unwrap_or(0) as f64 / 100.0,
                r.threshold
            ));
        }
    }
    if plaque_rate > 0.0 && plaque_seconds_per_spin > 3.0 {
        warnings.push(format!(
            "celebrations alone add {plaque_seconds_per_spin:.1} s to the average spin"
        ));
    }

    ModeTiers {
        mode: mode.to_string(),
        cost,
        source: source.to_string(),
        rows,
        plaque_rate,
        plaque_one_in: one_in(plaque_weight),
        seconds_per_spin,
        plaque_seconds_per_spin,
        warnings,
    }
}

// ─── Reading the game's own ladder ──────────────────────────────────────────

/// The ladders declared by a game's `winLevelThresholds.ts`, by mode name
/// (upper-case), plus the `DEFAULT` row every unlisted mode falls back to.
///
/// Deliberately a text scan rather than a TS parse: the file is a table of
/// literals by design ("the only file to edit"), and a scan degrades to None
/// instead of failing when a game writes it differently.
pub fn parse_basegame_steps(src: &str) -> Vec<(String, Vec<f64>)> {
    let Some(block) = slice_object(src, "BASEGAME_STEPS") else {
        return Vec::new();
    };
    // Comments first: the table ships a column header above the rows, and its
    // words would otherwise be read as part of the key that follows.
    let block = strip_comments(&block);
    let bytes = block.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(open) = block[i..].find('[').map(|p| i + p) {
        let Some(close) = block[open..].find(']').map(|p| open + p) else {
            break;
        };
        // `KEY:` immediately before the bracket — anything else (a call, a
        // spread) is not a ladder row.
        let head = block[i..open].trim_end();
        if let Some(colon) = head.rfind(':') {
            let key: String = head[..colon]
                .chars()
                .rev()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let between = head[colon + 1..].trim();
            if !key.is_empty() && between.is_empty() {
                let nums = parse_numbers(&block[open + 1..close]);
                if nums.len() >= 4 {
                    out.push((key.to_uppercase(), nums));
                }
            }
        }
        i = close + 1;
        let _ = bytes;
    }
    out
}

/// Drops `//` and `/* */` comments — string literals have no place in a table
/// of numbers, so quoting is not tracked.
fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' {
            match chars.peek() {
                Some('/') => {
                    for c in chars.by_ref() {
                        if c == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                    continue;
                }
                Some('*') => {
                    chars.next();
                    let mut prev = ' ';
                    for c in chars.by_ref() {
                        if prev == '*' && c == '/' {
                            break;
                        }
                        prev = c;
                    }
                    out.push(' ');
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out
}

/// `const NAME = { … }` → the text between the braces.
fn slice_object(src: &str, name: &str) -> Option<String> {
    let at = src.find(name)?;
    let open = src[at..].find('{').map(|p| at + p)?;
    let bytes = src.as_bytes();
    let mut depth = 0i32;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(src[open + 1..i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Numbers of a TS literal list, `1_000` and `0.1` included, comments dropped.
fn parse_numbers(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
            }
            continue;
        }
        if in_block_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
            }
            continue;
        }
        if c == '/' {
            match chars.peek() {
                Some('/') => {
                    chars.next();
                    in_line_comment = true;
                    continue;
                }
                Some('*') => {
                    chars.next();
                    in_block_comment = true;
                    continue;
                }
                _ => {}
            }
        }
        if c.is_ascii_digit() || c == '.' {
            cur.push(c);
        } else if c == '_' && !cur.is_empty() {
            // 1_000
        } else {
            if !cur.is_empty() {
                if let Ok(v) = cur.parse::<f64>() {
                    out.push(v);
                }
                cur.clear();
            }
        }
    }
    if let Ok(v) = cur.parse::<f64>() {
        out.push(v);
    }
    out
}

/// `presentDuration: 6.5 * SECOND` → seconds, in declaration order.
pub fn parse_present_durations(src: &str) -> Vec<f64> {
    let mut out = Vec::new();
    for part in src.split("presentDuration").skip(1) {
        let Some(colon) = part.find(':') else { continue };
        let tail: String = part[colon + 1..].chars().take(40).collect();
        let mut num = String::new();
        for c in tail.chars() {
            if c.is_ascii_digit() || c == '.' {
                num.push(c);
            } else if c == '_' {
                continue;
            } else if !num.is_empty() {
                break;
            } else if c.is_whitespace() {
                continue;
            } else {
                break;
            }
        }
        if let Ok(v) = num.parse::<f64>() {
            // `0` means "no hold": written without a SECOND factor.
            out.push(v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LutRow;

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
    fn cumulative_matches_the_awk_one_liner() {
        // 90% zero, 9% at 30x, 1% at 2000x. Steps: BIG at 25x.
        let l = lut(&[(0, 900, 0), (1, 90, 3_000), (2, 10, 200_000)]);
        let spec = TierSpec::default_for(50_000.0);
        let t = analyze(&l, "BASE", 1.0, &spec, "test");
        let big = t.rows.iter().find(|r| r.name == "BIG WIN").unwrap();
        assert!((big.at_least - 0.1).abs() < 1e-12, "{}", big.at_least);
        let top = t.rows.iter().find(|r| r.name == "TOP WIN").unwrap();
        assert!((top.at_least - 0.01).abs() < 1e-12);
        assert!((t.plaque_rate - 0.1).abs() < 1e-12);
    }

    #[test]
    fn seconds_per_spin_is_the_weighted_ladder() {
        // 1 spin in 10 at 30x → BIG WIN (6.5 s) → 0.65 s per spin.
        let l = lut(&[(0, 900, 0), (1, 100, 3_000)]);
        let spec = TierSpec::default_for(50_000.0);
        let t = analyze(&l, "BASE", 1.0, &spec, "test");
        assert!((t.plaque_seconds_per_spin - 0.65).abs() < 1e-9);
        assert!(t.warnings.iter().any(|w| w.contains("BIG WIN")));
    }

    #[test]
    fn zero_payout_stays_in_tier_one() {
        let l = lut(&[(0, 1, 0)]);
        let spec = TierSpec::default_for(100.0);
        let t = analyze(&l, "BASE", 1.0, &spec, "test");
        assert_eq!(t.rows[0].level, 1);
        assert!((t.rows[0].exactly - 1.0).abs() < 1e-12);
        assert!((t.plaque_rate - 0.0).abs() < 1e-12);
    }

    #[test]
    fn reads_the_games_own_ladder() {
        let src = r#"
            const BASEGAME_STEPS = {
                //        standard  small  nice  subst.   BIG  SUPER   MEGA   DEGEN
                DEFAULT: [0.1, 1, 2, 5, 25, 50, 100, 500],
                BASE: [0.1, 1, 2, 5, 25, 50, 100, 500],
                DEGEN: [50, 250, 500, 750, 1_000, 2_500, 5_000, 10_000],
            } as const satisfies Record<string, WinLevelSteps>;
        "#;
        let rows = parse_basegame_steps(src);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, "DEFAULT");
        assert_eq!(rows[2].1, vec![50.0, 250.0, 500.0, 750.0, 1000.0, 2500.0, 5000.0, 10000.0]);
    }

    #[test]
    fn reads_present_durations() {
        let src = "presentDuration: 0,\n presentDuration: 6.5 * SECOND,\n presentDuration: 13 * SECOND,";
        assert_eq!(parse_present_durations(src), vec![0.0, 6.5, 13.0]);
    }
}
