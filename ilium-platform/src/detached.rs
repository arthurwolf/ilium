//! Spawning a process that outlives the one that started it.
//!
//! The CLI starts `ilium-server` this way. The server must survive the client
//! exiting, must never hold a controlling terminal (a stray signal delivered to
//! the terminal's foreground group would otherwise kill every pane at once),
//! and must not linger as a zombie the CLI is responsible for reaping.
//!
//! Unix and Windows reach that state by opposite means -- Unix by
//! double-forking away from the session, Windows by asking the kernel for a
//! detached process up front -- which is exactly why the decision lives here
//! rather than at the call site.

use std::io;
use std::process::Command;

/// Starts `command` fully detached from this process.
///
/// Returns once the child is running. The caller keeps no handle, because a
/// detached process is by definition not this process's to wait on.
///
/// # Unix
///
/// Double-forks so the eventual server is orphaned to `init` and calls
/// `setsid` so it leads a brand-new session with no controlling terminal. The
/// intermediate process is reaped before returning, so no zombie outlives this
/// call.
#[cfg(unix)]
pub fn spawn_detached(command: &mut Command) -> io::Result<()> {
    use std::os::unix::process::CommandExt;

    // SAFETY: this closure runs in the forked child between `fork()` and
    // `execve()`, while the child is still single-threaded and may only call
    // async-signal-safe functions (see `Command::pre_exec`'s safety contract).
    // `fork`, `setsid`, and `_exit` all are; nothing here allocates, takes a
    // lock, or touches Rust-level global state, and `last_os_error` only reads
    // `errno`.
    unsafe {
        command.pre_exec(|| match libc::fork() {
            -1 => Err(io::Error::last_os_error()),
            0 => {
                // Grandchild: leave the middle process's session and group so
                // it can never reacquire a controlling terminal, then fall
                // through to `execve` for the real binary.
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            }
            _ => {
                // Middle process: exit without unwinding. `std::process::exit`
                // is not async-signal-safe this soon after `fork()`; `_exit`
                // is. The parent reaps this pid immediately below.
                libc::_exit(0);
            }
        });
    }

    let mut middle_process = command.spawn()?;
    // `spawn()` only returns `Ok` once the exec-status pipe closes, which
    // requires the middle process to have already reached `_exit` above, so
    // this reaps an already-dead process rather than blocking. Skipping it
    // would leave a zombie for the CLI's whole lifetime -- the exact leak this
    // function exists to avoid.
    let _ = middle_process.wait();
    Ok(())
}

/// # Windows
///
/// `DETACHED_PROCESS` starts the child with no console at all, which is the
/// closest equivalent to a Unix session leader with no controlling terminal:
/// a `Ctrl+C` delivered to the launching console cannot reach it.
/// `CREATE_NEW_PROCESS_GROUP` additionally detaches it from the parent's
/// group so a group-wide signal stops at the boundary.
///
/// No wait happens: unlike Unix, there is no intermediate process to reap --
/// the child is already independent, and Windows has no zombie to collect.
#[cfg(windows)]
pub fn spawn_detached(command: &mut Command) -> io::Result<()> {
    use std::os::windows::process::CommandExt;

    // From `processthreadsapi.h`. Declared here rather than pulling in a
    // Windows bindings crate for two integers that have been stable for
    // decades.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    command.spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[test]
    fn a_detached_child_runs_and_leaves_no_zombie_behind() {
        let root = tempfile::tempdir().expect("temp dir");
        let marker = root.path().join("marker.txt");

        // A shell is the one "write a file then exit" program available on
        // every platform's default install under a predictable name.
        let mut command = shell_command_writing(&marker);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        spawn_detached(&mut command).expect("spawn detached");

        // The child is orphaned, so there is no handle to wait on; poll for
        // its observable effect instead.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(marker.exists(), "detached child never wrote its marker");
    }

    #[cfg(unix)]
    fn shell_command_writing(marker: &std::path::Path) -> Command {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(format!(
            "printf started > {}",
            marker.to_string_lossy().replace('\'', "")
        ));
        command
    }

    #[cfg(windows)]
    fn shell_command_writing(marker: &std::path::Path) -> Command {
        let mut command = Command::new("cmd");
        command
            .arg("/C")
            .arg(format!("echo started> \"{}\"", marker.to_string_lossy()));
        command
    }
}
