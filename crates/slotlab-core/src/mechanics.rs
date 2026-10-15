//! Mécaniques : ce que les événements des books MONTRENT au joueur, compté sans
//! rien savoir du jeu.
//!
//! ── Le besoin ───────────────────────────────────────────────────────────────
//! La config d'une slot tire ses mécaniques avec des poids (BLOOD WATER : poisson
//! +1 / +N / ×N à 88/10/2), mais ce que le joueur voit, c'est la LUT qui le
//! décide — elle choisit PARMI les books, et un bonus trop riche publie les books
//! pauvres. Seule la répartition pondérée par la LUT dit la vérité, et elle
//! doit se lire pour TOUTES les slots, quelle que soit leur mécanique.
//!
//! ── La règle générique ──────────────────────────────────────────────────────
//! Chaque événement est lu comme un objet ; ses champs deviennent des FACETTES :
//!   · un champ scalaire (texte, nombre, booléen) : `sharkFeed.phase = "hunt"` ;
//!   · un tableau d'objets : chaque champ scalaire de chaque objet —
//!     `sharkFeed.hunts[].fish = "M"` (un repas = une occurrence) ;
//!   · un tableau de scalaires : `bonusSetup.symbols[] = "H1"` ;
//!   · dans un tableau d'objets, chaque champ croisé avec les champs TEXTE de
//!     l'objet (ses « sortes ») : `sharkFeed.hunts[].fishValue@fish = "V|3"` —
//!     sans quoi la valeur d'un poisson +N et le facteur d'un poisson ×N, qui
//!     partagent `fishValue`, se mélangeraient ;
//!   · le type de l'événement lui-même, sous la facette `#events`.
//! Format « actions » (UNDEAD KING) : un événement sans `type` est une suite
//! d'actions `{kind, data}` ; chacune se lit comme un événement de type `kind`
//! dont les champs sont ceux de `data`.
//! Sont sautés : les plateaux (tableaux de tableaux), les objets imbriqués, les
//! coordonnées (`reel`, `row`…) et `index`/`type`. Une facette qui prend plus de
//! `MAX_VALUES` valeurs distinctes est CONTINUE (montants, multiplicateurs
//! libres) : ce n'est pas une répartition, elle est écartée à l'import.
//!
//! ── Pourquoi `RawValue` ─────────────────────────────────────────────────────
//! L'import lit des fichiers de plusieurs Go. Un `serde_json::Value` par
//! événement construirait chaque plateau ; ici un champ n'est qu'une tranche du
//! texte, et un tableau n'est ouvert que si son premier élément est un objet ou
//! un scalaire — un plateau est reconnu à son `[[` et jamais parcouru.

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::collections::HashMap;

/// Au-delà, une facette est continue et n'est pas une répartition.
pub const MAX_VALUES: usize = 48;
/// Une valeur plus longue n'est pas une catégorie (identifiant, texte libre).
const MAX_VALUE_LEN: usize = 40;
/// Facette qui compte les événements par type.
pub const EVENTS_FACET: &str = "#events";
/// Champs qui ne portent pas de mécanique : position, ordre, plateau.
const SKIP_KEYS: &[&str] = &[
    "index",
    "type",
    "board",
    "boards",
    "padPositions",
    "positions",
    "position",
    "reel",
    "row",
    "reels",
    "rows",
    "col",
    "column",
    "x",
    "y",
];

/// Texte d'une valeur scalaire, ou `None` si ce n'en est pas une.
fn scalar(raw: &RawValue) -> Option<&str> {
    let s = raw.get().trim();
    let first = s.as_bytes().first()?;
    match first {
        b'"' => {
            let inner = &s[1..s.len().saturating_sub(1)];
            (inner.len() <= MAX_VALUE_LEN && !inner.contains('\\')).then_some(inner)
        }
        b'-' | b'0'..=b'9' | b't' | b'f' => (s.len() <= MAX_VALUE_LEN).then_some(s),
        _ => None, // null, objet, tableau
    }
}

fn skipped(key: &str) -> bool {
    // `…panel` / `…panels` : coordonnées `[rangée, colonne]` du format « actions ».
    SKIP_KEYS.contains(&key) || key.ends_with("panel") || key.ends_with("panels")
}

