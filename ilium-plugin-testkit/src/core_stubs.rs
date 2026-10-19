//! The 30 bundled core plugin stubs of `FakeSession::install_core` (reference-test-harness.md "Core stubs").

/// One core stub: its name, the extension points it exposes and the services it provides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreStub {
    /// Core plugin name, for example "ilium.agent-tools".
    pub name: String,
    /// Extension point names the stub exposes.
    pub extension_points: Vec<String>,
    /// Service names the stub provides.
    pub services: Vec<String>,
}

/// The table of core stubs, parsed from the "Core stubs" table of the harness page.
pub fn table() -> Vec<CoreStub> {
    unimplemented!("harness.core-stubs-table")
}

/// Looks up one stub by name; `None` for an unknown name (then `install_core` fails with `harness.core-unknown`).
pub fn find(_name: &str) -> Option<CoreStub> {
    unimplemented!("harness.core-stubs-table")
}
