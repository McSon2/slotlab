use super::format::{ModeBookIndex, Storage};
use super::zran::{self, Decoder, Snapshot};
use anyhow::{bail, ensure, Context, Result};
use lru::LruCache;
use rayon::prelude::*;
use std::fs::File;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

/// Parsed books (serde_json::Value) kept for re-reads (replay).
const PARSED_CACHE_ENTRIES: usize = 512;
/// …and at most this much source JSON across them. A parsed `Value` tree is
/// 5-10× its text, so 512 bonus books of a few hundred KB each would hold
/// gigabytes by entry count alone.
const PARSED_CACHE_BYTES: usize = 48 * 1024 * 1024;

/// A mode's books: the index, and the file its decompressed stream is read
/// from — the game's own `books_<MODE>.jsonl.zst` (`Storage::InPlace`), or
/// the re-framed copy when the source could not be entered in the middle.
pub struct ModeBooks {
    pub index: ModeBookIndex,
    file: File,
    file_path: PathBuf,
    /// `(len, mtime)` of `file` when opened. A rewrite IN PLACE (truncate then
    /// write, as Python's `open(…, "wb")` does) changes them: reading on with
    /// the old resume points would decode garbage, so every read checks first.
    /// A file replaced by rename keeps the old inode open here — still
    /// consistent with the index, still served until the re-import.
    stamp: (u64, Option<SystemTime>),
    /// Saved decoder states and windows of the resume points.
    ckpt: Option<File>,
    /// Indices into `index.books`, in stream order.
    by_offset: OnceLock<Vec<u32>>,
}

fn stamp_of(file: &File) -> Result<(u64, Option<SystemTime>)> {
    let meta = file.metadata()?;
    Ok((meta.len(), meta.modified().ok()))
}

fn read_exact_at(file: &File, buf: &mut [u8], mut pos: u64) -> Result<()> {
    let mut done = 0;
    while done < buf.len() {
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::read_at(file, &mut buf[done..], pos)?;
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_read(file, &mut buf[done..], pos)?;
        ensure!(n > 0, "unexpected end of file");
        done += n;
        pos += n as u64;
    }
    Ok(())
}

fn trim_end(line: &[u8]) -> &[u8] {
    line.trim_ascii_end()
}

impl ModeBooks {
    /// Opens a mode's books from its cache directory. `source`: the mode's
    /// `books_<MODE>.jsonl.zst`, read directly when the index says so.
    pub fn open(cache_dir: &Path, mode_name: &str, source: &Path) -> Result<Self> {
        let idx_path = cache_dir.join(format!("books_{mode_name}.idx"));
        let index = ModeBookIndex::read_from(&idx_path)
            .with_context(|| format!("reading {}", idx_path.display()))?;
        let file_path = match index.storage {
            Storage::InPlace => source.to_path_buf(),
            Storage::Copy => cache_dir.join(format!("books_{mode_name}.zcopy")),
        };
        let file = File::open(&file_path)
            .with_context(|| format!("opening {}", file_path.display()))?;
        let ckpt = if index.checkpoints.iter().any(|c| c.resume.is_some()) {
            let p = cache_dir.join(format!("books_{mode_name}.ckpt"));
            Some(File::open(&p).with_context(|| format!("opening {}", p.display()))?)
        } else {
            None
        };
        Self::from_parts(index, file, file_path, ckpt)
    }

