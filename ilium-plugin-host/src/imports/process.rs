//! Host implementation of `ilium:host/process` (api-process-control.md).
//!
//! One-shot runs execute on a worker thread; the exit is held until the host's next event tick
//! (`EXIT_DELIVERY_DELAY`) and then handed out once by `take-exit`. Long-lived children are kept
//! per plugin instance (two at most). Grants (`process.run:<program>`, `process.long-lived`) are
//! checked by the dispatch layer that knows the manifest; this import owns argument validation,
//! the pending-request cap, the child quota and the process work itself.
//!
//! State is thread-local: one plugin instance is driven from one host thread, so its quotas
//! never leak into another instance.

use ilium_plugin_api::generated::host::process::{
    ChildHandle, HostProcess, ProcessExit, ProcessSpec, SpawnSpec,
};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::{ChildId, RequestId};
use ilium_plugin_api::generated::types::Outcome;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
/// An exit becomes visible to `take-exit` only after the host's next event tick.
const EXIT_DELIVERY_DELAY: Duration = Duration::from_millis(50);
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

/// Host side of the `process` interface.
#[derive(Debug)]
pub struct ProcessImport;

/// A finished run waiting for `take-exit`.
type ExitSlot = Arc<Mutex<Option<(Instant, ProcessExit, Option<ErrorCode>)>>>;

/// One long-lived child.
struct ChildEntry {
    id: ChildId,
    child: Child,
    stdin: Option<ChildStdin>,
}

