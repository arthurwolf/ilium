//! Observation of the process that holds a whole-file exclusive lease.
use std::{io, path::Path};

use crate::process_control::PtyProcessIdentity;

/// Fail closed unless the birth-fenced peer owns this exact locked file.
#[cfg(target_os = "linux")]
pub fn exclusive_lock_owned_by(
    path: &Path,
    peer_process_id: u32,
    peer_identity: &PtyProcessIdentity,
) -> io::Result<bool> {
    use crate::{process_control::captured_process_is_current, secure_fs};
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;

    if !peer_identity.is_for_process(peer_process_id)
        || !captured_process_is_current(peer_identity)?
    {
        return Ok(false);
    }
    let file = secure_fs::open_private_regular_file(path, secure_fs::PrivateFileMode::Read)?;
    let metadata = file.metadata()?;
    // A partial kernel table cannot establish ownership. Keep both the read
    // and individual records bounded, even on hosts with many unrelated locks.
    const MAX_LOCK_TABLE_BYTES: u64 = 2 * 1024 * 1024;
    let mut table = String::new();
    std::fs::File::open("/proc/locks")?
        .take(MAX_LOCK_TABLE_BYTES + 1)
        .read_to_string(&mut table)?;
    if table.len() as u64 > MAX_LOCK_TABLE_BYTES || table.lines().any(|line| line.len() > 4096) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "incomplete kernel lock table",
        ));
    }
    let owned = table.lines().any(|line| {
        let mut fields = line.split_whitespace();
        let _record = fields.next();
        if fields.next() != Some("FLOCK")
            || fields.next() != Some("ADVISORY")
            || fields.next() != Some("WRITE")
            || fields.next().and_then(|value| value.parse::<u32>().ok()) != Some(peer_process_id)
        {
            return false;
        }
        let Some(device_inode) = fields.next() else {
            return false;
        };
        let mut identity = device_inode.split(':');
        let major = identity
            .next()
            .and_then(|value| u64::from_str_radix(value, 16).ok());
        let minor = identity
            .next()
            .and_then(|value| u64::from_str_radix(value, 16).ok());
        let inode = identity.next().and_then(|value| value.parse::<u64>().ok());
        identity.next().is_none()
            && major == Some(u64::from(libc::major(metadata.dev())))
            && minor == Some(u64::from(libc::minor(metadata.dev())))
            && inode == Some(metadata.ino())
            && fields.next() == Some("0")
            && fields.next() == Some("EOF")
            && fields.next().is_none()
    });
    let current = secure_fs::open_private_regular_file(path, secure_fs::PrivateFileMode::Read)?;
    Ok(owned
        && secure_fs::file_generation(&file)? == secure_fs::file_generation(&current)?
        && captured_process_is_current(peer_identity)?)
}

/// Unsupported kernels cannot supply the required ownership proof.
#[cfg(not(target_os = "linux"))]
pub fn exclusive_lock_owned_by(
    _path: &Path,
    _peer_process_id: u32,
    _peer_identity: &PtyProcessIdentity,
) -> io::Result<bool> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "kernel lock ownership unavailable",
    ))
}
