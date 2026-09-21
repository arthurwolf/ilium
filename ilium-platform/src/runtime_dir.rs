//! Where ilium puts per-user runtime state: session sockets and debug logs.
//!
//! The hard constraint here is socket path length. A Unix-domain socket
//! address is a fixed-size `sockaddr_un` whose `sun_path` is 108 bytes on
//! Linux but only 104 on macOS and the BSDs, and the kernel truncates or
//! rejects rather than growing it. That budget is small enough that the
//! *directory* choice alone can exhaust it: macOS hands every process a
//! per-session temporary directory like
//! `/var/folders/df/djsxfhc17x95674wsm_g8s980000gn/T/`, which spends half the
//! address before the file name starts.
//!
//! So the socket directory is chosen to be short on every platform rather than
//! merely conventional, and [`MAX_SOCKET_PATH_BYTES`] is published so callers
//! can budget the file name against the directory they actually got.

use std::io;
use std::path::PathBuf;

use crate::secure_fs;

/// The longest a session socket path may be, in bytes, including the
/// terminating NUL the kernel requires.
///
/// This is the smallest limit across supported platforms (macOS's 104-byte
/// `sun_path`) minus a small margin, so one number is correct everywhere
/// instead of a per-platform value callers would have to reason about.
pub const MAX_SOCKET_PATH_BYTES: usize = 100;

/// Directory holding one socket per project session, created private.
///
/// Linux uses `$XDG_RUNTIME_DIR` when the session manager provides one: it is
/// short, already per-user, and cleared at logout. macOS has no such thing and
/// its temp directory is far too long, so a short well-known path under `/tmp`
/// is used instead, suffixed with the user id so two users on one machine keep
/// separate, mutually inaccessible directories. Windows does not put named
/// pipes on the filesystem at all, but the same directory still holds the
/// per-session lock and marker files, so it resolves under the user's own
/// local application data.
pub fn session_socket_directory() -> io::Result<PathBuf> {
    let directory = match directory_override(SOCKET_DIR_ENV)? {
        Some(directory) => directory,
        None => socket_directory_path(),
    };
    secure_fs::create_private_directory(&directory)?;
    Ok(directory)
}

/// Environment override for the session socket directory.
///
/// The same lever [`DEBUG_LOG_DIR_ENV`] provides for logs, and it exists for
/// the same reason: a test needs its sessions kept away from the real user's.
///
/// Honoured on *every* platform, which `XDG_RUNTIME_DIR` is not -- Windows
/// resolves this directory from `%LOCALAPPDATA%` and never looked at the XDG
/// variable, so a test isolating itself that way was isolated on Unix and
/// sharing the real user's directory on Windows.
///
/// A relative value is resolved to an absolute path rather than kept relative;
/// see [`directory_override`] for why a still-relative one names two different
/// sockets.
pub const SOCKET_DIR_ENV: &str = "ILIUM_SOCKET_DIR";

/// Reads one of this module's environment overrides, resolved to an absolute
/// directory, or `None` when it is unset or empty.
///
/// The absolute part is the point. A path that is still relative means
/// "relative to whoever reads it", and the two processes that read these
/// directories do not share a working directory: the CLI resolves the socket
/// and log paths from *its* directory, then spawns the detached server with
/// `current_dir` set to the project root. A relative override therefore has
/// the client connecting to one socket while the server binds another (and the
/// server writing its log where the client never looks), with no error
/// anywhere -- the same failure the `XDG_RUNTIME_DIR` arm avoids by treating a
/// relative value as unset. Falling back is right for a variable ilium does not
/// own and whose spec mandates it; for ilium's own override, resolving the path
/// the caller asked for is both what they meant and unambiguous afterwards.
fn directory_override(variable: &str) -> io::Result<Option<PathBuf>> {
    resolve_directory_override(variable, std::env::var_os(variable))
}

