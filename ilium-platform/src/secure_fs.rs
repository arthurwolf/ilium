//! Directories and files that only their owner may read.
//!
//! ilium writes three kinds of user-private data: debug logs (which contain
//! terminal contents), session snapshots (which contain the workspace tree and
//! pane titles), and lock files under a shared temporary directory. All three
//! want the same guarantee, so the guarantee lives here once.
//!
//! On Unix that guarantee is explicit: mode `0o700` on directories, `0o600` on
//! files, `O_NOFOLLOW` so a pre-planted symlink in a world-writable directory
//! cannot redirect a write, `O_CLOEXEC` so a descriptor never leaks into a
//! spawned agent CLI, and an `lstat`-based check ahead of every `chmod` so a
//! symlink planted at the path itself (rather than encountered while opening
//! a file through it) is refused instead of silently chmod'd through.
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
/// Every directory this creates -- the leaf *and* each intermediate parent --
/// is owner-only from the moment `mkdir` returns, never for a window
/// afterwards. Re-running this on an existing directory re-applies the
/// restriction to the leaf, which matters because the directory may have been
/// created by an older build (or by a user's own `mkdir`) with looser
/// permissions.
pub fn create_private_directory(path: &Path) -> io::Result<()> {
    create_directory_tree_privately(path)?;
    restrict_directory_to_owner(path)
}

/// Creates `path` and any missing parents, passing the owner-only mode to
/// `mkdir` itself rather than chmod'ing afterwards.
///
/// Two things go wrong when the tree is created with a plain
/// `std::fs::create_dir_all` and only the leaf is chmod'd afterwards. The leaf
/// exists at the process umask's default mode (typically `0o755`) for the
/// whole window between `mkdir` and that `chmod`, so anyone can read it -- or,
/// in a world-writable parent, plant files inside it -- for as long as the
/// window lasts. And the intermediate parents are never narrowed at all: a
/// project's `<project>/.ilium` on the way to `.ilium/sessions`, or the
/// per-user root on the way to its `logs` subdirectory, stayed world-listable
/// forever. Handing the mode to `mkdir` fixes both: the mode applies to every
/// component this call creates, and it applies atomically at creation.
///
/// `umask` can only clear bits, so the result is never wider than `0o700`.
#[cfg(unix)]
fn create_directory_tree_privately(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

/// Windows has no mode argument to hand `mkdir`: directories created here
/// live under the user's own profile and inherit its ACL, which is what makes
/// them private in the first place. See the module comment.
#[cfg(not(unix))]
fn create_directory_tree_privately(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Restricts an existing directory to the current user.
#[cfg(unix)]
pub fn restrict_directory_to_owner(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    refuse_pre_existing_symlink(path)?;
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

    refuse_pre_existing_symlink(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

/// Refuses to act on a symlink already sitting at `path`.
///
/// `std::fs::set_permissions` follows symlinks, and so does the recursive
/// directory creation's "does this already exist" check (it falls
/// back to `path.is_dir()`, which resolves through a symlink and reports
/// success without creating anything). Together those two facts mean a
/// symlink pre-planted at a deterministic path in a world-writable directory
/// -- exactly what [`crate::runtime_dir::short_shared_directory`] resolves
/// under `/tmp` -- would make `create_private_directory` silently treat the
/// symlink's target as already private, then chmod that target instead of
/// failing. This lstat-based check sees the symlink itself and refuses
/// before `set_permissions` can be tricked into following it.
#[cfg(unix)]
fn refuse_pre_existing_symlink(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::other(format!(
            "refusing to follow pre-existing symlink at {}",
            path.display()
        ))),
        // A regular file or directory at the path is what the caller expects
        // to restrict.
        Ok(_) => Ok(()),

        // Nothing at the path is fine too: the caller may be about to create
        // it, and the follow-up operation produces the natural error if not.
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),

        // Any other lstat failure (EACCES, ELOOP, EIO, ...) means the symlink
        // question could not be answered at all -- propagate rather than
        // report "safe" for a path this guard never actually inspected.
        Err(error) => Err(error),
    }
}

/// See [`restrict_directory_to_owner`] for why Windows relies on inheritance.
#[cfg(not(unix))]
pub fn restrict_file_to_owner(path: &Path) -> io::Result<()> {
    let _ = path;
    Ok(())
}

/// Restricts an already-open file to the current user through its handle.
///
/// Operating on the descriptor (`fchmod` underneath) instead of the path
/// closes the race that [`restrict_file_to_owner`] cannot: nothing swapped in
/// at the path after the open can redirect this call, so callers that hold
/// the handle should always prefer this variant.
#[cfg(unix)]
pub fn restrict_open_file_to_owner(file: &std::fs::File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(std::fs::Permissions::from_mode(0o600))
}

/// See [`restrict_directory_to_owner`] for why Windows relies on inheritance.
#[cfg(not(unix))]
pub fn restrict_open_file_to_owner(file: &std::fs::File) -> io::Result<()> {
    let _ = file;
    Ok(())
}

/// Restricts an existing file to the current user, with the owner's execute
/// bit set.
///
/// For a copied executable (a test fixture binary, an installed helper), not
/// a data file: use [`restrict_file_to_owner`] for anything that should not
/// run.
#[cfg(unix)]
pub fn restrict_executable_file_to_owner(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    refuse_pre_existing_symlink(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

/// Windows derives executability from the file extension rather than a
/// permission bit; see the module comment for why no explicit ACL edit
/// happens here either.
#[cfg(not(unix))]
pub fn restrict_executable_file_to_owner(path: &Path) -> io::Result<()> {
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

    /// Regression test: only the leaf used to be restricted, so the
    /// intermediate directories `create_dir_all` made on the way to it -- a
    /// project's `.ilium` on the way to `.ilium/sessions`, the per-user root
    /// on the way to its `logs` -- kept whatever world-listable mode the
    /// umask gave them, exposing session and log *names* to every other user
    /// on the machine even though the files inside were `0o600`.
    #[cfg(unix)]
    #[test]
    fn create_private_directory_restricts_the_parents_it_creates_too() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temp dir");
        let outer = root.path().join("outer");
        let nested = outer.join("inner");

        create_private_directory(&nested).expect("creation");

        let outer_mode = std::fs::metadata(&outer)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            outer_mode, 0o700,
            "an intermediate directory created on the way to the leaf must be owner-only too"
        );
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

    /// Reproduces the attack the module doc calls out: a symlink pre-planted
    /// at ilium's deterministic path in a world-writable directory, pointing
    /// at a directory the attacker controls. `create_dir_all` alone would
    /// silently treat the symlink's target as "already exists" and
    /// `set_permissions` would chmod that target -- this asserts both that
    /// the call is refused and that the decoy's own permissions are left
    /// untouched, since a refusal that still mutated the target would not
    /// actually close the hole.
    #[cfg(unix)]
    #[test]
    fn create_private_directory_refuses_a_pre_planted_symlink() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temp dir");
        let decoy = root.path().join("attacker-owned");
        std::fs::create_dir(&decoy).expect("create decoy");
        std::fs::set_permissions(&decoy, std::fs::Permissions::from_mode(0o755))
            .expect("set decoy permissions");
        let victim_path = root.path().join("ilium-socket-dir");
        std::os::unix::fs::symlink(&decoy, &victim_path).expect("plant symlink");

        let result = create_private_directory(&victim_path);

        assert!(result.is_err(), "a pre-planted symlink must be refused");
        let decoy_mode = std::fs::metadata(&decoy)
            .expect("decoy metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            decoy_mode, 0o755,
            "the decoy directory must not be chmod'd through the symlink"
        );
    }
}
