//! Random access into the books' own `.jsonl.zst`, without re-compressing them.
//!
//! ── Why ─────────────────────────────────────────────────────────────────────
//! A game's books are ONE zstd frame of up to a few GB: reading book N means
//! decompressing every byte before it. SlotLab used to re-compress them into
//! independent 2 MiB blocks it could jump into — a second full copy of every
//! game's books on disk (8.5 GB for Pumpkin Spells, more than the books
//! themselves), rewritten after every `run.py prod`.
//!
//! Between two blocks of a frame, a zstd decoder carries exactly four things:
//! the history window (2 MiB at the levels the math SDKs write), the last
//! Huffman table, the last three FSE tables and the three repeat offsets.
//! `csrc/zstd_resume.c` saves them at a block boundary and restores them in a
//! fresh decoder, which then reads the ORIGINAL file from there. A resume
//! point costs ~150 KB once compressed (the window, mostly); one every 64 MiB
//! of books is ~3 % of the `.zst`, and reaching any book decompresses 32 MiB
//! on average (~15 ms). Every resume point is checked at import time by
//! re-decoding every book from it (`import.rs`).
//!
//! ── How it decodes ──────────────────────────────────────────────────────────
//! libzstd's buffer-less API (`ZSTD_decompressContinue`), one block at a time,
//! into two alternating buffers that each hold more than a window: when the
//! decoder moves to the other buffer, the one it leaves becomes the history it
//! may still reference ("extDict" in libzstd), so it is never overwritten
//! before a full window has been decoded after it.

use anyhow::{bail, ensure, Context, Result};
use std::ffi::{c_char, c_void, CStr};
use std::fs::File;

mod ffi {
    use std::ffi::{c_char, c_void};
    extern "C" {
        pub fn slz_state_size() -> usize;
        pub fn slz_header_version() -> u32;
        pub fn slz_lib_version() -> u32;
        pub fn slz_create() -> *mut c_void;
        pub fn slz_free(d: *mut c_void);
        pub fn slz_begin(d: *mut c_void) -> usize;
        pub fn slz_next_src_size(d: *mut c_void) -> usize;
        pub fn slz_continue(
            d: *mut c_void,
            dst: *mut u8,
            cap: usize,
            src: *const u8,
            n: usize,
        ) -> usize;
        pub fn slz_is_error(code: usize) -> u32;
        pub fn slz_error_name(code: usize) -> *const c_char;
        pub fn slz_stage(d: *const c_void) -> i32;
        pub fn slz_window_size(d: *const c_void) -> u64;
        pub fn slz_checksum_flag(d: *const c_void) -> u32;
        pub fn slz_save(d: *const c_void, out: *mut u8) -> i32;
        pub fn slz_resume(
            d: *mut c_void,
            state: *const u8,
            window: *const u8,
            wlen: usize,
        ) -> usize;
    }
}

// `ZSTD_dStage`, in declaration order.
const STAGE_FRAME_HEADER_SIZE: i32 = 0;
const STAGE_FRAME_HEADER: i32 = 1;
const STAGE_BLOCK_HEADER: i32 = 2;
const STAGE_BLOCK: i32 = 3;
const STAGE_LAST_BLOCK: i32 = 4;
const STAGE_CHECKSUM: i32 = 5;

/// Largest decompressed block (`ZSTD_BLOCKSIZE_MAX`).
const BLOCK_MAX: usize = 128 * 1024;
/// Each of the two output buffers, at least (≥ window + 2 blocks).
const MIN_SEGMENT: usize = 4 << 20;
/// Compressed bytes read from the file at a time.
const READ_AHEAD: usize = 1 << 20;
/// Largest window the decoder accepts (libzstd's own default limit).
const MAX_WINDOW: u64 = 128 << 20;
/// Largest window a resume point carries. Beyond it a resume point would cost
/// more than the span it saves decoding: the books are then copied instead.
pub const MAX_RESUME_WINDOW: u64 = 16 << 20;

