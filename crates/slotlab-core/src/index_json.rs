use crate::model::{GameIndex, ModeConfig};
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("invalid index.json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("index.json without any mode")]
    Empty,
}

/// Raw shape of index.json — `cost` accepted as int OR float (the math-sdk
/// sometimes writes `1.0`, the real RGS accepts it).
#[derive(Deserialize)]
struct RawIndex {
    modes: Vec<RawMode>,
}

#[derive(Deserialize)]
struct RawMode {
    name: String,
    cost: serde_json::Number,
    events: String,
    weights: String,
}

/// Tolerant parse: float cost accepted, case aliases deduplicated
/// (e.g. jammi declares `base` AND `BASE` pointing to the same files —
/// historical LGS workaround; we keep a single entry + aliases).
pub fn parse(bytes: &[u8]) -> Result<GameIndex, IndexError> {
    let raw: RawIndex = serde_json::from_slice(bytes)?;
    if raw.modes.is_empty() {
        return Err(IndexError::Empty);
    }
    let mut modes: Vec<ModeConfig> = Vec::new();
    for m in raw.modes {
        if let Some(existing) = modes
            .iter_mut()
            .find(|e| e.events == m.events && e.weights == m.weights)
        {
            if !existing.name.eq_ignore_ascii_case(&m.name)
                || existing.name == m.name
            {
                // same exact name repeated: ignore silently
            }
            if existing.name != m.name
                && !existing.aliases.contains(&m.name)
            {
                existing.aliases.push(m.name);
            }
            continue;
        }
        modes.push(ModeConfig {
            name: m.name,
            cost: m.cost.as_f64().unwrap_or(1.0),
            cost_is_int: m.cost.is_i64() || m.cost.is_u64(),
            events: m.events,
            weights: m.weights,
            aliases: Vec::new(),
        });
    }
    Ok(GameIndex { modes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cost_float_and_int() {
        let idx = parse(
            br#"{"modes":[
                {"name":"BASE","cost":1,"events":"books_BASE.jsonl.zst","weights":"lookUpTable_BASE_0.csv"},
                {"name":"BONUS","cost":250.0,"events":"books_BONUS.jsonl.zst","weights":"lookUpTable_BONUS_0.csv"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(idx.modes.len(), 2);
        assert_eq!(idx.modes[0].cost, 1.0);
        assert!(idx.modes[0].cost_is_int);
        assert_eq!(idx.modes[1].cost, 250.0);
        assert!(!idx.modes[1].cost_is_int);
    }

    #[test]
    fn dedup_case_aliases() {
        let idx = parse(
            br#"{"modes":[
                {"name":"base","cost":1,"events":"books_base.jsonl.zst","weights":"lut_base.csv"},
                {"name":"BASE","cost":1,"events":"books_base.jsonl.zst","weights":"lut_base.csv"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(idx.modes.len(), 1);
        assert_eq!(idx.modes[0].aliases, vec!["BASE".to_string()]);
        assert!(idx.resolve_mode("Base").is_some());
    }
}
