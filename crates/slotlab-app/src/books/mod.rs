pub mod format;
pub mod import;
pub mod mech;
pub mod store;
pub mod zran;

use crate::persist::{self, GameEntry};
use crate::registry::LoadedGame;
use anyhow::Result;
use import::ImportProgress;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use store::ModeBooks;

/// One lock per (game, mode).
type Gates = Mutex<HashMap<(String, String), Arc<Mutex<()>>>>;

/// Modes of a game imported at the same time. Each decodes its books on one
/// core and parses them on the shared pool: two or three keep every core busy.
const CONCURRENT_MODES: usize = 3;

/// A game's cache directory: `<app>/cache/<name>-<hash of publish_dir>/`.
pub fn game_cache_dir(game: &LoadedGame) -> PathBuf {
    entry_cache_dir(&game.entry)
}

fn entry_cache_dir(entry: &GameEntry) -> PathBuf {
    let hash = xxhash_rust::xxh3::xxh3_64(entry.publish_dir.to_string_lossy().as_bytes());
    persist::cache_dir().join(format!("{}-{:016x}", entry.name, hash))
}

/// Deletes every cache directory that no registered game points at (games
/// removed from SlotLab, or whose publish_dir moved), and in the others what
/// no longer serves: the full re-compressed copy of the books SlotLab kept
/// before reading them in place (`.blocks`, as big as the books themselves),
/// and files left half-written by an interrupted import.
pub fn prune_orphan_caches(entries: &[GameEntry]) {
    let keep: Vec<PathBuf> = entries.iter().map(entry_cache_dir).collect();
    let Ok(dirs) = std::fs::read_dir(persist::cache_dir()) else {
        return;
    };
    for dir in dirs.flatten().map(|d| d.path()) {
        if !dir.is_dir() {
            continue;
        }
        if keep.contains(&dir) {
            prune_stale_files(&dir);
            continue;
        }
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => tracing::info!("orphan books cache removed: {}", dir.display()),
            Err(e) => tracing::warn!("cannot remove orphan cache {}: {e}", dir.display()),
        }
    }
}

