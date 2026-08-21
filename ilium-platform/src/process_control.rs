//! Stopping a server process, and asking whether one is still alive.
//!
//! These are the CLI's *fallback* path. A session is normally stopped by
//! asking the server over IPC to shut itself down; these functions exist for
//! when that request cannot be delivered or is not honoured in time, and for
//! confirming afterwards that the exact process which owned the session socket
//! has actually gone.
//!
//! "Already gone" is success, not failure, everywhere below: a graceful
//! shutdown can complete between the liveness probe that chose this path and
//! the call itself, and treating that race as an error would make an ordinary
//! stop report a failure.

use std::io;

/// Asks the process to terminate.
///
/// On Unix this is `SIGTERM`, which the server handles to shut down cleanly.
/// Windows has no signal equivalent for a detached, console-less process, so
/// this is `TerminateProcess` -- abrupt by necessity. That difference is
/// acceptable precisely because this is the fallback: the graceful path is the
/// IPC shutdown request the caller already tried.
#[cfg(unix)]
pub fn terminate(process_id: u32) -> io::Result<()> {
    // `kill(0, ...)` signals the caller's *entire process group* -- delivered
    // here, that would SIGTERM this CLI and its shell job, not a server. Zero
    // can reach this function from a corrupted on-disk ready marker, so it
    // must be rejected, not passed through.
    if process_id == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "process id 0 would signal the whole process group",
        ));
    }
    // A truncating `as` cast can turn a `u32` at or above 2^31 into a
    // negative `pid_t`, and `kill` treats a negative pid as "signal this
    // whole process group" -- the opposite of the single-process semantics
    // this function promises its caller.
    let Ok(process_id) = libc::pid_t::try_from(process_id) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "process id does not fit in pid_t",
        ));
    };
    // SAFETY: `kill` takes no pointers and cannot corrupt this process's
    // memory; an invalid pid is reported through `errno`, not undefined
    // behaviour.
    let result = unsafe { libc::kill(process_id, libc::SIGTERM) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    // `ESRCH` means the process already exited -- the outcome asked for.
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }
    Err(error)
}

#[cfg(windows)]
pub fn terminate(process_id: u32) -> io::Result<()> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    // Asking whether the process still exists is more reliable than reading
    // the error code from a failed open. Windows reports an exited process
    // inconsistently -- `ERROR_INVALID_PARAMETER` when the id is gone entirely,
    // but `ERROR_ACCESS_DENIED` while an exited process still has a handle open
    // somewhere -- and "already gone" is the outcome asked for either way.
    if !is_running(process_id) {
        return Ok(());
    }
    // SAFETY: `OpenProcess` takes only integers and returns a handle this
    // function closes on every path below.
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, process_id) };
    if handle.is_null() {
        // Read the error left by `OpenProcess` before `is_running` makes its
        // own Win32 calls below and overwrites the thread-local last-error
        // value this function is about to report.
        let open_error = io::Error::last_os_error();
        // It exited between the check above and here, which is still the
        // outcome asked for.
        if !is_running(process_id) {
            return Ok(());
        }
        return Err(open_error);
    }
    // SAFETY: `handle` is a valid process handle owned by this function.
    let result = unsafe { TerminateProcess(handle, 1) };
    // Read the error immediately, before `CloseHandle` below can overwrite
    // the thread-local last-error value this function is about to report.
    let terminate_error = (result == 0).then(io::Error::last_os_error);
    // SAFETY: same handle, closed exactly once, before any early return.
    unsafe { CloseHandle(handle) };
    if let Some(error) = terminate_error {
        // `TerminateProcess` reports `ERROR_ACCESS_DENIED` for a process that
        // exited between the open above and the call -- which is the outcome
        // asked for, exactly like the null-handle path.
        if !is_running(process_id) {
            return Ok(());
        }
        return Err(error);
    }
    Ok(())
}

