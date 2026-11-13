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