fn prune_stale_files(dir: &Path) {
    let Ok(files) = std::fs::read_dir(dir) else {
        return;
    };
    for path in files.flatten().map(|f| f.path()) {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let stale = name.ends_with(".blocks") || name.ends_with(".tmp")
            // An index of another version is refused anyway, and the
            // mechanics next to it were computed from it.
            || (name.ends_with(".idx")
                && format::ModeBookIndex::read_header(&path)
                    .is_ok_and(|(v, _, _)| v != format::IDX_VERSION))
            || (name.ends_with(".mech")
                && format::MechFile::read_header(&path).map_or(true, |(v, _)| v != format::IDX_VERSION));
        if stale {
            match std::fs::remove_file(&path) {
                Ok(()) => tracing::info!("stale books file removed: {}", path.display()),
                Err(e) => tracing::warn!("cannot remove {}: {e}", path.display()),
            }
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
    /// The modes being imported right now, comma-separated.
    pub current_mode: Option<String>,
    pub modes_done: usize,
    pub modes_total: usize,
    /// Over every mode of this run.
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub books_done: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The modes whose books are open, with when each was last used: beyond the
/// memory budget, the least recently used is closed (re-opened on its next
/// spin). An index takes ~70 bytes per book — 90 MB for a million-book base
/// game — and a hub with every mode of two games open needed 1.8 GB.
#[derive(Default)]
struct OpenBooks {
    map: HashMap<(String, String), (Arc<ModeBooks>, u64)>,
    tick: u64,
}

/// Manager of imported books + in-progress imports, shared by the app.
#[derive(Default)]
pub struct BooksManager {
    pub store: store::BookStore,
    /// (game, canonical MODE) → open books (index + files).
    open: Mutex<OpenBooks>,
    /// Bytes of open indexes kept at most; 0 = no limit (the desktop app).
    open_budget: AtomicUsize,
    /// One lock per (game, mode) being opened: two first bets arriving
    /// together would otherwise both read the whole index and fingerprint the
    /// source. The second waits, then finds the first one's result in `open`.
    opening: Gates,
    /// One lock per (game, mode) whose mechanics are being computed.
    mech_gates: Gates,
    imports: Mutex<HashMap<String, ImportJob>>,
    /// Explicit cache directory per game name. Absent → derived from the
    /// publish_dir path, which only makes sense on the machine that ran
    /// the import.
    cache_dirs: Mutex<HashMap<String, PathBuf>>,
}

struct ImportJob {
    state: ImportState,
    running: Vec<String>,
    modes_done: usize,
    modes_total: usize,
    /// One per mode this run imports.
    progress: Vec<Arc<ImportProgress>>,
    error: Option<String>,
    cancel: Arc<AtomicBool>,
    /// Newer books arrived while this run was going: what to import once it
    /// has stopped.
    restart: Option<Arc<LoadedGame>>,
}

impl BooksManager {
    /// Declares where each game's books cache was copied. Replaces the
    /// whole mapping and drops the open books, so a redeployed game is
    /// re-read from disk instead of being served from the previous files.
    pub fn set_cache_dirs(&self, dirs: HashMap<String, PathBuf>) {
        *self.cache_dirs.lock().unwrap() = dirs;
        self.open.lock().unwrap().map.clear();
        self.store.clear_all();
    }

    /// Caps what the books hold in memory: the open indexes (least recently
    /// used closed first) and the parsed books kept for replays. For the hub,
    /// whose container has 1 GB for everything.
    pub fn limit_memory(&self, open_indexes: usize, parsed_books: usize) {
        self.open_budget.store(open_indexes, Ordering::Relaxed);
        self.store.set_parsed_budget(parsed_books);
    }

    fn open_get(&self, key: &(String, String)) -> Option<Arc<ModeBooks>> {
        let mut open = self.open.lock().unwrap();
        open.tick += 1;
        let tick = open.tick;
        open.map.get_mut(key).map(|(books, used)| {
            *used = tick;
            books.clone()
        })
    }

    fn open_insert(&self, key: (String, String), books: Arc<ModeBooks>) {
        let budget = self.open_budget.load(Ordering::Relaxed);
        let mut open = self.open.lock().unwrap();
        open.tick += 1;
        let tick = open.tick;
        open.map.insert(key.clone(), (books, tick));
        if budget == 0 {
            return;
        }
        loop {
            let total: usize = open.map.values().map(|(b, _)| b.heap_bytes()).sum();
            if total <= budget || open.map.len() <= 1 {
                return;
            }
            let Some(oldest) = open
                .map
                .iter()
                .filter(|(k, _)| **k != key)
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone())
            else {
                return;
            };
            open.map.remove(&oldest);
            tracing::info!(
                "books of {}/{} closed: {} MB of indexes open, budget {} MB",
                oldest.0,
                oldest.1,
                total >> 20,
                budget >> 20
            );
        }
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
        if let Some(b) = self.open_get(&key) {
            return Ok(b);
        }
        let gate = self.opening.lock().unwrap().entry(key.clone()).or_default().clone();
        let _opening = gate.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(b) = self.open_get(&key) {
            return Ok(b);
        }
        let mode = game
            .modes
            .iter()
            .find(|m| m.config.name == mode_name)
            .ok_or_else(|| anyhow::anyhow!("unknown mode {mode_name}"))?;
        let src = game.entry.publish_dir.join(&mode.config.events);
        let books = ModeBooks::open(&self.cache_dir_for(game), mode_name, &src)?;
        // The index must describe the CURRENT books_<MODE>.jsonl.zst. Checked
        // on open only (a fingerprint reads 2 MiB — far too slow per bet),
        // which is enough: every re-import goes through invalidate_mode().
        anyhow::ensure!(
            self.source_matches(game, books.index.source_fingerprint, books.index.source_len, &src),
            "books cache for {}/{} is stale (the math changed) — \
             SlotLab Games tab → Import books",
            game.entry.name,
            mode_name
        );
        let arc = Arc::new(books);
        self.open_insert(key, arc.clone());
        Ok(arc)
    }

    pub fn invalidate_game(&self, game_name: &str) {
        self.open.lock().unwrap().map.retain(|(g, _), _| g != game_name);
        // Parsed books outlive the index otherwise: the old book would be
        // served for every id already read.
        self.store.purge(game_name, None);
    }

    fn invalidate_mode(&self, game_name: &str, mode: &str) {
        self.open
            .lock()
            .unwrap()
            .map
            .retain(|(g, m), _| !(g == game_name && m == mode));
        self.store.purge(game_name, Some(mode));
    }

    pub fn import_status(&self, game_name: &str) -> Option<ImportStatus> {
        let imports = self.imports.lock().unwrap();
        imports.get(game_name).map(|j| {
            let sum = |f: fn(&ImportProgress) -> u64| j.progress.iter().map(|p| f(p)).sum();
            ImportStatus {
                state: j.state,
                current_mode: (!j.running.is_empty()).then(|| j.running.join(", ")),
                modes_done: j.modes_done,
                modes_total: j.modes_total,
                bytes_done: sum(|p| p.bytes_done.load(Ordering::Relaxed)),
                bytes_total: sum(|p| p.bytes_total.load(Ordering::Relaxed)),
                books_done: sum(|p| p.books_done.load(Ordering::Relaxed)),
                error: j.error.clone(),
            }
        })
    }

    /// Is a game's import already up to date (all .idx present with the
    /// right fingerprint)?
    pub fn is_imported(&self, game: &LoadedGame) -> bool {
        let cache = self.cache_dir_for(game);
        game.modes.iter().all(|m| {
            let idx = cache.join(format!("books_{}.idx", m.config.name));
            match format::ModeBookIndex::read_header(&idx) {
                Ok((format::IDX_VERSION, fingerprint, len)) => {
                    let src = game.entry.publish_dir.join(&m.config.events);
                    self.source_matches(game, fingerprint, len, &src)
                }
                _ => false,
            }
        })
    }

    /// Does an index still describe the source books it was built from?
    /// A game with a declared cache directory (the hub) is served from copies
    /// of the `.zst` that do not keep their mtime: there, the size the index
    /// was built from is what is checked (the books are read in place, so the
    /// copy must be there).
    fn source_matches(&self, game: &LoadedGame, idx_fingerprint: u64, idx_len: u64, src: &Path) -> bool {
        let deployed = self.cache_dirs.lock().unwrap().contains_key(&game.entry.name);
        if deployed {
            return std::fs::metadata(src).is_ok_and(|m| m.len() == idx_len);
        }
        idx_fingerprint == crate::registry::fingerprint_file(src)
    }

    /// Starts the import of the game's modes in the background. If an import
    /// of this game is already running, it is stopped and this one follows
    /// (the books changed again under it: finishing would index files that
    /// no longer exist).
    pub fn start_import(self: &Arc<Self>, game: Arc<LoadedGame>) {
        let game_name = game.entry.name.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut imports = self.imports.lock().unwrap();
            if let Some(j) = imports.get_mut(&game_name) {
                if j.state == ImportState::Running {
                    j.restart = Some(game);
                    j.cancel.store(true, Ordering::Relaxed);
                    return;
                }
            }
            imports.insert(
                game_name.clone(),
                ImportJob {
                    state: ImportState::Running,
                    running: Vec::new(),
                    modes_done: 0,
                    modes_total: game.modes.len(),
                    progress: Vec::new(),
                    error: None,
                    cancel: cancel.clone(),
                    restart: None,
                },
            );
        }
        let manager = self.clone();
        tokio::task::spawn_blocking(move || manager.run_import(game, cancel));
    }

    fn job<T>(&self, game: &str, f: impl FnOnce(&mut ImportJob) -> T) -> T {
        let mut imports = self.imports.lock().unwrap();
        f(imports.get_mut(game).expect("import job of a running import"))
    }

    fn run_import(self: Arc<Self>, game: Arc<LoadedGame>, cancel: Arc<AtomicBool>) {
        let name = game.entry.name.clone();
        let cache = self.cache_dir_for(&game);
        let symbols = crate::registry::front_symbols(game.entry.build_dir.as_deref());

        // The modes whose index is missing or stale, biggest first: the
        // longest decode starts first, the small ones fill in around it.
        struct Todo {
            mode: String,
            src: PathBuf,
            fingerprint: u64,
            progress: Arc<ImportProgress>,
        }
        let mut todo: Vec<Todo> = Vec::new();
        for m in &game.modes {
            let src = game.entry.publish_dir.join(&m.config.events);
            let fingerprint = crate::registry::fingerprint_file(&src);
            let idx = cache.join(format!("books_{}.idx", m.config.name));
            if let Ok((format::IDX_VERSION, existing, _)) = format::ModeBookIndex::read_header(&idx) {
                if existing == fingerprint {
                    continue;
                }
            }
            let progress = Arc::new(ImportProgress::default());
            let size = std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
            progress.bytes_total.store(size, Ordering::Relaxed);
            todo.push(Todo {
                mode: m.config.name.clone(),
                src,
                fingerprint,
                progress,
            });
        }
        todo.sort_by_key(|t| std::cmp::Reverse(t.progress.bytes_total.load(Ordering::Relaxed)));
        self.job(&name, |j| {
            j.modes_total = todo.len();
            j.progress = todo.iter().map(|t| t.progress.clone()).collect();
        });

        let next = AtomicUsize::new(0);
        let errors: Mutex<Vec<String>> = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for _ in 0..CONCURRENT_MODES.min(todo.len()) {
                scope.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= todo.len() || cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let t = &todo[i];
                    self.job(&name, |j| j.running.push(t.mode.clone()));
                    let started = std::time::Instant::now();
                    let outcome = import::import_mode(
                        &t.src,
                        &cache,
                        &t.mode,
                        t.fingerprint,
                        t.progress.clone(),
                        &symbols,
                        &cancel,
                    );
                    // A bet served DURING this mode's import opened the
                    // previous index: drop it now that the new one is there.
                    self.invalidate_mode(&name, &t.mode);
                    self.job(&name, |j| {
                        j.running.retain(|m| m != &t.mode);
                        j.modes_done += 1;
                    });
                    match outcome {
                        Ok(idx) => tracing::info!(
                            "books {name}/{}: {} books indexed in {:.1} s ({:?})",
                            t.mode,
                            idx.books.len(),
                            started.elapsed().as_secs_f64(),
                            idx.storage
                        ),
                        Err(e) if e.is::<import::Cancelled>() => {}
                        Err(e) => {
                            tracing::error!("import {name}/{} : {e:#}", t.mode);
                            errors.lock().unwrap().push(format!("{} : {e:#}", t.mode));
                        }
                    }
                });
            }
        });

        let errors = errors.into_inner().unwrap();
        let restart = self.job(&name, |j| {
            j.running.clear();
            j.state = if errors.is_empty() {
                ImportState::Done
            } else {
                ImportState::Failed
            };
            j.error = (!errors.is_empty()).then(|| errors.join(" · "));
            j.restart.take()
        });
        match restart {
            Some(newer) => {
                tracing::info!("books of {name} changed during the import: importing again");
                self.start_import(newer);
            }
            None if errors.is_empty() => tracing::info!("books import done: {name}"),
            None => {}
        }
    }

    /// A mode's mechanics (`books_<MODE>.mech`), computed from its books the
    /// first time they are asked for and kept next to the index.
    pub fn mechanics(&self, game: &LoadedGame, mode: &str) -> Result<format::MechFile> {
        let books = self.mode_books(game, mode)?;
        let path = self.cache_dir_for(game).join(format!("books_{mode}.mech"));
        let gate = self
            .mech_gates
            .lock()
            .unwrap()
            .entry((game.entry.name.clone(), mode.to_string()))
            .or_default()
            .clone();
        let _gate = gate.lock().unwrap_or_else(|e| e.into_inner());
        let fingerprint = books.index.source_fingerprint;
        if format::MechFile::read_header(&path).is_ok_and(|h| h == (format::IDX_VERSION, fingerprint)) {
            match format::MechFile::read_from(&path) {
                Ok(f) if f.mech.offsets.len() == books.index.books.len() + 1 => return Ok(f),
                Ok(_) => tracing::warn!("{} does not match its books: recomputed", path.display()),
                Err(e) => tracing::warn!("{}: {e:#} — recomputed", path.display()),
            }
        }
        let started = std::time::Instant::now();
        let file = format::MechFile {
            version: format::IDX_VERSION,
            source_fingerprint: fingerprint,
            mech: mech::build(&books, None)?,
        };
        tracing::info!(
            "mechanics of {}/{mode} computed in {:.1} s",
            game.entry.name,
            started.elapsed().as_secs_f64()
        );
        if let Err(e) = file.write_to(&path) {
            tracing::warn!("cannot keep {}: {e:#}", path.display());
        }
        Ok(file)
    }
}