/// Occurrences d'un book, par (facette, valeur). `events` : le tableau
/// `events` du book, chaque événement en texte brut.
pub fn scan_book(events: &[&RawValue], out: &mut HashMap<(String, String), u32>) {
    scan_book_with(events, |facet, value| {
        *out.entry((facet.to_string(), value.to_string())).or_insert(0) += 1;
    });
}

/// `scan_book` sans map : chaque occurrence est passée à `emit(facette, valeur)`.
/// L'import lit des centaines de millions d'occurrences ; les noms de facette
/// sont construits dans un seul tampon réutilisé, rien n'est alloué par
/// occurrence.
pub fn scan_book_with(events: &[&RawValue], mut emit: impl FnMut(&str, &str)) {
    let mut name = String::new();
    let mut pair = String::new();
    let mut bump = |facet: std::fmt::Arguments, value: &str| {
        use std::fmt::Write;
        name.clear();
        let _ = name.write_fmt(facet);
        emit(&name, value);
    };
    for ev in events {
        let Ok(obj) = serde_json::from_str::<HashMap<&str, &RawValue>>(ev.get()) else {
            continue;
        };
        if let Some(typ) = obj.get("type").and_then(|r| scalar(r)) {
            bump(format_args!("{EVENTS_FACET}"), typ);
            scan_fields(typ, &obj, &mut bump, &mut pair);
            continue;
        }
        // Format « actions » (UNDEAD KING) : chaque action `{kind, data}` se lit
        // comme un événement de type `kind` dont les champs sont ceux de `data`
        // (`ScepterAddWildMult.add`), ou `kind.data` quand `data` est un scalaire
        // (`BonusGames.data` : les free spins attribués).
        let Some(actions) = obj
            .get("actions")
            .and_then(|r| serde_json::from_str::<Vec<&RawValue>>(r.get()).ok())
        else {
            continue;
        };
        for action in actions {
            let Ok(a) = serde_json::from_str::<HashMap<&str, &RawValue>>(action.get()) else {
                continue;
            };
            let Some(kind) = a.get("kind").and_then(|r| scalar(r)) else {
                continue;
            };
            bump(format_args!("{EVENTS_FACET}"), kind);
            let Some(data) = a.get("data") else {
                continue;
            };
            if let Some(v) = scalar(data) {
                bump(format_args!("{kind}.data"), v);
            } else if let Ok(fields) = serde_json::from_str::<HashMap<&str, &RawValue>>(data.get()) {
                scan_fields(kind, &fields, &mut bump, &mut pair);
            }
        }
    }
}

/// Les facettes des champs d'un événement de type `typ` (règle du module).
fn scan_fields(
    typ: &str,
    obj: &HashMap<&str, &RawValue>,
    bump: &mut impl FnMut(std::fmt::Arguments, &str),
    pair: &mut String,
) {
    for (key, raw) in obj {
        if skipped(key) {
            continue;
        }
        if let Some(v) = scalar(raw) {
            bump(format_args!("{typ}.{key}"), v);
            continue;
        }
        let text = raw.get().trim_start();
        if !text.starts_with('[') || text[1..].trim_start().starts_with('[') {
            continue; // objet imbriqué, ou plateau
        }
        let Ok(items) = serde_json::from_str::<Vec<&RawValue>>(text) else {
            continue;
        };
        for item in items {
            if let Some(v) = scalar(item) {
                bump(format_args!("{typ}.{key}[]"), v);
                continue;
            }
            let Ok(fields) = serde_json::from_str::<HashMap<&str, &RawValue>>(item.get()) else {
                continue;
            };
            let scalars: Vec<(&str, &str, bool)> = fields
                .iter()
                .filter(|(field, _)| !skipped(field))
                .filter_map(|(field, fraw)| {
                    let v = scalar(fraw)?;
                    // Une « sorte » : un texte qui n'est ni un nombre ni un booléen.
                    let kind = fraw.get().trim_start().starts_with('"') && v.parse::<f64>().is_err();
                    Some((*field, v, kind))
                })
                .collect();
            for &(field, v, _) in &scalars {
                bump(format_args!("{typ}.{key}[].{field}"), v);
            }
            for &(kfield, kv, is_kind) in &scalars {
                if !is_kind {
                    continue;
                }
                for &(field, v, _) in &scalars {
                    if field != kfield {
                        pair.clear();
                        pair.push_str(kv);
                        pair.push('|');
                        pair.push_str(v);
                        bump(format_args!("{typ}.{key}[].{field}@{kfield}"), pair);
                    }
                }
            }
        }
    }
}

