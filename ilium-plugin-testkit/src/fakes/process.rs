//! Fake process (reference-test-harness.md "Fake process").

use std::cell::RefCell;
use std::rc::Rc;

use ilium_plugin_sdk_core::generated::host::{ProcessExit, ProcessSpec};
use ilium_plugin_sdk_core::types::PluginError;

/// What a matching run produces.
#[derive(Debug, Clone)]
pub(crate) enum Reply {
    Exit(ProcessExit),
    Fail(PluginError),
}

#[derive(Debug, Clone)]
struct ExactRule {
    program: String,
    arguments: Vec<String>,
    reply: Reply,
}

#[derive(Debug, Default)]
struct Rules {
    exact: Vec<ExactRule>,
    by_args: Vec<(Vec<String>, ProcessExit)>,
    by_program: Vec<(String, Reply)>,
    default_exit: Option<ProcessExit>,
    allow_unexpected: bool,
}

/// One expected run; finish it with `returns` or `fails`.
#[derive(Debug)]
pub struct ProcessExpectation {
    program: String,
    arguments: Vec<String>,
    rules: Rc<RefCell<Rules>>,
}

impl ProcessExpectation {
    fn register(self, reply: Reply) {
        self.rules.borrow_mut().exact.push(ExactRule {
            program: self.program,
            arguments: self.arguments,
            reply,
        });
    }

    /// The next matching run returns this exit.
    pub fn returns(self, exit: ProcessExit) {
        self.register(Reply::Exit(exit));
    }

    /// The next matching run completes as `failed(error)`.
    pub fn fails(self, error: PluginError) {
        self.register(Reply::Fail(error));
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
#[derive(Debug, Default)]
pub struct FakeProcess {
    rules: Rc<RefCell<Rules>>,
    calls: Vec<ProcessSpec>,
    children: Vec<ChildRecord>,
    in_flight: usize,
    max_concurrent: usize,
}

impl FakeProcess {
    /// Registers one expected run; `program` and `arguments` must match exactly.
    pub fn expect(&mut self, program: &str, arguments: &[&str]) -> ProcessExpectation {
        ProcessExpectation {
            program: program.to_string(),
            arguments: arguments.iter().map(|argument| argument.to_string()).collect(),
            rules: Rc::clone(&self.rules),
        }
    }

    /// Every run requested, in order, with `cwd`, `stdin` and `environment`.
    pub fn calls(&self) -> Vec<ProcessSpec> {
        self.calls.clone()
    }

    /// Unmatched runs return exit status 0 instead of failing the test.
    pub fn allow_unexpected(&mut self) {
        self.rules.borrow_mut().allow_unexpected = true;
    }

    /// Every child spawned, in order, with what the plugin wrote and signalled (lane hook, see manual_gaps_fakes.md).
    pub fn children(&self) -> Vec<ChildRecord> {
        self.children.clone()
    }

    /// The exit of every run no other rule matches (lane hook, see manual_gaps_fakes.md; DEV-464).
    pub fn default_exit(&mut self, exit: ProcessExit) {
        self.rules.borrow_mut().default_exit = Some(exit);
    }

    /// The exit of every run whose arguments equal `arguments`, whatever the program (lane hook, see manual_gaps_fakes.md).
    pub fn on_args(&mut self, arguments: &[&str], exit: ProcessExit) {
        self.rules.borrow_mut().by_args.push((
            arguments.iter().map(|argument| argument.to_string()).collect(),
            exit,
        ));
    }

    /// The exit of every run of `program`, whatever the arguments (lane hook, see manual_gaps_fakes.md; DEV-464).
    pub fn on_program(&mut self, program: &str, exit: ProcessExit) {
        self.rules
            .borrow_mut()
            .by_program
            .push((program.to_string(), Reply::Exit(exit)));
    }

    /// Every run of `program` completes as `failed(timed-out)` (lane hook, see manual_gaps_fakes.md; DEV-464).
    pub fn on_program_timeout(&mut self, program: &str) {
        self.rules.borrow_mut().by_program.push((
            program.to_string(),
            Reply::Fail(PluginError::timed_out("process run timed out")),
        ));
    }

    /// The largest number of runs that were in flight at the same time (lane hook, see manual_gaps_fakes.md).
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// Resolves one run against the rules and records it. Order: exact expectation (consumed),
    /// per-program rule, per-arguments rule, default exit, allow-unexpected.
    pub(crate) fn run(&mut self, spec: &ProcessSpec) -> Result<ProcessExit, PluginError> {
        self.calls.push(spec.clone());
        self.in_flight += 1;
        self.max_concurrent = self.max_concurrent.max(self.in_flight);
        self.in_flight -= 1;
        let mut rules = self.rules.borrow_mut();
        if let Some(index) = rules
            .exact
            .iter()
            .position(|rule| rule.program == spec.program && rule.arguments == spec.arguments)
        {
            let rule = rules.exact.remove(index);
            return match rule.reply {
                Reply::Exit(exit) => Ok(exit),
                Reply::Fail(error) => Err(error),
            };
        }
        if let Some((_, reply)) = rules.by_program.iter().find(|(name, _)| *name == spec.program) {
            return match reply.clone() {
                Reply::Exit(exit) => Ok(exit),
                Reply::Fail(error) => Err(error),
            };
        }
        if let Some((_, exit)) = rules.by_args.iter().find(|(args, _)| *args == spec.arguments) {
            return Ok(exit.clone());
        }
        if let Some(exit) = &rules.default_exit {
            return Ok(exit.clone());
        }
        if rules.allow_unexpected {
            return Ok(ProcessExit {
                status: Some(0),
                signal: None,
                stdout: Vec::new(),
                stderr: Vec::new(),
            });
        }
        Err(PluginError::new(
            ilium_plugin_sdk_core::types::ErrorCode::NotFound,
            format!(
                "{}: no expectation for {} {:?}",
                crate::errors::codes::UNEXPECTED_CALL,
                spec.program,
                spec.arguments
            ),
        ))
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

    #[test]
    fn expect_returns_registers_without_recording_a_call() {
        let mut host = FakeHost::new().grant("process.run:git");
        host.process()
            .expect("git", &["status"])
            .returns(exit_with_stdout(b"clean\n"));
        assert!(host.process().calls().is_empty());
    }

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

    #[test]
    fn expect_accepts_empty_program_and_arguments() {
        let mut host = FakeHost::new();
        host.process()
            .expect("", &[])
            .returns(exit_with_stdout(b""));
        assert!(host.process().calls().is_empty());
    }

    #[test]
    fn allow_unexpected_records_nothing_by_itself() {
        let mut host = FakeHost::new();
        host.process().allow_unexpected();
        assert!(host.process().calls().is_empty());
        assert!(host.process().children().is_empty());
    }

    #[test]
    fn catch_all_rules_do_not_count_as_runs() {
        let mut host = FakeHost::new();
        host.process().default_exit(exit_with_stdout(b"ok"));
        host.process()
            .on_args(&["--version"], exit_with_stdout(b"1.0"));
        host.process().on_program("ls", exit_with_stdout(b""));
        host.process().on_program_timeout("sleep");
        assert_eq!(host.process().max_concurrent(), 0);
        assert!(host.process().calls().is_empty());
    }

    #[test]
    fn children_is_empty_before_any_spawn() {
        let mut host = FakeHost::new().grant("process.long-lived");
        assert!(host.process().children().is_empty());
    }

    #[test]
    fn max_concurrent_is_zero_when_no_run_happened() {
        let mut host = FakeHost::new().grant("process.run:git");
        assert_eq!(host.process().max_concurrent(), 0);
    }
}
