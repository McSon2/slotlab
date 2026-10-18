use super::format::{BookRecord, Checkpoint, ModeBookIndex, ResumeInfo, Storage, IDX_VERSION};
use super::store::ModeBooks;
use super::zran::{self, Decoder, Snapshot};
use anyhow::{Context, Result};
use rayon::prelude::*;
use serde::Deserialize;
use slotlab_core::freespins::{summarize_with, FsEvent};
use slotlab_core::symbols::SymbolMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Decompressed bytes between two resume points: reaching a book decodes
/// half of it on average (~15 ms), each point costs ~150 KB (~3 % of the
/// `.zst` in all).
const RESUME_EVERY: u64 = 64 << 20;
/// Size of a chunk of complete lines handed to the parsers.
const CHUNK_TARGET: usize = 4 << 20;
/// Chunks parsed per parallel wave (bounds RAM: ~2 × BATCH × CHUNK_TARGET).
const BATCH: usize = 16;
/// zstd level of the resume points and of a copy's frames.
const LEVEL: i32 = 3;

/// Shared progress of an import (read by the status API).
#[derive(Debug, Default)]
pub struct ImportProgress {
    /// COMPRESSED bytes consumed from the source file.
    pub bytes_done: AtomicU64,
    pub bytes_total: AtomicU64,
    pub books_done: AtomicU64,
}

/// The import was stopped on purpose (newer books on disk): not a failure.
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("import cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// The source cannot be read in place: copy it instead (`Storage::Copy`).
#[derive(Debug)]
struct NeedsCopy(String);

impl std::fmt::Display for NeedsCopy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NeedsCopy {}

/// Minimal fields extracted from each book for the search index.
///
/// Les événements sont lus par `FsEvent` : leur type ou leurs actions (bits
/// d'événements, `fs_triggers`), les annonces de total de free spins
/// (`fs_retriggers`), la longueur de `positions` (`fs_tier`) et les symboles
/// des plateaux révélés (`sym_*`), tout le reste sauté sans être construit —
/// le type est même emprunté à la ligne au lieu d'être copié.
#[derive(Deserialize)]
struct BookMetaRaw<'a> {
    id: u64,
    #[serde(default, rename = "payoutMultiplier")]
    payout_multiplier: f64,
    #[serde(default)]
    criteria: Option<String>,
    /// Gain du bonus seul, en mises (champ des books du math-sdk).
    #[serde(default, rename = "freeGameWins")]
    free_game_wins: Option<f64>,
    #[serde(default, borrow)]
    events: Vec<FsEvent<'a>>,
}

/// Complete lines of the stream, from offset `base` (the last one may lack
/// its `\n` at the end of the stream).
struct Chunk {
    base: u64,
    buf: Vec<u8>,
}

/// What the sequential decoder hands over, in stream order.
enum Item {
    Chunk(Chunk),
    /// A resume point (`Storage::InPlace`).
    Resume(Snapshot),
    /// A frame starts there: a free starting point (multi-frame sources).
    FrameStart { comp: u64, dec: u64 },
}

/// Result of processing ONE chunk in parallel.
struct ChunkOut {
    /// Records with criteria/event indices LOCAL to the chunk.
    records: Vec<LocalRecord>,
    criteria_local: Vec<String>,
    event_types_local: Vec<String>,
    /// xxh3 of each book's line, to check the index by reading it back.
    hashes: Vec<u64>,
    /// `Storage::Copy`: the chunk as one independent zstd frame.
    frame: Option<Vec<u8>>,
}

struct LocalRecord {
    sim_id: u64,
    offset_in_chunk: u32,
    len: u32,
    payout_x100: u64,
    criteria_local: u16,
    event_bits_local: u64,
    fs_triggers: u16,
    fs_retriggers: u16,
    fs_tier: u8,
    fs_spins_initial: u16,
    fs_spins_final: u16,
    fg_win_x100: Option<u64>,
    /// Symboles low / high : base, base, free spins, free spins.
    sym: [u16; 4],
    /// Anticipation du spin payé.
    antic: bool,
}

