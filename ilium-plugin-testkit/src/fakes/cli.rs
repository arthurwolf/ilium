//! Fake command line (reference-test-harness.md DEV-473).

/// The result of one invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// Command name.
    pub name: String,
    /// Arguments after the name.
    pub arguments: Vec<String>,
}

impl Invocation {
    /// Standard output the plugin wrote.
    pub fn stdout(&self) -> String {
        unimplemented!("DEV-473")
    }

    /// Standard error the plugin wrote.
    pub fn stderr(&self) -> String {
        unimplemented!("DEV-473")
    }

    /// Exit code the plugin set.
    pub fn exit_code(&self) -> Option<u8> {
        unimplemented!("DEV-473")
    }
}

/// Handle on the fake command line of a host.
#[derive(Debug)]
pub struct FakeCli {
    _private: (),
}

impl FakeCli {
    /// Delivers `cli-invoked` with these arguments and stdin.
    pub fn invoke(&mut self, _args: &[&str], _stdin: &[u8]) -> Invocation {
        unimplemented!("DEV-473")
    }

    /// The current output and exit code of an earlier invocation (lane hook, see manual_gaps_fakes.md; DEV-473).
    pub fn refresh(&self, _invocation: &Invocation) -> Invocation {
        unimplemented!("DEV-473")
    }

    /// Delivers the cancel of a running invocation, as Ctrl-C would (lane hook, see manual_gaps_fakes.md; DEV-473).
    pub fn cancel(&mut self, _invocation: &Invocation) {
        unimplemented!("DEV-473")
    }

    /// Sets an environment variable seen by later invocations (lane hook, see manual_gaps_fakes.md; DEV-473).
    pub fn set_env(&mut self, _key: &str, _value: &str) {
        unimplemented!("DEV-473")
    }

    /// Removes an environment variable for later invocations (lane hook, see manual_gaps_fakes.md; DEV-473).
    pub fn unset_env(&mut self, _key: &str) {
        unimplemented!("DEV-473")
    }

    /// Sets the working directory of later invocations, in `"scope:path"` shorthand (lane hook, see manual_gaps_fakes.md).
    pub fn set_cwd(&mut self, _path: &str) {
        unimplemented!("DEV-473")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// A fake command line with no invocations yet, as a host hands it out.
    fn fresh_cli() -> FakeCli {
        FakeCli { _private: () }
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
