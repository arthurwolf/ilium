//! Fixture files and expectation files (reference-test-harness.md "Fixtures and expectation files").

use std::path::Path;

use crate::errors::HarnessError;

/// A parsed fixture with `"schema": 1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Fixture {
    /// The whole document; keys are validated against the schema with `fixture.unknown-key` and a JSON path.
    pub document: serde_json::Value,
}

/// Outcome of one fixture run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixtureStatus {
    /// Matched the expectation file, or has none (then `test.no-expectation` is a warning).
    Pass,
    /// Differs from the expectation.
    Fail(String),
    /// Trapped with the given trap code.
    Trap(String),
}

impl Fixture {
    /// Loads and validates a fixture file.
    pub fn load(_path: &Path) -> Result<Fixture, HarnessError> {
        unimplemented!("DEV-423")
    }
}

/// Runs one fixture against the plugin type and compares the expectation file next to it.
pub fn run<P: ilium_plugin_sdk_core::NativeEntry + Default + 'static>(
    _path: &Path,
) -> Result<FixtureStatus, HarnessError> {
    unimplemented!("DEV-423")
}
