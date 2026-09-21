//! Handing a URL or filesystem path to the operating system's own default
//! handler -- the browser for a URL, the registered application for a file,
//! the file manager for a folder. Every code path spawns through an argument
//! array or `ShellExecuteW`'s own parameters, never through a shell, so a
//! metacharacter in a URL or path (this crate's callers feed it terminal and
//! editor text, which can be attacker- or agent-influenced) is never
//! interpreted as a shell operator.
//!
//! Callers must restrict a URL to an allowed scheme before calling
//! [`open_url`] -- this module does not re-validate it, matching every other
//! primitive in this crate that trusts its caller to enforce policy before
//! reaching the OS boundary.

use std::io;
use std::path::Path;

/// Hands `url` to the user's default browser.
#[cfg(unix)]
pub fn open_url(url: &str) -> io::Result<()> {
    spawn_and_release(OPEN_COMMAND, std::ffi::OsStr::new(url))
}

#[cfg(windows)]
pub fn open_url(url: &str) -> io::Result<()> {
    shell_execute_open(std::ffi::OsStr::new(url))
}

/// Hands `path` to the OS's default file/folder opener -- the same handler a
/// double-click in a graphical file manager would trigger.
#[cfg(unix)]
pub fn open_path(path: &Path) -> io::Result<()> {
    spawn_and_release(OPEN_COMMAND, &opener_path_argument(path))
}

#[cfg(windows)]
pub fn open_path(path: &Path) -> io::Result<()> {
    shell_execute_open(path.as_os_str())
}

#[cfg(all(unix, not(target_os = "macos")))]
const OPEN_COMMAND: &str = "xdg-open";

#[cfg(target_os = "macos")]
const OPEN_COMMAND: &str = "open";

/// Makes `path` unmistakably a filename rather than an option.
///
/// Both `xdg-open` and macOS's `open` parse a leading `-` as the start of an
/// option, so a relative path whose first character is `-` -- a perfectly
/// legal filename, and one a user can click straight out of terminal output --
/// would be rejected as bad syntax by `xdg-open` or swallowed as a flag by
/// `open` instead of being opened. Prefixing `./` names the very same file
/// while removing the leading `-`. An absolute path can never begin with one,
/// so the common case borrows and allocates nothing.
///
/// A URL needs no equivalent guard: callers must already have restricted it
/// to an allowed scheme, and a scheme has to start with a letter.
#[cfg(unix)]
fn opener_path_argument(path: &Path) -> std::borrow::Cow<'_, std::ffi::OsStr> {
    use std::borrow::Cow;
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    // Inspected as bytes rather than through `to_str`: a non-UTF-8 filename is
    // still a filename and has to open exactly like any other.
    let bytes = path.as_os_str().as_bytes();
    if bytes.first() != Some(&b'-') {
        return Cow::Borrowed(path.as_os_str());
    }
    let mut prefixed: Vec<u8> = Vec::with_capacity(bytes.len() + 2);
    prefixed.extend_from_slice(b"./");
    prefixed.extend_from_slice(bytes);
    Cow::Owned(OsString::from_vec(prefixed))
}

/// Spawns `command arg` and reaps it on a detached thread. The opener usually
/// hands the target to an already-running browser or file manager and exits
/// within milliseconds, but it can equally become the parent of a freshly
/// started handler and live exactly as long as that handler does. Either way a
/// dropped `Child` is never reaped on Unix -- without this thread it stays a
/// zombie in the process table for the rest of this long-lived TUI process.
///
/// Stdio is explicitly nulled rather than inherited: the parent is a raw-mode
/// TUI holding the terminal's alternate screen, and `xdg-open`/`open` (or a
/// handler they invoke) writing diagnostics to an inherited stderr, or
/// reading from an inherited stdin, would corrupt or steal input from the
/// TUI's own screen.
#[cfg(unix)]
fn spawn_and_release(command: &'static str, arg: &std::ffi::OsStr) -> io::Result<()> {
    // An empty target can never open anything, and the opener's non-zero exit
    // arrives long after this function has returned `Ok` -- the caller would
    // report an "Opening" that never happened. Refuse while a real error can
    // still travel back to it.
    if arg.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("refusing to hand {command} an empty target"),
        ));
    }
    let mut child = std::process::Command::new(command)
        .arg(arg)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    // `Builder::spawn` instead of `thread::spawn`: the latter panics when the
    // OS cannot create a thread, and this runs in response to a user action.
    // If the reaper thread cannot be created, the opener has still launched --
    // log and accept a zombie entry (it is reaped at process exit) rather
    // than panicking the TUI or blocking it on an inline `wait()`.
    let reaper = std::thread::Builder::new()
        .name(format!("reap-{command}"))
        .spawn(move || match child.wait() {
            Ok(status) if !status.success() => {
                tracing::warn!("{command} exited with {status}");
            }
            Ok(_) => {}
            Err(error) => tracing::warn!("failed to reap {command}: {error}"),
        });
    if let Err(error) = reaper {
        tracing::warn!("could not spawn reaper thread for {command}: {error}");
    }
    Ok(())
}