/// Bytes of the entropy state saved at a resume point (`slz_state`).
pub fn state_size() -> usize {
    unsafe { ffi::slz_state_size() }
}

/// The shim was compiled against the headers of the libzstd it is linked
/// with — a mismatch would mean the struct layout it relies on is unknown.
pub fn self_check() -> Result<()> {
    let (header, lib) = unsafe { (ffi::slz_header_version(), ffi::slz_lib_version()) };
    ensure!(
        header == lib,
        "zstd resume shim built for zstd {header}, linked with {lib}"
    );
    Ok(())
}

fn check(code: usize) -> Result<usize> {
    unsafe {
        if ffi::slz_is_error(code) != 0 {
            let name = CStr::from_ptr(ffi::slz_error_name(code) as *const c_char);
            bail!("zstd: {}", name.to_string_lossy())
        }
    }
    Ok(code)
}

struct DCtx(*mut c_void);

// The context is only ever used by the decoder that owns it.
unsafe impl Send for DCtx {}

impl Drop for DCtx {
    fn drop(&mut self) {
        unsafe { ffi::slz_free(self.0) }
    }
}

#[cfg(unix)]
fn read_at(f: &File, buf: &mut [u8], pos: u64) -> std::io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(f, buf, pos)
}

#[cfg(windows)]
fn read_at(f: &File, buf: &mut [u8], pos: u64) -> std::io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(f, buf, pos)
}

/// One decoded block: `data` starts at offset `dec` of the decompressed stream.
pub struct Block<'a> {
    pub dec: u64,
    pub data: &'a [u8],
}

/// Everything needed to restart decoding at a block boundary.
pub struct Snapshot {
    /// Offset in the compressed file of the next block header.
    pub comp: u64,
    /// Offset in the decompressed stream at that point.
    pub dec: u64,
    /// The frame's window size (the synthetic header repeats it).
    pub window_size: u64,
    /// The frame ends with a 4-byte checksum (skipped after a resume: it
    /// covers the whole frame, so it cannot be checked from the middle).
    pub checksum: bool,
    /// `state_size()` bytes, opaque.
    pub state: Vec<u8>,
    /// The last `min(window_size, bytes since the frame start)` bytes.
    pub window: Vec<u8>,
}

pub struct Decoder<'f> {
    dctx: DCtx,
    src: &'f File,
    input: Vec<u8>,
    in_start: usize,
    in_end: usize,
    /// File offset of `input[in_end]`.
    file_pos: u64,
    /// File offset of the next byte to hand to the decoder.
    comp: u64,
    /// Decompressed offset of the next output byte.
    dec: u64,
    ring: [Vec<u8>; 2],
    cur: usize,
    fill: usize,
    prev_fill: usize,
    /// The decoder moved to the other buffer at least once since it started
    /// (before that, its history is `resume_window` + `ring[cur]`).
    switched: bool,
    frame_dec_start: u64,
    window_size: u64,
    checksum: bool,
    skip_checksum: bool,
    /// The history a resumed decoder starts from — referenced by libzstd.
    resume_window: Vec<u8>,
    /// Frames begun so far, and where the last one began `(comp, dec)`.
    frames: u64,
    last_frame: (u64, u64),
}

impl<'f> Decoder<'f> {
    fn alloc(src: &'f File, comp: u64, dec: u64) -> Result<Self> {
        let raw = unsafe { ffi::slz_create() };
        ensure!(!raw.is_null(), "zstd: cannot allocate a decoder");
        Ok(Self {
            dctx: DCtx(raw),
            src,
            input: vec![0; READ_AHEAD],
            in_start: 0,
            in_end: 0,
            file_pos: comp,
            comp,
            dec,
            ring: [Vec::new(), Vec::new()],
            cur: 0,
            fill: 0,
            prev_fill: 0,
            switched: false,
            frame_dec_start: dec,
            window_size: 0,
            checksum: false,
            skip_checksum: false,
            resume_window: Vec::new(),
            frames: 0,
            last_frame: (comp, dec),
        })
    }

