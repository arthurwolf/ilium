//! Host implementation of `ilium:host/process` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostProcess` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::process::{
    ChildHandle, HostProcess, ProcessExit, ProcessSpec, SpawnSpec,
};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{ChildId, RequestId};

/// Host side of the `process` interface.
#[derive(Debug)]
pub struct ProcessImport;

impl HostProcess for ProcessImport {
    fn run(&mut self, _spec: ProcessSpec) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::process::run")
    }

    fn take_exit(&mut self, _request: RequestId) -> Option<ProcessExit> {
        unimplemented!("host::imports::process::take_exit")
    }

    fn spawn(&mut self, _spec: SpawnSpec) -> Result<ChildHandle, PluginError> {
        unimplemented!("host::imports::process::spawn")
    }

    fn child_pid_hint(&mut self, _handle: ChildHandle) -> u32 {
        unimplemented!("host::imports::process::child_pid_hint")
    }

    fn child_write_stdin(
        &mut self,
        _handle: ChildHandle,
        _bytes: Vec<u8>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::process::child_write_stdin")
    }

    fn child_close_stdin(&mut self, _handle: ChildHandle) {
        unimplemented!("host::imports::process::child_close_stdin")
    }

    fn child_signal(&mut self, _handle: ChildHandle, _signal: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::process::child_signal")
    }

    fn child_id(&mut self, _handle: ChildHandle) -> ChildId {
        unimplemented!("host::imports::process::child_id")
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
