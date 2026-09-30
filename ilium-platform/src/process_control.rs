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
use std::path::Path;
use std::process::Command;
use std::time::Duration;
#[cfg(target_os = "linux")]
use std::time::Instant;

/// Observe an exclusively owned direct child without releasing its numeric identity.
/// The caller must not poll Child::wait/try_wait until process-group signalling is done.
#[cfg(target_os = "linux")]
pub fn child_exited_without_reaping(process_id: u32) -> io::Result<bool> {
    let process_id = checked_process_id(process_id)?;
    let mut information: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: valid output storage; WNOHANG is nonblocking and WNOWAIT forbids reaping.
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            process_id as libc::id_t,
            &mut information,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: waitid initializes the siginfo_t and si_pid selects its documented child field.
    Ok(unsafe { information.si_pid() } != 0)
}
/// Observe group disappearance without ever signalling a possibly recycled group ID.
#[cfg(target_os = "linux")]
pub fn wait_for_group_exit(process_id: u32, timeout: Duration) -> io::Result<()> {
    let process_id = checked_process_id(process_id)?;
    let deadline = Instant::now() + timeout;
    loop {
        // SAFETY: signal zero performs an existence/permission probe only.
        let result = unsafe { libc::kill(-process_id, 0) };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            return Err(error);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "command process group did not disappear",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// An advisory cross-process lease shared by cooperating Ilium workspace operations.
/// The lock file is permanent: unlinking it would create two independent lock domains.
#[derive(Debug)]
pub struct WorkspaceRepositoryLease {
    _file: std::fs::File,
}
#[cfg(unix)]
pub fn try_workspace_repository_lease(
    common_dir: &Path,
) -> io::Result<Option<WorkspaceRepositoryLease>> {
    use std::os::fd::AsRawFd;
    let directory = crate::secure_fs::NoFollowDirectory::open_root(common_dir)?;
    let name = std::ffi::OsStr::new("ilium-workspace-operations.lock");
    let file = match directory.create_regular(name) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            directory.open_regular(name)?
        }
        Err(error) => return Err(error),
    };
    // SAFETY: fcntl only inspects and changes flags on this owned descriptor.
    let descriptor_flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
    if descriptor_flags < 0
        || unsafe {
            libc::fcntl(
                file.as_raw_fd(),
                libc::F_SETFD,
                descriptor_flags | libc::FD_CLOEXEC,
            )
        } < 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the descriptor is owned and the nonblocking flock call takes no pointers.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        return Ok(Some(WorkspaceRepositoryLease { _file: file }));
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::WouldBlock {
        return Ok(None);
    }
    Err(error)
}
#[cfg(windows)]
pub fn try_workspace_repository_lease(
    common_dir: &Path,
) -> io::Result<Option<WorkspaceRepositoryLease>> {
    let file = crate::secure_fs::private_open_options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(common_dir.join("ilium-workspace-operations.lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(WorkspaceRepositoryLease { _file: file })),
        Err(error) => {
            let error: io::Error = error.into();
            if error.kind() == io::ErrorKind::WouldBlock {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

#[cfg(not(any(unix, windows)))]
pub fn try_workspace_repository_lease(
    _common_dir: &Path,
) -> io::Result<Option<WorkspaceRepositoryLease>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "workspace repository leasing is unavailable",
    ))
}

/// Birth identity of the PTY session leader. A bare PID is never sufficient
/// authority to signal a process after it might have exited and been reused.
#[derive(Debug)]
pub struct PtyProcessIdentity {
    #[cfg(target_os = "linux")]
    process_id: u32,
    #[cfg(target_os = "linux")]
    start_ticks: u64,
    #[cfg(target_os = "linux")]
    pidfd: std::os::fd::OwnedFd,
}

/// Successful, bounded observation after terminating a PTY lineage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtyTerminationProof {
    pub signalled_processes: usize,
}

/// Captures the directly spawned PTY child while its handle is still owned.
/// An unavailable capture must disable later worktree removal, not spawning.
#[cfg(target_os = "linux")]
pub fn capture_pty_process(process_id: u32) -> io::Result<PtyProcessIdentity> {
    use std::os::fd::FromRawFd;
    let pidfd = open_linux_pidfd(process_id)?;
    // SAFETY: `open_linux_pidfd` returned a new owned descriptor.
    let pidfd = unsafe { std::os::fd::OwnedFd::from_raw_fd(pidfd) };
    let identity = read_linux_process(process_id)?;
    if matches!(identity.state, 'Z' | 'X') || !linux_pidfd_is_live(&pidfd)? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "PTY child already exited",
        ));
    }
    Ok(PtyProcessIdentity {
        process_id,
        start_ticks: identity.start_ticks,
        pidfd,
    })
}

