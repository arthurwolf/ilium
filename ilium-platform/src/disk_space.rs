//! How much space is still free on the filesystem that holds a path, asked
//! before a large file is written there so the writer can refuse up front
//! instead of failing halfway with a partial file to clean up.
//!
//! Unix uses `statvfs` (the unprivileged `f_bavail` figure, the same one
//! `df` prints as "Avail"); Windows uses `GetDiskFreeSpaceExW`, which likewise
//! reports the bytes available to the calling user after quotas.

use std::io;
use std::path::Path;

/// Bytes the current user may still write on the filesystem holding `path`.
///
/// `path` need not exist yet: the question is asked of its nearest existing
/// ancestor, which is the filesystem the path would be created on. A path
/// without any existing ancestor is reported as `NotFound`.
pub fn available_bytes(path: &Path) -> io::Result<u64> {
    let mut candidate = path;
    while !candidate.exists() {
        candidate = candidate.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no existing ancestor of {}", path.display()),
            )
        })?;
    }
    available_bytes_of_existing(candidate)
}

#[cfg(unix)]
fn available_bytes_of_existing(path: &Path) -> io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;

    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))?;
    let mut filesystem = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `c_path` is a valid NUL-terminated string and `filesystem` is a
    // correctly sized output that the call fully initialises on success.
    if unsafe { libc::statvfs(c_path.as_ptr(), filesystem.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a zero return means the structure was written in full.
    let filesystem = unsafe { filesystem.assume_init() };
    // `f_bavail` is in units of the fragment size on every supported libc.
    Ok(widen(filesystem.f_bavail).saturating_mul(widen(filesystem.f_frsize)))
}

/// The `statvfs` fields are 32-bit on some platforms and 64-bit on others; a
/// generic widening keeps the arithmetic in `u64` everywhere without a cast
/// that is a no-op on one platform and required on another.
#[cfg(unix)]
fn widen(value: impl Into<u64>) -> u64 {
    value.into()
}

#[cfg(windows)]
fn available_bytes_of_existing(path: &Path) -> io::Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut available_to_caller: u64 = 0;
    // SAFETY: `wide` is NUL-terminated and outlives the call; the output
    // pointer is valid and the other two outputs are optional (null).
    let succeeded = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available_to_caller,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if succeeded == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(available_to_caller)
}

#[cfg(not(any(unix, windows)))]
fn available_bytes_of_existing(_path: &Path) -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "free disk space cannot be queried on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::available_bytes;

    #[test]
    fn a_missing_leaf_is_answered_by_its_existing_ancestor() {
        let temporary = tempfile::tempdir().unwrap();
        let existing = available_bytes(temporary.path()).unwrap();
        let future = available_bytes(&temporary.path().join("not").join("yet")).unwrap();
        // Both ask the same filesystem; other activity may move the figure a
        // little between the two calls, but not by orders of magnitude.
        let larger = existing.max(future);
        let smaller = existing.min(future);
        assert!(larger - smaller < larger / 2 + 1, "{existing} vs {future}");
    }

    #[test]
    fn a_path_with_no_existing_ancestor_is_not_found() {
        let error = available_bytes(std::path::Path::new("")).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }
}
