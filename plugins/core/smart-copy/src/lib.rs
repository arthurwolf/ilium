//! Core plugin `ilium.core.smart-copy` (manual page core-plugin-smart-copy.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::{Handler, Lifecycle, ilium_plugin};
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;

/// The server side of `ilium.core.smart-copy` (core-plugin-smart-copy.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct SmartCopyServer;

impl Lifecycle for SmartCopyServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.smart-copy: server lifecycle.activate")
    }
}

impl Handler for SmartCopyServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.smart-copy: server handler.on_events")
    }
}

/// The client side of `ilium.core.smart-copy` (core-plugin-smart-copy.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct SmartCopyClient;

impl Lifecycle for SmartCopyClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.smart-copy: client lifecycle.activate")
    }
}

impl Handler for SmartCopyClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.smart-copy: client handler.on_events")
    }
}
