//! Core plugin `ilium.core.kanban` (manual page core-plugin-kanban.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The client side of `ilium.core.kanban` (core-plugin-kanban.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct KanbanClient;

impl Lifecycle for KanbanClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.kanban: client lifecycle.activate")
    }
}

impl Handler for KanbanClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.kanban: client handler.on_events")
    }
}