    /// Books over files already open — the import checks its own work this way.
    pub fn from_parts(
        index: ModeBookIndex,
        file: File,
        file_path: PathBuf,
        ckpt: Option<File>,
    ) -> Result<Self> {
        let stamp = stamp_of(&file)?;
        if index.storage == Storage::InPlace {
            ensure!(
                stamp.0 == index.source_len,
                "{} is {} bytes, the index was built from {} — re-import the books",
                file_path.display(),
                stamp.0,
                index.source_len
            );
        }
        ensure!(
            index.checkpoints.first().is_some_and(|c| c.dec == 0),
            "books index without a starting point — re-import the books"
        );
        // `find` binary-searches and falls back to a linear scan when the ids
        // are not sorted — which would degrade EVERY spin of the mode into a
        // full walk of its index, silently. Say it once, loudly.
        if !index.books.windows(2).all(|w| w[0].sim_id <= w[1].sim_id) {
            tracing::warn!(
                "books index of {} is not sorted by sim_id ({} books): every lookup degrades to a linear scan — re-import the game",
                file_path.display(),
                index.books.len()
            );
        }
        Ok(Self {
            index,
            file,
            file_path,
            stamp,
            ckpt,
            by_offset: OnceLock::new(),
        })
    }

    pub fn into_index(self) -> ModeBookIndex {
        self.index
    }

    /// Memory the index holds (what the open-books budget counts).
    pub fn heap_bytes(&self) -> usize {
        use super::format::{BookRecord, Checkpoint};
        self.index.books.capacity() * std::mem::size_of::<BookRecord>()
            + self.index.checkpoints.capacity() * std::mem::size_of::<Checkpoint>()
            + self.by_offset.get().map_or(0, |o| o.capacity() * 4)
    }

    /// Refuses to read a file rewritten in place since it was opened.
    fn check_unchanged(&self) -> Result<()> {
        if self.index.storage == Storage::InPlace && stamp_of(&self.file)? != self.stamp {
            bail!(
                "{} changed on disk since it was indexed — SlotLab re-imports it (Games tab)",
                self.file_path.display()
            );
        }
        Ok(())
    }

    /// The last starting point at or before `offset` of the stream.
    fn checkpoint_for(&self, offset: u64) -> usize {
        self.index
            .checkpoints
            .partition_point(|c| c.dec <= offset)
            .saturating_sub(1)
    }

