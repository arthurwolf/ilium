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

pub use errors::{codes, HarnessError};
pub use fakes::tree::TreeNode;
pub use fuzz::FuzzFailure;
pub use golden::{assert_golden, assert_snapshot};
pub use host::{
    AudioCapture, CallOrder, Cell, ClientResponse, CollectPoint, Diagnostic, ExtensionEntry,
    FailureRecord, FakeAgents, FakeAudio, FakeClientRequests, FakeClipboard, FakeExtensions,
    FakeHelp, FakeHost, FakeHostBuilder, FakeInference, FakeJournal, FakeKernelSettings,
    FakeKeymap, FakeKeys, FakePanes, FakePlugins, FakeSessions, FakeTerminal, FakeTranscript,
    FakeVector, FakeWorkspace, HookChain, HostSnapshot, IntoJsonText, KeyGrab, LogLine, Mode,
    PluginHandle, Restart, Screen, ScreenCell, SentClientRequest,
};
pub use replay::ReplayMismatch;
pub use scene::FakeScene;
pub use session::FakeSession;
