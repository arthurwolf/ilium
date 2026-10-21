//! Core plugin `ilium.core.inference` (manual page core-plugin-inference.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::exports::ProviderCall;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::host::catalogs::{CatalogPage, CatalogQuery};
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::types::{Area, UiTree, ViewTarget};
use ilium_plugin_sdk::{
    ilium_plugin, CatalogProvider, Handler, InferenceProvider, Lifecycle, Views,
};

/// The server side of `ilium.core.inference` (core-plugin-inference.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["inference-provider", "catalog-provider"])]
#[derive(Default)]
pub struct InferenceServer;

impl Lifecycle for InferenceServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.inference: server lifecycle.activate")
    }
}

impl Handler for InferenceServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.inference: server handler.on_events")
    }
}

/// The `inference-provider` export of the server (core-plugin-inference.md "Provider", reference-wit.md section 8.8).
impl InferenceProvider for InferenceServer {
    fn complete(&mut self, _call: ProviderCall) -> Result<(), PluginError> {
        unimplemented!("ilium.core.inference: server inference-provider.complete")
    }

    fn models(&mut self) -> Vec<String> {
        unimplemented!("ilium.core.inference: server inference-provider.models")
    }
}

/// The `catalog-provider` export of the server (core-plugin-inference.md "Model catalog", reference-wit.md section 8.10).
impl CatalogProvider for InferenceServer {
    fn query_catalog(&mut self, _query: CatalogQuery) -> Result<CatalogPage, PluginError> {
        unimplemented!("ilium.core.inference: server catalog-provider.query_catalog")
    }
}

/// The client side of `ilium.core.inference` (core-plugin-inference.md "runs on both sides", world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct InferenceClient;

impl Lifecycle for InferenceClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.inference: client lifecycle.activate")
    }
}

impl Handler for InferenceClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.inference: client handler.on_events")
    }
}

impl Views for InferenceClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.inference: client views.render")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// The crate's shipped manifest, the file the plugin build packages with the component.
    fn shipped_manifest() -> serde_json::Value {
        serde_json::from_str(include_str!("../manifest.json")).expect("manifest.json must be valid JSON")
    }

    /// Grant strings the manifest declares on one side ("server" or "client").
    fn grants_on_side(manifest: &serde_json::Value, side: &str) -> Vec<String> {
        manifest["capabilities"][side]
            .as_array()
            .expect("capabilities must list each side as an array")
            .iter()
            .map(|grant| grant.as_str().expect("each grant must be a string").to_string())
            .collect()
    }

    /// REQ: COR-050
    #[test]
    fn complete_manifest_identifies_the_plugin_and_api_version() {
        let manifest = shipped_manifest();
        assert_eq!(manifest["id"], "ilium.core.inference");
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(
            manifest["activation"],
            serde_json::json!(["service:ilium:inference/provider"])
        );
        assert_eq!(manifest["limits_tier"], "first-party");
    }

    /// REQ: COR-050
    #[test]
    fn server_side_runs_in_the_server_base_world() {
        let manifest = shipped_manifest();
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["server"]["entry"], "server.wasm");
    }

    /// REQ: COR-050
    #[test]
    fn server_grants_match_the_complete_manifest() {
        let mut actual = grants_on_side(&shipped_manifest(), "server");
        actual.sort();
        let mut expected: Vec<String> = [
            "inference.provide",
            "http:api.openai.com",
            "http:localhost",
            "extensions.provide:ilium.core.inference/before-request",
            "extensions.provide:ilium.core.inference/after-response",
        ]
        .iter()
        .map(|grant| grant.to_string())
        .collect();
        expected.sort();
        assert_eq!(actual, expected);
    }

    /// REQ: COR-050
    #[test]
    fn client_grants_match_the_complete_manifest() {
        let mut actual = grants_on_side(&shipped_manifest(), "client");
        actual.sort();
        let mut expected: Vec<String> = [
            "ui.panels",
            "settings.write-kernel:inference",
            "settings.write-kernel:llm-instructions",
            "extensions.provide:ilium.core.inference/provider-profiles",
        ]
        .iter()
        .map(|grant| grant.to_string())
        .collect();
        expected.sort();
        assert_eq!(actual, expected);
    }

    /// REQ: COR-030
    #[test]
    fn no_grant_is_declared_twice_on_either_side() {
        let manifest = shipped_manifest();
        for side in ["server", "client"] {
            let grants = grants_on_side(&manifest, side);
            let mut unique = grants.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), grants.len(), "duplicate grant on the {side} side");
        }
    }

    /// REQ: COR-030
    #[test]
    fn owns_exactly_the_inference_and_llm_instructions_tabs() {
        let grants = grants_on_side(&shipped_manifest(), "client");
        let owned_tab_grants: Vec<&String> = grants
            .iter()
            .filter(|grant| grant.starts_with("settings.write-kernel:"))
            .collect();
        assert_eq!(owned_tab_grants.len(), 2);
        assert!(grants.iter().any(|grant| grant == "settings.write-kernel:inference"));
        assert!(grants.iter().any(|grant| grant == "settings.write-kernel:llm-instructions"));
    }

    /// REQ: COR-050
    #[test]
    fn client_side_does_not_hold_the_provider_grant() {
        let grants = grants_on_side(&shipped_manifest(), "client");
        assert!(!grants.iter().any(|grant| grant == "inference.provide"));
    }

    /// REQ: COR-050
    #[test]
    fn server_implements_provider_exports_and_client_implements_views() {
        fn assert_inference_provider<T: InferenceProvider>() {}
        fn assert_catalog_provider<T: CatalogProvider>() {}
        fn assert_views<T: Views>() {}
        assert_inference_provider::<InferenceServer>();
        assert_catalog_provider::<InferenceServer>();
        assert_views::<InferenceClient>();
    }

    /// REQ: COR-050
    #[test]
    fn every_grant_is_a_non_empty_token_without_whitespace() {
        let manifest = shipped_manifest();
        for side in ["server", "client"] {
            for grant in grants_on_side(&manifest, side) {
                assert!(!grant.is_empty(), "empty grant on the {side} side");
                assert!(
                    !grant.chars().any(char::is_whitespace),
                    "grant with whitespace on the {side} side: {grant}"
                );
            }
        }
    }

    /// REQ: COR-050
    #[test]
    fn extension_grants_only_name_this_plugins_own_points() {
        let manifest = shipped_manifest();
        for side in ["server", "client"] {
            for grant in grants_on_side(&manifest, side) {
                if grant.starts_with("extensions.provide:") {
                    assert!(
                        grant.starts_with("extensions.provide:ilium.core.inference/"),
                        "extension grant for another plugin: {grant}"
                    );
                }
            }
        }
    }
}
