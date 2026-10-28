//! Core plugin `ilium.core.voice` (manual page core-plugin-voice.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Minimal conforming bodies: no contributions are produced until the feature logic moves here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The server side of `ilium.core.voice` (core-plugin-voice.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct VoiceServer;

impl Lifecycle for VoiceServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for VoiceServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

/// The client side of `ilium.core.voice` (core-plugin-voice.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct VoiceClient;

impl Lifecycle for VoiceClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for VoiceClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    const REQUIRED_TOP_LEVEL_KEYS: &[&str] = &[
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
        "limits_tier",
        "contributes",
    ];

    fn parse_manifest(text: &str) -> Result<serde_json::Value, String> {
        let value: serde_json::Value = serde_json::from_str(text)
            .map_err(|error| format!("manifest is not valid JSON: {error}"))?;
        if value.is_object() {
            Ok(value)
        } else {
            Err("manifest must be a JSON object".to_string())
        }
    }

    fn load_manifest() -> serde_json::Value {
        parse_manifest(MANIFEST_TEXT).expect("manifest.json parses as a JSON object")
    }

    fn string_list(value: &serde_json::Value) -> Vec<String> {
        value
            .as_array()
            .expect("expected a JSON array")
            .iter()
            .map(|item| item.as_str().expect("expected a string").to_string())
            .collect()
    }

    fn missing_required_keys(manifest: &serde_json::Value) -> Vec<&'static str> {
        REQUIRED_TOP_LEVEL_KEYS
            .iter()
            .copied()
            .filter(|key| manifest.get(*key).is_none())
            .collect()
    }

    fn extension_point<'a>(
        manifest: &'a serde_json::Value,
        point_id: &str,
    ) -> &'a serde_json::Value {
        manifest["contributes"]["extension_points"]
            .as_array()
            .expect("contributes.extension_points is an array")
            .iter()
            .find(|point| point["id"].as_str() == Some(point_id))
            .expect("extension point is declared in contributes")
    }

    /// REQ: COR-050
    #[test]
    fn manifest_names_the_plugin_id_and_api_version() {
        let manifest = load_manifest();
        assert_eq!(manifest["id"].as_str(), Some("ilium.core.voice"));
        assert_eq!(manifest["api"].as_str(), Some("ilium:plugin@0.1.0"));
    }

    /// REQ: COR-050
    #[test]
    fn empty_or_non_object_manifest_text_is_rejected() {
        assert!(parse_manifest("").is_err(), "empty input must not parse");
        assert!(
            parse_manifest("[]").is_err(),
            "a JSON array is not a manifest"
        );
    }

    /// REQ: COR-050
    #[test]
    fn manifest_missing_a_required_key_is_detected() {
        let mut manifest = load_manifest();
        manifest
            .as_object_mut()
            .expect("manifest is an object")
            .remove("activation");
        assert_eq!(missing_required_keys(&manifest), vec!["activation"]);
    }

    /// REQ: COR-050
    #[test]
    fn activation_is_the_toggle_command_and_the_cli_verb() {
        let manifest = load_manifest();
        assert_eq!(
            string_list(&manifest["activation"]),
            vec!["command:voice.toggle".to_string(), "cli:voice".to_string()]
        );
    }

    /// REQ: COR-050
    #[test]
    fn each_side_names_its_wasm_world_and_has_a_plugin_struct() {
        let manifest = load_manifest();
        assert_eq!(
            manifest["server"]["world"].as_str(),
            Some("ilium:server/base@0.1.0")
        );
        assert_eq!(
            manifest["client"]["world"].as_str(),
            Some("ilium:client/base@0.1.0")
        );
        let _server = VoiceServer;
        let _client = VoiceClient;
    }

    /// REQ: COR-030
    #[test]
    fn voice_control_tab_is_granted_once_and_only_on_the_client() {
        let manifest = load_manifest();
        let client = string_list(&manifest["capabilities"]["client"]);
        let server = string_list(&manifest["capabilities"]["server"]);
        let tab_grant = "settings.write-kernel:voice-control";
        assert_eq!(
            client
                .iter()
                .filter(|grant| grant.as_str() == tab_grant)
                .count(),
            1
        );
        assert!(
            !server
                .iter()
                .any(|grant| grant.starts_with("settings.write-kernel:")),
            "the server side must not hold a kernel settings tab"
        );
    }

    /// REQ: EXT-052
    #[test]
    fn transcript_point_uses_the_20_ms_deadline_and_8_handler_cap() {
        let manifest = load_manifest();
        let transcript = extension_point(&manifest, "transcript");
        assert_eq!(transcript["deadline_ms"].as_u64(), Some(20));
        assert_eq!(transcript["max_handlers"].as_u64(), Some(8));
        assert_eq!(transcript["version"].as_str(), Some("0.1.0"));
    }

    /// REQ: EXT-052
    #[test]
    fn voice_intents_point_caps_entries_at_64_and_is_not_dynamic() {
        let manifest = load_manifest();
        let voice_intents = extension_point(&manifest, "voice-intents");
        assert_eq!(voice_intents["max_entries"].as_u64(), Some(64));
        assert_eq!(voice_intents["dynamic"].as_bool(), Some(false));
    }

    /// REQ: EXT-052
    #[test]
    fn every_extension_point_has_a_matching_client_provide_grant() {
        let manifest = load_manifest();
        let client = string_list(&manifest["capabilities"]["client"]);
        for point_id in ["transcript", "voice-intents"] {
            let grant = format!("extensions.provide:ilium.core.voice/{point_id}");
            assert!(client.contains(&grant), "missing grant {grant}");
        }
    }

    /// REQ: SEC-293
    #[test]
    fn extension_point_risk_marks_follow_the_corrected_table() {
        let manifest = load_manifest();
        assert_eq!(
            extension_point(&manifest, "transcript")["risk"].as_str(),
            Some("high")
        );
        assert_eq!(
            extension_point(&manifest, "voice-intents")["risk"].as_str(),
            Some("standard")
        );
    }
}
