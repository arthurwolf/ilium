//! Where the scan reads transcripts from.
//!
//! The scan never opens a file itself: it asks a [`TranscriptFs`], which the plugin implements over the host `fs`
//! import ([`HostTranscriptFs`], read-only scopes `agent-sessions:<agent>` and `agent-config:codex`) and the tests
//! implement over a real temporary directory. A path is a scope name plus a `/`-separated path inside it.

use std::io::{self, Read};

use ilium_plugin_sdk::host;
use ilium_plugin_sdk::host::{EntryKind, HostFs};
use ilium_plugin_sdk::types::{ErrorCode, FsPath, PluginError};

/// The largest read the host answers in one call (`fs.read`, 1 MiB).
const HOST_READ_BYTES: usize = 1024 * 1024;
/// The most entries one `list-dir` call returns (the host limit).
const HOST_LIST_ENTRIES: u32 = 4_096;

/// A scope of the host `fs` import and a path inside it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScanPath {
    pub scope: String,
    pub path: String,
}

impl ScanPath {
    pub fn new(scope: &str, path: &str) -> Self {
        Self {
            scope: scope.to_string(),
            path: path.to_string(),
        }
    }

    /// This path with `name` appended.
    pub fn child(&self, name: &str) -> Self {
        let path = if self.path.is_empty() {
            name.to_string()
        } else {
            format!("{}/{name}", self.path)
        };
        Self {
            scope: self.scope.clone(),
            path,
        }
    }

    /// The last path segment.
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or("")
    }

    /// `scope:path`, for warnings the user reads.
    pub fn display(&self) -> String {
        format!("{}:{}", self.scope, self.path)
    }

    /// The segments of the path.
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.path.split('/').filter(|part| !part.is_empty())
    }
}

/// What the scan needs to know about one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryInfo {
    pub is_dir: bool,
    pub is_file: bool,
    pub length: u64,
    /// Modification time in milliseconds since the Unix epoch, when the file system reports one.
    pub modified_ms: Option<i64>,
}

/// One entry of a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirItem {
    pub name: String,
    pub info: EntryInfo,
}

/// Read-only access to the transcript folders.
pub trait TranscriptFs {
    /// The entries of a directory; an error when it does not exist.
    fn list_dir(&self, path: &ScanPath) -> io::Result<Vec<DirItem>>;
    /// The entry itself; an error when it does not exist.
    fn metadata(&self, path: &ScanPath) -> io::Result<EntryInfo>;
    /// Up to `max` bytes at `offset`; an empty answer is the end of the file.
    fn read_at(&self, path: &ScanPath, offset: u64, max: usize) -> io::Result<Vec<u8>>;
}

/// A `Read` over the first `end` bytes of a file, one `read_at` call per `read`.
pub struct RangeReader<'a> {
    fs: &'a dyn TranscriptFs,
    path: &'a ScanPath,
    offset: u64,
    end: u64,
}

impl<'a> RangeReader<'a> {
    /// Reads `path` from its start up to `end` bytes.
    pub fn new(fs: &'a dyn TranscriptFs, path: &'a ScanPath, end: u64) -> Self {
        Self {
            fs,
            path,
            offset: 0,
            end,
        }
    }
}

impl Read for RangeReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = self.end.saturating_sub(self.offset);
        let wanted = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        if wanted == 0 {
            return Ok(0);
        }
        let chunk = self.fs.read_at(self.path, self.offset, wanted)?;
        let count = chunk.len().min(wanted);
        buffer[..count].copy_from_slice(&chunk[..count]);
        self.offset += count as u64;
        Ok(count)
    }
}

fn io_error(error: &PluginError) -> io::Error {
    let kind = match error.code {
        ErrorCode::NotFound => io::ErrorKind::NotFound,
        ErrorCode::NotPermitted => io::ErrorKind::PermissionDenied,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, error.message.clone())
}

fn host_path(path: &ScanPath) -> FsPath {
    FsPath {
        scope: path.scope.clone(),
        path: path.path.clone(),
    }
}

fn entry_info(metadata: &ilium_plugin_sdk::host::FsMetadata) -> EntryInfo {
    EntryInfo {
        is_dir: metadata.kind == EntryKind::Directory,
        is_file: metadata.kind == EntryKind::File,
        length: metadata.length,
        modified_ms: metadata
            .modified_unix_millis
            .map(|millis| i64::try_from(millis).unwrap_or(i64::MAX)),
    }
}

/// [`TranscriptFs`] over the host `fs` import. Symbolic links are neither files nor directories here, so the walk
/// never follows them.
#[derive(Debug, Default, Clone, Copy)]
pub struct HostTranscriptFs;

impl TranscriptFs for HostTranscriptFs {
    fn list_dir(&self, path: &ScanPath) -> io::Result<Vec<DirItem>> {
        let mut api = host();
        HostFs::list_dir(&mut *api, host_path(path), HOST_LIST_ENTRIES)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| DirItem {
                        info: entry_info(&entry.metadata),
                        name: entry.name,
                    })
                    .collect()
            })
            .map_err(|error| io_error(&error))
    }

    fn metadata(&self, path: &ScanPath) -> io::Result<EntryInfo> {
        let mut api = host();
        HostFs::metadata(&mut *api, host_path(path))
            .map(|metadata| entry_info(&metadata))
            .map_err(|error| io_error(&error))
    }

    fn read_at(&self, path: &ScanPath, offset: u64, max: usize) -> io::Result<Vec<u8>> {
        let mut api = host();
        let wanted = u32::try_from(max.min(HOST_READ_BYTES)).unwrap_or(u32::MAX);
        HostFs::read(&mut *api, host_path(path), offset, wanted).map_err(|error| io_error(&error))
    }
}