/// Split from the caller so the resolution can be tested without mutating the
/// environment, which is process-global and cannot be done safely while other
/// tests run in parallel.
fn resolve_directory_override(
    variable: &str,
    value: Option<std::ffi::OsString>,
) -> io::Result<Option<PathBuf>> {
    // An empty value is treated as unset, matching how an unset variable and a
    // variable exported as `""` are interchangeable in every shell.
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let directory = PathBuf::from(value);
    if directory.is_absolute() {
        return Ok(Some(directory));
    }

    // `std::path::absolute` is purely lexical: it prepends the current
    // directory without resolving symlinks or touching the filesystem, so it
    // cannot fail on a directory that does not exist yet -- which this one
    // usually does not, since the caller is about to create it. Its only
    // failure is an unavailable current directory, and naming the variable
    // keeps that from surfacing as a bare "No such file or directory" with no
    // hint of which override produced it.
    std::path::absolute(&directory).map(Some).map_err(|source| {
        io::Error::new(
            source.kind(),
            format!(
                "{variable}={} could not be resolved to an absolute path: {source}",
                directory.display()
            ),
        )
    })
}

/// `XDG_RUNTIME_DIR` wins wherever it is set, on every Unix.
///
/// On Linux that is the session manager's own short, per-user, logout-cleared
/// directory. macOS has no such convention, but honouring the variable there
/// too is what lets a caller point one ilium session at an isolated directory
/// -- which the integration tests rely on, and which silently failed while
/// this arm ignored the variable.
///
/// The fallback is deliberately neither `BaseDirs::runtime_dir` nor
/// `std::env::temp_dir()`: on macOS both resolve to the long per-session
/// `/var/folders/...` path that overflows `sun_path`. See the module comment.
#[cfg(unix)]
fn socket_directory_path() -> PathBuf {
    socket_directory_for_runtime_dir(std::env::var_os("XDG_RUNTIME_DIR"))
}

/// Split from the caller so the choice can be tested without mutating the
/// environment, which is process-global and cannot be done safely while other
/// tests run in parallel.
#[cfg(unix)]
fn socket_directory_for_runtime_dir(runtime_dir: Option<std::ffi::OsString>) -> PathBuf {
    runtime_dir
        .map(PathBuf::from)
        // The XDG spec requires the runtime directory to be an absolute path
        // and says a relative value must be treated as unset. Honouring a
        // relative one would resolve it against the process's current
        // directory, planting the socket wherever ilium happened to be
        // launched from. An empty value is not absolute either, so this one
        // check covers both invalid shapes.
        .filter(|runtime_dir| runtime_dir.is_absolute())
        .map(|runtime_dir| runtime_dir.join("ilium"))
        .unwrap_or_else(short_shared_directory)
}

#[cfg(windows)]
fn socket_directory_path() -> PathBuf {
    use directories::BaseDirs;

    BaseDirs::new()
        .map(|base_dirs| base_dirs.data_local_dir().join("ilium").join("run"))
        .unwrap_or_else(|| std::env::temp_dir().join("ilium"))
}

/// A short, per-user directory under the shared temporary root.
///
/// The user id suffix is what makes this safe to place in a world-writable
/// directory: each user gets a distinct path, and [`secure_fs`] creates it
/// `0o700` so no one else can enter it or plant symlinks inside.
#[cfg(unix)]
fn short_shared_directory() -> PathBuf {
    // SAFETY: `getuid` takes no arguments, touches no process memory, and is
    // documented as always succeeding.
    let user_id = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/.ilium-{user_id}"))
}

/// Environment override for the debug log root.
///
/// The default root is shared by every project on the machine, which is right
/// for a real user -- one place to look, one place to clear -- and wrong for
/// anything that needs to observe only its own logs. Tests are the obvious
/// case: they scan this root for the session they just started, and a root
/// holding hundreds of directories from previous runs makes that scan find
/// somebody else's.
///
/// A relative value is resolved to an absolute path rather than kept relative;
/// see [`directory_override`].
pub const DEBUG_LOG_DIR_ENV: &str = "ILIUM_DEBUG_LOG_DIR";

