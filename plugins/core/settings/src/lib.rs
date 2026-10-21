//! Core plugin `ilium.core.settings` (manual page core-plugin-settings.md).
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

/// The client side of `ilium.core.settings` (core-plugin-settings.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct SettingsClient;

impl Lifecycle for SettingsClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.settings: client lifecycle.activate")
    }
}

impl Handler for SettingsClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.settings: client handler.on_events")
    }
}

impl Views for SettingsClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.settings: client views.render")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use ilium_plugin_sdk::generated::types::events::{SettingsChange, SettingsOrigin};
    use ilium_plugin_sdk::generated::types::intents::PluginSide;

    const MANIFEST_JSON: &str = include_str!("../manifest.json");

    fn manifest() -> serde_json::Value {
        serde_json::from_str(MANIFEST_JSON).expect("manifest.json is valid JSON")
    }

    fn client_grants() -> Vec<String> {
        manifest()["capabilities"]["client"]
            .as_array()
            .expect("capabilities.client is an array")
            .iter()
            .map(|grant| grant.as_str().expect("each grant is a string").to_string())
            .collect()
    }

    fn settings_changed(keys: &[&str]) -> Event {
        Event::SettingsChanged(SettingsChange {
            keys: keys.iter().map(|key| key.to_string()).collect(),
            origin: SettingsOrigin::User,
            side: PluginSide::Client,
        })
    }

    /// kernel tabs (user-interface, icons, debug, about).
    /// REQ: COR-030
    #[test]
    fn manifest_declares_write_kernel_for_each_owned_tab() {
        let grants = client_grants();
        for tab in ["user-interface", "icons", "debug", "about"] {
            let grant = format!("settings.write-kernel:{tab}");
            assert!(grants.contains(&grant), "missing grant {grant}");
        }
    }

    /// tabs, and no grant declared twice.
    /// REQ: COR-030
    #[test]
    fn manifest_holds_no_extra_or_duplicate_kernel_tab_grant() {
        let grants = client_grants();
        let kernel_grants: Vec<&String> = grants
            .iter()
            .filter(|grant| grant.starts_with("settings.write-kernel:"))
            .collect();
        assert_eq!(kernel_grants.len(), 4, "exactly four kernel tabs are owned");
        let mut unique_grants = kernel_grants.clone();
        unique_grants.sort();
        unique_grants.dedup();
        assert_eq!(unique_grants.len(), kernel_grants.len(), "a kernel-tab grant is declared twice");
    }

    /// world and exports only `views`.
    /// REQ: COR-050
    #[test]
    fn manifest_declares_startup_activation_and_views_export() {
        let manifest = manifest();
        assert_eq!(manifest["activation"], serde_json::json!(["startup"]));
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
        assert_eq!(manifest["client"]["exports"], serde_json::json!(["views"]));
        assert_eq!(manifest["limits_tier"], "first-party");
    }

    /// panel exactly as the manual's Complete manifest section lists them.
    /// REQ: COR-050
    #[test]
    fn manifest_declares_first_run_step_and_overview_panel() {
        let manifest = manifest();
        let contributes = &manifest["contributes"];
        assert_eq!(
            contributes["onboarding_steps"],
            serde_json::json!([{ "id": "first-run", "title": "Welcome", "order": 10 }])
        );
        assert_eq!(
            contributes["panels"],
            serde_json::json!([{ "id": "overview", "title": "All settings", "modal": false }])
        );
    }

    /// accent `#4a9eff` with theme swatches.
    /// REQ: COR-011
    #[test]
    fn appearance_section_keeps_manual_defaults() {
        let manifest = manifest();
        let section = &manifest["contributes"]["settings"][0];
        assert_eq!(section["id"], "appearance");
        assert_eq!(section["tab"], "user-interface");
        assert_eq!(section["rows"][0]["key"], "density");
        assert_eq!(section["rows"][0]["default"], "comfortable");
        assert_eq!(section["rows"][1]["key"], "accent");
        assert_eq!(section["rows"][1]["default"], "#4a9eff");
        assert_eq!(section["rows"][1]["swatches"], "theme");
    }

    /// and writes no settings itself.
    /// REQ: COR-011
    #[test]
    fn density_change_produces_no_submission() {
        let mut client = SettingsClient::default();
        let submissions = client.on_events(vec![settings_changed(&["density"])]);
        assert!(submissions.is_empty(), "the host saves the change; the plugin submits nothing");
    }

    /// REQ: COR-011
    #[test]
    fn empty_event_batch_produces_no_submission() {
        let mut client = SettingsClient::default();
        assert!(client.on_events(Vec::new()).is_empty());
    }

    /// `ui.motion_level`) is not acted on and yields no submission.
    /// REQ: COR-011
    #[test]
    fn unrelated_setting_change_is_ignored() {
        let mut client = SettingsClient::default();
        let submissions = client.on_events(vec![settings_changed(&["ui.motion_level"])]);
        assert!(submissions.is_empty());
    }

    /// owned by this plugin.
    /// REQ: COR-011
    #[test]
    fn onboarding_step_renders_first_run_schema_form() {
        let mut client = SettingsClient::default();
        let tree = client.render(
            ViewTarget::OnboardingStep("first-run".to_string()),
            Area { columns: 80, rows: 24 },
        );
        let serialized = serde_json::to_string(&tree).expect("UiTree serializes");
        assert!(serialized.contains("schema-form"));
        assert!(serialized.contains("first-run"));
    }

    /// onboarding section.
    /// REQ: COR-011
    #[test]
    fn overview_panel_renders_appearance_schema_form() {
        let mut client = SettingsClient::default();
        let tree = client.render(
            ViewTarget::Panel("overview".to_string()),
            Area { columns: 80, rows: 24 },
        );
        let serialized = serde_json::to_string(&tree).expect("UiTree serializes");
        assert!(serialized.contains("schema-form"));
        assert!(serialized.contains("appearance"));
        assert!(!serialized.contains("first-run"));
    }
}
