use serde::{Deserialize, Serialize};

pub const IDX_MAGIC: &[u8; 8] = b"SLBIDX01";
/// À incrémenter à chaque changement de `BookRecord`/`ModeBookIndex` ou de ce
/// que l'import y calcule : un `.idx` d'une autre version est illisible, donc
/// « non importé », et l'import automatique du démarrage le reconstruit.
///   1 → 2 : `fs_retriggers` (règle générique de `slotlab_core::freespins`).
///   2 → 3 : `fs_tier` (positions du premier `freeSpinTrigger`).
///   3 → 4 : `fs_spins_initial`/`fs_spins_final` (spins du premier bonus) et
///           `fg_win_x100` (`freeGameWins` du book : le gain du bonus seul).
///   4 → 5 : relevé des mécaniques `books_<MODE>.mech` (`MechFile`), écrit avec
///           l'index — le passage à 5 force le ré-import qui le produit.
///   5 → 6 : mécaniques croisées par sorte (`champ@sorte`, cf. `mechanics`).
///   6 → 7 : symboles low / high révélés (`sym_*`, `slotlab_core::symbols`) et
///           format « actions » (UNDEAD KING : paliers lus sur `BonusGames`).
///   7 → 8 : `antic`, l'anticipation du spin payé (premier `reveal`).
///   8 → 9 : `fs_tier` pris sur le champ `tier` du premier `freeSpinTrigger` quand
///           le jeu le donne (SAUCER PANIC : 3 = LAB, 4 = FULL INVASION, tous deux
///           déclenchés par 3 symboles) ; sinon toujours ses `positions`.
///   9 → 10 : books lus EN PLACE dans le `.jsonl.zst` du jeu grâce à des points de
///           reprise zstd (`books_<MODE>.ckpt`, cf. `zran`) — plus de copie
///           `.blocks` ; `offset` absolu dans le flux ; index compressé ; relevé
///           des mécaniques calculé à la première demande, plus à l'import.
pub const IDX_VERSION: u32 = 10;
/// Bytes read by `read_header`: magic, version, fingerprint, source size.
const HEADER_LEN: usize = 28;

pub const MECH_MAGIC: &[u8; 8] = b"SLBMECH1";

/// Sidecar `books_<MODE>.mech` : les occurrences des mécaniques de chaque book
/// (`slotlab_core::mechanics`), dans l'ordre de `ModeBookIndex::books`. Séparé
/// de l'index : seul l'onglet Mechanics le lit, jamais le RGS — il est donc
/// calculé la première fois qu'on l'ouvre (`BooksManager::mechanics`).
#[derive(Debug, Serialize, Deserialize)]
pub struct MechFile {
    pub version: u32,
    pub source_fingerprint: u64,
    pub mech: slotlab_core::mechanics::MechIndex,
}

impl MechFile {
    pub fn write_to(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let body = zstd::bulk::compress(&bincode::serialize(self)?, 3)?;
        let mut bytes = Vec::with_capacity(20 + body.len());
        bytes.extend_from_slice(MECH_MAGIC);
        bytes.extend_from_slice(&self.version.to_le_bytes());
        bytes.extend_from_slice(&self.source_fingerprint.to_le_bytes());
        bytes.extend_from_slice(&body);
        write_atomic(path, &bytes)
    }

    /// `(version, source_fingerprint)` without reading the rest.
    pub fn read_header(path: &std::path::Path) -> anyhow::Result<(u32, u64)> {
        let head = read_head::<20>(path)?;
        anyhow::ensure!(&head[..8] == MECH_MAGIC, "invalid .mech file");
        Ok((
            u32::from_le_bytes(head[8..12].try_into().unwrap()),
            u64::from_le_bytes(head[12..20].try_into().unwrap()),
        ))
    }

    pub fn read_from(path: &std::path::Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)?;
        anyhow::ensure!(
            bytes.len() > 20 && &bytes[..8] == MECH_MAGIC,
            "invalid .mech file"
        );
        let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        anyhow::ensure!(
            version == IDX_VERSION,
            ".mech version {version}, expected {IDX_VERSION} — recomputed on next use"
        );
        Ok(bincode::deserialize(&zstd::decode_all(&bytes[20..])?)?)
    }
}

/// Where a mode's decompressed stream is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Storage {
    /// The game's own `books_<MODE>.jsonl.zst`, entered through resume points.
    InPlace,
    /// `books_<MODE>.zcopy`, the same stream re-cut into independent zstd
    /// frames. Only when the source cannot be resumed cheaply (a window over
    /// `zran::MAX_RESUME_WINDOW`) or a resume point failed its check.
    Copy,
}

/// A place where decoding can start.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Offset in the books file (source or copy).
    pub comp: u64,
    /// Offset in the decompressed stream.
    pub dec: u64,
    /// `None`: a frame starts at `comp`, a fresh decoder reads from there.
    pub resume: Option<ResumeInfo>,
}