/// Parses a chunk of lines (runs on the rayon pool).
fn process_chunk(
    chunk: &Chunk,
    mode_name: &str,
    symbols: &SymbolMap,
    storage: Storage,
) -> Result<ChunkOut> {
    let mut records = Vec::with_capacity(2048);
    let mut hashes = Vec::with_capacity(2048);
    let mut criteria_local: Vec<String> = Vec::new();
    let mut event_types_local: Vec<String> = Vec::new();

    let buf = &chunk.buf;
    let mut start = 0usize;
    while start < buf.len() {
        let end = memchr::memchr(b'\n', &buf[start..]).map_or(buf.len(), |i| start + i);
        let line = &buf[start..end];
        let at = start;
        start = end + 1;
        if line.trim_ascii().is_empty() {
            continue;
        }
        let meta: BookMetaRaw = serde_json::from_slice(line)
            .with_context(|| format!("invalid book JSON (mode {mode_name}) at byte {}", chunk.base + at as u64))?;

        let criteria = match &meta.criteria {
            Some(c) => match criteria_local.iter().position(|n| n == c) {
                Some(i) => i as u16,
                None => {
                    criteria_local.push(c.clone());
                    (criteria_local.len() - 1) as u16
                }
            },
            None => u16::MAX,
        };

        // Déclenchements, palier et retriggers sur TOUS les événements — la
        // boucle des bits ci-dessous saute ceux au-delà du 64e type.
        let fs = summarize_with(&meta.events, symbols);
        let mut event_bits: u64 = 0;
        for kind in meta.events.iter().flat_map(|ev| ev.kinds()) {
            let bit = match event_types_local.iter().position(|n| n == kind) {
                Some(i) => i,
                None if event_types_local.len() < 64 => {
                    event_types_local.push(kind.to_string());
                    event_types_local.len() - 1
                }
                None => continue,
            };
            event_bits |= 1u64 << bit;
        }

        hashes.push(xxhash_rust::xxh3::xxh3_64(line.trim_ascii_end()));
        records.push(LocalRecord {
            sim_id: meta.id,
            offset_in_chunk: at as u32,
            len: line.len() as u32,
            // The books' payoutMultiplier is already ×100 (aligned with LUT col 3).
            payout_x100: meta.payout_multiplier.round().max(0.0) as u64,
            criteria_local: criteria,
            event_bits_local: event_bits,
            fs_triggers: fs.triggers.min(u16::MAX as u32) as u16,
            fs_retriggers: fs.retriggers.min(u16::MAX as u32) as u16,
            fs_tier: fs.tier.min(u8::MAX as u32) as u8,
            fs_spins_initial: fs.spins_initial.min(u16::MAX as u32) as u16,
            fs_spins_final: fs.spins_final.min(u16::MAX as u32) as u16,
            fg_win_x100: meta
                .free_game_wins
                .filter(|v| v.is_finite())
                .map(|v| (v * 100.0).round().max(0.0) as u64),
            sym: [fs.symbols.low, fs.symbols.high, fs.symbols.low_fs, fs.symbols.high_fs]
                .map(|n| n.min(u16::MAX as u32) as u16),
            antic: fs.anticipation,
        });
    }

    let frame = match storage {
        Storage::Copy => Some(zstd::bulk::compress(buf, LEVEL)?),
        Storage::InPlace => None,
    };
    Ok(ChunkOut {
        records,
        criteria_local,
        event_types_local,
        hashes,
        frame,
    })
}

fn intern(global: &mut Vec<String>, name: &str) -> usize {
    match global.iter().position(|n| n == name) {
        Some(i) => i,
        None => {
            global.push(name.to_string());
            global.len() - 1
        }
    }
}

/// The resume point as stored in `.ckpt`: the state, then the window.
fn resume_blob(s: &Snapshot) -> Result<Vec<u8>> {
    let mut raw = Vec::with_capacity(s.state.len() + s.window.len());
    raw.extend_from_slice(&s.state);
    raw.extend_from_slice(&s.window);
    Ok(zstd::bulk::compress(&raw, LEVEL)?)
}

/// Decodes the source once, in order, and hands over chunks of complete
/// lines (and, in place, a resume point every `RESUME_EVERY` bytes).
fn decode_source(
    file: &File,
    storage: Storage,
    resume_every: u64,
    tx: &std::sync::mpsc::SyncSender<Result<Item>>,
    progress: &ImportProgress,
    cancel: &AtomicBool,
) -> Result<()> {
    let send = |item: Item| -> Result<()> {
        // A send error means the consumer stopped (it reports why).
        tx.send(Ok(item)).map_err(|_| anyhow::anyhow!(Cancelled))
    };
    let mut d = Decoder::at_frame(file, 0, 0)?;
    let mut chunk = Chunk {
        base: 0,
        buf: Vec::with_capacity(CHUNK_TARGET + (256 << 10)),
    };
    let mut next_resume = resume_every;
    let mut frames_seen = 1;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        match d.next_block()? {
            Some(b) => chunk.buf.extend_from_slice(b.data),
            None => break,
        }
        progress.bytes_done.store(d.comp_pos(), Ordering::Relaxed);

        let (frames, (comp, dec)) = d.frames();
        if frames > frames_seen {
            frames_seen = frames;
            if storage == Storage::InPlace {
                send(Item::FrameStart { comp, dec })?;
                next_resume = dec + resume_every;
            }
        }
        if storage == Storage::InPlace && d.dec_pos() >= next_resume && d.at_block_boundary() {
            match d.snapshot()? {
                Some(s) => send(Item::Resume(s))?,
                None => {
                    return Err(NeedsCopy(format!(
                        "compressed with a {} MiB window, over the {} MiB a resume point may carry",
                        d.window_size() >> 20,
                        zran::MAX_RESUME_WINDOW >> 20
                    ))
                    .into())
                }
            }
            next_resume = d.dec_pos() + resume_every;
        }

        if chunk.buf.len() >= CHUNK_TARGET {
            if let Some(nl) = memchr::memrchr(b'\n', &chunk.buf) {
                let mut next = Vec::with_capacity(CHUNK_TARGET + (256 << 10));
                next.extend_from_slice(&chunk.buf[nl + 1..]);
                chunk.buf.truncate(nl + 1);
                let base = chunk.base + chunk.buf.len() as u64;
                send(Item::Chunk(std::mem::replace(&mut chunk, Chunk { base, buf: next })))?;
            }
        }
    }
    if !chunk.buf.is_empty() {
        send(Item::Chunk(chunk))?;
    }
    Ok(())
}

