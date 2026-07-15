use crate::persist::{self, Config, GameEntry};
use anyhow::{bail, Context, Result};
use slotlab_core::model::{GameIndex, Lut, ModeConfig};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

/// A loaded mode: its config + its LUT (with prefix-sums ready for drawing).
pub struct LoadedMode {
    pub config: ModeConfig,
    pub lut: Arc<Lut>,
}

pub struct LoadedGame {
    pub entry: GameEntry,
    pub index: GameIndex,
    pub modes: Vec<LoadedMode>,
    /// Fingerprint of the source files (auto invalidation, cf. P2).
    pub fingerprint: u64,
}

impl LoadedGame {
    pub fn resolve_mode(&self, name: &str) -> Option<&LoadedMode> {
        self.modes.iter().find(|m| {
            m.config.name.eq_ignore_ascii_case(name)
                || m.config
                    .aliases
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(name))
        })
    }
}

pub struct Registry {
    games: RwLock<HashMap<String, Arc<LoadedGame>>>,
    config: Mutex<Config>,
}

impl Registry {
    /// Loads the persisted config and tries to load each game;
    /// games that fail to load stay registered (error visible in the UI).
    pub fn from_disk() -> (Self, Vec<(String, String)>) {
        let mut config = persist::load_config();
        // Migration: auto-detect the frontend build for games already
        // registered without a build_dir.
        let mut migrated = false;
        for entry in &mut config.games {
            if entry.build_dir.is_none() {
                entry.build_dir =
                    detect_build_dir(&entry.publish_dir, &entry.name);
                migrated |= entry.build_dir.is_some();
            }
        }
        if migrated {
            let _ = persist::save_config(&config);
        }
        let mut games = HashMap::new();
        let mut errors = Vec::new();
        for entry in &config.games {
            match load_game(entry.clone()) {
                Ok(g) => {
                    games.insert(entry.name.clone(), Arc::new(g));
                }
                Err(e) => errors.push((entry.name.clone(), format!("{e:#}"))),
            }
        }
        (
            Self { games: RwLock::new(games), config: Mutex::new(config) },
            errors,
        )
    }

    pub fn list_entries(&self) -> Vec<GameEntry> {
        self.config.lock().unwrap().games.clone()
    }

    pub fn get(&self, name: &str) -> Option<Arc<LoadedGame>> {
        self.games.read().unwrap().get(name).cloned()
    }

    pub fn register(&self, mut entry: GameEntry) -> Result<Arc<LoadedGame>> {
        if entry.name.trim().is_empty() {
            bail!("empty game name");
        }
        if entry.build_dir.is_none() {
            entry.build_dir = detect_build_dir(&entry.publish_dir, &entry.name);
        }
        let game = Arc::new(load_game(entry.clone())?);
        self.games
            .write()
            .unwrap()
            .insert(entry.name.clone(), game.clone());
        {
            let mut cfg = self.config.lock().unwrap();
            cfg.games.retain(|g| g.name != entry.name);
            cfg.games.push(entry);
            persist::save_config(&cfg)?;
        }
        Ok(game)
    }

    pub fn remove(&self, name: &str) -> Result<bool> {
        let removed = self.games.write().unwrap().remove(name).is_some();
        let mut cfg = self.config.lock().unwrap();
        let before = cfg.games.len();
        cfg.games.retain(|g| g.name != name);
        let changed = cfg.games.len() != before;
        if changed {
            persist::save_config(&cfg)?;
        }
        Ok(removed || changed)
    }

    /// Reloads a game from disk (after a math re-run, for example).
    pub fn reload(&self, name: &str) -> Result<Arc<LoadedGame>> {
        let mut entry = self
            .config
            .lock()
            .unwrap()
            .games
            .iter()
            .find(|g| g.name == name)
            .cloned()
            .context("unknown game")?;
        if entry.build_dir.is_none() {
            entry.build_dir = detect_build_dir(&entry.publish_dir, &entry.name);
            if entry.build_dir.is_some() {
                let mut cfg = self.config.lock().unwrap();
                if let Some(g) = cfg.games.iter_mut().find(|g| g.name == name) {
                    g.build_dir = entry.build_dir.clone();
                }
                let _ = persist::save_config(&cfg);
            }
        }
        let game = Arc::new(load_game(entry)?);
        self.games
            .write()
            .unwrap()
            .insert(name.to_string(), game.clone());
        Ok(game)
    }

    // ---- Bookmarks (persisted in config.json) ----

    pub fn list_bookmarks(&self, game: Option<&str>) -> Vec<crate::persist::Bookmark> {
        self.config
            .lock()
            .unwrap()
            .bookmarks
            .iter()
            .filter(|b| game.is_none_or(|g| b.game == g))
            .cloned()
            .collect()
    }

    pub fn add_bookmark(
        &self,
        game: String,
        mode: String,
        sim_id: u64,
        description: String,
        payout_x100: u64,
    ) -> Result<crate::persist::Bookmark> {
        let mut cfg = self.config.lock().unwrap();
        cfg.next_bookmark_id += 1;
        let bm = crate::persist::Bookmark {
            id: cfg.next_bookmark_id,
            game,
            mode,
            sim_id,
            description,
            payout_x100,
        };
        cfg.bookmarks.push(bm.clone());
        persist::save_config(&cfg)?;
        Ok(bm)
    }