/// A resume point in the middle of a frame (`zran::Snapshot` on disk).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ResumeInfo {
    pub window_size: u64,
    pub checksum: bool,
    /// One zstd frame of `books_<MODE>.ckpt`: the decoder state, then the window.
    pub blob_offset: u64,
    pub blob_len: u32,
    pub window_len: u32,
}

/// Location + search metadata of a book.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookRecord {
    pub sim_id: u64,
    /// Offset of the book's line in the decompressed stream.
    pub offset: u64,
    /// Length of the line, `\n` excluded.
    pub len: u32,
    /// The book's payoutMultiplier (already ×100 in the math-sdk files).
    pub payout_x100: u64,
    /// Index into `criteria_names` (u16::MAX = absent).
    pub criteria: u16,
    /// Bitset of event types present (bit i = event_type_names[i]).
    pub event_bits: u64,
    /// Number of `freeSpinTrigger`/`freespintrigger` events in the book.
    pub fs_triggers: u16,
    /// Retriggers à l'intérieur du ou des bonus : annonces d'un total de free
    /// spins qui dépasse tous les précédents (`slotlab_core::freespins`). Un
    /// book de BLOOD WATER à 8 repas qui montent le total en compte 8, là où
    /// `fs_triggers` dit 1.
    pub fs_retriggers: u16,
    /// Palier du bonus : nombre de `positions` du PREMIER `freeSpinTrigger`
    /// (les scatters qui l'ont déclenché). 0 = pas de bonus, ou un
    /// déclenchement qui ne dit pas ses positions.
    pub fs_tier: u8,
    /// Free spins attribués à l'entrée du premier bonus (0 : inconnu).
    pub fs_spins_initial: u16,
    /// Free spins du premier bonus retriggers compris (0 : inconnu).
    pub fs_spins_final: u16,
    /// `freeGameWins` du book ×100 — ce que le bonus a payé à lui seul, hors
    /// spin de base qui l'a ouvert. `None` : le book ne le dit pas (champ
    /// propre aux books du math-sdk).
    pub fg_win_x100: Option<u64>,
    /// Symboles payants low / high des plateaux révélés, hors bonus puis en
    /// free spins (`slotlab_core::symbols`) ; 0 : noms non classés ou books
    /// sans plateau.
    pub sym_low: u16,
    pub sym_high: u16,
    pub sym_low_fs: u16,
    pub sym_high_fs: u16,
    /// Le spin payé annonce le scatter manquant : `anticipation` non nul sur
    /// le premier `reveal` (`slotlab_core::freespins`).
    pub antic: bool,
}

/// Sidecar `books_<MODE>.idx`: everything needed to read and search
/// without ever re-scanning the original .jsonl.zst.
#[derive(Debug, Serialize, Deserialize)]
pub struct ModeBookIndex {
    pub version: u32,
    /// Fingerprint of the source books_<MODE>.jsonl.zst (invalidation).
    pub source_fingerprint: u64,
    /// Size of that file. What a deployed copy is checked against: the hub's
    /// copy of the `.zst` does not keep the mtime the fingerprint hashes.
    pub source_len: u64,
    pub storage: Storage,
    /// In stream order; the first one is always at offset 0.
    #[serde(deserialize_with = "exact_vec")]
    pub checkpoints: Vec<Checkpoint>,
    /// Decompressed size of the whole stream.
    pub stream_len: u64,
    /// Sorted by sim_id (math-sdk write order).
    #[serde(deserialize_with = "exact_vec")]
    pub books: Vec<BookRecord>,
    pub criteria_names: Vec<String>,
    /// Event types, index = bit position in `event_bits` (≤ 64).
    pub event_type_names: Vec<String>,
}

impl ModeBookIndex {
    pub fn find(&self, sim_id: u64) -> Option<&BookRecord> {
        match self.books.binary_search_by_key(&sim_id, |b| b.sim_id) {
            Ok(i) => Some(&self.books[i]),
            // Books not guaranteed sorted if the math-sdk changes: linear fallback.
            Err(_) => self.books.iter().find(|b| b.sim_id == sim_id),
        }
    }

    /// Raw header (what `read_header` reads), then the bincode body
    /// compressed: an index is a third of its size that way.
    pub fn write_to(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let body = zstd::bulk::compress(&bincode::serialize(self)?, 3)?;
        let mut bytes = Vec::with_capacity(HEADER_LEN + body.len());
        bytes.extend_from_slice(IDX_MAGIC);
        bytes.extend_from_slice(&self.version.to_le_bytes());
        bytes.extend_from_slice(&self.source_fingerprint.to_le_bytes());
        bytes.extend_from_slice(&self.source_len.to_le_bytes());
        bytes.extend_from_slice(&body);
        write_atomic(path, &bytes)
    }

