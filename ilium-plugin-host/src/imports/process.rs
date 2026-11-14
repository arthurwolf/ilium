//! Host implementation of `ilium:host/process` (api-process-control.md).
//!
//! One-shot runs execute on a worker thread of the import; the exit is held until the host's next
//! event tick (`EXIT_DELIVERY_DELAY`), then `request-completed` is queued for the plugin and the
//! exit is handed out once by `take-exit`. Long-lived children are supervised by one thread each:
//! it queues `child-output` for every chunk (or line) and `child-exited` at the end. Grants
//! (`process.run:<program>`, `process.long-lived`) are checked here against the instance's grants.
//!
//! Everything belongs to the import value, which is owned by one plugin instance: its quotas never
//! leak into another instance, and dropping it (the instance stops) cancels the pending runs and
//! sends SIGTERM, then SIGKILL after 2 s, to the children. At most 16 processes run per session
//! (`SessionCounters`), shared by `run` and `spawn`.

use ilium_platform::process_control::{
    lower_background_child_priority, prepare_process_tree, terminate, ProcessTreeGuard,
};
use ilium_plugin_api::generated::host::process::{
    ChildHandle, HostProcess, ProcessExit, ProcessSpec, SpawnSpec,
};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::events::OutputStream;
use ilium_plugin_api::generated::types::events::{ChildExitedEvent, ChildOutputEvent, Event};
use ilium_plugin_api::generated::types::ids::{ChildId, RequestId};
use ilium_plugin_api::generated::types::paths::FsPath;
use ilium_plugin_api::generated::types::Outcome;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::env::{completion_event, ImportEnv, SessionSlot};
use crate::runtime::pending_events::EventSink;

/// Stdin and combined output cap, 4 MiB.
const FOUR_MIB: usize = 4 * 1024 * 1024;
/// Longest one-shot timeout.
const MAX_TIMEOUT_MILLIS: u32 = 600_000;
/// Most arguments per spec.
const MAX_ARGUMENTS: usize = 1_024;
/// Most extra environment pairs per spec.
const MAX_ENVIRONMENT_PAIRS: usize = 64;
/// Pending one-shot requests per plugin before `busy`.
const PENDING_REQUESTS_PER_PLUGIN: usize = 64;
/// Long-lived children per plugin.
const CHILDREN_PER_PLUGIN: usize = 2;
/// Processes (one-shot and long-lived) one session runs at once.
pub const SESSION_PROCESSES: usize = 16;
/// An exit becomes visible to `take-exit` only after the host's next event tick.
const EXIT_DELIVERY_DELAY: Duration = Duration::from_millis(50);
/// One `child-output` event carries at most this many bytes; a longer chunk or line is split.
const OUTPUT_CHUNK_BYTES: usize = 1024 * 1024;
/// Bytes read from a child pipe per call outside line mode.
const READ_BUFFER_BYTES: usize = 64 * 1024;
/// The reader stops reading a pipe while this many events wait, so a slow plugin slows the child
/// down instead of losing output.
const OUTPUT_BACKLOG_EVENTS: usize = 2_048;
/// How long a child gets between SIGTERM and SIGKILL when its plugin stops.
const TERMINATE_GRACE: Duration = Duration::from_secs(2);
/// Most exited children whose handles the import keeps.
const KEPT_DEAD_CHILDREN: usize = 64;
/// Environment keys a plugin may not set (SEC-033).
const RESERVED_ENVIRONMENT_KEYS: &[&str] = &[
    "PATH",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "IFS",
    "BASH_ENV",
    "ENV",
];

/// A finished run waiting for `take-exit`.
type ExitSlot = Arc<Mutex<Option<(Instant, ProcessExit, Option<ErrorCode>)>>>;

/// One one-shot run of the plugin that `take-exit` has not collected.
struct PendingRun {
    slot: ExitSlot,
    /// Set when the import drops: the worker kills the program and queues nothing.
    cancel: Arc<AtomicBool>,
}

/// What the import tells the supervisor of a long-lived child.
#[derive(Default)]
struct ChildControl {
    /// Set when the plugin stopped: SIGTERM now, SIGKILL after the grace period.
    stop: AtomicBool,
    /// Set by `signal("kill")`: SIGKILL the process group now.
    kill: AtomicBool,
    /// Set by the supervisor once the child was reaped.
    exited: AtomicBool,
}

/// One long-lived child.
struct ChildEntry {
    id: ChildId,
    pid: u32,
    stdin: Option<ChildStdin>,
    control: Arc<ChildControl>,
    supervisor: Option<JoinHandle<()>>,
}

impl ChildEntry {
    fn is_live(&self) -> bool {
        !self.control.exited.load(Ordering::Acquire)
    }
}

/// Host side of the `process` interface of one plugin instance.
pub struct ProcessImport {
    env: ImportEnv,
    next_child: u32,
    pending: HashMap<u64, PendingRun>,
    children: HashMap<u32, ChildEntry>,
    /// Folders behind the scopes a `cwd` may name (`project`, `data`, `cache`, `package`).
    scope_roots: HashMap<String, PathBuf>,
}

impl std::fmt::Debug for ProcessImport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessImport")
            .field("plugin", &self.env.plugin())
            .field("pending", &self.pending.len())
            .field("children", &self.children.len())
            .finish()
    }
}

impl Default for ProcessImport {
    fn default() -> Self {
        Self::new(ImportEnv::unrestricted())
    }
}

fn error(code: ErrorCode, message: impl Into<String>) -> PluginError {
    PluginError {
        code,
        message: message.into(),
        retry_after_ms: None,
    }
}

