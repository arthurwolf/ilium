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
#[cfg(test)]
mod spec_tests {
    use super::*;
    use serde_json::{json, Value};

    /// The manifest shipped beside this crate; the page's "Complete manifest" is its specification.
    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    /// The four essential plugins that can never be disabled (core-plugins.md, LIF-276).
    const ESSENTIAL_PLUGIN_IDS: [&str; 4] = [
        "ilium.core.tree",
        "ilium.core.layout",
        "ilium.core.settings",
        "ilium.core.sessions",
    ];

    fn manifest() -> Value {
        serde_json::from_str(MANIFEST_TEXT).expect("manifest.json is valid JSON")
    }

    fn string_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("value is an array")
            .iter()
            .map(|item| item.as_str().expect("array item is a string").to_string())
            .collect()
    }

    fn extension_point(document: &Value, point_id: &str) -> Value {
        document["contributes"]["extension_points"]
            .as_array()
            .expect("extension_points is an array")
            .iter()
            .find(|point| point["id"] == point_id)
            .cloned()
            .expect("the extension point is declared")
    }

    /// Keys named in a JSON schema's `required` list that the value does not carry.
    fn missing_required_keys(schema: &Value, value: &Value) -> Vec<String> {
        string_list(&schema["required"])
            .into_iter()
            .filter(|key| value.get(key.as_str()).is_none())
            .collect()
    }

    /// REQ: COR-050
    #[test]
    fn cor_050_manifest_names_the_plugin_publisher_and_both_worlds() {
        let document = manifest();
        assert_eq!(document["api"], "ilium:plugin@0.1.0");
        assert_eq!(document["id"], "ilium.core.search");
        assert_eq!(document["publisher"], "Ilium");
        assert_eq!(document["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(document["client"]["world"], "ilium:client/base@0.1.0");
        // The crate declares both sides, as the page's sides line says.
        let _server_side = SearchServer::default();
        let _client_side = SearchClient::default();
    }

    /// REQ: COR-050
    #[test]
    fn cor_050_manifest_lists_the_exports_and_grants_per_side() {
        let document = manifest();
        assert_eq!(
            string_list(&document["server"]["exports"]),
            vec!["job-runner"]
        );
        assert_eq!(string_list(&document["client"]["exports"]), vec!["views"]);
        assert_eq!(
            string_list(&document["capabilities"]["client"]),
            vec!["ui.panels"]
        );
        let server_grants = string_list(&document["capabilities"]["server"]);
        for grant in [
            "inference.embed",
            "tree.read",
            "terminal.read-screen",
            "fs.read:project",
            "jobs.run",
        ] {
            assert!(
                server_grants.iter().any(|declared| declared == grant),
                "server grant {grant} is missing"
            );
        }
    }

    /// REQ: COR-050
    #[test]
    fn cor_050_activation_is_the_open_command_and_the_search_panel() {
        let document = manifest();
        assert_eq!(
            string_list(&document["activation"]),
            vec!["command:ilium.core.search/open", "view:search"]
        );
        let open_command_id = document["contributes"]["commands"][0]["id"]
            .as_str()
            .expect("command id is a string");
        let binding = &document["contributes"]["keybindings"][0];
        assert_eq!(open_command_id, "open");
        assert_eq!(binding["command"], json!("ilium.core.search/open"));
        assert_eq!(binding["chord"], json!("/"));
        assert_eq!(binding["mode"], json!("prefix"));
    }

    /// REQ: COR-050
    #[test]
    fn cor_050_sources_point_is_a_standard_collect_point_capped_at_64_entries() {
        let point = extension_point(&manifest(), "sources");
        assert_eq!(point["kind"], json!("collect"));
        assert_eq!(point["risk"], json!("standard"));
        assert_eq!(point["max_entries"], json!(64));
        assert_eq!(point["dynamic"], json!(false));
    }

    /// REQ: COR-050
    #[test]
    fn cor_050_rank_hook_is_a_transform_with_a_20_ms_deadline_and_eight_handlers() {
        let point = extension_point(&manifest(), "rank");
        assert_eq!(point["kind"], json!("hook"));
        assert_eq!(point["mode"], json!("transform"));
        assert_eq!(point["deadline_ms"], json!(20));
        assert_eq!(point["max_handlers"], json!(8));
    }

    /// REQ: COR-050
    #[test]
    fn cor_050_source_entry_without_a_service_fails_its_schema_and_a_complete_one_passes() {
        let point = extension_point(&manifest(), "sources");
        let incomplete = json!({ "id": "issues", "title": "Issues", "kind": "issue" });
        let complete = json!({ "id": "issues", "title": "Issues", "kind": "issue", "service": "example.issues/search" });
        assert_eq!(
            missing_required_keys(&point["entry_schema"], &incomplete),
            vec!["service".to_string()]
        );
        assert!(missing_required_keys(&point["entry_schema"], &complete).is_empty());
    }

    /// REQ: COR-050
    #[test]
    fn cor_050_truncated_manifest_text_is_rejected_as_not_a_manifest() {
        let truncated = MANIFEST_TEXT.trim_end().trim_end_matches('}');
        assert!(serde_json::from_str::<Value>(truncated).is_err());
        assert!(serde_json::from_str::<Value>("").is_err());
    }

    /// REQ: LIF-276
    #[test]
    fn lif_276_search_is_not_essential_so_the_plugins_screen_offers_its_switch() {
        assert_eq!(ESSENTIAL_PLUGIN_IDS.len(), 4);
        let plugin_id = manifest()["id"]
            .as_str()
            .expect("id is a string")
            .to_string();
        assert!(!ESSENTIAL_PLUGIN_IDS.contains(&plugin_id.as_str()));
    }
}
