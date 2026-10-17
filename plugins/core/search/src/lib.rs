//! Core plugin `ilium.core.search` (manual page core-plugin-search.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::exports::service_provider::ServiceCall;
use ilium_plugin_sdk::generated::exports::service_provider::ServiceReply;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::generated::types::layout::Area;
use ilium_plugin_sdk::generated::types::ui::UiTree;
use ilium_plugin_sdk::generated::types::ui::ViewTarget;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, JobRunner, Lifecycle, ServiceProvider, Views};

/// The server side of `ilium.core.search` (core-plugin-search.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["job-runner", "service-provider"])]
#[derive(Default)]
pub struct SearchServer;

impl Lifecycle for SearchServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.search: server lifecycle.activate")
    }
}

impl Handler for SearchServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.search: server handler.on_events")
    }
}

impl JobRunner for SearchServer {
    fn run(&mut self, _kind: String, _input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        unimplemented!("ilium.core.search: server job-runner.run")
    }
}

/// Service `ilium.core.search/query` (core-plugin-search.md "Services"): `query` answers with
/// `Done` JSON `{ "hits": [...] }`.
impl ServiceProvider for SearchServer {
    fn handle(&mut self, _call: ServiceCall) -> ServiceReply {
        unimplemented!("ilium.core.search: server service-provider.handle")
    }
}

/// The client side of `ilium.core.search` (core-plugin-search.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct SearchClient;

impl Lifecycle for SearchClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.search: client lifecycle.activate")
    }
}

impl Handler for SearchClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.search: client handler.on_events")
    }
}

impl Views for SearchClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.search: client views.render")
    }
}
