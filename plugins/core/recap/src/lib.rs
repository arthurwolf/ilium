//! Core plugin `ilium.core.recap` (manual page core-plugin-recap.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::{Handler, JobRunner, Lifecycle, ServiceProvider, ilium_plugin};
use ilium_plugin_sdk::generated::exports::service_provider::ServiceCall;
use ilium_plugin_sdk::generated::exports::service_provider::ServiceReply;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;

/// The server side of `ilium.core.recap` (core-plugin-recap.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["job-runner", "service-provider"])]
#[derive(Default)]
pub struct RecapServer;

impl Lifecycle for RecapServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.recap: server lifecycle.activate")
    }
}

impl Handler for RecapServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.recap: server handler.on_events")
    }
}

impl JobRunner for RecapServer {
    fn run(&mut self, _kind: String, _input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        unimplemented!("ilium.core.recap: server job-runner.run")
    }
}

impl ServiceProvider for RecapServer {
    fn handle(&mut self, _call: ServiceCall) -> ServiceReply {
        unimplemented!("ilium.core.recap: server service-provider.handle")
    }
}

/// The client side of `ilium.core.recap` (core-plugin-recap.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct RecapClient;

impl Lifecycle for RecapClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.recap: client lifecycle.activate")
    }
}

impl Handler for RecapClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.recap: client handler.on_events")
    }
}
