use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use slotlab_core::family;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/ui/games/{name}/search-meta", get(search_meta))
        .route("/api/ui/search", post(search))
}

/// Search metadata per mode: criteria and event types — all from the
/// .idx sidecars (never from the .zst files). Trigger stats live in
/// the Analyze bonus-stats endpoint (weighted by the LUT).
async fn search_meta(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<serde_json::Value> {
    // Opening a mode's books reads its whole index on first use.
    tokio::task::spawn_blocking(move || search_meta_sync(&state, name))
        .await
        .unwrap_or_else(|e| Json(json!({ "error": e.to_string() })))
}

fn search_meta_sync(state: &AppState, name: String) -> Json<serde_json::Value> {
    let Some(game) = state.registry.get(&name) else {
        return Json(json!({ "error": "unknown game" }));
    };
    let mut modes = Vec::new();
    for m in &game.modes {
        let Ok(books) = state.books.mode_books(&game, &m.config.name) else {
            modes.push(json!({
                "mode": m.config.name,
                "imported": false,
            }));
            continue;
        };
        let idx = &books.index;
        // Familles (`slotlab_core::family`) : paliers de bonus puis familles hors bonus, des
        // plus fréquentes aux plus rares — jamais les seaux de simulation.
        let fam_names: Vec<String> =
            idx.criteria_names.iter().map(|n| family::criteria_family(n)).collect();
        let mut tiers: std::collections::BTreeSet<u8> = std::collections::BTreeSet::new();
        let mut fams: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for b in &idx.books {
            if b.fs_triggers > 0 {
                tiers.insert(b.fs_tier);
            } else if let Some(f) = fam_names.get(b.criteria as usize) {
                *fams.entry(f.as_str()).or_default() += 1;
            }
        }
        let mut fam_rows: Vec<(&str, usize)> = fams.into_iter().collect();
        fam_rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let families: Vec<String> = tiers
            .iter()
            .filter(|&&t| t > 0)
            .chain(tiers.iter().filter(|&&t| t == 0))
            .map(|&t| family::tier_label(t))
            .chain(fam_rows.into_iter().map(|(f, _)| f.to_string()))
            .collect();
        modes.push(json!({
            "mode": m.config.name,
            "imported": true,
            "criteria": families,
            "eventTypes": idx.event_type_names,
            "totalBooks": idx.books.len(),
        }));
    }
    Json(json!({ "game": name, "modes": modes }))
}

/// Presence of the bonus in a book.
///
/// « Gros gain en jeu de base » est la recherche la plus demandée, et sans
/// ce filtre elle est impossible : au-delà de quelques dizaines de x, les
/// books de freegame écrasent en nombre ceux de basegame et occupent seuls
/// la première page de résultats.
#[derive(Deserialize, Default, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BonusFilter {
    #[default]
    Any,
    /// At least one freeSpinTrigger event.
    With,
    /// No freeSpinTrigger event at all.
    Without,
}

impl BonusFilter {
    pub fn keeps(self, fs_triggers: u16) -> bool {
        match self {
            BonusFilter::Any => true,
            BonusFilter::With => fs_triggers > 0,
            BonusFilter::Without => fs_triggers == 0,
        }
    }
}