/// `cmd /C start` is unsafe here: `cmd.exe` re-parses the whole command line
/// itself and treats `&`, `|`, `^`, `<`, `>` as its own operators regardless
/// of how the argument array was assembled. `ShellExecuteW` takes the target
/// as a single parameter with no such re-parsing step, so it is the only safe
/// way to invoke the OS's default handler for attacker-influenced text.
#[cfg(windows)]
fn shell_execute_open(target: &std::ffi::OsStr) -> io::Result<()> {
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    // Same contract as the Unix path: an empty target is a caller mistake, not
    // a shell-level failure, and both platforms must reject it identically so
    // callers never have to branch on the operating system.
    if target.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing to hand ShellExecuteW an empty target",
        ));
    }

    // Encoded straight from the caller's `OsStr` -- never round-tripped
    // through `str`, which would silently mangle a path containing an
    // unpaired UTF-16 surrogate (valid as `OsStr` on Windows, not valid
    // Unicode) into the lossy replacement character.
    let operation: Vec<u16> = OsStr::new("open").encode_wide().chain(once(0)).collect();
    let file: Vec<u16> = target.encode_wide().chain(once(0)).collect();

    // SAFETY: `operation` and `file` are NUL-terminated UTF-16 buffers kept
    // alive for the duration of this call; the remaining pointer arguments
    // are explicitly null, which `ShellExecuteW` accepts.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // A return value greater than 32 means success; anything else is one of
    // `Shell32`'s own small `SE_ERR_*` codes. Those are NOT `GetLastError`
    // codes, so they must never go through `io::Error::from_raw_os_error`,
    // which would relabel e.g. `SE_ERR_NOASSOC` (31) as the unrelated Win32
    // `ERROR_GEN_FAILURE`.
    if (result as usize) > 32 {
        Ok(())
    } else {
        Err(shell_execute_error(result as usize))
    }
}

/// Translates a `ShellExecuteW` failure return value (<= 32) into an
/// `io::Error` that names the actual shell-level failure. The `SE_ERR_*`
/// numbering only coincides with Win32 error codes for a handful of values,
/// so each known code is mapped explicitly instead of being reinterpreted as
/// an OS error number.
#[cfg(windows)]
fn shell_execute_error(code: usize) -> io::Error {
    use io::ErrorKind;

    let (kind, description) = match code {
        0 => (ErrorKind::OutOfMemory, "out of memory or resources"),
        2 => (ErrorKind::NotFound, "file not found (SE_ERR_FNF)"),
        3 => (ErrorKind::NotFound, "path not found (SE_ERR_PNF)"),
        5 => (
            ErrorKind::PermissionDenied,
            "access denied (SE_ERR_ACCESSDENIED)",
        ),
        8 => (ErrorKind::OutOfMemory, "out of memory (SE_ERR_OOM)"),
        // Documented alongside the `SE_ERR_*` codes, and returned for a target
        // that resolves to a malformed executable. Without this arm it fell
        // into the catch-all and was reported as an unknown failure.
        11 => (
            ErrorKind::InvalidData,
            "the executable file is invalid (ERROR_BAD_FORMAT)",
        ),
        26 => (ErrorKind::Other, "sharing violation (SE_ERR_SHARE)"),
        27 => (
            ErrorKind::Other,
            "incomplete or invalid file association (SE_ERR_ASSOCINCOMPLETE)",
        ),
        28 => (
            ErrorKind::TimedOut,
            "DDE transaction timed out (SE_ERR_DDETIMEOUT)",
        ),
        29 => (ErrorKind::Other, "DDE transaction failed (SE_ERR_DDEFAIL)"),
        30 => (ErrorKind::Other, "DDE server busy (SE_ERR_DDEBUSY)"),
        31 => (
            ErrorKind::Unsupported,
            "no application is associated with this file type (SE_ERR_NOASSOC)",
        ),
        32 => (
            ErrorKind::NotFound,
            "required shared library not found (SE_ERR_DLLNOTFOUND)",
        ),
        _ => (ErrorKind::Other, "unknown ShellExecuteW failure"),
    };
    io::Error::new(
        kind,
        format!("ShellExecuteW failed: {description} (code {code})"),
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_path_starting_with_a_dash_is_rewritten_so_it_cannot_read_as_an_option() {
        assert_eq!(
            &*opener_path_argument(Path::new("-weird name.txt")),
            std::ffi::OsStr::new("./-weird name.txt")
        );
    }

    #[test]
    fn an_ordinary_path_reaches_the_opener_byte_for_byte() {
        assert_eq!(
            &*opener_path_argument(Path::new("/home/user/notes.md")),
            std::ffi::OsStr::new("/home/user/notes.md")
        );
    }

    #[test]
    fn an_empty_target_fails_instead_of_reporting_a_launch_that_cannot_happen() {
        let error = spawn_and_release(OPEN_COMMAND, std::ffi::OsStr::new(""))
            .expect_err("an empty target must never reach the system opener");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
