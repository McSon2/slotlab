//! « Quelqu'un qui ne fait que 100 spins sur la machine repart avec combien ? »
//! (client, TOXIC SPINS 2026-09-28 : le ressenti « sec » se juge sur une session
//! courte, pas sur le RTP — la moyenne vient des rares gros gains).
//!
//! La distribution EXACTE du total rendu par N rounds indépendants d'un mode,
//! lue dans la lookup table : les rounds sont tirés indépendamment par le RGS,
//! le total est donc la convolution N fois de la loi d'un round. Elle se calcule
//! par transformée de Fourier (la puissance N de la fonction caractéristique)
//! sur une grille au dixième de mise — aucune simulation, aucun aléa, le même
//! chiffre à chaque lecture, en quelques millisecondes.
//!
//! Tout est en MISES du mode (« stakes ») : une session de 100 rounds d'un
//! ante à 3x engage 300 mises de base, et rendre 100 mises du mode = finir à
//! l'équilibre. Le même calcul tourne sur le marché
//! (`market::figures_of`) : la médiane d'une session courte se compare aux
//! pairs comme les autres figures. Marché des jeux de base, 2026-09-28 :
//! médiane 36 mises rendues sur 100, p25 20, p75 74 ; 19 % de sessions
//! gagnantes ; 24 % rendent moins de 20.

use serde::Serialize;

/// Rounds d'une session courte.
pub const SPINS: u32 = 100;
/// Pas de la grille, en mises.
const STEP: f64 = 0.1;
/// Un round qui rend plus que ça à lui seul fait une session gagnante quoi
/// qu'il arrive : tout ce qui dépasse est ramené sur cette case. Le total
/// d'une session reste exact partout sous ce plafond.
const CAP_PER_SPIN: f64 = 2.0 * SPINS as f64;
/// Session « catastrophe » : moins de ce taux de la mise totale rendu.
pub const BUST_RETURN: f64 = 0.2;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortSession {
    pub spins: u32,
    /// Total rendu par la session, en mises du mode : quartiles.
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
    /// Part des sessions qui finissent gagnantes (au moins la mise totale rendue).
    pub win: f64,
    /// Part des sessions qui rendent moins de `BUST_RETURN` × la mise totale.
    pub bust: f64,
}

/// `dist` : (gain d'un round en MISES du mode, poids). `None` sans masse.
pub fn short_session(dist: &[(f64, f64)], spins: u32) -> Option<ShortSession> {
    let w: f64 = dist.iter().filter(|d| d.1 > 0.0).map(|d| d.1).sum();
    if !(w > 0.0) || spins == 0 {
        return None;
    }
    let bins = (CAP_PER_SPIN / STEP).round() as usize + 1;
    let mut pmf = vec![0.0_f64; bins];
    for &(x, ww) in dist.iter().filter(|d| d.1 > 0.0) {
        // Ties to even, like Python's round(): an ante at 2x puts every other payout
        // exactly between two cells, and rounding them all up biased the total.
        let k = ((x.max(0.0) / STEP).round_ties_even() as usize).min(bins - 1);
        pmf[k] += ww / w;
    }
    let total = sum_pmf(&pmf, spins);
    let cum = |k_end: usize| total[..k_end.min(total.len())].iter().sum::<f64>();
    let mass: f64 = total.iter().sum();
    let quantile = |q: f64| {
        let target = q * mass;
        let mut acc = 0.0;
        for (k, &p) in total.iter().enumerate() {
            acc += p;
            if acc >= target - 1e-12 {
                return k as f64 * STEP;
            }
        }
        (total.len() - 1) as f64 * STEP
    };
    let at_least = |stakes: f64| (mass - cum((stakes / STEP).round() as usize)) / mass;
    Some(ShortSession {
        spins,
        p25: quantile(0.25),
        median: quantile(0.5),
        p75: quantile(0.75),
        win: at_least(spins as f64),
        bust: 1.0 - at_least(BUST_RETURN * spins as f64),
    })
}

/// Loi de la somme de `n` tirages de `pmf` (grille commune), par FFT.
fn sum_pmf(pmf: &[f64], n: u32) -> Vec<f64> {
    let support = (pmf.len() - 1) * n as usize + 1;
    let size = support.next_power_of_two();
    let mut a: Vec<(f64, f64)> = pmf.iter().map(|&p| (p, 0.0)).collect();
    a.resize(size, (0.0, 0.0));
    fft(&mut a, false);
    for z in &mut a {
        *z = cpow(*z, n);
    }
    fft(&mut a, true);
    let mut out: Vec<f64> = a[..support].iter().map(|z| z.0.max(0.0)).collect();
    let s: f64 = out.iter().sum();
    if s > 0.0 {
        for v in &mut out {
            *v /= s;
        }
    }
    out
}

fn cmul(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
}

