//! Relevé des mécaniques d'un mode (`slotlab_core::mechanics`), par book.
//!
//! Il coûtait les deux tiers de l'import (une seconde lecture de chaque
//! événement, champ par champ) pour un seul lecteur, l'onglet Mechanics :
//! il est désormais calculé la première fois qu'on ouvre cet onglet pour un
//! mode, en parallèle sur tous les cœurs grâce aux points de reprise des
//! books, puis gardé à côté de l'index (`books_<MODE>.mech`).

use super::store::ModeBooks;
use anyhow::Result;
use serde::Deserialize;
use serde_json::value::RawValue;
use slotlab_core::mechanics::{scan_book_with, MechIndex, MAX_VALUES};
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;

/// Les événements en texte brut : le relevé ne construit rien d'autre.
#[derive(Deserialize)]
struct BookEventsRaw<'a> {
    #[serde(default, borrow)]
    events: Vec<&'a RawValue>,
}

/// Facettes et valeurs, identifiants LOCAUX. Une facette qui dépasse
/// `MAX_VALUES` valeurs est marquée continue : elle sera écartée partout.
#[derive(Default)]
struct MechLocal {
    facets: Vec<String>,
    facet_ids: HashMap<String, u32>,
    over: Vec<bool>,
    nvals: Vec<usize>,
    values: Vec<(u32, String)>,
    /// Per facet id: value → value id. Looked up by `&str`, so the hundreds of
    /// millions of occurrences a mode holds allocate nothing once seen.
    value_ids: Vec<HashMap<String, u32>>,
}

impl MechLocal {
    fn facet_id(&mut self, facet: &str) -> u32 {
        if let Some(&f) = self.facet_ids.get(facet) {
            return f;
        }
        let f = self.facets.len() as u32;
        self.facets.push(facet.to_string());
        self.facet_ids.insert(facet.to_string(), f);
        self.over.push(false);
        self.nvals.push(0);
        self.value_ids.push(HashMap::new());
        f
    }

    fn value_id(&mut self, facet: &str, value: &str) -> Option<u32> {
        let f = self.facet_id(facet);
        if self.over[f as usize] {
            return None;
        }
        if let Some(&v) = self.value_ids[f as usize].get(value) {
            return Some(v);
        }
        if self.nvals[f as usize] >= MAX_VALUES {
            self.over[f as usize] = true;
            return None;
        }
        self.nvals[f as usize] += 1;
        let v = self.values.len() as u32;
        self.values.push((f, value.to_string()));
        self.value_ids[f as usize].insert(value.to_string(), v);
        Some(v)
    }
}

/// Ce qu'une tranche de books a relevé : ses tables locales, et pour chaque
/// book (index dans `ModeBookIndex::books`) ses occurrences (valeur locale, nombre).
#[derive(Default)]
struct Part {
    mech: MechLocal,
    books: Vec<(u32, Vec<(u32, u16)>)>,
}

/// Le relevé de tous les books d'un mode. `done` compte les books lus.
pub fn build(books: &ModeBooks, done: Option<&AtomicU64>) -> Result<MechIndex> {
    let parts: Vec<Part> = books.par_fold(
        || vec![Part::default()],
        |parts, i, line| {
            let part = parts.last_mut().expect("one part per stretch");
            let mut ids: Vec<u32> = Vec::new();
            if let Ok(raw) = serde_json::from_slice::<BookEventsRaw>(line) {
                scan_book_with(&raw.events, |f, v| {
                    if let Some(id) = part.mech.value_id(f, v) {
                        ids.push(id);
                    }
                });
            }
            // Occurrences → value ids, counted by run after a sort.
            ids.sort_unstable();
            let mut occ: Vec<(u32, u16)> = Vec::new();
            for &id in &ids {
                match occ.last_mut() {
                    Some((last, n)) if *last == id => *n = n.saturating_add(1),
                    _ => occ.push((id, 1)),
                }
            }
            part.books.push((i as u32, occ));
        },
        |mut a, mut b| {
            a.append(&mut b);
            a
        },
        done,
    )?;

    // Tables locales → globales ; une facette continue dans une tranche l'est
    // pour tout le mode.
    let mut g = MechLocal::default();
    let mut g_over: Vec<bool> = Vec::new();
    let mut per_book: Vec<Vec<(u32, u16)>> = vec![Vec::new(); books.index.books.len()];
    for part in parts {
        let map: Vec<Option<u32>> = part
            .mech
            .values
            .iter()
            .map(|(f, v)| g.value_id(&part.mech.facets[*f as usize], v))
            .collect();
        for (f, name) in part.mech.facets.iter().enumerate() {
            let gf = g.facet_id(name) as usize;
            if g_over.len() <= gf {
                g_over.resize(gf + 1, false);
            }
            g_over[gf] |= part.mech.over[f] || g.over[gf];
        }
        for (i, occ) in part.books {
            per_book[i as usize] = occ
                .iter()
                .filter_map(|&(v, n)| map[v as usize].map(|gv| (gv, n)))
                .collect();
        }
    }
    Ok(compact(&g, &g_over, &per_book))
}

