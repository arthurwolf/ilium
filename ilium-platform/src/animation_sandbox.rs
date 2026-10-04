//! OS authority boundary for unverified animation helpers.
//!
//! Linux requires a private bubblewrap namespace, a delegated cgroup-v2 memory
//! and task bound, inherited-descriptor scrubbing, and a process-wide seccomp
//! allowlist installed *after trusted V8 bootstrap but before package evaluation*.
//! Other OSes fail closed until their native sandbox has equivalent qualification.
//! A V8 heap limit is not an RSS limit, and V8's virtual reservations must not be
//! constrained with a small RLIMIT_AS. The Linux physical bound is memory.max.

use std::{
    io,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
};

/// Resolve the separately installed helper beside the real client executable.
/// The platform suffix is part of its filename; supported sandbox qualification
/// remains a separate check at launch.
pub fn helper_executable_path(client_executable: &Path) -> io::Result<PathBuf> {
    let parent = client_executable.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "client executable directory unavailable",
        )
    })?;
    Ok(parent.join(format!(
        "ilium-animation-helper{}",
        std::env::consts::EXE_SUFFIX
    )))
}

#[derive(Debug, Clone, Copy)]
pub struct SandboxLimits {
    pub memory_bytes: u64,
    pub maximum_tasks: u32,
    pub cpu_seconds: u64,
}
impl Default for SandboxLimits {
    fn default() -> Self {
        Self {
            memory_bytes: 384 * 1024 * 1024,
            maximum_tasks: 16,
            cpu_seconds: 86_400,
        }
    }
}
impl SandboxLimits {
    fn validate(self) -> io::Result<Self> {
        if !(64 * 1024 * 1024..=4 * 1024 * 1024 * 1024).contains(&self.memory_bytes)
            || !(4..=64).contains(&self.maximum_tasks)
            || !(1..=86_400).contains(&self.cpu_seconds)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "animation sandbox limits outside supported bounds",
            ));
        }
        Ok(self)
    }
}

/// A successfully installed barrier, never an assertion inferred from a flag.
#[derive(Debug, Clone, Copy)]
pub struct SealProof {
    pub all_threads_filtered: bool,
    pub descriptors_scrubbed: bool,
    pub direct_filesystem_denied: bool,
    pub direct_network_denied: bool,
    pub subprocesses_denied: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SandboxUsage {
    pub maximum_tasks: u32,
    pub current_tasks: u32,
    pub peak_tasks: Option<u32>,
    pub task_limit_events: u64,
    pub maximum_memory_bytes: u64,
    pub memory_limit_events: u64,
    pub memory_oom_kill_events: u64,
    pub current_memory_bytes: u64,
    /// `memory.peak` was introduced after Linux 5.15. Absence is not zero or
    /// an estimate; `memory.max` and `memory.events` remain the hard evidence.
    pub peak_memory_bytes: Option<u64>,
}

/// Direct child and its exclusively owned kernel resource domain. Call shutdown
/// from an owned worker, not the UI thread. Drop also kills/reaps that exact child.
pub struct SandboxChild {
    child: Child,
    #[cfg(target_os = "linux")]
    cgroup: std::sync::Arc<LinuxCgroup>,
    reaped: bool,
}
impl SandboxChild {
    #[cfg(target_os = "linux")]
    pub fn resource_usage(&self) -> io::Result<SandboxUsage> {
        self.cgroup.resource_usage()
    }
    #[cfg(not(target_os = "linux"))]
    pub fn resource_usage(&self) -> io::Result<SandboxUsage> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            unqualified_reason(),
        ))
    }
    pub fn id(&self) -> u32 {
        self.child.id()
    }
    pub fn cancel_handle(&self) -> SandboxCancel {
        SandboxCancel {
            #[cfg(target_os = "linux")]
            cgroup: std::sync::Arc::clone(&self.cgroup),
        }
    }
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let result = self.child.try_wait()?;
        if result.is_some() {
            self.reaped = true;
        }
        Ok(result)
    }
    /// Owned background reader only: observe actual natural process exit, then
    /// terminate any descendants remaining in this private resource domain.
    pub fn wait_for_exit(&mut self) -> io::Result<ExitStatus> {
        let status = self.child.wait()?;
        self.reaped = true;
        #[cfg(target_os = "linux")]
        self.cgroup.kill()?;
        Ok(status)
    }
    pub fn shutdown(&mut self) -> io::Result<ExitStatus> {
        #[cfg(target_os = "linux")]
        let domain_result = self.cgroup.kill();
        if !self.reaped {
            // Child is task-owned and its identity has not been released/reused.
            let _ = self.child.kill();
        }
        let status = self.child.wait();
        self.reaped = status.is_ok();
        #[cfg(target_os = "linux")]
        domain_result?;
        let status = status?;
        Ok(status)
    }
}
impl Drop for SandboxChild {
    fn drop(&mut self) {
        // A reaped launcher may still have descendants in its owned domain.
        #[cfg(target_os = "linux")]
        let _ = self.cgroup.kill();
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.reaped = true;
        }
    }
}