/// Imports `books_<MODE>.jsonl.zst` → `<cache>/books_<MODE>.idx` (+ `.ckpt`).
///
/// The books are NOT copied: the index points into the source itself, which
/// is decoded once, in order, while waves of chunks are parsed IN PARALLEL
/// (rayon) — JSON parsing, the dominant cost, uses all cores. Along the way
/// a resume point is saved every `RESUME_EVERY` bytes (`zran`); before the
/// index is written, every book is read back through them and compared.
/// If the source cannot be entered that way, the books are copied into
/// independent frames instead (`Storage::Copy`). `symbols`: how the game
/// names its low / high symbols (`SymbolMap::default()` for the math-sdk's
/// `H*` / `L*`). `cancel`: stops the import (error `Cancelled`).
pub fn import_mode(
    source: &Path,
    cache_dir: &Path,
    mode_name: &str,
    source_fingerprint: u64,
    progress: Arc<ImportProgress>,
    symbols: &SymbolMap,
    cancel: &AtomicBool,
) -> Result<ModeBookIndex> {
    import_spaced(source, cache_dir, mode_name, source_fingerprint, progress, symbols, cancel, RESUME_EVERY)
}

/// `import_mode` with resume points every `resume_every` bytes (the tests'
/// books are far smaller than one production stretch).
#[allow(clippy::too_many_arguments)]
fn import_spaced(
    source: &Path,
    cache_dir: &Path,
    mode_name: &str,
    source_fingerprint: u64,
    progress: Arc<ImportProgress>,
    symbols: &SymbolMap,
    cancel: &AtomicBool,
    resume_every: u64,
) -> Result<ModeBookIndex> {
    zran::self_check()?;
    std::fs::create_dir_all(cache_dir)?;
    let attempt = |storage| {
        import_as(
            source,
            cache_dir,
            mode_name,
            source_fingerprint,
            &progress,
            symbols,
            cancel,
            storage,
            resume_every,
        )
    };
    match attempt(Storage::InPlace) {
        Err(e) if e.is::<NeedsCopy>() => {
            tracing::warn!(
                "books {mode_name}: {e} — read from a re-framed copy instead of the source"
            );
            progress.bytes_done.store(0, Ordering::Relaxed);
            progress.books_done.store(0, Ordering::Relaxed);
            attempt(Storage::Copy)
        }
        other => other,
    }
}

fn side_path(cache_dir: &Path, mode_name: &str, storage: Storage) -> PathBuf {
    cache_dir.join(match storage {
        Storage::InPlace => format!("books_{mode_name}.ckpt"),
        Storage::Copy => format!("books_{mode_name}.zcopy"),
    })
}

fn with_tmp(path: &Path) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(".tmp");
    PathBuf::from(p)
}

