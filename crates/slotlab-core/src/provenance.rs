//! D'où viennent les gains d'un mode — tout pondéré par la LUT publiée :
//!
//!   · par tranche de gain (la grille 1-2-5-10 de `stats`) : la part portée
//!     par les books qui déclenchent un bonus, et par chacun de ses paliers ;
//!   · par palier : fréquence, gain moyen, part du RTP, et les retriggers
//!     (moyenne, min, médiane, max, part des bonus qui n'en font aucun) ;
//!   · par FAMILLE (`crate::family` : palier pour un bonus, famille du criteria
//!     sinon) : fréquence, gain moyen, part du RTP, part de ses books qui
//!     déclenchent un bonus, retriggers moyens de ces bonus.
//!
//! ── Générique ───────────────────────────────────────────────────────────────
//! « Bonus » = au moins un `freeSpinTrigger` ; palier = nombre de `positions`
//! du premier (`freespins::BookFs::tier`) ; retriggers = règle de `freespins`.
//! Un `wincap` est rangé au palier de son vrai déclenchement (ou hors bonus
//! s'il n'en a pas) ; les noms de criteria ne sont lus que pour retirer leurs
//! suffixes de simulation (`crate::family`).
//!
//! ── Pondération ─────────────────────────────────────────────────────────────
//! Un book n'est pas un spin : chaque part est une part de POIDS de LUT. Le
//! min, la médiane et le max des retriggers ne regardent que les books que la
//! LUT peut tirer (poids > 0) — un book jamais joué ne dit rien du jeu.

use crate::stats::{Bucket, BucketLookup};
use serde::Serialize;
use std::collections::BTreeMap;

/// Un book tel que ce module le lit : sa ligne de LUT, et ce que l'index des
/// books sait de lui.
#[derive(Debug, Clone, Copy, Default)]
pub struct BookFacts {
    pub sim_id: u64,
    pub weight: u64,
    /// Gain de la ligne de LUT (×100, en mises de base).
    pub payout_x100: u64,
    /// Au moins un `freeSpinTrigger`.
    pub bonus: bool,
    /// Palier du bonus (0 : bonus dont le déclenchement ne dit pas ses positions).
    pub tier: u8,
    pub retriggers: u16,
    /// Index dans les criteria du mode ; `None` : book sans criteria.
    pub criteria: Option<usize>,
    /// Free spins attribués / joués au premier bonus (0 : inconnu).
    pub spins_initial: u16,
    pub spins_final: u16,
    /// Gain du bonus seul (×100) ; `None` : le book ne le dit pas.
    pub fg_win_x100: Option<u64>,
}

/// Une tranche de gain et ce qui la porte.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketSplit {
    /// Bornes de la tranche, identiques à celles de `stats::Bucket`.
    pub start: f64,
    pub end: f64,
    /// Part du poids de la tranche portée par des books à bonus.
    pub bonus_share: f64,
    /// Part du poids de la tranche portée par chaque palier (books à bonus
    /// seulement) ; leur somme vaut `bonus_share`.
    pub tiers: Vec<TierShare>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierShare {
    /// `None` : bonus sans palier connu.
    pub tier: Option<u8>,
    pub share: f64,
}

/// Retriggers des bonus d'un groupe, pondérés.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetriggerStats {
    pub mean: f64,
    pub min: u16,
    /// Médiane pondérée : la plus petite valeur où le poids cumulé (trié par
    /// retriggers) atteint la moitié.
    pub median: u16,
    pub max: u16,
    /// Part des bonus qui ne font aucun retrigger.
    pub none_share: f64,
}

