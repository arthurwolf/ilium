//! Core plugin `ilium.core.editor` (manual page core-plugin-editor.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::{Handler, Lifecycle, ServiceProvider, TokenProvider, Views, ilium_plugin};
use ilium_plugin_sdk::generated::exports::service_provider::ServiceCall;
use ilium_plugin_sdk::generated::exports::service_provider::ServiceReply;
use ilium_plugin_sdk::generated::exports::token_provider::Token;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::ids::BufferId;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::generated::types::layout::Area;
use ilium_plugin_sdk::generated::types::ui::UiTree;
use ilium_plugin_sdk::generated::types::ui::ViewTarget;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;

/// The client side of `ilium.core.editor` (core-plugin-editor.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views", "token-provider", "service-provider"])]
#[derive(Default)]
pub struct EditorClient;

impl Lifecycle for EditorClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.editor: client lifecycle.activate")
    }
}

impl Handler for EditorClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.editor: client handler.on_events")
    }
}

impl Views for EditorClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.editor: client views.render")
    }
}

impl TokenProvider for EditorClient {
    fn tokens( &mut self, _buffer: BufferId, _first_line: u32, _lines: Vec<String>, _state_in: Option<Vec<u8>>, ) -> (Vec<Token>, Option<Vec<u8>>) {
        unimplemented!("ilium.core.editor: client token-provider.tokens")
    }
}

impl ServiceProvider for EditorClient {
    fn handle(&mut self, _call: ServiceCall) -> ServiceReply {
        unimplemented!("ilium.core.editor: client service-provider.handle")
    }
}
