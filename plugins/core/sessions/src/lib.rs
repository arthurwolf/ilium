//! Core plugin `ilium.core.sessions` (manual page core-plugin-sessions.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::{Handler, Lifecycle, ilium_plugin};
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;

/// The server side of `ilium.core.sessions` (core-plugin-sessions.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct SessionsServer;

impl Lifecycle for SessionsServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.sessions: server lifecycle.activate")
    }
}

impl Handler for SessionsServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.sessions: server handler.on_events")
    }
}

/// The client side of `ilium.core.sessions` (core-plugin-sessions.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct SessionsClient;

impl Lifecycle for SessionsClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.sessions: client lifecycle.activate")
    }
}

impl Handler for SessionsClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.sessions: client handler.on_events")
    }
}