#[allow(clippy::too_many_arguments)]
fn import_as(
    source: &Path,
    cache_dir: &Path,
    mode_name: &str,
    source_fingerprint: u64,
    progress: &ImportProgress,
    symbols: &SymbolMap,
    cancel: &AtomicBool,
    storage: Storage,
    resume_every: u64,
) -> Result<ModeBookIndex> {
    let file =
        File::open(source).with_context(|| format!("opening {}", source.display()))?;
    let source_len = file.metadata()?.len();
    progress.bytes_total.store(source_len, Ordering::Relaxed);

    // Resume points (in place) or the copy's frames, written in stream order.
    let side = side_path(cache_dir, mode_name, storage);
    let side_tmp = with_tmp(&side);
    let mut out = std::io::BufWriter::with_capacity(1 << 20, File::create(&side_tmp)?);
    let mut side_pos: u64 = 0;

    let mut checkpoints: Vec<Checkpoint> = match storage {
        Storage::InPlace => vec![Checkpoint { comp: 0, dec: 0, resume: None }],
        Storage::Copy => Vec::new(),
    };
    let mut books: Vec<BookRecord> = Vec::new();
    let mut hashes: Vec<u64> = Vec::new();
    let mut criteria_names: Vec<String> = Vec::new();
    let mut event_type_names: Vec<String> = Vec::new();
    let mut stream_len: u64 = 0;

    let result = std::thread::scope(|scope| -> Result<()> {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Result<Item>>(BATCH);
        let decoder = scope.spawn({
            let file = &file;
            move || {
                if let Err(e) = decode_source(file, storage, resume_every, &tx, progress, cancel) {
                    let _ = tx.send(Err(e));
                }
            }
        });

        let outcome = (|| -> Result<()> {
            loop {
                // A wave, in stream order: up to BATCH chunks, with the
                // resume points and frame starts met along the way.
                let mut wave: Vec<Item> = Vec::with_capacity(BATCH + 4);
                let mut chunks = 0;
                while chunks < BATCH {
                    match rx.recv() {
                        Ok(item) => {
                            let item = item?;
                            chunks += matches!(item, Item::Chunk(_)) as usize;
                            wave.push(item);
                        }
                        Err(_) => break, // decoder done
                    }
                }
                if wave.is_empty() {
                    return Ok(());
                }
                if cancel.load(Ordering::Relaxed) {
                    return Err(Cancelled.into());
                }

                // Parse (+ compression) in parallel, order preserved by collect.
                enum Done {
                    Chunk(ChunkOut),
                    Blob(Vec<u8>),
                    Nothing,
                }
                let outs: Vec<Result<Done>> = wave
                    .par_iter()
                    .map(|item| match item {
                        Item::Chunk(c) => {
                            process_chunk(c, mode_name, symbols, storage).map(Done::Chunk)
                        }
                        Item::Resume(s) => resume_blob(s).map(Done::Blob),
                        Item::FrameStart { .. } => Ok(Done::Nothing),
                    })
                    .collect();

                // Ordered write + merge of local tables → global.
                for (item, done) in wave.into_iter().zip(outs) {
                    match (item, done?) {
                        (Item::Chunk(c), Done::Chunk(o)) => {
                            if let Some(frame) = &o.frame {
                                checkpoints.push(Checkpoint { comp: side_pos, dec: c.base, resume: None });
                                out.write_all(frame)?;
                                side_pos += frame.len() as u64;
                            }
                            stream_len = c.base + c.buf.len() as u64;
                            merge_chunk(
                                c.base,
                                o,
                                &mut books,
                                &mut hashes,
                                &mut criteria_names,
                                &mut event_type_names,
                                progress,
                            );
                        }
                        (Item::Resume(s), Done::Blob(blob)) => {
                            out.write_all(&blob)?;
                            checkpoints.push(Checkpoint {
                                comp: s.comp,
                                dec: s.dec,
                                resume: Some(ResumeInfo {
                                    window_size: s.window_size,
                                    checksum: s.checksum,
                                    blob_offset: side_pos,
                                    blob_len: blob.len() as u32,
                                    window_len: s.window.len() as u32,
                                }),
                            });
                            side_pos += blob.len() as u64;
                        }
                        (Item::FrameStart { comp, dec }, _) => {
                            if checkpoints.last().is_none_or(|c| c.dec < dec) {
                                checkpoints.push(Checkpoint { comp, dec, resume: None });
                            }
                        }
                        _ => unreachable!("wave item and its result disagree"),
                    }
                }
            }
        })();
        drop(rx); // unblocks the decoder if we stopped early
        let _ = decoder.join();
        outcome
    });
    if let Err(e) = result {
        drop(out);
        let _ = std::fs::remove_file(&side_tmp);
        return Err(e);
    }
    out.flush()?;
    drop(out);
    if checkpoints.is_empty() {
        checkpoints.push(Checkpoint { comp: 0, dec: 0, resume: None });
    }

    if !books.windows(2).all(|w| w[0].sim_id <= w[1].sim_id) {
        let mut order: Vec<usize> = (0..books.len()).collect();
        order.sort_by_key(|&i| books[i].sim_id);
        books = order.iter().map(|&i| books[i].clone()).collect();
        hashes = order.iter().map(|&i| hashes[i]).collect();
    }

    let index = ModeBookIndex {
        version: IDX_VERSION,
        source_fingerprint,
        source_len,
        storage,
        checkpoints,
        stream_len,
        books,
        criteria_names,
        event_type_names,
    };

    // Every book read back the way the RGS and the scans will read it, and
    // compared with what the sequential pass saw.
    let (file, file_path) = match storage {
        Storage::InPlace => (file, source.to_path_buf()),
        Storage::Copy => (File::open(&side_tmp)?, side.clone()),
    };
    let ckpt = match storage {
        Storage::InPlace => Some(File::open(&side_tmp)?),
        Storage::Copy => None,
    };
    let check = ModeBooks::from_parts(index, file, file_path, ckpt)?;
    let differ = check.par_fold(
        || 0u64,
        |n, i, line| *n += (xxhash_rust::xxh3::xxh3_64(line) != hashes[i]) as u64,
        |a, b| a + b,
        None,
    );
    let index = check.into_index();
    match (differ, storage) {
        (Ok(0), _) => {}
        (Ok(n), Storage::InPlace) => {
            let _ = std::fs::remove_file(&side_tmp);
            return Err(NeedsCopy(format!("{n} book(s) differ when read back from the resume points")).into());
        }
        (Err(e), Storage::InPlace) => {
            let _ = std::fs::remove_file(&side_tmp);
            return Err(NeedsCopy(format!("reading back from the resume points failed: {e:#}")).into());
        }
        (Ok(n), Storage::Copy) => {
            let _ = std::fs::remove_file(&side_tmp);
            anyhow::bail!("{n} book(s) differ when read back from the copy");
        }
        (Err(e), Storage::Copy) => {
            let _ = std::fs::remove_file(&side_tmp);
            return Err(e.context("reading back the copy"));
        }
    }

    std::fs::rename(&side_tmp, &side)?;
    // What this storage replaces: the other one, the copy SlotLab kept
    // before reading in place, and the mechanics of the previous books.
    let other = match storage {
        Storage::InPlace => Storage::Copy,
        Storage::Copy => Storage::InPlace,
    };
    for stale in [
        side_path(cache_dir, mode_name, other),
        cache_dir.join(format!("books_{mode_name}.blocks")),
        cache_dir.join(format!("books_{mode_name}.mech")),
    ] {
        let _ = std::fs::remove_file(stale);
    }
    // Written last: a `.idx` present means a complete import.
    index.write_to(&cache_dir.join(format!("books_{mode_name}.idx")))?;
    Ok(index)
}

