//! Core plugin `ilium.core.titles` (manual page core-plugin-titles.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Minimal conforming bodies: no contributions are produced until the feature logic moves here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::types::{Area, UiTree, ViewTarget};
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, Views};

/// The server side of `ilium.core.titles` (core-plugin-titles.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct TitlesServer;

impl Lifecycle for TitlesServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for TitlesServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

/// The client side of `ilium.core.titles` (core-plugin-titles.md "runs on both sides", world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct TitlesClient;

impl Lifecycle for TitlesClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for TitlesClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl Views for TitlesClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        UiTree {
            version: 1,
            nodes: Vec::new(),
            root: 0,
        }
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use serde_json::{json, Value};

    /// The shipped manifest; core-plugin-titles.md "Complete manifest" is the contract it must meet.
    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    /// Kernel tabs this plugin owns, per core-plugin-titles.md "Kernel tabs this plugin owns".
    const OWNED_KERNEL_TABS: [&str; 2] = ["titles", "triggers"];

    fn parse_manifest(text: &str) -> Result<Value, serde_json::Error> {
        serde_json::from_str(text)
    }

    fn shipped_manifest() -> Value {
        parse_manifest(MANIFEST_TEXT).expect("shipped manifest.json parses as JSON")
    }

    fn grant_strings(side_grants: &Value) -> Vec<String> {
        side_grants
            .as_array()
            .map(|grants| {
                grants
                    .iter()
                    .filter_map(|grant| grant.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn server_capabilities(manifest: &Value) -> Vec<String> {
        grant_strings(&manifest["capabilities"]["server"])
    }

    /// Every grant held on either side, so a holder is counted wherever the manifest places it.
    fn all_capabilities(manifest: &Value) -> Vec<String> {
        let mut grants = server_capabilities(manifest);
        grants.extend(grant_strings(&manifest["capabilities"]["client"]));
        grants
    }

    fn kernel_tab_holder_violations(manifest: &Value) -> Vec<String> {
        let held = all_capabilities(manifest);
        OWNED_KERNEL_TABS
            .iter()
            .filter_map(|tab| {
                let grant = format!("settings.write-kernel:{tab}");
                let holders = held.iter().filter(|candidate| **candidate == grant).count();
                (holders != 1).then(|| format!("{grant} held {holders} times"))
            })
            .collect()
    }

    fn extension_point<'a>(manifest: &'a Value, point_id: &str) -> Option<&'a Value> {
        manifest["contributes"]["extension_points"]
            .as_array()?
            .iter()
            .find(|point| point["id"] == point_id)
    }

    #[test]
    fn both_sides_are_implemented_as_default_structs() {
        fn assert_default<T: Default>() {}
        assert_default::<TitlesServer>();
        assert_default::<TitlesClient>();
    }

    #[test]
    fn manifest_names_this_plugin_and_api_version() {
        let manifest = shipped_manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.titles");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn activation_is_startup_only() {
        let manifest = shipped_manifest();
        assert_eq!(manifest["activation"], json!(["startup"]));
    }

    /// REQ: COR-030, COR-033
    #[test]
    fn each_owned_kernel_tab_has_exactly_one_holder() {
        let violations = kernel_tab_holder_violations(&shipped_manifest());
        assert!(violations.is_empty(), "kernel tab holders: {violations:?}");
    }

    #[test]
    fn server_side_declares_the_base_grants() {
        let grants = server_capabilities(&shipped_manifest());
        for required in [
            "inference",
            "agent.read-prompts",
            "tree.read",
            "tree.write",
            "tree.decorate",
            "tree.restructure",
        ] {
            assert!(
                grants.iter().any(|grant| grant == required),
                "missing server grant {required}"
            );
        }
    }

    #[test]
    fn kernel_tab_grants_are_declared_on_the_server_side() {
        let grants = server_capabilities(&shipped_manifest());
        for tab in OWNED_KERNEL_TABS {
            let grant = format!("settings.write-kernel:{tab}");
            assert!(
                grants.contains(&grant),
                "missing server grant {grant}"
            );
        }
    }

    #[test]
    fn extension_points_carry_the_manual_limits() {
        let manifest = shipped_manifest();
        let before = extension_point(&manifest, "before-apply-title")
            .expect("before-apply-title is declared");
        assert_eq!(before["deadline_ms"], 20);
        assert_eq!(before["max_handlers"], 8);
        let styles = extension_point(&manifest, "title-styles").expect("title-styles is declared");
        assert_eq!(styles["max_entries"], 64);
    }

    #[test]
    fn extension_point_grants_are_provided_on_the_server_side() {
        let grants = server_capabilities(&shipped_manifest());
        for point in ["before-apply-title", "title-styles"] {
            let grant = format!("extensions.provide:ilium.core.titles/{point}");
            assert!(grants.contains(&grant), "missing {grant}");
        }
    }

    #[test]
    fn empty_manifest_text_is_rejected() {
        assert!(parse_manifest("").is_err());
    }

    /// REQ: COR-033
    #[test]
    fn duplicate_kernel_tab_holder_is_reported() {
        let mut manifest = shipped_manifest();
        let server_grants = manifest["capabilities"]["server"]
            .as_array_mut()
            .expect("server grants are an array");
        server_grants.push(json!("settings.write-kernel:titles"));
        let violations = kernel_tab_holder_violations(&manifest);
        assert_eq!(
            violations,
            vec!["settings.write-kernel:titles held 2 times".to_owned()]
        );
    }
}
