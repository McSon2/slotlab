//! « Sur N spins, combien de sessions n'atteignent jamais X ? » — exact, lu dans la LUT.
//!
//! La question du joueur qui repart sans rien de gros (BLOOD WATER, 2026-09-13 : une
//! session de 6 000 spins sur quatre ne voyait pas 500x). Les rounds sont
//! indépendants — le RGS tire chaque spin dans la LUT — : si une part `p` des spins
//! paie au moins X, une session de N spins n'en voit aucun avec la probabilité
//! (1 − p)^N. Pas de simulation : le chiffre est exact, et instantané pour n'importe
//! quel N et n'importe quel X.

use crate::model::Lut;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReach {
    pub spins: u32,
    /// Seuil, en multiples de la mise de base (la colonne payout de la LUT).
    pub x: f64,
    /// Part des spins qui paient au moins `x`.
    pub p: f64,
    /// Un spin sur … paie au moins `x` — `None` quand aucun ne le fait.
    pub one_in: Option<f64>,
    /// Nombre moyen de tels spins par session.
    pub expected: f64,
    /// Part des sessions qui ne paient JAMAIS `x` ou plus : leur meilleur gain reste
    /// en dessous.
    pub never: f64,
}

pub fn session_reach(lut: &Lut, spins: u32, x: f64) -> SessionReach {
    let at = x * 100.0 - 1e-9;
    let hit: u128 = lut
        .rows
        .iter()
        .filter(|r| r.payout_x100 as f64 >= at)
        .map(|r| r.weight as u128)
        .sum();
    let total = lut.total_weight.max(1) as f64;
    let p = (hit as f64 / total).min(1.0);
    // (1 − p)^N sans perdre les petits p : exp(N · ln(1 − p)).
    let never = if p >= 1.0 { 0.0 } else { (spins as f64 * (-p).ln_1p()).exp() };
    SessionReach {
        spins,
        x,
        p,
        one_in: (p > 0.0).then(|| 1.0 / p),
        expected: p * spins as f64,
        never,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LutRow;

    fn lut() -> Lut {
        Lut::new(vec![
            LutRow { sim_id: 0, weight: 50, payout_x100: 0 },
            LutRow { sim_id: 1, weight: 25, payout_x100: 100 },
            LutRow { sim_id: 2, weight: 25, payout_x100: 1_000 },
        ])
    }

    #[test]
    fn a_quarter_of_spins_reach_10x() {
        let r = session_reach(&lut(), 4, 10.0);
        assert!((r.p - 0.25).abs() < 1e-12);
        assert!((r.one_in.unwrap() - 4.0).abs() < 1e-12);
        assert!((r.expected - 1.0).abs() < 1e-12);
        assert!((r.never - 0.75f64.powi(4)).abs() < 1e-12);
    }

    #[test]
    fn the_threshold_is_inclusive() {
        // 1x exactly counts as reaching 1x.
        let r = session_reach(&lut(), 1, 1.0);
        assert!((r.p - 0.5).abs() < 1e-12);
    }

    #[test]
    fn nothing_reaches_beyond_the_top() {
        let r = session_reach(&lut(), 6_000, 20.0);
        assert_eq!(r.p, 0.0);
        assert_eq!(r.one_in, None);
        assert_eq!(r.never, 1.0);
    }
}