/// z^n par carrés successifs (exact à la précision double, |z| ≤ 1).
fn cpow(z: (f64, f64), mut n: u32) -> (f64, f64) {
    let mut base = z;
    let mut acc = (1.0, 0.0);
    while n > 0 {
        if n & 1 == 1 {
            acc = cmul(acc, base);
        }
        base = cmul(base, base);
        n >>= 1;
    }
    acc
}

/// Transformée de Fourier discrète, radix 2 en place (`a.len()` puissance de 2) ;
/// `inverse` divise par la taille.
fn fft(a: &mut [(f64, f64)], inverse: bool) {
    let n = a.len();
    debug_assert!(n.is_power_of_two());
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = 2.0 * std::f64::consts::PI / len as f64 * if inverse { 1.0 } else { -1.0 };
        let wlen = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let mut w = (1.0, 0.0);
            for k in 0..len / 2 {
                let u = a[start + k];
                let v = cmul(a[start + k + len / 2], w);
                a[start + k] = (u.0 + v.0, u.1 + v.1);
                a[start + k + len / 2] = (u.0 - v.0, u.1 - v.1);
                w = cmul(w, wlen);
            }
        }
        len <<= 1;
    }
    if inverse {
        for z in a.iter_mut() {
            z.0 /= n as f64;
            z.1 /= n as f64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_coin_flip_sums_to_a_binomial() {
        // Un round rend 0 ou 2 mises à parts égales : sur 100 rounds, le total
        // suit 2 × Binomiale(100, ½) — médiane 100, p25 ≈ 94, p75 ≈ 106.
        let s = short_session(&[(0.0, 1.0), (2.0, 1.0)], 100).unwrap();
        assert!((s.median - 100.0).abs() < 1e-9, "{}", s.median);
        assert!((s.p25 - 94.0).abs() < 1e-9 && (s.p75 - 106.0).abs() < 1e-9, "{} {}", s.p25, s.p75);
        // P(≥ 50 faces sur 100) = 0,5398.
        assert!((s.win - 0.5398).abs() < 1e-3, "{}", s.win);
        assert!(s.bust.abs() < 1e-9);
    }

    #[test]
    fn a_sure_loss_and_a_sure_win() {
        let dead = short_session(&[(0.0, 1.0)], 100).unwrap();
        assert_eq!((dead.median, dead.win, dead.bust), (0.0, 0.0, 1.0));
        let rich = short_session(&[(1.5, 3.0)], 100).unwrap();
        assert!((rich.median - 150.0).abs() < 1e-9 && (rich.win - 1.0).abs() < 1e-9 && rich.bust.abs() < 1e-9);
    }

    #[test]
    fn huge_wins_are_capped_without_losing_the_winners() {
        // 1 round sur 1 000 rend 100 000 mises : chacune de ces sessions est gagnante.
        let s = short_session(&[(0.0, 999.0), (100_000.0, 1.0)], 100).unwrap();
        let p_hit = 1.0 - 0.999f64.powi(100);
        assert!((s.win - p_hit).abs() < 1e-9, "{} {}", s.win, p_hit);
        assert_eq!(s.median, 0.0);
    }

    #[test]
    fn the_fft_matches_a_direct_convolution_at_full_size() {
        // A realistic round: mostly dead, small wins, a rare big one — summed 100 times
        // by FFT (2^18 points) and by plain repeated convolution.
        let pmf_src = [(0.0, 0.62), (0.3, 0.2), (1.3, 0.12), (5.7, 0.05), (48.0, 0.009), (199.0, 0.001)];
        let bins = (CAP_PER_SPIN / STEP).round() as usize + 1;
        let mut pmf = vec![0.0_f64; bins];
        for &(x, p) in &pmf_src {
            pmf[(x / STEP).round() as usize] += p;
        }
        let fast = sum_pmf(&pmf, 100);
        let mut slow = vec![1.0_f64];
        let nz: Vec<(usize, f64)> = pmf.iter().enumerate().filter(|(_, &p)| p > 0.0).map(|(k, &p)| (k, p)).collect();
        for _ in 0..100 {
            let mut next = vec![0.0_f64; slow.len() + bins - 1];
            for (i, &a) in slow.iter().enumerate() {
                if a == 0.0 {
                    continue;
                }
                for &(k, p) in &nz {
                    next[i + k] += a * p;
                }
            }
            slow = next;
        }
        assert_eq!(fast.len(), slow.len());
        let worst = fast.iter().zip(&slow).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        assert!(worst < 1e-9, "worst absolute gap {worst}");
        let cum = |v: &[f64], k: usize| v[..k].iter().sum::<f64>();
        assert!((cum(&fast, 1000) - cum(&slow, 1000)).abs() < 1e-9);
    }

    #[test]
    fn nothing_without_mass() {
        assert!(short_session(&[(5.0, 0.0)], 100).is_none());
        assert!(short_session(&[], 100).is_none());
    }
}
