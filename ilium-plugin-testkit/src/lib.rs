//! Test harness for Ilium plugins (reference-test-harness.md, testing.md).
//!
//! The same API runs in native mode (the plugin is a library) and component mode (the built `.wasm`).

pub mod conformance;
pub mod core_stubs;
pub mod coverage;
pub mod errors;
pub mod fakes;
pub mod fixture;
pub mod fuzz;
pub mod golden;
pub mod host;
pub mod manual;
pub mod replay;
pub mod scene;
pub mod session;

pub use errors::{HarnessError, codes};
pub use fakes::tree::TreeNode;
pub use fuzz::FuzzFailure;
pub use golden::{assert_golden, assert_snapshot};
pub use host::{
    Cell, Diagnostic, FailureRecord, FakeHost, FakeHostBuilder, HostSnapshot, LogLine, Mode,
    PluginHandle, Restart, Screen, ScreenCell,
};
pub use replay::ReplayMismatch;
pub use scene::FakeScene;
pub use session::FakeSession;
