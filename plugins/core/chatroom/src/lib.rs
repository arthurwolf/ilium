//! Core plugin `ilium.core.chatroom` (manual page core-plugin-chatroom.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::exports::VirtualRowChild;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, VirtualRowsProvider};

/// The server side of `ilium.core.chatroom` (core-plugin-chatroom.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["virtual-rows-provider"])]
#[derive(Default)]
pub struct ChatroomServer;

impl Lifecycle for ChatroomServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.chatroom: server lifecycle.activate")
    }
}

impl Handler for ChatroomServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.chatroom: server handler.on_events")
    }
}

/// The `virtual-rows-provider` export of the server: the unread-messages tree row (core-plugin-chatroom.md "Tree row", reference-wit.md section 8.12).
impl VirtualRowsProvider for ChatroomServer {
    fn list_virtual_children(
        &mut self,
        _row_kind: String,
        _parent: String,
        _cursor: Option<String>,
    ) -> Result<(Vec<VirtualRowChild>, Option<String>), PluginError> {
        unimplemented!("ilium.core.chatroom: server virtual-rows-provider.list_virtual_children")
    }
}

/// The client side of `ilium.core.chatroom` (core-plugin-chatroom.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct ChatroomClient;

impl Lifecycle for ChatroomClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.chatroom: client lifecycle.activate")
    }
}

impl Handler for ChatroomClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.chatroom: client handler.on_events")
    }
}