    fn decoder_at(&self, k: usize) -> Result<Decoder<'_>> {
        let cp = &self.index.checkpoints[k];
        let Some(r) = cp.resume else {
            return Decoder::at_frame(&self.file, cp.comp, cp.dec);
        };
        let ckpt = self
            .ckpt
            .as_ref()
            .context("resume points file missing — re-import the books")?;
        let mut blob = vec![0u8; r.blob_len as usize];
        read_exact_at(ckpt, &mut blob, r.blob_offset).context("reading a resume point")?;
        let size = zran::state_size();
        let mut state = zstd::bulk::decompress(&blob, size + r.window_len as usize)
            .context("decompressing a resume point")?;
        ensure!(
            state.len() == size + r.window_len as usize,
            "corrupted resume point — re-import the books"
        );
        let window = state.split_off(size);
        Decoder::resume(
            &self.file,
            Snapshot {
                comp: cp.comp,
                dec: cp.dec,
                window_size: r.window_size,
                checksum: r.checksum,
                state,
                window,
            },
        )
    }

    /// Bytes `[start, end)` of the decompressed stream.
    pub fn read_range(&self, start: u64, end: u64) -> Result<Vec<u8>> {
        self.check_unchanged()?;
        let mut d = self.decoder_at(self.checkpoint_for(start))?;
        let mut out = Vec::with_capacity((end - start) as usize);
        while start + (out.len() as u64) < end {
            let b = d
                .next_block()?
                .context("the books stream ends before the book — re-import the books")?;
            let b_end = b.dec + b.data.len() as u64;
            let want = start + out.len() as u64;
            if b_end <= want {
                continue;
            }
            let from = (want - b.dec) as usize;
            let to = (end.min(b_end) - b.dec) as usize;
            out.extend_from_slice(&b.data[from..to]);
        }
        Ok(out)
    }

    /// Raw JSON line of a book.
    pub fn book_line(&self, sim_id: u64) -> Result<String> {
        let rec = self
            .index
            .find(sim_id)
            .with_context(|| format!("book {sim_id} missing from the index"))?;
        let bytes = self.read_range(rec.offset, rec.offset + rec.len as u64)?;
        Ok(String::from_utf8_lossy(trim_end(&bytes)).into_owned())
    }

    fn by_offset(&self) -> &[u32] {
        self.by_offset.get_or_init(|| {
            let books = &self.index.books;
            let mut order: Vec<u32> = (0..books.len() as u32).collect();
            if !books.windows(2).all(|w| w[0].offset <= w[1].offset) {
                order.sort_by_key(|&i| books[i as usize].offset);
            }
            order
        })
    }

    /// Calls `f(book index, line)` for the books `order[lo..hi]`, in stream
    /// order, decoding once from the starting point before the first.
    fn visit(&self, lo: usize, hi: usize, f: &mut dyn FnMut(usize, &[u8])) -> Result<()> {
        if lo >= hi {
            return Ok(());
        }
        let order = self.by_offset();
        let books = &self.index.books;
        let first = books[order[lo] as usize].offset;
        let mut d = self.decoder_at(self.checkpoint_for(first))?;
        // `buf` holds the stream from `buf_start`: the book being completed,
        // never more than one block ahead of it.
        let mut buf: Vec<u8> = Vec::new();
        let mut buf_start = first;
        let mut next = lo;
        while next < hi {
            let b = d
                .next_block()?
                .context("the books stream ends before its last book — re-import the books")?;
            let want = buf_start + buf.len() as u64;
            let b_end = b.dec + b.data.len() as u64;
            if b_end <= want {
                continue;
            }
            buf.extend_from_slice(&b.data[(want - b.dec) as usize..]);
            let buf_end = buf_start + buf.len() as u64;
            while next < hi {
                let i = order[next] as usize;
                let (at, end) = (books[i].offset, books[i].offset + books[i].len as u64);
                if end > buf_end {
                    break;
                }
                f(i, trim_end(&buf[(at - buf_start) as usize..(end - buf_start) as usize]));
                next += 1;
            }
            if next < hi {
                let keep = books[order[next] as usize].offset;
                if keep >= buf_end {
                    buf.clear();
                } else {
                    buf.drain(..(keep - buf_start) as usize);
                }
                buf_start = keep;
            }
        }
        Ok(())
    }

    /// The first `n` books of the stream (stream order).
    pub fn first_lines(&self, n: usize) -> Result<Vec<String>> {
        self.check_unchanged()?;
        let mut out = Vec::with_capacity(n);
        self.visit(0, n.min(self.index.books.len()), &mut |_, line| {
            out.push(String::from_utf8_lossy(line).into_owned());
        })?;
        Ok(out)
    }

    /// Every book once, in parallel: one task per stretch between two
    /// starting points, each decoding its stretch alone. `fold(acc, book
    /// index, line)` sees the books of a stretch in stream order; the
    /// accumulators are then merged by `reduce`. `done` counts the books.
    pub fn par_fold<A, I, F, R>(
        &self,
        identity: I,
        fold: F,
        reduce: R,
        done: Option<&AtomicU64>,
    ) -> Result<A>
    where
        A: Send,
        I: Fn() -> A + Sync + Send,
        F: Fn(&mut A, usize, &[u8]) + Sync + Send,
        R: Fn(A, A) -> A + Sync + Send,
    {
        self.check_unchanged()?;
        let order = self.by_offset();
        let books = &self.index.books;
        let cps = &self.index.checkpoints;
        let bound = |k: usize| -> usize {
            match cps.get(k) {
                Some(c) => order.partition_point(|&i| books[i as usize].offset < c.dec),
                None => order.len(),
            }
        };
        (0..cps.len())
            .into_par_iter()
            .map(|k| -> Result<A> {
                let (lo, hi) = (bound(k), bound(k + 1));
                let mut acc = identity();
                self.visit(lo, hi, &mut |i, line| fold(&mut acc, i, line))?;
                if let Some(d) = done {
                    d.fetch_add((hi - lo) as u64, Ordering::Relaxed);
                }
                Ok(acc)
            })
            .try_reduce(&identity, |a, b| Ok(reduce(a, b)))
    }
}

