//! The client's plugin host: the only client module that touches `ilium-plugin-host`
//! (test plan section 1.2). Client-side instances render views and handle input.
//! Skeleton: nothing calls it yet, so no existing code path changes behaviour.

// Not wired into the client yet; the implementation phase calls it from the app.
#![allow(dead_code, unused_variables)]

use ilium_plugin_host::lifecycle::{ShutdownReport, StartMode, StartReport};

/// Owns the client-side plugin instances.
#[derive(Debug)]
pub struct ClientPluginHost {
    start_mode: StartMode,
}

impl ClientPluginHost {
    /// A host that will start in the given mode.
    pub fn new(start_mode: StartMode) -> ClientPluginHost {
        ClientPluginHost { start_mode }
    }

    /// Starts the client-side instances of enabled plugins.
    pub fn start(&self) -> StartReport {
        unimplemented!("client::plugin_host::ClientPluginHost::start")
    }

    /// Stops all client-side instances within the shutdown budget.
    pub fn shutdown(&self) -> ShutdownReport {
        unimplemented!("client::plugin_host::ClientPluginHost::shutdown")
    }
}
