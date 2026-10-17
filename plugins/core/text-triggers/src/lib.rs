//! Core plugin `ilium.core.text-triggers` (manual page core-plugin-text-triggers.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The server side of `ilium.core.text-triggers` (core-plugin-text-triggers.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct TextTriggersServer;

impl Lifecycle for TextTriggersServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.text-triggers: server lifecycle.activate")
    }
}

impl Handler for TextTriggersServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.text-triggers: server handler.on_events")
    }
}

/// The client side of `ilium.core.text-triggers` (core-plugin-text-triggers.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct TextTriggersClient;

impl Lifecycle for TextTriggersClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.text-triggers: client lifecycle.activate")
    }
}

impl Handler for TextTriggersClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.text-triggers: client handler.on_events")
    }
}