/// Cloneable wake capability for this exact owned domain. Its retained cgroup
/// guard prevents path removal/reuse while a blocked pipe worker still owns it.
#[derive(Clone)]
pub struct SandboxCancel {
    #[cfg(target_os = "linux")]
    cgroup: std::sync::Arc<LinuxCgroup>,
}
impl SandboxCancel {
    #[cfg(target_os = "linux")]
    pub fn resource_usage(&self) -> io::Result<SandboxUsage> {
        self.cgroup.resource_usage()
    }
    #[cfg(not(target_os = "linux"))]
    pub fn resource_usage(&self) -> io::Result<SandboxUsage> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            unqualified_reason(),
        ))
    }
    #[cfg(target_os = "linux")]
    pub fn terminate(&self) -> io::Result<()> {
        self.cgroup.kill()
    }
    #[cfg(not(target_os = "linux"))]
    pub fn terminate(&self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            unqualified_reason(),
        ))
    }
}

#[cfg(target_os = "linux")]
struct LinuxCgroup {
    path: PathBuf,
    directory: std::fs::File,
}
#[cfg(target_os = "linux")]
impl LinuxCgroup {
    fn open(&self, name: &str, flags: libc::c_int) -> io::Result<std::fs::File> {
        use std::os::fd::{AsRawFd, FromRawFd};
        let name = std::ffi::CString::new(name)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid cgroup file name"))?;
        // Open relative to the retained cgroup directory descriptor. Replacing
        // the pathname cannot redirect kill or accounting to another domain.
        // SAFETY: both the directory descriptor and NUL-terminated name live
        // through the call, and a successful descriptor is owned by File.
        let descriptor = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { std::fs::File::from_raw_fd(descriptor) })
    }

    fn read(&self, name: &str) -> io::Result<String> {
        use std::io::Read;
        let mut value = String::new();
        self.open(name, libc::O_RDONLY)?
            .read_to_string(&mut value)?;
        Ok(value)
    }

    fn write(&self, name: &str, value: &[u8]) -> io::Result<()> {
        use std::io::Write;
        self.open(name, libc::O_WRONLY)?.write_all(value)
    }

    fn unified_membership_path(membership: &str) -> io::Result<&Path> {
        use std::path::Component;
        let mut unified = membership
            .lines()
            .filter_map(|line| line.strip_prefix("0::"));
        let relative = unified.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "unified cgroup-v2 membership is absent",
            )
        })?;
        if unified.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "duplicate unified cgroup membership",
            ));
        }
        let relative = Path::new(relative);
        if !relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::RootDir | Component::Normal(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid unified cgroup membership path",
            ));
        }
        Ok(relative)
    }

    fn current_cgroup() -> io::Result<PathBuf> {
        use std::path::Component;
        let membership = std::fs::read_to_string("/proc/self/cgroup")?;
        let relative = Self::unified_membership_path(&membership)?;
        let mut path = PathBuf::from("/sys/fs/cgroup");
        for component in relative.components() {
            if let Component::Normal(name) = component {
                path.push(name);
            }
        }
        if !path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "current cgroup is not visible under /sys/fs/cgroup",
            ));
        }
        Ok(path)
    }

    fn delegated_parent() -> io::Result<PathBuf> {
        let mount = Path::new("/sys/fs/cgroup");
        let current = Self::current_cgroup()?;
        for candidate in current
            .ancestors()
            .take_while(|path| path.starts_with(mount))
        {
            let Ok(enabled) = std::fs::read_to_string(candidate.join("cgroup.subtree_control"))
            else {
                continue;
            };
            if !["memory", "pids", "cpu"].iter().all(|name| {
                enabled
                    .split_whitespace()
                    .any(|controller| controller == *name)
            }) {
                continue;
            }
            // Migrating a child from our present leaf to a sibling requires
            // write authority over their common ancestor's cgroup.procs.
            if std::fs::OpenOptions::new()
                .write(true)
                .open(candidate.join("cgroup.procs"))
                .is_ok()
            {
                return Ok(candidate.to_path_buf());
            }
        }
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "animation sandbox needs an owned delegated cgroup-v2 ancestor with memory, pids and cpu enabled",
        ))
    }

    fn resource_usage(&self) -> io::Result<SandboxUsage> {
        fn number<T: std::str::FromStr>(group: &LinuxCgroup, name: &str) -> io::Result<T> {
            group.read(name)?.trim().parse().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid sandbox resource counter",
                )
            })
        }
        fn event(group: &LinuxCgroup, file: &str, key: &str) -> io::Result<u64> {
            let events = group.read(file)?;
            events
                .lines()
                .filter_map(|line| line.split_once(' '))
                .find_map(|(name, count)| (name == key).then_some(count))
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing cgroup event"))?
                .parse()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid cgroup event"))
        }
        Ok(SandboxUsage {
            maximum_tasks: number(self, "pids.max")?,
            current_tasks: number(self, "pids.current")?,
            peak_tasks: match number(self, "pids.peak") {
                Ok(value) => Some(value),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            },
            task_limit_events: event(self, "pids.events", "max")?,
            maximum_memory_bytes: number(self, "memory.max")?,
            memory_limit_events: event(self, "memory.events", "max")?,
            memory_oom_kill_events: event(self, "memory.events", "oom_kill")?,
            current_memory_bytes: number(self, "memory.current")?,
            peak_memory_bytes: match number(self, "memory.peak") {
                Ok(value) => Some(value),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            },
        })
    }
    fn create(limits: SandboxLimits) -> io::Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        // A fixed user@UID.service path can be outside this process's own
        // cgroup branch. The kernel requires authority at the common ancestor
        // before a child can migrate across siblings. Never alter a parent.
        let root = Self::delegated_parent()?;
        // SAFETY: getpid only reads this process's kernel identity.
        let process_id = unsafe { libc::getpid() };
        let path = root.join(format!(
            "ilium-animation-{process_id}-{}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?; // Exclusive creation: never adopt another domain.
        let directory = match std::fs::File::open(&path) {
            Ok(directory) => directory,
            Err(error) => {
                let _ = std::fs::remove_dir(&path);
                return Err(error);
            }
        };
        let group = Self { path, directory };
        group.write("memory.max", limits.memory_bytes.to_string().as_bytes())?;
        group.write("memory.swap.max", b"0")?;
        group.write("memory.oom.group", b"1")?;
        group.write("pids.max", limits.maximum_tasks.to_string().as_bytes())?;
        group.write("cpu.max", b"100000 100000")?;
        Ok(group)
    }
    fn kill(&self) -> io::Result<()> {
        use std::time::{Duration, Instant};
        self.write("cgroup.kill", b"1")?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let events = self.read("cgroup.events")?;
            if events.lines().any(|line| line == "populated 0") {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "owned animation cgroup did not drain after cgroup.kill",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod cgroup_path_tests {
    use super::LinuxCgroup;
    use std::path::Path;

    #[test]
    fn unified_membership_rejects_ambiguous_or_escaping_paths() {
        assert_eq!(
            LinuxCgroup::unified_membership_path("0::/user.slice/client.scope\n")
                .expect("valid path"),
            Path::new("/user.slice/client.scope")
        );
        for invalid in [
            "1:name=/legacy\n",
            "0::relative\n",
            "0::/../sibling\n",
            "0::/one\n0::/two\n",
        ] {
            assert!(LinuxCgroup::unified_membership_path(invalid).is_err());
        }
    }
}
#[cfg(target_os = "linux")]
impl Drop for LinuxCgroup {
    fn drop(&mut self) {
        // Exact newly created domain only; no recursive deletion or parent edit.
        let _ = std::fs::remove_dir(&self.path);
    }
}

/// Spawn only the trusted helper executable. The package travels over bounded
/// stdin IPC; no package path, credentials, home directory or host socket is mounted.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub fn spawn_helper(executable: &Path, limits: SandboxLimits) -> io::Result<SandboxChild> {
    spawn_confined(
        executable,
        &["--ipc".into()],
        "/ilium-animation-helper",
        limits,
    )
}
/// Trusted native decoder only: stdin/output pipe authority and the same private
/// namespace/cgroup custody as the helper. Arguments are host-authored, never
/// script strings. Libraries may read the immutable runtime mounts; this is
/// namespace/resource isolation, not the helper's post-bootstrap seccomp seal.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub fn spawn_video_decoder(
    executable: &Path,
    arguments: &[String],
    limits: SandboxLimits,
) -> io::Result<SandboxChild> {
    if arguments.len() > 128
        || arguments
            .iter()
            .any(|arg| arg.len() > 4096 || arg.contains('\0'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid trusted decoder arguments",
        ));
    }
    spawn_confined(executable, arguments, "/ilium-video-decoder", limits)
}
#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
pub fn spawn_video_decoder(
    _executable: &Path,
    _arguments: &[String],
    _limits: SandboxLimits,
) -> io::Result<SandboxChild> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        unqualified_reason(),
    ))
}
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn spawn_confined(
    executable: &Path,
    arguments: &[String],
    mounted_executable: &str,
    limits: SandboxLimits,
) -> io::Result<SandboxChild> {
    use std::os::unix::process::CommandExt;
    let limits = limits.validate()?;
    let executable = executable.canonicalize()?;
    if !executable.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "animation helper is not a regular file",
        ));
    }
    if !Path::new("/usr/bin/bwrap").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Linux animation sandbox requires /usr/bin/bwrap",
        ));
    }
    let cgroup = std::sync::Arc::new(LinuxCgroup::create(limits)?);
    use std::os::fd::AsRawFd;
    let membership = cgroup.open("cgroup.procs", libc::O_WRONLY)?;
    let membership_descriptor = membership.as_raw_fd();
    let mut command = Command::new("/usr/bin/bwrap");
    command.env_clear().args([
        // --unshare-all silently treats user and cgroup namespaces as optional.
        // Require each namespace: an unavailable kernel prerequisite must
        // prevent the helper from starting, not weaken its authority boundary.
        "--unshare-user",
        "--unshare-ipc",
        "--unshare-pid",
        "--unshare-net",
        "--unshare-uts",
        "--unshare-cgroup",
        "--as-pid-1",
        "--die-with-parent",
        "--new-session",
        "--cap-drop",
        "ALL",
        "--clearenv",
        "--setenv",
        "LANG",
        "C",
        "--setenv",
        "LC_ALL",
        "C",
        "--ro-bind",
        "/usr",
        "/usr",
        "--tmpfs",
        "/tmp",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
    ]);
    for library_root in ["/lib", "/lib64"] {
        // AArch64 distributions need not have /lib64. Bind only roots that
        // actually exist; /usr remains the mandatory runtime root.
        if Path::new(library_root).exists() {
            command.args(["--ro-bind", library_root, library_root]);
        }
    }
    command
        .arg("--ro-bind")
        .arg(&executable)
        .args([mounted_executable, "--ro-bind"])
        .arg(&cgroup.path)
        .args([
            "/ilium-sandbox-limits",
            "--chdir",
            "/tmp",
            "--",
            mounted_executable,
        ])
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: forked-child closure uses only async-signal-safe OS primitives.
    // The preopened cgroup descriptor names this exact newly created domain;
    // numeric limits and descriptor are captured before fork.
    unsafe {
        command.pre_exec(move || {
            // "0" means this child; joining before exec bounds bwrap and every descendant.
            let result = libc::write(membership_descriptor, b"0".as_ptr().cast(), 1);
            if result != 1 {
                return Err(io::Error::last_os_error());
            }
            let core = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            let cpu = libc::rlimit {
                rlim_cur: limits.cpu_seconds,
                rlim_max: limits.cpu_seconds,
            };
            if libc::setrlimit(libc::RLIMIT_CORE, &core) != 0
                || libc::setrlimit(libc::RLIMIT_CPU, &cpu) != 0
            {
                return Err(io::Error::last_os_error());
            }
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            // CLOEXEC preserves Rust's exec-error descriptor until successful exec,
            // while preventing arbitrary caller descriptors reaching bwrap/helper.
            if libc::syscall(libc::SYS_close_range, 3_u32, u32::MAX, 4_u32) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn()?;
    drop(membership);
    Ok(SandboxChild {
        child,
        cgroup,
        reaped: false,
    })
}
#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
pub fn spawn_helper(_executable: &Path, _limits: SandboxLimits) -> io::Result<SandboxChild> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        unqualified_reason(),
    ))
}

