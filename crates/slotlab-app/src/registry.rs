use crate::persist::{self, Config, GameEntry};
use anyhow::{bail, Context, Result};
use slotlab_core::model::{GameIndex, Lut, ModeConfig};
use slotlab_core::stats::ModeStats;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

/// A loaded mode: its config + its LUT (with prefix-sums ready for drawing).
pub struct LoadedMode {
    pub config: ModeConfig,
    pub lut: Arc<Lut>,
    /// `stats::analyze` of `lut`, computed on first use. The LUT never changes
    /// in place (a reload builds a new `LoadedMode`), so it is computed once
    /// instead of on every games list, stats, checks and bonus request.
    stats: OnceLock<ModeStats>,
}

impl LoadedMode {
    pub fn new(config: ModeConfig, lut: Arc<Lut>) -> Self {
        Self { config, lut, stats: OnceLock::new() }
    }

    pub fn stats(&self) -> &ModeStats {
        self.stats
            .get_or_init(|| slotlab_core::stats::analyze(&self.lut, self.config.cost))
    }
}

pub struct LoadedGame {
    pub entry: GameEntry,
    pub index: GameIndex,
    pub modes: Vec<LoadedMode>,
    /// Fingerprint of the source files (auto invalidation, cf. P2).
    pub fingerprint: u64,
    /// When these files were read. A `run.py prod` that lands later leaves
    /// every panel showing the previous math (client, WILD CURRENT
    /// 2026-10-05: an hour of feedback on a version already replaced) —
    /// `source_changed` says so, and the watcher reloads.
    pub loaded_at: std::time::SystemTime,
}

impl LoadedGame {
    /// The paths this game is read from: index.json, every lookup table,
    /// every books file.
    pub fn source_paths(&self) -> Vec<std::path::PathBuf> {
        let dir = &self.entry.publish_dir;
        let mut out = vec![dir.join("index.json")];
        for m in &self.modes {
            out.push(dir.join(&m.config.weights));
            out.push(dir.join(&m.config.events));
        }
        out
    }

    /// Newest modification time among the source files (`None`: unreadable).
    pub fn source_mtime(&self) -> Option<std::time::SystemTime> {
        self.source_paths()
            .iter()
            .filter_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
            .max()
    }

    /// A source file changed on disk since this load.
    pub fn source_changed(&self) -> bool {
        self.source_mtime().is_some_and(|m| m > self.loaded_at)
    }

    /// `<repo>/frontend` — the monorepo root holding `apps/` and `packages/`,
    /// which is what the source-level checks grep.
    pub fn frontend_root(&self) -> Option<std::path::PathBuf> {
        let app = self.entry.app_dir.as_ref()?;
        Some(app.parent()?.parent()?.to_path_buf())
    }

    /// The cheapest mode — the base game, in the sense every ladder and every
    /// cross-mode check uses.
    pub fn base_mode(&self) -> Option<&LoadedMode> {
        self.modes
            .iter()
            .min_by(|a, b| a.config.cost.total_cmp(&b.config.cost))
    }

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
                entry.build_dir = detect_build_dir(&entry.publish_dir, &entry.name);
                migrated |= entry.build_dir.is_some();
            }
            if entry.app_dir.is_none() {
                entry.app_dir = detect_app_dir(&entry.publish_dir, &entry.name);
                migrated |= entry.app_dir.is_some();
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
                    // A build never seen before is written down here, so the
                    // next load can say what a `run.py prod` changed.
                    crate::history::record(&g);
                    games.insert(entry.name.clone(), Arc::new(g));
                }
                Err(e) => errors.push((entry.name.clone(), format!("{e:#}"))),
            }
        }
        (
            Self {
                games: RwLock::new(games),
                config: Mutex::new(config),
            },
            errors,
        )
    }

    /// Empty registry, not backed by the persisted config: used by the hub,
    /// which has neither a UI nor a game picker and rediscovers its games
    /// from disk on every reload.
    pub fn empty() -> Self {
        Self {
            games: RwLock::new(HashMap::new()),
            config: Mutex::new(Config::default()),
        }
    }

    /// Replaces the whole set of loaded games in one shot. Returns the
    /// entries that failed to load, by name. Games already served keep
    /// working until the swap: handlers hold an Arc on their own copy.
    pub fn load_all(&self, entries: Vec<GameEntry>) -> Vec<(String, String)> {
        let mut games = HashMap::new();
        let mut errors = Vec::new();
        for entry in entries {
            match load_game(entry.clone()) {
                Ok(g) => {
                    games.insert(entry.name.clone(), Arc::new(g));
                }
                Err(e) => errors.push((entry.name.clone(), format!("{e:#}"))),
            }
        }
        *self.games.write().unwrap() = games;
        errors
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
        if entry.app_dir.is_none() {
            entry.app_dir = detect_app_dir(&entry.publish_dir, &entry.name);
        }
        let game = Arc::new(load_game(entry.clone())?);
        crate::history::record(&game);
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
        // A build (or an app folder) that appeared since the last load: the
        // detection is retried on every reload, and persisted when it lands.
        let mut found = false;
        if entry.build_dir.is_none() {
            entry.build_dir = detect_build_dir(&entry.publish_dir, &entry.name);
            found |= entry.build_dir.is_some();
        }
        if entry.app_dir.is_none() {
            entry.app_dir = detect_app_dir(&entry.publish_dir, &entry.name);
            found |= entry.app_dir.is_some();
        }
        if found {
            let mut cfg = self.config.lock().unwrap();
            if let Some(g) = cfg.games.iter_mut().find(|g| g.name == name) {
                g.build_dir = entry.build_dir.clone();
                g.app_dir = entry.app_dir.clone();
            }
            let _ = persist::save_config(&cfg);
        }
        let game = Arc::new(load_game(entry)?);
        crate::history::record(&game);
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
                            stats: m.stats.clone(),
                        })
                        .collect(),
                    fingerprint: g.fingerprint,
                    loaded_at: g.loaded_at,
                }),
            );
        }
        Ok(())
    }
}

