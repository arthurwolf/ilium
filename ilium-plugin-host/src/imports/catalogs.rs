//! Host implementation of `ilium:host/catalogs` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostCatalogs` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::catalogs::{CatalogPage, CatalogQuery, HostCatalogs};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::ui::CatalogItem;

/// Host side of the `catalogs` interface.
#[derive(Debug)]
pub struct CatalogsImport;

impl HostCatalogs for CatalogsImport {
    fn query(&mut self, q: CatalogQuery) -> Result<CatalogPage, PluginError> {
        unimplemented!("host::imports::catalogs::query")
    }

    fn query_async(&mut self, q: CatalogQuery) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::catalogs::query_async")
    }

    fn take_page(&mut self, request: RequestId) -> Option<CatalogPage> {
        unimplemented!("host::imports::catalogs::take_page")
    }

    fn lookup(&mut self, catalog: String, id: String) -> Option<CatalogItem> {
        unimplemented!("host::imports::catalogs::lookup")
    }
}
