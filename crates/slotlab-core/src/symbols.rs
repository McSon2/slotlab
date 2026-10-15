//! Symboles payants LOW / HIGH vus à l'écran.
//!
//! ── La règle studio ─────────────────────────────────────────────────────────
//! Maxime, 2026-10-06 (UNDEAD KING) : les symboles payants révélés à l'écran
//! visent 50 % low / 50 % high, dans le jeu de base comme dans les free spins.
//! Trop peu de highs, et le jeu paraît « numb » (moon_howl, 2026-07-24) : la
//! part des lignes gagnantes d'un symbole suit le CUBE de sa densité.
//!
//! ── Ce qui est compté ───────────────────────────────────────────────────────
//! Chaque plateau RÉVÉLÉ d'un book (`freespins::summarize_with`) :
//! `reveal.board` du math-sdk (jeu de base ou free spins selon `gameType`),
//! `GridSnapshot.cols` du format `actions` (free spins entre `BonusGames` et
//! `FreeSpinsWin`). Les tumbles ne sont pas recomptés : c'est le premier regard
//! sur le spin. Spéciaux (wild, scatter, bonus…) exclus.
//!
//! ── Low ou high ─────────────────────────────────────────────────────────────
//! Les noms du math-sdk le disent : `H1`…`Hn` high, `L1`…`Ln` low. Un jeu dont
//! les books nomment autrement leurs symboles (UNDEAD KING : `P1`…`P9`) est lu
//! à travers la table de son front : `math.symbols` d'un `game-config.json`
//! (`{"P1": "L5", …, "P9": "H1"}`). Sans l'un ni l'autre, rien n'est compté et
//! la règle ne tourne pas.

use crate::provenance::BookFacts;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymClass {
    Low,
    High,
}

/// Comment lire les noms des symboles d'un jeu.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SymbolMap {
    /// Nom dans les books → nom math-sdk (`P1` → `L5`).
    aliases: HashMap<String, String>,
}

impl SymbolMap {
    /// La table `math.symbols` d'un `game-config.json` de front ; `None` si le
    /// fichier n'en porte pas.
    pub fn from_front_config(bytes: &[u8]) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
        let table = v.get("math")?.get("symbols")?.as_object()?;
        let aliases: HashMap<String, String> = table
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
            .collect();
        (!aliases.is_empty()).then_some(Self { aliases })
    }

    pub fn is_empty(&self) -> bool {
        self.aliases.is_empty()
    }

    pub fn classify(&self, name: &str) -> Option<SymClass> {
        classify_sdk(self.aliases.get(name).map_or(name, String::as_str))
    }
}

/// `H1`…`H99` high, `L1`…`L99` low, tout le reste ni l'un ni l'autre.
pub fn classify_sdk(name: &str) -> Option<SymClass> {
    let b = name.as_bytes();
    if !(2..=3).contains(&b.len()) || !b[1..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    match b[0] {
        b'H' | b'h' => Some(SymClass::High),
        b'L' | b'l' => Some(SymClass::Low),
        _ => None,
    }
}

/// Symboles payants révélés par un book, hors bonus et en free spins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SymbolTally {
    pub low: u32,
    pub high: u32,
    pub low_fs: u32,
    pub high_fs: u32,
}

impl SymbolTally {
    pub fn add(&mut self, class: SymClass, n: u32, free_spins: bool) {
        let slot = match (class, free_spins) {
            (SymClass::Low, false) => &mut self.low,
            (SymClass::High, false) => &mut self.high,
            (SymClass::Low, true) => &mut self.low_fs,
            (SymClass::High, true) => &mut self.high_fs,
        };
        *slot = slot.saturating_add(n);
    }
}

/// Symboles payants vus par round, pondérés par la LUT.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Mix {
    pub low: f64,
    pub high: f64,
}