/// Occurrences de tous les books d'un mode, dans l'ordre de l'index des books.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MechIndex {
    /// Noms des facettes (`sharkFeed.hunts[].fish`).
    pub facets: Vec<String>,
    /// (facette, valeur) de chaque identifiant de valeur.
    pub values: Vec<(u32, String)>,
    /// `entries[offsets[i]..offsets[i + 1]]` : les occurrences du book i.
    pub offsets: Vec<u32>,
    /// (identifiant de valeur, nombre d'occurrences dans le book).
    pub entries: Vec<(u32, u16)>,
}

/// Une valeur d'une facette, sur la population demandée.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueReport {
    pub value: String,
    /// Occurrences par round, pondérées par la LUT (ce que le joueur voit).
    pub per_round: f64,
    /// Occurrences par book, sans pondération (ce que la simulation a produit).
    pub per_round_raw: f64,
    /// Part des rounds où elle apparaît au moins une fois (LUT).
    pub rounds_with: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FacetReport {
    /// Type d'événement (`sharkFeed`), ou `#events`.
    pub event: String,
    /// Chemin dans l'événement (`hunts[].fish`) ; vide pour `#events`.
    pub path: String,
    pub values: Vec<ValueReport>,
    /// Occurrences par round, toutes valeurs (LUT / brut).
    pub per_round: f64,
    pub per_round_raw: f64,
    /// Part des rounds où la facette apparaît (LUT).
    pub rounds_with: f64,
    /// Plus grand écart de part entre LUT et simulation, en points (0-100).
    pub drift_points: f64,
    /// Nombre d'occurrences par round (toutes valeurs) : (nombre, probabilité
    /// LUT), rounds sans occurrence compris. Au-delà de `COUNT_CAP`, groupés.
    pub count_dist: Vec<(u32, f64)>,
}

/// Dernière classe de `count_dist` : « COUNT_CAP ou plus ».
pub const COUNT_CAP: u32 = 60;

