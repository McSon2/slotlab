use super::format::ModeBookIndex;
use anyhow::{Context, Result};
use lru::LruCache;
use memmap2::Mmap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Number of decompressed blocks kept in RAM (~2 MiB each → ~256 MiB).
const BLOCK_CACHE_ENTRIES: usize = 128;
/// Parsed books (serde_json::Value) kept for re-reads (replay).
const PARSED_CACHE_ENTRIES: usize = 512;

/// A mode's books: index + mmap of the .blocks file.
pub struct ModeBooks {
    pub index: ModeBookIndex,
    mmap: Mmap,
}

impl ModeBooks {
    pub fn open(cache_dir: &PathBuf, mode_name: &str) -> Result<Self> {
        let idx_path = cache_dir.join(format!("books_{mode_name}.idx"));
        let blocks_path = cache_dir.join(format!("books_{mode_name}.blocks"));
        let index = ModeBookIndex::read_from(&idx_path)
            .with_context(|| format!("reading {}", idx_path.display()))?;
        let file = std::fs::File::open(&blocks_path)
            .with_context(|| format!("opening {}", blocks_path.display()))?;
        // SAFETY: cache file owned by SlotLab, replaced only by atomic
        // rename.
        let mmap = unsafe { Mmap::map(&file)? };
        Ok(Self { index, mmap })
    }
}

type BlockKey = (String, String, u32); // (game, mode, block_no)
type BookKey = (String, String, u64); // (game, mode, sim_id)

/// Process-wide cache: decompressed blocks (LRU) + parsed books (LRU).
pub struct BookStore {
    blocks: Mutex<LruCache<BlockKey, Arc<Vec<u8>>>>,
    parsed: Mutex<LruCache<BookKey, Arc<serde_json::Value>>>,
}

impl Default for BookStore {
    fn default() -> Self {
        Self {
            blocks: Mutex::new(LruCache::new(
                NonZeroUsize::new(BLOCK_CACHE_ENTRIES).unwrap(),
            )),
            parsed: Mutex::new(LruCache::new(
                NonZeroUsize::new(PARSED_CACHE_ENTRIES).unwrap(),
            )),
        }
    }
}

impl BookStore {
    /// Raw JSON line of a book (slice of the decompressed block).
    pub fn book_line(
        &self,
        game: &str,
        mode: &str,
        books: &ModeBooks,
        sim_id: u64,
    ) -> Result<String> {
        let rec = books
            .index
            .find(sim_id)
            .with_context(|| format!("book {sim_id} missing from index {mode}"))?;
        let key: BlockKey = (game.to_string(), mode.to_string(), rec.block);
        let block = {
            let mut cache = self.blocks.lock().unwrap();
            cache.get(&key).cloned()
        };
        let block = match block {
            Some(b) => b,
            None => {
                let info = books
                    .index
                    .blocks
                    .get(rec.block as usize)
                    .context("block out of index")?;
                let start = info.offset as usize;
                let end = start + info.compressed_len as usize;
                let compressed = books
                    .mmap
                    .get(start..end)
                    .context("block out of the .blocks file")?;
                let raw = zstd::bulk::decompress(
                    compressed,
                    info.uncompressed_len as usize,
                )?;
                let arc = Arc::new(raw);
                self.blocks.lock().unwrap().put(key, arc.clone());
                arc
            }
        };
        let start = rec.offset_in_block as usize;
        let end = start + rec.len as usize;
        let line = block
            .get(start..end)
            .context("book out of the decompressed block")?;
        Ok(String::from_utf8_lossy(line).trim_end().to_string())
    }

    /// Parsed book (with LRU cache for replay-style re-reads).
    pub fn book_value(
        &self,
        game: &str,
        mode: &str,
        books: &ModeBooks,
        sim_id: u64,
    ) -> Result<Arc<serde_json::Value>> {
        let key: BookKey = (game.to_string(), mode.to_string(), sim_id);
        if let Some(v) = self.parsed.lock().unwrap().get(&key).cloned() {
            return Ok(v);
        }
        let line = self.book_line(game, mode, books, sim_id)?;
        let value: serde_json::Value =
            serde_json::from_str(&line).context("parse book JSON")?;
        let arc = Arc::new(value);
        self.parsed.lock().unwrap().put(key, arc.clone());
        Ok(arc)
    }
}