fn merge_chunk(
    base: u64,
    c: ChunkOut,
    books: &mut Vec<BookRecord>,
    hashes: &mut Vec<u64>,
    criteria_names: &mut Vec<String>,
    event_type_names: &mut Vec<String>,
    progress: &ImportProgress,
) {
    // Remap local criteria → global.
    let criteria_map: Vec<u16> = c
        .criteria_local
        .iter()
        .map(|n| intern(criteria_names, n) as u16)
        .collect();
    // Remap local event bits → global.
    let event_map: Vec<Option<u32>> = c
        .event_types_local
        .iter()
        .map(|n| {
            let gi = intern(event_type_names, n);
            (gi < 64).then_some(gi as u32)
        })
        .collect();
    progress
        .books_done
        .fetch_add(c.records.len() as u64, Ordering::Relaxed);
    hashes.extend_from_slice(&c.hashes);
    for r in c.records {
        let mut bits: u64 = 0;
        let mut local = r.event_bits_local;
        while local != 0 {
            let i = local.trailing_zeros() as usize;
            local &= local - 1;
            if let Some(Some(g)) = event_map.get(i) {
                bits |= 1u64 << g;
            }
        }
        books.push(BookRecord {
            sim_id: r.sim_id,
            offset: base + r.offset_in_chunk as u64,
            len: r.len,
            payout_x100: r.payout_x100,
            criteria: if r.criteria_local == u16::MAX {
                u16::MAX
            } else {
                criteria_map[r.criteria_local as usize]
            },
            event_bits: bits,
            fs_triggers: r.fs_triggers,
            fs_retriggers: r.fs_retriggers,
            fs_tier: r.fs_tier,
            fs_spins_initial: r.fs_spins_initial,
            fs_spins_final: r.fs_spins_final,
            fg_win_x100: r.fg_win_x100,
            sym_low: r.sym[0],
            sym_high: r.sym[1],
            sym_low_fs: r.sym[2],
            sym_high_fs: r.sym[3],
            antic: r.antic,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Répertoire jetable, propre à ce test et à ce processus.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("slotlab-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Resume points every 256 KiB: the test books cross many of them.
    fn import(src: &Path, dir: &Path) -> ModeBookIndex {
        import_spaced(
            src,
            dir,
            "BASE",
            42,
            Arc::new(ImportProgress::default()),
            &SymbolMap::default(),
            &AtomicBool::new(false),
            256 << 10,
        )
        .unwrap()
    }

    #[test]
    fn import_indexes_triggers_and_retriggers() {
        let sdk = r#"{"id":1,"payoutMultiplier":2500,"criteria":"freegame","baseGameWins":0.0,"freeGameWins":25.0,"events":[
            {"index":0,"type":"freeSpinTrigger","totalFs":10,"positions":[{"reel":0,"row":1},{"reel":2,"row":0},{"reel":4,"row":2}]},
            {"index":1,"type":"updateFreeSpin","amount":1,"total":10},
            {"index":2,"type":"freeSpinRetrigger","totalFs":15},
            {"index":3,"type":"updateFreeSpin","amount":2,"total":15},
            {"index":4,"type":"freeSpinEnd","amount":2500}]}"#;
        // Façon BLOOD WATER : pas d'événement de retrigger, un repas = un spin ;
        // 4 repas montent le total, le dernier est au plafond.
        let bloodwater = r#"{"id":2,"payoutMultiplier":154380,"criteria":"freegame","events":[
            {"index":0,"type":"freeSpinTrigger","totalFs":6,"positions":[{"reel":2,"row":3},{"reel":2,"row":4}]},
            {"index":1,"type":"updateFreeSpin","amount":1,"total":6},
            {"index":2,"type":"sharkFeed","hunts":[{"fish":"F","totalFs":7}],"sharks":[{"reel":2,"row":3,"multiplier":1}]},
            {"index":3,"type":"updateFreeSpin","amount":2,"total":7},
            {"index":4,"type":"sharkFeed","hunts":[{"fish":"F","totalFs":8},{"fish":"F","totalFs":9},{"fish":"M","totalFs":10}]},
            {"index":5,"type":"sharkFeed","hunts":[{"fish":"F","totalFs":10}]},
            {"index":6,"type":"updateFreeSpin","amount":3,"total":10},
            {"index":7,"type":"freeSpinEnd","amount":154140}]}"#;
        let no_bonus = r#"{"id":3,"payoutMultiplier":40,"criteria":"basegame","events":[
            {"index":0,"type":"reveal","gameType":"basegame","board":[[{"name":"H1"}]]},
            {"index":1,"type":"sharkFeed","hunts":[{"fish":"X","totalFs":0}]},
            {"index":2,"type":"finalWin","amount":40}]}"#;
        // Une ligne par book, comme dans les fichiers du math-sdk.
        let jsonl: String = [sdk, bloodwater, no_bonus]
            .iter()
            .map(|b| b.lines().map(str::trim).collect::<String>() + "\n")
            .collect();

        let dir = scratch("import-retriggers");
        let src = dir.join("books_BASE.jsonl.zst");
        std::fs::write(&src, zstd::bulk::compress(jsonl.as_bytes(), 3).unwrap()).unwrap();
        let index = import(&src, &dir);
        assert_eq!(index.storage, Storage::InPlace);

        let got = |idx: &ModeBookIndex| -> Vec<(u64, u16, u16, u8)> {
            idx.books
                .iter()
                .map(|b| (b.sim_id, b.fs_triggers, b.fs_retriggers, b.fs_tier))
                .collect()
        };
        // (id, freeSpinTrigger, retriggers, palier) : `fs_triggers` garde son
        // sens, `fs_retriggers` distingue les deux bonus, `fs_tier` compte les
        // positions du déclenchement.
        let expected = vec![(1, 1, 1, 3), (2, 1, 4, 2), (3, 0, 0, 0)];
        assert_eq!(got(&index), expected);
        // Relu depuis le disque : le champ survit à l'aller-retour bincode.
        let back = ModeBookIndex::read_from(&dir.join("books_BASE.idx")).unwrap();
        assert_eq!(got(&back), expected);
        assert!(back.event_type_names.iter().any(|t| t == "sharkFeed"));
        // Spins du premier bonus (attribués, joués) et gain du bonus seul : lu
        // quand le book le dit, absent sinon.
        let extra: Vec<(u16, u16, Option<u64>)> = back
            .books
            .iter()
            .map(|b| (b.fs_spins_initial, b.fs_spins_final, b.fg_win_x100))
            .collect();
        assert_eq!(extra, [(10, 15, Some(2500)), (6, 10, None), (0, 0, None)]);
        // Lu dans la source même : rien d'autre que l'index et le point de départ.
        assert!(!dir.join("books_BASE.blocks").exists() && !dir.join("books_BASE.zcopy").exists());
        let books = ModeBooks::open(&dir, "BASE", &src).unwrap();
        let line = books.book_line(2).unwrap();
        assert!(line.starts_with(r#"{"id":2,"#) && line.ends_with("]}"), "{line}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Book-like lines, enough of them for several resume points once the
    /// spacing is small — and a few blank lines, which are not books.
    fn many_books(n: usize) -> String {
        let mut s = String::new();
        let mut x: u64 = 0x2545F4914F6CDD1D;
        for id in 0..n {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let k = (x % 40) as usize;
            let reveal: String = (0..k)
                .map(|i| format!(r#"{{"name":"{}"}}"#, ["H1", "L2", "L3", "W"][(x as usize + i) % 4]))
                .collect::<Vec<_>>()
                .join(",");
            s.push_str(&format!(
                r#"{{"id":{id},"payoutMultiplier":{},"criteria":"basegame","events":[{{"index":0,"type":"reveal","board":[[{reveal}]]}}]}}"#,
                x % 5000
            ));
            s.push('\n');
            if id % 997 == 0 {
                s.push('\n');
            }
        }
        s
    }

    #[test]
    fn every_book_reads_back_through_resume_points() {
        // 64 MiB between resume points would need a huge fixture: the same
        // machinery is exercised by reading each book alone, then all of
        // them in parallel, on a stream of several frames.
        let text = many_books(60_000);
        let dir = scratch("import-readback");
        let src = dir.join("books_BASE.jsonl.zst");
        let half = text.len() / 2;
        let cut = half + text[half..].find('\n').unwrap() + 1;
        let mut file = zstd::bulk::compress(&text.as_bytes()[..cut], 3).unwrap();
        file.extend(zstd::bulk::compress(&text.as_bytes()[cut..], 9).unwrap());
        std::fs::write(&src, &file).unwrap();

        let index = import(&src, &dir);
        assert_eq!((index.books.len(), index.storage), (60_000, Storage::InPlace));
        let resumes = index.checkpoints.iter().filter(|c| c.resume.is_some()).count();
        let frames = index.checkpoints.iter().filter(|c| c.resume.is_none()).count();
        assert!(resumes > 10, "{resumes} resume points");
        assert_eq!(frames, 2, "the start, and the start of the second frame");
        assert!(index.checkpoints.windows(2).all(|w| w[0].dec < w[1].dec));
        let books = ModeBooks::open(&dir, "BASE", &src).unwrap();
        let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
        for id in [0u64, 1, 996, 997, 998, 29_999, 30_000, 59_999] {
            assert_eq!(books.book_line(id).unwrap(), lines[id as usize], "book {id}");
        }
        let seen = books
            .par_fold(
                Vec::new,
                |v: &mut Vec<(usize, String)>, i, line| v.push((i, String::from_utf8(line.to_vec()).unwrap())),
                |mut a, mut b| {
                    a.append(&mut b);
                    a
                },
                None,
            )
            .unwrap();
        assert_eq!(seen.len(), 60_000);
        for (i, line) in seen {
            assert_eq!(line, lines[i]);
        }
        assert_eq!(books.first_lines(3).unwrap(), lines[..3]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_copy_reads_the_same_books() {
        // The fallback when the source cannot be entered in the middle: the
        // same stream, re-cut into independent frames next to the index.
        let text = many_books(20_000);
        let dir = scratch("import-copy");
        let src = dir.join("books_BASE.jsonl.zst");
        std::fs::write(&src, zstd::bulk::compress(text.as_bytes(), 3).unwrap()).unwrap();
        let progress = ImportProgress::default();
        let index = import_as(
            &src,
            &dir,
            "BASE",
            42,
            &progress,
            &SymbolMap::default(),
            &AtomicBool::new(false),
            Storage::Copy,
            256 << 10,
        )
        .unwrap();
        assert_eq!(index.storage, Storage::Copy);
        assert!(index.checkpoints.len() > 1 && index.checkpoints.iter().all(|c| c.resume.is_none()));
        assert!(dir.join("books_BASE.zcopy").exists() && !dir.join("books_BASE.ckpt").exists());
        let books = ModeBooks::open(&dir, "BASE", &src).unwrap();
        let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
        for id in [0u64, 4_321, 19_999] {
            assert_eq!(books.book_line(id).unwrap(), lines[id as usize]);
        }
        let n = books.par_fold(|| 0usize, |n, _, _| *n += 1, |a, b| a + b, None).unwrap();
        assert_eq!(n, 20_000);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_rewritten_source_is_refused_not_misread() {
        let text = many_books(5_000);
        let dir = scratch("import-rewrite");
        let src = dir.join("books_BASE.jsonl.zst");
        std::fs::write(&src, zstd::bulk::compress(text.as_bytes(), 3).unwrap()).unwrap();
        import(&src, &dir);
        let books = ModeBooks::open(&dir, "BASE", &src).unwrap();
        assert!(books.book_line(10).is_ok());
        // Rewritten IN PLACE (same inode), as `open(…, "wb")` does.
        let other = many_books(6_000);
        std::fs::write(&src, zstd::bulk::compress(other.as_bytes(), 5).unwrap()).unwrap();
        let err = books.book_line(10).unwrap_err().to_string();
        assert!(err.contains("changed on disk"), "{err}");
        assert!(ModeBooks::open(&dir, "BASE", &src).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Compares an index with one the import wrote before v10 (`.blocks`
    /// era) for the same books: every field the panels read must be equal.
    fn same_as_v9(index: &ModeBookIndex, old_path: &str) {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct OldBlock {
            offset: u64,
            compressed_len: u32,
            uncompressed_len: u32,
        }
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct OldRecord {
            sim_id: u64,
            block: u32,
            offset_in_block: u32,
            len: u32,
            payout_x100: u64,
            criteria: u16,
            event_bits: u64,
            fs_triggers: u16,
            fs_retriggers: u16,
            fs_tier: u8,
            fs_spins_initial: u16,
            fs_spins_final: u16,
            fg_win_x100: Option<u64>,
            sym_low: u16,
            sym_high: u16,
            sym_low_fs: u16,
            sym_high_fs: u16,
            antic: bool,
        }
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct OldIndex {
            version: u32,
            source_fingerprint: u64,
            blocks: Vec<OldBlock>,
            books: Vec<OldRecord>,
            criteria_names: Vec<String>,
            event_type_names: Vec<String>,
        }
        let bytes = std::fs::read(old_path).unwrap();
        let old: OldIndex = bincode::deserialize(&bytes[8..]).unwrap();
        assert_eq!(old.version, 9);
        assert_eq!(old.books.len(), index.books.len());
        let crit = |names: &[String], c: u16| names.get(c as usize).cloned();
        let events = |names: &[String], bits: u64| -> Vec<String> {
            let mut v: Vec<String> = (0..64).filter(|i| bits >> i & 1 == 1).map(|i| names[i].clone()).collect();
            v.sort();
            v
        };
        for (o, n) in old.books.iter().zip(&index.books) {
            assert_eq!(o.sim_id, n.sim_id);
            assert_eq!(o.len, n.len + 1, "book {}: line length", n.sim_id);
            assert_eq!(
                (o.payout_x100, o.fs_triggers, o.fs_retriggers, o.fs_tier, o.fs_spins_initial, o.fs_spins_final, o.fg_win_x100),
                (n.payout_x100, n.fs_triggers, n.fs_retriggers, n.fs_tier, n.fs_spins_initial, n.fs_spins_final, n.fg_win_x100),
                "book {}",
                n.sim_id
            );
            assert_eq!(
                (o.sym_low, o.sym_high, o.sym_low_fs, o.sym_high_fs, o.antic),
                (n.sym_low, n.sym_high, n.sym_low_fs, n.sym_high_fs, n.antic),
                "book {}",
                n.sim_id
            );
            assert_eq!(crit(&old.criteria_names, o.criteria), crit(&index.criteria_names, n.criteria));
            assert_eq!(
                events(&old.event_type_names, o.event_bits),
                events(&index.event_type_names, n.event_bits),
                "book {}",
                n.sim_id
            );
        }
        println!("index identical to the previous import's ({} books, field by field)", index.books.len());
    }

    /// Importe un vrai fichier de books, relit chaque book depuis les points de
    /// reprise, et affiche ce que coûte l'index :
    /// `BOOKS=…/books_BONUS.jsonl.zst cargo test -p slotlab-app --release real_books
    /// -- --ignored --nocapture`. `FRONT=<build du front>` lit les noms de
    /// symboles à travers sa table (`registry::front_symbols`). Le cache est
    /// écrit sous `IMPORT_OUT` (sinon le dossier temporaire du système), puis
    /// supprimé.
    #[test]
    #[ignore]
    fn real_books() {
        let src = std::path::PathBuf::from(std::env::var("BOOKS").expect("set BOOKS"));
        let out = std::env::var("IMPORT_OUT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir())
            .join(format!("slotlab-real-import-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        let t = std::time::Instant::now();
        let front = std::env::var("FRONT").ok().map(std::path::PathBuf::from);
        let symbols = crate::registry::front_symbols(front.as_deref());
        let index = import_mode(
            &src,
            &out,
            "REAL",
            0,
            Arc::new(ImportProgress::default()),
            &symbols,
            &AtomicBool::new(false),
        )
        .unwrap();
        let took = t.elapsed().as_secs_f64();
        let size = |p: PathBuf| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let src_len = size(src.clone());
        let (idx, ckpt) = (size(out.join("books_REAL.idx")), size(out.join("books_REAL.ckpt")));
        println!(
            "{} books ({:.2} GB of JSON) imported in {:.1} s — {:?}, {} starting points",
            index.books.len(),
            index.stream_len as f64 / 1e9,
            took,
            index.storage,
            index.checkpoints.len()
        );
        println!(
            "cache: index {:.1} MB + resume points {:.1} MB = {:.1} % of the {:.0} MB source",
            idx as f64 / 1e6,
            ckpt as f64 / 1e6,
            (idx + ckpt) as f64 * 100.0 / src_len as f64,
            src_len as f64 / 1e6
        );
        if let Ok(old) = std::env::var("OLD_IDX") {
            same_as_v9(&index, &old);
        }
        let books = ModeBooks::open(&out, "REAL", &src).unwrap();
        let mut lat = Vec::new();
        for k in 0..30u64 {
            let rec = &index.books[(k as usize * 7919 * 13) % index.books.len()];
            let t = std::time::Instant::now();
            let line = books.book_line(rec.sim_id).unwrap();
            lat.push(t.elapsed().as_secs_f64() * 1e3);
            #[derive(Deserialize)]
            struct Id {
                id: u64,
            }
            assert_eq!(serde_json::from_str::<Id>(&line).unwrap().id, rec.sim_id);
        }
        lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("one book: median {:.1} ms, max {:.1} ms", lat[lat.len() / 2], lat[lat.len() - 1]);
        let t = std::time::Instant::now();
        let n = books.par_fold(|| 0u64, |n, _, _| *n += 1, |a, b| a + b, None).unwrap();
        println!("every book read in parallel: {n} in {:.2} s", t.elapsed().as_secs_f64());
        if std::env::var("MECH").is_ok() {
            let t = std::time::Instant::now();
            let mech = super::super::mech::build(&books, None).unwrap();
            println!(
                "mechanics: {} facets, {} occurrences in {:.1} s",
                mech.facets.len(),
                mech.entries.len(),
                t.elapsed().as_secs_f64()
            );
            // OLD_MECH: a `.mech` written by the import before v10 (bincode
            // right after the magic), for the same books.
            if let Ok(old) = std::env::var("OLD_MECH") {
                let bytes = std::fs::read(old).unwrap();
                let old: super::super::format::MechFile = bincode::deserialize(&bytes[8..]).unwrap();
                // Facet and value numbers follow a HashMap's order (they
                // never were stable): compare by (facet, value) names.
                let name = |m: &slotlab_core::mechanics::MechIndex, v: u32| {
                    let (f, val) = &m.values[v as usize];
                    (m.facets[*f as usize].clone(), val.clone())
                };
                let new_id: std::collections::HashMap<(String, String), u32> =
                    (0..mech.values.len() as u32).map(|v| (name(&mech, v), v)).collect();
                assert_eq!(old.mech.values.len(), mech.values.len());
                assert_eq!(old.mech.offsets, mech.offsets);
                let map: Vec<u32> = (0..old.mech.values.len() as u32)
                    .map(|v| new_id[&name(&old.mech, v)])
                    .collect();
                for b in 0..mech.offsets.len() - 1 {
                    let span = |m: &slotlab_core::mechanics::MechIndex| m.offsets[b] as usize..m.offsets[b + 1] as usize;
                    let mut a: Vec<(u32, u16)> =
                        old.mech.entries[span(&old.mech)].iter().map(|&(v, n)| (map[v as usize], n)).collect();
                    let mut c: Vec<(u32, u16)> = mech.entries[span(&mech)].to_vec();
                    a.sort_unstable();
                    c.sort_unstable();
                    assert_eq!(a, c, "book #{b}");
                }
                println!("mechanics identical to the previous import's (by name, book by book)");
            }
        }
        let mut tiers = std::collections::BTreeMap::<u8, u64>::new();
        let mut dist = std::collections::BTreeMap::<u16, u64>::new();
        for b in &index.books {
            *tiers.entry(b.fs_tier).or_default() += 1;
            *dist.entry(b.fs_retriggers).or_default() += 1;
        }
        println!("retriggers → books : {dist:?}");
        println!("paliers → books : {tiers:?}");
        if std::env::var("KEEP").is_ok() {
            println!("cache kept in {}", out.display());
        } else {
            std::fs::remove_dir_all(&out).ok();
        }
    }

    /// Relit un cache EXISTANT (`CACHE`, mode `MODE`, source `BOOKS`) — par
    /// exemple écrit sur une autre architecture — et compare chaque book à une
    /// décompression intégrale du `.zst`, ligne par ligne :
    /// `CACHE=… MODE=REAL BOOKS=… cargo test -p slotlab-app --release
    /// real_books_read_back -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_books_read_back() {
        use std::io::BufRead;
        let src = PathBuf::from(std::env::var("BOOKS").expect("set BOOKS"));
        let cache = PathBuf::from(std::env::var("CACHE").expect("set CACHE"));
        let mode = std::env::var("MODE").unwrap_or_else(|_| "REAL".into());
        let mut reference = std::collections::HashMap::new();
        let mut r = std::io::BufReader::new(zstd::stream::read::Decoder::new(File::open(&src).unwrap()).unwrap());
        let (mut line, mut at) = (Vec::new(), 0u64);
        loop {
            line.clear();
            let n = r.read_until(b'\n', &mut line).unwrap();
            if n == 0 {
                break;
            }
            let text = line.strip_suffix(b"\n").unwrap_or(&line);
            if !text.trim_ascii().is_empty() {
                reference.insert(at, xxhash_rust::xxh3::xxh3_64(text.trim_ascii_end()));
            }
            at += n as u64;
        }
        let books = ModeBooks::open(&cache, &mode, &src).unwrap();
        let t = std::time::Instant::now();
        let (seen, differ) = books
            .par_fold(
                || (0u64, 0u64),
                |acc, i, line| {
                    acc.0 += 1;
                    let rec = &books.index.books[i];
                    acc.1 += (reference.get(&rec.offset) != Some(&xxhash_rust::xxh3::xxh3_64(line))) as u64;
                },
                |a, b| (a.0 + b.0, a.1 + b.1),
                None,
            )
            .unwrap();
        println!(
            "{} ({}): {seen} books read back in {:.2} s, {differ} differ from the plain decode ({} lines in it)",
            std::env::consts::ARCH,
            books.index.checkpoints.len(),
            t.elapsed().as_secs_f64(),
            reference.len()
        );
        assert_eq!((seen as usize, differ), (reference.len(), 0));
        assert_eq!(books.book_line(books.index.books[books.index.books.len() / 2].sim_id).is_ok(), true);
    }
}
