use super::format::{BlockInfo, BookRecord, MechFile, ModeBookIndex, IDX_VERSION};
use anyhow::{Context, Result};
use rayon::prelude::*;
use serde::Deserialize;
use serde_json::value::RawValue;
use slotlab_core::freespins::{summarize, FsEvent};
use slotlab_core::mechanics::{scan_book_with, MechIndex, MAX_VALUES};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Target size of a decompressed block (~200-500 books per block).
const BLOCK_TARGET: usize = 2 * 1024 * 1024;
/// Blocks' zstd level: 3 = good ratio/speed for repetitive JSON.
const BLOCK_ZSTD_LEVEL: i32 = 3;
/// Chunks processed per parallel wave (bounds RAM: ~BATCH × 2 MiB × 2).
const BATCH: usize = 32;

/// Shared progress of an import (read by the status API).
#[derive(Debug, Default)]
pub struct ImportProgress {
    /// COMPRESSED bytes consumed from the source file.
    pub bytes_done: AtomicU64,
    pub bytes_total: AtomicU64,
    pub books_done: AtomicU64,
}

/// Minimal fields extracted from each book for the search index.
///
/// Les événements sont lus par `FsEvent` : leur type (bits d'événements,
/// `fs_triggers`), les annonces de total de free spins (`fs_retriggers`) et
/// la longueur de `positions` (`fs_tier`), tout le reste sauté sans être
/// construit — le type est même emprunté à la ligne au lieu d'être copié.
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

/// Les événements en texte brut, pour le relevé des mécaniques
/// (`slotlab_core::mechanics`) — une seconde lecture de la ligne, qui ne
/// construit rien.
#[derive(Deserialize)]
struct BookEventsRaw<'a> {
    #[serde(default, borrow)]
    events: Vec<&'a RawValue>,
}

/// Facettes et valeurs d'un chunk, identifiants LOCAUX. Une facette qui dépasse
/// `MAX_VALUES` valeurs est marquée continue : elle sera écartée partout.
#[derive(Default)]
struct MechLocal {
    facets: Vec<String>,
    facet_ids: HashMap<String, u32>,
    over: Vec<bool>,
    nvals: Vec<usize>,
    values: Vec<(u32, String)>,
    /// Per facet id: value → value id. Looked up by `&str`, so the hundreds of
    /// millions of occurrences an import reads allocate nothing once seen.
    value_ids: Vec<HashMap<String, u32>>,
}

impl MechLocal {
    fn facet_id(&mut self, facet: &str) -> u32 {
        if let Some(&f) = self.facet_ids.get(facet) {
            return f;
        }
        let f = self.facets.len() as u32;
        self.facets.push(facet.to_string());
        self.facet_ids.insert(facet.to_string(), f);
        self.over.push(false);
        self.nvals.push(0);
        self.value_ids.push(HashMap::new());
        f
    }

    fn value_id(&mut self, facet: &str, value: &str) -> Option<u32> {
        let f = self.facet_id(facet);
        if self.over[f as usize] {
            return None;
        }
        if let Some(&v) = self.value_ids[f as usize].get(value) {
            return Some(v);
        }
        if self.nvals[f as usize] >= MAX_VALUES {
            self.over[f as usize] = true;
            return None;
        }
        self.nvals[f as usize] += 1;
        let v = self.values.len() as u32;
        self.values.push((f, value.to_string()));
        self.value_ids[f as usize].insert(value.to_string(), v);
        Some(v)
    }
}

