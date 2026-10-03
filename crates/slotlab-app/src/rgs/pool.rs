//! Tirage restreint : le RGS tire un book au hasard, pondéré par la LUT, mais
//! seulement parmi ceux d'un mode qui ouvrent un bonus (palier optionnel).
//!
//! Pour enchaîner des bonus DIFFÉRENTS depuis le jeu de base — là où forcer un
//! bookid rejoue toujours le même. Le tirage garde les proportions naturelles
//! du mode : un bonus 3 scatters sort aussi souvent, relativement à un 4
//! scatters, qu'en jeu réel.

use crate::books::BooksManager;
use crate::registry::LoadedGame;
use anyhow::{ensure, Result};
use serde::Serialize;
use slotlab_core::model::{Lut, LutRow};
use std::collections::BTreeMap;

/// Ce qu'on garde : un mode, et éventuellement un seul palier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolFilter {
    pub mode: String,
    /// Nombre de scatters du déclenchement (`fs_tier`) ; `None` = tout bonus.
    pub tier: Option<u8>,
}

/// Les books éligibles d'un filtre, prêts au tirage pondéré.
pub struct Pool {
    pub filter: PoolFilter,
    /// Empreinte du jeu au moment du calcul : un rechargement de la math
    /// (autre LUT, autres books) oblige à le refaire.
    pub fingerprint: u64,
    pub lut: Lut,
    /// Poids total du mode, pour dire la rareté du pool (« 1 sur X »).
    pub mode_weight: u64,
}

impl Pool {
    pub fn build(books: &BooksManager, game: &LoadedGame, filter: PoolFilter) -> Result<Self> {
        let mode = game
            .resolve_mode(&filter.mode)
            .ok_or_else(|| anyhow::anyhow!("unknown mode: {}", filter.mode))?;
        let index = &books.mode_books(game, &mode.config.name)?.index;
        let rows: Vec<LutRow> = mode
            .lut
            .rows
            .iter()
            .filter(|r| r.weight > 0)
            .filter(|r| {
                index.find(r.sim_id).is_some_and(|b| {
                    b.fs_triggers > 0 && filter.tier.is_none_or(|t| b.fs_tier == t)
                })
            })
            .copied()
            .collect();
        ensure!(
            !rows.is_empty(),
            "no book of {} opens a bonus{}",
            mode.config.name,
            filter.tier.map(|t| format!(" with {t} scatters")).unwrap_or_default()
        );
        Ok(Self {
            filter: PoolFilter { mode: mode.config.name.clone(), tier: filter.tier },
            fingerprint: game.fingerprint,
            lut: Lut::new(rows),
            mode_weight: mode.lut.total_weight,
        })
    }

    pub fn summary(&self) -> PoolSummary {
        PoolSummary {
            mode: self.filter.mode.clone(),
            tier: self.filter.tier,
            books: self.lut.rows.len(),
            one_in: one_in(self.mode_weight, self.lut.total_weight),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolSummary {
    pub mode: String,
    pub tier: Option<u8>,
    /// Books distincts dans le pool.
    pub books: usize,
    /// Rareté du pool en jeu réel : un tour sur `one_in`.
    pub one_in: f64,
}

/// Les choix offerts pour un mode : tout bonus, puis chaque palier connu.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolOptions {
    pub mode: String,
    pub any: PoolSummary,
    /// Paliers croissants ; vide si les déclenchements ne disent pas leurs
    /// positions (palier 0), auquel cas seul « tout bonus » a un sens.
    pub tiers: Vec<PoolSummary>,
}

/// Modes a round of which is a spin, not a purchase — the same line the
/// "feel" rules of the analysis draw (base, hunt/ante). Buys are left out
/// before their index is even opened.
pub const SPIN_MODE_MAX_COST: f64 = 10.0;

/// Every spin mode where the restriction means something: some books open a
/// bonus, and not all of them.
pub fn all_options(books: &BooksManager, game: &LoadedGame) -> Result<Vec<PoolOptions>> {
    let mut modes: Vec<_> = game
        .modes
        .iter()
        .filter(|m| m.config.cost < SPIN_MODE_MAX_COST)
        .collect();
    modes.sort_by(|a, b| a.config.cost.total_cmp(&b.config.cost));
    let mut out = Vec::new();
    for m in modes {
        let o = options(books, game, &m.config.name)?;
        if o.any.books > 0 && o.any.one_in > 1.0 {
            out.push(o);
        }
    }
    Ok(out)
}

pub fn options(books: &BooksManager, game: &LoadedGame, mode_name: &str) -> Result<PoolOptions> {
    let mode = game
        .resolve_mode(mode_name)
        .ok_or_else(|| anyhow::anyhow!("unknown mode: {mode_name}"))?;
    let index = &books.mode_books(game, &mode.config.name)?.index;
    let mut any = (0usize, 0u64);
    let mut by_tier: BTreeMap<u8, (usize, u64)> = BTreeMap::new();
    for r in mode.lut.rows.iter().filter(|r| r.weight > 0) {
        let Some(b) = index.find(r.sim_id) else { continue };
        if b.fs_triggers == 0 {
            continue;
        }
        any.0 += 1;
        any.1 += r.weight;
        if b.fs_tier > 0 {
            let e = by_tier.entry(b.fs_tier).or_default();
            e.0 += 1;
            e.1 += r.weight;
        }
    }
    let total = mode.lut.total_weight;
    let summary = |tier, (books, w): (usize, u64)| PoolSummary {
        mode: mode.config.name.clone(),
        tier,
        books,
        one_in: one_in(total, w),
    };
    Ok(PoolOptions {
        mode: mode.config.name.clone(),
        any: summary(None, any),
        tiers: by_tier.into_iter().map(|(t, v)| summary(Some(t), v)).collect(),
    })
}

fn one_in(total: u64, part: u64) -> f64 {
    if part == 0 {
        0.0
    } else {
        total as f64 / part as f64
    }
}
