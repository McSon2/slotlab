//! Les populations d'un mode, et le même bloc de statistiques pour chacune.
//!
//! Une moyenne ne dit pas ce que vit le joueur : « les bonus à 4 scatters
//! paient 96,5x en moyenne » peut vouloir dire que la moitié rend moins de 9x
//! et que quelques gains extrêmes portent le reste. Ce module décrit donc
//! chaque sous-ensemble de books par sa DISTRIBUTION — médiane, percentiles,
//! part sous un seuil, concentration de la valeur dans le haut — et pas
//! seulement par son gain moyen.
//!
//! ── Les populations ─────────────────────────────────────────────────────────
//!   · `all`          tous les spins du mode ;
//!   · `nobonus`      les spins qui n'ouvrent aucun bonus ;
//!   · `bonus`        ceux qui en ouvrent un (absent quand c'est tout le mode) ;
//!   · `tier:N`       les bonus d'un palier (positions du déclenchement,
//!                    `freespins::BookFs::tier`), toutes portes d'entrée confondues ;
//!   · `tier:N/crit:C` une variante : le palier N ouvert par le criteria C —
//!                    listée seulement quand le palier a plusieurs criteria ;
//!   · `crit:C`       tous les books d'un criteria (feature sans bonus comprise).
//!
//! ── Pondération ─────────────────────────────────────────────────────────────
//! Tout est pondéré par la LUT publiée : c'est le jeu tel qu'il se joue. Une
//! population sans poids n'est pas listée — elle n'arrive jamais. Les gains
//! sont en mises de BASE (×100 dans `BookFacts`), comme partout dans SlotLab.

use crate::provenance::BookFacts;
use serde::Serialize;
use std::collections::BTreeMap;

/// Bornes des tranches, communes à TOUS les modes et toutes les populations :
/// deux distributions se superposent tranche à tranche. La tranche 0 est le
/// gain nul exact ; la dernière est ouverte.
pub const BAND_EDGES: &[f64] = &[
    0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0,
    10_000.0, 20_000.0, 50_000.0, 100_000.0,
];

/// Seuils toujours mesurés (en plus du coût du mode et des seuils demandés).
pub const DEFAULT_THRESHOLDS: &[f64] = &[
    1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0,
];

/// Percentiles publiés.
pub const QUANTILES: &[f64] = &[0.05, 0.10, 0.25, 0.50, 0.75, 0.90, 0.95, 0.99, 0.999];

