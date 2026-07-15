use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A registered game (persisted in config.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameEntry {
    /// Game slug, used in URLs (/api/rgs/<name>/...).
    pub name: String,
    /// Absolute path to .../library/publish_files/
    pub publish_dir: PathBuf,
    /// Custom frontend URL (Vite dev). Empty = build served by SlotLab.
    #[serde(default)]
    pub front_url: String,
    /// Game static build, auto-detected from publish_dir
    /// (`<repo>/frontend/apps/<game>/build`), served under /games/<name>/.
    #[serde(default)]
    pub build_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: u64,
    pub game: String,
    pub mode: String,
    pub sim_id: u64,
    #[serde(default)]
    pub description: String,
    /// Multiplier ×100 at bookmark time (display).
    #[serde(default)]
    pub payout_x100: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub games: Vec<GameEntry>,
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
    #[serde(default)]
    pub next_bookmark_id: u64,
}

/// `~/Library/Application Support/slotlab` (macOS) or `~/.slotlab` otherwise.
pub fn app_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    if cfg!(target_os = "macos") {
        Path::new(&home).join("Library/Application Support/slotlab")
    } else {
        Path::new(&home).join(".slotlab")
    }
}

pub fn config_path() -> PathBuf {
    app_dir().join("config.json")
}

pub fn cache_dir() -> PathBuf {
    app_dir().join("cache")
}

pub fn load_config() -> Config {
    match std::fs::read(config_path()) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            tracing::warn!("config.json unreadable ({e}), starting from scratch");
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

/// Atomic write: tmp + rename (never a half-written config).
pub fn save_config(config: &Config) -> Result<()> {
    let dir = app_dir();
    std::fs::create_dir_all(&dir).context("creating app directory")?;
    let tmp = dir.join("config.json.tmp");
    let bytes = serde_json::to_vec_pretty(config)?;
    std::fs::write(&tmp, bytes).context("writing tmp config")?;
    std::fs::rename(&tmp, config_path()).context("rename config")?;
    Ok(())
}
