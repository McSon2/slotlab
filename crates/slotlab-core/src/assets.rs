//! Asset budget of a game's frontend build: what the player downloads to start
//! playing, and how much video memory the textures occupy once uploaded.
//!
//! ── Why this is not a `du -sh` ──────────────────────────────────────────────
//! Two things make the folder size a poor answer, and both are systematic:
//!
//!   · **mutually exclusive variants.** A Spine skeleton is shipped at several
//!     atlas scales (`atlas/x02/`, `atlas/x04/`, `atlas/x06/`…) and the game
//!     loads exactly ONE of them, picked from the device. Audio is shipped as
//!     `.ogg` + `.m4a` + `.mp3` and the browser plays exactly one. Summing the
//!     folder counts every alternative, which no player ever downloads;
//!   · **video memory has nothing to do with file size.** A texture is
//!     decompressed to RGBA8 on the GPU: `width × height × 4`, whatever the
//!     encoder achieved. A 20 KB page can cost 10 MB of VRAM — and that is not a
//!     hypothetical, it is what a sparsely packed atlas page actually does.
//!
//! So the report gives a RANGE — the lightest and the heaviest configuration a
//! device can end up with — rather than one number that is true for nobody.
//!
//! ── What a "variant group" is ───────────────────────────────────────────────
//! The unit is the SKELETON, not the file. A `.atlas` names the pages it needs
//! (and their size, which is why nothing has to be decoded here); two `.atlas`
//! files sharing a base name are two variants of the same skeleton. This is what
//! makes the count exact when a scale needs MORE pages than another — a page
//! that only exists under `x2/` is charged to the `x2` variant, never to the
//! baseline.
//!
//! ⚠️ Conventions this relies on, all of them observed in the shipped builds:
//! Spine pages are declared by a sibling `.atlas`, alternative scales live in
//! sibling directories under a common parent, and interchangeable audio shares a
//! stem. A build that breaks them is not mis-reported — its files simply fall
//! back to being counted once each, which is the honest reading of "we cannot
//! tell these apart".
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Bytes per pixel once the GPU has decompressed the texture (RGBA8).
const BYTES_PER_PIXEL: u64 = 4;

/// Mipmaps add a third of the base level (1 + 1/4 + 1/16 + …).
const MIPMAP_FACTOR: f64 = 4.0 / 3.0;

/// Below this, a page carries so little ink for its surface that the packing is
/// worth a look: it costs full VRAM for almost no image.
///
/// ⚠️ A proxy, and deliberately a coarse one — measuring real coverage would mean
/// decoding every page. Encoded weight per megapixel separates a packed sheet
/// (~45–170 KB/Mpx in the shipped builds) from a nearly empty one (~8) by more
/// than an order of magnitude, so the threshold does not need to be precise.
const SPARSE_PAGE_KB_PER_MPX: f64 = 30.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub path: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub vram: u64,
}