/// Écarte les facettes continues, renumérote le reste, aplatit les occurrences par book.
fn compact(g: &MechLocal, over: &[bool], per_book: &[Vec<(u32, u16)>]) -> MechIndex {
    let keep_facet = |f: u32| !over.get(f as usize).copied().unwrap_or(false);
    let mut facet_new: Vec<Option<u32>> = vec![None; g.facets.len()];
    let mut out = MechIndex::default();
    let mut value_new: Vec<Option<u32>> = vec![None; g.values.len()];
    for (v, (f, value)) in g.values.iter().enumerate() {
        if !keep_facet(*f) {
            continue;
        }
        let nf = *facet_new[*f as usize].get_or_insert_with(|| {
            out.facets.push(g.facets[*f as usize].clone());
            (out.facets.len() - 1) as u32
        });
        value_new[v] = Some(out.values.len() as u32);
        out.values.push((nf, value.clone()));
    }
    out.offsets.reserve(per_book.len() + 1);
    out.offsets.push(0);
    for b in per_book {
        out.entries
            .extend(b.iter().filter_map(|&(v, n)| value_new[v as usize].map(|nv| (nv, n))));
        out.offsets.push(out.entries.len() as u32);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::books::import::{import_mode, ImportProgress};
    use slotlab_core::symbols::SymbolMap;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    #[test]
    fn mechanics_are_counted_per_book() {
        let books = [
            r#"{"id":1,"payoutMultiplier":0,"events":[{"index":0,"type":"sharkFeed","hunts":[{"fish":"F","totalFs":7}]}]}"#,
            r#"{"id":2,"payoutMultiplier":0,"events":[{"index":0,"type":"sharkFeed","hunts":[{"fish":"F","totalFs":8},{"fish":"F","totalFs":9},{"fish":"M","totalFs":10}]},{"index":1,"type":"sharkFeed","hunts":[{"fish":"F","totalFs":10}]}]}"#,
            r#"{"id":3,"payoutMultiplier":0,"events":[{"index":0,"type":"reveal","board":[[{"name":"H1"}]]},{"index":1,"type":"sharkFeed","hunts":[{"fish":"X","totalFs":0}]}]}"#,
        ];
        let jsonl: String = books.iter().map(|b| format!("{b}\n")).collect();
        let dir = std::env::temp_dir().join(format!("slotlab-mech-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("books_BASE.jsonl.zst");
        std::fs::write(&src, zstd::bulk::compress(jsonl.as_bytes(), 3).unwrap()).unwrap();
        import_mode(
            &src,
            &dir,
            "BASE",
            1,
            Arc::new(ImportProgress::default()),
            &SymbolMap::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let mb = ModeBooks::open(&dir, "BASE", &src).unwrap();
        let mech = build(&mb, None).unwrap();

        // Rangé dans l'ordre des books ; `totalFs` (5 valeurs ici) reste une répartition.
        assert_eq!(mech.offsets.len(), mb.index.books.len() + 1);
        let fish = mech.facets.iter().position(|f| f == "sharkFeed.hunts[].fish").unwrap() as u32;
        let count = |book: usize, value: &str| -> u16 {
            let (a, b) = (mech.offsets[book] as usize, mech.offsets[book + 1] as usize);
            mech.entries[a..b]
                .iter()
                .filter(|(v, _)| mech.values[*v as usize] == (fish, value.to_string()))
                .map(|e| e.1)
                .sum()
        };
        assert_eq!((count(1, "F"), count(1, "M"), count(0, "F"), count(2, "X")), (3, 1, 1, 1));
        assert!(!mech.facets.iter().any(|f| f.contains("board")));
        std::fs::remove_dir_all(&dir).ok();
    }
}