pub fn unqualified_reason() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows animation helper AppContainer/Job isolation is not qualified; unverified packages are disabled"
    } else if cfg!(target_os = "macos") {
        "macOS animation helper Seatbelt/resource isolation is not qualified; unverified packages are disabled"
    } else {
        "animation helper isolation requires qualified Linux x86_64/aarch64 bubblewrap, delegated cgroup-v2 and TSYNC seccomp"
    }
}

/// Verify the actual kernel domain from inside the namespace, before sealing.
/// A directly invoked helper must fail instead of trusting an "isolated" flag.
#[cfg(target_os = "linux")]
pub fn verify_helper_environment(limits: SandboxLimits) -> io::Result<()> {
    let limits = limits.validate()?;
    // SAFETY: this helper's namespace identity is read-only.
    if unsafe { libc::getpid() } != 1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "animation helper must own a private PID namespace as PID1",
        ));
    }
    for descriptor in [0, 1] {
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: fstat initializes a correctly sized structure on success.
        if unsafe { libc::fstat(descriptor, metadata.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let metadata = unsafe { metadata.assume_init() };
        if metadata.st_mode & libc::S_IFMT != libc::S_IFIFO {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "helper broker descriptor is not a private pipe",
            ));
        }
    }
    let status = std::fs::read_to_string("/proc/self/status")?;
    if !status.lines().any(|line| line == "NoNewPrivs:\t1")
        || !status
            .lines()
            .any(|line| line == "CapEff:\t0000000000000000")
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "helper retained privilege acquisition or capabilities",
        ));
    }
    let directory = std::ffi::CString::new("/ilium-sandbox-limits")
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "sandbox limits path"))?;
    let mut filesystem = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: valid path and correctly sized output initialized by successful syscall.
    if unsafe { libc::statvfs(directory.as_ptr(), filesystem.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let filesystem = unsafe { filesystem.assume_init() };
    if filesystem.f_flag & libc::ST_RDONLY == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "helper cgroup verification mount is writable",
        ));
    }
    let root = Path::new("/ilium-sandbox-limits");
    let membership = std::fs::read_to_string("/proc/self/cgroup")?;
    if !membership.lines().any(|line| line == "0::/")
        || !std::fs::read_to_string(root.join("cgroup.procs"))?
            .lines()
            .any(|line| line == "1")
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "helper is not in its mounted owned cgroup domain",
        ));
    }
    let memory = std::fs::read_to_string(root.join("memory.max"))?
        .trim()
        .parse::<u64>()
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "sandbox memory.max is not finite",
            )
        })?;
    let tasks = std::fs::read_to_string(root.join("pids.max"))?
        .trim()
        .parse::<u32>()
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "sandbox pids.max is not finite",
            )
        })?;
    let swap = std::fs::read_to_string(root.join("memory.swap.max"))?;
    let cpu = std::fs::read_to_string(root.join("cpu.max"))?;
    if memory > limits.memory_bytes
        || tasks > limits.maximum_tasks
        || swap.trim() != "0"
        || cpu.trim() != "100000 100000"
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "kernel sandbox resource bounds do not match the accepted limits",
        ));
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
pub fn verify_helper_environment(_limits: SandboxLimits) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        unqualified_reason(),
    ))
}

