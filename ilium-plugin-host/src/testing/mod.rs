//! Test support for the host and the conformance suite, behind the cargo feature `testing`
//! (test plan section 1.3): a fake kernel, a fake network, in-process guests, a component-text
//! loader and the host harness. LIF-279: the same conformance cases run against this harness and
//! against the FakeHost of `ilium-plugin-testkit`.

pub mod fake_kernel;
pub mod fake_net;
pub mod harness;
pub mod native_guest;
pub mod wat;

pub use fake_kernel::{FakeKernel, FakePaneScreen, HttpAnswer};
pub use fake_net::{FakeNet, FakeResponse, SeenRequest};
pub use harness::{
    a_client_applied_intent, applying_side, CompletionView, DetectReport, DiagnosticsSection,
    HarnessHandle, HostHarness, ImportReply, ImportResult, Isolation, ManualClock, NullGuest,
    PeerMessage, RetainedContribution, ReviewRow, StoreDataView, SystemLogMessage,
    TickObservation,
};
pub use native_guest::NativeGuest;
pub use wat::wat_component;
