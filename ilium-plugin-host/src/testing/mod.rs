//! Test support for the host and the conformance suite, behind the cargo feature `testing`
//! (test plan section 1.3): a fake kernel, in-process guests, a component-text loader and the
//! host harness. LIF-279: the same conformance cases run against this harness and against the
//! FakeHost of `ilium-plugin-testkit`.

pub mod fake_kernel;
pub mod harness;
pub mod native_guest;
pub mod wat;

pub use fake_kernel::FakeKernel;
pub use harness::{HarnessHandle, HostHarness, ManualClock};
pub use native_guest::NativeGuest;
pub use wat::wat_component;