#[cfg(not(target_os = "linux"))]
pub fn capture_pty_process(_process_id: u32) -> io::Result<PtyProcessIdentity> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "PTY process proof requires Linux procfs",
    ))
}

/// Terminates the captured PTY leader and descendants visible through its
/// parent lineage or Linux session. Only PID-identity-checked pidfds are
/// signalled. The caller must also run `processes_using_directory` after this
/// returns before removing a worktree: a process which daemonized before this
/// scan can have escaped both relationships.
#[cfg(target_os = "linux")]
pub fn terminate_pty_process_tree(
    root: &PtyProcessIdentity,
    timeout: Duration,
) -> io::Result<PtyTerminationProof> {
    if timeout.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "zero PTY termination timeout",
        ));
    }
    let deadline = Instant::now() + timeout;
    let current_root = read_linux_process(root.process_id)?;
    if current_root.start_ticks != root.start_ticks
        || matches!(current_root.state, 'Z' | 'X')
        || !linux_pidfd_is_live(&root.pidfd)?
    {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "PTY leader identity is no longer live",
        ));
    }
    let mut targets = linux_pty_lineage(root.process_id)?;
    if !targets.iter().any(|process| {
        process.process_id == root.process_id
            && process.start_ticks == root.start_ticks
            && !matches!(process.state, 'Z' | 'X')
    }) {
        return Err(io::Error::other("PTY leader was not in procfs snapshot"));
    }
    // Capture descendants before signalling the leader. Once it exits they
    // may be reparented and their original parent chain is no longer visible.
    let mut signalled_processes = 0;
    targets.sort_by_key(|process| process.process_id == root.process_id);
    for target in &targets {
        if target.state == 'Z' {
            continue;
        }
        let signalled = if target.process_id == root.process_id {
            signal_linux_owned_root(root, *target, libc::SIGKILL)?
        } else {
            signal_linux_identity(*target, libc::SIGKILL)?
        };
        if signalled {
            signalled_processes += 1;
        }
    }
    loop {
        let mut any_target_live = false;
        for target in &targets {
            any_target_live |= linux_identity_is_live(*target)?;
        }
        if !any_target_live {
            // Surviving session members may have forked during the first scan.
            let remaining = linux_pty_lineage(root.process_id)?;
            if remaining.iter().all(|process| process.state == 'Z') {
                return Ok(PtyTerminationProof {
                    signalled_processes,
                });
            }
            if !linux_identity_is_live(LinuxProcess {
                start_ticks: root.start_ticks,
                ..current_root
            })? {
                return Err(io::Error::other(
                    "PTY leader exited before all descendants were captured",
                ));
            }
            for target in remaining.iter().filter(|process| process.state != 'Z') {
                if signal_linux_identity(*target, libc::SIGKILL)? {
                    signalled_processes += 1;
                }
            }
            targets = remaining;
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "PTY descendant remained live after termination",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(not(target_os = "linux"))]
pub fn terminate_pty_process_tree(
    _root: &PtyProcessIdentity,
    _timeout: Duration,
) -> io::Result<PtyTerminationProof> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "PTY process proof requires Linux procfs",
    ))
}

