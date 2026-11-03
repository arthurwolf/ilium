//! Private, bounded restart cache for incremental session statistics.
//! Cache entries are advisory: an invalid or unavailable entry is a miss.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::StatsRequest;
use crate::session_stats::StatsAccumulator;

const VERSION: u32 = 1;
// JSON containers can expand in memory, so the read bound is much smaller
// than the separately admitted decoder workspace.
const MAX_BYTES: u64 = 4 * 1024 * 1024;
const ANCHOR_BYTES: u64 = 4096;
const SLOTS: u8 = 64;

pub(super) fn source_identity(
    path: &Path,
) -> io::Result<ilium_platform::file_identity::FileIdentity> {
    ilium_platform::file_identity::identity(&ilium_platform::secure_fs::open_regular_file(path)?)
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct Owner {
    class: ilium_core::AgentClass,
    session_id: String,
    project_path: PathBuf,
    home: PathBuf,
}
impl From<&StatsRequest> for Owner {
    fn from(request: &StatsRequest) -> Self {
        Self {
            class: request.class.clone(),
            session_id: request.session_id.clone(),
            project_path: request.project_path.clone(),
            home: request.home.clone(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Fingerprint {
    path: PathBuf,
    offset: u64,
    device: u64,
    inode: u64,
    digest: Vec<u8>,
}
impl Fingerprint {
    fn capture(path: &Path, offset: u64) -> io::Result<Self> {
        let mut file = ilium_platform::secure_fs::open_regular_file(path)?;
        if file.metadata()?.len() < offset {
            return Err(io::Error::other("transcript was truncated"));
        }
        let identity = ilium_platform::file_identity::identity(&file)?;
        let size = offset.min(ANCHOR_BYTES) as usize;
        let mut bytes = vec![0; size];
        file.read_exact(&mut bytes)?;
        let mut hash = Sha256::new();
        hash.update(&bytes);
        file.seek(SeekFrom::Start(offset.saturating_sub(size as u64)))?;
        file.read_exact(&mut bytes)?;
        hash.update(&bytes);
        Ok(Self {
            path: path.to_path_buf(),
            offset,
            device: identity.device,
            inode: identity.inode,
            digest: hash.finalize().to_vec(),
        })
    }
    fn valid(&self) -> bool {
        Self::capture(&self.path, self.offset).is_ok_and(|other| {
            self.device == other.device && self.inode == other.inode && self.digest == other.digest
        })
    }
}

#[derive(Serialize, Deserialize)]
struct Cache {
    version: u32,
    owner: Owner,
    main: Fingerprint,
    extra: Vec<Fingerprint>,
    accumulator: StatsAccumulator,
}

fn slot(request: &StatsRequest) -> Option<PathBuf> {
    #[cfg(test)]
    let root = request.home.join(".cache/ilium");
    #[cfg(not(test))]
    let root = directories::ProjectDirs::from("", "", "ilium")?
        .cache_dir()
        .to_path_buf();
    let key = serde_json::to_vec(&Owner::from(request)).ok()?;
    let hash = Sha256::digest(key);
    Some(
        root.join("session-statistics")
            .join(format!("slot-{:02}.json", hash[0] % SLOTS)),
    )
}

fn read(path: &Path, request: &StatsRequest) -> Option<Cache> {
    let file = ilium_platform::secure_fs::open_regular_file(path).ok()?;
    ilium_platform::secure_fs::validate_private_open_file(&file).ok()?;
    if file.metadata().ok()?.len() > MAX_BYTES {
        return None;
    }
    let cache: Cache = serde_json::from_reader(BufReader::new(file.take(MAX_BYTES))).ok()?;
    cache.accumulator.check_bounds().ok()?;
    if cache.version != VERSION
        || cache.owner != Owner::from(request)
        || cache.accumulator.class() != &request.class
        || cache.accumulator.offset() != cache.main.offset
        || !cache.main.valid()
        || cache.extra.len() != cache.accumulator.extra_offsets().len()
    {
        return None;
    }
    let mut paths = HashSet::new();
    for fingerprint in &cache.extra {
        if !paths.insert(&fingerprint.path)
            || cache.accumulator.extra_offsets().get(&fingerprint.path) != Some(&fingerprint.offset)
            || !fingerprint.valid()
        {
            return None;
        }
    }
    Some(cache)
}

pub(super) fn load(request: &StatsRequest) -> Option<(PathBuf, Box<StatsAccumulator>)> {
    let cache = read(&slot(request)?, request)?;
    Some((cache.main.path, Box::new(cache.accumulator)))
}

pub(super) fn save(
    request: &StatsRequest,
    path: &Path,
    accumulator: &StatsAccumulator,
    cancelled: impl Fn() -> bool,
) -> io::Result<()> {
    let destination =
        slot(request).ok_or_else(|| io::Error::other("no statistics cache directory"))?;
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("no statistics cache parent"))?;
    ilium_platform::secure_fs::create_private_directory(parent)?;
    let _lock = ilium_platform::file_lock::ExclusiveFileLock::try_acquire(
        &destination.with_extension("lock"),
    )?
    .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "statistics cache slot busy"))?;
    let main = Fingerprint::capture(path, accumulator.offset())?;
    let extra = accumulator
        .extra_offsets()
        .iter()
        .map(|(path, offset)| Fingerprint::capture(path, *offset))
        .collect::<io::Result<Vec<_>>>()?;
    if let Some(previous) = read(&destination, request) {
        if previous.main.device == main.device
            && previous.main.inode == main.inode
            && (previous.main.offset > main.offset
                || previous.extra.iter().any(|old| {
                    extra
                        .iter()
                        .find(|new| new.path == old.path)
                        .is_none_or(|new| new.offset < old.offset)
                }))
        {
            return Ok(());
        }
    }
    let document = CacheRef {
        version: VERSION,
        owner: Owner::from(request),
        main,
        extra,
        accumulator,
    };
    let temporary = parent.join(format!(".statistics-{}.tmp", uuid::Uuid::new_v4()));
    let mut output = ilium_platform::secure_fs::private_open_options()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        let mut writer = BufWriter::new(LimitedWriter {
            output: &mut output,
            remaining: MAX_BYTES,
        });
        serde_json::to_writer(&mut writer, &document).map_err(io::Error::other)?;
        writer.flush()?;
        drop(writer);
        output.sync_all()?;
        if cancelled() {
            return Err(io::Error::other("statistics cache write cancelled"));
        }
        ilium_platform::secure_fs::replace_file_durably(&temporary, &destination)
    })();
    drop(output);
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[derive(Serialize)]
struct CacheRef<'a> {
    version: u32,
    owner: Owner,
    main: Fingerprint,
    extra: Vec<Fingerprint>,
    accumulator: &'a StatsAccumulator,
}

struct LimitedWriter<'a> {
    output: &'a mut File,
    remaining: u64,
}
impl Write for LimitedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(io::Error::other("statistics cache exceeds size limit"));
        }
        let written = self.output.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_and_oversized_cache_entries_are_misses() {
        let home = tempfile::tempdir().unwrap();
        let request = StatsRequest {
            class: ilium_core::AgentClass::Claude,
            session_id: "synthetic".into(),
            project_path: home.path().join("project"),
            home: home.path().to_path_buf(),
        };
        let path = slot(&request).unwrap();
        ilium_platform::secure_fs::create_private_directory(path.parent().unwrap()).unwrap();
        let mut file = ilium_platform::secure_fs::private_open_options()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.write_all(b"{broken").unwrap();
        drop(file);
        assert!(load(&request).is_none());
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(MAX_BYTES + 1).unwrap();
        drop(file);
        assert!(load(&request).is_none());
    }
}
