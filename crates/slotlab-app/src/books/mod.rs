pub mod format;
pub mod import;
pub mod store;

use crate::persist;
use crate::registry::LoadedGame;
use anyhow::Result;
use import::ImportProgress;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use store::ModeBooks;

/// A game's cache directory: `<app>/cache/<name>-<hash of publish_dir>/`.
pub fn game_cache_dir(game: &LoadedGame) -> PathBuf {
    let hash = xxhash_rust::xxh3::xxh3_64(
        game.entry.publish_dir.to_string_lossy().as_bytes(),
    );
    persist::cache_dir().join(format!("{}-{:016x}", game.entry.name, hash))
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
    imports: Mutex<HashMap<String, ImportJob>>,
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
    /// A mode's books, opened from the cache if present and up to date.
    /// Does NOT trigger an import.
    pub fn mode_books(
        &self,
        game: &LoadedGame,
        mode_name: &str,
    ) -> Result<Arc<ModeBooks>> {
        let key = (game.entry.name.clone(), mode_name.to_string());
        if let Some(b) = self.open.lock().unwrap().get(&key).cloned() {
            return Ok(b);
        }
        let books = ModeBooks::open(&game_cache_dir(game), mode_name)?;
        let arc = Arc::new(books);
        self.open.lock().unwrap().insert(key, arc.clone());
        Ok(arc)
    }

    pub fn invalidate_game(&self, game_name: &str) {
        self.open
            .lock()
            .unwrap()
            .retain(|(g, _), _| g != game_name);
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
        let cache = game_cache_dir(game);
        game.modes.iter().all(|m| {
            let idx = cache.join(format!("books_{}.idx", m.config.name));
            match format::ModeBookIndex::read_from(&idx) {
                Ok(index) => {
                    let src = game.entry.publish_dir.join(&m.config.events);
                    index.source_fingerprint
                        == crate::registry::fingerprint_file(&src)
                }
                Err(_) => false,
            }
        })
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
            let cache = game_cache_dir(&game);
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
                if let Ok(existing) = format::ModeBookIndex::read_from(&idx_path) {
                    if existing.source_fingerprint == fp {
                        continue;
                    }
                }

                if let Err(e) = import::import_mode(
                    &src,
                    &cache,
                    &mode.config.name,
                    fp,
                    progress,
                ) {
                    tracing::error!(
                        "import {}/{} : {e:#}",
                        game.entry.name,
                        mode.config.name
                    );
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