/// Irreversible all-thread syscall barrier. Call only inside an owned helper,
/// after its package and trusted engine state are resident and before scripts.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub fn seal_current_helper() -> io::Result<SealProof> {
    use libc::{sock_filter, sock_fprog};
    const LOAD: u16 = 0x20;
    const EQ: u16 = 0x15;
    const AND: u16 = 0x54;
    const RET: u16 = 0x06;
    const ALLOW: u32 = 0x7fff_0000;
    const KILL: u32 = 0x8000_0000;
    const DENY: u32 = 0x0005_0000 | libc::EPERM as u32;
    #[cfg(target_arch = "x86_64")]
    const ARCH: u32 = 0xc000_003e;
    #[cfg(target_arch = "aarch64")]
    const ARCH: u32 = 0xc000_00b7;
    fn statement(code: u16, k: u32) -> sock_filter {
        sock_filter {
            code,
            jt: 0,
            jf: 0,
            k,
        }
    }
    fn jump(code: u16, k: u32, jt: u8, jf: u8) -> sock_filter {
        sock_filter { code, jt, jf, k }
    }
    fn rule(code: &mut Vec<sock_filter>, syscall: libc::c_long, body: &[sock_filter]) {
        code.push(jump(EQ, syscall as u32, 0, body.len() as u8));
        code.extend_from_slice(body);
    }
    // Close inherited/non-protocol descriptors before installing the barrier.
    // All later file acquisition, duplication, socket creation and descriptor
    // transfer syscalls are denied. Only broker read0/write1 and diagnostic write2 survive.
    // SAFETY: closes only this exclusively owned helper's nonstandard descriptors.
    if unsafe { libc::syscall(libc::SYS_close_range, 3_u32, u32::MAX, 0_u32) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: changes this helper's privilege acquisition/dump policy only.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0
        || unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut code = vec![
        statement(LOAD, 4),
        jump(EQ, ARCH, 1, 0),
        statement(RET, KILL),
        statement(LOAD, 0),
    ];
    // read is solely from parent broker; write solely to its pipes. BPF checks
    // the complete 64-bit descriptor rather than accepting truncated aliases.
    rule(
        &mut code,
        libc::SYS_read,
        &[
            statement(LOAD, 20),
            jump(EQ, 0, 0, 3),
            statement(LOAD, 16),
            jump(EQ, 0, 0, 1),
            statement(RET, ALLOW),
            statement(RET, DENY),
        ],
    );
    for call in [libc::SYS_write, libc::SYS_writev] {
        rule(
            &mut code,
            call,
            &[
                statement(LOAD, 20),
                jump(EQ, 0, 0, 4),
                statement(LOAD, 16),
                jump(EQ, 1, 1, 0),
                jump(EQ, 2, 0, 1),
                statement(RET, ALLOW),
                statement(RET, DENY),
            ],
        );
    }
    rule(
        &mut code,
        libc::SYS_mmap,
        &[
            statement(LOAD, 40),
            statement(AND, libc::MAP_ANONYMOUS as u32),
            jump(EQ, libc::MAP_ANONYMOUS as u32, 0, 3),
            statement(LOAD, 48),
            jump(EQ, u32::MAX, 0, 1),
            statement(RET, ALLOW),
            statement(RET, DENY),
        ],
    );
    let thread_flags = (libc::CLONE_VM
        | libc::CLONE_FS
        | libc::CLONE_FILES
        | libc::CLONE_SIGHAND
        | libc::CLONE_THREAD
        | libc::CLONE_SYSVSEM
        | libc::CLONE_SETTLS
        | libc::CLONE_PARENT_SETTID
        | libc::CLONE_CHILD_CLEARTID) as u32;
    let required = (libc::CLONE_VM | libc::CLONE_SIGHAND | libc::CLONE_THREAD) as u32;
    rule(
        &mut code,
        libc::SYS_clone,
        &[
            statement(LOAD, 20),
            jump(EQ, 0, 0, 7),
            statement(LOAD, 16),
            statement(AND, !thread_flags),
            jump(EQ, 0, 0, 4),
            statement(LOAD, 16),
            statement(AND, required),
            jump(EQ, required, 0, 1),
            statement(RET, ALLOW),
            statement(RET, DENY),
        ],
    );
    // clone3's flags live behind a pointer and cannot be checked by classic BPF.
    // ENOSYS makes libc use the filtered legacy pthread clone path.
    rule(
        &mut code,
        libc::SYS_clone3,
        &[statement(RET, 0x0005_0000 | libc::ENOSYS as u32)],
    );
    // Signal only this private process, never a parent or unrelated process.
    // SAFETY: getpid reads this helper's PID namespace identity before filtering.
    let process_id = unsafe { libc::getpid() } as u32;
    rule(
        &mut code,
        libc::SYS_tgkill,
        &[
            statement(LOAD, 20),
            jump(EQ, 0, 0, 3),
            statement(LOAD, 16),
            jump(EQ, process_id, 0, 1),
            statement(RET, ALLOW),
            statement(RET, DENY),
        ],
    );
    // V8 memory management, private synchronization, entropy and monotonic/civil
    // clocks. No pathname lookup, ioctl, socket, exec, tracing, descriptor transfer,
    // namespace change, cgroup access, process_vm access or io_uring is admitted.
    let calls = [
        libc::SYS_munmap,
        libc::SYS_mprotect,
        libc::SYS_madvise,
        libc::SYS_mremap,
        libc::SYS_brk,
        libc::SYS_futex,
        libc::SYS_sched_yield,
        libc::SYS_nanosleep,
        libc::SYS_clock_nanosleep,
        libc::SYS_clock_gettime,
        libc::SYS_getrandom,
        libc::SYS_getpid,
        libc::SYS_gettid,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigreturn,
        libc::SYS_sigaltstack,
        libc::SYS_set_robust_list,
        libc::SYS_rseq,
        libc::SYS_restart_syscall,
        libc::SYS_exit,
        libc::SYS_exit_group,
    ];
    for call in calls {
        rule(&mut code, call, &[statement(RET, ALLOW)]);
    }
    #[cfg(target_arch = "x86_64")]
    rule(&mut code, libc::SYS_arch_prctl, &[statement(RET, ALLOW)]);
    code.push(statement(RET, DENY));
    let program = sock_fprog {
        len: u16::try_from(code.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "seccomp program too large")
        })?,
        filter: code.as_mut_ptr(),
    };
    // TSYNC applies to V8 background/watchdog threads already running. A positive
    // return is a thread-id synchronization failure, not successful installation.
    // SAFETY: the kernel copies the immutable well-sized BPF program synchronously.
    let result = unsafe { libc::syscall(libc::SYS_seccomp, 1_u32, 1_u32, &program) };
    if result != 0 {
        return Err(if result > 0 {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "seccomp TSYNC could not seal all helper threads",
            )
        } else {
            io::Error::last_os_error()
        });
    }
    Ok(SealProof {
        all_threads_filtered: true,
        descriptors_scrubbed: true,
        direct_filesystem_denied: true,
        direct_network_denied: true,
        subprocesses_denied: true,
    })
}
#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
pub fn seal_current_helper() -> io::Result<SealProof> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        unqualified_reason(),
    ))
}

