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
    fn query(&mut self, _q: CatalogQuery) -> Result<CatalogPage, PluginError> {
        unimplemented!("host::imports::catalogs::query")
    }

    fn query_async(&mut self, _q: CatalogQuery) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::catalogs::query_async")
    }

    fn take_page(&mut self, _request: RequestId) -> Option<CatalogPage> {
        unimplemented!("host::imports::catalogs::take_page")
    }

    fn lookup(&mut self, _catalog: String, _id: String) -> Option<CatalogItem> {
        unimplemented!("host::imports::catalogs::lookup")
    }
}

#[cfg(test)]
mod tests {
    use super::CatalogsImport;
    use ilium_plugin_api::generated::host::catalogs::{CatalogPage, CatalogQuery, HostCatalogs};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::RequestId;

    /// A host catalogue from the manual's list (`ilium.palettes`, read-only kernel data).
    const HOST_CATALOGUE: &str = "ilium.palettes";
    /// A host catalogue that needs the `inference` grant.
    const MODELS_CATALOGUE: &str = "ilium.models";
    /// A plugin catalogue, global id `<plugin-id>/<id>`, served by another plugin.
    const PLUGIN_CATALOGUE: &str = "example.themes/themes";
    /// A catalogue id that no host or plugin declares.
    const UNKNOWN_CATALOGUE: &str = "no.such.catalogue/none";

    fn query_of(catalog: &str, filter: &str, limit: u16) -> CatalogQuery {
        CatalogQuery {
            catalog: catalog.to_owned(),
            filter: filter.to_owned(),
            cursor: None,
            limit,
        }
    }

    fn expect_page(result: Result<CatalogPage, PluginError>) -> CatalogPage {
        match result {
            Ok(page) => page,
            Err(error) => panic!("expected a catalogue page, got {:?}: {}", error.code, error.message),
        }
    }

    fn page_error_code(result: Result<CatalogPage, PluginError>) -> ErrorCode {
        match result {
            Ok(_) => panic!("expected an error, got a catalogue page"),
            Err(error) => error.code,
        }
    }

    fn request_error_code(result: Result<RequestId, PluginError>) -> ErrorCode {
        match result {
            Ok(request) => panic!("expected an error, got request {request:?}"),
            Err(error) => error.code,
        }
    }

    fn expect_request(result: Result<RequestId, PluginError>) -> RequestId {
        match result {
            Ok(request) => request,
            Err(error) => panic!("expected a request id, got {:?}: {}", error.code, error.message),
        }
    }

    fn contains_ignoring_case(haystack: &str, needle: &str) -> bool {
        haystack.to_lowercase().contains(&needle.to_lowercase())
    }

    /// REQ: API-001
    #[test]
    fn query_host_catalogue_returns_page_within_limit() {
        let mut import = CatalogsImport;
        let page = expect_page(import.query(query_of(HOST_CATALOGUE, "", 50)));
        assert!(page.items.len() <= 50, "page holds {} items, limit was 50", page.items.len());
        assert!(!page.items.is_empty(), "the palette catalogue should hold the 38 presets");
    }

    /// REQ: API-061
    #[test]
    fn query_accepts_limit_one_boundary() {
        let mut import = CatalogsImport;
        let page = expect_page(import.query(query_of(HOST_CATALOGUE, "", 1)));
        assert!(page.items.len() <= 1, "limit 1 returned {} items", page.items.len());
    }

    /// REQ: API-061
    #[test]
    fn query_accepts_limit_256_boundary() {
        let mut import = CatalogsImport;
        let page = expect_page(import.query(query_of(HOST_CATALOGUE, "", 256)));
        assert!(page.items.len() <= 256, "limit 256 returned {} items", page.items.len());
    }

