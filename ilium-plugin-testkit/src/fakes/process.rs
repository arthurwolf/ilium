//! Fake process (reference-test-harness.md "Fake process").

use ilium_plugin_sdk_core::generated::host::{ProcessExit, ProcessSpec};
use ilium_plugin_sdk_core::types::PluginError;

/// One expected run; finish it with `returns` or `fails`.
#[derive(Debug)]
pub struct ProcessExpectation {
    _private: (),
}

impl ProcessExpectation {
    /// The next matching run returns this exit.
    pub fn returns(self, _exit: ProcessExit) {
        unimplemented!("DEV-464")
    }

    /// The next matching run completes as `failed(error)`.
    pub fn fails(self, _error: PluginError) {
        unimplemented!("DEV-464")
    }
}

/// A long-running child the plugin spawned with `process.spawn` (lane hook, see manual_gaps_fakes.md; DEV-464).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildRecord {
    /// Child id as the plugin received it.
    pub id: u64,
    /// Program name.
    pub program: String,
    /// Arguments after the program.
    pub arguments: Vec<String>,
    /// Lines the plugin wrote to the child's stdin, in order.
    pub stdin_lines: Vec<String>,
    /// Signals the plugin sent, by name (`"terminate"`, `"kill"`), in order.
    pub signals: Vec<String>,
    /// `SpawnSpec::stdin_open` of the spawn (reference-wit.md section 5.14).
    pub stdin_open: bool,
    /// `SpawnSpec::line_mode` of the spawn (reference-wit.md section 5.14).
    pub line_mode: bool,
}

/// Handle on the fake process service of a host.
#[derive(Debug)]
pub struct FakeProcess {
    _private: (),
}

impl FakeProcess {
    /// Registers one expected run; `program` and `arguments` must match exactly.
    pub fn expect(&mut self, _program: &str, _arguments: &[&str]) -> ProcessExpectation {
        unimplemented!("DEV-464")
    }

    /// Every run requested, in order, with `cwd`, `stdin` and `environment`.
    pub fn calls(&self) -> Vec<ProcessSpec> {
        unimplemented!("DEV-464")
    }

    /// Unmatched runs return exit status 0 instead of failing the test.
    pub fn allow_unexpected(&mut self) {
        unimplemented!("DEV-460")
    }

    /// Every child spawned, in order, with what the plugin wrote and signalled (lane hook, see manual_gaps_fakes.md).
    pub fn children(&self) -> Vec<ChildRecord> {
        unimplemented!("DEV-464")
    }

    /// The exit of every run no other rule matches (lane hook, see manual_gaps_fakes.md; DEV-464).
    pub fn default_exit(&mut self, _exit: ProcessExit) {
        unimplemented!("DEV-464")
    }

    /// The exit of every run whose arguments equal `arguments`, whatever the program (lane hook, see manual_gaps_fakes.md).
    pub fn on_args(&mut self, _arguments: &[&str], _exit: ProcessExit) {
        unimplemented!("DEV-464")
    }

    /// The exit of every run of `program`, whatever the arguments (lane hook, see manual_gaps_fakes.md; DEV-464).
    pub fn on_program(&mut self, _program: &str, _exit: ProcessExit) {
        unimplemented!("DEV-464")
    }

    /// Every run of `program` completes as `failed(timed-out)` (lane hook, see manual_gaps_fakes.md; DEV-464).
    pub fn on_program_timeout(&mut self, _program: &str) {
        unimplemented!("DEV-464")
    }

    /// The largest number of runs that were in flight at the same time (lane hook, see manual_gaps_fakes.md).
    pub fn max_concurrent(&self) -> usize {
        unimplemented!("DEV-464")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use crate::host::FakeHost;
    use ilium_plugin_sdk_core::types::ErrorCode;

    fn exit_with_stdout(stdout: &[u8]) -> ProcessExit {
        ProcessExit {
            status: Some(0),
            signal: None,
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }

    /// REQ: [DEV-464] Registering an expectation with `returns` MUST NOT record a call; `calls()` only lists runs that were requested.
    #[test]
    fn expect_returns_registers_without_recording_a_call() {
        let mut host = FakeHost::new().grant("process.run:git");
        host.process()
            .expect("git", &["status"])
            .returns(exit_with_stdout(b"clean\n"));
        assert!(host.process().calls().is_empty());
    }

    /// REQ: [DEV-464] An expectation that finishes with `fails` MUST register the error without recording a call.
    #[test]
    fn expect_fails_registers_a_permission_error_without_recording_a_call() {
        let mut host = FakeHost::new().grant("process.run:rm");
        let denied = PluginError {
            code: ErrorCode::NotPermitted,
            message: "denied".to_string(),
            retry_after_ms: None,
        };
        host.process().expect("rm", &["-rf", "/data"]).fails(denied);
        assert!(host.process().calls().is_empty());
    }

    /// REQ: [DEV-464] An empty program name and an empty argument list are valid exact-match inputs; registration MUST accept them without recording a call.
    #[test]
    fn expect_accepts_empty_program_and_arguments() {
        let mut host = FakeHost::new();
        host.process().expect("", &[]).returns(exit_with_stdout(b""));
        assert!(host.process().calls().is_empty());
    }

    /// REQ: [DEV-460] `allow_unexpected` MUST be a setting only: it MUST NOT by itself record a run or a child.
    #[test]
    fn allow_unexpected_records_nothing_by_itself() {
        let mut host = FakeHost::new();
        host.process().allow_unexpected();
        assert!(host.process().calls().is_empty());
        assert!(host.process().children().is_empty());
    }

    /// REQ: [reference-test-harness "Fake process"] `default_exit`, `on_args`, `on_program` and `on_program_timeout` are catch-all rules and MUST NOT count as runs in flight.
    #[test]
    fn catch_all_rules_do_not_count_as_runs() {
        let mut host = FakeHost::new();
        host.process().default_exit(exit_with_stdout(b"ok"));
        host.process().on_args(&["--version"], exit_with_stdout(b"1.0"));
        host.process().on_program("ls", exit_with_stdout(b""));
        host.process().on_program_timeout("sleep");
        assert_eq!(host.process().max_concurrent(), 0);
        assert!(host.process().calls().is_empty());
    }

    /// REQ: [reference-wit.md section 5.14] `children()` MUST be empty until the plugin spawns a long-lived child (`process.spawn`).
    #[test]
    fn children_is_empty_before_any_spawn() {
        let mut host = FakeHost::new().grant("process.long-lived");
        assert!(host.process().children().is_empty());
    }

    /// REQ: [reference-test-harness "Fake process"] `max_concurrent` MUST be 0 when no run was in flight, so an unused fake reports no concurrency.
    #[test]
    fn max_concurrent_is_zero_when_no_run_happened() {
        let mut host = FakeHost::new().grant("process.run:git");
        assert_eq!(host.process().max_concurrent(), 0);
    }
}
