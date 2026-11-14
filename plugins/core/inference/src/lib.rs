//! Core plugin `ilium.core.inference` (manual page core-plugin-inference.md).
//!
//! One struct per side declared in `manifest.json`, each implementing the lifecycle, the handler and the exports
//! its manifest lists. The pure inference logic moved from `ilium-inference` and `ilium-kilo-gateway` lives in the
//! modules below; the HTTP exchanges run on the host `http` import.

pub mod kilo;
pub mod redact;
pub mod request;
pub mod settings;
pub mod test_provider;

use ilium_plugin_sdk::exports::ProviderCall;
use ilium_plugin_sdk::generated::host::{HostClock, HostInference, HostSettings};
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::host::catalogs::{CatalogPage, CatalogQuery};
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::types::{Activation, CatalogItem, ErrorCode};
use ilium_plugin_sdk::types::{Area, RequestRef, UiTree, ViewTarget};
use ilium_plugin_sdk::{
    host, ilium_plugin, CatalogProvider, Handler, InferenceProvider, Lifecycle, Views,
};

/// The server side of `ilium.core.inference` (core-plugin-inference.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["inference-provider", "catalog-provider"])]
#[derive(Default)]
pub struct InferenceServer {
    settings: serde_json::Value,
    loaded_models: Vec<String>,
    /// The request of the Test provider run in flight; a newer run replaces it, so the result
    /// of the older one is ignored.
    test: Option<RequestRef>,
    /// The monotonic clock reading when the run in flight started.
    test_started: u64,
}

impl Lifecycle for InferenceServer {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        validate_settings(&context.settings_json)?;
        self.settings =
            serde_json::from_str(&context.settings_json).unwrap_or(serde_json::Value::Null);
        // The row reads "Not run yet" until a test finishes; a host without the row refuses
        // the write, which only means there is nothing to show.
        let _ = HostSettings::set_info(
            &mut *host(),
            test_provider::LAST_TEST_ROW.to_owned(),
            test_provider::NOT_RUN_YET.to_owned(),
        );
        Ok(())
    }
}

impl Handler for InferenceServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CommandInvoked(invoked)
                    if invoked
                        .command
                        .ends_with(test_provider::TEST_COMMAND_SUFFIX) =>
                {
                    self.start_test();
                }
                Event::RequestCompleted(done) if self.test.as_ref() == Some(&done.request) => {
                    self.finish_test(done.request);
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

/// The `inference-provider` export of the server (core-plugin-inference.md "Provider", reference-wit.md section 8.8).
impl InferenceProvider for InferenceServer {
    fn complete(&mut self, _call: ProviderCall) -> Result<(), PluginError> {
        Err(PluginError {
            code: ilium_plugin_sdk::types::ErrorCode::Unsupported,
            message: "no model is configured for this provider".to_string(),
            retry_after_ms: None,
        })
    }

    fn models(&mut self) -> Vec<String> {
        self.offered_models()
    }
}

/// The `catalog-provider` export of the server (core-plugin-inference.md "Model catalog", reference-wit.md section 8.10).
impl CatalogProvider for InferenceServer {
    fn query_catalog(&mut self, query: CatalogQuery) -> Result<CatalogPage, PluginError> {
        if query.catalog != "models" {
            return Err(error(ErrorCode::NotFound, "unknown catalogue"));
        }
        if !(1..=256).contains(&query.limit) {
            return Err(error(ErrorCode::InvalidArgument, "limit must be 1 to 256"));
        }
        let filter = query.filter.to_lowercase();
        let matching: Vec<String> = self
            .offered_models()
            .into_iter()
            .filter(|name| name.to_lowercase().contains(&filter))
            .collect();
        let start = match &query.cursor {
            None => 0,
            Some(cursor) => cursor
                .parse::<usize>()
                .map_err(|_| error(ErrorCode::InvalidArgument, "cursor is not valid"))?,
        };
        let end = (start + usize::from(query.limit)).min(matching.len());
        let items = matching
            .get(start.min(end)..end)
            .unwrap_or_default()
            .iter()
            .map(|name| CatalogItem {
                id: name.clone(),
                title: name.clone(),
                ..CatalogItem::default()
            })
            .collect();
        Ok(CatalogPage {
            items,
            next: (end < matching.len()).then(|| end.to_string()),
            total: Some(matching.len() as u32),
        })
    }
}

impl InferenceServer {
    /// Sends the one test request. A run already in flight is replaced.
    fn start_test(&mut self) {
        let mut api = host();
        self.test_started = HostClock::monotonic_millis(&mut *api);
        self.test = HostInference::complete(&mut *api, test_provider::request())
            .ok()
            .map(RequestRef::Call);
    }

    /// Writes latency and model, or `failed` when the host returned no completion.
    fn finish_test(&mut self, request: RequestRef) {
        self.test = None;
        let mut api = host();
        let elapsed = HostClock::monotonic_millis(&mut *api).saturating_sub(self.test_started);
        let completion = match request {
            RequestRef::Call(id) => HostInference::take_completion(&mut *api, id),
            RequestRef::Submission(_) => None,
        };
        let text = test_provider::result_text(
            elapsed,
            completion
                .as_ref()
                .map(|completion| completion.model.as_str()),
        );
        let _ = HostSettings::set_info(&mut *api, test_provider::LAST_TEST_ROW.to_owned(), text);
    }

    /// The saved model of the selected provider, then the loaded list, sorted and without duplicates.
    fn offered_models(&self) -> Vec<String> {
        let mut names = self.loaded_models.clone();
        let selected = self.settings["selected_provider"].as_str().unwrap_or("");
        let table = match selected {
            "open_ai" => "openai",
            "ollama" => "ollama",
            "kilo_gateway" => "kilo_gateway",
            _ => "",
        };
        if let Some(saved) = self.settings[table]["model"].as_str() {
            if !saved.is_empty() {
                names.push(saved.to_string());
            }
        }
        names.sort();
        names.dedup();
        names
    }
}

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_string(),
        retry_after_ms: None,
    }
}

