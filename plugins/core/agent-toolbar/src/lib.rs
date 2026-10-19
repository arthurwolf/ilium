//! Core plugin `ilium.core.agent-toolbar` (manual page core-plugin-agent-toolbar.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The server side of `ilium.core.agent-toolbar` (core-plugin-agent-toolbar.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct AgentToolbarServer;

impl Lifecycle for AgentToolbarServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.agent-toolbar: server lifecycle.activate")
    }
}

impl Handler for AgentToolbarServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.agent-toolbar: server handler.on_events")
    }
}

/// The client side of `ilium.core.agent-toolbar` (core-plugin-agent-toolbar.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct AgentToolbarClient;

impl Lifecycle for AgentToolbarClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.agent-toolbar: client lifecycle.activate")
    }
}

impl Handler for AgentToolbarClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.agent-toolbar: client handler.on_events")
    }
}