thread_local! {
    static NEXT_REQUEST: Cell<u64> = const { Cell::new(1) };
    static NEXT_CHILD: Cell<u32> = const { Cell::new(1) };
    static PENDING: RefCell<HashMap<u64, ExitSlot>> = RefCell::new(HashMap::new());
    static CHILDREN: RefCell<HashMap<u32, ChildEntry>> = RefCell::new(HashMap::new());
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
    if spec.stdin.as_ref().is_some_and(|bytes| bytes.len() > FOUR_MIB) {
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

/// Builds the command of a spec with the given stdio wiring.
fn command_of(spec: &ProcessSpec) -> Command {
    let mut command = Command::new(&spec.program);
    command.args(&spec.arguments);
    for (key, value) in &spec.environment {
        command.env(key, value);
    }
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

/// Runs one spec to completion or timeout; never panics, a failure to start is a non-zero exit.
fn execute_one_shot(spec: &ProcessSpec) -> (ProcessExit, Option<ErrorCode>) {
    let mut command = command_of(spec);
    command
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(spawn_error) => {
            let code = if spawn_error.kind() == std::io::ErrorKind::NotFound {
                ErrorCode::NotFound
            } else {
                ErrorCode::Io
            };
            return (
                ProcessExit {
                    status: Some(127),
                    signal: None,
                    stdout: Vec::new(),
                    stderr: spawn_error.to_string().into_bytes(),
                },
                Some(code),
            );
        }
    };
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
            Ok(None) if Instant::now() >= deadline => {
                timed_out = true;
                let _ = child.kill();
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

/// Waits up to `budget` for a one-shot request to finish and maps it to its completion outcome
/// (`ok`, or `failed(code)` per api-process-control.md); `None` when the request is unknown here
/// or still running after the budget. The exit stays available for `take-exit`.
pub fn await_outcome(request: RequestId, budget: Duration) -> Option<Outcome> {
    let slot = PENDING.with(|pending| pending.borrow().get(&request.0).cloned())?;
    let deadline = Instant::now() + budget;
    loop {
        let finished = slot
            .lock()
            .ok()
            .and_then(|held| held.as_ref().map(|(_, _, failure)| *failure));
        if let Some(failure) = finished {
            return Some(match failure {
                None => Outcome::Ok,
                Some(code) => Outcome::Failed(error(code, "the process request failed")),
            });
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(2));
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

impl HostProcess for ProcessImport {
    fn run(&mut self, spec: ProcessSpec) -> Result<RequestId, PluginError> {
        validate_spec(&spec, true)?;
        let pending_count = PENDING.with(|pending| pending.borrow().len());
        if pending_count >= PENDING_REQUESTS_PER_PLUGIN {
            return Err(error(ErrorCode::Busy, "64 process requests are already pending"));
        }
        let request_number = NEXT_REQUEST.with(|next| {
            let number = next.get();
            next.set(number + 1);
            number
        });
        let slot: ExitSlot = Arc::new(Mutex::new(None));
        let worker_slot = Arc::clone(&slot);
        std::thread::spawn(move || {
            let (exit, failure) = execute_one_shot(&spec);
            if let Ok(mut held) = worker_slot.lock() {
                *held = Some((Instant::now(), exit, failure));
            }
        });
        PENDING.with(|pending| pending.borrow_mut().insert(request_number, slot));
        Ok(RequestId(request_number))
    }

    fn take_exit(&mut self, request: RequestId) -> Option<ProcessExit> {
        PENDING.with(|pending| {
            let mut pending = pending.borrow_mut();
            let slot = pending.get(&request.0)?;
            let ready = {
                let held = slot.lock().ok()?;
                held.as_ref()
                    .is_some_and(|(finished_at, _, _)| finished_at.elapsed() >= EXIT_DELIVERY_DELAY)
            };
            if !ready {
                return None;
            }
            let slot = pending.remove(&request.0)?;
            let exit = slot.lock().ok()?.take();
            exit.map(|(_, exit, _)| exit)
        })
    }

    fn spawn(&mut self, spec: SpawnSpec) -> Result<ChildHandle, PluginError> {
        // Children have no timeout: the one-shot timeout is ignored.
        validate_spec(&spec.process, false)?;
        if CHILDREN.with(|children| children.borrow().len()) >= CHILDREN_PER_PLUGIN {
            return Err(error(
                ErrorCode::QuotaExceeded,
                "a plugin may run 2 long-lived children",
            ));
        }
        let mut command = command_of(&spec.process);
        command
            .stdin(if spec.stdin_open {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|spawn_error| {
            error(
                ErrorCode::NotFound,
                format!("cannot start the program: {spawn_error}"),
            )
        })?;
        let stdin = child.stdin.take();
        let handle_number = NEXT_CHILD.with(|next| {
            let number = next.get();
            next.set(number + 1);
            number
        });
        CHILDREN.with(|children| {
            children.borrow_mut().insert(
                handle_number,
                ChildEntry {
                    id: ChildId(u64::from(handle_number)),
                    child,
                    stdin,
                },
            );
        });
        Ok(ChildHandle(handle_number))
    }

    fn child_pid_hint(&mut self, handle: ChildHandle) -> u32 {
        CHILDREN.with(|children| {
            children
                .borrow()
                .get(&handle.0)
                .map_or(0, |entry| entry.child.id())
        })
    }

    fn child_write_stdin(
        &mut self,
        handle: ChildHandle,
        bytes: Vec<u8>,
    ) -> Result<(), PluginError> {
        CHILDREN.with(|children| {
            let mut children = children.borrow_mut();
            let entry = children
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
                    error(ErrorCode::Conflict, format!("stdin write failed: {write_error}"))
                })
        })
    }

    fn child_close_stdin(&mut self, handle: ChildHandle) {
        CHILDREN.with(|children| {
            if let Some(entry) = children.borrow_mut().get_mut(&handle.0) {
                entry.stdin = None;
            }
        });
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
        CHILDREN.with(|children| {
            let mut children = children.borrow_mut();
            let entry = children
                .get_mut(&handle.0)
                .ok_or_else(|| error(ErrorCode::NotFound, "the child handle is not live"))?;
            if signal_flag == "KILL" {
                // An already exited child is not an error for a kill.
                let _ = entry.child.kill();
                return Ok(());
            }
            let delivered = Command::new("kill")
                .arg(format!("-{signal_flag}"))
                .arg(entry.child.id().to_string())
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
        })
    }

    fn child_id(&mut self, handle: ChildHandle) -> ChildId {
        CHILDREN.with(|children| {
            children
                .borrow()
                .get(&handle.0)
                .map_or(ChildId(u64::from(handle.0)), |entry| entry.id)
        })
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
        let mut import = ProcessImport;
        let mut spec = process_spec("echo");
        spec.timeout_millis = 0;
        let error = error_of(HostProcess::run(&mut import, spec));
        assert_code(&error, "invalid-argument");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_accepts_the_minimum_timeout_of_one_millisecond() {
        let mut import = ProcessImport;
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
        let mut import = ProcessImport;
        let mut spec = process_spec("echo");
        spec.timeout_millis = MAX_TIMEOUT_MILLIS;
        let result = HostProcess::run(&mut import, spec);
        assert!(result.is_ok(), "600,000 ms is the upper bound and is valid");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_rejects_a_timeout_one_above_the_maximum() {
        let mut import = ProcessImport;
        let mut spec = process_spec("echo");
        spec.timeout_millis = MAX_TIMEOUT_MILLIS + 1;
        let error = error_of(HostProcess::run(&mut import, spec));
        assert_code(&error, "invalid-argument");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_accepts_empty_arguments_environment_and_stdin() {
        let mut import = ProcessImport;
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
        let mut import = ProcessImport;
        let mut spec = process_spec("cat");
        spec.stdin = Some(Vec::new());
        let result = HostProcess::run(&mut import, spec);
        assert!(result.is_ok(), "a zero-byte stdin is within the 4 MiB cap");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_accepts_stdin_of_exactly_four_mib() {
        let mut import = ProcessImport;
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
        let mut import = ProcessImport;
        let mut spec = process_spec("cat");
        spec.stdin = Some(vec![0u8; FOUR_MIB + 1]);
        let error = error_of(HostProcess::run(&mut import, spec));
        assert_code(&error, "invalid-argument");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_accepts_exactly_1024_arguments() {
        let mut import = ProcessImport;
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
        let mut import = ProcessImport;
        let mut spec = process_spec("echo");
        spec.arguments = vec!["x".to_owned(); MAX_ARGUMENTS + 1];
        let error = error_of(HostProcess::run(&mut import, spec));
        assert_code(&error, "invalid-argument");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_accepts_exactly_64_environment_pairs() {
        let mut import = ProcessImport;
        let mut spec = process_spec("echo");
        spec.environment = harmless_environment(MAX_ENVIRONMENT_PAIRS);
        let result = HostProcess::run(&mut import, spec);
        assert!(result.is_ok(), "64 pairs is the upper bound and is valid");
    }

    /// REQ: WIT:ilium:host/process.run
    #[test]
    fn run_rejects_65_environment_pairs_with_invalid_argument() {
        let mut import = ProcessImport;
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
            let mut import = ProcessImport;
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
        let mut import = ProcessImport;
        let exit = HostProcess::take_exit(&mut import, RequestId(999));
        assert!(exit.is_none());
    }

    /// REQ: WIT:ilium:host/process.take-exit
    #[test]
    fn take_exit_of_request_zero_is_none() {
        let mut import = ProcessImport;
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
