//! Process lifetime control: bounded child trees, stopping a server process,
//! and asking whether one is still alive.
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
//!
//! Progress probes use [`prepare_process_tree`] plus [`ProcessTreeGuard`]
//! instead of killing only the shell process. That distinction matters because
//! shell commands routinely spawn children which otherwise survive a timeout
//! or cancellation and keep output pipes or other resources open.

use std::io;
use std::process::Command;

/// Configures `command` so its process and ordinary descendants form one
/// kernel-owned termination unit.
///
/// Call this before spawning, then immediately create a [`ProcessTreeGuard`]
/// from the returned child's process id. The guard terminates that unit when
/// explicitly requested or when dropped, which makes cancellation safe too.
///
/// Unix can establish the process group atomically in the child before exec.
/// Windows creates a new console process group here and the guard additionally
/// assigns the spawned process to a kill-on-close Job Object. The standard
/// process API does not expose a suspended child's primary thread, so Windows
/// has an unavoidable spawn-to-assignment window; callers must attach the guard
/// immediately and fail closed if assignment is unsuccessful.
#[cfg(unix)]
pub fn prepare_process_tree(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // Zero asks the child to use its own pid as the new process-group id. This
    // happens after fork and before exec, before agent-authored code can run.
    command.process_group(0);
}

#[cfg(windows)]
pub fn prepare_process_tree(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    // This isolates console-control delivery. Descendant lifetime is enforced
    // by ProcessTreeGuard's Job Object rather than by this console grouping.
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

/// Owns the operating-system termination unit for one spawned command tree.
///
/// Dropping an armed guard is intentionally destructive: it is the cancellation
/// path used when an async probe future is aborted. Call [`Self::terminate`]
/// after ordinary completion as well, so descendants that kept running after
/// their original shell exited cannot leak out of a completed probe.
#[must_use = "dropping the guard immediately terminates the configured process tree"]
#[derive(Debug)]
pub struct ProcessTreeGuard {
    #[cfg(unix)]
    process_group_id: Option<libc::pid_t>,
    // Store the Windows HANDLE as an integer so this ownership token remains
    // Send across async suspension points. It is converted back only inside
    // the platform-specific implementation below.
    #[cfg(windows)]
    job_handle: Option<isize>,
}

impl ProcessTreeGuard {
    /// Attaches a guard to a child previously configured with
    /// [`prepare_process_tree`].
    ///
    /// On Windows this can fail if the process cannot be assigned to the Job
    /// Object (for example because of a restrictive outer job). Callers must
    /// then stop and reap the direct child rather than run it unguarded.
    #[cfg(unix)]
    pub fn attach(process_id: u32) -> io::Result<Self> {
        let process_group_id = checked_process_id(process_id)?;
        Ok(Self {
            process_group_id: Some(process_group_id),
        })
    }

    #[cfg(windows)]
    pub fn attach(process_id: u32) -> io::Result<Self> {
        use std::ptr;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };

        if process_id == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "process id 0 cannot own a process tree",
            ));
        }

        // SAFETY: null security attributes and name request a private Job
        // Object. The returned owned handle is closed on every path below.
        let job_handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if job_handle.is_null() {
            return Err(io::Error::last_os_error());
        }

        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `limits` has exactly the type and size required by the
        // selected information class and remains live for the call.
        let configured = unsafe {
            SetInformationJobObject(
                job_handle,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        };
        if configured == 0 {
            let error = io::Error::last_os_error();
            // SAFETY: `job_handle` is owned here and has not been closed.
            unsafe { CloseHandle(job_handle) };
            return Err(error);
        }

        // Open a separate process handle rather than retaining one owned by a
        // particular async runtime. The child handle held by the caller keeps
        // even a very short-lived process object addressable during this step.
        // SAFETY: integer arguments only; the returned handle is checked and
        // closed below.
        let process_handle =
            unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, process_id) };
        if process_handle.is_null() {
            let error = io::Error::last_os_error();
            // SAFETY: `job_handle` is owned here and has not been closed.
            unsafe { CloseHandle(job_handle) };
            return Err(error);
        }

        // SAFETY: both handles are valid. Assignment causes this process and
        // descendants created afterwards to be terminated when the Job handle
        // is closed.
        let assigned = unsafe { AssignProcessToJobObject(job_handle, process_handle) };
        let assignment_error = (assigned == 0).then(io::Error::last_os_error);
        // SAFETY: this function owns both handles at this point. The process
        // itself remains alive after closing our duplicate process handle.
        unsafe { CloseHandle(process_handle) };
        if let Some(error) = assignment_error {
            // SAFETY: assignment failed, so closing the private Job cannot
            // affect an unrelated process.
            unsafe { CloseHandle(job_handle) };
            return Err(error);
        }

        Ok(Self {
            job_handle: Some(job_handle as isize),
        })
    }

    /// Immediately terminates the guarded process and all descendants still
    /// belonging to its operating-system termination unit.
    ///
    /// The operation is idempotent. "Already gone" is success.
    #[cfg(unix)]
    pub fn terminate(&mut self) -> io::Result<()> {
        let Some(process_group_id) = self.process_group_id else {
            return Ok(());
        };
        // A negative pid addresses the process group. `checked_process_id`
        // rejects zero and values that cannot safely be negated.
        // SAFETY: kill takes no pointers; invalid/stale group ids are reported
        // through errno.
        let result = unsafe { libc::kill(-process_group_id, libc::SIGKILL) };
        if result == 0 {
            self.process_group_id = None;
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            self.process_group_id = None;
            return Ok(());
        }
        Err(error)
    }

    #[cfg(windows)]
    pub fn terminate(&mut self) -> io::Result<()> {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;

        let Some(raw_job_handle) = self.job_handle.take() else {
            return Ok(());
        };
        let job_handle = raw_job_handle as HANDLE;
        // SAFETY: the handle is the live, private Job Object owned by this
        // guard. Closing it is required even if explicit termination reports
        // an error; KILL_ON_JOB_CLOSE provides the second termination path.
        let terminated = unsafe { TerminateJobObject(job_handle, 1) };
        let termination_error = (terminated == 0).then(io::Error::last_os_error);
        // SAFETY: taken above, therefore closed exactly once.
        unsafe { CloseHandle(job_handle) };
        termination_error.map_or(Ok(()), Err)
    }
}

impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

#[cfg(unix)]
fn checked_process_id(process_id: u32) -> io::Result<libc::pid_t> {
    if process_id == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "process id 0 cannot own a process tree",
        ));
    }
    let process_id = libc::pid_t::try_from(process_id).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "process id does not fit in pid_t",
        )
    })?;
    // The negative representation is used by kill(2) for a process group. A
    // pid_t minimum cannot be negated, though a valid positive u32 can never
    // normally reach it; keep the arithmetic explicit nonetheless.
    process_id.checked_neg().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "process id cannot be represented as a process group",
        )
    })?;
    Ok(process_id)
}

/// Asks the process to terminate.
///
/// This is abrupt on every platform, and acceptably so precisely because it is
/// the fallback: the graceful path is the IPC shutdown request the caller
/// already tried. On Unix it is `SIGTERM`, for which the server installs no
/// handler, so the default disposition ends it without running the shutdown
/// cleanup `ilium_server::run` performs on its own (final snapshot flush,
/// session endpoint removal). On Windows it is `TerminateProcess`, there being
/// no signal equivalent for a detached, console-less process. A caller that
/// needs the server's own cleanup to run has to reach it over IPC, not here.
///
/// Process id 0 is rejected as invalid input on both platforms rather than
/// treated as "already gone", so a corrupted on-disk pid cannot be mistaken
/// for a successfully stopped server.
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

    // Zero is never a pid this process tracks; it reaches here only from a
    // corrupted on-disk ready marker. Refusing it keeps the contract identical
    // to the Unix build, where zero would otherwise signal a whole process
    // group -- reporting a stop nobody performed would be worse than an error.
    if process_id == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "process id 0 is not a process this session can stop",
        ));
    }
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

    #[cfg(unix)]
    #[test]
    fn dropping_process_tree_guard_stops_the_shell_and_its_descendant() {
        use std::io::{BufRead, BufReader};
        use std::process::Stdio;

        let mut command = std::process::Command::new("/bin/sh");
        command
            .args(["-c", "sleep 60 & descendant=$!; echo $descendant; wait"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        prepare_process_tree(&mut command);
        let mut child = command.spawn().expect("spawn isolated process tree");
        let root_process_id = child.id();
        let guard = ProcessTreeGuard::attach(root_process_id).expect("attach process tree guard");
        let stdout = child.stdout.take().expect("child stdout");
        let mut descendant_line = String::new();
        BufReader::new(stdout)
            .read_line(&mut descendant_line)
            .expect("read descendant pid");
        let descendant_process_id = descendant_line
            .trim()
            .parse::<u32>()
            .expect("numeric descendant pid");
        assert!(is_running(root_process_id));
        assert!(is_running(descendant_process_id));

        drop(guard);
        child.wait().expect("reap process-tree root");

        assert!(!is_running(root_process_id));
        wait_until_not_running(descendant_process_id);
    }

    // Pid 0 means "the caller's own process group" to `kill`; the guards must
    // keep it from ever reaching the syscall.
    #[cfg(unix)]
    #[test]
    fn pid_zero_is_rejected_not_signalled() {
        assert!(!is_running(0));
        let error = terminate(0).expect_err("terminating pid 0 must be refused");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        let error = ProcessTreeGuard::attach(0).expect_err("guarding pid 0 must be refused");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    fn wait_until_not_running(process_id: u32) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while is_running(process_id) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            !is_running(process_id),
            "descendant {process_id} survived process-tree termination"
        );
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