/// Un palier de bonus.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierRow {
    /// Nombre de positions au déclenchement ; `None` : inconnu.
    pub tier: Option<u8>,
    /// Books du palier.
    pub count: usize,
    pub probability: f64,
    pub one_in: f64,
    /// Gain moyen du book entier (jeu de base compris), en mises de base.
    pub avg_win: f64,
    pub rtp_contribution: f64,
    /// Part du RTP du mode.
    pub rtp_share: f64,
    /// `None` si aucun book du palier n'a de poids.
    pub retriggers: Option<RetriggerStats>,
    /// Criteria des books du palier, du plus lourd au plus léger.
    pub criteria: Vec<String>,
}

/// Un criteria du mode.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CriteriaRow {
    /// `(none)` pour les books sans criteria.
    pub criteria: String,
    pub count: usize,
    pub probability: f64,
    pub one_in: f64,
    pub avg_win: f64,
    pub rtp_contribution: f64,
    pub rtp_share: f64,
    /// Part du poids de ses books qui déclenchent un bonus.
    pub bonus_share: f64,
    /// Ses books qui déclenchent (compte brut).
    pub books_with_trigger: usize,
    /// Retriggers moyens de ses bonus ; `None` s'il n'en déclenche aucun.
    pub retriggers_mean: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub buckets: Vec<BucketSplit>,
    pub tiers: Vec<TierRow>,
    pub criteria: Vec<CriteriaRow>,
}

#[derive(Default)]
struct TierAcc {
    count: usize,
    weight: u64,
    wp: f64,
    criteria: BTreeMap<String, u64>,
    /// (retriggers, poids) des books tirables.
    retriggers: Vec<(u16, u64)>,
}

#[derive(Default)]
struct CriteriaAcc {
    count: usize,
    weight: u64,
    wp: f64,
    bonus_count: usize,
    bonus_weight: u64,
    /// Σ poids × retriggers, sur ses books à bonus.
    retriggers_wsum: f64,
}