    pub fn remove_bookmark(&self, id: u64) -> Result<bool> {
        let mut cfg = self.config.lock().unwrap();
        let before = cfg.bookmarks.len();
        cfg.bookmarks.retain(|b| b.id != id);
        let changed = cfg.bookmarks.len() != before;
        if changed {
            persist::save_config(&cfg)?;
        }
        Ok(changed)
    }

    /// Updates a game's frontend URL without reloading the math.
    pub fn set_front_url(&self, name: &str, front_url: &str) -> Result<()> {
        let mut cfg = self.config.lock().unwrap();
        let entry = cfg
            .games
            .iter_mut()
            .find(|g| g.name == name)
            .context("unknown game")?;
        entry.front_url = front_url.to_string();
        let entry = entry.clone();
        persist::save_config(&cfg)?;
        drop(cfg);
        // Propagate to the loaded game (without re-parsing LUT/books).
        let mut games = self.games.write().unwrap();
        if let Some(g) = games.get(name).cloned() {
            let mut cloned_entry = g.entry.clone();
            cloned_entry.front_url = entry.front_url;
            games.insert(
                name.to_string(),
                Arc::new(LoadedGame {
                    entry: cloned_entry,
                    index: g.index.clone(),
                    modes: g
                        .modes
                        .iter()
                        .map(|m| LoadedMode {
                            config: m.config.clone(),
                            lut: m.lut.clone(),
                        })
                        .collect(),
                    fingerprint: g.fingerprint,
                }),
            );
        }
        Ok(())
    }
}

/// Fingerprint of a file: size + mtime + xxh3 of the first/last MiB.
pub fn fingerprint_file(path: &Path) -> u64 {
    use std::io::{Read, Seek, SeekFrom};
    use xxhash_rust::xxh3::Xxh3;
    let mut h = Xxh3::new();
    if let Ok(meta) = std::fs::metadata(path) {
        h.update(&meta.len().to_le_bytes());
        if let Ok(mtime) = meta.modified() {
            if let Ok(d) = mtime.duration_since(std::time::UNIX_EPOCH) {
                h.update(&d.as_nanos().to_le_bytes());
            }
        }
        if let Ok(mut f) = std::fs::File::open(path) {
            let mut buf = vec![0u8; 1024 * 1024];
            if let Ok(n) = f.read(&mut buf) {
                h.update(&buf[..n]);
            }
            if meta.len() > 2 * 1024 * 1024 {
                let _ = f.seek(SeekFrom::End(-(1024 * 1024)));
                if let Ok(n) = f.read(&mut buf) {
                    h.update(&buf[..n]);
                }
            }
        }
    }
    h.digest()
}

/// Guesses the frontend static build from publish_dir:
/// `<repo>/math/games/<id>/library/publish_files` →
/// `<repo>/frontend/apps/<game name>/build`, otherwise the single
/// existing `frontend/apps/*/build`.
pub fn detect_build_dir(publish_dir: &Path, game_name: &str) -> Option<std::path::PathBuf> {
    let s = publish_dir.to_string_lossy();
    let root = s.split("/math/games/").next()?;
    let apps = Path::new(root).join("frontend/apps");
    let named = apps.join(game_name).join("build");
    if named.join("index.html").is_file() {
        return Some(named);
    }
    let mut candidates: Vec<std::path::PathBuf> = std::fs::read_dir(&apps)
        .ok()?
        .flatten()
        .map(|e| e.path().join("build"))
        .filter(|p| p.join("index.html").is_file())
        .collect();
    if candidates.len() == 1 {
        return candidates.pop();
    }
    None
}

fn load_game(entry: GameEntry) -> Result<LoadedGame> {
    let dir = &entry.publish_dir;
    if !dir.is_dir() {
        bail!("publish_files directory not found: {}", dir.display());
    }
    let index_bytes = std::fs::read(dir.join("index.json"))
        .with_context(|| format!("reading {}/index.json", dir.display()))?;
    let index = slotlab_core::index_json::parse(&index_bytes)
        .context("parse index.json")?;

    use xxhash_rust::xxh3::Xxh3;
    let mut fp = Xxh3::new();
    fp.update(&fingerprint_file(&dir.join("index.json")).to_le_bytes());

    let mut modes = Vec::new();
    for mode in &index.modes {
        let lut_path = dir.join(&mode.weights);
        let lut_bytes = std::fs::read(&lut_path)
            .with_context(|| format!("reading LUT {}", lut_path.display()))?;
        let lut = slotlab_core::lut::parse(&lut_bytes)
            .with_context(|| format!("parse LUT {}", mode.weights))?;
        fp.update(&fingerprint_file(&lut_path).to_le_bytes());
        fp.update(&fingerprint_file(&dir.join(&mode.events)).to_le_bytes());
        modes.push(LoadedMode { config: mode.clone(), lut: Arc::new(lut) });
    }
    Ok(LoadedGame { entry, index, modes, fingerprint: fp.digest() })
}
