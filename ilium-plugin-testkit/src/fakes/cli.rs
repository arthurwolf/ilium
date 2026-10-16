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