/// Root directory for timestamped debug logs, created private.
///
/// Logs contain terminal contents, so this is owner-only for the same reason
/// the socket directory is. On Unix it deliberately shares the short per-user
/// path: a long log path costs nothing, but keeping one root keeps cleanup
/// (and a user's own `rm -rf`) to a single place -- unless
/// [`DEBUG_LOG_DIR_ENV`] names one explicitly.
pub fn debug_log_root() -> io::Result<PathBuf> {
    let directory = match directory_override(DEBUG_LOG_DIR_ENV)? {
        Some(directory) => directory,
        None => debug_log_root_path()?,
    };
    secure_fs::create_private_directory(&directory)?;
    Ok(directory)
}

/// Restricts the shared per-user root before joining `logs` onto it.
///
/// [`session_socket_directory`] is what normally locks [`short_shared_directory`]
/// down to `0o700`, but the two resolvers run independently, and whenever
/// `XDG_RUNTIME_DIR` is set the socket path never touches this shared root at
/// all (it resolves under `XDG_RUNTIME_DIR` instead). Without this call the
/// root that `create_dir_all` creates on the way to `logs` would keep
/// whatever default, world-readable mode `mkdir` gave it -- contradicting
/// [`short_shared_directory`]'s own documented guarantee that no one else can
/// enter it.
#[cfg(unix)]
fn debug_log_root_path() -> io::Result<PathBuf> {
    let shared_root = short_shared_directory();
    secure_fs::create_private_directory(&shared_root)?;
    Ok(shared_root.join("logs"))
}