/// Lists same-user processes with a cwd or open descriptor below the canonical root.
/// Any unreadable same-user cwd/descriptor scan fails closed because an empty list is a
/// deletion gate, not a best-effort status display.
#[cfg(target_os = "linux")]
pub fn processes_using_directory(root: &Path) -> io::Result<Vec<u32>> {
    use std::os::unix::fs::MetadataExt;
    let root = std::fs::canonicalize(root)?;
    let own_uid = unsafe { libc::geteuid() };
    let mut users = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    for entry in std::fs::read_dir("/proc")? {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "process cwd inspection exceeded its deadline",
            ));
        }
        let entry = entry?;
        let Some(process_id) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if metadata.uid() != own_uid {
            continue;
        }
        match std::fs::read_link(entry.path().join("cwd")) {
            Ok(cwd) if cwd.starts_with(&root) => users.push(process_id),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // A vanished proc entry is normal; an existing live process
                // with an unreadable cwd is not proof that the path is free.
                match read_linux_process(process_id) {
                    Ok(process) if matches!(process.state, 'Z' | 'X') => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Ok(_) => {
                        return Err(io::Error::other(format!(
                            "cwd of live process {process_id} is unavailable"
                        )))
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("cannot inspect cwd of process {process_id}: {error}"),
                ))
            }
        }
        if linux_process_has_worktree_fd(process_id, &root, deadline)? {
            users.push(process_id);
        }
    }
    users.sort_unstable();
    users.dedup();
    Ok(users)
}

#[cfg(not(target_os = "linux"))]
pub fn processes_using_directory(_root: &Path) -> io::Result<Vec<u32>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "process cwd proof requires Linux procfs",
    ))
}

/// Open descriptors matter even when a process changed its cwd out of the worktree.
/// This remains an observation, not an exclusion of future external opens or mmap-only use.
#[cfg(target_os = "linux")]
fn linux_process_has_worktree_fd(
    process_id: u32,
    root: &Path,
    deadline: Instant,
) -> io::Result<bool> {
    let entries = match std::fs::read_dir(format!("/proc/{process_id}/fd")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return match read_linux_process(process_id) {
                Ok(process) if matches!(process.state, 'Z' | 'X') => Ok(false),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
                Ok(_) => Err(io::Error::other(format!(
                    "fds of live process {process_id} are unavailable"
                ))),
                Err(error) => Err(error),
            };
        }
        Err(error) => return Err(error),
    };
    for entry in entries {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "process file-descriptor inspection exceeded its deadline",
            ));
        }
        let entry = entry?;
        match std::fs::read_link(entry.path()) {
            Ok(target) if target.starts_with(root) => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug)]
struct LinuxProcess {
    process_id: u32,
    parent_id: u32,
    session_id: u32,
    state: char,
    start_ticks: u64,
}

#[cfg(target_os = "linux")]
fn read_linux_process(process_id: u32) -> io::Result<LinuxProcess> {
    let stat = std::fs::read_to_string(format!("/proc/{process_id}/stat"))?;
    parse_linux_stat(process_id, &stat)
}

#[cfg(target_os = "linux")]
fn parse_linux_stat(process_id: u32, stat: &str) -> io::Result<LinuxProcess> {
    // `comm` is parenthesized and may contain spaces or closing parentheses.
    let tail = stat
        .rsplit_once(") ")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "malformed proc stat"))?
        .1;
    let fields: Vec<&str> = tail.split_whitespace().collect();
    let field = |index: usize| {
        fields
            .get(index)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "short proc stat"))
    };
    let parse = |index: usize| -> io::Result<u32> {
        field(index)?
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid proc stat integer"))
    };
    Ok(LinuxProcess {
        process_id,
        state: field(0)?
            .chars()
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing proc state"))?,
        parent_id: parse(1)?,
        session_id: parse(3)?,
        start_ticks: field(19)?
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid proc start time"))?,
    })
}

#[cfg(target_os = "linux")]
fn linux_pty_lineage(root_process_id: u32) -> io::Result<Vec<LinuxProcess>> {
    let mut processes = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(process_id) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        match read_linux_process(process_id) {
            Ok(process) => processes.push(process),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return Err(error),
            Err(error) => return Err(error),
        }
    }
    let mut selected: std::collections::HashSet<u32> = processes
        .iter()
        .filter(|process| process.session_id == root_process_id)
        .map(|process| process.process_id)
        .collect();
    selected.insert(root_process_id);
    loop {
        let old_size = selected.len();
        for process in &processes {
            if selected.contains(&process.parent_id) {
                selected.insert(process.process_id);
            }
        }
        if selected.len() == old_size {
            break;
        }
    }
    Ok(processes
        .into_iter()
        .filter(|process| selected.contains(&process.process_id))
        .collect())
}

