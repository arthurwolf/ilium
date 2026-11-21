//! Bounded discovery of candidate session endpoints for the polling leader.
//!
//! Registry entries are hints, not proof that a server is live or that it is
//! authorized to provide transcript evidence. The caller must authenticate
//! each endpoint and fence its current incarnation before subscribing. Never
//! send credentials to a hint before verifying its peer and ownership.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use ilium_platform::secure_fs;

const MAX_REGISTRY_ENTRIES: usize = 4096;
const MAX_ENDPOINT_BYTES: u64 = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEndpointHint {
    pub incarnation: uuid::Uuid,
    pub socket_path: PathBuf,
}

#[derive(Debug, Default)]
pub struct Discovery {
    pub endpoints: Vec<SessionEndpointHint>,
    /// Some entries could not be safely observed; absence is not proof of no sessions.
    pub incomplete: bool,
}

/// A bounded pass over the registry; endpoint hints still require authentication.
/// Repeat after exhaustion: directory changes during a pass are not a snapshot.
pub struct DiscoveryCursor {
    entries: std::fs::ReadDir,
}

impl DiscoveryCursor {
    pub fn open(directory: &Path) -> io::Result<Self> {
        Ok(Self {
            entries: std::fs::read_dir(directory)?,
        })
    }

    pub fn next_page(&mut self, maximum_entries: usize) -> io::Result<Option<Discovery>> {
        if !(1..=MAX_REGISTRY_ENTRIES).contains(&maximum_entries) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid discovery page size",
            ));
        }
        let mut result = Discovery::default();
        let mut observed_any = false;
        for _ in 0..maximum_entries {
            let Some(entry) = self.entries.next() else {
                break;
            };
            observed_any = true;
            observe_entry(entry, &mut result);
        }
        result.endpoints.sort_by_key(|entry| entry.incarnation);
        Ok(observed_any.then_some(result))
    }
}

/// Run on an admitted I/O worker, not the asynchronous server loop.
pub fn discover(directory: &Path) -> io::Result<Discovery> {
    let mut cursor = DiscoveryCursor::open(directory)?;
    let mut result = cursor.next_page(MAX_REGISTRY_ENTRIES)?.unwrap_or_default();
    if cursor.next_page(1)?.is_some() {
        result.incomplete = true;
    }
    Ok(result)
}

fn observe_entry(entry: io::Result<std::fs::DirEntry>, result: &mut Discovery) {
    let entry = match entry {
        Ok(entry) => entry,
        Err(_) => {
            result.incomplete = true;
            return;
        }
    };
    let name = entry.file_name();
    let Some(name) = name.to_str() else {
        result.incomplete = true;
        return;
    };
    let Some(identifier) = name
        .strip_prefix("server-")
        .and_then(|name| name.strip_suffix(".json"))
    else {
        return;
    };
    let Ok(incarnation) = uuid::Uuid::parse_str(identifier) else {
        result.incomplete = true;
        return;
    };
    let observed = (|| {
        let file =
            secure_fs::open_private_regular_file(&entry.path(), secure_fs::PrivateFileMode::Read)?;
        let mut bytes = Vec::new();
        file.take(MAX_ENDPOINT_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_ENDPOINT_BYTES {
            return Err(io::Error::other("session endpoint record exceeds limit"));
        }
        let socket_path: PathBuf = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if !socket_path.is_absolute() {
            return Err(io::Error::other("session endpoint must be absolute"));
        }
        Ok(SessionEndpointHint {
            incarnation,
            socket_path,
        })
    })();
    match observed {
        Ok(hint) => result.endpoints.push(hint),
        Err(_) => result.incomplete = true,
    }
}

/// An owned endpoint publication; explicit removal runs on an I/O worker.
/// Keeping the original file open prevents replacement from reusing its identity.
pub struct Publication {
    path: PathBuf,
    generation: (u64, u64),
    file: std::fs::File,
    _lease: ilium_platform::file_lock::ExclusiveFileLock,
}

impl Publication {
    pub fn remove(&self) -> io::Result<()> {
        remove_record(&self.path, self.generation)
    }
}

fn remove_record(path: &Path, generation: (u64, u64)) -> io::Result<()> {
    let current = match secure_fs::open_private_regular_file(path, secure_fs::PrivateFileMode::Read)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if secure_fs::file_generation(&current)? != generation {
        return Err(io::Error::other(
            "polling endpoint record was replaced; preserving its new owner",
        ));
    }
    std::fs::remove_file(path)
}

/// Publish only an endpoint hint, never a claim that a peer is authenticated.
/// Run on an admitted I/O worker. Failed writes remove only this owned inode.
pub fn publish(
    directory: &Path,
    incarnation: uuid::Uuid,
    socket_path: &Path,
) -> io::Result<Publication> {
    use std::io::Write;
    if incarnation.is_nil() || !socket_path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid polling endpoint identity",
        ));
    }
    let bytes = serde_json::to_vec(socket_path).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_ENDPOINT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "polling endpoint record exceeds its bound",
        ));
    }
    secure_fs::create_private_directory(directory)?;
    let path = directory.join(format!("server-{incarnation}.json"));
    let file = secure_fs::open_private_regular_file(&path, secure_fs::PrivateFileMode::CreateNew)?;
    let generation = secure_fs::file_generation(&file)?;
    // The peer must prove ownership, not just readability of a stale hint.
    // Lock the admitted inode before publishing its endpoint contents.
    let lease = match file.try_clone().and_then(|opened| {
        ilium_platform::file_lock::ExclusiveFileLock::try_acquire_opened(opened)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "polling publication lease is held",
            )
        })
    }) {
        Ok(lease) => lease,
        Err(error) => {
            return match remove_record(&path, generation) {
                Ok(()) => Err(error),
                Err(cleanup) => Err(io::Error::other(format!(
                    "polling publication lease failed: {error}; owned record cleanup failed: {cleanup}"
                ))),
            };
        }
    };
    let mut publication = Publication {
        path,
        generation,
        file,
        _lease: lease,
    };
    if let Err(error) = publication
        .file
        .write_all(&bytes)
        .and_then(|()| publication.file.sync_all())
    {
        return match publication.remove() {
            Ok(()) => Err(error),
            Err(cleanup) => Err(io::Error::other(format!(
                "polling endpoint publication failed: {error}; owned record cleanup failed: {cleanup}"
            ))),
        };
    }
    Ok(publication)
}