    /// `(version, source_fingerprint, source_len)` from the first bytes only —
    /// enough to know whether a cache is up to date without decompressing the
    /// whole index (tens of MB for a million books).
    pub fn read_header(path: &std::path::Path) -> anyhow::Result<(u32, u64, u64)> {
        let head = read_head::<HEADER_LEN>(path)?;
        anyhow::ensure!(&head[..8] == IDX_MAGIC, "invalid .idx magic");
        Ok((
            u32::from_le_bytes(head[8..12].try_into().unwrap()),
            u64::from_le_bytes(head[12..20].try_into().unwrap()),
            u64::from_le_bytes(head[20..28].try_into().unwrap()),
        ))
    }

    /// Read straight from the file through the decompressor: no copy of the
    /// file nor of its decompressed bytes is ever held, and the book list is
    /// allocated once at its exact size (`exact_vec`). The hub runs in 1 GB:
    /// loading an index used to take three times its final size.
    pub fn read_from(path: &std::path::Path) -> anyhow::Result<Self> {
        use std::io::Read;
        let mut file = std::fs::File::open(path)?;
        let mut head = [0u8; 12];
        file.read_exact(&mut head)
            .map_err(|_| anyhow::anyhow!("invalid .idx magic"))?;
        anyhow::ensure!(&head[..8] == IDX_MAGIC, "invalid .idx magic");
        // La version se lit AVANT le reste : un ancien format désérialisé avec
        // la structure actuelle mélangerait les champs avant d'échouer (ou
        // pire, de réussir avec du garbage). Avant la 10, `version` était le
        // premier champ du bincode, aux mêmes octets 8..12.
        let version = u32::from_le_bytes(head[8..12].try_into().unwrap());
        anyhow::ensure!(
            version == IDX_VERSION,
            ".idx version {version}, expected {IDX_VERSION} — books must be re-imported"
        );
        let mut rest = [0u8; HEADER_LEN - 12];
        file.read_exact(&mut rest)
            .map_err(|_| anyhow::anyhow!("truncated .idx"))?;
        let idx: Self = bincode::deserialize_from(zstd::stream::read::Decoder::new(file)?)?;
        anyhow::ensure!(idx.version == IDX_VERSION, "unknown .idx version");
        Ok(idx)
    }
}

/// A `Vec` allocated once at the length the file announces (bincode writes
/// it first), instead of serde's capped pre-allocation that then doubles:
/// a million books would otherwise reserve up to twice their size, and hold
/// the slack for as long as the index stays open. Capped all the same, in
/// case a damaged file announces an absurd length.
fn exact_vec<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Exact<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Exact<T> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a sequence")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
            let mut v = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1 << 25));
            while let Some(x) = seq.next_element()? {
                v.push(x);
            }
            Ok(v)
        }
    }
    d.deserialize_seq(Exact(std::marker::PhantomData))
}

fn read_head<const N: usize>(path: &std::path::Path) -> anyhow::Result<[u8; N]> {
    use std::io::Read;
    let mut head = [0u8; N];
    std::fs::File::open(path)?.read_exact(&mut head)?;
    Ok(head)
}

/// tmp + rename: a reader never sees half a file.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("slotlab-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn empty_index(fingerprint: u64) -> ModeBookIndex {
        ModeBookIndex {
            version: IDX_VERSION,
            source_fingerprint: fingerprint,
            source_len: 1234,
            storage: Storage::InPlace,
            checkpoints: vec![Checkpoint { comp: 0, dec: 0, resume: None }],
            stream_len: 0,
            books: vec![],
            criteria_names: vec!["basegame".into()],
            event_type_names: vec![],
        }
    }

    #[test]
    fn header_round_trip() {
        let dir = scratch("idx-header");
        let path = dir.join("books_BASE.idx");
        empty_index(7).write_to(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[8..12], &IDX_VERSION.to_le_bytes());
        let back = ModeBookIndex::read_from(&path).unwrap();
        assert_eq!((back.source_fingerprint, back.source_len), (7, 1234));
        assert_eq!(ModeBookIndex::read_header(&path).unwrap(), (IDX_VERSION, 7, 1234));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_index_of_another_version_is_refused() {
        // Un `.idx` d'une version antérieure : refusé sur son en-tête, sans
        // que le reste — ici du garbage — soit lu. C'est ce refus qui le fait
        // passer pour « non importé » et déclenche le ré-import.
        let dir = scratch("idx-old");
        let path = dir.join("books_BASE.idx");
        for old in 1..IDX_VERSION {
            let mut bytes = IDX_MAGIC.to_vec();
            bytes.extend_from_slice(&old.to_le_bytes());
            bytes.extend_from_slice(&[0xAB; 64]);
            std::fs::write(&path, &bytes).unwrap();
            let err = ModeBookIndex::read_from(&path).unwrap_err().to_string();
            assert!(
                err.contains(&format!("version {old}")) && err.contains("re-imported"),
                "{err}"
            );
            assert_eq!(ModeBookIndex::read_header(&path).unwrap().0, old);
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
