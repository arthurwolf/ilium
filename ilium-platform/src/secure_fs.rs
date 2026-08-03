//! Directories and files that only their owner may read.
//!
//! ilium writes three kinds of user-private data: debug logs (which contain
//! terminal contents), session snapshots (which contain the workspace tree and
//! pane titles), and lock files under a shared temporary directory. All three
//! want the same guarantee, so the guarantee lives here once.
//!
//! On Unix that guarantee is explicit: mode `0o700` on directories, `0o600` on
//! files, `O_NOFOLLOW` so a pre-planted symlink in a world-writable directory
//! cannot redirect a write, and `O_CLOEXEC` so a descriptor never leaks into a
//! spawned agent CLI.
//!
//! On Windows it is inherited: the paths involved live under the user's own
//! profile (see [`crate::runtime_dir`]), whose ACL already denies other
//! non-administrative users. Windows has no `chmod` equivalent that maps onto
//! those mode bits, and rewriting a DACL per file would be both slower and
//! easier to get wrong than relying on the profile's inherited permissions.

use std::fs::OpenOptions;
use std::io;
use std::path::Path;

/// Creates `path` and any missing parents, restricted to the current user.
///
/// Re-running this on an existing directory re-applies the restriction, which
/// matters because the directory may have been created by an older build (or
/// by a user's own `mkdir`) with looser permissions.
pub fn create_private_directory(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    restrict_directory_to_owner(path)
}

/// Restricts an existing directory to the current user.
#[cfg(unix)]
pub fn restrict_directory_to_owner(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

/// Windows directories under the user profile are already owner-private; see
/// the module comment for why no explicit ACL edit happens here.
#[cfg(not(unix))]
pub fn restrict_directory_to_owner(path: &Path) -> io::Result<()> {
    let _ = path;
    Ok(())
}

/// Restricts an existing file to the current user.
///
/// Callers use this after a create-then-rename sequence, where the final path
/// did not exist at open time and so could not be opened privately.
#[cfg(unix)]
pub fn restrict_file_to_owner(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

/// See [`restrict_directory_to_owner`] for why Windows relies on inheritance.
#[cfg(not(unix))]
pub fn restrict_file_to_owner(path: &Path) -> io::Result<()> {
    let _ = path;
    Ok(())
}

/// Open options that produce an owner-only file, with the access mode left to
/// the caller: appending for a log, read-write for a lock, create-new for a
/// snapshot's temporary file.
///
/// Returning the builder rather than an opened file keeps every caller's
/// intent visible at its own call site while the privacy decision stays here.
#[cfg(unix)]
pub fn private_open_options() -> OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    // `O_NOFOLLOW` refuses a symlink at the final path component, which is the
    // attack a world-writable parent directory invites. `O_CLOEXEC` keeps the
    // descriptor out of spawned agent CLIs.
    options
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    options
}

/// Windows has no `O_NOFOLLOW`/`O_CLOEXEC` equivalent to set here: handles are
/// non-inheritable by default, and reparse-point traversal is governed by the
/// directory's own ACL rather than a per-open flag.
#[cfg(not(unix))]
pub fn private_open_options() -> OpenOptions {
    OpenOptions::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_private_directory_is_idempotent_and_restricts_an_existing_directory() {
        let root = tempfile::tempdir().expect("temp dir");
        let nested = root.path().join("outer").join("inner");

        create_private_directory(&nested).expect("first creation");
        // A second call must succeed on the already-created path, because the
        // session resolver runs it on every attach, not only the first.
        create_private_directory(&nested).expect("second creation");

        assert!(nested.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&nested)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700);
        }
    }

    #[test]
    fn private_open_options_creates_an_owner_only_file() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("private.log");

        let file = private_open_options()
            .create(true)
            .append(true)
            .open(&path)
            .expect("open private file");
        drop(file);

        assert!(path.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn restrict_file_to_owner_tightens_a_file_created_by_a_rename() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("renamed.json");
        std::fs::write(&path, b"{}").expect("write file");

        restrict_file_to_owner(&path).expect("restrict");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