/// Result order. `Id` keeps the order of the math-sdk files.
#[derive(Deserialize, Default, Clone, Copy, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SearchSort {
    #[default]
    PayoutDesc,
    PayoutAsc,
    Id,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub game: String,
    /// Empty → all modes.
    #[serde(default)]
    pub mode: Option<String>,
    /// Bounds in bet multiplier (inclusive).
    #[serde(default)]
    pub payout_min: Option<f64>,
    #[serde(default)]
    pub payout_max: Option<f64>,
    #[serde(default)]
    pub criteria: Option<String>,
    /// The book must contain ALL of these event types.
    #[serde(default)]
    pub event_types: Vec<String>,
    /// The book must contain NONE of these event types.
    #[serde(default)]
    pub exclude_event_types: Vec<String>,
    #[serde(default)]
    pub bonus: BonusFilter,
    /// Bornes sur le nombre d'événements `freeSpinTrigger` du book.
    #[serde(default)]
    pub min_fs_triggers: Option<u16>,
    #[serde(default)]
    pub max_fs_triggers: Option<u16>,
    /// Bornes sur le nombre de retriggers DANS le bonus (`fs_retriggers`) :
    /// ce que le filtre « Retriggers ≥ » veut dire. Compter les
    /// `freeSpinTrigger` à la place répondait 1 pour tout book de bonus de
    /// BLOOD WATER, quels que soient ses retriggers.
    #[serde(default)]
    pub min_retriggers: Option<u16>,
    #[serde(default)]
    pub max_retriggers: Option<u16>,
    #[serde(default)]
    pub sort: SearchSort,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchHit {
    mode: String,
    sim_id: u64,
    payout_x100: u64,
    criteria: Option<String>,
    fs_triggers: u16,
    fs_retriggers: u16,
    one_in: f64,
}

/// A match, kept lightweight: the page is only built after sorting.
struct Match {
    mode: usize,
    sim_id: u64,
    payout_x100: u64,
    criteria: u16,
    fs_triggers: u16,
    fs_retriggers: u16,
    fs_tier: u8,
}

async fn search(
    State(state): State<AppState>,
    Json(q): Json<SearchQuery>,
) -> Json<serde_json::Value> {
    // Walks every book index of the game (a million rows each): off the runtime.
    tokio::task::spawn_blocking(move || search_sync(&state, q))
        .await
        .unwrap_or_else(|e| Json(json!({ "error": e.to_string() })))
}