    /// A decoder at a frame start: offset `comp` of the file, `dec` of the
    /// decompressed stream (0, 0 for the start of the file).
    pub fn at_frame(src: &'f File, comp: u64, dec: u64) -> Result<Self> {
        let d = Self::alloc(src, comp, dec)?;
        check(unsafe { ffi::slz_begin(d.dctx.0) })?;
        Ok(d)
    }

    /// A decoder picking the stream up at a resume point.
    pub fn resume(src: &'f File, s: Snapshot) -> Result<Self> {
        ensure!(
            s.state.len() == state_size(),
            "resume point of {} bytes, expected {}",
            s.state.len(),
            state_size()
        );
        ensure!(
            s.window_size <= MAX_WINDOW && s.window.len() as u64 <= s.window_size,
            "invalid resume point (window {} of {})",
            s.window.len(),
            s.window_size
        );
        let mut d = Self::alloc(src, s.comp, s.dec)?;
        d.size_ring(s.window_size);
        d.resume_window = s.window;
        check(unsafe {
            ffi::slz_resume(
                d.dctx.0,
                s.state.as_ptr(),
                d.resume_window.as_ptr(),
                d.resume_window.len(),
            )
        })?;
        // A frame header with the original window, and nothing else: no
        // content size (the count restarts here), no checksum, no dictionary.
        let header = [0x28, 0xB5, 0x2F, 0xFD, 0x00, window_descriptor(s.window_size)];
        let first = unsafe { ffi::slz_next_src_size(d.dctx.0) };
        ensure!(first == 5, "zstd resume: unexpected header size {first}");
        check(unsafe { ffi::slz_continue(d.dctx.0, std::ptr::null_mut(), 0, header.as_ptr(), 5) })?;
        check(unsafe {
            ffi::slz_continue(d.dctx.0, std::ptr::null_mut(), 0, header[5..].as_ptr(), 1)
        })?;
        ensure!(d.stage() == STAGE_BLOCK_HEADER, "zstd resume: header not accepted");
        d.window_size = s.window_size;
        d.skip_checksum = s.checksum;
        d.frame_dec_start = s.dec - d.resume_window.len() as u64;
        Ok(d)
    }

    pub fn dec_pos(&self) -> u64 {
        self.dec
    }

    pub fn comp_pos(&self) -> u64 {
        self.comp
    }

    /// Window size of the frame being decoded.
    pub fn window_size(&self) -> u64 {
        self.window_size
    }

    /// Frames begun so far and where the last one began `(comp, dec)`.
    pub fn frames(&self) -> (u64, (u64, u64)) {
        (self.frames, self.last_frame)
    }

    fn stage(&self) -> i32 {
        unsafe { ffi::slz_stage(self.dctx.0) }
    }

    /// Between two blocks of a frame: where `snapshot` may be taken.
    pub fn at_block_boundary(&self) -> bool {
        self.stage() == STAGE_BLOCK_HEADER
    }

    fn size_ring(&mut self, window: u64) {
        let seg = MIN_SEGMENT.max(window as usize + 2 * BLOCK_MAX);
        for r in &mut self.ring {
            if r.len() < seg {
                *r = vec![0; seg];
            }
        }
    }

    /// Makes `n` input bytes available; false at the end of the file.
    fn fill_input(&mut self, n: usize) -> Result<bool> {
        if self.in_end - self.in_start >= n {
            return Ok(true);
        }
        if self.in_start > 0 {
            self.input.copy_within(self.in_start..self.in_end, 0);
            self.in_end -= self.in_start;
            self.in_start = 0;
        }
        if self.input.len() < n {
            self.input.resize(n.next_power_of_two(), 0);
        }
        while self.in_end < n {
            let k = read_at(self.src, &mut self.input[self.in_end..], self.file_pos)
                .context("reading the books file")?;
            if k == 0 {
                return Ok(false);
            }
            self.in_end += k;
            self.file_pos += k as u64;
        }
        Ok(true)
    }

