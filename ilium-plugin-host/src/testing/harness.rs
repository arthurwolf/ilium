//! The host harness: one builder that wires a fake kernel, guests, grants, limits and a manual
//! clock into a host under test (test plan section 1.3 `testing`).

use std::sync::Arc;
use std::time::Duration;

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::requests::RequestResult;
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::limits::Limits;
use ilium_plugin_api::{Side, Tier};

use super::fake_kernel::FakeKernel;
use super::native_guest::NativeGuest;
use crate::audit::AuditRecord;
use crate::calls::Trap;
use crate::dispatch::PumpReport;
use crate::error::HostError;
use ilium_plugin_api::generated::host::plugins::InstanceState;
use crate::telemetry::LogLine;

/// A clock the test advances by hand; milliseconds since an arbitrary start.
#[derive(Debug, Clone, Default)]
pub struct ManualClock {
    now_millis: u64,
}

impl ManualClock {
    /// A clock at time zero.
    pub fn new() -> ManualClock {
        ManualClock { now_millis: 0 }
    }

    /// The current reading in milliseconds.
    pub fn now_millis(&self) -> u64 {
        self.now_millis
    }
}

/// Builder for a host under test.
pub struct HostHarness {
    tier: Tier,
    side: Side,
    grants: Vec<String>,
    limits: Limits,
    kernel: Option<Arc<FakeKernel>>,
    native_guest: Option<Box<dyn NativeGuest>>,
    wat_text: Option<String>,
    clock: ManualClock,
}

impl HostHarness {
    /// A harness for a T3 server-side plugin with no grants and default limits.
    pub fn new() -> HostHarness {
        HostHarness {
            tier: Tier::T3,
            side: Side::Server,
            grants: Vec::new(),
            limits: Limits::default(),
            kernel: None,
            native_guest: None,
            wat_text: None,
            clock: ManualClock::new(),
        }
    }

    /// Sets the trust tier.
    pub fn tier(mut self, tier: Tier) -> HostHarness {
        self.tier = tier;
        self
    }

    /// Sets the side.
    pub fn side(mut self, side: Side) -> HostHarness {
        self.side = side;
        self
    }

    /// Sets the granted capabilities, as grant strings.
    pub fn grants(mut self, grants: &[&str]) -> HostHarness {
        self.grants = grants.iter().map(|grant| (*grant).to_owned()).collect();
        self
    }

    /// Sets the manifest limits.
    pub fn limits(mut self, limits: Limits) -> HostHarness {
        self.limits = limits;
        self
    }

    /// Uses this kernel instead of an empty one.
    pub fn kernel(mut self, kernel: Arc<FakeKernel>) -> HostHarness {
        self.kernel = Some(kernel);
        self
    }

    /// Runs this in-process guest instead of a component.
    pub fn with_native_guest(mut self, guest: Box<dyn NativeGuest>) -> HostHarness {
        self.native_guest = Some(guest);
        self
    }

    /// Runs the component compiled from this component-model text.
    pub fn with_wat(mut self, text: &str) -> HostHarness {
        self.wat_text = Some(text.to_owned());
        self
    }

    /// Uses this manual clock.
    pub fn clock(mut self, clock: ManualClock) -> HostHarness {
        self.clock = clock;
        self
    }

    /// Builds the host and activates the plugin.
    pub fn build(self) -> Result<HarnessHandle, HostError> {
        unimplemented!("host::testing::HostHarness::build")
    }
}

impl Default for HostHarness {
    fn default() -> HostHarness {
        HostHarness::new()
    }
}

/// A running host under test.
#[derive(Debug)]
pub struct HarnessHandle {
    clock: ManualClock,
}

impl HarnessHandle {
    /// Queues events for the plugin.
    pub fn deliver(&self, events: Vec<Event>) {
        unimplemented!("host::testing::HarnessHandle::deliver")
    }

    /// Runs the dispatcher until quiet or `max_rounds`.
    pub fn pump(&self, max_rounds: u32) -> PumpReport {
        unimplemented!("host::testing::HarnessHandle::pump")
    }

    /// The outcomes waiting for the plugin.
    pub fn outcomes(&self, plugin: &PluginId) -> Vec<RequestResult> {
        unimplemented!("host::testing::HarnessHandle::outcomes")
    }

    /// The traps that ended calls so far.
    pub fn failures(&self) -> Vec<Trap> {
        unimplemented!("host::testing::HarnessHandle::failures")
    }

    /// The captured log lines.
    pub fn logs(&self) -> Vec<LogLine> {
        unimplemented!("host::testing::HarnessHandle::logs")
    }

    /// The audit records written so far.
    pub fn audit(&self) -> Vec<AuditRecord> {
        unimplemented!("host::testing::HarnessHandle::audit")
    }

    /// The instance state.
    pub fn state(&self) -> InstanceState {
        unimplemented!("host::testing::HarnessHandle::state")
    }

    /// Moves the manual clock forward and fires due timers.
    pub fn advance_clock(&mut self, by: Duration) {
        unimplemented!("host::testing::HarnessHandle::advance_clock")
    }
}
