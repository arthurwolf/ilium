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

#[cfg(test)]
mod spec_tests {
    use std::path::PathBuf;

    fn load_manifest() -> serde_json::Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
        let text = std::fs::read_to_string(&path).expect("manifest.json is readable");
        serde_json::from_str(&text).expect("manifest.json is valid JSON")
    }

    fn load_manual_page() -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../docs/plugins-manual/pages/core-plugin-keys-and-help.md");
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
    }

    fn string_array(value: &serde_json::Value) -> Vec<String> {
        value
            .as_array()
            .expect("value is a JSON array")
            .iter()
            .map(|item| item.as_str().expect("array item is a string").to_string())
            .collect()
    }

    fn client_grants(manifest: &serde_json::Value) -> Vec<String> {
        string_array(&manifest["capabilities"]["client"])
    }

    /// Rows of the manual's action table, cells in column order with backticks stripped.
    fn parse_action_rows(page: &str) -> Vec<Vec<String>> {
        page.lines()
            .skip_while(|line| !line.starts_with("| Action name"))
            .skip(2)
            .take_while(|line| line.starts_with('|'))
            .map(|line| {
                line.trim_matches('|')
                    .split('|')
                    .map(|cell| cell.trim().trim_matches('`').to_string())
                    .collect()
            })
            .collect()
    }

    /// REQ: COR-050
    /// The manifest activates the plugin on exactly the three triggers the manual lists.
    #[test]
    fn manifest_activates_on_the_three_declared_triggers() {
        let triggers = string_array(&load_manifest()["activation"]);
        assert_eq!(
            triggers,
            vec![
                "command:ilium.core.keys-and-help/show-keys".to_string(),
                "command:ilium.core.keys-and-help/show-plugins".to_string(),
                "settings:keyboard".to_string(),
            ]
        );
    }

    /// REQ: COR-050
    /// Identity and client side of the manifest match the manual's complete manifest.
    #[test]
    fn manifest_identity_and_client_side_match_the_manual() {
        let manifest = load_manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.keys-and-help");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
        assert_eq!(manifest["client"]["exports"], serde_json::json!(["views"]));
    }

    /// REQ: COR-050
    /// The key reference and the plugin manager appear in the palette under the Help category.
    #[test]
    fn palette_commands_are_listed_under_the_help_category() {
        let manifest = load_manifest();
        let commands = manifest["contributes"]["commands"]
            .as_array()
            .expect("commands are declared as an array");
        let help_command_ids: Vec<&str> = commands
            .iter()
            .filter(|command| command["category"] == "Help")
            .filter_map(|command| command["id"].as_str())
            .collect();
        assert_eq!(help_command_ids, vec!["show-keys", "show-plugins"]);
    }

    /// REQ: COR-030
    /// The plugin is the holder of the write grant for the Keyboard tab and for no other tab.
    #[test]
    fn plugin_holds_the_write_grant_for_the_keyboard_tab_only() {
        let writes: Vec<String> = client_grants(&load_manifest())
            .into_iter()
            .filter(|grant| grant.starts_with("settings.write-kernel:"))
            .collect();
        assert_eq!(writes, vec!["settings.write-kernel:keyboard".to_string()]);
    }

    /// REQ: COR-011
    /// The action table lists exactly 35 kernel actions.
    #[test]
    fn keymap_table_lists_exactly_35_actions() {
        let rows = parse_action_rows(&load_manual_page());
        assert_eq!(rows.len(), 35);
    }

    /// REQ: COR-011
    /// Every action has a global command id of the form kernel.<name>.
    #[test]
    fn every_action_has_a_kernel_command_id() {
        let rows = parse_action_rows(&load_manual_page());
        for row in &rows {
            assert_eq!(row.len(), 5, "row {row:?} has five columns");
            assert_eq!(row[1], format!("kernel.{}", row[0]));
        }
    }

    /// REQ: COR-011
    /// Only the rename action has no default key.
    #[test]
    fn only_rename_is_unbound_by_default() {
        let rows = parse_action_rows(&load_manual_page());
        let unbound: Vec<&str> = rows
            .iter()
            .filter(|row| row[2] == "unbound")
            .map(|row| row[0].as_str())
            .collect();
        assert_eq!(unbound, vec!["rename"]);
    }

    /// REQ: COR-011
    /// Invalid input: text without an action table yields no rows.
    #[test]
    fn action_parser_yields_no_rows_without_a_table() {
        assert!(parse_action_rows("").is_empty());
        assert!(parse_action_rows("# no table here\n").is_empty());
    }

    /// REQ: EXT-052
    /// The manifest declares both owned extension points with the values the manual gives.
    #[test]
    fn manifest_declares_both_owned_extension_points() {
        let manifest = load_manifest();
        let points = manifest["contributes"]["extension_points"]
            .as_array()
            .expect("extension points are declared as an array");
        let point_ids: Vec<&str> = points.iter().filter_map(|point| point["id"].as_str()).collect();
        assert_eq!(point_ids, vec!["key-hints", "help-sources"]);
        for point in points {
            assert_eq!(point["max_entries"], 64);
            assert_eq!(point["dynamic"], false);
            assert_eq!(point["risk"], "standard");
        }
    }

    /// REQ: EXT-052
    /// Failure case: the client grants must provide each owned point, otherwise the point is unusable.
    #[test]
    fn manifest_grants_provide_for_each_owned_extension_point() {
        let grants = client_grants(&load_manifest());
        for point_id in ["key-hints", "help-sources"] {
            let expected = format!("extensions.provide:ilium.core.keys-and-help/{point_id}");
            assert!(grants.contains(&expected), "missing grant {expected}");
        }
    }
}
