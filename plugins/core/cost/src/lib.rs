//! Core plugin `ilium.core.cost` (manual page core-plugin-cost.md).
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
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, ServiceProvider};

/// The server side of `ilium.core.cost` (core-plugin-cost.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
pub struct CostServer;

impl Lifecycle for CostServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.cost: server lifecycle.activate")
    }
}

impl Handler for CostServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.cost: server handler.on_events")
    }
}

impl ServiceProvider for CostServer {
    fn handle(&mut self, _call: ServiceCall) -> ServiceReply {
        unimplemented!("ilium.core.cost: server service-provider.handle")
    }
}

/// The client side of `ilium.core.cost` (core-plugin-cost.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct CostClient;

impl Lifecycle for CostClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.cost: client lifecycle.activate")
    }
}

impl Handler for CostClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.cost: client handler.on_events")
    }
}

impl Views for CostClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.cost: client views.render")
    }
}
