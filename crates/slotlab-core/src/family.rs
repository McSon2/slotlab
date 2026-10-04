//! Ce que le joueur vit, pas comment le book a été simulé.
//!
//! Un criteria est un seau de SIMULATION : les maths en créent plusieurs pour
//! une même chose vue en jeu — `multi_spin`, `multi_spin_0`, `multi_spin_big`,
//! `multi_spin_cap` sont toutes la MULTI SPIN ; `freegame_4`, `rampage_tail`,
//! `freegame_4__t16` sont tous un bonus à 4 scatters. Les afficher séparément
//! noie l'analyse (Toxic Spins, 2026-10-04 : 15 lignes « 4 scatters ·
//! freegame_4__tN »). SlotLab range donc chaque book dans une FAMILLE :
//!   · un book qui ouvre un bonus → son palier (« 3 scatters »), quel que soit
//!     son criteria ;
//!   · sinon → le nom de son criteria sans suffixe de simulation (`__…`,
//!     `_0`, `_big`, `_cap`, `_tail`, `_mid`, `_court`…), les spins ordinaires
//!     (`0`, `basegame`, `base`…) réunis sous « base game ».
//!
//! Les noms exacts restent dans les books : seuls l'affichage et les filtres
//! regroupent.

/// Suffixes qui disent comment un book a été tiré, pas ce qu'il est.
const SIM_SUFFIXES: &[&str] = &["0", "zero", "big", "cap", "tail", "mid", "court", "small", "win", "loss"];

/// Spins ordinaires du jeu de base, quel que soit leur seau.
const PLAIN: &[&str] = &["0", "zero", "base", "basegame", "base_game", "plain"];

pub const BASE_GAME: &str = "base game";

/// Famille d'un criteria de spin SANS bonus.
pub fn criteria_family(name: &str) -> String {
    let mut c = name.split("__").next().unwrap_or(name).to_string();
    if PLAIN.contains(&c.as_str()) {
        return BASE_GAME.to_string();
    }
    loop {
        let Some((head, tail)) = c.rsplit_once('_') else { break };
        if head.is_empty() || !SIM_SUFFIXES.contains(&tail) {
            break;
        }
        c = head.to_string();
    }
    if PLAIN.contains(&c.as_str()) {
        BASE_GAME.to_string()
    } else {
        c
    }
}

/// Libellé d'un palier de bonus (positions du déclenchement ; 0 = inconnu).
pub fn tier_label(tier: u8) -> String {
    if tier == 0 {
        "untiered bonus".to_string()
    } else {
        format!("{tier} scatters")
    }
}

/// Famille d'un book : son palier s'il ouvre un bonus, sinon celle de son criteria.
pub fn book_family(criteria: Option<&str>, bonus: bool, tier: u8) -> String {
    if bonus {
        tier_label(tier)
    } else {
        criteria.map_or_else(|| "(none)".to_string(), criteria_family)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_of_toxic_spins() {
        for (c, f) in [
            ("0", BASE_GAME),
            ("basegame", BASE_GAME),
            ("base_big", BASE_GAME),
            ("nearmiss", "nearmiss"),
            ("nearmiss_0", "nearmiss"),
            ("multi_spin", "multi_spin"),
            ("multi_spin_0", "multi_spin"),
            ("multi_spin_big", "multi_spin"),
            ("multi_spin_cap", "multi_spin"),
            ("degen_tail", "degen"),
            ("degen_0", "degen"),
            ("mystery_s2_0", "mystery_s2"),
            ("freegame_4__t16", "freegame_4"),
            ("wincap", "wincap"),
        ] {
            assert_eq!(criteria_family(c), f, "{c}");
        }
        assert_eq!(book_family(Some("rampage_tail"), true, 4), "4 scatters");
        assert_eq!(book_family(Some("multi_spin_cap"), false, 0), "multi_spin");
    }
}