/// The client side of `ilium.core.inference` (core-plugin-inference.md "runs on both sides", world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct InferenceClient;

impl Lifecycle for InferenceClient {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        validate_settings(&context.settings_json)
    }
}

impl Handler for InferenceClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl Views for InferenceClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        empty_tree()
    }
}

/// Activation succeeds when the settings JSON is an object (an empty string means no settings).
fn validate_settings(settings_json: &str) -> Result<(), PluginError> {
    if settings_json.trim().is_empty() {
        return Ok(());
    }
    match serde_json::from_str::<serde_json::Value>(settings_json) {
        Ok(value) if value.is_object() => Ok(()),
        _ => Err(PluginError {
            code: ilium_plugin_sdk::types::ErrorCode::InvalidArgument,
            message: "settings must be one JSON object".to_string(),
            retry_after_ms: None,
        }),
    }
}

/// An empty, valid view tree: one root node that draws nothing.
fn empty_tree() -> UiTree {
    use ilium_plugin_sdk::types::{NodeFlags, UiNode, Widget};
    UiTree {
        version: 1,
        nodes: vec![UiNode {
            key: None,
            widget: Widget::Clear,
            children: Vec::new(),
            flags: NodeFlags::default(),
            tooltip: None,
            tab_index: None,
            disabled_reason: None,
        }],
        root: 0,
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// The crate's shipped manifest, the file the plugin build packages with the component.
    fn shipped_manifest() -> serde_json::Value {
        serde_json::from_str(include_str!("../manifest.json"))
            .expect("manifest.json must be valid JSON")
    }

    /// Grant strings the manifest declares on one side ("server" or "client").
    fn grants_on_side(manifest: &serde_json::Value, side: &str) -> Vec<String> {
        manifest["capabilities"][side]
            .as_array()
            .expect("capabilities must list each side as an array")
            .iter()
            .map(|grant| {
                grant
                    .as_str()
                    .expect("each grant must be a string")
                    .to_string()
            })
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
            assert_eq!(
                unique.len(),
                grants.len(),
                "duplicate grant on the {side} side"
            );
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
        assert!(grants
            .iter()
            .any(|grant| grant == "settings.write-kernel:inference"));
        assert!(grants
            .iter()
            .any(|grant| grant == "settings.write-kernel:llm-instructions"));
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

    /// Every settings row bound to the kernel `inference` table names a key that exists in the
    /// table, in a tab the client side owns, and carries the default of that key, so a fresh
    /// install shows what the kernel uses.
    #[test]
    fn kernel_bound_rows_name_real_keys_with_the_real_defaults() {
        let manifest = shipped_manifest();
        let owned = grants_on_side(&manifest, "client");
        let table = serde_json::to_value(settings::InferenceSettings::default())
            .expect("the settings serialize");
        let mut bound = 0;
        let sections = manifest["contributes"]["settings"][0]["sections"]
            .as_array()
            .expect("sections");
        for row in sections
            .iter()
            .flat_map(|section| section["rows"].as_array().into_iter().flatten())
        {
            let Some(tab) = row["kernel_tab"].as_str() else {
                continue;
            };
            assert!(
                owned.contains(&format!("settings.write-kernel:{tab}")),
                "row {} binds a tab the client does not own",
                row["key"]
            );
            let mut current = &table;
            for part in row["kernel_key"].as_str().expect("kernel_key").split('.') {
                current = &current[part];
            }
            assert!(!current.is_null(), "row {} names no key", row["key"]);
            if !row["default"].is_null() && row["key"] != "selected_provider" {
                assert_eq!(
                    &row["default"], current,
                    "default of row {} differs from the kernel's",
                    row["key"]
                );
            }
            bound += 1;
        }
        assert!(bound >= 14, "the provider rows are bound: {bound}");
    }
}