fn invalid(message: &str) -> PluginError {
    error(ErrorCode::InvalidArgument, message)
}

/// Checks the limits and reserved keys of a spec (api-process-control.md, Limits).
fn validate_spec(spec: &ProcessSpec, check_timeout: bool) -> Result<(), PluginError> {
    if spec.program.is_empty() {
        return Err(invalid("program is empty"));
    }
    if check_timeout && (spec.timeout_millis == 0 || spec.timeout_millis > MAX_TIMEOUT_MILLIS) {
        return Err(invalid("timeout-millis must be 1 to 600000"));
    }
    if spec.arguments.len() > MAX_ARGUMENTS {
        return Err(invalid("more than 1024 arguments"));
    }
    if spec.environment.len() > MAX_ENVIRONMENT_PAIRS {
        return Err(invalid("more than 64 environment pairs"));
    }
    if spec
        .stdin
        .as_ref()
        .is_some_and(|bytes| bytes.len() > FOUR_MIB)
    {
        return Err(invalid("stdin is larger than 4 MiB"));
    }
    for (key, _) in &spec.environment {
        let is_reserved = RESERVED_ENVIRONMENT_KEYS
            .iter()
            .any(|reserved| reserved.eq_ignore_ascii_case(key))
            || key.starts_with("LD_")
            || key.starts_with("DYLD_");
        if is_reserved {
            return Err(invalid("the environment key is reserved"));
        }
    }
    Ok(())
}

/// Builds the command of a spec: a clean environment (`PATH`, `HOME`, `LANG` and the plugin's
/// extras), the folder it runs in, and its own process group, so killing the group ends its
/// descendants too.
fn command_of(spec: &ProcessSpec, working_folder: Option<&Path>) -> Command {
    let mut command = Command::new(&spec.program);
    command.args(&spec.arguments);
    command.env_clear();
    for name in ["PATH", "HOME"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.env(
        "LANG",
        std::env::var_os("LANG").unwrap_or_else(|| "C".into()),
    );
    for (key, value) in &spec.environment {
        command.env(key, value);
    }
    if let Some(folder) = working_folder {
        command.current_dir(folder);
    }
    prepare_process_tree(&mut command);
    command
}

/// Reads a pipe into a buffer, stopping at `limit` bytes.
fn read_capped(mut pipe: impl Read, limit: usize) -> (Vec<u8>, bool) {
    let mut collected = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        match pipe.read(&mut buffer) {
            Ok(0) | Err(_) => return (collected, false),
            Ok(count) => {
                collected.extend_from_slice(&buffer[..count]);
                if collected.len() > limit {
                    collected.truncate(limit);
                    return (collected, true);
                }
            }
        }
    }
}

/// The refusal code of a program that could not start.
fn spawn_failure_code(spawn_error: &std::io::Error) -> ErrorCode {
    if spawn_error.kind() == std::io::ErrorKind::NotFound {
        ErrorCode::NotFound
    } else {
        ErrorCode::Io
    }
}

/// A one-shot program that started, with the guard of its process tree.
struct Started {
    child: Child,
    tree: Option<ProcessTreeGuard>,
}

/// Starts a one-shot program; a program that cannot start is a finished run with exit 127 and the
/// failure code of the completion (api-process-control.md: the call is accepted, the completion
/// is `failed(not-found)`).
fn start_one_shot(
    spec: &ProcessSpec,
    working_folder: Option<&Path>,
) -> Result<Started, (ProcessExit, ErrorCode)> {
    let mut command = command_of(spec, working_folder);
    command
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match command.spawn() {
        Ok(child) => {
            let tree = ProcessTreeGuard::attach(child.id()).ok();
            Ok(Started { child, tree })
        }
        Err(spawn_error) => Err((
            ProcessExit {
                status: Some(127),
                signal: None,
                stdout: Vec::new(),
                stderr: spawn_error.to_string().into_bytes(),
            },
            spawn_failure_code(&spawn_error),
        )),
    }
}

/// Runs a started one-shot program to completion, timeout or cancellation.
fn finish_one_shot(
    started: Started,
    spec: &ProcessSpec,
    cancel: &AtomicBool,
) -> (ProcessExit, Option<ErrorCode>) {
    let Started {
        mut child,
        mut tree,
    } = started;
    if let (Some(bytes), Some(mut stdin)) = (spec.stdin.clone(), child.stdin.take()) {
        std::thread::spawn(move || {
            // The program may exit without reading; a broken pipe is not a failure.
            let _ = stdin.write_all(&bytes);
        });
    }
    let stdout_reader = child
        .stdout
        .take()
        .map(|pipe| std::thread::spawn(move || read_capped(pipe, FOUR_MIB)));
    let stderr_reader = child
        .stderr
        .take()
        .map(|pipe| std::thread::spawn(move || read_capped(pipe, FOUR_MIB)));
    let deadline = Instant::now() + Duration::from_millis(u64::from(spec.timeout_millis));
    let mut timed_out = false;
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if cancel.load(Ordering::Acquire) => {
                kill_tree(&mut child, tree.as_mut());
                break child.wait().ok();
            }
            Ok(None) if Instant::now() >= deadline => {
                timed_out = true;
                kill_tree(&mut child, tree.as_mut());
                break child.wait().ok();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(2)),
            Err(_) => break None,
        }
    };
    let (stdout, stdout_capped) = stdout_reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let (stderr, stderr_capped) = stderr_reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let signal = if timed_out {
        Some("kill".to_owned())
    } else {
        exit_status.as_ref().and_then(signal_name_of)
    };
    let failure = if timed_out {
        Some(ErrorCode::TimedOut)
    } else if stdout_capped || stderr_capped {
        Some(ErrorCode::QuotaExceeded)
    } else {
        None
    };
    (
        ProcessExit {
            status: exit_status.and_then(|status| status.code()),
            signal,
            stdout,
            stderr,
        },
        failure,
    )
}