    fn skip(&mut self, n: u64) -> Result<()> {
        let buffered = (self.in_end - self.in_start) as u64;
        if n <= buffered {
            self.in_start += n as usize;
        } else {
            self.in_start = 0;
            self.in_end = 0;
            self.file_pos = self.comp + n;
        }
        self.comp += n;
        Ok(())
    }

    /// Hands the next `n` input bytes to libzstd. `out`: the call decodes a
    /// block (else a header, written nowhere — a non-empty `dst` there would
    /// make libzstd file the history away as a discontinuity).
    fn feed(&mut self, n: usize, out: bool) -> Result<usize> {
        let (dst, cap) = if out {
            if self.ring[self.cur].len() - self.fill < BLOCK_MAX {
                self.prev_fill = self.fill;
                self.cur ^= 1;
                self.fill = 0;
                self.switched = true;
            }
            let r = &mut self.ring[self.cur];
            // SAFETY: `fill < r.len()`; the ring is never reallocated while a
            // frame is being decoded (only `size_ring` at a frame start).
            (unsafe { r.as_mut_ptr().add(self.fill) }, r.len() - self.fill)
        } else {
            (std::ptr::null_mut(), 0)
        };
        let src = unsafe { self.input.as_ptr().add(self.in_start) };
        let r = check(unsafe { ffi::slz_continue(self.dctx.0, dst, cap, src, n) })?;
        self.in_start += n;
        self.comp += n as u64;
        Ok(r)
    }