/// Fingerprint of a file: size + mtime + xxh3 of the first/last MiB.
///
/// Memoized per (path, size, mtime): the games list and the import poll ask
/// for it every few seconds, and each fresh computation reads 2 MiB. A file
/// rewritten with the same size AND the same nanosecond mtime is the only
/// change this misses — the hash itself could not tell either apart better.
pub fn fingerprint_file(path: &Path) -> u64 {
    use std::io::{Read, Seek, SeekFrom};
    use xxhash_rust::xxh3::Xxh3;
    type Key = (std::path::PathBuf, u64, u128);
    static MEMO: OnceLock<Mutex<HashMap<Key, u64>>> = OnceLock::new();

    let mut h = Xxh3::new();
    let Ok(meta) = std::fs::metadata(path) else {
        return h.digest();
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos());
    let key = mtime.map(|t| (path.to_path_buf(), meta.len(), t));
    let memo = MEMO.get_or_init(Default::default);
    if let Some(k) = &key {
        if let Some(&fp) = memo.lock().unwrap().get(k) {
            return fp;
        }
    }

    h.update(&meta.len().to_le_bytes());
    if let Some(t) = mtime {
        h.update(&t.to_le_bytes());
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
    let fp = h.digest();
    if let Some(k) = key {
        let mut memo = memo.lock().unwrap();
        if memo.len() >= 4096 {
            memo.clear();
        }
        memo.insert(k, fp);
    }
    fp
}

/// Guesses the frontend static build from publish_dir:
/// `<repo>/math/games/<id>/library/publish_files` →
/// `<repo>/frontend/apps/<game name>/build`, otherwise the single
/// existing `frontend/apps/*/build`, otherwise a client's prebuilt front
/// delivered next to the math (`detect_client_dist`).
pub fn detect_build_dir(publish_dir: &Path, game_name: &str) -> Option<std::path::PathBuf> {
    let s = publish_dir.to_string_lossy();
    let root = Path::new(s.split("/math/games/").next()?);
    let apps = root.join("frontend/apps");
    let named = apps.join(game_name).join("build");
    if named.join("index.html").is_file() {
        return Some(named);
    }
    let mut candidates: Vec<std::path::PathBuf> = std::fs::read_dir(&apps)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("build"))
        .filter(|p| p.join("index.html").is_file())
        .collect();
    match candidates.len() {
        1 => candidates.pop(),
        0 => detect_client_dist(root),
        _ => None,
    }
}

/// A client's prebuilt frontend delivered next to the math, outside any
/// monorepo (Gaspi's UNDEAD KING: `<root>/Undead-King-front-dist/`): the
/// single folder directly under the project root that is a BUILD — its
/// `dist/` or `build/`, or the folder itself when it holds an `index.html`
/// and no `package.json` (an `index.html` next to a `package.json` is the
/// source of a Vite app, not something a browser can play).
fn detect_client_dist(root: &Path) -> Option<std::path::PathBuf> {
    let mut candidates = Vec::new();
    for e in std::fs::read_dir(root).ok()?.flatten() {
        let dir = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !dir.is_dir() || name.starts_with('.') || matches!(name.as_ref(), "math" | "docs" | "node_modules") {
            continue;
        }
        let built = [dir.join("dist"), dir.join("build")]
            .into_iter()
            .find(|d| d.join("index.html").is_file());
        if let Some(d) = built {
            candidates.push(d);
        } else if dir.join("index.html").is_file() && !dir.join("package.json").exists() {
            candidates.push(dir);
        }
    }
    if candidates.len() == 1 {
        candidates.pop()
    } else {
        None
    }
}