/// Kills the program and everything it started: the whole process group when the guard exists,
/// the program alone otherwise.
fn kill_tree(child: &mut Child, tree: Option<&mut ProcessTreeGuard>) {
    let group_killed = tree.is_some_and(|guard| guard.terminate().is_ok());
    if !group_killed {
        let _ = child.kill();
    }
}

/// The completion outcome of a finished one-shot run (`ok`, or `failed(code)` per
/// api-process-control.md).
fn outcome_of_failure(failure: Option<ErrorCode>) -> Outcome {
    match failure {
        None => Outcome::Ok,
        Some(code) => Outcome::Failed(error(code, "the process request failed")),
    }
}

/// The documented name of the signal that ended a process, if one did.
#[cfg(unix)]
fn signal_name_of(status: &std::process::ExitStatus) -> Option<String> {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(|number| {
        match number {
            1 => "hangup",
            2 => "interrupt",
            15 => "terminate",
            _ => "kill",
        }
        .to_owned()
    })
}

#[cfg(not(unix))]
fn signal_name_of(_status: &std::process::ExitStatus) -> Option<String> {
    None
}

/// Splits `bytes` into events of at most `OUTPUT_CHUNK_BYTES` and queues them, waiting while the
/// plugin's queue is backed up.
fn send_output(sink: &EventSink, child: ChildId, stream: OutputStream, bytes: &[u8]) {
    for chunk in bytes.chunks(OUTPUT_CHUNK_BYTES) {
        while sink.backlog() >= OUTPUT_BACKLOG_EVENTS {
            std::thread::sleep(Duration::from_millis(5));
        }
        sink.send(Event::ChildOutput(ChildOutputEvent {
            child,
            stream,
            bytes: chunk.to_vec(),
        }));
    }
}

/// Reads one pipe of a child to its end and queues what it prints, line by line in line mode.
fn pump_pipe(
    pipe: impl Read,
    sink: &EventSink,
    child: ChildId,
    stream: OutputStream,
    line_mode: bool,
) {
    if line_mode {
        let mut reader = BufReader::new(pipe);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => send_output(sink, child, stream, &line),
            }
        }
    }
    let mut pipe = pipe;
    let mut buffer = vec![0u8; READ_BUFFER_BYTES];
    loop {
        match pipe.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(count) => send_output(sink, child, stream, &buffer[..count]),
        }
    }
}