impl Mix {
    /// Part des highs ; `None` sans aucun symbole compté.
    pub fn high_share(&self) -> Option<f64> {
        let n = self.low + self.high;
        (n > 0.0).then(|| self.high / n)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolsSeen {
    pub base: Mix,
    pub free_spins: Mix,
}

/// Ce que le joueur voit d'un mode : symboles low / high par round, pondérés
/// par la LUT. `None` : aucun book du mode n'a de symbole classé (books sans
/// plateau, ou noms inconnus).
pub fn seen(facts: &[BookFacts]) -> Option<SymbolsSeen> {
    let mut w = 0.0_f64;
    let mut acc = [0.0_f64; 4];
    for f in facts {
        let fw = f.weight as f64;
        w += fw;
        for (a, n) in acc.iter_mut().zip([f.sym_low, f.sym_high, f.sym_low_fs, f.sym_high_fs]) {
            *a += fw * n as f64;
        }
    }
    if !(w > 0.0) || acc.iter().all(|&a| a == 0.0) {
        return None;
    }
    Some(SymbolsSeen {
        base: Mix { low: acc[0] / w, high: acc[1] / w },
        free_spins: Mix { low: acc[2] / w, high: acc[3] / w },
    })
}

/// Anticipations vues par le joueur — rouleaux qui ralentissent pour le scatter
/// manquant sur le spin payé —, en part des rounds, pondérées par la LUT.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnticipationSeen {
    /// Part des rounds dont le spin payé a une anticipation.
    pub all: f64,
    /// Part des rounds avec une anticipation et SANS bonus : le near miss
    /// ressenti, forcé ou naturel (BUBBLE HEX, 2026-10-09 : la « Feature
    /// nearmiss » n'en voyait que la moitié).
    pub without_bonus: f64,
    /// Part des rounds à bonus annoncés par une anticipation ; `None` : aucun
    /// bonus dans le mode.
    pub bonus_announced: Option<f64>,
}

/// Anticipations d'un mode ; `None` : aucun book n'en porte (jeu sans
/// anticipation, ou books d'un format qui ne la dit pas).
pub fn anticipation(facts: &[BookFacts]) -> Option<AnticipationSeen> {
    let (mut w, mut a, mut a_nb, mut b, mut a_b) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    for f in facts {
        let fw = f.weight as f64;
        w += fw;
        if f.bonus {
            b += fw;
        }
        if f.antic {
            a += fw;
            if f.bonus {
                a_b += fw;
            } else {
                a_nb += fw;
            }
        }
    }
    if !(w > 0.0) || a == 0.0 {
        return None;
    }
    Some(AnticipationSeen { all: a / w, without_bonus: a_nb / w, bonus_announced: (b > 0.0).then(|| a_b / b) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anticipation_splits_near_misses_from_announced_bonuses() {
        let book = |weight, antic, bonus| BookFacts { weight, antic, bonus, ..Default::default() };
        // 100 spins : 6 near misses, 2 bonus annoncés sur 4, 88 spins calmes.
        let facts = [book(88, false, false), book(6, true, false), book(2, true, true), book(2, false, true), book(2, false, false)];
        let a = anticipation(&facts).unwrap();
        assert_eq!(a.all, 0.08);
        assert_eq!(a.without_bonus, 0.06);
        assert_eq!(a.bonus_announced, Some(0.5));
        assert!(anticipation(&[book(10, false, true)]).is_none());
    }

    #[test]
    fn sdk_names_classify_themselves() {
        let m = SymbolMap::default();
        assert_eq!(m.classify("H1"), Some(SymClass::High));
        assert_eq!(m.classify("L12"), Some(SymClass::Low));
        for other in ["W", "S", "SC", "H", "HX", "BONUS", "WILD12", "H123", "P1"] {
            assert_eq!(m.classify(other), None, "{other}");
        }
    }

    #[test]
    fn a_front_table_renames_the_book_symbols() {
        let cfg = br#"{"schemaVersion":1,"math":{"symbols":{"P1":"L5","P5":"L1","P6":"H4","P9":"H1"}},"modes":[]}"#;
        let m = SymbolMap::from_front_config(cfg).expect("math.symbols");
        assert_eq!(m.classify("P1"), Some(SymClass::Low));
        assert_eq!(m.classify("P9"), Some(SymClass::High));
        assert_eq!(m.classify("BONUS"), None);
        // Un nom absent de la table garde la lecture math-sdk.
        assert_eq!(m.classify("H2"), Some(SymClass::High));
        assert!(SymbolMap::from_front_config(br#"{"math":{}}"#).is_none());
        assert!(SymbolMap::from_front_config(b"not json").is_none());
    }

    #[test]
    fn seen_weights_each_book_by_the_lut() {
        let book = |weight, sym: [u16; 4]| BookFacts {
            weight,
            sym_low: sym[0],
            sym_high: sym[1],
            sym_low_fs: sym[2],
            sym_high_fs: sym[3],
            ..Default::default()
        };
        // 3 spins de base (20 low / 10 high), 1 bonus (15/15 puis 100/200).
        let facts = [book(3, [20, 10, 0, 0]), book(1, [15, 15, 100, 200])];
        let s = seen(&facts).unwrap();
        assert_eq!(s.base, Mix { low: 18.75, high: 11.25 });
        assert_eq!(s.free_spins, Mix { low: 25.0, high: 50.0 });
        assert_eq!(s.base.high_share(), Some(0.375));
        assert!(seen(&[book(5, [0; 4])]).is_none());
    }
}