    /// Decodes the next block. `None` at the end of the file.
    pub fn next_block(&mut self) -> Result<Option<Block<'_>>> {
        loop {
            let stage = self.stage();
            let n = unsafe { ffi::slz_next_src_size(self.dctx.0) };
            match stage {
                STAGE_FRAME_HEADER_SIZE if n == 0 => {
                    // A frame just ended.
                    if self.skip_checksum {
                        ensure!(self.fill_input(4)?, "books file truncated (frame checksum)");
                        self.skip(4)?;
                        self.skip_checksum = false;
                    }
                    if !self.fill_input(1)? {
                        return Ok(None);
                    }
                    check(unsafe { ffi::slz_begin(self.dctx.0) })?;
                }
                STAGE_FRAME_HEADER_SIZE => {
                    if !self.fill_input(1)? {
                        return Ok(None);
                    }
                    ensure!(self.fill_input(4)?, "books file: trailing bytes after the last frame");
                    let at = self.in_start;
                    let magic = u32::from_le_bytes(self.input[at..at + 4].try_into().unwrap());
                    if magic & 0xFFFF_FFF0 == 0x184D_2A50 {
                        // Skippable frame: metadata, never books.
                        ensure!(self.fill_input(8)?, "books file truncated (skippable frame)");
                        let size = u32::from_le_bytes(
                            self.input[at + 4..at + 8].try_into().unwrap(),
                        );
                        self.skip(8 + size as u64)?;
                        check(unsafe { ffi::slz_begin(self.dctx.0) })?;
                        continue;
                    }
                    ensure!(self.fill_input(n)?, "books file truncated (frame header)");
                    self.frames += 1;
                    self.last_frame = (self.comp, self.dec);
                    self.frame_dec_start = self.dec;
                    self.feed(n, false)?;
                }
                STAGE_FRAME_HEADER => {
                    ensure!(self.fill_input(n)?, "books file truncated (frame header)");
                    self.feed(n, false)?;
                    self.window_size = unsafe { ffi::slz_window_size(self.dctx.0) };
                    self.checksum = unsafe { ffi::slz_checksum_flag(self.dctx.0) } != 0;
                    ensure!(
                        self.window_size <= MAX_WINDOW,
                        "books compressed with a {} MiB window (at most {} MiB)",
                        self.window_size >> 20,
                        MAX_WINDOW >> 20
                    );
                    // No history crosses a frame boundary: the buffers may grow.
                    self.size_ring(self.window_size);
                }
                STAGE_BLOCK_HEADER | STAGE_CHECKSUM => {
                    ensure!(self.fill_input(n)?, "books file truncated");
                    self.feed(n, false)?;
                }
                STAGE_BLOCK | STAGE_LAST_BLOCK => {
                    ensure!(self.fill_input(n)?, "books file truncated (block)");
                    let r = self.feed(n, true)?;
                    let start = self.fill;
                    let dec = self.dec;
                    self.fill += r;
                    self.dec += r as u64;
                    return Ok(Some(Block {
                        dec,
                        data: &self.ring[self.cur][start..start + r],
                    }));
                }
                other => bail!("zstd: unexpected decoder stage {other}"),
            }
        }
    }

    /// The decoder's state at the current block boundary, or `None` when a
    /// resume point here would need a window over `MAX_RESUME_WINDOW`.
    pub fn snapshot(&self) -> Result<Option<Snapshot>> {
        ensure!(self.at_block_boundary(), "snapshot outside a block boundary");
        let want = self.window_size.min(self.dec - self.frame_dec_start);
        if want > MAX_RESUME_WINDOW {
            return Ok(None);
        }
        let mut state = vec![0u8; state_size()];
        ensure!(
            unsafe { ffi::slz_save(self.dctx.0, state.as_mut_ptr()) } == 0,
            "zstd: cannot save the decoder state"
        );
        let window = self
            .history_tail(want as usize)
            .context("zstd: history shorter than the window")?;
        Ok(Some(Snapshot {
            comp: self.comp,
            dec: self.dec,
            window_size: self.window_size,
            checksum: self.checksum,
            state,
            window,
        }))
    }

    /// The last `want` decompressed bytes, from the buffers (and, right after
    /// a resume, from the window it started with).
    fn history_tail(&self, want: usize) -> Option<Vec<u8>> {
        let cur = &self.ring[self.cur][..self.fill];
        let before: &[u8] = if self.switched {
            &self.ring[self.cur ^ 1][..self.prev_fill]
        } else {
            &self.resume_window
        };
        if want > before.len() + cur.len() {
            return None;
        }
        let mut out = Vec::with_capacity(want);
        if want > cur.len() {
            out.extend_from_slice(&before[before.len() - (want - cur.len())..]);
            out.extend_from_slice(cur);
        } else {
            out.extend_from_slice(&cur[cur.len() - want..]);
        }
        Some(out)
    }
}