/// One option inside a group of mutually exclusive variants.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Variant {
    /// What distinguishes it from its siblings: the scale directory (`x04`), or
    /// the container extension (`ogg`).
    pub label: String,
    pub bytes: u64,
    pub vram: u64,
    pub pages: Vec<Page>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupKind {
    /// Spine skeleton shipped at several atlas scales — one is loaded.
    AtlasScale,
    /// Same sound in several containers — the browser plays one.
    AudioFormat,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantGroup {
    pub kind: GroupKind,
    pub name: String,
    pub variants: Vec<Variant>,
}

impl VariantGroup {
    fn lightest(&self) -> &Variant {
        self.variants
            .iter()
            .min_by_key(|v| (v.bytes, v.vram))
            .expect("a group always holds at least one variant")
    }

    fn heaviest(&self) -> &Variant {
        self.variants
            .iter()
            .max_by_key(|v| (v.bytes, v.vram))
            .expect("a group always holds at least one variant")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Code,
    Texture,
    Audio,
    Font,
    Other,
}

impl Category {
    fn label(self) -> &'static str {
        match self {
            Category::Code => "code",
            Category::Texture => "texture",
            Category::Audio => "audio",
            Category::Font => "font",
            Category::Other => "other",
        }
    }
}

/// A file that stands alone — no alternative, always downloaded.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub path: String,
    pub category: Category,
    pub bytes: u64,
    pub vram: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryTotal {
    pub category: Category,
    pub label: String,
    pub bytes: u64,
    pub vram: u64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    pub download: u64,
    pub vram: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Warning {
    pub path: String,
    pub message: String,
    /// VRAM that the finding is about — what a fix would be worth.
    pub vram: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetReport {
    pub build_dir: String,
    pub file_count: usize,
    /// Everything on disk, every variant included — the `du -sh` number, given
    /// only so the range below can be read against it.
    pub disk_bytes: u64,
    /// Lightest configuration a device can land on (smallest variant of each group).
    pub min: Budget,
    /// Heaviest configuration (largest variant of each group).
    pub max: Budget,
    pub groups: Vec<VariantGroup>,
    /// Standalone files, heaviest first.
    pub items: Vec<Item>,
    /// Totals by category, at the HEAVIEST configuration.
    pub categories: Vec<CategoryTotal>,
    pub warnings: Vec<Warning>,
}

// ── image dimensions, read from the header ──────────────────────────────────
//
// ⚠️ Only what is needed to know the VRAM, and no image crate for it: a decoder
// would pull a large dependency tree to read two integers that sit in the first
// thirty bytes of the file.

fn be16(b: &[u8], at: usize) -> u32 {
    ((b[at] as u32) << 8) | b[at + 1] as u32
}

fn be32(b: &[u8], at: usize) -> u32 {
    ((b[at] as u32) << 24) | ((b[at + 1] as u32) << 16) | ((b[at + 2] as u32) << 8) | b[at + 3] as u32
}

fn le16(b: &[u8], at: usize) -> u32 {
    (b[at] as u32) | ((b[at + 1] as u32) << 8)
}

fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    // 8-byte signature, then the IHDR chunk: length, type, width, height.
    (b.len() >= 24 && &b[..8] == b"\x89PNG\r\n\x1a\n" && &b[12..16] == b"IHDR")
        .then(|| (be32(b, 16), be32(b, 20)))
}

fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = b[i + 1];
        // SOF0..SOF15, minus the markers that are not frame headers.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            return Some((be16(b, i + 7), be16(b, i + 5)));
        }
        if matches!(marker, 0xD8 | 0xD9) || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        i += 2 + be16(b, i + 2) as usize;
    }
    None
}

fn webp_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 30 || &b[..4] != b"RIFF" || &b[8..12] != b"WEBP" {
        return None;
    }
    match &b[12..16] {
        // Extended format: the canvas size, minus one, on three little-endian bytes.
        b"VP8X" => {
            let w = (b[24] as u32) | ((b[25] as u32) << 8) | ((b[26] as u32) << 16);
            let h = (b[27] as u32) | ((b[28] as u32) << 8) | ((b[29] as u32) << 16);
            Some((w + 1, h + 1))
        }
        // Lossless: 14 bits of width-1 then 14 bits of height-1, after the 0x2F byte.
        b"VP8L" => {
            let bits = (b[21] as u32) | ((b[22] as u32) << 8) | ((b[23] as u32) << 16)
                | ((b[24] as u32) << 24);
            Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        // Lossy: after the 3-byte start code, 14 bits of width then of height.
        b"VP8 " => Some((le16(b, 26) & 0x3FFF, le16(b, 28) & 0x3FFF)),
        _ => None,
    }
}

/// Pixel dimensions of an image file, or `None` when the format is not one we
/// read. A `None` costs an under-estimated VRAM, never a wrong one: the caller
/// reports zero rather than a guess.
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    png_size(bytes)
        .or_else(|| webp_size(bytes))
        .or_else(|| jpeg_size(bytes))
}

fn vram_of(width: u32, height: u32, mipmapped: bool) -> u64 {
    let base = width as u64 * height as u64 * BYTES_PER_PIXEL;
    if mipmapped {
        (base as f64 * MIPMAP_FACTOR) as u64
    } else {
        base
    }
}

// ── scanning ────────────────────────────────────────────────────────────────

fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_image(ext: &str) -> bool {
    matches!(ext, "webp" | "png" | "jpg" | "jpeg" | "avif" | "gif")
}

fn is_audio(ext: &str) -> bool {
    matches!(ext, "ogg" | "m4a" | "mp3" | "wav" | "aac" | "opus" | "webm")
}

