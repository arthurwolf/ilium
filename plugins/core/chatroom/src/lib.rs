//! Core plugin `ilium.core.chatroom` (manual page core-plugin-chatroom.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::exports::VirtualRowChild;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::generated::types::layout::Area;
use ilium_plugin_sdk::generated::types::ui::UiTree;
use ilium_plugin_sdk::generated::types::ui::ViewTarget;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, Views, VirtualRowsProvider};

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
#[ilium_plugin(side = "client", exports = ["views"])]
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

impl Views for ChatroomClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.chatroom: client views.render")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// The shipped manifest, read at compile time so the tests check the same bytes the host loads.
    fn shipped_manifest() -> serde_json::Value {
        serde_json::from_str(include_str!("../manifest.json")).expect("manifest.json is valid JSON")
    }

    fn array_contains(values: &serde_json::Value, expected: &str) -> bool {
        values
            .as_array()
            .map(|entries| entries.iter().any(|entry| entry.as_str() == Some(expected)))
            .unwrap_or(false)
    }

    fn extension_point<'a>(
        manifest: &'a serde_json::Value,
        point_id: &str,
    ) -> Option<&'a serde_json::Value> {
        manifest["contributes"]["extension_points"]
            .as_array()?
            .iter()
            .find(|point| point["id"].as_str() == Some(point_id))
    }

    /// REQ: COR-050
    #[test]
    fn activation_is_startup_and_chatroom_topic() {
        let manifest = shipped_manifest();
        assert!(array_contains(&manifest["activation"], "startup"));
        assert!(array_contains(&manifest["activation"], "topic:chatroom"));
    }

    #[test]
    fn manifest_identifies_the_chatroom_plugin() {
        let manifest = shipped_manifest();
        assert_eq!(manifest["api"].as_str(), Some("ilium:plugin@0.1.0"));
        assert_eq!(manifest["id"].as_str(), Some("ilium.core.chatroom"));
        assert_eq!(manifest["version"].as_str(), Some("0.1.0"));
    }

    #[test]
    fn each_side_names_its_entry_and_world() {
        let manifest = shipped_manifest();
        assert_eq!(manifest["server"]["entry"].as_str(), Some("server.wasm"));
        assert_eq!(
            manifest["server"]["world"].as_str(),
            Some("ilium:server/base@0.1.0")
        );
        assert_eq!(manifest["client"]["entry"].as_str(), Some("client.wasm"));
        assert_eq!(
            manifest["client"]["world"].as_str(),
            Some("ilium:client/base@0.1.0")
        );
    }

    /// and the server grants agent-setup services and the project file scope).
    #[test]
    fn grants_cover_project_files_and_agent_setup_without_agent_config() {
        let manifest = shipped_manifest();
        let server_grants = &manifest["capabilities"]["server"];
        assert!(array_contains(server_grants, "fs.write:project"));
        assert!(array_contains(server_grants, "topics.subscribe:chatroom"));
        assert!(array_contains(
            server_grants,
            "services.use:ilium.core.agent-setup"
        ));
        assert!(!array_contains(server_grants, "agent-config"));
        assert!(!array_contains(
            &manifest["capabilities"]["client"],
            "agent-config"
        ));
    }

    /// Compile-time check: the struct for each side implements the trait its manifest export names.
    #[test]
    fn side_structs_implement_their_manifest_exports() {
        fn assert_virtual_rows_provider<T: VirtualRowsProvider>() {}
        fn assert_views<T: Views>() {}
        assert_virtual_rows_provider::<ChatroomServer>();
        assert_views::<ChatroomClient>();
    }

    #[test]
    fn chatroom_row_lists_the_room_file_for_an_empty_parent() {
        let mut server = ChatroomServer::default();
        let result = server.list_virtual_children("chatroom".to_string(), String::new(), None);
        let (children, cursor) = match result {
            Ok(page) => page,
            Err(_) => panic!("the chatroom row must list its children"),
        };
        assert_eq!(children.len(), 1);
        assert!(cursor.is_none());
    }

    #[test]
    fn unknown_row_kind_is_refused() {
        let mut server = ChatroomServer::default();
        let result = server.list_virtual_children("not-chatroom".to_string(), String::new(), None);
        assert!(result.is_err());
    }

    /// and 8 handlers; message-renderers is a client collect point with 64 entries). Defaults are the manual's.
    /// REQ: EXT-052
    #[test]
    fn owned_extension_points_declare_their_limits() {
        let manifest = shipped_manifest();

        let before_post =
            extension_point(&manifest, "before-post").expect("before-post is declared");
        assert_eq!(before_post["kind"].as_str(), Some("hook"));
        assert_eq!(before_post["mode"].as_str(), Some("transform"));
        assert_eq!(before_post["deadline_ms"].as_u64(), Some(20));
        assert_eq!(before_post["max_handlers"].as_u64(), Some(8));

        let message_renderers =
            extension_point(&manifest, "message-renderers").expect("message-renderers is declared");
        assert_eq!(message_renderers["kind"].as_str(), Some("collect"));
        assert_eq!(message_renderers["max_entries"].as_u64(), Some(64));
    }

    /// REQ: SEC-293
    #[test]
    fn extension_point_risk_marks_follow_the_security_table() {
        let manifest = shipped_manifest();
        let before_post =
            extension_point(&manifest, "before-post").expect("before-post is declared");
        let message_renderers =
            extension_point(&manifest, "message-renderers").expect("message-renderers is declared");
        assert_eq!(before_post["risk"].as_str(), Some("high"));
        assert_eq!(message_renderers["risk"].as_str(), Some("standard"));
    }
}