/// Répartition de chaque facette sur les books retenus. `weights[i]` : poids
/// LUT du book i (0 si absent de la LUT) ; `include[i]` : le book est dans la
/// population. Les facettes à une seule valeur ne sont gardées que si elles
/// comptent quelque chose (plus d'une occurrence par round en moyenne, ou pas
/// dans tous les rounds) ; `#events` est toujours gardée.
pub fn aggregate(idx: &MechIndex, weights: &[f64], include: &[bool]) -> Vec<FacetReport> {
    let nv = idx.values.len();
    let mut w_occ = vec![0.0_f64; nv];
    let mut r_occ = vec![0.0_f64; nv];
    let mut w_with = vec![0.0_f64; nv];
    let nf = idx.facets.len();
    let mut facet_w_with = vec![0.0_f64; nf];
    let mut total_w = 0.0_f64;
    let mut total_r = 0.0_f64;
    let mut seen = vec![u32::MAX; nf];
    // Occurrences par book et par facette → distribution du nombre par round.
    let mut book_tot = vec![0u32; nf];
    let mut touched: Vec<usize> = Vec::new();
    let mut dist = vec![vec![0.0_f64; COUNT_CAP as usize + 1]; nf];

    let books = idx.offsets.len().saturating_sub(1);
    for i in 0..books.min(weights.len()).min(include.len()) {
        if !include[i] {
            continue;
        }
        let w = weights[i];
        total_w += w;
        total_r += 1.0;
        let (a, b) = (idx.offsets[i] as usize, idx.offsets[i + 1] as usize);
        for &(vid, count) in &idx.entries[a..b] {
            let v = vid as usize;
            w_occ[v] += w * count as f64;
            r_occ[v] += count as f64;
            w_with[v] += w;
            let f = idx.values[v].0 as usize;
            if seen[f] != i as u32 {
                seen[f] = i as u32;
                facet_w_with[f] += w;
                touched.push(f);
            }
            book_tot[f] += count as u32;
        }
        for f in touched.drain(..) {
            dist[f][book_tot[f].min(COUNT_CAP) as usize] += w;
            book_tot[f] = 0;
        }
    }
    if total_r == 0.0 {
        return Vec::new();
    }
    let tw = if total_w > 0.0 { total_w } else { 1.0 };

    let mut by_facet: Vec<Vec<usize>> = vec![Vec::new(); nf];
    for (v, (f, _)) in idx.values.iter().enumerate() {
        by_facet[*f as usize].push(v);
    }
    let mut out = Vec::new();
    for (f, vals) in by_facet.iter().enumerate() {
        let per_round: f64 = vals.iter().map(|&v| w_occ[v]).sum::<f64>() / tw;
        let per_round_raw: f64 = vals.iter().map(|&v| r_occ[v]).sum::<f64>() / total_r;
        if per_round_raw == 0.0 && per_round == 0.0 {
            continue;
        }
        let name = &idx.facets[f];
        let rounds_with = facet_w_with[f] / tw;
        if name != EVENTS_FACET
            && vals.iter().filter(|&&v| r_occ[v] > 0.0).count() < 2
            && per_round <= 1.0 + 1e-9
            && rounds_with >= 1.0 - 1e-9
        {
            continue; // constante, une fois par round : ne dit rien
        }
        let mut values: Vec<ValueReport> = vals
            .iter()
            .filter(|&&v| r_occ[v] > 0.0 || w_occ[v] > 0.0)
            .map(|&v| ValueReport {
                value: idx.values[v].1.clone(),
                per_round: w_occ[v] / tw,
                per_round_raw: r_occ[v] / total_r,
                rounds_with: w_with[v] / tw,
            })
            .collect();
        let numeric = values.iter().all(|v| v.value.parse::<f64>().is_ok());
        if numeric {
            values.sort_by(|a, b| {
                let (x, y) = (a.value.parse::<f64>().unwrap(), b.value.parse::<f64>().unwrap());
                x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
            });
        } else {
            values.sort_by(|a, b| {
                b.per_round
                    .partial_cmp(&a.per_round)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.value.cmp(&b.value))
            });
        }
        let drift_points = if per_round > 0.0 && per_round_raw > 0.0 {
            values
                .iter()
                .map(|v| (v.per_round / per_round - v.per_round_raw / per_round_raw).abs() * 100.0)
                .fold(0.0, f64::max)
        } else {
            0.0
        };
        let (event, path) = match name.split_once('.') {
            Some((e, p)) => (e.to_string(), p.to_string()),
            None => (name.clone(), String::new()),
        };
        let mut count_dist = vec![(0u32, (1.0 - rounds_with).max(0.0))];
        count_dist.extend(
            dist[f]
                .iter()
                .enumerate()
                .skip(1)
                .filter(|(_, &w)| w > 0.0)
                .map(|(n, &w)| (n as u32, w / tw)),
        );
        out.push(FacetReport {
            event,
            path,
            values,
            per_round,
            per_round_raw,
            rounds_with,
            drift_points,
            count_dist,
        });
    }
    out
}

// ─── Stats clés : ce qu'un humain veut lire ─────────────────────────────────
//
// Le relevé générique liste TOUT (anticipations, niveaux de gain, totaux de
// free spins…) sous des noms de champs : exact, mais illisible. Une slot déclare
// donc, dans `slotlab.mechanics.json` à côté de sa math, les quelques stats qui
// comptent, avec des noms en clair et la répartition PRÉVUE ; SlotLab les lit
// dans le relevé et dit si ce que le joueur voit tient la promesse.

fn default_tolerance() -> f64 {
    3.0
}