/// The supervisor of one long-lived child: it pumps the child's output, honours stop and kill
/// requests, reaps the child and queues `child-exited` once all output is queued.
fn supervise(
    mut child: Child,
    mut tree: Option<ProcessTreeGuard>,
    control: &ChildControl,
    sink: &EventSink,
    id: ChildId,
    line_mode: bool,
) {
    let readers: Vec<JoinHandle<()>> = [
        child
            .stdout
            .take()
            .map(|pipe| (Box::new(pipe) as Box<dyn Read + Send>, OutputStream::Stdout)),
        child
            .stderr
            .take()
            .map(|pipe| (Box::new(pipe) as Box<dyn Read + Send>, OutputStream::Stderr)),
    ]
    .into_iter()
    .flatten()
    .map(|(pipe, stream)| {
        let reader_sink = sink.clone();
        std::thread::spawn(move || pump_pipe(pipe, &reader_sink, id, stream, line_mode))
    })
    .collect();
    let mut terminated_at: Option<Instant> = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if control.kill.load(Ordering::Acquire) {
            kill_tree(&mut child, tree.as_mut());
        } else if control.stop.load(Ordering::Acquire) {
            match terminated_at {
                None => {
                    let _ = terminate(child.id());
                    terminated_at = Some(Instant::now());
                }
                Some(at) if at.elapsed() >= TERMINATE_GRACE => {
                    kill_tree(&mut child, tree.as_mut());
                }
                Some(_) => {}
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    // A descendant that outlives the child can keep a pipe open; the group kill ends it.
    if let Some(guard) = tree.as_mut() {
        let _ = guard.terminate();
    }
    for reader in readers {
        let _ = reader.join();
    }
    control.exited.store(true, Ordering::Release);
    if !control.stop.load(Ordering::Acquire) {
        sink.send(Event::ChildExited(ChildExitedEvent {
            child: id,
            status: status.as_ref().and_then(|status| status.code()),
            signal: status.as_ref().and_then(signal_name_of),
        }));
    }
}

impl ProcessImport {
    /// The import of the instance `env` describes.
    pub fn new(env: ImportEnv) -> Self {
        Self {
            env,
            next_child: 1,
            pending: HashMap::new(),
            children: HashMap::new(),
            scope_roots: HashMap::new(),
        }
    }

    /// Lets `cwd` name `scope`, a folder on disk. A `cwd` still needs `fs.read` for the scope
    /// unless the scope is `data`, `cache` or `package`, which are always readable.
    pub fn set_scope_root(&mut self, scope: &str, root: PathBuf) {
        self.scope_roots.insert(
            scope.to_owned(),
            std::fs::canonicalize(&root).unwrap_or(root),
        );
    }

    /// The real folder a `cwd` names, after the scope and path checks.
    fn working_folder(&self, cwd: &FsPath) -> Result<PathBuf, PluginError> {
        let always_readable = matches!(cwd.scope.as_str(), "data" | "cache" | "package");
        if !always_readable && !self.env.holds(&format!("fs.read:{}", cwd.scope)) {
            return Err(error(
                ErrorCode::NotPermitted,
                "the cwd lies outside a scope the plugin can read",
            ));
        }
        let root = self.scope_roots.get(&cwd.scope).ok_or_else(|| {
            error(
                ErrorCode::NotPermitted,
                "the cwd lies outside a scope the plugin can read",
            )
        })?;
        if cwd.path.starts_with('/') || cwd.path.contains('\\') || cwd.path.contains('\0') {
            return Err(error(
                ErrorCode::NotPermitted,
                "the cwd must be relative to its scope",
            ));
        }
        let mut folder = root.clone();
        for part in cwd.path.split('/') {
            match part {
                "" | "." => {}
                ".." => return Err(error(ErrorCode::NotPermitted, "the cwd leaves its scope")),
                name => folder.push(name),
            }
        }
        let canonical = std::fs::canonicalize(&folder)
            .map_err(|_| error(ErrorCode::NotFound, "the cwd does not exist"))?;
        if !canonical.starts_with(root) {
            return Err(error(
                ErrorCode::NotPermitted,
                "the cwd leaves its scope through a link",
            ));
        }
        Ok(canonical)
    }

    /// The grant check of one program: `process.run:<program>`.
    fn require_run_grant(&self, program: &str) -> Result<(), PluginError> {
        if self.env.holds(&format!("process.run:{program}")) {
            Ok(())
        } else {
            Err(error(
                ErrorCode::NotPermitted,
                "the plugin holds no process.run grant for this program",
            ))
        }
    }

    fn live_children(&self) -> usize {
        self.children
            .values()
            .filter(|entry| entry.is_live())
            .count()
    }

    /// Waits up to `budget` for a one-shot request to finish and maps it to its completion
    /// outcome (`ok`, or `failed(code)` per api-process-control.md); `None` when the request is
    /// unknown here or still running after the budget. The exit stays available for `take-exit`.
    pub fn await_outcome(&self, request: RequestId, budget: Duration) -> Option<Outcome> {
        let slot = Arc::clone(&self.pending.get(&request.0)?.slot);
        let deadline = Instant::now() + budget;
        loop {
            // Answer only once `take-exit` can hand the exit out.
            let finished = slot.lock().ok().and_then(|held| {
                held.as_ref().and_then(|(finished_at, _, failure)| {
                    (finished_at.elapsed() >= EXIT_DELIVERY_DELAY).then_some(*failure)
                })
            });
            if let Some(failure) = finished {
                return Some(outcome_of_failure(failure));
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Drops what no handle can reach any more: exited children beyond the kept few.
    fn prune_dead_children(&mut self) {
        let dead: Vec<u32> = self
            .children
            .iter()
            .filter(|(_, entry)| !entry.is_live())
            .map(|(handle, _)| *handle)
            .collect();
        if dead.len() <= KEPT_DEAD_CHILDREN {
            return;
        }
        let mut oldest_first = dead;
        oldest_first.sort_unstable();
        for handle in oldest_first.into_iter().take_while({
            let mut excess = self
                .children
                .values()
                .filter(|entry| !entry.is_live())
                .count();
            move |_| {
                let drop_this = excess > KEPT_DEAD_CHILDREN;
                excess = excess.saturating_sub(1);
                drop_this
            }
        }) {
            if let Some(mut entry) = self.children.remove(&handle) {
                if let Some(supervisor) = entry.supervisor.take() {
                    let _ = supervisor.join();
                }
            }
        }
    }
}

impl Drop for ProcessImport {
    fn drop(&mut self) {
        for run in self.pending.values() {
            run.cancel.store(true, Ordering::Release);
        }
        // The supervisors own the SIGTERM, grace period and SIGKILL; they are not joined here, so
        // a stopping instance never waits two seconds.
        for entry in self.children.values() {
            entry.control.stop.store(true, Ordering::Release);
        }
    }
}

impl HostProcess for ProcessImport {
    fn run(&mut self, spec: ProcessSpec) -> Result<RequestId, PluginError> {
        validate_spec(&spec, true)?;
        self.require_run_grant(&spec.program)?;
        let working_folder = match &spec.cwd {
            Some(cwd) => Some(self.working_folder(cwd)?),
            None => None,
        };
        if self.pending.len() >= PENDING_REQUESTS_PER_PLUGIN {
            return Err(error(
                ErrorCode::Busy,
                "64 process requests are already pending",
            ));
        }
        let Some(session_slot) = self.env.session().try_acquire_process(SESSION_PROCESSES) else {
            return Err(error(
                ErrorCode::Busy,
                "the session already runs 16 processes",
            ));
        };
        let request = self.env.next_request();
        let slot: ExitSlot = Arc::new(Mutex::new(None));
        let cancel = Arc::new(AtomicBool::new(false));
        // The program starts in this call, so the session cap counts real running programs and a
        // burst of calls is paced by the cost of starting them.
        let started = start_one_shot(&spec, working_folder.as_deref());
        let worker_slot = Arc::clone(&slot);
        let worker_cancel = Arc::clone(&cancel);
        let sink = self.env.sink();
        let worker = sink.worker();
        std::thread::spawn(move || {
            let _worker = worker;
            let (exit, failure) = match started {
                Ok(started) => finish_one_shot(started, &spec, &worker_cancel),
                Err((exit, code)) => (exit, Some(code)),
            };
            // The process is gone: its slot frees now, not after the delivery delay.
            drop(session_slot);
            if let Ok(mut held) = worker_slot.lock() {
                *held = Some((Instant::now(), exit, failure));
            }
            // `take-exit` answers only after the host's next event tick; the completion waits
            // for the same moment so the plugin never sees it before the exit is collectable.
            std::thread::sleep(EXIT_DELIVERY_DELAY);
            if !worker_cancel.load(Ordering::Acquire) {
                sink.send(completion_event(request, outcome_of_failure(failure)));
            }
        });
        self.pending.insert(request.0, PendingRun { slot, cancel });
        Ok(request)
    }

    fn take_exit(&mut self, request: RequestId) -> Option<ProcessExit> {
        let run = self.pending.get(&request.0)?;
        let ready = {
            let held = run.slot.lock().ok()?;
            held.as_ref()
                .is_some_and(|(finished_at, _, _)| finished_at.elapsed() >= EXIT_DELIVERY_DELAY)
        };
        if !ready {
            return None;
        }
        let run = self.pending.remove(&request.0)?;
        let exit = run.slot.lock().ok()?.take();
        exit.map(|(_, exit, _)| exit)
    }

    fn spawn(&mut self, spec: SpawnSpec) -> Result<ChildHandle, PluginError> {
        // Children have no timeout: the one-shot timeout is ignored.
        validate_spec(&spec.process, false)?;
        if !self.env.holds("process.long-lived") {
            return Err(error(
                ErrorCode::NotPermitted,
                "the plugin holds no process.long-lived grant",
            ));
        }
        self.require_run_grant(&spec.process.program)?;
        self.prune_dead_children();
        if self.live_children() >= CHILDREN_PER_PLUGIN {
            return Err(error(
                ErrorCode::QuotaExceeded,
                "a plugin may run 2 long-lived children",
            ));
        }
        let Some(session_slot) = self.env.session().try_acquire_process(SESSION_PROCESSES) else {
            return Err(error(
                ErrorCode::QuotaExceeded,
                "the session already runs 16 processes",
            ));
        };
        let working_folder = match &spec.process.cwd {
            Some(cwd) => Some(self.working_folder(cwd)?),
            None => None,
        };
        let mut command = command_of(&spec.process, working_folder.as_deref());
        command
            .stdin(if spec.stdin_open {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        lower_background_child_priority(&mut command);
        let mut child = command.spawn().map_err(|spawn_error| {
            error(
                spawn_failure_code(&spawn_error),
                format!("cannot start the program: {spawn_error}"),
            )
        })?;
        let tree = ProcessTreeGuard::attach(child.id()).ok();
        let stdin = child.stdin.take();
        let handle_number = self.next_child;
        self.next_child += 1;
        let id = ChildId(u64::from(handle_number));
        let pid = child.id();
        let control = Arc::new(ChildControl::default());
        let supervisor_control = Arc::clone(&control);
        let sink = self.env.sink();
        let worker = sink.worker();
        let line_mode = spec.line_mode;
        let supervisor = std::thread::spawn(move || {
            let _worker = worker;
            let _session_slot: SessionSlot = session_slot;
            supervise(child, tree, &supervisor_control, &sink, id, line_mode);
        });
        self.children.insert(
            handle_number,
            ChildEntry {
                id,
                pid,
                stdin,
                control,
                supervisor: Some(supervisor),
            },
        );
        Ok(ChildHandle(handle_number))
    }

    fn child_pid_hint(&mut self, handle: ChildHandle) -> u32 {
        self.children
            .get(&handle.0)
            .filter(|entry| entry.is_live())
            .map_or(0, |entry| entry.pid)
    }

    fn child_write_stdin(
        &mut self,
        handle: ChildHandle,
        bytes: Vec<u8>,
    ) -> Result<(), PluginError> {
        let entry = self
            .children
            .get_mut(&handle.0)
            .ok_or_else(|| error(ErrorCode::NotFound, "the child handle is not live"))?;
        let stdin = entry
            .stdin
            .as_mut()
            .ok_or_else(|| error(ErrorCode::Conflict, "stdin is closed"))?;
        stdin
            .write_all(&bytes)
            .and_then(|()| stdin.flush())
            .map_err(|write_error| {
                error(
                    ErrorCode::Conflict,
                    format!("stdin write failed: {write_error}"),
                )
            })
    }

    fn child_close_stdin(&mut self, handle: ChildHandle) {
        if let Some(entry) = self.children.get_mut(&handle.0) {
            entry.stdin = None;
        }
    }

    fn child_signal(&mut self, handle: ChildHandle, signal: String) -> Result<(), PluginError> {
        let signal_flag = match signal.as_str() {
            "interrupt" => "INT",
            "terminate" => "TERM",
            "hangup" => "HUP",
            "kill" => "KILL",
            _ => {
                return Err(invalid(
                    "signal must be interrupt, terminate, hangup or kill",
                ))
            }
        };
        let entry = self
            .children
            .get_mut(&handle.0)
            .ok_or_else(|| error(ErrorCode::NotFound, "the child handle is not live"))?;
        if !entry.is_live() {
            // A signal to a child that already exited is not an error: the outcome it asks for
            // already holds.
            return Ok(());
        }
        match signal_flag {
            "KILL" => {
                entry.control.kill.store(true, Ordering::Release);
                Ok(())
            }
            "TERM" => terminate(entry.pid).map_err(|terminate_error| {
                error(
                    ErrorCode::Unsupported,
                    format!("cannot deliver the signal: {terminate_error}"),
                )
            }),
            _ => {
                let delivered = Command::new("kill")
                    .arg(format!("-{signal_flag}"))
                    .arg(entry.pid.to_string())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                match delivered {
                    Ok(_) => Ok(()),
                    Err(kill_error) => Err(error(
                        ErrorCode::Unsupported,
                        format!("cannot deliver the signal: {kill_error}"),
                    )),
                }
            }
        }
    }

    fn child_id(&mut self, handle: ChildHandle) -> ChildId {
        self.children
            .get(&handle.0)
            .map_or(ChildId(u64::from(handle.0)), |entry| entry.id)
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessImport;
    use crate::testing::imports_fixture::ImportsFixture;
    use ilium_plugin_api::generated::host::process::{
        ChildHandle, HostProcess, ProcessSpec, SpawnSpec,
    };
    use ilium_plugin_api::generated::types::errors::PluginError;
    use ilium_plugin_api::generated::types::ids::RequestId;
    use ilium_plugin_api::Tier;

    /// 4 MiB, the stdin and output cap of `process.run` (api-process-control.md, Limits).
    const FOUR_MIB: usize = 4 * 1024 * 1024;
    /// Maximum `timeout_millis` of a one-shot run (api-process-control.md, Limits).
    const MAX_TIMEOUT_MILLIS: u32 = 600_000;
    /// Maximum argument count of one spec (api-process-control.md, Limits).
    const MAX_ARGUMENTS: usize = 1_024;
    /// Maximum extra environment pairs of one spec (api-process-control.md, Limits).
    const MAX_ENVIRONMENT_PAIRS: usize = 64;
    /// Pending requests per plugin before `busy` (reference-errors-and-limits.md, `pending_requests`).
    const PENDING_REQUESTS_PER_PLUGIN: usize = 64;

    /// A one-shot spec for `program` with no arguments, no stdin and a valid timeout.
    fn process_spec(program: &str) -> ProcessSpec {
        ProcessSpec {
            program: program.to_owned(),
            arguments: Vec::new(),
            cwd: None,
            stdin: None,
            environment: Vec::new(),
            timeout_millis: 5_000,
        }
    }

    /// A long-lived spawn spec for `program` with stdin open and line mode on.
    fn spawn_spec(program: &str) -> SpawnSpec {
        SpawnSpec {
            process: process_spec(program),
            stdin_open: true,
            line_mode: true,
        }
    }

    /// A T1 imports fixture that holds exactly the given grants.
    fn fixture_with_grants(grants: &[&str]) -> ImportsFixture {
        ImportsFixture::builder()
            .tier(Tier::T1)
            .grants(grants)
            .build()
    }

    /// Unwraps an error result, failing the test when the call succeeded.
    fn error_of<T: std::fmt::Debug>(result: Result<T, PluginError>) -> PluginError {
        match result {
            Ok(value) => panic!("expected an error, got {value:?}"),
            Err(error) => error,
        }
    }

    /// Asserts that a plugin error carries the documented code.
    fn assert_code(error: &PluginError, expected_code: &str) {
        assert_eq!(
            error.code.as_str(),
            expected_code,
            "unexpected error message: {}",
            error.message
        );
    }

    /// Unwraps a successful spawn, failing the test with the error otherwise.
    fn child_of(result: Result<ChildHandle, PluginError>) -> ChildHandle {
        match result {
            Ok(handle) => handle,
            Err(error) => panic!(
                "expected a child handle, got {} ({})",
                error.code.as_str(),
                error.message
            ),
        }
    }

    /// Spawns one child through a fixture that holds `process.long-lived` and `process.run:tail`.
    fn spawn_one_child(fixture: &mut ImportsFixture) -> ChildHandle {
        child_of(fixture.process().spawn(spawn_spec("tail")))
    }

    /// Environment pairs with `count` harmless keys.
    fn harmless_environment(count: usize) -> Vec<(String, String)> {
        (0..count)
            .map(|index| (format!("ILIUM_TEST_{index}"), "1".to_owned()))
            .collect()
    }

    /// Asserts that `signal_name` is accepted by a live child's `signal` call.
    fn assert_signal_accepted(signal_name: &str) {
        let mut fixture = fixture_with_grants(&["process.long-lived", "process.run:tail"]);
        let handle = spawn_one_child(&mut fixture);
        let result = fixture
            .process()
            .child_signal(handle, signal_name.to_owned());
        assert!(
            result.is_ok(),
            "signal {signal_name} must be accepted for a live child"
        );
    }

    // ---- run: spec validation -------------------------------------------------------------

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_rejects_a_zero_timeout_with_invalid_argument() {
        let mut import = ProcessImport::default();
        let mut spec = process_spec("echo");
        spec.timeout_millis = 0;
        let error = error_of(HostProcess::run(&mut import, spec));
        assert_code(&error, "invalid-argument");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_accepts_the_minimum_timeout_of_one_millisecond() {
        let mut import = ProcessImport::default();
        let mut spec = process_spec("echo");
        spec.timeout_millis = 1;
        let result = HostProcess::run(&mut import, spec);
        assert!(
            result.is_ok(),
            "a 1 ms timeout is the lower bound and is valid"
        );
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_accepts_the_maximum_timeout_of_600000_millis() {
        let mut import = ProcessImport::default();
        let mut spec = process_spec("echo");
        spec.timeout_millis = MAX_TIMEOUT_MILLIS;
        let result = HostProcess::run(&mut import, spec);
        assert!(result.is_ok(), "600,000 ms is the upper bound and is valid");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_rejects_a_timeout_one_above_the_maximum() {
        let mut import = ProcessImport::default();
        let mut spec = process_spec("echo");
        spec.timeout_millis = MAX_TIMEOUT_MILLIS + 1;
        let error = error_of(HostProcess::run(&mut import, spec));
        assert_code(&error, "invalid-argument");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_accepts_empty_arguments_environment_and_stdin() {
        let mut import = ProcessImport::default();
        let spec = process_spec("echo");
        assert!(spec.arguments.is_empty());
        assert!(spec.environment.is_empty());
        assert!(spec.stdin.is_none());
        let result = HostProcess::run(&mut import, spec);
        assert!(result.is_ok(), "empty optional inputs are valid");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_accepts_an_empty_stdin_buffer() {
        let mut import = ProcessImport::default();
        let mut spec = process_spec("cat");
        spec.stdin = Some(Vec::new());
        let result = HostProcess::run(&mut import, spec);
        assert!(result.is_ok(), "a zero-byte stdin is within the 4 MiB cap");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_accepts_stdin_of_exactly_four_mib() {
        let mut import = ProcessImport::default();
        let mut spec = process_spec("cat");
        spec.stdin = Some(vec![0u8; FOUR_MIB]);
        let result = HostProcess::run(&mut import, spec);
        assert!(
            result.is_ok(),
            "stdin of exactly 4 MiB is the upper bound and is valid"
        );
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_rejects_stdin_one_byte_above_four_mib() {
        let mut import = ProcessImport::default();
        let mut spec = process_spec("cat");
        spec.stdin = Some(vec![0u8; FOUR_MIB + 1]);
        let error = error_of(HostProcess::run(&mut import, spec));
        assert_code(&error, "invalid-argument");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_accepts_exactly_1024_arguments() {
        let mut import = ProcessImport::default();
        let mut spec = process_spec("echo");
        spec.arguments = vec!["x".to_owned(); MAX_ARGUMENTS];
        let result = HostProcess::run(&mut import, spec);
        assert!(
            result.is_ok(),
            "1,024 arguments is the upper bound and is valid"
        );
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_rejects_1025_arguments_with_invalid_argument() {
        let mut import = ProcessImport::default();
        let mut spec = process_spec("echo");
        spec.arguments = vec!["x".to_owned(); MAX_ARGUMENTS + 1];
        let error = error_of(HostProcess::run(&mut import, spec));
        assert_code(&error, "invalid-argument");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_accepts_exactly_64_environment_pairs() {
        let mut import = ProcessImport::default();
        let mut spec = process_spec("echo");
        spec.environment = harmless_environment(MAX_ENVIRONMENT_PAIRS);
        let result = HostProcess::run(&mut import, spec);
        assert!(result.is_ok(), "64 pairs is the upper bound and is valid");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_rejects_65_environment_pairs_with_invalid_argument() {
        let mut import = ProcessImport::default();
        let mut spec = process_spec("echo");
        spec.environment = harmless_environment(MAX_ENVIRONMENT_PAIRS + 1);
        let error = error_of(HostProcess::run(&mut import, spec));
        assert_code(&error, "invalid-argument");
    }

    /// REQ: SEC-033
    #[test]
    fn run_rejects_every_reserved_environment_key_with_invalid_argument() {
        let reserved_keys = [
            "PATH",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "LD_AUDIT",
            "DYLD_INSERT_LIBRARIES",
            "IFS",
            "BASH_ENV",
        ];
        for reserved_key in reserved_keys {
            let mut import = ProcessImport::default();
            let mut spec = process_spec("echo");
            spec.environment = vec![(reserved_key.to_owned(), "/tmp".to_owned())];
            let error = error_of(HostProcess::run(&mut import, spec));
            assert_eq!(
                error.code.as_str(),
                "invalid-argument",
                "reserved key {reserved_key} must be refused before the program starts"
            );
        }
    }

    // ---- run: grants and quotas (through the imports fixture, which holds the grant check) -

    /// REQ: SEC-031
    #[test]
    fn run_without_any_grant_is_not_permitted() {
        let mut fixture = fixture_with_grants(&[]);
        let error = error_of(fixture.process().run(process_spec("echo")));
        assert_code(&error, "not-permitted");
    }

    /// REQ: SEC-031
    #[test]
    fn run_with_a_grant_for_another_program_is_not_permitted() {
        let mut fixture = fixture_with_grants(&["process.run:git"]);
        let error = error_of(fixture.process().run(process_spec("echo")));
        assert_code(&error, "not-permitted");
    }

    /// REQ: SEC-031
    #[test]
    fn run_with_the_matching_program_grant_returns_a_request_id() {
        let mut fixture = fixture_with_grants(&["process.run:echo"]);
        let result = fixture.process().run(process_spec("echo"));
        assert!(result.is_ok(), "the exact program grant must allow the run");
    }

    /// REQ: ABI-013
    #[test]
    fn run_returns_busy_once_64_requests_are_pending() {
        let mut fixture = fixture_with_grants(&["process.run:echo"]);
        for _ in 0..PENDING_REQUESTS_PER_PLUGIN {
            let result = fixture.process().run(process_spec("echo"));
            assert!(result.is_ok(), "the first 64 pending requests are accepted");
        }
        let error = error_of(fixture.process().run(process_spec("echo")));
        assert_code(&error, "busy");
    }

    // ---- take-exit -------------------------------------------------------------------------

    /// REQ: WIT:ilium:host/process.take-exit
    #[test]
    fn take_exit_of_an_unknown_request_is_none() {
        let mut import = ProcessImport::default();
        let exit = HostProcess::take_exit(&mut import, RequestId(999));
        assert!(exit.is_none());
    }

    /// REQ: WIT:ilium:host/process.take-exit
    #[test]
    fn take_exit_of_request_zero_is_none() {
        let mut import = ProcessImport::default();
        let exit = HostProcess::take_exit(&mut import, RequestId(0));
        assert!(exit.is_none());
    }

    /// REQ: WIT:ilium:host/process.take-exit
    #[test]
    fn take_exit_of_a_pending_request_is_none_before_the_program_exits() {
        let mut fixture = fixture_with_grants(&["process.run:echo"]);
        let request = match fixture.process().run(process_spec("echo")) {
            Ok(request) => request,
            Err(error) => panic!("run must start: {}", error.message),
        };
        let exit = fixture.process().take_exit(request);
        assert!(
            exit.is_none(),
            "no exit is held while the program still runs"
        );
    }

    // ---- spawn -----------------------------------------------------------------------------

    /// REQ: SEC-031
    #[test]
    fn spawn_without_the_long_lived_grant_is_not_permitted() {
        let mut fixture = fixture_with_grants(&["process.run:tail"]);
        let error = error_of(fixture.process().spawn(spawn_spec("tail")));
        assert_code(&error, "not-permitted");
    }

    /// REQ: SEC-031
    #[test]
    fn spawn_without_the_program_grant_is_not_permitted() {
        let mut fixture = fixture_with_grants(&["process.long-lived"]);
        let error = error_of(fixture.process().spawn(spawn_spec("tail")));
        assert_code(&error, "not-permitted");
    }

    /// REQ: WIT:ilium:host/process.spawn
    #[test]
    fn spawn_with_both_grants_returns_a_child_handle() {
        let mut fixture = fixture_with_grants(&["process.long-lived", "process.run:tail"]);
        let result = fixture.process().spawn(spawn_spec("tail"));
        assert!(result.is_ok(), "both grants present must allow the spawn");
    }

    /// REQ: WIT:ilium:host/process.spawn
    #[test]
    fn spawn_ignores_a_zero_timeout_because_children_have_no_timeout() {
        let mut fixture = fixture_with_grants(&["process.long-lived", "process.run:tail"]);
        let mut spec = spawn_spec("tail");
        spec.process.timeout_millis = 0;
        let result = fixture.process().spawn(spec);
        assert!(
            result.is_ok(),
            "the timeout of a one-shot spec is ignored for children, so 0 is not refused"
        );
    }

    /// REQ: SEC-033
    #[test]
    fn spawn_rejects_a_reserved_environment_key_with_invalid_argument() {
        let mut fixture = fixture_with_grants(&["process.long-lived", "process.run:tail"]);
        let mut spec = spawn_spec("tail");
        spec.process.environment = vec![("LD_PRELOAD".to_owned(), "/tmp/x.so".to_owned())];
        let error = error_of(fixture.process().spawn(spec));
        assert_code(&error, "invalid-argument");
    }

    /// REQ: WIT:ilium:host/process.spawn
    #[test]
    fn spawn_refuses_a_third_child_of_one_plugin_with_quota_exceeded() {
        let mut fixture = fixture_with_grants(&["process.long-lived", "process.run:tail"]);
        let _first = spawn_one_child(&mut fixture);
        let _second = spawn_one_child(&mut fixture);
        let error = error_of(fixture.process().spawn(spawn_spec("tail")));
        assert_code(&error, "quota-exceeded");
    }

    // ---- child handle calls ----------------------------------------------------------------

    /// REQ: WIT:ilium:host/process.child.signal
    #[test]
    fn child_signal_accepts_interrupt() {
        assert_signal_accepted("interrupt");
    }

    /// REQ: WIT:ilium:host/process.child.signal
    #[test]
    fn child_signal_accepts_terminate() {
        assert_signal_accepted("terminate");
    }

    /// REQ: WIT:ilium:host/process.child.signal
    #[test]
    fn child_signal_accepts_hangup() {
        assert_signal_accepted("hangup");
    }

    /// REQ: WIT:ilium:host/process.child.signal
    #[test]
    fn child_signal_accepts_kill() {
        assert_signal_accepted("kill");
    }

    /// REQ: WIT:ilium:host/process.child.signal
    #[test]
    fn child_signal_rejects_a_name_outside_the_four_documented_signals() {
        let mut fixture = fixture_with_grants(&["process.long-lived", "process.run:tail"]);
        let handle = spawn_one_child(&mut fixture);
        let error = error_of(fixture.process().child_signal(handle, "stop".to_owned()));
        assert_code(&error, "invalid-argument");
    }

    /// REQ: WIT:ilium:host/process.child.signal
    #[test]
    fn child_signal_rejects_an_empty_signal_name() {
        let mut fixture = fixture_with_grants(&["process.long-lived", "process.run:tail"]);
        let handle = spawn_one_child(&mut fixture);
        let error = error_of(fixture.process().child_signal(handle, String::new()));
        assert_code(&error, "invalid-argument");
    }

    /// REQ: WIT:ilium:host/process.child.write-stdin
    #[test]
    fn child_write_stdin_accepts_an_empty_write_on_an_open_stdin() {
        let mut fixture = fixture_with_grants(&["process.long-lived", "process.run:cat"]);
        let handle = child_of(fixture.process().spawn(spawn_spec("cat")));
        let result = fixture.process().child_write_stdin(handle, Vec::new());
        assert!(result.is_ok(), "an empty write to an open stdin is valid");
    }
}