fn categorise(rel: &str, ext: &str) -> Category {
    if is_audio(ext) {
        return Category::Audio;
    }
    if matches!(ext, "woff2" | "woff" | "ttf" | "otf" | "fnt") || rel.contains("/fonts/") {
        return Category::Font;
    }
    if is_image(ext) {
        return Category::Texture;
    }
    if matches!(ext, "js" | "mjs" | "css" | "html" | "map") {
        return Category::Code;
    }
    Category::Other
}

fn walk(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(dir) = std::fs::read_dir(root) else {
        return;
    };
    for entry in dir.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => walk(&path, out),
            Ok(t) if t.is_file() => out.push(path),
            _ => {}
        }
    }
}

/// Pages declared by a `.atlas`, with the size the file itself states.
///
/// The libGDX format alternates a page name and its properties; `size:W,H` sits
/// right under the name. Reading it is what spares us decoding every page — and
/// it is also the only source that says which pages a given scale actually needs.
fn atlas_pages(text: &str) -> Vec<(String, u32, u32)> {
    let mut pages = Vec::new();
    let mut pending: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            pending = None;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("size:") {
            if let Some(name) = pending.take() {
                let mut it = rest.split(',');
                let w = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                let h = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                pages.push((name, w, h));
            }
            continue;
        }
        // A page name is an unindented line naming an image file.
        if line.starts_with(char::is_whitespace) || trimmed.contains(':') {
            continue;
        }
        if is_image(&ext_of(Path::new(trimmed))) {
            pending = Some(trimmed.to_string());
        }
    }
    pages
}

