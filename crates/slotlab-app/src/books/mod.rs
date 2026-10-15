pub mod format;
pub mod import;
pub mod store;

use crate::persist::{self, GameEntry};
use crate::registry::LoadedGame;
use anyhow::Result;
use import::ImportProgress;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use store::ModeBooks;

/// A game's cache directory: `<app>/cache/<name>-<hash of publish_dir>/`.
pub fn game_cache_dir(game: &LoadedGame) -> PathBuf {
    entry_cache_dir(&game.entry)
}

fn entry_cache_dir(entry: &GameEntry) -> PathBuf {
    let hash = xxhash_rust::xxh3::xxh3_64(entry.publish_dir.to_string_lossy().as_bytes());
    persist::cache_dir().join(format!("{}-{:016x}", entry.name, hash))
}

/// Deletes every cache directory that no registered game points at: games
/// removed from SlotLab, or whose publish_dir moved. A books cache is a full
/// copy of the books (several GB per game), nothing else ever reclaims it.
pub fn prune_orphan_caches(entries: &[GameEntry]) {
    let keep: Vec<PathBuf> = entries.iter().map(entry_cache_dir).collect();
    let Ok(dirs) = std::fs::read_dir(persist::cache_dir()) else {
        return;
    };
    for dir in dirs.flatten().map(|d| d.path()) {
        if !dir.is_dir() || keep.contains(&dir) {
            continue;
        }
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => tracing::info!("orphan books cache removed: {}", dir.display()),
            Err(e) => tracing::warn!("cannot remove orphan cache {}: {e}", dir.display()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)] // Idle: default state on the UI side
pub enum ImportState {
    Idle,
    Running,
    Done,
    Failed,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportStatus {
    pub state: ImportState,
    pub current_mode: Option<String>,
    pub modes_done: usize,
    pub modes_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub books_done: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Manager of imported books + in-progress imports, shared by the app.
#[derive(Default)]
pub struct BooksManager {
    pub store: store::BookStore,
    /// (game, canonical MODE) → open books (index + mmap).
    open: Mutex<HashMap<(String, String), Arc<ModeBooks>>>,
    /// One lock per (game, mode) being opened: two first bets arriving
    /// together would otherwise both read the whole index and fingerprint the
    /// source. The second waits, then finds the first one's result in `open`.
    opening: Mutex<HashMap<(String, String), Arc<Mutex<()>>>>,
    imports: Mutex<HashMap<String, ImportJob>>,
    /// Explicit cache directory per game name. Absent → derived from the
    /// publish_dir path, which only makes sense on the machine that ran
    /// the import.
    cache_dirs: Mutex<HashMap<String, PathBuf>>,
}

struct ImportJob {
    state: ImportState,
    current_mode: Option<String>,
    modes_done: usize,
    modes_total: usize,
    progress: Arc<ImportProgress>,
    error: Option<String>,
}

impl BooksManager {
    /// Declares where each game's books cache was copied. Replaces the
    /// whole mapping and drops the open mmaps, so a redeployed game is
    /// re-read from disk instead of being served from the previous file.
    pub fn set_cache_dirs(&self, dirs: HashMap<String, PathBuf>) {
        *self.cache_dirs.lock().unwrap() = dirs;
        self.open.lock().unwrap().clear();
        self.store.clear_all();
    }

    /// Cache directory of a game.
    pub fn cache_dir_for(&self, game: &LoadedGame) -> PathBuf {
        self.cache_dirs
            .lock()
            .unwrap()
            .get(&game.entry.name)
            .cloned()
            .unwrap_or_else(|| game_cache_dir(game))
    }

    /// A mode's books, opened from the cache if present and up to date.
    /// Does NOT trigger an import.
    pub fn mode_books(&self, game: &LoadedGame, mode_name: &str) -> Result<Arc<ModeBooks>> {
        let key = (game.entry.name.clone(), mode_name.to_string());
        if let Some(b) = self.open.lock().unwrap().get(&key).cloned() {
            return Ok(b);
        }
        let gate = self.opening.lock().unwrap().entry(key.clone()).or_default().clone();
        let _opening = gate.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(b) = self.open.lock().unwrap().get(&key).cloned() {
            return Ok(b);
        }
        let books = ModeBooks::open(&self.cache_dir_for(game), mode_name)?;
        // The cache must match the CURRENT books_<MODE>.jsonl.zst. Checked on
        // open only (a fingerprint reads 2 MiB — far too slow per bet), which
        // is enough: every re-import goes through invalidate_game().
        if let Some(mode) = game.modes.iter().find(|m| m.config.name == mode_name) {
            let src = game.entry.publish_dir.join(&mode.config.events);
            anyhow::ensure!(
                self.source_matches(game, books.index.source_fingerprint, &src),
                "books cache for {}/{} is stale (the math changed) — \
                 SlotLab Games tab → Import books",
                game.entry.name,
                mode_name
            );
        }
        let arc = Arc::new(books);
        self.open.lock().unwrap().insert(key, arc.clone());
        Ok(arc)
    }

    pub fn invalidate_game(&self, game_name: &str) {
        self.open.lock().unwrap().retain(|(g, _), _| g != game_name);
        // Blocks and parsed books outlive the index otherwise: the new
        // offsets would be read out of the old decompressed blocks.
        self.store.purge_game(game_name);
    }

    pub fn import_status(&self, game_name: &str) -> Option<ImportStatus> {
        let imports = self.imports.lock().unwrap();
        imports.get(game_name).map(|j| ImportStatus {
            state: j.state,
            current_mode: j.current_mode.clone(),
            modes_done: j.modes_done,
            modes_total: j.modes_total,
            bytes_done: j.progress.bytes_done.load(Ordering::Relaxed),
            bytes_total: j.progress.bytes_total.load(Ordering::Relaxed),
            books_done: j.progress.books_done.load(Ordering::Relaxed),
            error: j.error.clone(),
        })
    }

    /// Is a game's import already up to date (all .idx present with the
    /// right fingerprint)?
    pub fn is_imported(&self, game: &LoadedGame) -> bool {
        let cache = self.cache_dir_for(game);
        game.modes.iter().all(|m| {
            let idx = cache.join(format!("books_{}.idx", m.config.name));
            match format::ModeBookIndex::read_header(&idx) {
                Ok((format::IDX_VERSION, fingerprint)) => {
                    let src = game.entry.publish_dir.join(&m.config.events);
                    self.source_matches(game, fingerprint, &src)
                }
                _ => false,
            }
        })
    }

    /// Does an index still describe the source books it was built from?
    /// A game with a declared cache directory (the hub) is served from a
    /// copy that never carries the `.jsonl.zst` — Deploy checks freshness
    /// on the importing machine before sending. There, an absent source
    /// means "shipped without it", not "stale": the `.idx` is trusted.
    fn source_matches(&self, game: &LoadedGame, idx_fingerprint: u64, src: &Path) -> bool {
        let deployed = self.cache_dirs.lock().unwrap().contains_key(&game.entry.name);
        if deployed && !src.exists() {
            return true;
        }
        idx_fingerprint == crate::registry::fingerprint_file(src)
    }

    /// Starts the import of all the game's modes in the background.
    /// No-op if an import is already running for this game.
    pub fn start_import(self: &Arc<Self>, game: Arc<LoadedGame>) {
        let game_name = game.entry.name.clone();
        {
            let mut imports = self.imports.lock().unwrap();
            if let Some(j) = imports.get(&game_name) {
                if j.state == ImportState::Running {
                    return;
                }
            }
            imports.insert(
                game_name.clone(),
                ImportJob {
                    state: ImportState::Running,
                    current_mode: None,
                    modes_done: 0,
                    modes_total: game.modes.len(),
                    progress: Arc::new(ImportProgress::default()),
                    error: None,
                },
            );
        }
        self.invalidate_game(&game_name);
        let manager = self.clone();
        tokio::task::spawn_blocking(move || {
            let cache = manager.cache_dir_for(&game);
            let symbols = crate::registry::front_symbols(game.entry.build_dir.as_deref());
            for (i, mode) in game.modes.iter().enumerate() {
                let progress = Arc::new(ImportProgress::default());
                {
                    let mut imports = manager.imports.lock().unwrap();
                    let j = imports.get_mut(&game.entry.name).unwrap();
                    j.current_mode = Some(mode.config.name.clone());
                    j.modes_done = i;
                    j.progress = progress.clone();
                }
                let src = game.entry.publish_dir.join(&mode.config.events);
                let fp = crate::registry::fingerprint_file(&src);

                // Skip if THIS mode's cache is already up to date.
                let idx_path = cache.join(format!("books_{}.idx", mode.config.name));
                if let Ok((format::IDX_VERSION, existing)) =
                    format::ModeBookIndex::read_header(&idx_path)
                {
                    if existing == fp {
                        continue;
                    }
                }

                let outcome =
                    import::import_mode(&src, &cache, &mode.config.name, fp, progress, &symbols);
                // A bet served DURING this mode's import re-opened the OLD
                // .blocks (same path until the rename) and cached its index +
                // blocks. Re-invalidate now that the new files are in place.
                manager.invalidate_game(&game.entry.name);
                if let Err(e) = outcome {
                    tracing::error!("import {}/{} : {e:#}", game.entry.name, mode.config.name);
                    let mut imports = manager.imports.lock().unwrap();
                    let j = imports.get_mut(&game.entry.name).unwrap();
                    j.state = ImportState::Failed;
                    j.error = Some(format!("{} : {e:#}", mode.config.name));
                    return;
                }
            }
            let mut imports = manager.imports.lock().unwrap();
            let j = imports.get_mut(&game.entry.name).unwrap();
            j.state = ImportState::Done;
            j.modes_done = j.modes_total;
            j.current_mode = None;
            tracing::info!("books import done: {}", game.entry.name);
        });
    }
}