struct CountingReader<R> {
    inner: R,
    counter: Arc<ImportProgress>,
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.counter
            .bytes_done
            .fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// Result of processing ONE chunk in parallel (future block).
struct ChunkOut {
    compressed: Vec<u8>,
    uncompressed_len: u32,
    /// Records with criteria/event indices LOCAL to the chunk.
    records: Vec<LocalRecord>,
    criteria_local: Vec<String>,
    event_types_local: Vec<String>,
    mech: MechLocal,
}

struct LocalRecord {
    sim_id: u64,
    offset_in_block: u32,
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
    /// Occurrences (valeur locale, nombre) des mécaniques du book.
    mech: Vec<(u32, u16)>,
}

/// A future block: its lines, each followed by `\n`, already laid out as the
/// block will be stored — compressed as is, never copied.
struct Chunk {
    buf: Vec<u8>,
    /// (start, length without the `\n`) of each line in `buf`.
    lines: Vec<(usize, usize)>,
}

/// Parses + compresses a chunk of lines (runs on the rayon pool).
fn process_chunk(chunk: &Chunk, mode_name: &str) -> Result<ChunkOut> {
    let mut records = Vec::with_capacity(chunk.lines.len());
    let mut criteria_local: Vec<String> = Vec::new();
    let mut event_types_local: Vec<String> = Vec::new();
    let mut mech = MechLocal::default();
    let mut ids: Vec<u32> = Vec::new();

    for &(start, len) in &chunk.lines {
        let line = &chunk.buf[start..start + len];
        let meta: BookMetaRaw = serde_json::from_slice(line)
            .with_context(|| format!("invalid book JSON (mode {mode_name})"))?;

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
        let fs = summarize(&meta.events);
        let mut event_bits: u64 = 0;
        for ev in &meta.events {
            if ev.typ.is_empty() {
                continue;
            }
            let bit = match event_types_local.iter().position(|n| *n == ev.typ) {
                Some(i) => i,
                None if event_types_local.len() < 64 => {
                    event_types_local.push(ev.typ.to_string());
                    event_types_local.len() - 1
                }
                None => continue,
            };
            event_bits |= 1u64 << bit;
        }

        // Occurrences → value ids, counted by run after a sort.
        ids.clear();
        if let Ok(raw) = serde_json::from_slice::<BookEventsRaw>(line) {
            scan_book_with(&raw.events, |f, v| {
                if let Some(id) = mech.value_id(f, v) {
                    ids.push(id);
                }
            });
        }
        ids.sort_unstable();
        let mut mech_book: Vec<(u32, u16)> = Vec::new();
        for &id in &ids {
            match mech_book.last_mut() {
                Some((last, n)) if *last == id => *n = n.saturating_add(1),
                _ => mech_book.push((id, 1)),
            }
        }

        records.push(LocalRecord {
            sim_id: meta.id,
            offset_in_block: start as u32,
            len: (len + 1) as u32,
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
            mech: mech_book,
        });
    }

    let compressed = zstd::bulk::compress(&chunk.buf, BLOCK_ZSTD_LEVEL)?;
    Ok(ChunkOut {
        compressed,
        uncompressed_len: chunk.buf.len() as u32,
        records,
        criteria_local,
        event_types_local,
        mech,
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

/// Imports `books_<MODE>.jsonl.zst` → `<cache>/books_<MODE>.blocks` + `.idx`.
/// Pipeline: sequential zstd decoding (the source stream is single-frame) →
/// waves of chunks parsed + re-compressed IN PARALLEL (rayon) → ordered
/// write. JSON parsing, the dominant cost, uses all cores.
pub fn import_mode(
    source: &Path,
    cache_dir: &Path,
    mode_name: &str,
    source_fingerprint: u64,
    progress: Arc<ImportProgress>,
) -> Result<ModeBookIndex> {
    std::fs::create_dir_all(cache_dir)?;
    let file =
        std::fs::File::open(source).with_context(|| format!("opening {}", source.display()))?;
    progress
        .bytes_total
        .store(file.metadata()?.len(), Ordering::Relaxed);

    let counting = CountingReader {
        inner: file,
        counter: progress.clone(),
    };
    let decoder = zstd::stream::read::Decoder::new(counting)?;
    let mut reader = BufReader::with_capacity(4 * 1024 * 1024, decoder);

    let blocks_path = cache_dir.join(format!("books_{mode_name}.blocks"));
    let blocks_tmp = cache_dir.join(format!("books_{mode_name}.blocks.tmp"));
    let mut out = std::io::BufWriter::new(std::fs::File::create(&blocks_tmp)?);

    let mut blocks: Vec<BlockInfo> = Vec::new();
    let mut books: Vec<BookRecord> = Vec::new();
    let mut criteria_names: Vec<String> = Vec::new();
    let mut event_type_names: Vec<String> = Vec::new();
    // Mécaniques, identifiants globaux ; une entrée par book, dans l'ordre de `books`.
    let mut g_mech = MechLocal::default();
    let mut g_over: Vec<bool> = Vec::new();
    let mut mech_books: Vec<Vec<(u32, u16)>> = Vec::new();
    let mut file_offset: u64 = 0;

    // Decoding runs on its own thread, one wave ahead: the source is a single
    // zstd frame (sequential by nature), and waiting for it between waves
    // left every core idle for the ~2 GB/s it takes to inflate.
    std::thread::scope(|scope| -> Result<()> {
        let (tx, rx) = std::sync::mpsc::sync_channel::<std::io::Result<Chunk>>(BATCH);
        scope.spawn(move || {
            let mut line = Vec::new();
            loop {
                let mut chunk = Chunk {
                    buf: Vec::with_capacity(BLOCK_TARGET + 64 * 1024),
                    lines: Vec::with_capacity(600),
                };
                let mut eof = false;
                while chunk.buf.len() < BLOCK_TARGET {
                    line.clear();
                    match reader.read_until(b'\n', &mut line) {
                        Ok(0) => {
                            eof = true;
                            break;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            let _ = tx.send(Err(e));
                            return;
                        }
                    }
                    let trimmed = line.trim_ascii_end();
                    if trimmed.is_empty() {
                        continue;
                    }
                    chunk.lines.push((chunk.buf.len(), trimmed.len()));
                    chunk.buf.extend_from_slice(trimmed);
                    chunk.buf.push(b'\n');
                }
                // A send error means the consumer stopped (import failed).
                if (!chunk.lines.is_empty() && tx.send(Ok(chunk)).is_err()) || eof {
                    return;
                }
            }
        });

        loop {
            // A wave of chunks, in source order.
            let mut batch: Vec<Chunk> = Vec::with_capacity(BATCH);
            while batch.len() < BATCH {
                match rx.recv() {
                    Ok(chunk) => batch.push(chunk?),
                    Err(_) => break, // decoder done
                }
            }
            if batch.is_empty() {
                break;
            }

                // Parse + compression in parallel, order preserved by collect.
                let outs: Vec<Result<ChunkOut>> = batch
                    .par_iter()
                    .map(|chunk| process_chunk(chunk, mode_name))
                    .collect();

                // Ordered write + merge of local tables → global.
                for out_chunk in outs {
                    let c = out_chunk?;
                    let block_no = blocks.len() as u32;
                    out.write_all(&c.compressed)?;
                    blocks.push(BlockInfo {
                        offset: file_offset,
                        compressed_len: c.compressed.len() as u32,
                        uncompressed_len: c.uncompressed_len,
                    });
                    file_offset += c.compressed.len() as u64;

                    // Remap local criteria → global.
                    let criteria_map: Vec<u16> = c
                        .criteria_local
                        .iter()
                        .map(|n| intern(&mut criteria_names, n) as u16)
                        .collect();
                    // Remap local event bits → global.
                    let event_map: Vec<Option<u32>> = c
                        .event_types_local
                        .iter()
                        .map(|n| {
                            let gi = intern(&mut event_type_names, n);
                            (gi < 64).then_some(gi as u32)
                        })
                        .collect();

                    // Remap local mechanics → global ; une facette continue dans un chunk
                    // l'est pour tout le mode.
                    let mech_map: Vec<Option<u32>> = c
                        .mech
                        .values
                        .iter()
                        .map(|(f, v)| g_mech.value_id(&c.mech.facets[*f as usize], v))
                        .collect();
                    for (f, name) in c.mech.facets.iter().enumerate() {
                        let g = g_mech.facet_id(name) as usize;
                        if g_over.len() <= g {
                            g_over.resize(g + 1, false);
                        }
                        g_over[g] |= c.mech.over[f] || g_mech.over[g];
                    }

                    progress
                        .books_done
                        .fetch_add(c.records.len() as u64, Ordering::Relaxed);
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
                            block: block_no,
                            offset_in_block: r.offset_in_block,
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
                        });
                        mech_books.push(
                            r.mech
                                .iter()
                                .filter_map(|&(v, n)| mech_map[v as usize].map(|g| (g, n)))
                                .collect(),
                        );
                    }
                }
        }
        Ok(())
    })?;
    out.flush()?;
    drop(out);
    std::fs::rename(&blocks_tmp, &blocks_path)?;

    if !books.windows(2).all(|w| w[0].sim_id <= w[1].sim_id) {
        let mut order: Vec<usize> = (0..books.len()).collect();
        order.sort_by_key(|&i| books[i].sim_id);
        books = order.iter().map(|&i| books[i].clone()).collect();
        mech_books = order.iter().map(|&i| std::mem::take(&mut mech_books[i])).collect();
    }

    // Le relevé des mécaniques, écrit AVANT l'index : un `.idx` présent veut
    // dire un import complet.
    MechFile {
        version: IDX_VERSION,
        source_fingerprint,
        mech: compact_mech(&g_mech, &g_over, &mech_books),
    }
    .write_to(&cache_dir.join(format!("books_{mode_name}.mech")))?;

    let index = ModeBookIndex {
        version: IDX_VERSION,
        source_fingerprint,
        blocks,
        books,
        criteria_names,
        event_type_names,
    };
    index.write_to(&cache_dir.join(format!("books_{mode_name}.idx")))?;
    Ok(index)
}

/// Écarte les facettes continues, renumérote le reste, aplatit les occurrences par book.
fn compact_mech(g: &MechLocal, over: &[bool], per_book: &[Vec<(u32, u16)>]) -> MechIndex {
    let keep_facet = |f: u32| !over.get(f as usize).copied().unwrap_or(false);
    let mut facet_new: Vec<Option<u32>> = vec![None; g.facets.len()];
    let mut out = MechIndex::default();
    let mut value_new: Vec<Option<u32>> = vec![None; g.values.len()];
    for (v, (f, value)) in g.values.iter().enumerate() {
        if !keep_facet(*f) {
            continue;
        }
        let nf = *facet_new[*f as usize].get_or_insert_with(|| {
            out.facets.push(g.facets[*f as usize].clone());
            (out.facets.len() - 1) as u32
        });
        value_new[v] = Some(out.values.len() as u32);
        out.values.push((nf, value.clone()));
    }
    out.offsets.reserve(per_book.len() + 1);
    out.offsets.push(0);
    for b in per_book {
        out.entries
            .extend(b.iter().filter_map(|&(v, n)| value_new[v as usize].map(|nv| (nv, n))));
        out.offsets.push(out.entries.len() as u32);
    }
    out
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
        let index =
            import_mode(&src, &dir, "BASE", 42, Arc::new(ImportProgress::default())).unwrap();

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
        // Relevé des mécaniques : les repas du book 2 (3 F, 1 M), rangés dans
        // l'ordre des books ; `totalFs` (6 valeurs ici) reste une répartition.
        let mech = MechFile::read_from(&dir.join("books_BASE.mech")).unwrap().mech;
        assert_eq!(mech.offsets.len(), back.books.len() + 1);
        let fish = mech.facets.iter().position(|f| f == "sharkFeed.hunts[].fish").unwrap() as u32;
        let count = |book: usize, value: &str| -> u16 {
            let (a, b) = (mech.offsets[book] as usize, mech.offsets[book + 1] as usize);
            mech.entries[a..b]
                .iter()
                .filter(|(v, _)| mech.values[*v as usize] == (fish, value.to_string()))
                .map(|e| e.1)
                .sum()
        };
        assert_eq!((count(1, "F"), count(1, "M"), count(0, "F"), count(2, "X")), (4, 1, 0, 1));
        assert!(!mech.facets.iter().any(|f| f.contains("board")));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Importe un vrai fichier de books et affiche ses retriggers :
    /// `BOOKS=…/books_BONUS.jsonl.zst cargo test -p slotlab-app --release real_books
    /// -- --ignored --nocapture`. Le cache est écrit sous `IMPORT_OUT` (sinon le
    /// dossier temporaire du système), puis supprimé.
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
        let index =
            import_mode(&src, &out, "REAL", 0, Arc::new(ImportProgress::default())).unwrap();
        println!(
            "{} books importés en {:.1} s",
            index.books.len(),
            t.elapsed().as_secs_f64()
        );
        for b in index.books.iter().take(5) {
            println!(
                "  id {:>7}  freeSpinTrigger={}  retriggers={}",
                b.sim_id, b.fs_triggers, b.fs_retriggers
            );
        }
        let mut dist = std::collections::BTreeMap::<u16, u64>::new();
        for b in &index.books {
            *dist.entry(b.fs_retriggers).or_default() += 1;
        }
        println!("retriggers → books : {dist:?}");
        std::fs::remove_dir_all(&out).ok();
    }
}
