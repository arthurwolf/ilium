//! Core plugin `ilium.core.kanban` (manual page core-plugin-kanban.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The client side of `ilium.core.kanban` (core-plugin-kanban.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct KanbanClient;

impl Lifecycle for KanbanClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.kanban: client lifecycle.activate")
    }
}

impl Handler for KanbanClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.kanban: client handler.on_events")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    fn shipped_manifest() -> serde_json::Value {
        serde_json::from_str(include_str!("../manifest.json")).expect("manifest.json is valid JSON")
    }

    fn capability_list(manifest: &serde_json::Value, side: &str) -> Vec<String> {
        manifest["capabilities"][side]
            .as_array()
            .expect("capabilities declares a list for this side")
            .iter()
            .map(|grant| grant.as_str().expect("grant is a string").to_string())
            .collect()
    }

    fn settings_row<'a>(manifest: &'a serde_json::Value, key: &str) -> &'a serde_json::Value {
        manifest["contributes"]["settings"][0]["rows"]
            .as_array()
            .expect("settings tab declares rows")
            .iter()
            .find(|row| row["key"] == key)
            .expect("settings row exists")
    }

    fn number_row_accepts(row: &serde_json::Value, value: i64) -> bool {
        let minimum = row["minimum"].as_i64().expect("number row has a minimum");
        let maximum = row["maximum"].as_i64().expect("number row has a maximum");
        (minimum..=maximum).contains(&value)
    }

    /// REQ: COR-050
    #[test]
    fn identity_matches_complete_manifest() {
        let manifest = shipped_manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.kanban");
        assert_eq!(manifest["name"], "Kanban");
        assert_eq!(manifest["version"], "0.1.0");
        assert_eq!(manifest["publisher"], "Ilium");
        assert_eq!(manifest["client"]["entry"], "client.wasm");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
        assert_eq!(
            manifest["activation"],
            serde_json::json!(["pane-kind:board"])
        );
        let _client = KanbanClient::default();
    }

    /// REQ: COR-050
    #[test]
    fn board_pane_kind_matches_manual_fragment() {
        let manifest = shipped_manifest();
        let pane = &manifest["contributes"]["pane_kinds"][0];
        assert_eq!(pane["id"], "board");
        assert_eq!(pane["title"], "Board");
        assert_eq!(pane["icon"], "layout-kanban");
        assert_eq!(pane["primitive"], "basic");
        assert_eq!(pane["wants_keys"], true);
        assert_eq!(pane["persist"], "metadata");
    }

    /// REQ: COR-050
    #[test]
    fn kanban_settings_tab_rows_match_manual() {
        let manifest = shipped_manifest();
        let tab = &manifest["contributes"]["settings"][0];
        assert_eq!(tab["kind"], "tab");
        assert_eq!(tab["id"], "kanban");
        assert_eq!(tab["title"], "Kanban Board");
        assert_eq!(tab["scope"], "client");
        let preview = settings_row(&manifest, "card_preview");
        assert_eq!(preview["type"], "toggle");
        assert_eq!(preview["default"], true);
        let width = settings_row(&manifest, "column_width");
        assert_eq!(width["type"], "number");
        assert_eq!(width["minimum"], 12);
        assert_eq!(width["maximum"], 60);
        assert_eq!(width["step"], 1);
        assert_eq!(width["unit"], "cells");
        assert_eq!(width["default"], 24);
    }

    /// REQ: COR-050
    #[test]
    fn column_width_rejects_values_outside_declared_range() {
        let manifest = shipped_manifest();
        let width = settings_row(&manifest, "column_width");
        assert!(!number_row_accepts(width, -1));
        assert!(!number_row_accepts(width, 11));
        assert!(!number_row_accepts(width, 61));
        assert!(number_row_accepts(width, 12));
        assert!(number_row_accepts(width, 24));
        assert!(number_row_accepts(width, 60));
    }

    /// REQ: COR-030
    #[test]
    fn kanban_board_tab_has_one_holder_on_the_server_side() {
        let manifest = shipped_manifest();
        let grant = "settings.write-kernel:kanban-board";
        let client_grants = capability_list(&manifest, "client");
        assert!(
            !client_grants.iter().any(|held| held.as_str() == grant),
            "the client part must never hold the kernel tab grant"
        );
        let server_grants = capability_list(&manifest, "server");
        assert_eq!(
            server_grants
                .iter()
                .filter(|held| held.as_str() == grant)
                .count(),
            1
        );
    }

    /// REQ: COR-050
    #[test]
    fn client_grants_match_complete_manifest() {
        let manifest = shipped_manifest();
        let grants = capability_list(&manifest, "client");
        for expected in [
            "panes.provide-kind",
            "fs.read:project",
            "fs.write:project",
            "fs.watch",
            "fs.pick",
            "fs.read:picked",
            "editor.read",
            "services.use:ilium.core.kanban/set-setting",
            "extensions.provide:ilium.core.kanban/card-actions",
            "extensions.provide:ilium.core.kanban/card-moved",
        ] {
            assert!(
                grants.iter().any(|held| held.as_str() == expected),
                "missing client grant {expected}"
            );
        }
        assert_eq!(grants.len(), 10);
    }

    /// REQ: COR-050
    #[test]
    fn extension_points_match_manual_table() {
        let manifest = shipped_manifest();
        let points = manifest["contributes"]["extension_points"]
            .as_array()
            .expect("extension points are declared");
        let card_actions = points
            .iter()
            .find(|point| point["id"] == "card-actions")
            .expect("card-actions is declared");
        assert_eq!(card_actions["kind"], "collect");
        assert_eq!(card_actions["max_entries"], 64);
        assert_eq!(card_actions["dynamic"], false);
        let card_moved = points
            .iter()
            .find(|point| point["id"] == "card-moved")
            .expect("card-moved is declared");
        assert_eq!(card_moved["kind"], "hook");
        assert_eq!(card_moved["mode"], "observe");
        assert_eq!(card_moved["deadline_ms"], 20);
        assert_eq!(card_moved["max_handlers"], 8);
    }
}