/// Replaces the calling process with `command`, and so does not return on
/// success -- only a failure to start the replacement comes back.
///
/// The client uses this to restart itself in place: the terminal keeps talking
/// to one process at one place in the shell's job control, rather than gaining
/// a child that outlives its parent's prompt.
///
/// Unix does this natively with `exec`. Windows has no equivalent, so the
/// closest honest approximation is to start the replacement and exit
/// immediately, handing over the console. The observable difference is a brief
/// moment where both processes exist.
#[cfg(unix)]
pub fn replace_current_process(command: &mut std::process::Command) -> io::Error {
    use std::os::unix::process::CommandExt;

    // `exec` only returns when it fails; on success this process is gone.
    command.exec()
}

#[cfg(windows)]
pub fn replace_current_process(command: &mut std::process::Command) -> io::Error {
    match command.spawn() {
        // The replacement owns the console now; leaving immediately is what
        // makes this stand in for `exec`.
        Ok(_) => std::process::exit(0),
        Err(error) => error,
    }
}

/// Whether the process still exists.
///
/// Used to confirm that the exact process which owned a session socket has
/// exited. A socket probe alone is not enough: a dying listener can still
/// accept a queued connection and look alive.
#[cfg(unix)]
pub fn is_running(process_id: u32) -> bool {
    // `kill(0, 0)` probes the caller's *own process group*, which always
    // exists, so passing zero through would report a nonexistent tracked
    // process as running forever. Zero is never a pid this process tracks.
    if process_id == 0 {
        return false;
    }
    // Same truncating-cast hazard as `terminate`: a `u32` that doesn't fit in
    // `pid_t` is not a real pid this process could be tracking, so report it
    // as not running rather than let the cast flip its sign.
    let Ok(process_id) = libc::pid_t::try_from(process_id) else {
        return false;
    };
    // SAFETY: signal 0 performs only the existence and permission check that
    // a real signal would, and delivers nothing.
    let result = unsafe { libc::kill(process_id, 0) };
    // `EPERM` proves the process exists while belonging to another user, which
    // still answers the question asked.
    result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
pub fn is_running(process_id: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: integers in, handle out; closed on every path below.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return false;
    }
    let mut exit_code: u32 = 0;
    // SAFETY: `handle` is valid and `exit_code` is a live local the callee
    // only writes.
    let queried = unsafe { GetExitCodeProcess(handle, &mut exit_code) };
    // SAFETY: same handle, closed exactly once.
    unsafe { CloseHandle(handle) };
    // A handle can outlive the process itself while something still holds it,
    // so an exit code other than `STILL_ACTIVE` means genuinely finished.
    queried != 0 && exit_code == STILL_ACTIVE as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_process_is_running() {
        assert!(is_running(std::process::id()));
    }

    #[test]
    fn a_finished_process_is_not_running_and_terminating_it_succeeds() {
        let mut child = short_lived_child();
        let process_id = child.id();
        child.wait().expect("child exits");

        assert!(!is_running(process_id));
        // "Already gone" is the outcome asked for, so this must not error.
        terminate(process_id).expect("terminating an exited process is success");
    }

    #[test]
    fn terminate_stops_a_running_process() {
        let mut child = long_lived_child();
        let process_id = child.id();

        terminate(process_id).expect("terminate");

        child.wait().expect("child is reaped");
        assert!(!is_running(process_id));
    }

    // Pid 0 means "the caller's own process group" to `kill`; the guards must
    // keep it from ever reaching the syscall.
    #[cfg(unix)]
    #[test]
    fn pid_zero_is_rejected_not_signalled() {
        assert!(!is_running(0));
        let error = terminate(0).expect_err("terminating pid 0 must be refused");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    fn short_lived_child() -> std::process::Child {
        std::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("spawn")
    }

    #[cfg(unix)]
    fn long_lived_child() -> std::process::Child {
        std::process::Command::new("/bin/sh")
            .args(["-c", "sleep 60"])
            .spawn()
            .expect("spawn")
    }

    #[cfg(windows)]
    fn short_lived_child() -> std::process::Child {
        std::process::Command::new("cmd")
            .args(["/C", "exit 0"])
            .spawn()
            .expect("spawn")
    }

    #[cfg(windows)]
    fn long_lived_child() -> std::process::Child {
        std::process::Command::new("cmd")
            .args(["/C", "timeout /T 60 /NOBREAK"])
            .spawn()
            .expect("spawn")
    }
}
