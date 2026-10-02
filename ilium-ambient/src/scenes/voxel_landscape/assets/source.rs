//! Worker-only read adapters. A directory capability, not canonicalize-then-open,
//! contains every path lookup. No archive member is extracted to the filesystem.
use super::budget::{zeroed_bytes, ByteBudget, Cancel, Limits, Reservation};
use super::error::{AssetError, Result};
use super::identity::{AssetPath, BlobOrigin, Digest256, SourceBlob};
use cap_std::fs::Dir;
use ilium_platform::secure_fs::NoFollowDirectory;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read, path::Path};
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceLimits {
    pub archive_bytes: u64,
    pub member_bytes: u64,
    pub total_member_bytes: u64,
    pub entries: usize,
    pub depth: usize,
}
impl Default for SourceLimits {
    fn default() -> Self {
        Self {
            archive_bytes: 256 * 1024 * 1024,
            member_bytes: 64 * 1024 * 1024,
            total_member_bytes: 2 * 1024 * 1024 * 1024,
            entries: 32768,
            depth: 32,
        }
    }
}
impl SourceLimits {
    pub fn validate(self) -> Result<()> {
        let cap = Self::default();
        for (name, value, ceiling) in [
            ("archive bytes", self.archive_bytes, cap.archive_bytes),
            ("member bytes", self.member_bytes, cap.member_bytes),
            (
                "total source bytes",
                self.total_member_bytes,
                cap.total_member_bytes,
            ),
            ("source entries", self.entries as u64, cap.entries as u64),
            ("source depth", self.depth as u64, cap.depth as u64),
        ] {
            check_limit(name, value, ceiling)?;
            if value == 0 {
                return Err(AssetError::InvalidMetadata(format!("zero {name}")));
            }
        }
        Ok(())
    }
}
pub(crate) fn check_limit(name: &'static str, value: u64, cap: u64) -> Result<()> {
    if value > cap {
        return Err(AssetError::Limit {
            resource: name,
            requested: value,
            limit: cap,
        });
    }
    Ok(())
}
pub(crate) fn io_error(error: std::io::Error) -> AssetError {
    AssetError::InvalidMetadata(format!(
        "local source I/O: {}",
        super::error::summary(&error.to_string())
    ))
}
#[derive(Debug)]
pub struct SourceBytes {
    pub(crate) bytes: Vec<u8>,
    pub(crate) digest: Digest256,
    pub(crate) reservation: Reservation,
}
impl SourceBytes {
    pub fn read_exact_size(
        mut reader: impl Read,
        length: u64,
        cap: u64,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        check_limit("source read bytes", length, cap)?;
        let length = usize::try_from(length).map_err(|_| AssetError::Allocation)?;
        let reservation = budget.reserve(length as u64, cancel)?;
        let mut bytes = zeroed_bytes(length)?;
        for chunk in bytes.chunks_mut(64 * 1024) {
            cancel.check()?;
            reader.read_exact(chunk).map_err(io_error)?;
        }
        cancel.check()?;
        let mut extra = [0_u8; 1];
        if reader.read(&mut extra).map_err(io_error)? != 0 {
            return Err(AssetError::InvalidMetadata(
                "source grew beyond its admitted byte length".into(),
            ));
        }
        let digest = Digest256::of_checked(&bytes, cancel)?;
        Ok(Self {
            bytes,
            digest,
            reservation,
        })
    }
    pub fn from_slice(
        bytes: &[u8],
        cap: u64,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        Self::read_exact_size(bytes, bytes.len() as u64, cap, budget, cancel)
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn digest(&self) -> Digest256 {
        self.digest
    }
    pub(crate) fn uses_budget(&self, budget: &ByteBudget) -> bool {
        self.reservation.belongs_to(budget)
    }
    pub fn verify(&self, expected: Digest256) -> Result<()> {
        if self.digest != expected {
            return Err(AssetError::Integrity {
                expected: expected.to_string(),
                actual: self.digest.to_string(),
            });
        }
        Ok(())
    }
    pub fn into_blob(
        self,
        origin: BlobOrigin,
        expected: Option<Digest256>,
        limits: &Limits,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<SourceBlob> {
        SourceBlob::from_reserved(
            self.bytes,
            origin,
            expected,
            limits,
            self.reservation,
            budget,
            cancel,
        )
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct MemberInfo {
    pub path: AssetPath,
    pub bytes: u64,
}
/// Adapter calls are blocking and only legal on the owned asset worker.
/// `None` means a genuinely absent member. Corruption/I/O failure is never None.
pub trait AssetSource: Send + Sync {
    fn members(&self) -> &BTreeMap<AssetPath, MemberInfo>;
    fn read(
        &self,
        path: &AssetPath,
        cap: u64,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Option<SourceBytes>>;
    fn source_digest(&self) -> Option<Digest256>;
}
pub struct LocalTree {
    root: Dir,
    read_root: NoFollowDirectory,
    members: BTreeMap<AssetPath, MemberInfo>,
    pins: BTreeMap<AssetPath, Digest256>,
    limits: SourceLimits,
    reservations: Vec<Reservation>,
}
impl LocalTree {
    /// The ambient root is an explicit trusted user configuration, never pack data.
    /// A supplied pin inventory is optional but authoritative for every listed pin.
    pub fn open(
        root: &Path,
        pins: BTreeMap<AssetPath, Digest256>,
        limits: SourceLimits,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        limits.validate()?;
        cancel.check()?;
        check_limit("source hash pins", pins.len() as u64, limits.entries as u64)?;
        let pin_charge = budget.reserve(pins.len() as u64 * 1536, cancel)?;
        let read_root = NoFollowDirectory::open_root(root).map_err(io_error)?;
        let root = Dir::from_std_file(read_root.try_clone_file().map_err(io_error)?);
        let mut value = Self {
            root,
            read_root,
            members: BTreeMap::new(),
            pins,
            limits,
            reservations: vec![pin_charge],
        };
        value.scan(budget, cancel)?;
        for path in value.pins.keys() {
            if !value.members.contains_key(path) {
                return Err(AssetError::InvalidMetadata(format!(
                    "pinned member absent: {}",
                    path.as_str()
                )));
            }
        }
        Ok(value)
    }
    fn scan(&mut self, budget: &ByteBudget, cancel: Cancel<'_>) -> Result<()> {
        let mut pending = vec![(None::<AssetPath>, 0_usize)];
        let mut count = 0_usize;
        let mut total = 0_u64;
        while let Some((parent, depth)) = pending.pop() {
            cancel.check()?;
            let directory = parent.as_ref().map_or(".", AssetPath::as_str);
            for entry in self.root.read_dir(directory).map_err(io_error)? {
                cancel.check()?;
                let entry = entry.map_err(io_error)?;
                let name = entry.file_name();
                let name = name
                    .to_str()
                    .ok_or_else(|| AssetError::InvalidPath("non-UTF-8 source name".into()))?;
                let child = AssetPath::parse(name)?;
                let path = match &parent {
                    Some(parent) => parent.join(&child)?,
                    None => child,
                };
                count += 1;
                check_limit("source entries", count as u64, self.limits.entries as u64)?;
                check_limit("source depth", (depth + 1) as u64, self.limits.depth as u64)?;
                self.reservations.push(budget.reserve(2048, cancel)?);
                let metadata = self
                    .root
                    .symlink_metadata(path.as_str())
                    .map_err(io_error)?;
                if metadata.file_type().is_symlink() {
                    return Err(AssetError::InvalidPath(format!(
                        "symlink source member: {}",
                        path.as_str()
                    )));
                }
                if metadata.is_dir() {
                    pending.push((Some(path), depth + 1));
                    continue;
                }
                if !metadata.is_file() {
                    return Err(AssetError::InvalidPath("non-regular source member".into()));
                }
                check_limit("member bytes", metadata.len(), self.limits.member_bytes)?;
                total = total
                    .checked_add(metadata.len())
                    .ok_or(AssetError::Allocation)?;
                check_limit("total source bytes", total, self.limits.total_member_bytes)?;
                if self
                    .members
                    .insert(
                        path.clone(),
                        MemberInfo {
                            path,
                            bytes: metadata.len(),
                        },
                    )
                    .is_some()
                {
                    return Err(AssetError::Duplicate("directory member".into()));
                }
            }
        }
        Ok(())
    }
}
impl AssetSource for LocalTree {
    fn members(&self) -> &BTreeMap<AssetPath, MemberInfo> {
        &self.members
    }
    fn source_digest(&self) -> Option<Digest256> {
        None
    }
    fn read(
        &self,
        path: &AssetPath,
        cap: u64,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Option<SourceBytes>> {
        cancel.check()?;
        if !self
            .reservations
            .first()
            .is_some_and(|charge| charge.belongs_to(budget))
        {
            return Err(AssetError::InvalidMetadata(
                "tree read account differs from its index account".into(),
            ));
        }
        let Some(info) = self.members.get(path) else {
            return Ok(None);
        };
        // Every component is opened relative to the previous admitted handle.
        // open_regular uses O_NONBLOCK on Unix, so a raced-in FIFO cannot hold
        // this worker inside open before the regular-file check can run.
        let mut components = path.as_str().split('/').peekable();
        let mut directory: Option<NoFollowDirectory> = None;
        let mut file = loop {
            let component = components
                .next()
                .ok_or_else(|| AssetError::InvalidPath("empty member path".into()))?;
            let name = std::ffi::OsStr::new(component);
            let parent = directory.as_ref().unwrap_or(&self.read_root);
            if components.peek().is_none() {
                break parent.open_regular(name).map_err(io_error)?;
            }
            directory = Some(parent.open_directory(name).map_err(io_error)?);
            cancel.check()?;
        };
        let before = file.metadata().map_err(io_error)?;
        if !before.is_file() || before.len() != info.bytes {
            return Err(AssetError::InvalidMetadata(
                "indexed source changed type or length".into(),
            ));
        }
        let bytes = SourceBytes::read_exact_size(
            &mut file,
            info.bytes,
            cap.min(self.limits.member_bytes),
            budget,
            cancel,
        )?;
        let after = file.metadata().map_err(io_error)?;
        if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
            return Err(AssetError::InvalidMetadata(
                "source changed during read".into(),
            ));
        }
        if let Some(pin) = self.pins.get(path) {
            bytes.verify(*pin)?;
        }
        Ok(Some(bytes))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    #[test]
    fn bounded_reader_rejects_growth_short_reads_wrong_hash_and_cancellation() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(4096).unwrap();
        assert!(SourceBytes::read_exact_size(&b"abc"[..], 2, 10, &budget, cancel).is_err());
        assert!(SourceBytes::read_exact_size(&b"a"[..], 2, 10, &budget, cancel).is_err());
        assert_eq!(budget.used(), 0);
        let bytes = SourceBytes::from_slice(b"abc", 10, &budget, cancel).unwrap();
        assert!(bytes.verify(Digest256::of(b"xyz")).is_err());
        drop(bytes);
        stop.store(true, std::sync::atomic::Ordering::Release);
        assert!(matches!(
            SourceBytes::from_slice(b"x", 10, &budget, cancel),
            Err(AssetError::Cancelled)
        ));
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn tree_index_is_sorted_and_changed_or_unpinned_paths_do_not_escape() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("nested/b"), b"bbb").unwrap();
        std::fs::write(dir.path().join("a"), b"aaa").unwrap();
        let budget = ByteBudget::new(1 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let path = AssetPath::parse("a").unwrap();
        let pins = BTreeMap::from([(path.clone(), Digest256::of(b"aaa"))]);
        let tree =
            LocalTree::open(dir.path(), pins, SourceLimits::default(), &budget, cancel).unwrap();
        assert_eq!(tree.members().keys().next().unwrap().as_str(), "a");
        assert_eq!(
            tree.read(&path, 10, &budget, cancel)
                .unwrap()
                .unwrap()
                .bytes(),
            b"aaa"
        );
        std::fs::write(dir.path().join("a"), b"bbb").unwrap();
        assert!(tree.read(&path, 10, &budget, cancel).is_err());
        assert!(tree
            .read(&AssetPath::parse("missing").unwrap(), 10, &budget, cancel)
            .unwrap()
            .is_none());
    }
    #[cfg(unix)]
    #[test]
    fn tree_rejects_external_links_and_post_index_symlink_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), b"secret").unwrap();
        std::fs::write(dir.path().join("a"), b"public").unwrap();
        let budget = ByteBudget::new(1 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let tree = LocalTree::open(
            dir.path(),
            BTreeMap::new(),
            SourceLimits::default(),
            &budget,
            cancel,
        )
        .unwrap();
        std::fs::remove_file(dir.path().join("a")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("a")).unwrap();
        assert!(tree
            .read(&AssetPath::parse("a").unwrap(), 10, &budget, cancel)
            .is_err());
        assert!(LocalTree::open(
            dir.path(),
            BTreeMap::new(),
            SourceLimits::default(),
            &budget,
            cancel
        )
        .is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn indexed_file_replaced_by_fifo_is_rejected_before_a_writer_arrives() {
        use std::sync::atomic::Ordering;
        use std::time::{Duration, Instant};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("member");
        std::fs::write(&path, b"abc").unwrap();
        let budget = ByteBudget::new(1 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let tree = LocalTree::open(
            dir.path(),
            BTreeMap::new(),
            SourceLimits::default(),
            &budget,
            cancel,
        )
        .unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success());
        let writer_path = path.clone();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(writer_path)
                .unwrap();
        });
        let started = Instant::now();
        let result = tree.read(&AssetPath::parse("member").unwrap(), 3, &budget, cancel);
        let elapsed = started.elapsed();
        stop.store(true, Ordering::Release);
        writer.join().unwrap();
        assert!(result.is_err());
        assert!(
            elapsed < Duration::from_millis(150),
            "FIFO admission waited {elapsed:?}"
        );
        assert_eq!(budget.used(), 2048);
    }
}
