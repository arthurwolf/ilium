//! Core plugin `ilium.core.session-convert` (manual page core-plugin-session-convert.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::exports::menu_provider::MenuContext;
use ilium_plugin_sdk::generated::exports::menu_provider::MenuEntry;
use ilium_plugin_sdk::generated::exports::service_provider::ServiceCall;
use ilium_plugin_sdk::generated::exports::service_provider::ServiceReply;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::generated::types::layout::Area;
use ilium_plugin_sdk::generated::types::ui::UiTree;
use ilium_plugin_sdk::generated::types::ui::ViewTarget;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{
    ilium_plugin, Handler, JobRunner, Lifecycle, MenuProvider, ServiceProvider, Views,
};

/// The server side of `ilium.core.session-convert` (core-plugin-session-convert.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["job-runner", "service-provider"])]
#[derive(Default)]
pub struct SessionConvertServer;

impl Lifecycle for SessionConvertServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.session-convert: server lifecycle.activate")
    }
}

impl Handler for SessionConvertServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.session-convert: server handler.on_events")
    }
}

impl JobRunner for SessionConvertServer {
    fn run(&mut self, _kind: String, _input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        unimplemented!("ilium.core.session-convert: server job-runner.run")
    }
}

impl ServiceProvider for SessionConvertServer {
    fn handle(&mut self, _call: ServiceCall) -> ServiceReply {
        unimplemented!("ilium.core.session-convert: server service-provider.handle")
    }
}

/// The client side of `ilium.core.session-convert` (core-plugin-session-convert.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views", "menu-provider"])]
#[derive(Default)]
pub struct SessionConvertClient;

impl Lifecycle for SessionConvertClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.session-convert: client lifecycle.activate")
    }
}

impl Handler for SessionConvertClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.session-convert: client handler.on_events")
    }
}

impl Views for SessionConvertClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.session-convert: client views.render")
    }
}

impl MenuProvider for SessionConvertClient {
    fn menu_items(&mut self, _context: MenuContext) -> Vec<MenuEntry> {
        unimplemented!("ilium.core.session-convert: client menu-provider.menu_items")
    }
}
