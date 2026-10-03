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
pub const IDX_VERSION: u32 = 6;

pub const MECH_MAGIC: &[u8; 8] = b"SLBMECH1";

/// Sidecar `books_<MODE>.mech` : les occurrences des mécaniques de chaque book
/// (`slotlab_core::mechanics`), dans l'ordre de `ModeBookIndex::books`. Séparé
/// de l'index : seul l'onglet Mechanics le lit, jamais le RGS.
#[derive(Debug, Serialize, Deserialize)]
pub struct MechFile {
    pub version: u32,
    pub source_fingerprint: u64,
    pub mech: slotlab_core::mechanics::MechIndex,
}

impl MechFile {
    pub fn write_to(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let mut bytes = Vec::with_capacity(64 + self.mech.entries.len() * 8);
        bytes.extend_from_slice(MECH_MAGIC);
        bincode::serialize_into(&mut bytes, self)?;
        let tmp = path.with_extension("mech.tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn read_from(path: &std::path::Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)?;
        anyhow::ensure!(
            bytes.len() > 12 && &bytes[..8] == MECH_MAGIC,
            "invalid .mech file"
        );
        let version = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        anyhow::ensure!(
            version == IDX_VERSION,
            ".mech version {version}, expected {IDX_VERSION} — books must be re-imported"
        );
        Ok(bincode::deserialize(&bytes[8..])?)
    }
}

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

    /// `(version, source_fingerprint)` from the first 20 bytes only — enough
    /// to know whether a cache is up to date without deserializing the whole
    /// index (tens of MB for a million books). Relies on the field order that
    /// `version_sits_right_after_the_magic` pins down.
    pub fn read_header(path: &std::path::Path) -> anyhow::Result<(u32, u64)> {
        use std::io::Read;
        let mut head = [0u8; 20];
        std::fs::File::open(path)?.read_exact(&mut head)?;
        anyhow::ensure!(&head[..8] == IDX_MAGIC, "invalid .idx magic");
        let version = u32::from_le_bytes(head[8..12].try_into().unwrap());
        let fingerprint = u64::from_le_bytes(head[12..20].try_into().unwrap());
        Ok((version, fingerprint))
    }

    pub fn read_from(path: &std::path::Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)?;
        anyhow::ensure!(
            bytes.len() > 8 && &bytes[..8] == IDX_MAGIC,
            "invalid .idx magic"
        );
        // La version se lit AVANT le reste : `version` est le premier champ et
        // bincode l'écrit en u32 petit-boutiste. Désérialiser un ancien format
        // avec la structure actuelle mélangerait les champs sur tout le
        // fichier avant d'échouer (ou pire, de réussir avec du garbage).
        let version = bytes
            .get(8..12)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .unwrap_or(0);
        anyhow::ensure!(
            version == IDX_VERSION,
            ".idx version {version}, expected {IDX_VERSION} — books must be re-imported"
        );
        let idx: Self = bincode::deserialize(&bytes[8..])?;
        anyhow::ensure!(idx.version == IDX_VERSION, "unknown .idx version");
        Ok(idx)
    }
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

    #[test]
    fn version_sits_right_after_the_magic() {
        // `read_from` lit la version dans les octets 8..12 avant tout le reste :
        // ça ne tient que si bincode écrit `version` en premier, en u32
        // petit-boutiste (encodage fixe). Vérifié ici plutôt que supposé.
        let dir = scratch("idx-version");
        let path = dir.join("books_BASE.idx");
        let idx = ModeBookIndex {
            version: IDX_VERSION,
            source_fingerprint: 7,
            blocks: vec![],
            books: vec![],
            criteria_names: vec!["basegame".into()],
            event_type_names: vec![],
        };
        idx.write_to(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[8..12], &IDX_VERSION.to_le_bytes());
        assert_eq!(ModeBookIndex::read_from(&path).unwrap().source_fingerprint, 7);
        assert_eq!(ModeBookIndex::read_header(&path).unwrap(), (IDX_VERSION, 7));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_index_of_another_version_is_refused() {
        // Un `.idx` d'une version antérieure (v1 sans `fs_retriggers`, v2 sans
        // `fs_tier`) : refusé sur son en-tête, sans que le reste — ici du
        // garbage — soit désérialisé. C'est ce refus qui le fait passer pour
        // « non importé » et déclenche le ré-import.
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
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
