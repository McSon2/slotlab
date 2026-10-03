use serde::{Deserialize, Serialize};

/// A bet mode as declared by the `index.json` of publish_files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeConfig {
    /// Canonical name as written in index.json (e.g. "BASE" or "base").
    pub name: String,
    /// Mode cost in bet multiples. Stake's official index.json schema declares
    /// this as `<float>` (its examples use 1.0 / 100.0), so fractional costs
    /// such as 1.5x are legal — the `cost_format` check only rejects values the
    /// fixed-point bet arithmetic can't hold (≤ 0, or finer than a thousandth).
    pub cost: f64,
    /// true if index.json declared the cost as a JSON integer (`1`, not `1.0`).
    /// Informational only: every published studio game writes integers, so this
    /// flags a deviation from that convention — it is NOT an RGS requirement.
    #[serde(default)]
    pub cost_is_int: bool,
    /// Books file (jsonl.zst), relative to the publish_files folder.
    pub events: String,
    /// Lookup table file (csv), relative to the publish_files folder.
    pub weights: String,
    /// Other index.json names pointing to the same files (case aliases).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameIndex {
    pub modes: Vec<ModeConfig>,
}

impl GameIndex {
    /// Case-insensitive resolution (the frontend sends "BASE",
    /// some index.json files declare "base").
    pub fn resolve_mode(&self, name: &str) -> Option<&ModeConfig> {
        self.modes
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .or_else(|| {
                self.modes.iter().find(|m| {
                    m.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
                })
            })
    }
}

/// One row of lookUpTable_<MODE>_0.csv: `sim_id, weight, payout×100`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LutRow {
    pub sim_id: u64,
    pub weight: u64,
    /// Win multiplier × 100 (250 = 2.5x). In units of the mode's COST.
    pub payout_x100: u64,
}

/// Loaded LUT + prefix sums for weighted sampling in O(log n).
#[derive(Debug, Clone)]
pub struct Lut {
    pub rows: Vec<LutRow>,
    /// prefix[i] = Σ weights[0..=i]
    pub prefix: Vec<u64>,
    pub total_weight: u64,
    /// Rows in ascending sim_id order (the math-sdk writes them so): lookups
    /// by id are then a binary search instead of a scan of every row.
    sorted_by_id: bool,
}

impl Lut {
    pub fn new(rows: Vec<LutRow>) -> Self {
        let mut prefix = Vec::with_capacity(rows.len());
        let mut acc: u64 = 0;
        for r in &rows {
            acc = acc
                .checked_add(r.weight)
                .expect("sum of LUT weights overflows u64");
            prefix.push(acc);
        }
        let sorted_by_id = rows.windows(2).all(|w| w[0].sim_id < w[1].sim_id);
        Self { rows, prefix, total_weight: acc, sorted_by_id }
    }

    /// The row of a sim_id.
    pub fn row(&self, sim_id: u64) -> Option<&LutRow> {
        if self.sorted_by_id {
            let i = self.rows.binary_search_by_key(&sim_id, |r| r.sim_id).ok()?;
            Some(&self.rows[i])
        } else {
            self.rows.iter().find(|r| r.sim_id == sim_id)
        }
    }

    /// Samples a row according to the weights; `roll` must be in [0, total_weight).
    pub fn pick(&self, roll: u64) -> &LutRow {
        let i = self.prefix.partition_point(|&p| p <= roll);
        &self.rows[i]
    }
}
