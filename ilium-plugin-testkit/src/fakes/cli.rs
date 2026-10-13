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
}
