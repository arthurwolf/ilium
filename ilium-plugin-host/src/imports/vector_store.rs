//! Where the `ilium:host/vector` indexes of one plugin persist between instances.
//!
//! Each index is one file `<index id>.vec` in the plugin's own host-owned folder
//! (`<data dir>/plugins/vector/<plugin>/`, see `wiring::wave_two::Backends::vector_root`). The
//! plugin's `fs` scopes never reach that folder, so a plugin cannot corrupt the file the host
//! reads back. A write goes to a temporary file first and is renamed into place, so a crash
//! leaves either the old or the new index, never half of one.
//!
//! File layout, every integer little-endian:
//!
//! ```text
//! magic  b"IVX1"
//! u32    name length, then the name in UTF-8
//! u32    dimensions
//! u32    entry count
//! entry: u32 id length, the id in UTF-8, then `dimensions` f32 values
//! ```
//!
//! The format is binary because an index may hold 50,000 entries of more than a thousand
//! dimensions; JSON would be several times larger and slower to read.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 4] = b"IVX1";
const EXTENSION: &str = "vec";
/// Longest name or entry id a file may carry; longer values mean the file is damaged.
const TEXT_LIMIT: usize = 64 * 1024;

/// One persisted index.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoredIndex {
    /// The name the plugin gave `create-index`.
    pub name: String,
    /// The length of every vector in the index.
    pub dimensions: usize,
    /// Vectors by entry id.
    pub entries: BTreeMap<String, Vec<f32>>,
}

/// Encodes an index into the file layout above.
pub(crate) fn encode(index: &StoredIndex) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(
        16 + index.name.len()
            + index
                .entries
                .iter()
                .map(|(id, vector)| 4 + id.len() + vector.len() * 4)
                .sum::<usize>(),
    );
    bytes.extend_from_slice(MAGIC);
    push_text(&mut bytes, &index.name);
    push_count(&mut bytes, index.dimensions);
    push_count(&mut bytes, index.entries.len());
    for (id, vector) in &index.entries {
        push_text(&mut bytes, id);
        for value in vector {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

/// Decodes a file; `None` when the bytes are not a whole index in the layout above.
pub(crate) fn decode(bytes: &[u8]) -> Option<StoredIndex> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(4)? != MAGIC {
        return None;
    }
    let name = reader.text()?;
    let dimensions = reader.count()?;
    let count = reader.count()?;
    if dimensions == 0 {
        return None;
    }
    let mut entries = BTreeMap::new();
    for _ in 0..count {
        let id = reader.text()?;
        let mut vector = Vec::with_capacity(dimensions);
        for _ in 0..dimensions {
            let raw: [u8; 4] = reader.take(4)?.try_into().ok()?;
            vector.push(f32::from_le_bytes(raw));
        }
        entries.insert(id, vector);
    }
    if reader.at != bytes.len() {
        return None;
    }
    Some(StoredIndex {
        name,
        dimensions,
        entries,
    })
}

fn push_count(bytes: &mut Vec<u8>, count: usize) {
    // Every count is bounded far below u32::MAX by the vector limits (256 texts, 50,000
    // entries, u16 dimensions, 64 KiB text), so the conversion cannot saturate in practice.
    let count = u32::try_from(count).unwrap_or(u32::MAX);
    bytes.extend_from_slice(&count.to_le_bytes());
}

fn push_text(bytes: &mut Vec<u8>, text: &str) {
    push_count(bytes, text.len());
    bytes.extend_from_slice(text.as_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(length)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn count(&mut self) -> Option<usize> {
        let raw: [u8; 4] = self.take(4)?.try_into().ok()?;
        usize::try_from(u32::from_le_bytes(raw)).ok()
    }

    fn text(&mut self) -> Option<String> {
        let length = self.count()?;
        if length > TEXT_LIMIT {
            return None;
        }
        String::from_utf8(self.take(length)?.to_vec()).ok()
    }
}

/// The index folder of one plugin.
#[derive(Debug, Clone)]
pub(crate) struct VectorStore {
    folder: PathBuf,
}

impl VectorStore {
    /// The store in `folder`; the folder is created on the first write.
    pub(crate) fn new(folder: impl Into<PathBuf>) -> Self {
        Self {
            folder: folder.into(),
        }
    }

    fn path_of(&self, id: u32) -> PathBuf {
        self.folder.join(format!("{id}.{EXTENSION}"))
    }

    /// Every readable index of the folder by id. A missing folder holds none; a damaged or
    /// unreadable file is skipped, so one bad index never hides the others.
    pub(crate) fn load(&self) -> BTreeMap<u32, StoredIndex> {
        let Ok(listing) = std::fs::read_dir(&self.folder) else {
            return BTreeMap::new();
        };
        listing
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let id = index_id_of(&path)?;
                let bytes = std::fs::read(&path).ok()?;
                Some((id, decode(&bytes)?))
            })
            .collect()
    }

    /// Writes one index, replacing its previous file in one rename.
    pub(crate) fn save(&self, id: u32, index: &StoredIndex) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.folder)?;
        let target = self.path_of(id);
        let staging = self.folder.join(format!("{id}.{EXTENSION}.tmp"));
        {
            let mut file = std::fs::File::create(&staging)?;
            file.write_all(&encode(index))?;
            file.sync_all()?;
        }
        std::fs::rename(&staging, &target)
    }

    /// Removes the file of one index; a missing file is not an error.
    pub(crate) fn remove(&self, id: u32) -> std::io::Result<()> {
        match std::fs::remove_file(self.path_of(id)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}

/// The id of an index file name such as `3.vec`.
fn index_id_of(path: &Path) -> Option<u32> {
    if path.extension()? != EXTENSION {
        return None;
    }
    path.file_stem()?.to_str()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StoredIndex {
        StoredIndex {
            name: "notes".to_owned(),
            dimensions: 2,
            entries: BTreeMap::from([
                ("a".to_owned(), vec![1.0, 0.0]),
                ("b".to_owned(), vec![-0.5, 0.25]),
            ]),
        }
    }

    #[test]
    fn an_index_round_trips_through_its_bytes() {
        assert_eq!(decode(&encode(&sample())), Some(sample()));
    }

    #[test]
    fn truncated_or_foreign_bytes_are_not_an_index() {
        let bytes = encode(&sample());
        assert_eq!(decode(&bytes[..bytes.len() - 1]), None);
        assert_eq!(decode(b"JUNK"), None);
        let mut trailing = bytes;
        trailing.push(0);
        assert_eq!(decode(&trailing), None);
    }

    #[test]
    fn the_store_saves_loads_and_removes_indexes() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let store = VectorStore::new(scratch.path().join("vector").join("example.notes"));
        assert!(store.load().is_empty(), "a missing folder holds no index");
        store.save(3, &sample()).expect("save");
        std::fs::write(
            scratch.path().join("vector/example.notes/9.vec"),
            b"damaged",
        )
        .expect("damaged file");
        let loaded = store.load();
        assert_eq!(loaded.len(), 1, "the damaged file is skipped");
        assert_eq!(loaded.get(&3), Some(&sample()));
        store.remove(3).expect("remove");
        store.remove(3).expect("removing twice is fine");
        assert!(store.load().is_empty());
    }
}
