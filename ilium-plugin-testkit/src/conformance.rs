//! Conformance suite entry (reference-conformance-suite.md).

use crate::errors::HarnessError;

/// One conformance case result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResult {
    /// Stable test name.
    pub name: String,
    /// True when the case passed.
    pub passed: bool,
    /// Failure reason, empty for a pass.
    pub reason: String,
}

/// Runs the whole conformance suite against a plugin type in the current mode.
pub fn run_suite<P: ilium_plugin_sdk_core::NativeEntry + Default>() -> Result<Vec<CaseResult>, HarnessError> {
    unimplemented!("conformance.suite")
}