fn rel_of(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Builds the report for a frontend build directory.
///
/// `mipmapped` names the files whose textures the game generates mipmaps for
/// (they cost a third more VRAM). Paths are matched by suffix, so
/// `"typo_gains_gold.webp"` is enough.
pub fn analyse(build_dir: &Path, mipmapped: &[&str]) -> AssetReport {
    let mut files = Vec::new();
    walk(build_dir, &mut files);
    files.sort();

    let size_of = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let disk_bytes: u64 = files.iter().map(|p| size_of(p)).sum();
    let is_mipmapped =
        |rel: &str| mipmapped.iter().any(|m| rel.ends_with(m) || rel.contains(m));

    // ── 1. Spine atlases, and the pages they claim ──────────────────────────
    //
    // Grouped by base name so that `x02/symbols.atlas` and `x06/symbols.atlas`
    // become two variants of one skeleton. The variant label is the directory
    // that distinguishes them.
    let mut claimed: HashSet<PathBuf> = HashSet::new();
    let mut atlas_groups: BTreeMap<String, Vec<Variant>> = BTreeMap::new();

    for path in files.iter().filter(|p| ext_of(p) == "atlas") {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let dir = path.parent().unwrap_or(build_dir);
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let label = dir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "—".into());

        claimed.insert(path.clone());
        let mut pages = Vec::new();
        let mut bytes = size_of(path);
        let mut vram = 0;
        for (name, w, h) in atlas_pages(&text) {
            let page_path = dir.join(&name);
            let page_bytes = size_of(&page_path);
            claimed.insert(page_path.clone());
            let rel = rel_of(build_dir, &page_path);
            let page_vram = vram_of(w, h, is_mipmapped(&rel));
            bytes += page_bytes;
            vram += page_vram;
            pages.push(Page {
                path: rel,
                bytes: page_bytes,
                width: w,
                height: h,
                vram: page_vram,
            });
        }
        atlas_groups
            .entry(stem)
            .or_default()
            .push(Variant { label, bytes, vram, pages });
    }

    // ── 2. Interchangeable audio containers, grouped by stem ────────────────
    let mut audio_groups: BTreeMap<String, Vec<Variant>> = BTreeMap::new();
    for path in files.iter().filter(|p| is_audio(&ext_of(p))) {
        let stem = path
            .with_extension("")
            .to_string_lossy()
            .trim_start_matches(&*build_dir.to_string_lossy())
            .trim_start_matches(['/', '\\'])
            .replace('\\', "/");
        claimed.insert(path.clone());
        audio_groups.entry(stem).or_default().push(Variant {
            label: ext_of(path),
            bytes: size_of(path),
            vram: 0,
            pages: Vec::new(),
        });
    }

    let mut groups: Vec<VariantGroup> = Vec::new();
    for (name, mut variants) in atlas_groups {
        variants.sort_by_key(|v| v.bytes);
        groups.push(VariantGroup { kind: GroupKind::AtlasScale, name, variants });
    }
    for (name, mut variants) in audio_groups {
        variants.sort_by_key(|v| v.bytes);
        groups.push(VariantGroup { kind: GroupKind::AudioFormat, name, variants });
    }
    // A single-variant "group" is not a choice — it belongs with the plain files,
    // where it is counted once and reads as what it is.
    let (groups, singles): (Vec<_>, Vec<_>) =
        groups.into_iter().partition(|g| g.variants.len() > 1);

    // ── 3. Everything else: counted once, always downloaded ─────────────────
    let mut items: Vec<Item> = Vec::new();
    for path in files.iter().filter(|p| !claimed.contains(*p)) {
        let rel = rel_of(build_dir, path);
        let ext = ext_of(path);
        let (width, height) = if is_image(&ext) {
            std::fs::read(path)
                .ok()
                .and_then(|b| image_size(&b))
                .unwrap_or((0, 0))
        } else {
            (0, 0)
        };
        items.push(Item {
            category: categorise(&rel, &ext),
            bytes: size_of(path),
            vram: vram_of(width, height, is_mipmapped(&rel)),
            width,
            height,
            path: rel,
        });
    }
    // Single-variant atlases rejoin the plain files, pages and all.
    for group in &singles {
        for variant in &group.variants {
            for page in &variant.pages {
                items.push(Item {
                    path: page.path.clone(),
                    category: Category::Texture,
                    bytes: page.bytes,
                    vram: page.vram,
                    width: page.width,
                    height: page.height,
                });
            }
        }
    }
    items.sort_by(|a, b| b.vram.cmp(&a.vram).then(b.bytes.cmp(&a.bytes)));

    // ── 4. The range ────────────────────────────────────────────────────────
    let base_download: u64 = items.iter().map(|i| i.bytes).sum();
    let base_vram: u64 = items.iter().map(|i| i.vram).sum();
    let min = Budget {
        download: base_download + groups.iter().map(|g| g.lightest().bytes).sum::<u64>(),
        vram: base_vram + groups.iter().map(|g| g.lightest().vram).sum::<u64>(),
    };
    let max = Budget {
        download: base_download + groups.iter().map(|g| g.heaviest().bytes).sum::<u64>(),
        vram: base_vram + groups.iter().map(|g| g.heaviest().vram).sum::<u64>(),
    };

    // ── 5. Totals by category, at the heaviest configuration ────────────────
    let mut by_category: BTreeMap<Category, (u64, u64)> = BTreeMap::new();
    for item in &items {
        let e = by_category.entry(item.category).or_default();
        e.0 += item.bytes;
        e.1 += item.vram;
    }
    for group in &groups {
        let heaviest = group.heaviest();
        let category = match group.kind {
            GroupKind::AtlasScale => Category::Texture,
            GroupKind::AudioFormat => Category::Audio,
        };
        let e = by_category.entry(category).or_default();
        e.0 += heaviest.bytes;
        e.1 += heaviest.vram;
    }
    let categories = by_category
        .into_iter()
        .map(|(category, (bytes, vram))| CategoryTotal {
            category,
            label: category.label().to_string(),
            bytes,
            vram,
        })
        .collect();

    // ── 6. Sparse pages — full VRAM for almost no image ─────────────────────
    let mut warnings = Vec::new();
    let mut flag = |path: &str, bytes: u64, w: u32, h: u32, vram: u64| {
        let mpx = (w as f64 * h as f64) / 1_000_000.0;
        if mpx < 0.5 || bytes == 0 {
            return;
        }
        let kb_per_mpx = (bytes as f64 / 1024.0) / mpx;
        if kb_per_mpx < SPARSE_PAGE_KB_PER_MPX {
            warnings.push(Warning {
                path: path.to_string(),
                message: format!(
                    "{w}×{h} for only {:.0} KB ({kb_per_mpx:.0} KB/Mpx): the page is nearly \
                     empty but still costs its full VRAM. Worth a repack.",
                    bytes as f64 / 1024.0
                ),
                vram,
            });
        }
    };
    for group in &groups {
        for variant in &group.variants {
            for page in &variant.pages {
                flag(&page.path, page.bytes, page.width, page.height, page.vram);
            }
        }
    }
    for item in items.iter().filter(|i| i.category == Category::Texture) {
        flag(&item.path, item.bytes, item.width, item.height, item.vram);
    }
    warnings.sort_by(|a, b| b.vram.cmp(&a.vram));

    AssetReport {
        build_dir: build_dir.to_string_lossy().to_string(),
        file_count: files.len(),
        disk_bytes,
        min,
        max,
        groups,
        items,
        categories,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_png_dimensions() {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        b.extend_from_slice(&13u32.to_be_bytes());
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&1920u32.to_be_bytes());
        b.extend_from_slice(&1080u32.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0]);
        assert_eq!(image_size(&b), Some((1920, 1080)));
    }

    #[test]
    fn reads_webp_extended_dimensions() {
        // VP8X stores canvas size minus one, little-endian on three bytes.
        let mut b = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
        b.extend_from_slice(&[0; 8]); // chunk size + flags
        let (w, h) = (4059u32 - 1, 2166u32 - 1);
        b.extend_from_slice(&w.to_le_bytes()[..3]);
        b.extend_from_slice(&h.to_le_bytes()[..3]);
        assert_eq!(image_size(&b), Some((4059, 2166)));
    }

    #[test]
    fn reads_webp_lossless_dimensions() {
        let mut b = b"RIFF\0\0\0\0WEBPVP8L".to_vec();
        b.extend_from_slice(&[0, 0, 0, 0, 0x2F]); // chunk size + signature byte
        let bits: u32 = (512 - 1) | ((1024 - 1) << 14);
        b.extend_from_slice(&bits.to_le_bytes());
        b.extend_from_slice(&[0; 8]);
        assert_eq!(image_size(&b), Some((512, 1024)));
    }

    #[test]
    fn parses_pages_and_their_declared_size() {
        let atlas = "\
slotframeanimation.webp
size:610,938
filter:Linear,Linear
SPINFX_00000
  bounds:2,2,606,934

slotframeanimation_2.webp
size:1950,1946
filter:Linear,Linear
cadre
  bounds:2,2,1946,1942
";
        assert_eq!(
            atlas_pages(atlas),
            vec![
                ("slotframeanimation.webp".to_string(), 610, 938),
                ("slotframeanimation_2.webp".to_string(), 1950, 1946),
            ]
        );
    }

    #[test]
    fn vram_follows_pixels_not_file_size() {
        // The whole point of the report: 1620×1614 costs 10 MB whatever it weighs.
        assert_eq!(vram_of(1620, 1614, false), 1620 * 1614 * 4);
        // Mipmaps add a third.
        assert_eq!(vram_of(512, 1024, true), (512 * 1024 * 4 * 4) / 3);
    }
}