#[cfg(windows)]
fn debug_log_root_path() -> io::Result<PathBuf> {
    use directories::BaseDirs;

    let root = BaseDirs::new()
        .map(|base_dirs| base_dirs.data_local_dir().join("ilium").join("logs"))
        .unwrap_or_else(|| std::env::temp_dir().join("ilium").join("logs"));
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the tests below that create or inspect permissions on the
    /// real `short_shared_directory()` path: it is a single deterministic
    /// location per user, shared by every test in this module that doesn't
    /// set an environment override, so mutating its mode from one test could
    /// otherwise be observed mid-flight by another running in parallel.
    static SHARED_DIRECTORY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn the_socket_directory_leaves_room_for_a_session_file_name() {
        // `session_socket_directory` creates and restricts the real shared
        // directory when no runtime directory is set, so this test must hold
        // the same lock as the other tests touching that path -- otherwise it
        // can re-restrict the shared parent mid-flight and let the
        // loosen-then-restrict regression test below pass without actually
        // exercising the restriction.
        let _guard = SHARED_DIRECTORY_LOCK.lock().expect("lock poisoned");
        let directory = session_socket_directory().expect("socket directory");

        // The whole point of this module: whatever directory a platform
        // resolves to must leave a usable file-name budget, not merely exist.
        // 40 bytes is enough for a digest, a session name, and `.sock`.
        let remaining = MAX_SOCKET_PATH_BYTES.saturating_sub(directory.as_os_str().len());
        assert!(
            remaining >= 40,
            "socket directory {directory:?} leaves only {remaining} bytes for a file name"
        );
    }

    /// Honouring `XDG_RUNTIME_DIR` is what lets a caller isolate one session,
    /// which the integration tests depend on. It silently did not happen on
    /// macOS while that arm resolved a fixed path instead.
    #[cfg(unix)]
    #[test]
    fn an_explicit_runtime_directory_is_honoured() {
        let requested = std::ffi::OsString::from("/tmp/ilium-runtime-test");

        let resolved = socket_directory_for_runtime_dir(Some(requested));

        assert_eq!(resolved, PathBuf::from("/tmp/ilium-runtime-test/ilium"));
    }

    /// An unset, empty, or relative value must fall back rather than
    /// resolving something relative to the current directory -- the XDG spec
    /// requires the runtime directory to be absolute and says an invalid
    /// value is to be treated as unset.
    #[cfg(unix)]
    #[test]
    fn an_absent_empty_or_relative_runtime_directory_falls_back_to_the_short_shared_path() {
        let fallback = short_shared_directory();

        assert_eq!(socket_directory_for_runtime_dir(None), fallback);
        assert_eq!(
            socket_directory_for_runtime_dir(Some(std::ffi::OsString::new())),
            fallback
        );
        assert_eq!(
            socket_directory_for_runtime_dir(Some(std::ffi::OsString::from("relative/run"))),
            fallback
        );
    }

    #[test]
    fn the_socket_directory_is_created_and_private() {
        let _guard = SHARED_DIRECTORY_LOCK.lock().expect("lock poisoned");
        let directory = session_socket_directory().expect("socket directory");

        assert!(directory.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&directory)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700);
        }
    }

    #[test]
    fn the_debug_log_root_is_created_and_private() {
        let _guard = SHARED_DIRECTORY_LOCK.lock().expect("lock poisoned");
        let directory = debug_log_root().expect("log root");

        assert!(directory.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&directory)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700);
        }
    }

    /// Regression test for the bug this module's `debug_log_root_path` fix
    /// closes: `create_dir_all` on the way to `logs` leaves the shared parent
    /// at whatever mode `mkdir` chose, and nothing else in the log-only path
    /// ever restricted that parent when `XDG_RUNTIME_DIR` sends the socket
    /// directory elsewhere. This loosens the parent first so the assertion
    /// actually exercises the restriction rather than observing a mode some
    /// earlier test call already left behind.
    ///
    /// The parent is resolved through [`short_shared_directory`] rather than
    /// from what [`debug_log_root`] returns, because this test *chmods* what it
    /// finds: going through the public function would let a
    /// [`DEBUG_LOG_DIR_ENV`] set in the ambient environment aim a
    /// permission change at some unrelated directory's parent.
    #[cfg(unix)]
    #[test]
    fn the_debug_log_root_restricts_the_shared_parent_even_if_left_loose() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = SHARED_DIRECTORY_LOCK.lock().expect("lock poisoned");
        let parent = short_shared_directory();
        secure_fs::create_private_directory(&parent).expect("create shared parent");
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755))
            .expect("loosen shared parent");

        let logs = debug_log_root_path().expect("log root path");

        assert_eq!(logs.parent(), Some(parent.as_path()));
        let mode = std::fs::metadata(&parent)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode, 0o700,
            "the log root resolver must re-restrict the shared parent, not just the logs leaf"
        );
    }

    /// A relative override must not stay relative: the CLI resolves these
    /// directories from its own working directory and then spawns the server
    /// with a different one, so a path meaning "relative to here" would name
    /// two different sockets and two different log roots.
    #[test]
    fn a_relative_override_is_resolved_against_the_current_directory() {
        let relative = Some(std::ffi::OsString::from("relative/run"));

        let resolved = resolve_directory_override(SOCKET_DIR_ENV, relative)
            .expect("resolve override")
            .expect("override present");

        assert!(
            resolved.is_absolute(),
            "a relative override must be made absolute, got {resolved:?}"
        );
        assert!(resolved.ends_with("relative/run"), "got {resolved:?}");
    }

    /// An absolute override is handed back exactly as given -- resolution must
    /// not normalise away a component the user deliberately pointed at.
    #[test]
    fn an_absolute_override_is_used_verbatim() {
        let absolute = Some(std::ffi::OsString::from(ABSOLUTE_OVERRIDE));

        let resolved =
            resolve_directory_override(DEBUG_LOG_DIR_ENV, absolute).expect("resolve override");

        assert_eq!(resolved, Some(PathBuf::from(ABSOLUTE_OVERRIDE)));
    }

    /// An override exported as `""` is the shell's way of saying "unset", and
    /// treating it as a path would resolve the current directory itself.
    #[test]
    fn an_unset_or_empty_override_resolves_to_nothing() {
        let unset = resolve_directory_override(SOCKET_DIR_ENV, None).expect("unset override");
        let empty = resolve_directory_override(SOCKET_DIR_ENV, Some(std::ffi::OsString::new()))
            .expect("empty override");

        assert_eq!(unset, None);
        assert_eq!(empty, None);
    }

    /// Absolute on both families, so the two tests above assert the same thing
    /// everywhere instead of a Unix-shaped path that Windows calls relative.
    #[cfg(unix)]
    const ABSOLUTE_OVERRIDE: &str = "/tmp/ilium-override-test";
    #[cfg(windows)]
    const ABSOLUTE_OVERRIDE: &str = r"C:\ilium-override-test";
}