/// How the game names its low / high symbols, read from its frontend build:
/// the `math.symbols` table of a `game-config.json` (UNDEAD KING's front maps
/// the books' `P1`…`P9` to `L5`…`H1`) at the root of the build or one or two
/// folders down (`assets/<game>/game-config.json`). The math-sdk's own `H*` /
/// `L*` otherwise.
pub fn front_symbols(build_dir: Option<&Path>) -> slotlab_core::symbols::SymbolMap {
    let Some(build) = build_dir else {
        return Default::default();
    };
    let mut configs = vec![build.join("game-config.json")];
    let mut dirs = vec![build.to_path_buf()];
    for depth in 0..2 {
        let mut next = Vec::new();
        for d in &dirs {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    configs.push(p.join("game-config.json"));
                    if depth == 0 {
                        next.push(p);
                    }
                }
            }
        }
        dirs = next;
    }
    configs
        .iter()
        .filter_map(|p| std::fs::read(p).ok())
        .find_map(|bytes| slotlab_core::symbols::SymbolMap::from_front_config(&bytes))
        .unwrap_or_default()
}

/// The game's frontend app folder: `<repo>/frontend/apps/<name>`, or the
/// single app of the monorepo when the folder is not named after the game
/// (`fortune_neko` ships `apps/maneki-888`).
pub fn detect_app_dir(publish_dir: &Path, game_name: &str) -> Option<std::path::PathBuf> {
    let s = publish_dir.to_string_lossy();
    let root = s.split("/math/games/").next()?;
    let apps = Path::new(root).join("frontend/apps");
    let named = apps.join(game_name);
    if named.join("package.json").is_file() {
        return Some(named);
    }
    let mut candidates: Vec<std::path::PathBuf> = std::fs::read_dir(&apps)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("package.json").is_file())
        .collect();
    if candidates.len() == 1 {
        return candidates.pop();
    }
    None
}

fn load_game(entry: GameEntry) -> Result<LoadedGame> {
    // Taken BEFORE reading: a file rewritten while this load runs counts as
    // changed, and the watcher reads it again.
    let loaded_at = std::time::SystemTime::now();
    let dir = &entry.publish_dir;
    if !dir.is_dir() {
        bail!("publish_files directory not found: {}", dir.display());
    }
    let index_bytes = std::fs::read(dir.join("index.json"))
        .with_context(|| format!("reading {}/index.json", dir.display()))?;
    let index = slotlab_core::index_json::parse(&index_bytes).context("parse index.json")?;

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
        modes.push(LoadedMode::new(mode.clone(), Arc::new(lut)));
    }
    Ok(LoadedGame {
        entry,
        index,
        modes,
        fingerprint: fp.digest(),
        loaded_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("slotlab-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_client_front_next_to_the_math_is_the_build() {
        // UNDEAD KING : pas de monorepo, le dist du client à la racine du projet,
        // à côté d'une source Vite (index.html + package.json) qui ne compte pas.
        let root = scratch("client-dist");
        let publish = root.join("math/games/undeadking/library/publish_files");
        std::fs::create_dir_all(&publish).unwrap();
        touch(&root.join("docs/index.html"), "");
        touch(&root.join("front-src/index.html"), "");
        touch(&root.join("front-src/package.json"), "{}");
        touch(&root.join("Undead-King-front-dist/index.html"), "<!doctype html>");
        touch(
            &root.join("Undead-King-front-dist/assets/undead-king/game-config.json"),
            r#"{"math":{"symbols":{"P1":"L5","P9":"H1"}}}"#,
        );
        let build = detect_build_dir(&publish, "undeadking");
        assert_eq!(build.as_deref(), Some(root.join("Undead-King-front-dist").as_path()));
        // Sa table de symboles est lue dans `assets/<jeu>/game-config.json`.
        let symbols = front_symbols(build.as_deref());
        assert_eq!(symbols.classify("P9"), Some(slotlab_core::symbols::SymClass::High));
        assert!(front_symbols(None).is_empty());

        // Deux builds candidats : on ne devine pas.
        touch(&root.join("other/dist/index.html"), "");
        assert_eq!(detect_build_dir(&publish, "undeadking"), None);

        // Un monorepo garde la priorité.
        touch(&root.join("frontend/apps/undeadking/build/index.html"), "");
        assert_eq!(
            detect_build_dir(&publish, "undeadking").as_deref(),
            Some(root.join("frontend/apps/undeadking/build").as_path())
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
