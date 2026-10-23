//! Persistent cache for expensive, deterministic preparation results.
//!
//! An entry is one file: `ILPC` magic, format version, the 32-byte key digest,
//! the uncompressed length, a SHA-256 of the compressed body, then the
//! deflate-compressed serde payload. Loading never trusts the file: any
//! mismatch (magic, version, key, length, checksum, decode) is a miss, the bad
//! entry is removed, and the caller recomputes. Call only from worker threads;
//! these functions do blocking filesystem I/O.
use flate2::{read::DeflateDecoder, write::DeflateEncoder, Compression};
use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

/// Bump when any cached payload type or the key derivation changes.
pub const FORMAT_VERSION: u32 = 1;
const MAGIC: &[u8; 4] = b"ILPC";
const HEADER_BYTES: usize = 4 + 4 + 32 + 8 + 32;
/// Upper bound on one decompressed payload; larger entries are never written or read.
pub const MAX_PAYLOAD_BYTES: u64 = 256 << 20;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("prepared cache payload exceeds {MAX_PAYLOAD_BYTES} bytes")]
    TooLarge,
    #[error("prepared cache I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("prepared cache payload encoding failed: {0}")]
    Encode(#[from] serde_json::Error),
}

/// Identity of one cache entry: everything that shapes the cached result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key([u8; 32]);

impl Key {
    pub fn hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// Builds a [`Key`] from length-prefixed parts so concatenations cannot collide.
pub struct KeyBuilder(Sha256);

impl KeyBuilder {
    pub fn new(kind: &str) -> Self {
        let mut builder = Self(Sha256::new());
        builder.part(&FORMAT_VERSION.to_le_bytes());
        builder.part(kind.as_bytes());
        builder
    }

    pub fn part(&mut self, bytes: &[u8]) -> &mut Self {
        self.0.update((bytes.len() as u64).to_le_bytes());
        self.0.update(bytes);
        self
    }

    pub fn text(&mut self, value: &str) -> &mut Self {
        self.part(value.as_bytes())
    }

    pub fn number(&mut self, value: u64) -> &mut Self {
        self.part(&value.to_le_bytes())
    }

    /// Adds size and modification time of a file, or of every file below a
    /// directory (bounded). A missing path adds a distinct marker, so creating
    /// it later changes the key. Metadata, not content, so a same-size edit that
    /// preserves the modification time is not noticed.
    pub fn path_fingerprint(&mut self, path: &Path) -> &mut Self {
        let mut pending = vec![path.to_owned()];
        let mut visited = 0usize;
        while let Some(current) = pending.pop() {
            visited += 1;
            if visited > 100_000 {
                self.text("fingerprint-truncated");
                break;
            }
            match fs::metadata(&current) {
                Ok(meta) if meta.is_dir() => {
                    let mut names: Vec<PathBuf> = fs::read_dir(&current)
                        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
                        .unwrap_or_default();
                    names.sort();
                    self.text("dir").text(&current.to_string_lossy());
                    pending.extend(names.into_iter().rev());
                }
                Ok(meta) => {
                    let modified = meta
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                        .map_or(0, |elapsed| elapsed.as_nanos() as u64);
                    self.text("file")
                        .text(&current.to_string_lossy())
                        .number(meta.len())
                        .number(modified);
                }
                Err(_) => {
                    self.text("missing").text(&current.to_string_lossy());
                }
            }
        }
        self
    }

    pub fn finish(&self) -> Key {
        Key(self.0.clone().finalize().into())
    }
}

#[derive(Clone, Debug)]
pub struct PreparedCache {
    root: PathBuf,
}

impl PreparedCache {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The user-facing location is `~/.ilium/cache/minecraft`. A scene given a
    /// non-default `cache_dir` (tests, isolated runs) keeps its cache inside it,
    /// so isolated runs never touch the user's real cache.
    pub fn for_scene_cache_dir(scene_cache_dir: &Path) -> Self {
        if scene_cache_dir == crate::source::default_cache_dir() {
            if let Some(dirs) = directories::BaseDirs::new() {
                return Self::at(
                    dirs.home_dir()
                        .join(".ilium")
                        .join("cache")
                        .join("minecraft"),
                );
            }
        }
        Self::at(scene_cache_dir.join("prepared"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn entry_path(&self, kind: &str, key: &Key) -> PathBuf {
        self.root.join(kind).join(format!("{}.ilpc", key.hex()))
    }

    /// Returns the cached value, or `None` on any miss or damage (damaged
    /// entries are deleted so the recomputed value replaces them).
    pub fn load<T: DeserializeOwned>(&self, kind: &str, key: &Key) -> Option<T> {
        let path = self.entry_path(kind, key);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "prepared cache unreadable");
                return None;
            }
        };
        match decode::<T>(&bytes, key) {
            Some(value) => Some(value),
            None => {
                tracing::warn!(path = %path.display(), "prepared cache entry invalid; recomputing");
                let _ = fs::remove_file(&path);
                None
            }
        }
    }

