//! Core plugin `ilium.core.editor` (manual page core-plugin-editor.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

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
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, ServiceProvider, TokenProvider, Views};

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
    fn tokens(
        &mut self,
        _buffer: BufferId,
        _first_line: u32,
        _lines: Vec<String>,
        _state_in: Option<Vec<u8>>,
    ) -> (Vec<Token>, Option<Vec<u8>>) {
        unimplemented!("ilium.core.editor: client token-provider.tokens")
    }
}

impl ServiceProvider for EditorClient {
    fn handle(&mut self, _call: ServiceCall) -> ServiceReply {
        unimplemented!("ilium.core.editor: client service-provider.handle")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use serde_json::{json, Value};

    /// Top-level keys of the complete manifest shown in core-plugin-editor.md ("Complete manifest").
    const REQUIRED_TOP_LEVEL_KEYS: [&str; 14] = [
        "api",
        "id",
        "name",
        "version",
        "publisher",
        "description",
        "license",
        "server",
        "client",
        "capabilities",
        "activation",
        "unstable",
        "contributes",
        "limits_tier",
    ];

    /// Client exports in the order the manual lists them.
    const CLIENT_EXPORTS: [&str; 3] = ["views", "token-provider", "service-provider"];

    /// The kernel grant that owns the built-in `editor` Settings tab.
    const EDITOR_TAB_GRANT: &str = "settings.write-kernel:editor";

    /// The shipped manifest, parsed from the file next to this crate's `Cargo.toml`.
    fn load_manifest() -> Value {
        serde_json::from_str(include_str!("../manifest.json")).expect("manifest.json is valid JSON")
    }

    /// Required top-level keys absent from `manifest`, in the manual's order.
    fn missing_required_keys(manifest: &Value) -> Vec<&'static str> {
        REQUIRED_TOP_LEVEL_KEYS
            .iter()
            .copied()
            .filter(|key| manifest.get(*key).is_none())
            .collect()
    }

    /// Strings of a JSON array; anything else yields an empty list.
    fn strings_at(value: &Value) -> Vec<String> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn client_exports(manifest: &Value) -> Vec<String> {
        strings_at(&manifest["client"]["exports"])
    }

    fn client_exports_match_manual(manifest: &Value) -> bool {
        client_exports(manifest) == CLIENT_EXPORTS
    }

    /// REQ: COR-050
    #[test]
    fn manifest_has_every_key_of_the_complete_manifest() {
        let manifest = load_manifest();
        assert_eq!(missing_required_keys(&manifest), Vec::<&str>::new());
    }

    /// REQ: COR-050
    #[test]
    fn client_side_entry_world_and_exports_match_the_manual() {
        let manifest = load_manifest();
        assert_eq!(manifest["id"], "ilium.core.editor");
        assert_eq!(manifest["client"]["entry"], "client.wasm");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
        assert_eq!(client_exports(&manifest), CLIENT_EXPORTS);
    }

    /// REQ: COR-050
    #[test]
    fn server_side_part_exports_only_service_provider() {
        let manifest = load_manifest();
        assert_eq!(manifest["server"]["entry"], "server.wasm");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(
            strings_at(&manifest["server"]["exports"]),
            ["service-provider"]
        );
    }

    /// REQ: COR-030
    #[test]
    fn client_capabilities_never_hold_the_editor_kernel_grant() {
        let manifest = load_manifest();
        let client_capabilities = strings_at(&manifest["capabilities"]["client"]);
        assert!(!client_capabilities
            .iter()
            .any(|capability| capability == EDITOR_TAB_GRANT));
    }

    /// REQ: COR-030
    #[test]
    fn exactly_one_side_holds_the_editor_kernel_grant_on_the_server() {
        let manifest = load_manifest();
        let client_capabilities = strings_at(&manifest["capabilities"]["client"]);
        let server_capabilities = strings_at(&manifest["capabilities"]["server"]);
        let holder_count = client_capabilities
            .iter()
            .chain(server_capabilities.iter())
            .filter(|capability| capability == &EDITOR_TAB_GRANT)
            .count();
        assert_eq!(holder_count, 1);
        assert!(server_capabilities
            .iter()
            .any(|capability| capability == EDITOR_TAB_GRANT));
    }

    /// REQ: COR-050
    #[test]
    fn activation_is_lazy_on_the_editor_pane_kind() {
        let manifest = load_manifest();
        assert_eq!(strings_at(&manifest["activation"]), ["pane-kind:editor"]);
    }

    /// REQ: SEC-293
    #[test]
    fn before_save_hook_carries_high_risk_and_the_manual_limits() {
        let manifest = load_manifest();
        let extension_points = manifest["contributes"]["extension_points"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let before_save = extension_points
            .iter()
            .find(|point| point["id"] == "before-save")
            .expect("before-save extension point is declared");
        assert_eq!(before_save["risk"], "high");
        assert_eq!(before_save["deadline_ms"], 20);
        assert_eq!(before_save["max_handlers"], 8);
    }

    /// REQ: COR-050
    #[test]
    fn plugin_type_implements_the_traits_behind_every_manifest_export() {
        fn assert_provides_manifest_exports<
            T: Lifecycle + Handler + Views + TokenProvider + ServiceProvider,
        >() {
        }
        assert_provides_manifest_exports::<EditorClient>();
        let _default_client = EditorClient;
    }

    /// REQ: COR-050
    #[test]
    fn empty_or_truncated_manifest_is_rejected() {
        assert!(serde_json::from_str::<Value>("").is_err());
        assert!(serde_json::from_str::<Value>("{\"id\": \"ilium.core.editor\"").is_err());
        assert_eq!(
            missing_required_keys(&json!({})),
            REQUIRED_TOP_LEVEL_KEYS.to_vec()
        );
    }

    /// REQ: COR-050
    #[test]
    fn manifest_without_the_views_export_does_not_match_the_manual() {
        let mut manifest = load_manifest();
        manifest["client"]["exports"] = json!(["token-provider", "service-provider"]);
        assert!(!client_exports_match_manual(&manifest));
        assert_eq!(
            client_exports(&manifest),
            ["token-provider", "service-provider"]
        );
    }
}
