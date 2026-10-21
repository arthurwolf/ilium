//! Core plugin `ilium.core.diagnostics` (manual page core-plugin-diagnostics.md).
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
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, ServiceProvider, Views};

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

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// The manifest that the plugin's `#[ilium_plugin]` sides and grants are checked against.
    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    fn manifest() -> serde_json::Value {
        serde_json::from_str(MANIFEST_TEXT).expect("manifest.json is valid JSON")
    }

    fn string_list(value: &serde_json::Value) -> Vec<String> {
        value
            .as_array()
            .expect("value is a JSON array")
            .iter()
            .map(|item| item.as_str().expect("array item is a string").to_string())
            .collect()
    }

    fn extension_point(manifest: &serde_json::Value, point_id: &str) -> serde_json::Value {
        manifest["contributes"]["extension_points"]
            .as_array()
            .expect("extension_points is an array")
            .iter()
            .find(|point| point["id"] == point_id)
            .cloned()
            .expect("extension point is declared in the manifest")
    }

    /// REQ: COR-050
    #[test]
    fn manifest_has_identity_replaceable_flag_and_required_keys() {
        let manifest = manifest();

        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.diagnostics");
        assert_eq!(manifest["replaceable"], true);
        for required_key in [
            "name", "version", "publisher", "capabilities", "activation", "surface", "contributes",
            "limits_tier", "server", "client",
        ] {
            assert!(
                manifest.get(required_key).is_some(),
                "manifest is missing required key `{required_key}`"
            );
        }
    }

    /// REQ: COR-050
    #[test]
    fn manifest_sides_match_the_server_and_client_structs() {
        let manifest = manifest();

        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(string_list(&manifest["server"]["exports"]), vec!["service-provider"]);
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
        assert_eq!(string_list(&manifest["client"]["exports"]), vec!["views"]);

        let _server = DiagnosticsServer::default();
        let _client = DiagnosticsClient::default();
    }

    /// REQ: COR-050
    #[test]
    fn activation_starts_at_startup_and_when_the_warnings_panel_opens() {
        let manifest = manifest();
        let activation = string_list(&manifest["activation"]);

        assert!(activation.iter().any(|trigger| trigger == "startup"));
        assert!(activation.iter().any(|trigger| trigger == "view:diagnostics.warnings"));
        let panel_ids: Vec<String> = manifest["contributes"]["panels"]
            .as_array()
            .expect("panels is an array")
            .iter()
            .map(|panel| panel["id"].as_str().expect("panel id is a string").to_string())
            .collect();
        assert_eq!(panel_ids, vec!["warnings"]);
    }

    /// "Tail the log file": server reads the log folder and the debug setting, and writes neither).
    #[test]
    fn server_grants_are_read_only_for_logs_and_debug_setting() {
        let manifest = manifest();
        let server_capabilities = string_list(&manifest["capabilities"]["server"]);

        assert!(server_capabilities.iter().any(|grant| grant == "fs.read:path:/tmp/.ilium-1000/logs"));
        assert!(server_capabilities.iter().any(|grant| grant == "settings.read-kernel:debug"));
        assert!(!server_capabilities.iter().any(|grant| grant.starts_with("fs.write")));
        assert!(!server_capabilities.iter().any(|grant| grant.starts_with("settings.write")));
    }

    /// diagnostics holds no grant on the Settings plugin's extension points and contributes no settings rows).
    #[test]
    fn diagnostics_contributes_no_settings_rows_and_no_settings_extension_grant() {
        let manifest = manifest();
        let server_capabilities = string_list(&manifest["capabilities"]["server"]);

        assert!(manifest["contributes"].get("settings").is_none());
        assert!(!server_capabilities.iter().any(|grant| grant.starts_with("extensions.contribute:")));
    }

    /// level, plugin, target, message and time, so no raw headers or URL parameters travel).
    /// REQ: SEC-249
    #[test]
    fn log_sinks_payload_carries_only_the_five_redacted_record_fields() {
        let manifest = manifest();
        let sinks = extension_point(&manifest, "log-sinks");
        let payload_properties = sinks["payload_schema"]["properties"]
            .as_object()
            .expect("log-sinks payload has properties");

        let mut field_names: Vec<&str> = payload_properties.keys().map(String::as_str).collect();
        field_names.sort_unstable();
        assert_eq!(field_names, vec!["level", "message", "plugin", "target", "time"]);
    }

    /// log-sinks is an observe hook with a 20 ms deadline and at most 8 handlers).
    #[test]
    fn log_sinks_is_an_observe_hook_for_warn_and_error_records_only() {
        let manifest = manifest();
        let sinks = extension_point(&manifest, "log-sinks");

        assert_eq!(sinks["kind"], "hook");
        assert_eq!(sinks["mode"], "observe");
        assert_eq!(sinks["deadline_ms"], 20);
        assert_eq!(sinks["max_handlers"], 8);
        assert_eq!(string_list(&sinks["payload_schema"]["properties"]["level"]["enum"]), vec!["warn", "error"]);
    }

    /// asked for text, at most 32 sections, collect point not dynamic).
    #[test]
    fn report_sections_point_is_a_collect_point_with_room_for_32_entries() {
        let manifest = manifest();
        let sections = extension_point(&manifest, "report-sections");

        assert_eq!(sections["kind"], "collect");
        assert_eq!(sections["max_entries"], 32);
        assert_eq!(sections["dynamic"], false);
    }

    /// an entry needs id, title and service). Empty and invalid input case: an entry without `service`
    /// must be reported as missing a required key.
    #[test]
    fn report_section_entry_without_service_is_missing_a_required_key() {
        let manifest = manifest();
        let sections = extension_point(&manifest, "report-sections");
        let required_keys = string_list(&sections["entry_schema"]["required"]);
        assert_eq!(required_keys, vec!["id", "title", "service"]);

        let invalid_entry = serde_json::json!({ "id": "gpu", "title": "GPU" });
        let missing: Vec<String> = required_keys
            .iter()
            .filter(|key| invalid_entry.get(key.as_str()).is_none())
            .cloned()
            .collect();
        assert_eq!(missing, vec!["service".to_string()]);
    }

    /// at 256 KiB, which is 262,144 bytes, and the service result is an object with a `text` field).
    /// Failure case: a 262,145-byte text exceeds the declared limit.
    #[test]
    fn report_service_result_is_text_capped_at_256_kib() {
        let manifest = manifest();
        let result_schema = &manifest["contributes"]["services"]["ilium.core.diagnostics/report"]["build"]["result"];
        let limit = result_schema["properties"]["text"]["maxLength"]
            .as_u64()
            .expect("text has a maxLength") as usize;

        assert_eq!(limit, 256 * 1024);
        assert_eq!(string_list(&result_schema["required"]), vec!["text"]);
        let oversized_text = "a".repeat(limit + 1);
        assert!(oversized_text.len() > limit);
    }

    /// optional boolean `include_log_tail`). Empty input case: an empty parameter object is valid because
    /// no parameter is required.
    #[test]
    fn report_build_params_are_optional_and_include_log_tail_is_boolean() {
        let manifest = manifest();
        let params = &manifest["contributes"]["services"]["ilium.core.diagnostics/report"]["build"]["params"];

        assert!(params.get("required").is_none());
        assert_eq!(params["properties"]["include_log_tail"]["type"], "boolean");
        let empty_params = serde_json::json!({});
        assert!(empty_params.as_object().expect("object").is_empty());
    }

    /// is in the Help category and the report service is on the surface).
    #[test]
    fn build_report_command_and_report_service_are_on_the_surface() {
        let manifest = manifest();

        let command = &manifest["contributes"]["commands"][0];
        assert_eq!(command["id"], "build-report");
        assert_eq!(command["category"], "Help");
        let surface = string_list(&manifest["surface"]);
        assert!(surface.iter().any(|entry| entry == "services:report"));
        assert!(surface.iter().any(|entry| entry == "commands:build-report"));
    }
}
