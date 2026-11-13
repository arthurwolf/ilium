//! Guest fixture `conformance-probe`: the `conformance` test lane writes the probe behaviour
//! (CONTRACT.txt). What is here is the smallest server component the SDK can build: it proves
//! the wasm32 glue of `#[ilium_plugin]` end to end (lifecycle and handler exports, one host
//! import that needs no grant, restore bytes through deactivate).

use ilium_plugin_sdk::host::Level;
use ilium_plugin_sdk::types::{Activation, DeactivationReason, Event, PluginError, Submission};
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The line the probe logs once it is active; host tests look for it.
pub const ACTIVATED_LINE: &str = "conformance-probe activated";

/// The server side of the probe.
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct ConformanceProbe {
    /// Events received since activation; returned as restore bytes on deactivate.
    events_seen: u32,
}

impl Lifecycle for ConformanceProbe {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        self.events_seen = 0;
        ilium_plugin_sdk::log::log(Level::Info, ACTIVATED_LINE);
        Ok(())
    }

    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        Some(self.events_seen.to_be_bytes().to_vec())
    }
}

impl Handler for ConformanceProbe {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let received = u32::try_from(batch.len()).unwrap_or(u32::MAX);
        self.events_seen = self.events_seen.saturating_add(received);
        Vec::new()
    }
}
