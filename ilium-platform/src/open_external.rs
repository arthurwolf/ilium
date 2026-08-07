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
    shell_execute_open(url)
}

/// Hands `path` to the OS's default file/folder opener -- the same handler a
/// double-click in a graphical file manager would trigger.
#[cfg(unix)]
pub fn open_path(path: &Path) -> io::Result<()> {
    spawn_and_release(OPEN_COMMAND, path.as_os_str())
}

#[cfg(windows)]
pub fn open_path(path: &Path) -> io::Result<()> {
    shell_execute_open(&path.to_string_lossy())
}

#[cfg(all(unix, not(target_os = "macos")))]
const OPEN_COMMAND: &str = "xdg-open";

#[cfg(target_os = "macos")]
const OPEN_COMMAND: &str = "open";

/// Spawns `command arg` and reaps it on a detached thread. The opener process
/// hands off to the real browser/file-manager and exits almost immediately,
/// but a dropped `Child` is never reaped on Unix -- without this it stays a
/// zombie in the process table for the rest of this long-lived TUI process.
#[cfg(unix)]
fn spawn_and_release(command: &'static str, arg: &std::ffi::OsStr) -> io::Result<()> {
    let mut child = std::process::Command::new(command).arg(arg).spawn()?;
    std::thread::spawn(move || match child.wait() {
        Ok(status) if !status.success() => {
            tracing::warn!("{command} exited with {status}");
        }
        Ok(_) => {}
        Err(error) => tracing::warn!("failed to reap {command}: {error}"),
    });
    Ok(())
}

/// `cmd /C start` is unsafe here: `cmd.exe` re-parses the whole command line
/// itself and treats `&`, `|`, `^`, `<`, `>` as its own operators regardless
/// of how the argument array was assembled. `ShellExecuteW` takes the target
/// as a single parameter with no such re-parsing step, so it is the only safe
/// way to invoke the OS's default handler for attacker-influenced text.
#[cfg(windows)]
fn shell_execute_open(target: &str) -> io::Result<()> {
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation: Vec<u16> = OsStr::new("open").encode_wide().chain(once(0)).collect();
    let file: Vec<u16> = OsStr::new(target).encode_wide().chain(once(0)).collect();

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
    // `Shell32`'s own small error codes, not a `GetLastError` code.
    if (result as usize) > 32 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32))
    }
}
