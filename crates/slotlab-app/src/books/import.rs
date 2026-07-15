use super::format::{BlockInfo, BookRecord, ModeBookIndex, IDX_VERSION};
use anyhow::{Context, Result};
use rayon::prelude::*;
use serde::Deserialize;
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
#[derive(Deserialize)]
struct BookMetaRaw {
    id: u64,
    #[serde(default, rename = "payoutMultiplier")]
    payout_multiplier: f64,
    #[serde(default)]
    criteria: Option<String>,
    #[serde(default)]
    events: Vec<EventTypeOnly>,
}

#[derive(Deserialize)]
struct EventTypeOnly {
    #[serde(rename = "type", default)]
    typ: String,
}

struct CountingReader<R> {
    inner: R,
    counter: Arc<ImportProgress>,
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.counter.bytes_done.fetch_add(n as u64, Ordering::Relaxed);
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
}

struct LocalRecord {
    sim_id: u64,
    offset_in_block: u32,
    len: u32,
    payout_x100: u64,
    criteria_local: u16,
    event_bits_local: u64,
    fs_triggers: u16,
}

/// Parses + compresses a chunk of lines (runs on the rayon pool).
fn process_chunk(lines: &[String], mode_name: &str) -> Result<ChunkOut> {
    let total_len: usize = lines.iter().map(|l| l.len() + 1).sum();
    let mut buf = Vec::with_capacity(total_len);
    let mut records = Vec::with_capacity(lines.len());
    let mut criteria_local: Vec<String> = Vec::new();
    let mut event_types_local: Vec<String> = Vec::new();

    for line in lines {
        let meta: BookMetaRaw = serde_json::from_str(line)
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

        let mut event_bits: u64 = 0;
        let mut fs_triggers: u16 = 0;
        for ev in &meta.events {
            if ev.typ.is_empty() {
                continue;
            }
            let bit = match event_types_local.iter().position(|n| n == &ev.typ) {
                Some(i) => i,
                None if event_types_local.len() < 64 => {
                    event_types_local.push(ev.typ.clone());
                    event_types_local.len() - 1
                }
                None => continue,
            };
            event_bits |= 1u64 << bit;
            if ev.typ.eq_ignore_ascii_case("freeSpinTrigger") {
                fs_triggers = fs_triggers.saturating_add(1);
            }
        }

        records.push(LocalRecord {
            sim_id: meta.id,
            offset_in_block: buf.len() as u32,
            len: (line.len() + 1) as u32,
            // The books' payoutMultiplier is already ×100 (aligned with LUT col 3).
            payout_x100: meta.payout_multiplier.round().max(0.0) as u64,
            criteria_local: criteria,
            event_bits_local: event_bits,
            fs_triggers,
        });
        buf.extend_from_slice(line.as_bytes());
        buf.push(b'\n');
    }

    let compressed = zstd::bulk::compress(&buf, BLOCK_ZSTD_LEVEL)?;
    Ok(ChunkOut {
        compressed,
        uncompressed_len: buf.len() as u32,
        records,
        criteria_local,
        event_types_local,
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
    let file = std::fs::File::open(source)
        .with_context(|| format!("opening {}", source.display()))?;
    progress
        .bytes_total
        .store(file.metadata()?.len(), Ordering::Relaxed);

    let counting = CountingReader { inner: file, counter: progress.clone() };
    let decoder = zstd::stream::read::Decoder::new(counting)?;
    let mut reader = BufReader::with_capacity(4 * 1024 * 1024, decoder);

    let blocks_path = cache_dir.join(format!("books_{mode_name}.blocks"));
    let blocks_tmp = cache_dir.join(format!("books_{mode_name}.blocks.tmp"));
    let mut out = std::io::BufWriter::new(std::fs::File::create(&blocks_tmp)?);

    let mut blocks: Vec<BlockInfo> = Vec::new();
    let mut books: Vec<BookRecord> = Vec::new();
    let mut criteria_names: Vec<String> = Vec::new();
    let mut event_type_names: Vec<String> = Vec::new();
    let mut file_offset: u64 = 0;
    let mut eof = false;

    while !eof {
        // Read a wave of chunks (sequential decoding).
        let mut batch: Vec<Vec<String>> = Vec::with_capacity(BATCH);
        while batch.len() < BATCH && !eof {
            let mut chunk: Vec<String> = Vec::with_capacity(600);
            let mut size = 0usize;
            while size < BLOCK_TARGET {
                let mut line = String::new();
                let n = reader.read_line(&mut line)?;
                if n == 0 {
                    eof = true;
                    break;
                }
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    continue;
                }
                size += trimmed.len() + 1;
                chunk.push(trimmed.to_string());
            }
            if !chunk.is_empty() {
                batch.push(chunk);
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
                });
            }
        }
    }
    out.flush()?;
    drop(out);
    std::fs::rename(&blocks_tmp, &blocks_path)?;

    if !books.windows(2).all(|w| w[0].sim_id <= w[1].sim_id) {
        books.sort_by_key(|b| b.sim_id);
    }

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