    /// Publishes atomically: write a unique temporary file, then rename.
    pub fn store<T: Serialize>(&self, kind: &str, key: &Key, value: &T) -> Result<(), Error> {
        let raw = serde_json::to_vec(value)?;
        if raw.len() as u64 > MAX_PAYLOAD_BYTES {
            return Err(Error::TooLarge);
        }
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&raw)?;
        let body = encoder.finish()?;
        let mut file_bytes = Vec::with_capacity(HEADER_BYTES + body.len());
        file_bytes.extend_from_slice(MAGIC);
        file_bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        file_bytes.extend_from_slice(&key.0);
        file_bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        file_bytes.extend_from_slice(&Sha256::digest(&body));
        file_bytes.extend_from_slice(&body);
        let path = self.entry_path(kind, key);
        let directory = path.parent().unwrap_or(&self.root);
        fs::create_dir_all(directory)?;
        let temporary = directory.join(format!(".{}.{}.tmp", key.hex(), std::process::id()));
        let result = (|| -> io::Result<()> {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(&file_bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        Ok(result?)
    }
}

fn decode<T: DeserializeOwned>(bytes: &[u8], key: &Key) -> Option<T> {
    if bytes.len() < HEADER_BYTES || &bytes[..4] != MAGIC {
        return None;
    }
    if bytes[4..8] != FORMAT_VERSION.to_le_bytes() || bytes[8..40] != key.0 {
        return None;
    }
    let raw_len = u64::from_le_bytes(bytes[40..48].try_into().ok()?);
    if raw_len > MAX_PAYLOAD_BYTES {
        return None;
    }
    let body = &bytes[HEADER_BYTES..];
    if Sha256::digest(body).as_slice() != &bytes[48..80] {
        return None;
    }
    let mut raw = Vec::new();
    // Read one byte past the declared length so an oversize stream is detected.
    DeflateDecoder::new(body)
        .take(raw_len + 1)
        .read_to_end(&mut raw)
        .ok()?;
    if raw.len() as u64 != raw_len {
        return None;
    }
    serde_json::from_slice(&raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn key(label: &str) -> Key {
        KeyBuilder::new("test").text(label).finish()
    }

    #[test]
    fn round_trip_and_miss_on_other_key() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PreparedCache::at(directory.path());
        let value: BTreeSet<String> = (0..500).map(|n| format!("minecraft:block/{n}")).collect();
        assert!(cache.load::<BTreeSet<String>>("ids", &key("a")).is_none());
        cache.store("ids", &key("a"), &value).unwrap();
        assert_eq!(
            cache.load::<BTreeSet<String>>("ids", &key("a")),
            Some(value)
        );
        assert!(cache.load::<BTreeSet<String>>("ids", &key("b")).is_none());
    }

    #[test]
    fn entry_is_compressed() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PreparedCache::at(directory.path());
        let value = vec!["minecraft:block/stone".to_owned(); 2000];
        cache.store("ids", &key("a"), &value).unwrap();
        let size = fs::metadata(cache.entry_path("ids", &key("a")))
            .unwrap()
            .len();
        assert!(size < 2000, "entry was {size} bytes");
    }

    #[test]
    fn corrupted_and_truncated_entries_miss_and_are_removed() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PreparedCache::at(directory.path());
        let value = vec![1u32, 2, 3];
        cache.store("n", &key("a"), &value).unwrap();
        let path = cache.entry_path("n", &key("a"));
        let mut bytes = fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() ^= 0xff;
        fs::write(&path, &bytes).unwrap();
        assert!(cache.load::<Vec<u32>>("n", &key("a")).is_none());
        assert!(!path.exists());
        cache.store("n", &key("a"), &value).unwrap();
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
        assert!(cache.load::<Vec<u32>>("n", &key("a")).is_none());
    }

    #[test]
    fn format_version_bump_is_a_miss() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PreparedCache::at(directory.path());
        cache.store("n", &key("a"), &vec![1u32]).unwrap();
        let path = cache.entry_path("n", &key("a"));
        let mut bytes = fs::read(&path).unwrap();
        bytes[4..8].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());
        fs::write(&path, bytes).unwrap();
        assert!(cache.load::<Vec<u32>>("n", &key("a")).is_none());
    }

    #[test]
    fn changed_input_file_changes_the_fingerprint() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("r.0.0.mca");
        fs::write(&file, b"one").unwrap();
        let before = KeyBuilder::new("k")
            .path_fingerprint(directory.path())
            .finish();
        fs::write(&file, b"three").unwrap();
        let after = KeyBuilder::new("k")
            .path_fingerprint(directory.path())
            .finish();
        assert_ne!(before, after);
        let missing = KeyBuilder::new("k")
            .path_fingerprint(&directory.path().join("x"))
            .finish();
        assert_ne!(missing, after);
    }

    #[test]
    fn non_default_cache_dir_stays_inside_it() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PreparedCache::for_scene_cache_dir(directory.path());
        assert!(cache.root().starts_with(directory.path()));
    }
}
