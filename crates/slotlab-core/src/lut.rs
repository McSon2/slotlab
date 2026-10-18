use crate::model::{Lut, LutRow};

#[derive(Debug, thiserror::Error)]
pub enum LutError {
    #[error("line {0}: expected 3 columns `sim_id,weight,payout`")]
    Columns(usize),
    #[error("line {0}: invalid number")]
    Number(usize),
    #[error("empty LUT")]
    Empty,
}

/// Parses a headerless CSV lookUpTable, 3 columns:
/// `sim_id, weight (u64), payout×100 (u64)`. ~110k lines → manual split,
/// no need for a csv crate.
pub fn parse(bytes: &[u8]) -> Result<Lut, LutError> {
    let text = String::from_utf8_lossy(bytes);
    let mut rows = Vec::with_capacity(128 * 1024);
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut cols = line.split(',');
        let (a, b, c) = match (cols.next(), cols.next(), cols.next()) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => return Err(LutError::Columns(i + 1)),
        };
        if cols.next().is_some() {
            return Err(LutError::Columns(i + 1));
        }
        let parse_u64 = |s: &str| s.trim().parse::<u64>();
        let row = LutRow {
            sim_id: parse_u64(a).map_err(|_| LutError::Number(i + 1))?,
            weight: parse_u64(b).map_err(|_| LutError::Number(i + 1))?,
            payout_x100: parse_u64(c).map_err(|_| LutError::Number(i + 1))?,
        };
        rows.push(row);
    }
    if rows.is_empty() {
        return Err(LutError::Empty);
    }
    // Grown by doubling: up to half of it would stay reserved for as long as
    // the game is loaded (a hub holds every mode of every game).
    rows.shrink_to_fit();
    Ok(Lut::new(rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_pick() {
        let lut = parse(b"0,10,250\n1,30,0\n2,60,1000\n").unwrap();
        assert_eq!(lut.total_weight, 100);
        assert_eq!(lut.pick(0).sim_id, 0);
        assert_eq!(lut.pick(9).sim_id, 0);
        assert_eq!(lut.pick(10).sim_id, 1);
        assert_eq!(lut.pick(39).sim_id, 1);
        assert_eq!(lut.pick(40).sim_id, 2);
        assert_eq!(lut.pick(99).sim_id, 2);
    }
}