    /// REQ: API-061
    #[test]
    fn query_limit_zero_is_invalid_argument() {
        let mut import = CatalogsImport;
        let code = page_error_code(import.query(query_of(HOST_CATALOGUE, "", 0)));
        assert_eq!(code, ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn query_limit_257_is_invalid_argument() {
        let mut import = CatalogsImport;
        let code = page_error_code(import.query(query_of(HOST_CATALOGUE, "", 257)));
        assert_eq!(code, ErrorCode::InvalidArgument);
    }

    /// REQ: API-002
    #[test]
    fn query_unknown_catalogue_is_not_found() {
        let mut import = CatalogsImport;
        let code = page_error_code(import.query(query_of(UNKNOWN_CATALOGUE, "", 20)));
        assert_eq!(code, ErrorCode::NotFound);
    }

    /// REQ: API-002
    #[test]
    fn query_empty_catalogue_name_is_not_found() {
        let mut import = CatalogsImport;
        let code = page_error_code(import.query(query_of("", "", 20)));
        assert_eq!(code, ErrorCode::NotFound);
    }

    /// REQ: API-004
    #[test]
    fn query_plugin_catalogue_is_not_permitted_because_sync_query_serves_host_catalogues_only() {
        let mut import = CatalogsImport;
        let code = page_error_code(import.query(query_of(PLUGIN_CATALOGUE, "", 20)));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: API-004
    #[test]
    fn query_models_catalogue_without_inference_grant_is_not_permitted() {
        let mut import = CatalogsImport;
        let code = page_error_code(import.query(query_of(MODELS_CATALOGUE, "", 20)));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: API-001
    #[test]
    fn query_filter_is_case_insensitive_substring_over_title_id_and_keywords() {
        let mut import = CatalogsImport;
        let page = expect_page(import.query(query_of(HOST_CATALOGUE, "SUN", 256)));
        assert!(!page.items.is_empty(), "the filter 'SUN' should match the sunset and sunrise presets");
        for item in &page.items {
            let matches = contains_ignoring_case(&item.title, "sun")
                || contains_ignoring_case(&item.id, "sun")
                || item.keywords.iter().any(|keyword| contains_ignoring_case(keyword, "sun"));
            assert!(matches, "item {} ({}) does not match the filter 'sun'", item.id, item.title);
        }
    }

    /// REQ: API-001
    #[test]
    fn query_title_prefix_matches_rank_before_other_matches() {
        let mut import = CatalogsImport;
        let page = expect_page(import.query(query_of(HOST_CATALOGUE, "su", 256)));
        let mut seen_non_prefix = false;
        for item in &page.items {
            let is_prefix = item.title.to_lowercase().starts_with("su");
            assert!(
                !(is_prefix && seen_non_prefix),
                "title '{}' starts with 'su' but ranks after a non-prefix match",
                item.title
            );
            if !is_prefix {
                seen_non_prefix = true;
            }
        }
    }

    /// REQ: API-002
    #[test]
    fn query_async_unknown_catalogue_is_not_found() {
        let mut import = CatalogsImport;
        let code = request_error_code(import.query_async(query_of(UNKNOWN_CATALOGUE, "", 20)));
        assert_eq!(code, ErrorCode::NotFound);
    }

    /// REQ: API-061
    #[test]
    fn query_async_limit_zero_is_invalid_argument() {
        let mut import = CatalogsImport;
        let code = request_error_code(import.query_async(query_of(PLUGIN_CATALOGUE, "", 0)));
        assert_eq!(code, ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn query_async_limit_257_is_invalid_argument() {
        let mut import = CatalogsImport;
        let code = request_error_code(import.query_async(query_of(PLUGIN_CATALOGUE, "", 257)));
        assert_eq!(code, ErrorCode::InvalidArgument);
    }

    /// REQ: API-004
    #[test]
    fn query_async_other_plugin_catalogue_without_services_use_is_not_permitted() {
        let mut import = CatalogsImport;
        let code = request_error_code(import.query_async(query_of("other.plugin/themes", "", 20)));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: API-005
    #[test]
    fn query_async_plugin_catalogue_returns_request_id() {
        let mut import = CatalogsImport;
        let request = expect_request(import.query_async(query_of(PLUGIN_CATALOGUE, "", 20)));
        assert!(import.take_page(request).is_none(), "a page must not be ready in the same call");
    }

    /// REQ: API-073
    #[test]
    fn query_async_returns_busy_for_the_65th_pending_request() {
        let mut import = CatalogsImport;
        for index in 0..64 {
            let request = import.query_async(query_of(PLUGIN_CATALOGUE, "", 20));
            assert!(request.is_ok(), "request {index} of 64 was refused: {request:?}");
        }
        let code = request_error_code(import.query_async(query_of(PLUGIN_CATALOGUE, "", 20)));
        assert_eq!(code, ErrorCode::Busy);
    }

    /// REQ: ABI-014
    #[test]
    fn take_page_unknown_request_returns_none() {
        let mut import = CatalogsImport;
        assert!(import.take_page(RequestId(u64::MAX)).is_none());
    }

    /// REQ: ABI-014
    #[test]
    fn take_page_before_completion_returns_none() {
        let mut import = CatalogsImport;
        let request = expect_request(import.query_async(query_of(PLUGIN_CATALOGUE, "", 20)));
        assert!(import.take_page(request).is_none());
    }

    /// REQ: API-001
    #[test]
    fn lookup_returns_item_for_an_id_from_a_query() {
        let mut import = CatalogsImport;
        let page = expect_page(import.query(query_of(HOST_CATALOGUE, "", 1)));
        let Some(first) = page.items.first() else {
            panic!("the palette catalogue returned no items to look up");
        };
        let found = import.lookup(HOST_CATALOGUE.to_owned(), first.id.clone());
        match found {
            Some(item) => assert_eq!(item.id, first.id),
            None => panic!("lookup did not find id {} that query returned", first.id),
        }
    }

    /// REQ: API-001
    #[test]
    fn lookup_unknown_id_returns_none() {
        let mut import = CatalogsImport;
        assert!(import.lookup(HOST_CATALOGUE.to_owned(), "no-such-item-id".to_owned()).is_none());
    }

    /// REQ: API-001
    #[test]
    fn lookup_unknown_catalogue_returns_none() {
        let mut import = CatalogsImport;
        assert!(import.lookup(UNKNOWN_CATALOGUE.to_owned(), "anything".to_owned()).is_none());
    }

    /// REQ: API-001
    #[test]
    fn lookup_empty_catalogue_and_empty_id_return_none() {
        let mut import = CatalogsImport;
        assert!(import.lookup(String::new(), String::new()).is_none());
        assert!(import.lookup(HOST_CATALOGUE.to_owned(), String::new()).is_none());
    }
}
