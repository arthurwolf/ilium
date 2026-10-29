//! Fail-closed local resource-ledger durability primitives.
//!
//! Every transaction holds a separate stable lock and reconciles the current
//! file before applying a mutation. A held descriptor can prove that an unchanged
//! inode was already reconciled; replacement always requires fresh durability.
use crate::{file_lock::ExclusiveFileLock, secure_fs};
use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
/// One bounded, process-local durability proof, never catalog authority.
#[derive(Debug, Default)]
pub struct ResourceLedgerReadCache(std::sync::Mutex<Option<VerifiedLedger>>);
#[derive(Debug)]
struct VerifiedLedger {
    path: PathBuf,
    missing: bool,
    // Keeping the descriptor open prevents inode-number reuse from making an
    // unacknowledged replacement look like the previously reconciled file.
    file: std::fs::File,
    identity: LedgerIdentity,
}
#[derive(Debug, PartialEq, Eq)]
struct LedgerIdentity {
    device: u64,
    inode: u64,
    length: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
fn ledger_identity(file: &std::fs::File) -> io::Result<LedgerIdentity> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        Ok(LedgerIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = file;
        Err(unsupported())
    }
}
#[derive(Debug)]
pub struct ResourceLedgerFile {
    path: PathBuf,
    _lock: ExclusiveFileLock,
}
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "resource queue requires a supported local durable filesystem",
    )
}
fn verify_local(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
        let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: path is terminated and stats points to writable statfs storage.
        if unsafe { libc::statfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: statfs initialized stats on success.
        let kind = unsafe { stats.assume_init() }.f_type as u64;
        // Explicit local filesystems: ext, XFS and Btrfs. Volatile, layered, network/FUSE
        // mounts and unknown implementations fail closed rather than imply leases.
        if ![0xef53, 0x58465342, 0x9123683e].contains(&kind) {
            return Err(unsupported());
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(unsupported())
    }
}
// Faults are compiled only into this crate's unit-test binary. Each child
// selects its own thread-local stage; release builds contain no test switch.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReplaceTestStage {
    BeforeTempFlush,
    AfterTempFlush,
    BeforeAcknowledgement,
}
#[cfg(test)]
thread_local! {static REPLACE_TEST_CRASH:std::cell::Cell<Option<ReplaceTestStage>>=const{std::cell::Cell::new(None)};static REPLACE_TEST_WRITE_ERROR:std::cell::Cell<Option<i32>>=const{std::cell::Cell::new(None)};}
#[cfg(test)]
thread_local! {static RECONCILE_TEST_ERROR:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
#[cfg(test)]
fn crash_at_test_stage(stage: ReplaceTestStage) {
    if REPLACE_TEST_CRASH.with(|configured| configured.get() == Some(stage)) {
        std::process::exit(86)
    }
}
impl ResourceLedgerFile {
    /// One nonblocking attempt. The admitted caller owns retries/cancellation.
    pub fn try_lock(path: &Path) -> io::Result<Option<Self>> {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "resource catalog path must be absolute",
            ));
        }
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "catalog has no parent"))?;
        let mut absent = Vec::new();
        let mut ancestor = Some(parent);
        while let Some(path) = ancestor {
            if path.exists() {
                break;
            }
            absent.push(path.to_path_buf());
            ancestor = path.parent();
        }
        secure_fs::create_private_directory(parent)?;
        verify_local(parent)?;
        for directory in absent.iter().rev() {
            secure_fs::sync_parent_directory(directory)?;
        }
        // Resolve the directory once, preserving the no-follow final-component rule.
        let parent = parent.canonicalize()?;
        let name = path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "catalog filename absent")
        })?;
        let path = parent.join(name);
        let lock_path = path.with_extension("lock");
        let Some(lock) = ExclusiveFileLock::try_acquire(&lock_path)? else {
            return Ok(None);
        };
        // read_reconciled synchronizes this directory with the catalog. Doing
        // it before every read would force a journal flush even for an unchanged
        // file with an existing durability proof. replace also syncs the parent.
        Ok(Some(Self { path, _lock: lock }))
    }
    pub fn read_reconciled(&self, maximum: u64) -> io::Result<Option<Vec<u8>>> {
        self.read_with_cache(maximum, None)
    }
    pub fn read_reconciled_cached(
        &self,
        maximum: u64,
        cache: &ResourceLedgerReadCache,
    ) -> io::Result<Option<Vec<u8>>> {
        self.read_with_cache(maximum, Some(cache))
    }
    fn read_with_cache(
        &self,
        maximum: u64,
        cache: Option<&ResourceLedgerReadCache>,
    ) -> io::Result<Option<Vec<u8>>> {
        let mut cached = cache
            .map(|cache| {
                cache
                    .0
                    .lock()
                    .map_err(|_| io::Error::other("resource durability cache poisoned"))
            })
            .transpose()?;
        let file = match secure_fs::private_open_options()
            .read(true)
            .write(true)
            .open(&self.path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let parent = self
                    .path
                    .parent()
                    .ok_or_else(|| io::Error::other("catalog parent absent"))?;
                let directory = std::fs::File::open(parent)?;
                let identity = ledger_identity(&directory)?;
                let verified = cached
                    .as_ref()
                    .and_then(|entry| entry.as_ref())
                    .is_some_and(|entry| {
                        entry.missing
                            && entry.path == self.path
                            && entry.identity == identity
                            && ledger_identity(&entry.file).is_ok_and(|held| held == identity)
                    });
                if !verified {
                    #[cfg(test)]
                    if RECONCILE_TEST_ERROR.with(|fault| fault.replace(false)) {
                        return Err(io::Error::other(
                            "injected durability reconciliation failure",
                        ));
                    }
                    secure_fs::sync_parent_directory(&self.path)?;
                    if let Some(cached) = cached.as_mut() {
                        **cached = Some(VerifiedLedger {
                            path: self.path.clone(),
                            missing: true,
                            file: directory,
                            identity,
                        });
                    }
                }
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "catalog is not a regular file",
            ));
        }
        secure_fs::restrict_open_file_to_owner(&file)?;
        if file.metadata()?.len() > maximum {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "catalog exceeds byte capacity",
            ));
        }
        let identity = ledger_identity(&file)?;
        let verified = cached
            .as_ref()
            .and_then(|entry| entry.as_ref())
            .is_some_and(|entry| {
                !entry.missing
                    && entry.path == self.path
                    && entry.identity == identity
                    && ledger_identity(&entry.file).is_ok_and(|held| held == identity)
            });
        if !verified {
            #[cfg(test)]
            if RECONCILE_TEST_ERROR.with(|fault| fault.replace(false)) {
                return Err(io::Error::other(
                    "injected durability reconciliation failure",
                ));
            }
            file.sync_all()?;
            secure_fs::sync_parent_directory(&self.path)?;
            if let Some(cached) = cached.as_mut() {
                **cached = Some(VerifiedLedger {
                    path: self.path.clone(),
                    missing: false,
                    file: file.try_clone()?,
                    identity,
                });
            }
        }
        let mut bytes = Vec::new();
        file.take(maximum + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > maximum {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "catalog exceeds byte capacity",
            ));
        }
        Ok(Some(bytes))
    }
    /// An error may mean rename happened without a durability acknowledgement.
    /// The next holder MUST call read_reconciled before doing anything else.
    pub fn replace(&self, bytes: &[u8]) -> io::Result<()> {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(".resource-{}-{sequence}.tmp", std::process::id());
        let temp = self.path.with_file_name(name);
        let mut file = secure_fs::private_open_options()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let result = (|| {
            #[cfg(test)]
            if let Some(code) = REPLACE_TEST_WRITE_ERROR.with(|fault| fault.take()) {
                return Err(io::Error::from_raw_os_error(code));
            }
            file.write_all(bytes)?;
            #[cfg(test)]
            crash_at_test_stage(ReplaceTestStage::BeforeTempFlush);
            file.sync_all()?;
            #[cfg(test)]
            crash_at_test_stage(ReplaceTestStage::AfterTempFlush);
            secure_fs::replace_file_durably(&temp, &self.path)?;
            #[cfg(test)]
            crash_at_test_stage(ReplaceTestStage::BeforeAcknowledgement);
            Ok(())
        })();
        // Only our uniquely created temporary path is eligible for cleanup.
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn durable_test_directory() -> tempfile::TempDir {
        let root = std::env::var_os("ILIUM_RESOURCE_TEST_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/data/ilium-resource-queue-tests"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".scratch-reaper"), b"").unwrap();
        tempfile::tempdir_in(root).unwrap()
    }
    #[test]
    fn cached_read_skips_only_previously_reconciled_inode_and_rechecks_replacements() {
        let directory = durable_test_directory();
        let path = directory.path().join("catalog.json");
        let cache = ResourceLedgerReadCache::default();
        let ledger = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
        assert!(ledger
            .read_reconciled_cached(1024, &cache)
            .unwrap()
            .is_none());
        RECONCILE_TEST_ERROR.with(|fault| fault.set(true));
        assert!(ledger
            .read_reconciled_cached(1024, &cache)
            .unwrap()
            .is_none());
        ledger.replace(b"acknowledged").unwrap();
        assert!(ledger.read_reconciled_cached(1024, &cache).is_err());
        assert_eq!(
            ledger
                .read_reconciled_cached(1024, &cache)
                .unwrap()
                .unwrap(),
            b"acknowledged"
        );
        drop(ledger);
        RECONCILE_TEST_ERROR.with(|fault| fault.set(true));
        let ledger = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
        // Same file, still held open by the cache: no new durability operation.
        assert_eq!(
            ledger
                .read_reconciled_cached(1024, &cache)
                .unwrap()
                .unwrap(),
            b"acknowledged"
        );
        ledger.replace(b"new-owner").unwrap();
        // A replacement must not inherit the old inode's proof, even though
        // the old descriptor remains open and the path did not change.
        assert!(ledger
            .read_reconciled_cached(1024, &cache)
            .unwrap_err()
            .to_string()
            .contains("injected durability"));
        assert_eq!(
            ledger
                .read_reconciled_cached(1024, &cache)
                .unwrap()
                .unwrap(),
            b"new-owner"
        );
        RECONCILE_TEST_ERROR.with(|fault| fault.set(true));
        assert!(ledger
            .read_reconciled_cached(1024, &ResourceLedgerReadCache::default())
            .is_err());
    }
    #[test]
    fn warmed_cache_reconciles_child_rename_without_acknowledgement() {
        let directory = durable_test_directory();
        let path = directory.path().join("catalog.json");
        let cache = ResourceLedgerReadCache::default();
        let ledger = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
        ledger.replace(b"old-owner").unwrap();
        assert_eq!(
            ledger
                .read_reconciled_cached(1024, &cache)
                .unwrap()
                .unwrap(),
            b"old-owner"
        );
        drop(ledger);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "resource_fs::tests::child_crash_after_unacknowledged_rename",
                "--nocapture",
            ])
            .env("ILIUM_RESOURCE_CRASH_RENAME_PATH", &path)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73));
        let ledger = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
        RECONCILE_TEST_ERROR.with(|fault| fault.set(true));
        assert!(ledger.read_reconciled_cached(1024, &cache).is_err());
        assert_eq!(
            ledger
                .read_reconciled_cached(1024, &cache)
                .unwrap()
                .unwrap(),
            b"new-owner"
        );
    }
    #[test]
    fn child_crash_after_unacknowledged_rename() {
        let Some(path) = std::env::var_os("ILIUM_RESOURCE_CRASH_RENAME_PATH") else {
            return;
        };
        let path = PathBuf::from(path);
        let lock = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
        lock.read_reconciled(1024).unwrap();
        let temporary = path.with_file_name("unacknowledged.tmp");
        let mut file = secure_fs::private_open_options()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .unwrap();
        file.write_all(b"new-owner").unwrap();
        file.sync_all().unwrap();
        std::fs::rename(&temporary, &path).unwrap();
        std::process::exit(73);
    }
    #[test]
    fn new_process_reconciles_unacknowledged_rename() {
        let directory = durable_test_directory();
        let path = directory.path().join("catalog.json");
        let lock = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
        lock.replace(b"old-owner").unwrap();
        drop(lock);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "resource_fs::tests::child_crash_after_unacknowledged_rename",
            ])
            .env("ILIUM_RESOURCE_CRASH_RENAME_PATH", &path)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73));
        let next = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
        assert_eq!(next.read_reconciled(1024).unwrap().unwrap(), b"new-owner");
        next.replace(b"acknowledged-owner").unwrap();
        drop(next);
        let reopened = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
        assert_eq!(
            reopened.read_reconciled(1024).unwrap().unwrap(),
            b"acknowledged-owner"
        );
    }

    #[test]
    fn child_crashes_at_real_replacement_stage() {
        let Some(path) = std::env::var_os("ILIUM_RESOURCE_STAGE_PATH") else {
            return;
        };
        let stage = match std::env::var("ILIUM_RESOURCE_STAGE").unwrap().as_str() {
            "before-flush" => ReplaceTestStage::BeforeTempFlush,
            "after-flush" => ReplaceTestStage::AfterTempFlush,
            "before-ack" => ReplaceTestStage::BeforeAcknowledgement,
            _ => panic!("unknown test stage"),
        };
        let ledger = ResourceLedgerFile::try_lock(Path::new(&path))
            .unwrap()
            .unwrap();
        ledger.read_reconciled(1024).unwrap();
        REPLACE_TEST_CRASH.with(|configured| configured.set(Some(stage)));
        ledger.replace(b"next-acknowledgement").unwrap();
        panic!("crash hook did not execute");
    }
    #[test]
    fn actual_replace_crash_boundaries_preserve_old_or_new_authority() {
        for stage in ["before-flush", "after-flush", "before-ack"] {
            let directory = durable_test_directory();
            let path = directory.path().join("catalog.json");
            let ledger = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
            ledger.replace(b"previous-acknowledged").unwrap();
            drop(ledger);
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "resource_fs::tests::child_crashes_at_real_replacement_stage",
                ])
                .env("ILIUM_RESOURCE_STAGE_PATH", &path)
                .env("ILIUM_RESOURCE_STAGE", stage)
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(86), "stage {stage}");
            let ledger = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
            let expected = if stage == "before-ack" {
                b"next-acknowledgement".as_slice()
            } else {
                b"previous-acknowledged".as_slice()
            };
            assert_eq!(
                ledger.read_reconciled(1024).unwrap().unwrap(),
                expected,
                "stage {stage}"
            );
            ledger.replace(b"subsequent-acknowledged").unwrap();
            drop(ledger);
            let reopened = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
            assert_eq!(
                reopened.read_reconciled(1024).unwrap().unwrap(),
                b"subsequent-acknowledged"
            );
            // Orphans from pre-rename crashes remain untrusted, never adopted.
            if stage != "before-ack" {
                assert!(std::fs::read_dir(directory.path())
                    .unwrap()
                    .any(|entry| entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .ends_with(".tmp")));
            }
        }
    }
    #[cfg(not(windows))]
    #[test]
    fn directory_sync_failure_reports_uncertainty_and_next_holder_reconciles() {
        let directory = durable_test_directory();
        let path = directory.path().join("catalog.json");
        let ledger = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
        ledger.replace(b"acknowledged").unwrap();
        secure_fs::fail_next_replacement_directory_sync_for_test();
        let failure = ledger.replace(b"published-but-unacknowledged").unwrap_err();
        assert!(failure.to_string().contains("durability unknown"));
        drop(ledger);
        let next = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
        assert_eq!(
            next.read_reconciled(1024).unwrap().unwrap(),
            b"published-but-unacknowledged"
        );
        next.replace(b"reconciled-successor").unwrap();
        drop(next);
        assert_eq!(
            ResourceLedgerFile::try_lock(&path)
                .unwrap()
                .unwrap()
                .read_reconciled(1024)
                .unwrap()
                .unwrap(),
            b"reconciled-successor"
        );
    }
    #[cfg(unix)]
    #[test]
    fn write_failures_preserve_acknowledged_catalog_and_cleanup_own_temp() {
        let directory = durable_test_directory();
        let path = directory.path().join("catalog.json");
        let ledger = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
        ledger.replace(b"acknowledged").unwrap();
        for code in [libc::ENOSPC, libc::EROFS, libc::EACCES] {
            REPLACE_TEST_WRITE_ERROR.with(|fault| fault.set(Some(code)));
            assert_eq!(
                ledger
                    .replace(b"must-not-publish")
                    .unwrap_err()
                    .raw_os_error(),
                Some(code)
            );
            assert_eq!(
                ledger.read_reconciled(1024).unwrap().unwrap(),
                b"acknowledged"
            );
            assert!(!std::fs::read_dir(directory.path())
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")));
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn volatile_or_kernel_filesystems_are_explicitly_unsupported() {
        assert_eq!(
            verify_local(Path::new("/proc")).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }
    #[cfg(unix)]
    #[test]
    fn stable_lock_symlink_cannot_redirect_mutation() {
        use std::os::unix::fs::symlink;
        let directory = durable_test_directory();
        let path = directory.path().join("catalog.json");
        let target = directory.path().join("unrelated");
        std::fs::write(&target, b"preserved").unwrap();
        symlink(&target, path.with_extension("lock")).unwrap();
        assert!(ResourceLedgerFile::try_lock(&path).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"preserved");
        assert!(!path.exists());
    }
    #[cfg(unix)]
    #[test]
    fn committed_ledger_and_stable_lock_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let directory = durable_test_directory();
        let path = directory.path().join("catalog.json");
        let ledger = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
        ledger.replace(b"owner-only").unwrap();
        for file in [&path, &path.with_extension("lock")] {
            assert_eq!(
                std::fs::metadata(file).unwrap().permissions().mode() & 0o077,
                0
            );
        }
    }

    #[test]
    fn relative_catalog_is_refused() {
        assert!(ResourceLedgerFile::try_lock(Path::new("relative/catalog.json")).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn final_symlink_is_refused() {
        use std::os::unix::fs::symlink;
        let directory = durable_test_directory();
        let root = directory.path();
        let source = root.join("source");
        std::fs::write(&source, b"do not change").unwrap();
        let path = root.join("catalog.json");
        symlink(&source, &path).unwrap();
        let lock = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
        assert!(lock.read_reconciled(1024).is_err());
        drop(lock);
        assert_eq!(std::fs::read(&source).unwrap(), b"do not change");
    }
}

/// Platform clock sample; the application maps it into its pure domain clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceClockSample {
    pub unix_millis: i64,
    pub monotonic_millis: u64,
    pub boot_id: String,
}
/// Birth identity is scoped by boot; a reused process number is never live proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceProcessIdentity {
    pub process_id: u32,
    pub started_at: u64,
    pub boot_id: String,
}
pub fn resource_clock() -> io::Result<ResourceClockSample> {
    #[cfg(target_os = "linux")]
    {
        let unix_millis = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_millis(),
        )
        .map_err(io::Error::other)?;
        let mut clock = std::mem::MaybeUninit::<libc::timespec>::uninit();
        // SAFETY: clock points to writable timespec storage; success initializes it.
        if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, clock.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: clock_gettime succeeded and initialized the full timespec.
        let clock = unsafe { clock.assume_init() };
        let seconds = u64::try_from(clock.tv_sec).map_err(io::Error::other)?;
        let nanos = u64::try_from(clock.tv_nsec).map_err(io::Error::other)?;
        let monotonic_millis = seconds
            .checked_mul(1000)
            .and_then(|n| n.checked_add(nanos / 1_000_000))
            .ok_or_else(|| io::Error::other("monotonic clock overflow"))?;
        let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?
            .trim()
            .to_string();
        if boot_id.is_empty() {
            return Err(io::Error::other("empty boot identity"));
        }
        Ok(ResourceClockSample {
            unix_millis,
            monotonic_millis,
            boot_id,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(unsupported())
    }
}
pub fn capture_resource_process(process_id: u32) -> io::Result<ResourceProcessIdentity> {
    #[cfg(target_os = "linux")]
    {
        let boot_id = resource_clock()?.boot_id;
        let stat = std::fs::read_to_string(format!("/proc/{process_id}/stat"))?;
        let tail = stat
            .rsplit_once(") ")
            .ok_or_else(|| io::Error::other("malformed process stat"))?
            .1;
        let mut fields = tail.split_whitespace();
        let state = fields
            .next()
            .ok_or_else(|| io::Error::other("missing process state"))?;
        if matches!(state, "Z" | "X") {
            return Err(io::Error::new(io::ErrorKind::NotFound, "process exited"));
        }
        let started_at = fields
            .nth(18)
            .ok_or_else(|| io::Error::other("missing process birth time"))?
            .parse()
            .map_err(io::Error::other)?;
        Ok(ResourceProcessIdentity {
            process_id,
            started_at,
            boot_id,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = process_id;
        Err(unsupported())
    }
}
pub fn resource_process_is_live(identity: &ResourceProcessIdentity) -> io::Result<bool> {
    match capture_resource_process(identity.process_id) {
        Ok(current) => Ok(current == *identity),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}
#[cfg(test)]
mod identity_tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn process_birth_and_boot_fence_reused_identity() {
        let mut identity = capture_resource_process(std::process::id()).unwrap();
        assert!(resource_process_is_live(&identity).unwrap());
        identity.started_at += 1;
        assert!(!resource_process_is_live(&identity).unwrap());
        identity.boot_id = "another boot".into();
        assert!(!resource_process_is_live(&identity).unwrap());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn resource_clock_is_shared_monotonic_boot_time() {
        let a = resource_clock().unwrap();
        let b = resource_clock().unwrap();
        assert!(!a.boot_id.is_empty());
        assert_eq!(a.boot_id, b.boot_id);
        assert!(b.monotonic_millis >= a.monotonic_millis);
    }
}