#[cfg(test)]
mod smoke {
    /// Prints the report for a real build — `cargo test -p slotlab-core smoke -- --nocapture`.
    #[test]
    #[ignore]
    fn dump_real_build() {
        let dir = std::env::var("BUILD_DIR").expect("set BUILD_DIR");
        let r = super::analyse(std::path::Path::new(&dir), &["typo_gains_gold.webp"]);
        let mb = |b: u64| b as f64 / 1_048_576.0;
        println!("files {} | disk {:.2} Mo", r.file_count, mb(r.disk_bytes));
        println!("min  dl {:.2} Mo | vram {:.1} Mo", mb(r.min.download), mb(r.min.vram));
        println!("max  dl {:.2} Mo | vram {:.1} Mo", mb(r.max.download), mb(r.max.vram));
        for g in &r.groups {
            println!("[{:?}] {}", g.kind, g.name);
            for v in &g.variants {
                println!("    {:<6} {:7.2} Mo dl  {:7.1} Mo vram  ({} pages)", v.label, mb(v.bytes), mb(v.vram), v.pages.len());
            }
        }
        for c in &r.categories { println!("cat {:<9} {:7.2} Mo dl {:7.1} Mo vram", c.label, mb(c.bytes), mb(c.vram)); }
        for i in r.items.iter().take(8) { println!("item {:<52} {:7.2} Mo dl {:6.1} Mo vram", i.path, mb(i.bytes), mb(i.vram)); }
        for w in &r.warnings { println!("WARN {} — {}", w.path, w.message); }
    }
}