/// `buckets` : les tranches de `stats::analyze` sur la même LUT (c'est d'elles
/// que viennent les bornes). `books` : une entrée par ligne de LUT.
pub fn analyze(
    books: &[BookFacts],
    cost: f64,
    buckets: &[Bucket],
    criteria_names: &[String],
) -> Provenance {
    let cost = if cost > 0.0 { cost } else { 1.0 };
    let x = |b: &BookFacts| b.payout_x100 as f64 / 100.0;
    let t: f64 = books.iter().map(|b| b.weight as f64).sum();
    let total_wp: f64 = books.iter().map(|b| b.weight as f64 * x(b)).sum();
    let rtp = if t > 0.0 { total_wp / t / cost } else { 0.0 };

    // Familles (`crate::family`) : un book à bonus se range sous son palier, un spin sans
    // bonus sous la famille de son criteria — jamais par seau de simulation (2026-10-04).
    let crit_of = |b: &BookFacts| b.criteria.and_then(|i| criteria_names.get(i)).map(|n| n.as_str());
    let lookup = BucketLookup::new(buckets);
    let mut bucket_w = vec![0u64; buckets.len()];
    let mut bucket_bonus = vec![0u64; buckets.len()];
    let mut bucket_tiers: Vec<BTreeMap<u8, u64>> = vec![BTreeMap::new(); buckets.len()];
    let mut tiers: BTreeMap<u8, TierAcc> = BTreeMap::new();
    let mut crits: BTreeMap<String, CriteriaAcc> = BTreeMap::new();

    for b in books {
        let wp = b.weight as f64 * x(b);
        if let Some(i) = lookup.find(b.payout_x100) {
            bucket_w[i] += b.weight;
            if b.bonus {
                bucket_bonus[i] += b.weight;
                *bucket_tiers[i].entry(b.tier).or_default() += b.weight;
            }
        }
        if b.bonus {
            let e = tiers.entry(b.tier).or_default();
            e.count += 1;
            e.weight += b.weight;
            e.wp += wp;
            *e.criteria.entry(crate::family::criteria_family(crit_of(b).unwrap_or("(none)"))).or_default() += b.weight;
            if b.weight > 0 {
                e.retriggers.push((b.retriggers, b.weight));
            }
        }
        let c = crits.entry(crate::family::book_family(crit_of(b), b.bonus, b.tier)).or_default();
        c.count += 1;
        c.weight += b.weight;
        c.wp += wp;
        if b.bonus {
            c.bonus_count += 1;
            c.bonus_weight += b.weight;
            c.retriggers_wsum += b.weight as f64 * b.retriggers as f64;
        }
    }

    let ratio = |part: u64, whole: u64| {
        if whole > 0 {
            part as f64 / whole as f64
        } else {
            0.0
        }
    };
    let one_in = |w: u64| if w > 0 { t / w as f64 } else { f64::INFINITY };
    // Palier 0 (« inconnu ») en dernier : c'est le reste, pas un palier.
    let tier_key = |tier: u8| (tier == 0, tier);
    let tier_label = |tier: u8| (tier > 0).then_some(tier);

    let split = buckets
        .iter()
        .enumerate()
        .map(|(i, bk)| {
            let mut shares: Vec<(u8, u64)> = bucket_tiers[i].iter().map(|(&k, &w)| (k, w)).collect();
            shares.sort_by_key(|&(k, _)| tier_key(k));
            BucketSplit {
                start: bk.start,
                end: bk.end,
                bonus_share: ratio(bucket_bonus[i], bucket_w[i]),
                tiers: shares
                    .into_iter()
                    .map(|(k, w)| TierShare {
                        tier: tier_label(k),
                        share: ratio(w, bucket_w[i]),
                    })
                    .collect(),
            }
        })
        .collect();

    let mut tier_rows: Vec<(u8, TierRow)> = tiers
        .into_iter()
        .map(|(tier, acc)| {
            let contribution = if t > 0.0 { acc.wp / t / cost } else { 0.0 };
            let mut crit: Vec<(String, u64)> = acc.criteria.into_iter().collect();
            crit.sort_by(|a, b| b.1.cmp(&a.1));
            let row = TierRow {
                tier: tier_label(tier),
                count: acc.count,
                probability: if t > 0.0 { acc.weight as f64 / t } else { 0.0 },
                one_in: one_in(acc.weight),
                avg_win: if acc.weight > 0 {
                    acc.wp / acc.weight as f64
                } else {
                    0.0
                },
                rtp_contribution: contribution,
                rtp_share: if rtp > 0.0 { contribution / rtp } else { 0.0 },
                retriggers: retrigger_stats(acc.retriggers),
                criteria: crit.into_iter().map(|(c, _)| c).collect(),
            };
            (tier, row)
        })
        .collect();
    tier_rows.sort_by_key(|(tier, _)| tier_key(*tier));

    let mut criteria: Vec<CriteriaRow> = crits
        .into_iter()
        .map(|(c, acc)| {
            let contribution = if t > 0.0 { acc.wp / t / cost } else { 0.0 };
            CriteriaRow {
                criteria: c,
                count: acc.count,
                probability: if t > 0.0 { acc.weight as f64 / t } else { 0.0 },
                one_in: one_in(acc.weight),
                avg_win: if acc.weight > 0 {
                    acc.wp / acc.weight as f64
                } else {
                    0.0
                },
                rtp_contribution: contribution,
                rtp_share: if rtp > 0.0 { contribution / rtp } else { 0.0 },
                bonus_share: ratio(acc.bonus_weight, acc.weight),
                books_with_trigger: acc.bonus_count,
                retriggers_mean: (acc.bonus_weight > 0)
                    .then(|| acc.retriggers_wsum / acc.bonus_weight as f64),
            }
        })
        .collect();
    // Le plus probable d'abord : se lit comme « ce qu'on obtient d'habitude ».
    criteria.sort_by(|a, b| b.probability.total_cmp(&a.probability));

    Provenance {
        buckets: split,
        tiers: tier_rows.into_iter().map(|(_, row)| row).collect(),
        criteria,
    }
}

