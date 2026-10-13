//! Core plugin `ilium.core.diagnostics` (manual page core-plugin-diagnostics.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::{Handler, Lifecycle, ServiceProvider, Views, ilium_plugin};
use ilium_plugin_sdk::generated::exports::service_provider::ServiceCall;
use ilium_plugin_sdk::generated::exports::service_provider::ServiceReply;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::generated::types::layout::Area;
use ilium_plugin_sdk::generated::types::ui::UiTree;
use ilium_plugin_sdk::generated::types::ui::ViewTarget;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;

/// The server side of `ilium.core.diagnostics` (core-plugin-diagnostics.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
pub struct DiagnosticsServer;

impl Lifecycle for DiagnosticsServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.diagnostics: server lifecycle.activate")
    }
}

impl Handler for DiagnosticsServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.diagnostics: server handler.on_events")
    }
}

impl ServiceProvider for DiagnosticsServer {
    fn handle(&mut self, _call: ServiceCall) -> ServiceReply {
        unimplemented!("ilium.core.diagnostics: server service-provider.handle")
    }
}

/// The client side of `ilium.core.diagnostics` (core-plugin-diagnostics.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct DiagnosticsClient;

impl Lifecycle for DiagnosticsClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.diagnostics: client lifecycle.activate")
    }
}

impl Handler for DiagnosticsClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.diagnostics: client handler.on_events")
    }
}

impl Views for DiagnosticsClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.diagnostics: client views.render")
    }
}