/// Smallest Window_Descriptor (RFC 8878 §3.1.1.1.2) covering `size`.
fn window_descriptor(size: u64) -> u8 {
    for exponent in 0u8..32 {
        let base = 1u64 << (10 + exponent as u64);
        for mantissa in 0u8..8 {
            if base + (base / 8) * mantissa as u64 >= size {
                return (exponent << 3) | mantissa;
            }
        }
    }
    u8::MAX
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("slotlab-zran-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Book-like JSON lines plus the block kinds a real stream can hold:
    /// an RLE run, incompressible bytes (raw blocks), long repeats.
    fn corpus(seed: u64, lines: usize) -> Vec<u8> {
        let mut x = seed | 1;
        let mut rnd = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let mut out = Vec::new();
        for id in 0..lines {
            let n = (rnd() % 12) as usize;
            let mut events = String::new();
            for e in 0..n {
                let sym = ["H1", "H2", "L1", "L2", "L3", "W", "S"][(rnd() % 7) as usize];
                events.push_str(&format!(
                    r#"{{"index":{e},"type":"reveal","board":[[{{"name":"{sym}"}},{{"name":"L{}"}}]],"amount":{}}},"#,
                    rnd() % 5,
                    rnd() % 1000
                ));
            }
            events.pop();
            out.extend_from_slice(
                format!(
                    r#"{{"id":{id},"payoutMultiplier":{},"criteria":"basegame","events":[{events}]}}"#,
                    rnd() % 10_000
                )
                .as_bytes(),
            );
            out.push(b'\n');
            if id == lines / 3 {
                out.extend(std::iter::repeat_n(b'a', 300_000));
                out.push(b'\n');
            }
            if id == lines / 2 {
                out.extend((0..200_000).map(|_| (rnd() & 0xFF) as u8));
                out.push(b'\n');
            }
        }
        out
    }

    fn compress(data: &[u8], level: i32, checksum: bool, window_log: u32) -> Vec<u8> {
        let mut enc = zstd::stream::Encoder::new(Vec::new(), level).unwrap();
        enc.include_checksum(checksum).unwrap();
        enc.window_log(window_log).unwrap();
        enc.write_all(data).unwrap();
        enc.finish().unwrap()
    }

    /// Decodes `file` from the start, snapshotting at EVERY block boundary,
    /// then resumes from each snapshot and checks it reproduces the rest of
    /// the stream byte for byte.
    fn every_boundary_resumes(name: &str, file: &[u8], plain: &[u8]) {
        let dir = scratch(name);
        let path = dir.join("books.jsonl.zst");
        std::fs::write(&path, file).unwrap();
        let f = File::open(&path).unwrap();

        let mut d = Decoder::at_frame(&f, 0, 0).unwrap();
        let mut out = Vec::new();
        let mut snaps = Vec::new();
        while let Some(b) = d.next_block().unwrap() {
            assert_eq!(b.dec as usize, out.len());
            out.extend_from_slice(b.data);
            if d.at_block_boundary() {
                if let Some(s) = d.snapshot().unwrap() {
                    snaps.push(s);
                }
            }
        }
        assert_eq!(out, plain, "sequential decode differs from the plain text");
        assert!(snaps.len() > 5, "too few blocks to test ({})", snaps.len());

        for s in snaps {
            let at = s.dec as usize;
            let mut r = Decoder::resume(&f, s).unwrap();
            let mut tail = Vec::new();
            while let Some(b) = r.next_block().unwrap() {
                assert_eq!(b.dec as usize, at + tail.len());
                tail.extend_from_slice(b.data);
            }
            assert!(tail == plain[at..], "resume at {at} decodes something else");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resumes_at_every_block_boundary() {
        self_check().unwrap();
        let plain = corpus(7, 6_000);
        every_boundary_resumes("l3", &compress(&plain, 3, false, 21), &plain);
    }

    #[test]
    fn resumes_with_checksum_and_high_level() {
        let plain = corpus(11, 4_000);
        every_boundary_resumes("l19", &compress(&plain, 19, true, 22), &plain);
    }

    #[test]
    fn resumes_across_frames_and_skippable_frames() {
        // Two frames (the second with a checksum) around a skippable frame:
        // the stream is their concatenation, as for `zstd -dc`.
        let a = corpus(3, 2_500);
        let b = corpus(5, 2_500);
        let mut file = compress(&a, 3, false, 20);
        file.extend_from_slice(&0x184D_2A53u32.to_le_bytes());
        file.extend_from_slice(&5u32.to_le_bytes());
        file.extend_from_slice(b"hello");
        file.extend_from_slice(&compress(&b, 5, true, 21));
        let mut plain = a.clone();
        plain.extend_from_slice(&b);
        every_boundary_resumes("frames", &file, &plain);
    }

    #[test]
    fn window_descriptor_covers_the_window() {
        assert_eq!(window_descriptor(2 << 20), 11 << 3);
        assert_eq!(window_descriptor(1 << 10), 0);
        let wd = window_descriptor(3_000_000);
        let (e, m) = ((wd >> 3) as u64, (wd & 7) as u64);
        let base = 1u64 << (10 + e);
        assert!(base + base / 8 * m >= 3_000_000);
    }
}