/// Points de la courbe « part sous x » : 1 - 1,5 - 2 - 3 - 5 - 7 par décade.
fn cdf_grid() -> Vec<f64> {
    let mut out = Vec::new();
    for e in -1..=5 {
        let decade = 10f64.powi(e);
        for m in [1.0, 1.5, 2.0, 3.0, 5.0, 7.0] {
            let v = ((decade * m) * 1000.0).round() / 1000.0;
            if v <= 100_000.0 {
                out.push(v);
            }
        }
    }
    out
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quantile {
    pub q: f64,
    pub value: f64,
    /// Un book tirable à ce gain exact — pour le rejouer.
    pub sim_id: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Threshold {
    pub x: f64,
    /// P(gain < x) dans la population.
    pub below: f64,
    /// P(gain ≥ x).
    pub at_least: f64,
    /// Part de la VALEUR de la population portée par les gains ≥ x.
    pub value_at_least: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Band {
    /// [start, end) en mises de base ; `start == end == 0` : gain nul exact ;
    /// `end` infini : tranche ouverte (sérialisé `null`).
    pub start: f64,
    pub end: Option<f64>,
    pub books: usize,
    /// Books effectifs de la tranche (Kish : (Σw)² / Σw²) — combien portent vraiment le poids.
    pub effective: f64,
    /// Part de la population qui tombe dans la tranche.
    pub share: f64,
    /// P(gain < end) : la part cumulée à la sortie de la tranche.
    pub cum_share: f64,
    /// Part de la valeur de la population portée par la tranche.
    pub value_share: f64,
    /// Gain moyen dans la tranche.
    pub avg: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CdfPoint {
    pub x: f64,
    /// P(gain < x).
    pub below: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupRow {
    /// Retriggers, ou spins attribués, selon la table.
    pub key: u32,
    pub books: usize,
    pub share: f64,
    pub avg: f64,
    pub median: f64,
    pub value_share: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Retriggers {
    /// Part des bonus qui retriggent au moins une fois.
    pub rate: f64,
    pub mean: f64,
    pub max: u32,
    /// Gain moyen des bonus avec / sans retrigger (`None` : aucun de ce côté).
    pub avg_with: Option<f64>,
    pub avg_without: Option<f64>,
    /// Part de la valeur portée par les bonus retriggés.
    pub value_share_with: f64,
    pub rows: Vec<GroupRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Spins {
    /// Free spins attribués à l'entrée, moyenne pondérée.
    pub mean_initial: f64,
    /// Free spins joués (retriggers compris), moyenne pondérée.
    pub mean_final: f64,
    /// Par nombre de spins attribués.
    pub rows: Vec<GroupRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BonusOnly {
    /// Gain du bonus seul (hors spin de base qui l'a ouvert).
    pub mean: f64,
    pub median: f64,
    /// Part du gain total des books portée par le bonus lui-même.
    pub share_of_payout: f64,
}

/// Montants ronds : la signature des mécaniques « UN board × UN multi »
/// (wild multiplicateur, multi spin, Degen) et des écrans pleins — un gain
/// quelconque au 0,1 près ne tombe sur un multiple de 100 que dans ~0,1 % des
/// cas, de 1 000 dans ~0,01 %. La max win du MODE est exclue : elle est ronde
/// par construction (plafond).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundAmounts {
    /// Probabilité (par spin du mode) d'un gain ≥ 100x hors max win.
    pub at_least_100: f64,
    /// Part de ces gains qui sont un multiple exact de 100x.
    pub share_100: f64,
    /// Probabilité (par spin du mode) d'un gain ≥ 1 000x hors max win.
    pub at_least_1000: f64,
    /// Part de ces gains qui sont un multiple exact de 1 000x.
    pub share_1000: f64,
    /// Montant rond le plus fréquent (≥ 100x) et sa part des gains ≥ 100x.
    pub top_value: Option<f64>,
    pub top_share: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopPayout {
    pub payout: f64,
    /// Par spin du mode.
    pub one_in: f64,
    /// Dans la population.
    pub share: f64,
    pub books: usize,
    pub sim_ids: Vec<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Population {
    pub key: String,
    /// `all` | `nobonus` | `bonus` | `tier` | `variant` | `criteria`
    pub kind: &'static str,
    pub label: String,
    pub parent: Option<String>,
    pub tier: Option<u8>,
    pub criteria: Option<String>,

    // ── Échantillon ──
    /// Books de la population (tirables ou non).
    pub books: usize,
    /// Books que la LUT peut tirer (poids > 0).
    pub drawable_books: usize,
    /// Nombre effectif de résultats (Kish : (Σw)² / Σw²) — combien de books
    /// portent vraiment la masse.
    pub effective_outcomes: f64,

    // ── Place dans le mode ──
    /// Par spin du mode.
    pub probability: f64,
    pub one_in: f64,
    /// Part des bonus du mode (populations de bonus seulement).
    pub share_of_bonuses: Option<f64>,
    /// Contribution au RTP du mode (÷ coût).
    pub rtp_contribution: f64,
    pub rtp_share: f64,

    // ── Distribution (dans la population) ──
    pub mean: f64,
    pub std_dev: f64,
    /// std / moyenne : 0 = toujours le même gain.
    pub cv: f64,
    pub min: f64,
    pub median: f64,
    pub max: f64,
    /// Part de la population au gain max.
    pub max_share: f64,
    /// Moyenne ÷ médiane ; `None` si la médiane est nulle.
    pub mean_over_median: Option<f64>,
    pub quantiles: Vec<Quantile>,
    pub zero_rate: f64,
    /// P(gain ≥ coût du mode).
    pub profitable_rate: f64,
    /// Part de la valeur portée par le 1 % / 10 % le plus haut.
    pub top1_share: f64,
    pub top10_share: f64,
    /// Moyenne des 99 % restants.
    pub mean_without_top1: f64,
    pub thresholds: Vec<Threshold>,
    pub bands: Vec<Band>,
    pub cdf: Vec<CdfPoint>,
    pub top_payouts: Vec<TopPayout>,
    /// `None` : aucun gain ≥ 100x hors max win.
    pub round_amounts: Option<RoundAmounts>,

    // ── Bonus ──
    pub retriggers: Option<Retriggers>,
    pub spins: Option<Spins>,
    pub bonus_only: Option<BonusOnly>,
}

/// Toutes les populations d'un mode.
///
/// `thresholds` s'ajoute aux seuils par défaut et au coût du mode.
pub fn analyze(
    books: &[BookFacts],
    cost: f64,
    criteria_names: &[String],
    thresholds: &[f64],
) -> Vec<Population> {
    let cost = if cost > 0.0 { cost } else { 1.0 };
    let total_w: f64 = books.iter().map(|b| b.weight as f64).sum();
    if total_w <= 0.0 {
        return Vec::new();
    }
    let mode_value: f64 = books.iter().map(|b| b.weight as f64 * x(b)).sum();
    let mode_max_x100 = books.iter().filter(|b| b.weight > 0).map(|b| b.payout_x100).max().unwrap_or(0);
    let bonus_w: f64 = books.iter().filter(|b| b.bonus).map(|b| b.weight as f64).sum();
    let any_nobonus = books.iter().any(|b| !b.bonus && b.weight > 0);

    let mut ts: Vec<f64> = DEFAULT_THRESHOLDS.iter().copied().chain([cost]).collect();
    ts.extend(thresholds.iter().copied().filter(|v| v.is_finite() && *v > 0.0));
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);

    let ctx = Ctx { total_w, mode_value, mode_max_x100, bonus_w, cost, thresholds: ts, cdf: cdf_grid() };
    // Criteria de TRANCHE `<source>__<suffixe>` (échantillonnage ciblé d'une tranche de gain,
    // même mécanique que la source) : rangés sous leur source, sinon chaque tranche ciblée se
    // lirait comme une variante du bonus (Toxic Spins : 15 lignes « 4 scatters · freegame_4__tN »).
    let family_name = |n: &str| n.split("__").next().unwrap_or(n).to_string();
    let fam_idx: Vec<usize> = criteria_names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let f = family_name(n);
            criteria_names.iter().position(|m| family_name(m) == f).unwrap_or(i)
        })
        .collect();
    let fam = |c: Option<usize>| c.map(|i| fam_idx.get(i).copied().unwrap_or(i));
    let crit_name = |c: Option<usize>| {
        c.and_then(|i| criteria_names.get(i).map(|n| family_name(n)))
            .unwrap_or_else(|| "(none)".to_string())
    };
    let tier_label = |t: u8| {
        if t == 0 {
            "untiered bonus".to_string()
        } else {
            format!("{t} scatters")
        }
    };
    let tier_key = |t: u8| if t == 0 { "tier:?".to_string() } else { format!("tier:{t}") };

    let mut out = Vec::new();
    let mut push = |p: Option<Population>| {
        if let Some(p) = p {
            out.push(p);
        }
    };

    push(ctx.population(books.iter(), Meta::new("all", "all", "All spins")));
    if any_nobonus && bonus_w > 0.0 {
        push(ctx.population(
            books.iter().filter(|b| !b.bonus),
            Meta::new("nobonus", "nobonus", "Spins without bonus").parent("all"),
        ));
        push(ctx.population(
            books.iter().filter(|b| b.bonus),
            Meta::new("bonus", "bonus", "All bonuses").parent("all"),
        ));
    }

    // Paliers croissants, « untiered » en dernier ; criteria par poids décroissant.
    let mut tiers: BTreeMap<(bool, u8), BTreeMap<Option<usize>, u64>> = BTreeMap::new();
    let mut crits: BTreeMap<Option<usize>, u64> = BTreeMap::new();
    for b in books {
        if b.bonus {
            *tiers.entry((b.tier == 0, b.tier)).or_default().entry(fam(b.criteria)).or_default() +=
                b.weight;
        }
        *crits.entry(fam(b.criteria)).or_default() += b.weight;
    }
    let bonus_parent = if any_nobonus { "bonus" } else { "all" };
    for (&(_, tier), by_crit) in &tiers {
        let key = tier_key(tier);
        push(ctx.population(
            books.iter().filter(|b| b.bonus && b.tier == tier),
            Meta::new(&key, "tier", &tier_label(tier)).parent(bonus_parent).tier(tier),
        ));
        let mut variants: Vec<(Option<usize>, u64)> =
            by_crit.iter().filter(|(_, w)| **w > 0).map(|(c, w)| (*c, *w)).collect();
        if variants.len() < 2 {
            continue;
        }
        variants.sort_by(|a, b| b.1.cmp(&a.1));
        for (c, _) in variants {
            let name = crit_name(c);
            push(ctx.population(
                books.iter().filter(|b| b.bonus && b.tier == tier && fam(b.criteria) == c),
                Meta::new(
                    &format!("{key}/crit:{name}"),
                    "variant",
                    &format!("{} · {name}", tier_label(tier)),
                )
                .parent(&key)
                .tier(tier)
                .criteria(&name),
            ));
        }
    }
    let mut crit_rows: Vec<(Option<usize>, u64)> =
        crits.into_iter().filter(|(_, w)| *w > 0).collect();
    crit_rows.sort_by(|a, b| b.1.cmp(&a.1));
    for (c, _) in crit_rows {
        let name = crit_name(c);
        push(ctx.population(
            books.iter().filter(|b| fam(b.criteria) == c),
            Meta::new(&format!("crit:{name}"), "criteria", &name).parent("all").criteria(&name),
        ));
    }
    out
}

fn x(b: &BookFacts) -> f64 {
    b.payout_x100 as f64 / 100.0
}

struct Meta {
    key: String,
    kind: &'static str,
    label: String,
    parent: Option<String>,
    tier: Option<u8>,
    criteria: Option<String>,
}

impl Meta {
    fn new(key: &str, kind: &'static str, label: &str) -> Self {
        Meta {
            key: key.to_string(),
            kind,
            label: label.to_string(),
            parent: None,
            tier: None,
            criteria: None,
        }
    }
    fn parent(mut self, p: &str) -> Self {
        self.parent = Some(p.to_string());
        self
    }
    fn tier(mut self, t: u8) -> Self {
        self.tier = (t > 0).then_some(t);
        self
    }
    fn criteria(mut self, c: &str) -> Self {
        self.criteria = Some(c.to_string());
        self
    }
}

struct Ctx {
    total_w: f64,
    mode_value: f64,
    mode_max_x100: u64,
    bonus_w: f64,
    cost: f64,
    thresholds: Vec<f64>,
    cdf: Vec<f64>,
}

impl Ctx {
    fn population<'a>(
        &self,
        members: impl Iterator<Item = &'a BookFacts>,
        meta: Meta,
    ) -> Option<Population> {
        let all: Vec<&BookFacts> = members.collect();
        let books = all.len();
        // Seuls les books tirables décrivent le jeu.
        let mut rows: Vec<&BookFacts> = all.into_iter().filter(|b| b.weight > 0).collect();
        if rows.is_empty() {
            return None;
        }
        rows.sort_by_key(|b| b.payout_x100);

        let w: f64 = rows.iter().map(|b| b.weight as f64).sum();
        let w2: f64 = rows.iter().map(|b| (b.weight as f64).powi(2)).sum();
        // Préfixes (poids, valeur) sur les gains croissants : « part sous x »,
        // « valeur au-dessus de x » et quantiles en O(log n) au lieu d'un
        // parcours complet par seuil — 50 seuils × 30 populations × 350 000
        // books coûtaient 7 s sur un jeu à 7 modes.
        let mut cum_w = Vec::with_capacity(rows.len() + 1);
        let mut cum_v = Vec::with_capacity(rows.len() + 1);
        cum_w.push(0.0_f64);
        cum_v.push(0.0_f64);
        for b in &rows {
            cum_w.push(cum_w.last().unwrap() + b.weight as f64);
            cum_v.push(cum_v.last().unwrap() + b.weight as f64 * x(b));
        }
        let value = *cum_v.last().unwrap();
        let mean = value / w;
        let var = rows.iter().map(|b| b.weight as f64 * (x(b) - mean).powi(2)).sum::<f64>() / w;
        let std_dev = var.sqrt();
        let max = x(rows[rows.len() - 1]);
        let max_w: f64 = rows
            .iter()
            .rev()
            .take_while(|b| b.payout_x100 == rows[rows.len() - 1].payout_x100)
            .map(|b| b.weight as f64)
            .sum();
        let idx_below = |x100: u64| rows.partition_point(|b| b.payout_x100 < x100);
        // `+ 0.0` : pas de -0.0 dans le JSON.
        let below = |x100: u64| cum_w[idx_below(x100)] / w + 0.0;
        let value_at_least = |x100: u64| {
            if value > 0.0 {
                (value - cum_v[idx_below(x100)]) / value + 0.0
            } else {
                0.0
            }
        };
        let quantile_at = |q: f64| {
            let target = q * w - 1e-9 * w;
            let i = cum_w[1..].partition_point(|&c| c < target).min(rows.len() - 1);
            rows[i]
        };

        let quantiles: Vec<Quantile> = QUANTILES
            .iter()
            .map(|&q| {
                let b = quantile_at(q);
                Quantile { q, value: x(b), sim_id: b.sim_id }
            })
            .collect();
        let median = quantiles.iter().find(|q| q.q == 0.5).map_or(0.0, |q| q.value);

        let (top1, rest1) = top_slice(&rows, w, 0.01);
        let (top10, _) = top_slice(&rows, w, 0.10);

        let x100 = |v: f64| (v * 100.0).round() as u64;
        let thresholds = self
            .thresholds
            .iter()
            .map(|&t| {
                let b = below(x100(t));
                Threshold { x: t, below: b, at_least: 1.0 - b, value_at_least: value_at_least(x100(t)) }
            })
            .collect();
        let cdf = self.cdf.iter().map(|&v| CdfPoint { x: v, below: below(x100(v)) }).collect();

        Some(Population {
            key: meta.key,
            kind: meta.kind,
            label: meta.label,
            parent: meta.parent,
            tier: meta.tier,
            criteria: meta.criteria,
            books,
            drawable_books: rows.len(),
            effective_outcomes: if w2 > 0.0 { w * w / w2 } else { 0.0 },
            probability: w / self.total_w,
            one_in: self.total_w / w,
            share_of_bonuses: (meta.kind != "all"
                && meta.kind != "nobonus"
                && meta.kind != "criteria"
                && self.bonus_w > 0.0)
                .then(|| w / self.bonus_w),
            rtp_contribution: value / self.total_w / self.cost,
            rtp_share: if self.mode_value > 0.0 { value / self.mode_value } else { 0.0 },
            mean,
            std_dev,
            cv: if mean > 0.0 { std_dev / mean } else { 0.0 },
            min: x(rows[0]),
            median,
            max,
            max_share: max_w / w,
            mean_over_median: (median > 0.0).then(|| mean / median),
            quantiles,
            zero_rate: below(1),
            profitable_rate: 1.0 - below(x100(self.cost)) + 0.0,
            top1_share: if value > 0.0 { top1 / value } else { 0.0 },
            top10_share: if value > 0.0 { top10 / value } else { 0.0 },
            mean_without_top1: rest1,
            thresholds,
            bands: bands(&rows, w, value),
            cdf,
            top_payouts: top_payouts(&rows, w, self.total_w),
            round_amounts: round_amounts(&rows, self.total_w, self.mode_max_x100),
            retriggers: retriggers(&rows, w, value),
            spins: spins(&rows, w, value),
            bonus_only: bonus_only(&rows, w),
        })
    }
}

/// Le book au quantile `q` : le premier (gains croissants) où la part cumulée
/// atteint `q`.
fn quantile_book<'a>(rows: &[&'a BookFacts], w: f64, q: f64) -> &'a BookFacts {
    let target = q * w;
    let mut cum = 0.0;
    for b in rows {
        cum += b.weight as f64;
        if cum >= target - 1e-9 * w {
            return b;
        }
    }
    rows[rows.len() - 1]
}

/// (valeur portée par la tranche haute de masse `frac`, moyenne du reste).
/// Inclusion partielle du book à cheval, comme le CVaR de `stats`.
fn top_slice(rows: &[&BookFacts], w: f64, frac: f64) -> (f64, f64) {
    let target = w * frac;
    let mut mass = 0.0;
    let mut top = 0.0;
    for b in rows.iter().rev() {
        if mass >= target {
            break;
        }
        let take = (b.weight as f64).min(target - mass);
        top += take * x(b);
        mass += take;
    }
    let value: f64 = rows.iter().map(|b| b.weight as f64 * x(b)).sum();
    let rest_w = w - mass;
    (top, if rest_w > 0.0 { (value - top) / rest_w } else { 0.0 })
}

fn bands(rows: &[&BookFacts], w: f64, value: f64) -> Vec<Band> {
    // Bornes ×100 : 0 exact, puis ]0 ; 0,5[, [0,5 ; 1[, … [100 000 ; ∞[.
    let edges: Vec<u64> = BAND_EDGES.iter().map(|e| (e * 100.0).round() as u64).collect();
    let n = edges.len() + 2;
    let mut acc = vec![(0usize, 0.0_f64, 0.0_f64); n];
    let mut acc_w2 = vec![0.0_f64; n];
    for b in rows {
        let i = if b.payout_x100 == 0 {
            0
        } else {
            1 + edges.partition_point(|&e| e <= b.payout_x100)
        };
        acc[i].0 += 1;
        acc[i].1 += b.weight as f64;
        acc[i].2 += b.weight as f64 * x(b);
        acc_w2[i] += (b.weight as f64).powi(2);
    }
    let mut cum = 0.0;
    let mut out = Vec::with_capacity(n);
    for (i, &(books, bw, bv)) in acc.iter().enumerate() {
        let (start, end) = match i {
            0 => (0.0, Some(0.0)),
            1 => (0.0, Some(BAND_EDGES[0])),
            _ => (BAND_EDGES[i - 2], BAND_EDGES.get(i - 1).copied()),
        };
        cum += bw / w;
        out.push(Band {
            start,
            end,
            books,
            effective: if acc_w2[i] > 0.0 { bw * bw / acc_w2[i] } else { 0.0 },
            share: bw / w,
            cum_share: cum.min(1.0),
            value_share: if value > 0.0 { bv / value } else { 0.0 },
            avg: if bw > 0.0 { bv / bw } else { 0.0 },
        });
    }
    out
}

fn round_amounts(rows: &[&BookFacts], total_w: f64, mode_max_x100: u64) -> Option<RoundAmounts> {
    let (mut w100, mut r100, mut w1000, mut r1000) = (0.0, 0.0, 0.0, 0.0);
    let mut by_value: BTreeMap<u64, f64> = BTreeMap::new();
    for b in rows {
        let p = b.payout_x100;
        if p < 10_000 || p == mode_max_x100 {
            continue;
        }
        let bw = b.weight as f64;
        w100 += bw;
        if p % 10_000 == 0 {
            r100 += bw;
            *by_value.entry(p).or_default() += bw;
        }
        if p >= 100_000 {
            w1000 += bw;
            if p % 100_000 == 0 {
                r1000 += bw;
            }
        }
    }
    if w100 <= 0.0 {
        return None;
    }
    let top = by_value.iter().max_by(|a, b| a.1.total_cmp(b.1)).map(|(&v, &bw)| (v, bw));
    Some(RoundAmounts {
        at_least_100: w100 / total_w,
        share_100: r100 / w100,
        at_least_1000: w1000 / total_w,
        share_1000: if w1000 > 0.0 { r1000 / w1000 } else { 0.0 },
        top_value: top.map(|(v, _)| v as f64 / 100.0),
        top_share: top.map_or(0.0, |(_, bw)| bw / w100),
    })
}

fn top_payouts(rows: &[&BookFacts], w: f64, total_w: f64) -> Vec<TopPayout> {
    let mut out: Vec<TopPayout> = Vec::new();
    let mut i = rows.len();
    while i > 0 && out.len() < 5 {
        let p = rows[i - 1].payout_x100;
        let (mut pw, mut books, mut sim_ids) = (0.0, 0usize, Vec::new());
        while i > 0 && rows[i - 1].payout_x100 == p {
            pw += rows[i - 1].weight as f64;
            books += 1;
            if sim_ids.len() < 4 {
                sim_ids.push(rows[i - 1].sim_id);
            }
            i -= 1;
        }
        out.push(TopPayout {
            payout: p as f64 / 100.0,
            one_in: total_w / pw,
            share: pw / w,
            books,
            sim_ids,
        });
    }
    out
}

/// Regroupe les books par clé et décrit chaque groupe dans la population.
fn group_rows(rows: &[&BookFacts], w: f64, value: f64, key: impl Fn(&BookFacts) -> u32) -> Vec<GroupRow> {
    // `rows` est trié par gain : chaque groupe l'est aussi, la médiane se lit
    // en une passe.
    let mut groups: BTreeMap<u32, Vec<&BookFacts>> = BTreeMap::new();
    for b in rows {
        groups.entry(key(b)).or_default().push(b);
    }
    groups
        .into_iter()
        .map(|(k, g)| {
            let gw: f64 = g.iter().map(|b| b.weight as f64).sum();
            let gv: f64 = g.iter().map(|b| b.weight as f64 * x(b)).sum();
            GroupRow {
                key: k,
                books: g.len(),
                share: gw / w,
                avg: gv / gw,
                median: x(quantile_book(&g, gw, 0.5)),
                value_share: if value > 0.0 { gv / value } else { 0.0 },
            }
        })
        .collect()
}

fn retriggers(rows: &[&BookFacts], w: f64, value: f64) -> Option<Retriggers> {
    let bonus: Vec<&BookFacts> = rows.iter().copied().filter(|b| b.bonus).collect();
    // Seulement quand toute la population est faite de bonus : sur un mélange,
    // « part des bonus qui retriggent » se lirait comme une part des spins.
    if bonus.len() != rows.len() {
        return None;
    }
    let groups = group_rows(rows, w, value, |b| b.retriggers as u32);
    let with = |f: &dyn Fn(u32) -> bool| {
        let (gw, gv) = rows
            .iter()
            .filter(|b| f(b.retriggers as u32))
            .fold((0.0, 0.0), |(a, v), b| (a + b.weight as f64, v + b.weight as f64 * x(b)));
        (gw, gv)
    };
    let (ww, wv) = with(&|r| r > 0);
    let (nw, nv) = with(&|r| r == 0);
    Some(Retriggers {
        rate: ww / w,
        mean: rows.iter().map(|b| b.weight as f64 * b.retriggers as f64).sum::<f64>() / w,
        max: rows.iter().map(|b| b.retriggers as u32).max().unwrap_or(0),
        avg_with: (ww > 0.0).then(|| wv / ww),
        avg_without: (nw > 0.0).then(|| nv / nw),
        value_share_with: if value > 0.0 { wv / value } else { 0.0 },
        rows: groups,
    })
}

fn spins(rows: &[&BookFacts], w: f64, value: f64) -> Option<Spins> {
    if !rows.iter().all(|b| b.bonus && b.spins_initial > 0) {
        return None;
    }
    let mean = |f: &dyn Fn(&BookFacts) -> u16| {
        rows.iter().map(|b| b.weight as f64 * f(b) as f64).sum::<f64>() / w
    };
    Some(Spins {
        mean_initial: mean(&|b| b.spins_initial),
        mean_final: mean(&|b| b.spins_final.max(b.spins_initial)),
        rows: group_rows(rows, w, value, |b| b.spins_initial as u32),
    })
}

fn bonus_only(rows: &[&BookFacts], w: f64) -> Option<BonusOnly> {
    if !rows.iter().all(|b| b.bonus && b.fg_win_x100.is_some()) {
        return None;
    }
    let fg = |b: &BookFacts| b.fg_win_x100.unwrap_or(0) as f64 / 100.0;
    let fg_value: f64 = rows.iter().map(|b| b.weight as f64 * fg(b)).sum();
    let value: f64 = rows.iter().map(|b| b.weight as f64 * x(b)).sum();
    let mut sorted: Vec<(f64, f64)> = rows.iter().map(|b| (fg(b), b.weight as f64)).collect();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cum = 0.0;
    let mut median = sorted.last().map_or(0.0, |s| s.0);
    for &(v, bw) in &sorted {
        cum += bw;
        if cum >= 0.5 * w - 1e-9 * w {
            median = v;
            break;
        }
    }
    Some(BonusOnly {
        mean: fg_value / w,
        median,
        share_of_payout: if value > 0.0 { fg_value / value } else { 0.0 },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(sim_id: u64, weight: u64, payout: f64) -> BookFacts {
        BookFacts {
            sim_id,
            weight,
            payout_x100: (payout * 100.0).round() as u64,
            ..Default::default()
        }
    }

    fn bonus(sim_id: u64, weight: u64, payout: f64, tier: u8, retriggers: u16, crit: usize) -> BookFacts {
        BookFacts {
            bonus: true,
            tier,
            retriggers,
            criteria: Some(crit),
            spins_initial: 10,
            spins_final: 10 + 5 * retriggers,
            fg_win_x100: Some(((payout - 1.0).max(0.0) * 100.0).round() as u64),
            ..book(sim_id, weight, payout)
        }
    }

    fn names() -> Vec<String> {
        ["basegame", "freegame_3", "freegame_4", "wincap"].iter().map(|s| s.to_string()).collect()
    }

    /// 1 000 de poids : 900 spins de base, 80 bonus à 3 scatters, 20 à 4
    /// (dont un wincap). Le palier 4 a une moyenne tirée par un gain extrême.
    fn mode() -> Vec<BookFacts> {
        let base = |id, w, p| BookFacts { criteria: Some(0), ..book(id, w, p) };
        vec![
            base(1, 700, 0.0),
            base(2, 150, 0.5),
            base(3, 50, 2.0),
            bonus(10, 40, 5.0, 3, 0, 1),
            bonus(11, 30, 12.0, 3, 1, 1),
            bonus(12, 10, 40.0, 3, 2, 1),
            bonus(20, 10, 6.0, 4, 0, 2),
            bonus(21, 9, 8.0, 4, 0, 2),
            bonus(22, 1, 1000.0, 4, 3, 3),
            // Jamais tiré : ne pèse sur rien.
            bonus(23, 0, 5000.0, 4, 9, 2),
        ]
    }

    fn pop<'a>(ps: &'a [Population], key: &str) -> &'a Population {
        ps.iter().find(|p| p.key == key).unwrap_or_else(|| panic!("no {key}"))
    }

    #[test]
    fn lists_the_populations_of_a_base_mode() {
        let ps = analyze(&mode(), 1.0, &names(), &[]);
        let keys: Vec<&str> = ps.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "all",
                "nobonus",
                "bonus",
                "tier:3",
                "tier:4",
                "tier:4/crit:freegame_4",
                "tier:4/crit:wincap",
                "crit:basegame",
                "crit:freegame_3",
                "crit:freegame_4",
                "crit:wincap",
            ]
        );
        // Les paliers se partagent les bonus ; les tiers se partagent le mode.
        let t3 = pop(&ps, "tier:3");
        let t4 = pop(&ps, "tier:4");
        assert!((t3.share_of_bonuses.unwrap() + t4.share_of_bonuses.unwrap() - 1.0).abs() < 1e-12);
        let sum: f64 = ["nobonus", "bonus"].iter().map(|k| pop(&ps, k).rtp_share).sum();
        assert!((sum - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_median_tells_what_the_mean_hides() {
        let ps = analyze(&mode(), 1.0, &names(), &[]);
        let t4 = pop(&ps, "tier:4");
        // (10×6 + 9×8 + 1×1000) / 20 = 56,6 ; médiane 6 (10 sur 20 à 6).
        assert!((t4.mean - 56.6).abs() < 1e-9, "{}", t4.mean);
        assert_eq!(t4.median, 6.0);
        assert!((t4.mean_over_median.unwrap() - 56.6 / 6.0).abs() < 1e-9);
        // 95 % du palier paie moins de 10x.
        let under10 = t4.thresholds.iter().find(|t| t.x == 10.0).unwrap();
        assert!((under10.below - 0.95).abs() < 1e-12);
        // Le 1 % du haut (0,2 de poids, pris sur le book à 1000x) :
        // 0,2 × 1000 / 1132 de valeur.
        assert!((t4.top1_share - 200.0 / 1132.0).abs() < 1e-9, "{}", t4.top1_share);
        // Le book jamais tiré n'entre ni dans le max ni dans l'échantillon.
        assert_eq!(t4.max, 1000.0);
        assert_eq!((t4.books, t4.drawable_books), (4, 3));
        assert!((t4.max_share - 0.05).abs() < 1e-12);
        // Le percentile renvoie un book rejouable à ce gain exact.
        let p50 = t4.quantiles.iter().find(|q| q.q == 0.5).unwrap();
        assert_eq!((p50.value, p50.sim_id), (6.0, 20));
    }

    #[test]
    fn bands_and_cdf_add_up() {
        let ps = analyze(&mode(), 1.0, &names(), &[7.0]);
        for p in &ps {
            let share: f64 = p.bands.iter().map(|b| b.share).sum();
            let value: f64 = p.bands.iter().map(|b| b.value_share).sum();
            assert!((share - 1.0).abs() < 1e-9, "{} {share}", p.key);
            assert!(p.rtp_contribution == 0.0 || (value - 1.0).abs() < 1e-9, "{} {value}", p.key);
            assert!((p.bands.last().unwrap().cum_share - 1.0).abs() < 1e-9);
            assert!(p.cdf.windows(2).all(|w| w[0].below <= w[1].below));
        }
        let all = pop(&ps, "all");
        // Gain nul exact : sa propre tranche.
        assert!((all.bands[0].share - 0.7).abs() < 1e-12);
        assert_eq!(all.zero_rate, 0.7);
        // Le seuil demandé est mesuré, ainsi que le coût.
        assert!(all.thresholds.iter().any(|t| t.x == 7.0));
        // P(< 1) = 0,85 donc 15 % rendent au moins la mise.
        assert!((all.profitable_rate - 0.15).abs() < 1e-12);
    }

    #[test]
    fn retriggers_spins_and_bonus_only_win() {
        let ps = analyze(&mode(), 1.0, &names(), &[]);
        let t3 = pop(&ps, "tier:3");
        let r = t3.retriggers.as_ref().unwrap();
        // 40 sans, 40 avec (30 à 1, 10 à 2) sur 80.
        assert!((r.rate - 0.5).abs() < 1e-12);
        assert!((r.mean - 50.0 / 80.0).abs() < 1e-12);
        assert_eq!(r.max, 2);
        assert_eq!(r.avg_without, Some(5.0));
        assert!((r.avg_with.unwrap() - (30.0 * 12.0 + 10.0 * 40.0) / 40.0).abs() < 1e-9);
        assert_eq!(r.rows.iter().map(|g| g.key).collect::<Vec<_>>(), [0, 1, 2]);
        let s = t3.spins.as_ref().unwrap();
        assert_eq!(s.mean_initial, 10.0);
        assert!((s.mean_final - (10.0 + 5.0 * 50.0 / 80.0)).abs() < 1e-9);
        let fg = t3.bonus_only.as_ref().unwrap();
        // Le spin de base qui ouvre vaut 1x ici : le bonus seul paie 1x de moins.
        assert!((fg.mean - (t3.mean - 1.0)).abs() < 1e-9);
        // Sur un mélange de spins, pas de retriggers : ils ne se liraient pas.
        assert!(pop(&ps, "all").retriggers.is_none());
        assert!(pop(&ps, "nobonus").spins.is_none());
    }

    #[test]
    fn a_buy_mode_has_no_nobonus_and_no_duplicate_bonus_row() {
        let books: Vec<BookFacts> =
            vec![bonus(1, 60, 50.0, 4, 0, 2), bonus(2, 40, 200.0, 4, 1, 2)];
        let ps = analyze(&books, 100.0, &names(), &[]);
        let keys: Vec<&str> = ps.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["all", "tier:4", "crit:freegame_4"]);
        let t4 = pop(&ps, "tier:4");
        assert_eq!(t4.parent.as_deref(), Some("all"));
        // Rentable = au moins le coût du mode (100x) : 40 %.
        assert!((t4.profitable_rate - 0.4).abs() < 1e-12);
        assert!((t4.rtp_contribution - 1.1).abs() < 1e-12);
    }

    #[test]
    fn round_amounts_skip_the_max_win_and_count_by_spin_of_the_mode() {
        let base = |id, w, p| BookFacts { criteria: Some(0), ..book(id, w, p) };
        let books = vec![
            base(1, 9_000, 0.0),
            // ≥ 100x hors bonus : 3 de poids ronds (2 × 3 000x + 1 × 200x), 1 non rond.
            base(2, 2, 3000.0),
            base(3, 1, 200.0),
            base(4, 1, 1234.5),
            // Max win du mode : ronde par construction, jamais comptée.
            base(5, 1, 20_000.0),
            bonus(10, 995, 150.0, 3, 0, 1),
        ];
        let ps = analyze(&books, 1.0, &names(), &[]);
        let nb = pop(&ps, "nobonus").round_amounts.as_ref().expect("nobonus ≥ 100x");
        assert!((nb.at_least_100 - 4.0 / 10_000.0).abs() < 1e-12);
        assert!((nb.share_100 - 0.75).abs() < 1e-12);
        assert!((nb.at_least_1000 - 3.0 / 10_000.0).abs() < 1e-12);
        assert!((nb.share_1000 - 2.0 / 3.0).abs() < 1e-12);
        assert_eq!(nb.top_value, Some(3000.0));
        assert!((nb.top_share - 0.5).abs() < 1e-12);
        // 150x : pas un multiple de 100.
        let b = pop(&ps, "bonus").round_amounts.as_ref().expect("bonus ≥ 100x");
        assert_eq!(b.share_100, 0.0);
        assert_eq!(b.top_value, None);
    }
}
