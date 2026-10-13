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
}
