use serde::{Deserialize, Serialize};

pub const IDX_MAGIC: &[u8; 8] = b"SLBIDX01";
pub const IDX_VERSION: u32 = 1;

/// An independent zstd frame in the `.blocks` file.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BlockInfo {
    /// Offset of the frame within the .blocks.
    pub offset: u64,
    pub compressed_len: u32,
    pub uncompressed_len: u32,
}

/// Location + search metadata of a book.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookRecord {
    pub sim_id: u64,
    pub block: u32,
    /// Offset of the JSON line within the decompressed block.
    pub offset_in_block: u32,
    pub len: u32,
    /// The book's payoutMultiplier (already ×100 in the math-sdk files).
    pub payout_x100: u64,
    /// Index into `criteria_names` (u16::MAX = absent).
    pub criteria: u16,
    /// Bitset of event types present (bit i = event_type_names[i]).
    pub event_bits: u64,
    /// Number of `freeSpinTrigger`/`freespintrigger` events in the book.
    pub fs_triggers: u16,
}

/// Sidecar `books_<MODE>.idx`: everything needed to read and search
/// without ever re-scanning the original .jsonl.zst.
#[derive(Debug, Serialize, Deserialize)]
pub struct ModeBookIndex {
    pub version: u32,
    /// Fingerprint of the source books_<MODE>.jsonl.zst (invalidation).
    pub source_fingerprint: u64,
    pub blocks: Vec<BlockInfo>,
    /// Sorted by sim_id (math-sdk write order).
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

    pub fn write_to(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let mut bytes = Vec::with_capacity(64 + self.books.len() * 48);
        bytes.extend_from_slice(IDX_MAGIC);
        bincode::serialize_into(&mut bytes, self)?;
        let tmp = path.with_extension("idx.tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn read_from(path: &std::path::Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)?;
        anyhow::ensure!(
            bytes.len() > 8 && &bytes[..8] == IDX_MAGIC,
            "invalid .idx magic"
        );
        let idx: Self = bincode::deserialize(&bytes[8..])?;
        anyhow::ensure!(idx.version == IDX_VERSION, "unknown .idx version");
        Ok(idx)
    }
}
