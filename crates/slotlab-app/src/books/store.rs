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
/// …and at most this much source JSON across them. A parsed `Value` tree is
/// 5-10× its text, so 512 bonus books of a few hundred KB each would hold
/// gigabytes by entry count alone.
const PARSED_CACHE_BYTES: usize = 48 * 1024 * 1024;

/// A mode's books: index + mmap of the .blocks file.
pub struct ModeBooks {
    pub index: ModeBookIndex,
    mmap: Mmap,
}

impl ModeBooks {
    /// Decompresses one block, WITHOUT touching the shared LRU.
    ///
    /// A full scan walks every block once and would otherwise evict the
    /// working set the RGS is serving spins from — a cache is for what gets
    /// read twice, and a scan reads each block exactly once.
    pub fn block_bytes(&self, block: usize) -> Result<Vec<u8>> {
        let info = self
            .index
            .blocks
            .get(block)
            .context("block out of index")?;
        let start = info.offset as usize;
        let end = start + info.compressed_len as usize;
        let compressed = self
            .mmap
            .get(start..end)
            .context("block out of the .blocks file")?;
        Ok(zstd::bulk::decompress(
            compressed,
            info.uncompressed_len as usize,
        )?)
    }

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
        // `find` binary-searches and falls back to a linear scan when the ids
        // are not sorted — which would degrade EVERY spin of the mode into a
        // full walk of its index, silently. Say it once, loudly.
        if !index.books.windows(2).all(|w| w[0].sim_id <= w[1].sim_id) {
            tracing::warn!(
                "books index for {mode_name} is not sorted by sim_id ({} books):                  every lookup degrades to a linear scan — re-import the game",
                index.books.len()
            );
        }
        Ok(Self { index, mmap })
    }
}

type BlockKey = (String, String, u32); // (game, mode, block_no)
type BookKey = (String, String, u64); // (game, mode, sim_id)

/// Process-wide cache: decompressed blocks (LRU) + parsed books (LRU).
pub struct BookStore {
    blocks: Mutex<LruCache<BlockKey, Arc<Vec<u8>>>>,
    parsed: Mutex<ParsedCache>,
}

/// The book's `events`, with the size of the JSON line it was parsed from.
struct ParsedCache {
    lru: LruCache<BookKey, (Arc<serde_json::Value>, usize)>,
    bytes: usize,
}

impl ParsedCache {
    fn put(&mut self, key: BookKey, value: Arc<serde_json::Value>, len: usize) {
        if let Some((_, (_, old))) = self.lru.push(key, (value, len)) {
            self.bytes -= old;
        }
        self.bytes += len;
        while self.bytes > PARSED_CACHE_BYTES && self.lru.len() > 1 {
            match self.lru.pop_lru() {
                Some((_, (_, n))) => self.bytes -= n,
                None => break,
            }
        }
    }

    fn clear(&mut self) {
        self.lru.clear();
        self.bytes = 0;
    }
}

impl Default for BookStore {
    fn default() -> Self {
        Self {
            blocks: Mutex::new(LruCache::new(
                NonZeroUsize::new(BLOCK_CACHE_ENTRIES).unwrap(),
            )),
            parsed: Mutex::new(ParsedCache {
                lru: LruCache::new(NonZeroUsize::new(PARSED_CACHE_ENTRIES).unwrap()),
                bytes: 0,
            }),
        }
    }
}

impl BookStore {
    /// Drops every cached block / parsed book of a game.
    ///
    /// MANDATORY after a re-import: the two LRUs are keyed by
    /// `(game, mode, block_no)` and `(game, mode, sim_id)` — keys that a new
    /// `.blocks` file reuses. Keeping them would apply the NEW index's
    /// offsets to an OLD decompressed block (garbage slice → "trailing
    /// characters") and keep serving the OLD books for already-read sim_ids.
    /// Case-insensitive: the RGS keys with the URL's game name, imports with
    /// the registry entry's.
    pub fn purge_game(&self, game: &str) {
        let mut blocks = self.blocks.lock().unwrap();
        let keys: Vec<BlockKey> = blocks
            .iter()
            .filter(|(k, _)| k.0.eq_ignore_ascii_case(game))
            .map(|(k, _)| k.clone())
            .collect();
        for k in keys {
            blocks.pop(&k);
        }
        drop(blocks);
        let mut parsed = self.parsed.lock().unwrap();
        let keys: Vec<BookKey> = parsed
            .lru
            .iter()
            .filter(|(k, _)| k.0.eq_ignore_ascii_case(game))
            .map(|(k, _)| k.clone())
            .collect();
        for k in keys {
            if let Some((_, n)) = parsed.lru.pop(&k) {
                parsed.bytes -= n;
            }
        }
    }

    /// Drops every cached block / parsed book, all games.
    pub fn clear_all(&self) {
        self.blocks.lock().unwrap().clear();
        self.parsed.lock().unwrap().clear();
    }

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
                let raw = zstd::bulk::decompress(compressed, info.uncompressed_len as usize)?;
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

    /// A book's `events` array, parsed (LRU cache for replay-style re-reads).
    /// Only the events are kept: they are all the RGS sends, and a round
    /// shares the cached `Arc` instead of deep-copying it out of the book.
    pub fn book_events(
        &self,
        game: &str,
        mode: &str,
        books: &ModeBooks,
        sim_id: u64,
    ) -> Result<Arc<serde_json::Value>> {
        #[derive(serde::Deserialize)]
        struct Book {
            events: Option<serde_json::Value>,
        }
        let key: BookKey = (game.to_string(), mode.to_string(), sim_id);
        if let Some((v, _)) = self.parsed.lock().unwrap().lru.get(&key) {
            return Ok(v.clone());
        }
        let line = self.book_line(game, mode, books, sim_id)?;
        let book: Book = serde_json::from_str(&line).context("parse book JSON")?;
        let events = Arc::new(book.events.context(format!("book {sim_id} has no events"))?);
        self.parsed.lock().unwrap().put(key, events.clone(), line.len());
        Ok(events)
    }
}
