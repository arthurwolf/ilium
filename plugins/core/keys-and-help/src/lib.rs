//! Core plugin `ilium.core.keys-and-help` (manual page core-plugin-keys-and-help.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::generated::types::layout::Area;
use ilium_plugin_sdk::generated::types::ui::UiTree;
use ilium_plugin_sdk::generated::types::ui::ViewTarget;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, Views};

/// The client side of `ilium.core.keys-and-help` (core-plugin-keys-and-help.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct KeysAndHelpClient;

impl Lifecycle for KeysAndHelpClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.keys-and-help: client lifecycle.activate")
    }
}

impl Handler for KeysAndHelpClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.keys-and-help: client handler.on_events")
    }
}

impl Views for KeysAndHelpClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.keys-and-help: client views.render")
    }
}