/// Une valeur d'une stat clé : son nom en clair et sa part prévue.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyValueDef {
    pub value: String,
    #[serde(default)]
    pub label: Option<String>,
    /// Poids prévu (normalisé sur les valeurs qui en ont un).
    #[serde(default)]
    pub planned: Option<f64>,
    /// Poids prévu propre à un mode (remplace `planned`).
    #[serde(default)]
    pub planned_by_mode: HashMap<String, f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatDef {
    pub title: String,
    /// Facette du relevé : `sharkFeed.hunts[].fish`, `…fishValue@fish`.
    pub field: String,
    /// Sur une facette croisée : ne garder que cette sorte (`V`), préfixe retiré.
    #[serde(default)]
    pub only: Option<String>,
    /// Libellé des valeurs non déclarées : `{value}` remplacé (« ×{value} »).
    #[serde(default)]
    pub label_template: Option<String>,
    /// Valeurs écartées (le ×1 d'une phase sans multiplicateur…).
    #[serde(default)]
    pub exclude: Vec<String>,
    /// `bonus` (défaut ; tous les rounds si le mode n'a pas de bonus), `all`, `nobonus`.
    #[serde(default)]
    pub scope: Option<String>,
    /// Modes concernés (défaut : tous ceux où la facette existe).
    #[serde(default)]
    pub modes: Option<Vec<String>>,
    /// Ce que compte un round (« fish eaten per {round} » : `{round}` devient
    /// « bonus » ou « spin » selon le périmètre).
    #[serde(default)]
    pub per_round: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub values: Vec<KeyValueDef>,
    /// Écart toléré entre prévu et vu, en points.
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KeyStatsFile {
    pub stats: Vec<KeyStatDef>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatRow {
    pub value: String,
    pub label: String,
    /// Part vue en jeu (LUT), 0-1.
    pub seen: f64,
    /// Part prévue, 0-1.
    pub planned: Option<f64>,
    pub per_round: f64,
    /// Part des rounds où elle apparaît au moins une fois.
    pub rounds_with: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStat {
    pub title: String,
    pub note: Option<String>,
    /// Périmètre réellement lu (`bonus` retombe sur `all` sans bonus).
    pub scope: String,
    pub per_round_label: Option<String>,
    pub per_round: f64,
    pub rounds_with: f64,
    /// (médiane, 9 rounds sur 10 au plus, maximum) par round — sans `only`.
    pub count: Option<(u32, u32, u32)>,
    pub rows: Vec<KeyStatRow>,
    /// Plus grand écart prévu/vu, en points.
    pub worst_gap: Option<f64>,
    pub tolerance: f64,
    /// `ok`, `off` (écart > tolérance) ou `info` (rien de prévu).
    pub status: &'static str,
}

fn quantile(dist: &[(u32, f64)], q: f64) -> u32 {
    let mut acc = 0.0;
    for &(n, p) in dist {
        acc += p;
        if acc >= q - 1e-12 {
            return n;
        }
    }
    dist.last().map(|d| d.0).unwrap_or(0)
}

/// Évalue une stat clé sur le relevé d'un périmètre. `None` : la facette
/// n'existe pas dans ce mode (ou le mode n'est pas concerné).
pub fn key_stat(def: &KeyStatDef, mode: &str, scope: &str, facets: &[FacetReport]) -> Option<KeyStat> {
    if def.modes.as_ref().is_some_and(|m| !m.iter().any(|x| x == mode)) {
        return None;
    }
    let (event, path) = def.field.split_once('.').unwrap_or((def.field.as_str(), ""));
    let facet = facets.iter().find(|f| f.event == event && f.path == path)?;
    let prefix = def.only.as_ref().map(|o| format!("{o}|"));
    let values: Vec<(String, &ValueReport)> = facet
        .values
        .iter()
        .filter_map(|v| match &prefix {
            Some(p) => v.value.strip_prefix(p.as_str()).map(|s| (s.to_string(), v)),
            None => Some((v.value.clone(), v)),
        })
        .filter(|(k, _)| !def.exclude.contains(k))
        .collect();
    let total: f64 = values.iter().map(|(_, v)| v.per_round).sum();
    if total <= 0.0 {
        return None;
    }
    let planned_of = |d: &KeyValueDef| d.planned_by_mode.get(mode).copied().or(d.planned);
    let planned_total: f64 = def.values.iter().filter_map(planned_of).sum();
    let mut rows: Vec<KeyStatRow> = Vec::new();
    // D'abord les valeurs déclarées, dans l'ordre du fichier ; puis les autres.
    for d in &def.values {
        let v = values.iter().find(|(k, _)| *k == d.value);
        rows.push(KeyStatRow {
            value: d.value.clone(),
            label: d.label.clone().unwrap_or_else(|| d.value.clone()),
            seen: v.map(|(_, r)| r.per_round / total).unwrap_or(0.0),
            planned: planned_of(d).filter(|_| planned_total > 0.0).map(|p| p / planned_total),
            per_round: v.map(|(_, r)| r.per_round).unwrap_or(0.0),
            rounds_with: v.map(|(_, r)| r.rounds_with).unwrap_or(0.0),
        });
    }
    for (k, r) in &values {
        if !def.values.iter().any(|d| d.value == *k) {
            rows.push(KeyStatRow {
                value: k.clone(),
                label: def
                    .label_template
                    .as_ref()
                    .map(|t| t.replace("{value}", k))
                    .unwrap_or_else(|| k.clone()),
                seen: r.per_round / total,
                planned: None,
                per_round: r.per_round,
                rounds_with: r.rounds_with,
            });
        }
    }
    // Une valeur déclarée, sans rien de prévu et jamais vue, n'apprend rien.
    rows.retain(|r| r.seen > 0.0 || r.planned.is_some_and(|p| p > 0.0));
    let gaps: Vec<f64> = rows
        .iter()
        .filter_map(|r| r.planned.map(|p| (r.seen - p).abs() * 100.0))
        .collect();
    let worst_gap = gaps.iter().copied().reduce(f64::max);
    let status = match worst_gap {
        None => "info",
        Some(g) if g > def.tolerance => "off",
        Some(_) => "ok",
    };
    let filtered = def.only.is_some() || !def.exclude.is_empty();
    let count = (!filtered).then(|| {
        let d = &facet.count_dist;
        (quantile(d, 0.5), quantile(d, 0.9), d.iter().rev().find(|x| x.1 > 0.0).map(|x| x.0).unwrap_or(0))
    });
    let rounds_with = if !filtered {
        facet.rounds_with
    } else {
        values.iter().map(|(_, v)| v.rounds_with).fold(0.0, f64::max)
    };
    Some(KeyStat {
        title: def.title.clone(),
        note: def.note.clone(),
        scope: scope.to_string(),
        // `{round}` : « bonus » ou « spin » selon le périmètre réellement lu.
        per_round_label: def
            .per_round
            .as_ref()
            .map(|l| l.replace("{round}", if scope == "bonus" { "bonus" } else { "spin" })),
        per_round: total,
        rounds_with,
        count,
        rows,
        worst_gap,
        tolerance: def.tolerance,
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(book: &str) -> HashMap<(String, String), u32> {
        #[derive(Deserialize)]
        struct B<'a> {
            #[serde(borrow)]
            events: Vec<&'a RawValue>,
        }
        let b: B = serde_json::from_str(book).unwrap();
        let mut out = HashMap::new();
        scan_book(&b.events, &mut out);
        out
    }

    fn get(m: &HashMap<(String, String), u32>, f: &str, v: &str) -> u32 {
        m.get(&(f.to_string(), v.to_string())).copied().unwrap_or(0)
    }

    #[test]
    fn reads_scalars_arrays_of_objects_and_skips_boards() {
        let m = scan(
            r#"{"events":[
            {"index":1,"type":"reveal","gameType":"freegame","board":[[{"name":"H1"}],[{"name":"W"}]]},
            {"index":2,"type":"sharkFeed","hunts":[
                {"from":{"reel":1,"row":3},"fish":"V","fishValue":3,"totalFs":7},
                {"fish":"F","fishValue":0,"totalFs":8},{"fish":"F","fishValue":0,"totalFs":9}]},
            {"index":3,"type":"bonusSetup","symbols":["H1","H1","L2"],"positions":[{"reel":0,"row":1}]}]}"#,
        );
        assert_eq!(get(&m, EVENTS_FACET, "reveal"), 1);
        assert_eq!(get(&m, EVENTS_FACET, "sharkFeed"), 1);
        assert_eq!(get(&m, "reveal.gameType", "freegame"), 1);
        assert_eq!(get(&m, "sharkFeed.hunts[].fish", "F"), 2);
        assert_eq!(get(&m, "sharkFeed.hunts[].fish", "V"), 1);
        assert_eq!(get(&m, "sharkFeed.hunts[].fishValue", "3"), 1);
        assert_eq!(get(&m, "bonusSetup.symbols[]", "H1"), 2);
        // Croisé avec la sorte du poisson : la valeur d'un +N n'est pas celle d'un ×N.
        assert_eq!(get(&m, "sharkFeed.hunts[].fishValue@fish", "V|3"), 1);
        assert_eq!(get(&m, "sharkFeed.hunts[].fishValue@fish", "F|0"), 2);
        // Plateau, objet imbriqué, coordonnées, index : jamais des facettes.
        assert!(m.keys().all(|(f, _)| !f.contains("board")
            && !f.contains("from")
            && !f.contains("positions")
            && f != "reveal.index"));
    }

    #[test]
    fn reads_the_actions_format() {
        let m = scan(
            r#"{"events":[
            {"index":0,"win":0,"actions":[{"kind":"Haywire"},{"kind":"GridSnapshot","data":{"board":0,"progressiveBarValue":0,"cols":[["P1","BONUS"],["P9","P2"]]}}]},
            {"index":1,"win":0,"actions":[{"kind":"ScepterAddWildMult","data":{"scepter_panel":[0,5],"wild_panel":[2,5],"from":1,"to":3,"add":2}},
                {"kind":"ScepterAddWildMult","data":{"scepter_panel":[1,5],"wild_panel":[2,5],"from":3,"to":8,"add":5}}]},
            {"index":2,"win":0,"actions":[{"kind":"BonusGames","data":10}]},
            {"index":3,"win":1.5,"actions":[{"kind":"LinesHit","data":{"line":1,"streak":3,"panels":[[0,0],[0,1],[0,2]],"symbol":"P9"}}]}]}"#,
        );
        assert_eq!(get(&m, EVENTS_FACET, "Haywire"), 1);
        assert_eq!(get(&m, EVENTS_FACET, "ScepterAddWildMult"), 2);
        assert_eq!(get(&m, "ScepterAddWildMult.add", "2"), 1);
        assert_eq!(get(&m, "ScepterAddWildMult.add", "5"), 1);
        assert_eq!(get(&m, "BonusGames.data", "10"), 1);
        assert_eq!(get(&m, "LinesHit.symbol", "P9"), 1);
        assert_eq!(get(&m, "LinesHit.streak", "3"), 1);
        // Plateau et coordonnées `[rangée, colonne]` : jamais des facettes.
        assert!(m.keys().all(|(f, _)| !f.contains("cols") && !f.contains("panel") && !f.contains("board")));
    }

    #[test]
    fn key_stat_compares_seen_to_planned() {
        let idx = MechIndex {
            facets: vec!["sharkFeed.hunts[].fishValue@fish".into()],
            values: vec![(0, "V|2".into()), (0, "V|3".into()), (0, "M|2".into())],
            offsets: vec![0, 3],
            entries: vec![(0, 3), (1, 1), (2, 5)],
        };
        let facets = aggregate(&idx, &[1.0], &[true]);
        let def: KeyStatDef = serde_json::from_str(
            r#"{"title":"+N","field":"sharkFeed.hunts[].fishValue@fish","only":"V",
                "values":[{"value":"2","label":"+2","planned":55},{"value":"3","label":"+3","planned":45}]}"#,
        )
        .unwrap();
        let k = key_stat(&def, "BASE", "bonus", &facets).unwrap();
        // Vu : +2 = 75 %, +3 = 25 % (les ×2 sont exclus) ; prévu 55/45 → 20 points.
        assert_eq!(k.rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["+2", "+3"]);
        assert!((k.rows[0].seen - 0.75).abs() < 1e-12, "{:?}", k.rows);
        assert!((k.worst_gap.unwrap() - 20.0).abs() < 1e-9);
        assert_eq!(k.status, "off");
        assert!((k.per_round - 4.0).abs() < 1e-12);
    }

    #[test]
    fn aggregate_weights_by_the_lut() {
        // Deux books : le premier (2 F) pèse 3, le second (1 M) pèse 1.
        let idx = MechIndex {
            facets: vec!["sharkFeed.hunts[].fish".into()],
            values: vec![(0, "F".into()), (0, "M".into())],
            offsets: vec![0, 1, 2],
            entries: vec![(0, 2), (1, 1)],
        };
        let r = aggregate(&idx, &[3.0, 1.0], &[true, true]);
        assert_eq!(r.len(), 1);
        let f = &r[0];
        assert_eq!((f.event.as_str(), f.path.as_str()), ("sharkFeed", "hunts[].fish"));
        // LUT : F = 6/4 = 1,5 par round, M = 0,25 ; brut : F = 1, M = 0,5.
        let v = |s: &str| f.values.iter().find(|x| x.value == s).unwrap();
        assert!((v("F").per_round - 1.5).abs() < 1e-12);
        assert!((v("M").per_round - 0.25).abs() < 1e-12);
        assert!((v("F").per_round_raw - 1.0).abs() < 1e-12);
        assert!((v("M").rounds_with - 0.25).abs() < 1e-12);
        // Part de F : 85,7 % (LUT) contre 66,7 % (brut) → 19 points d'écart.
        assert!((f.drift_points - (6.0 / 7.0 - 2.0 / 3.0) * 100.0).abs() < 1e-9);
        // Par round : 2 repas (3/4 des rounds), 1 repas (1/4), jamais 0.
        assert_eq!(f.count_dist, vec![(0, 0.0), (1, 0.25), (2, 0.75)]);
    }
}