type BookKey = (String, String, u64); // (game, mode, sim_id)

/// Process-wide cache of parsed books (LRU), for replay-style re-reads.
pub struct BookStore {
    parsed: Mutex<ParsedCache>,
}

/// The book's `events`, with the size of the JSON line it was parsed from.
struct ParsedCache {
    lru: LruCache<BookKey, (Arc<serde_json::Value>, usize)>,
    bytes: usize,
    /// At most this much source JSON (`PARSED_CACHE_BYTES` by default).
    budget: usize,
}

impl ParsedCache {
    fn put(&mut self, key: BookKey, value: Arc<serde_json::Value>, len: usize) {
        if let Some((_, (_, old))) = self.lru.push(key, (value, len)) {
            self.bytes -= old;
        }
        self.bytes += len;
        while self.bytes > self.budget && self.lru.len() > 1 {
            match self.lru.pop_lru() {
                Some((_, (_, n))) => self.bytes -= n,
                None => break,
            }
        }
    }

    fn retain(&mut self, keep: impl Fn(&BookKey) -> bool) {
        let gone: Vec<BookKey> = self
            .lru
            .iter()
            .filter(|(k, _)| !keep(k))
            .map(|(k, _)| k.clone())
            .collect();
        for k in gone {
            if let Some((_, n)) = self.lru.pop(&k) {
                self.bytes -= n;
            }
        }
    }
}

impl Default for BookStore {
    fn default() -> Self {
        Self {
            parsed: Mutex::new(ParsedCache {
                lru: LruCache::new(NonZeroUsize::new(PARSED_CACHE_ENTRIES).unwrap()),
                bytes: 0,
                budget: PARSED_CACHE_BYTES,
            }),
        }
    }
}

impl BookStore {
    /// Drops every parsed book of a game (`mode`: of that mode only).
    ///
    /// MANDATORY after a re-import: the cache is keyed by `(game, mode,
    /// sim_id)`, which the new books reuse — it would keep serving the OLD
    /// book for every id already read. Case-insensitive: the RGS keys with
    /// the URL's game name, imports with the registry entry's.
    pub fn purge(&self, game: &str, mode: Option<&str>) {
        self.parsed.lock().unwrap().retain(|k| {
            !(k.0.eq_ignore_ascii_case(game) && mode.is_none_or(|m| k.1.eq_ignore_ascii_case(m)))
        });
    }

    /// Source JSON kept parsed, at most.
    pub fn set_parsed_budget(&self, bytes: usize) {
        let mut p = self.parsed.lock().unwrap();
        p.budget = bytes;
        while p.bytes > p.budget && p.lru.len() > 1 {
            match p.lru.pop_lru() {
                Some((_, (_, n))) => p.bytes -= n,
                None => break,
            }
        }
    }

    /// Drops every parsed book, all games.
    pub fn clear_all(&self) {
        let mut p = self.parsed.lock().unwrap();
        p.lru.clear();
        p.bytes = 0;
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
            id: Option<u64>,
            events: Option<serde_json::Value>,
        }
        let key: BookKey = (game.to_string(), mode.to_string(), sim_id);
        if let Some((v, _)) = self.parsed.lock().unwrap().lru.get(&key) {
            return Ok(v.clone());
        }
        let line = books.book_line(sim_id)?;
        let book: Book = serde_json::from_str(&line).context("parse book JSON")?;
        // The line at the indexed offset IS this book, or the file is not the
        // one indexed: never hand a player another round than the LUT drew.
        if let Some(id) = book.id {
            ensure!(
                id == sim_id,
                "book {sim_id} read back as book {id} — the books file changed since it was indexed, re-import the books"
            );
        }
        let events = Arc::new(book.events.context(format!("book {sim_id} has no events"))?);
        self.parsed.lock().unwrap().put(key, events.clone(), line.len());
        Ok(events)
    }
}