/// Actual hostile syscalls for an exclusively owned helper's qualification mode.
/// Every result must be denied; this is not a JavaScript absence-of-API test.
#[cfg(target_os = "linux")]
pub fn hostile_syscall_probe() -> io::Result<Vec<(&'static str, bool)>> {
    let mut result = Vec::new();
    let denied = |value: libc::c_long| {
        value == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    };
    let path = std::ffi::CString::new("/tmp/ilium-animation-forbidden-probe")
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "probe path"))?;
    let secret = std::ffi::CString::new("/etc/passwd")
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "probe path"))?;
    // SAFETY: initialized static buffers and paths, actual syscalls in this helper.
    unsafe {
        result.push((
            "open_read",
            denied(libc::open(secret.as_ptr(), libc::O_RDONLY) as libc::c_long),
        ));
        result.push((
            "open_write",
            denied(
                libc::open(path.as_ptr(), libc::O_CREAT | libc::O_WRONLY, 0o600) as libc::c_long,
            ),
        ));
        let mut data = [0_u8; 16];
        result.push((
            "read_nonbroker_fd",
            denied(libc::read(3, data.as_mut_ptr().cast(), data.len()) as libc::c_long),
        ));
        result.push((
            "write_nonbroker_fd",
            denied(libc::write(3, data.as_ptr().cast(), data.len()) as libc::c_long),
        ));
        result.push((
            "socket",
            denied(libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) as libc::c_long),
        ));
        result.push((
            "fork",
            denied(libc::syscall(
                libc::SYS_clone,
                libc::SIGCHLD,
                0_usize,
                0_usize,
                0_usize,
                0_usize,
            ) as libc::c_long),
        ));
        let executable = std::ffi::CString::new("/usr/bin/true")
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "probe executable"))?;
        let arguments = [executable.as_ptr(), std::ptr::null()];
        result.push((
            "exec",
            denied(libc::execv(executable.as_ptr(), arguments.as_ptr()) as libc::c_long),
        ));
        result.push((
            "metadata",
            denied(libc::access(secret.as_ptr(), libc::F_OK) as libc::c_long),
        ));
        result.push(("ioctl", denied(libc::ioctl(0, 0, 0) as libc::c_long)));
    }
    Ok(result)
}
#[cfg(not(target_os = "linux"))]
pub fn hostile_syscall_probe() -> io::Result<Vec<(&'static str, bool)>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        unqualified_reason(),
    ))
}