#[cfg(target_os = "linux")]
fn linux_identity_is_live(identity: LinuxProcess) -> io::Result<bool> {
    match read_linux_process(identity.process_id) {
        Ok(current) => Ok(current.start_ticks == identity.start_ticks
            && current.state != 'Z'
            && current.state != 'X'),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "linux")]
fn signal_linux_identity(identity: LinuxProcess, signal: libc::c_int) -> io::Result<bool> {
    let pidfd = match open_linux_pidfd(identity.process_id) {
        Ok(pidfd) => pidfd,
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => return Ok(false),
        Err(error) => return Err(error),
    };
    let outcome = (|| {
        if !linux_identity_is_live(identity)? {
            return Ok(false);
        }
        signal_linux_pidfd(pidfd, signal)
    })();
    unsafe { libc::close(pidfd) };
    outcome
}

#[cfg(target_os = "linux")]
fn open_linux_pidfd(process_id: u32) -> io::Result<libc::c_int> {
    let process_id = checked_process_id(process_id)?;
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, process_id, 0) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        libc::c_int::try_from(fd)
            .map_err(|_| io::Error::other("pidfd does not fit in file descriptor"))
    }
}

#[cfg(target_os = "linux")]
fn signal_linux_pidfd(pidfd: libc::c_int, signal: libc::c_int) -> io::Result<bool> {
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd,
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    if result == 0 {
        Ok(true)
    } else {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(false)
        } else {
            Err(error)
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_pidfd_is_live(pidfd: &std::os::fd::OwnedFd) -> io::Result<bool> {
    use std::os::fd::AsRawFd;
    signal_linux_pidfd(pidfd.as_raw_fd(), 0)
}

#[cfg(target_os = "linux")]
fn signal_linux_owned_root(
    root: &PtyProcessIdentity,
    observed: LinuxProcess,
    signal: libc::c_int,
) -> io::Result<bool> {
    use std::os::fd::AsRawFd;
    if observed.start_ticks != root.start_ticks || !linux_pidfd_is_live(&root.pidfd)? {
        return Ok(false);
    }
    signal_linux_pidfd(root.pidfd.as_raw_fd(), signal)
}

/// Lowers a child Git process without changing the interactive server's own
/// scheduling priority. Best effort: a scheduling restriction must not make a
/// valid Git operation fail. The child also inherits an already-lower priority.
#[cfg(target_os = "linux")]
pub fn lower_background_child_priority(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // The pre-exec closure only makes direct, allocation-free system calls.
    // Git's own hooks and filter-driver descendants inherit these settings.
    unsafe {
        command.pre_exec(|| {
            let _ = libc::setpriority(libc::PRIO_PROCESS, 0, 19);
            const IOPRIO_WHO_PROCESS: libc::c_long = 1;
            const IOPRIO_CLASS_IDLE: libc::c_long = 3;
            const IOPRIO_CLASS_SHIFT: libc::c_long = 13;
            let _ = libc::syscall(
                libc::SYS_ioprio_set,
                IOPRIO_WHO_PROCESS,
                0,
                IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT,
            );
            Ok(())
        });
    }
}

#[cfg(not(target_os = "linux"))]
pub fn lower_background_child_priority(_command: &mut Command) {}

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
        // Darwin reports EPERM, not ESRCH, when every remaining member of the
        // group is an exited-but-unreaped zombie. The group was created by this
        // guard's own child, so there is nothing left to kill.
        if cfg!(target_os = "macos") && error.raw_os_error() == Some(libc::EPERM) {
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

/// Observe a direct setup child without reaping it. Its PID continues to
/// reserve the process-group ID until the supervisor has signalled the group.
#[cfg(target_os = "linux")]
pub fn workspace_setup_child_exited_without_reaping(process_id: u32) -> io::Result<bool> {
    let process_id = checked_process_id(process_id)?;
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    // SAFETY: `info` is valid output storage and the checked PID names our
    // direct child. WNOHANG does not block and WNOWAIT does not reap it.
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            process_id as libc::id_t,
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: Linux initializes siginfo on success; a zero si_pid means no
    // waitable state change under WNOHANG.
    let info = unsafe { info.assume_init() };
    Ok(unsafe { info.si_pid() } != 0)
}

/// Verify that a signalled process group has exited. This only observes;
/// it never signals a group whose numeric ID might have been reused.
#[cfg(unix)]
pub fn wait_for_process_group_exit(
    process_id: u32,
    timeout: std::time::Duration,
) -> io::Result<()> {
    let process_group_id = checked_process_id(process_id)?;
    let started = std::time::Instant::now();
    loop {
        // SAFETY: signal zero is an existence check for the checked group ID.
        let result = unsafe { libc::kill(-process_group_id, 0) };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            return Err(error);
        }
        if started.elapsed() >= timeout {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "process group has not been observed to exit",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
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

    #[cfg(target_os = "linux")]
    #[test]
    fn proc_stat_parser_handles_parenthesized_command_names() {
        let stat = "42 (a strange ) name) S 10 11 12 0 0 0 0 0 0 0 0 0 0 0 0 0 1 0 12345 0";
        let process = parse_linux_stat(42, stat).expect("parse stat");
        assert_eq!(process.parent_id, 10);
        assert_eq!(process.session_id, 12);
        assert_eq!(process.start_ticks, 12345);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stale_pty_birth_identity_cannot_signal_a_live_process() {
        let identity = capture_pty_process(std::process::id()).expect("self identity");
        let stale = PtyProcessIdentity {
            start_ticks: identity.start_ticks.saturating_add(1),
            ..identity
        };
        let error = terminate_pty_process_tree(&stale, Duration::from_millis(100))
            .expect_err("stale identity must fail");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(is_running(std::process::id()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn directory_use_scan_has_component_boundary() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path().join("worktree");
        let sibling = directory.path().join("worktree-other");
        std::fs::create_dir(&root).expect("root");
        std::fs::create_dir(&sibling).expect("sibling");
        let mut child = Command::new("/bin/sleep")
            .arg("60")
            .current_dir(&root)
            .spawn()
            .expect("sleep in root");
        let root_users = processes_using_directory(&root);
        let sibling_users = processes_using_directory(&sibling);
        child.kill().expect("kill child");
        child.wait().expect("reap child");
        // A same-user process can disallow /proc cwd inspection (for example
        // systemd --user). That must block deletion rather than produce a
        // partial empty list. The path boundary is asserted when the host
        // permits a complete scan.
        match (root_users, sibling_users) {
            (Ok(root_users), Ok(sibling_users)) => {
                assert!(root_users.contains(&child.id()));
                assert!(!sibling_users.contains(&child.id()));
            }
            (Err(error), _) | (_, Err(error)) => {
                assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            }
        }
    }

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
    #[cfg(unix)]
    #[test]
    fn workspace_repository_lease_is_exclusive_and_released_on_drop() {
        let directory = tempfile::tempdir().expect("lease directory");
        let root = std::fs::canonicalize(directory.path()).unwrap();
        let first = try_workspace_repository_lease(&root)
            .unwrap()
            .expect("first lease");
        assert!(try_workspace_repository_lease(&root).unwrap().is_none());
        drop(first);
        let second = try_workspace_repository_lease(&root)
            .unwrap()
            .expect("released lease");
        assert!(root.join("ilium-workspace-operations.lock").is_file());
        drop(second);
        assert!(root.join("ilium-workspace-operations.lock").is_file());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn worktree_fd_is_detected_even_when_cwd_is_elsewhere() {
        let directory = tempfile::tempdir().expect("descriptor directory");
        let root = std::fs::canonicalize(directory.path()).unwrap();
        let file = std::fs::File::create(root.join("in-use")).unwrap();
        assert!(linux_process_has_worktree_fd(
            std::process::id(),
            &root,
            Instant::now() + Duration::from_secs(3)
        )
        .unwrap());
        drop(file);
        assert!(!linux_process_has_worktree_fd(
            std::process::id(),
            &root,
            Instant::now() + Duration::from_secs(3)
        )
        .unwrap());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn direct_child_observation_leaves_exit_status_waitable() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 17"]);
        prepare_process_tree(&mut command);
        let mut child = command.spawn().unwrap();
        let process_id = child.id();
        let mut guard = ProcessTreeGuard::attach(process_id).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !child_exited_without_reaping(process_id).unwrap() {
            assert!(Instant::now() < deadline, "child did not exit");
            std::thread::sleep(Duration::from_millis(5));
        }
        guard.terminate().unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(17));
        wait_for_group_exit(process_id, Duration::from_secs(3)).unwrap();
    }
}
