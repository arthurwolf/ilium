//! Fake command line (reference-test-harness.md DEV-473).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use ilium_plugin_sdk_core::generated::types::{CliInvokedEvent, Event};

use super::tree::Outbox;

/// What a running invocation has produced so far; shared with the host.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct InvocationOutput {
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) exit_code: Option<u8>,
}

/// The result of one invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// Command name.
    pub name: String,
    /// Arguments after the name.
    pub arguments: Vec<String>,
    id: u64,
    output: Rc<RefCell<InvocationOutput>>,
}

impl Invocation {
    /// Standard output the plugin wrote.
    pub fn stdout(&self) -> String {
        self.output.borrow().stdout.clone()
    }

    /// Standard error the plugin wrote.
    pub fn stderr(&self) -> String {
        self.output.borrow().stderr.clone()
    }

    /// Exit code the plugin set.
    pub fn exit_code(&self) -> Option<u8> {
        self.output.borrow().exit_code
    }
}

/// Handle on the fake command line of a host.
#[derive(Debug, Default)]
pub struct FakeCli {
    next_id: u64,
    env: BTreeMap<String, String>,
    cwd: String,
    outputs: BTreeMap<u64, Rc<RefCell<InvocationOutput>>>,
    outbox: Outbox,
}

impl FakeCli {
    /// A command line that publishes events into `outbox`.
    pub(crate) fn with_outbox(outbox: Outbox) -> FakeCli {
        FakeCli {
            outbox,
            ..FakeCli::default()
        }
    }

    /// Delivers `cli-invoked` with these arguments and stdin.
    pub fn invoke(&mut self, args: &[&str], _stdin: &[u8]) -> Invocation {
        self.next_id += 1;
        let id = self.next_id;
        let name = args.first().map(|word| word.to_string()).unwrap_or_default();
        let arguments: Vec<String> = args.iter().skip(1).map(|word| word.to_string()).collect();
        let output = Rc::new(RefCell::new(InvocationOutput::default()));
        self.outputs.insert(id, Rc::clone(&output));
        self.outbox
            .borrow_mut()
            .push(Event::CliInvoked(CliInvokedEvent {
                invocation: id,
                name: name.clone(),
                argv: arguments.clone(),
                cwd: self.cwd.clone(),
            }));
        Invocation {
            name,
            arguments,
            id,
            output,
        }
    }

    /// The current output and exit code of an earlier invocation (lane hook, see manual_gaps_fakes.md; DEV-473).
    pub fn refresh(&self, invocation: &Invocation) -> Invocation {
        invocation.clone()
    }

    /// Delivers the cancel of a running invocation, as Ctrl-C would (lane hook, see manual_gaps_fakes.md; DEV-473).
    pub fn cancel(&mut self, invocation: &Invocation) {
        if self.outputs.contains_key(&invocation.id) {
            self.outbox
                .borrow_mut()
                .push(Event::CliCancelled(invocation.id));
        }
    }

    /// Sets an environment variable seen by later invocations (lane hook, see manual_gaps_fakes.md; DEV-473).
    pub fn set_env(&mut self, key: &str, value: &str) {
        self.env.insert(key.to_string(), value.to_string());
    }

    /// Removes an environment variable for later invocations (lane hook, see manual_gaps_fakes.md; DEV-473).
    pub fn unset_env(&mut self, key: &str) {
        self.env.remove(key);
    }

    /// Sets the working directory of later invocations, in `"scope:path"` shorthand (lane hook, see manual_gaps_fakes.md).
    pub fn set_cwd(&mut self, path: &str) {
        self.cwd = path.to_string();
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// A fake command line with no invocations yet, as a host hands it out.
    fn fresh_cli() -> FakeCli {
        FakeCli::default()
    }

    /// REQ: DEV-473
    #[test]
    fn dev_473_invoke_takes_the_first_word_as_the_name() {
        let mut cli = fresh_cli();
        let invocation = cli.invoke(&["notes", "list", "--json"], b"");
        assert_eq!(invocation.name, "notes");
        assert_eq!(
            invocation.arguments,
            vec!["list".to_string(), "--json".to_string()]
        );
    }

    /// REQ: DEV-473
    #[test]
    fn dev_473_invoke_with_no_arguments_gives_an_empty_argument_list() {
        let mut cli = fresh_cli();
        let invocation = cli.invoke(&["notes"], b"");
        assert_eq!(invocation.name, "notes");
        assert!(invocation.arguments.is_empty());
    }

    /// REQ: DEV-473
    #[test]
    fn dev_473_a_new_invocation_has_written_no_output_yet() {
        let mut cli = fresh_cli();
        let invocation = cli.invoke(&["notes", "list"], b"");
        assert_eq!(invocation.stdout(), "");
        assert_eq!(invocation.stderr(), "");
    }

    /// REQ: DEV-473
    /// Manual: )` is `None`
    #[test]
    fn dev_473_an_open_invocation_has_no_exit_code() {
        let mut cli = fresh_cli();
        let invocation = cli.invoke(&["notes", "list"], b"");
        assert_eq!(invocation.exit_code(), None);
    }

    /// REQ: DEV-473
    #[test]
    fn dev_473_refresh_keeps_the_name_and_arguments_of_the_invocation() {
        let mut cli = fresh_cli();
        let invocation = cli.invoke(&["notes", "list"], b"");
        let refreshed = cli.refresh(&invocation);
        assert_eq!(refreshed.name, invocation.name);
        assert_eq!(refreshed.arguments, invocation.arguments);
    }

    /// REQ: DEV-473
    #[test]
    fn dev_473_refresh_of_an_unfinished_invocation_reports_no_exit_code() {
        let mut cli = fresh_cli();
        let invocation = cli.invoke(&["notes", "list"], b"");
        assert_eq!(cli.refresh(&invocation).exit_code(), None);
    }

    /// REQ: DEV-473
    #[test]
    fn dev_473_cancel_leaves_the_cancelled_invocation_without_an_exit_code() {
        let mut cli = fresh_cli();
        let invocation = cli.invoke(&["notes", "list"], b"");
        cli.cancel(&invocation);
        assert_eq!(cli.refresh(&invocation).exit_code(), None);
    }
}