fn search_sync(state: &AppState, q: SearchQuery) -> Json<serde_json::Value> {
    let Some(game) = state.registry.get(&q.game) else {
        return Json(json!({ "error": "unknown game" }));
    };
    let min_x100 = q.payout_min.map(|v| (v * 100.0).round() as u64);
    let max_x100 = q.payout_max.map(|v| (v * 100.0).round() as u64);
    let limit = q.limit.min(500);

    let mut matches: Vec<Match> = Vec::new();
    let mut not_imported: Vec<String> = Vec::new();
    // Criteria families per mode index, read once (the page is built after sorting).
    let mut criteria_names: Vec<Vec<String>> = vec![Vec::new(); game.modes.len()];

    for (mi, m) in game.modes.iter().enumerate() {
        if let Some(want) = &q.mode {
            if !m.config.name.eq_ignore_ascii_case(want) {
                continue;
            }
        }
        let Ok(books) = state.books.mode_books(&game, &m.config.name) else {
            not_imported.push(m.config.name.clone());
            continue;
        };
        let idx = &books.index;
        criteria_names[mi] = idx.criteria_names.iter().map(|n| family::criteria_family(n)).collect();

        // Required / forbidden bits for the requested event types (in THIS mode).
        let bits_of = |names: &[String], unknown: &mut bool| -> u64 {
            let mut bits = 0u64;
            for t in names {
                match idx
                    .event_type_names
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(t))
                {
                    Some(i) => bits |= 1u64 << i,
                    None => *unknown = true,
                }
            }
            bits
        };
        let mut unknown_type = false;
        let required_bits = bits_of(&q.event_types, &mut unknown_type);
        if unknown_type {
            continue; // this mode doesn't have that event type → no hits
        }
        // Un type absent du mode n'exclut rien : tous les books le respectent.
        let mut ignored = false;
        let forbidden_bits = bits_of(&q.exclude_event_types, &mut ignored);

        // Filtre par FAMILLE : « 4 scatters » = tout book qui ouvre un bonus à 4 scatters ;
        // « multi_spin » = tout spin sans bonus d'un criteria de cette famille.
        let want = q.criteria.as_deref().map(str::trim).filter(|c| !c.is_empty());
        let want_tier: Option<u8> = want.and_then(|w| {
            if w.eq_ignore_ascii_case(&family::tier_label(0)) {
                Some(0)
            } else {
                w.strip_suffix(" scatters").and_then(|n| n.trim().parse().ok())
            }
        });
        let crit_ok: Vec<bool> = idx
            .criteria_names
            .iter()
            .map(|n| want.is_some_and(|w| family::criteria_family(n).eq_ignore_ascii_case(w)))
            .collect();

        for b in &idx.books {
            if let Some(min) = min_x100 {
                if b.payout_x100 < min {
                    continue;
                }
            }
            if let Some(max) = max_x100 {
                if b.payout_x100 > max {
                    continue;
                }
            }
            if want.is_some() {
                let ok = if b.fs_triggers > 0 {
                    want_tier == Some(b.fs_tier)
                } else {
                    crit_ok.get(b.criteria as usize).copied().unwrap_or(false)
                };
                if !ok {
                    continue;
                }
            }
            if required_bits != 0 && (b.event_bits & required_bits) != required_bits {
                continue;
            }
            if forbidden_bits != 0 && (b.event_bits & forbidden_bits) != 0 {
                continue;
            }
            if !q.bonus.keeps(b.fs_triggers) {
                continue;
            }
            if let Some(minfs) = q.min_fs_triggers {
                if b.fs_triggers < minfs {
                    continue;
                }
            }
            if let Some(maxfs) = q.max_fs_triggers {
                if b.fs_triggers > maxfs {
                    continue;
                }
            }
            if q.min_retriggers.is_some_and(|min| b.fs_retriggers < min)
                || q.max_retriggers.is_some_and(|max| b.fs_retriggers > max)
            {
                continue;
            }
            matches.push(Match {
                mode: mi,
                sim_id: b.sim_id,
                payout_x100: b.payout_x100,
                criteria: b.criteria,
                fs_triggers: b.fs_triggers,
                fs_retriggers: b.fs_retriggers,
                fs_tier: b.fs_tier,
            });
        }
    }

    let total = matches.len();
    // Only the requested page is shown: partition the first offset+limit
    // matches out, then sort those — not the hundreds of thousands a broad
    // search returns. The tie-break is total, so the page is deterministic.
    let order: Option<fn(&Match, &Match) -> std::cmp::Ordering> = match q.sort {
        SearchSort::PayoutDesc => Some(|a, b| {
            b.payout_x100.cmp(&a.payout_x100).then(a.sim_id.cmp(&b.sim_id)).then(a.mode.cmp(&b.mode))
        }),
        SearchSort::PayoutAsc => Some(|a, b| {
            a.payout_x100.cmp(&b.payout_x100).then(a.sim_id.cmp(&b.sim_id)).then(a.mode.cmp(&b.mode))
        }),
        SearchSort::Id => None,
    };
    if let Some(order) = order {
        let keep = q.offset.saturating_add(limit);
        if keep < matches.len() {
            matches.select_nth_unstable_by(keep, order);
            matches.truncate(keep);
        }
        matches.sort_unstable_by(order);
    }

    let hits: Vec<SearchHit> = matches
        .iter()
        .skip(q.offset)
        .take(limit)
        .map(|h| {
            let m = &game.modes[h.mode];
            // Weight per sim_id for the "1 in N" frequency of hits.
            let total_weight = m.lut.total_weight as f64;
            let weight = m.lut.row(h.sim_id).map_or(0, |r| r.weight);
            let criteria = if h.fs_triggers > 0 {
                Some(family::tier_label(h.fs_tier))
            } else {
                criteria_names[h.mode].get(h.criteria as usize).cloned()
            };
            SearchHit {
                mode: m.config.name.clone(),
                sim_id: h.sim_id,
                payout_x100: h.payout_x100,
                criteria,
                fs_triggers: h.fs_triggers,
                fs_retriggers: h.fs_retriggers,
                one_in: if weight > 0 {
                    total_weight / weight as f64
                } else {
                    f64::INFINITY
                },
            }
        })
        .collect();

    Json(json!({
        "total": total,
        "hits": hits,
        "notImported": not_imported,
    }))
}