/// Statistiques pondérées d'une liste (retriggers, poids > 0).
fn retrigger_stats(mut rows: Vec<(u16, u64)>) -> Option<RetriggerStats> {
    rows.sort_unstable_by_key(|&(r, _)| r);
    let total: u128 = rows.iter().map(|&(_, w)| w as u128).sum();
    if total == 0 {
        return None;
    }
    let mut cum: u128 = 0;
    let mut median = rows[rows.len() - 1].0;
    for &(r, w) in &rows {
        cum += w as u128;
        if cum * 2 >= total {
            median = r;
            break;
        }
    }
    let wsum: f64 = rows.iter().map(|&(r, w)| r as f64 * w as f64).sum();
    let none: u128 = rows
        .iter()
        .filter(|&&(r, _)| r == 0)
        .map(|&(_, w)| w as u128)
        .sum();
    Some(RetriggerStats {
        mean: wsum / total as f64,
        min: rows[0].0,
        median,
        max: rows[rows.len() - 1].0,
        none_share: none as f64 / total as f64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Lut, LutRow};
    use crate::stats;

    /// (poids, gain ×100, bonus, palier, retriggers, criteria)
    type Row = (u64, u64, bool, u8, u16, Option<usize>);

    fn facts(rows: &[Row]) -> Vec<BookFacts> {
        rows.iter()
            .map(|&(weight, payout_x100, bonus, tier, retriggers, criteria)| BookFacts {
                weight,
                payout_x100,
                bonus,
                tier,
                retriggers,
                criteria,
                ..Default::default()
            })
            .collect()
    }

    fn buckets_of(books: &[BookFacts]) -> Vec<Bucket> {
        let lut = Lut::new(
            books
                .iter()
                .enumerate()
                .map(|(i, b)| LutRow {
                    sim_id: i as u64,
                    weight: b.weight,
                    payout_x100: b.payout_x100,
                })
                .collect(),
        );
        stats::analyze(&lut, 1.0).buckets
    }

    fn names() -> Vec<String> {
        ["basegame", "freegame_2", "freegame_3", "wincap", "wildmult"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// Un mode de base miniature. Tranche [10, 20) : 6 de poids en jeu de base
    /// (wildmult, pas de bonus), 3 en palier 2, 1 en palier 3.
    fn mode() -> Vec<BookFacts> {
        facts(&[
            (80, 0, false, 0, 0, Some(0)),
            (6, 1_500, false, 0, 0, Some(4)),
            (2, 1_200, true, 2, 1, Some(1)),
            (1, 1_800, true, 2, 4, Some(1)),
            (1, 1_000, true, 3, 0, Some(2)),
            // Jamais tiré : ne pèse sur rien, ni sur le min des retriggers.
            (0, 1_900, true, 2, 99, Some(1)),
            // Wincap qui déclenche : rangé à son vrai palier (3), pas ailleurs.
            (1, 2_000_000, true, 3, 6, Some(3)),
        ])
    }

    #[test]
    fn buckets_split_bonus_and_tiers() {
        let books = mode();
        let buckets = buckets_of(&books);
        let p = analyze(&books, 1.0, &buckets, &names());
        let band = p.buckets.iter().find(|b| b.start == 10.0).expect("[10, 20)");
        assert!((band.bonus_share - 0.4).abs() < 1e-12, "{}", band.bonus_share);
        let tiers: Vec<(Option<u8>, f64)> = band.tiers.iter().map(|t| (t.tier, t.share)).collect();
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].0, Some(2));
        assert!((tiers[0].1 - 0.3).abs() < 1e-12);
        assert_eq!(tiers[1].0, Some(3));
        assert!((tiers[1].1 - 0.1).abs() < 1e-12);
        // Mêmes tranches que `stats`, dans le même ordre.
        let bounds: Vec<(f64, f64)> = p.buckets.iter().map(|b| (b.start, b.end)).collect();
        let expected: Vec<(f64, f64)> = buckets.iter().map(|b| (b.start, b.end)).collect();
        assert_eq!(bounds, expected);
        // Le max win est un bonus de palier 3 : 100 % de sa tranche.
        let top = p.buckets.last().unwrap();
        assert_eq!(top.bonus_share, 1.0);
        assert_eq!(top.tiers.len(), 1);
        assert_eq!(top.tiers[0].tier, Some(3));
    }

    #[test]
    fn tiers_carry_weighted_retriggers() {
        let books = mode();
        let p = analyze(&books, 1.0, &buckets_of(&books), &names());
        let tiers: Vec<Option<u8>> = p.tiers.iter().map(|t| t.tier).collect();
        assert_eq!(tiers, [Some(2), Some(3)]);

        let two = &p.tiers[0];
        assert_eq!(two.count, 3);
        // Poids 2 + 1 (+ 0) sur 91.
        assert!((two.one_in - 91.0 / 3.0).abs() < 1e-9);
        assert!((two.avg_win - (2.0 * 12.0 + 18.0) / 3.0).abs() < 1e-12);
        let r = two.retriggers.as_ref().unwrap();
        // Le book à poids 0 (99 retriggers) n'entre ni dans le max ni ailleurs.
        assert_eq!((r.min, r.median, r.max), (1, 1, 4));
        assert!((r.mean - 2.0).abs() < 1e-12);
        assert_eq!(r.none_share, 0.0);
        assert_eq!(two.criteria, ["freegame_2"]);

        let three = &p.tiers[1];
        let r = three.retriggers.as_ref().unwrap();
        // Poids 1 à 0 retrigger, poids 1 à 6 : médiane 0 (la moitié est
        // atteinte dès la première valeur), moitié des bonus sans retrigger.
        assert_eq!((r.min, r.median, r.max), (0, 0, 6));
        assert!((r.none_share - 0.5).abs() < 1e-12);
        assert_eq!(three.criteria.len(), 2);
        // Les parts de RTP des paliers plus celle du jeu de base font 100 %.
        let wildmult = p.criteria.iter().find(|c| c.criteria == "wildmult").unwrap();
        let sum = two.rtp_share + three.rtp_share + wildmult.rtp_share;
        assert!((sum - 1.0).abs() < 1e-12, "{sum}");
    }

    #[test]
    fn criteria_rows_read_bonus_share_and_retriggers() {
        let books = mode();
        let p = analyze(&books, 1.0, &buckets_of(&books), &names());
        let by = |n: &str| p.criteria.iter().find(|c| c.criteria == n).unwrap();
        // Un book à bonus se range sous son palier (familles, 2026-10-04).
        let fg2 = by("2 scatters");
        assert_eq!(fg2.count, 3);
        assert_eq!(fg2.books_with_trigger, 3);
        assert_eq!(fg2.bonus_share, 1.0);
        // (2×1 + 1×4 + 0×99) / 3
        assert!((fg2.retriggers_mean.unwrap() - 2.0).abs() < 1e-12);
        let wm = by("wildmult");
        assert_eq!(wm.bonus_share, 0.0);
        assert_eq!(wm.retriggers_mean, None);
        assert!((wm.avg_win - 15.0).abs() < 1e-12);
        // Le plus probable d'abord.
        assert_eq!(p.criteria[0].criteria, "base game");
    }

    #[test]
    fn a_bonus_without_positions_is_its_own_row_last() {
        let books = facts(&[
            (10, 0, false, 0, 0, None),
            (1, 500, true, 0, 2, None),
            (1, 900, true, 4, 0, None),
        ]);
        let p = analyze(&books, 1.0, &buckets_of(&books), &[]);
        let tiers: Vec<Option<u8>> = p.tiers.iter().map(|t| t.tier).collect();
        assert_eq!(tiers, [Some(4), None]);
        assert_eq!(p.criteria[0].criteria, "(none)");
    }
}
