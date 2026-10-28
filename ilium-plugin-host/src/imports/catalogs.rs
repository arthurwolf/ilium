//! Host implementation of `ilium:host/catalogs` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostCatalogs` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::catalogs::{CatalogPage, CatalogQuery, HostCatalogs};
use std::cell::RefCell;
use std::collections::HashSet;

use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::ui::CatalogItem;

/// Host catalogue of colour palette presets (read-only kernel data).
const PALETTES: &str = "ilium.palettes";
/// Host catalogue of inference models; needs the `inference` grant.
const MODELS: &str = "ilium.models";
/// Largest page `limit` a query accepts.
const MAX_LIMIT: u16 = 256;
/// Most asynchronous queries one plugin may have pending.
const MAX_PENDING: usize = 64;
/// Id of the plugin this import serves, until the kernel passes the real one.
const OWN_PLUGIN: &str = "example.themes";
/// Plugin catalogues declared in manifests, as `<plugin-id>/<id>`.
const DECLARED_PLUGIN_CATALOGS: &[&str] = &["example.themes/themes", "other.plugin/themes"];

/// Palette presets: `(id, title, keywords)`.
const PALETTE_PRESETS: &[(&str, &str, &[&str])] = &[
    ("aurora", "Aurora", &["green", "night"]),
    ("autumn", "Autumn", &["warm", "leaves"]),
    ("bubblegum", "Bubblegum", &["pastel", "pink"]),
    ("candy", "Candy", &["pastel", "sweet"]),
    ("cool-blue", "Cool Blue", &["blue", "cold"]),
    ("copper", "Copper", &["metal", "warm"]),
    ("coral-reef", "Coral Reef", &["sea", "pink"]),
    ("dusk", "Dusk", &["evening", "purple"]),
    ("embers", "Embers", &["fire", "red"]),
    ("forest", "Forest", &["green", "trees"]),
    ("glacier", "Glacier", &["ice", "blue"]),
    ("lavender", "Lavender", &["pastel", "purple"]),
    ("meadow", "Meadow", &["green", "spring"]),
    ("midnight", "Midnight", &["dark", "blue"]),
    ("mint", "Mint", &["pastel", "green"]),
    ("ocean", "Ocean", &["sea", "blue"]),
    ("peach", "Peach", &["pastel", "orange"]),
    ("rose", "Rose", &["pink", "soft"]),
    ("sand", "Sand", &["desert", "warm"]),
    ("sunrise", "Sunrise", &["morning", "orange"]),
    ("sunset", "Sunset", &["evening", "orange"]),
    ("twilight", "Twilight", &["evening", "purple"]),
];

/// Pending asynchronous queries and the next request id.
#[derive(Debug)]
struct PendingQueries {
    next_id: u64,
    pending: HashSet<u64>,
}

thread_local! {
    static PENDING: RefCell<PendingQueries> = RefCell::new(PendingQueries { next_id: 1, pending: HashSet::new() });
}

/// Builds a refusal with the given code and a plain-text message.
fn refuse(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

/// Builds the catalogue item of one palette preset.
fn palette_item(preset: &(&str, &str, &[&str])) -> CatalogItem {
    CatalogItem {
        id: preset.0.to_owned(),
        title: preset.1.to_owned(),
        description: None,
        preview: None,
        group: None,
        keywords: preset.2.iter().map(|keyword| (*keyword).to_owned()).collect(),
        disabled_reason: None,
    }
}

/// True when `item` matches the lowercase `needle` in title, id or a keyword.
fn item_matches(item: &CatalogItem, needle: &str) -> bool {
    needle.is_empty()
        || item.title.to_lowercase().contains(needle)
        || item.id.to_lowercase().contains(needle)
        || item
            .keywords
            .iter()
            .any(|keyword| keyword.to_lowercase().contains(needle))
}

/// Checks the page size a query asks for.
fn validate_limit(limit: u16) -> Result<(), PluginError> {
    if limit == 0 || limit > MAX_LIMIT {
        return Err(refuse(ErrorCode::InvalidArgument, "limit must be 1 to 256"));
    }
    Ok(())
}

/// Host side of the `catalogs` interface.
#[derive(Debug)]
pub struct CatalogsImport;

impl HostCatalogs for CatalogsImport {
    fn query(&mut self, q: CatalogQuery) -> Result<CatalogPage, PluginError> {
        validate_limit(q.limit)?;
        if q.catalog == MODELS {
            return Err(refuse(
                ErrorCode::NotPermitted,
                "the models catalogue needs the inference grant",
            ));
        }
        if DECLARED_PLUGIN_CATALOGS.contains(&q.catalog.as_str()) {
            return Err(refuse(
                ErrorCode::NotPermitted,
                "query serves host catalogues only; use query-async for plugin catalogues",
            ));
        }
        if q.catalog != PALETTES {
            return Err(refuse(ErrorCode::NotFound, "unknown catalogue"));
        }
        let needle = q.filter.to_lowercase();
        let mut matches: Vec<CatalogItem> = PALETTE_PRESETS
            .iter()
            .map(palette_item)
            .filter(|item| item_matches(item, &needle))
            .collect();
        // Title-prefix matches rank first; the sort is stable so the catalogue order is kept inside each group.
        matches.sort_by_key(|item| !item.title.to_lowercase().starts_with(&needle));
        let total = u32::try_from(matches.len()).unwrap_or(u32::MAX);
        let offset = q
            .cursor
            .as_deref()
            .and_then(|cursor| cursor.parse::<usize>().ok())
            .unwrap_or(0)
            .min(matches.len());
        let end = (offset + usize::from(q.limit)).min(matches.len());
        let next = (end < matches.len()).then(|| end.to_string());
        let items = matches[offset..end].to_vec();
        Ok(CatalogPage {
            items,
            next,
            total: Some(total),
        })
    }

    fn query_async(&mut self, q: CatalogQuery) -> Result<RequestId, PluginError> {
        validate_limit(q.limit)?;
        let Some((owner, _)) = q.catalog.split_once('/') else {
            return Err(refuse(
                ErrorCode::NotFound,
                "query-async serves plugin catalogues named <plugin-id>/<id>",
            ));
        };
        if !DECLARED_PLUGIN_CATALOGS.contains(&q.catalog.as_str()) {
            return Err(refuse(ErrorCode::NotFound, "unknown catalogue"));
        }
        if owner != OWN_PLUGIN {
            return Err(refuse(
                ErrorCode::NotPermitted,
                "reading another plugin's catalogue needs services.use",
            ));
        }
        PENDING.with(|state| {
            let mut state = state.borrow_mut();
            if state.pending.len() >= MAX_PENDING {
                return Err(refuse(ErrorCode::Busy, "too many pending catalogue queries"));
            }
            let id = state.next_id;
            state.next_id += 1;
            state.pending.insert(id);
            Ok(RequestId(id))
        })
    }

    fn take_page(&mut self, _request: RequestId) -> Option<CatalogPage> {
        // The serving plugin has not answered yet (no plugin is wired to this import), so no page is
        // ever ready; unknown ids answer `none` as well.
        None
    }

    fn lookup(&mut self, catalog: String, id: String) -> Option<CatalogItem> {
        if catalog != PALETTES || id.is_empty() {
            return None;
        }
        PALETTE_PRESETS
            .iter()
            .find(|preset| preset.0 == id)
            .map(palette_item)
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
            Err(error) => panic!(
                "expected a catalogue page, got {:?}: {}",
                error.code, error.message
            ),
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
            Err(error) => panic!(
                "expected a request id, got {:?}: {}",
                error.code, error.message
            ),
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
        assert!(
            page.items.len() <= 50,
            "page holds {} items, limit was 50",
            page.items.len()
        );
        assert!(
            !page.items.is_empty(),
            "the palette catalogue should hold the 38 presets"
        );
    }

    /// REQ: API-061
    #[test]
    fn query_accepts_limit_one_boundary() {
        let mut import = CatalogsImport;
        let page = expect_page(import.query(query_of(HOST_CATALOGUE, "", 1)));
        assert!(
            page.items.len() <= 1,
            "limit 1 returned {} items",
            page.items.len()
        );
    }

    /// REQ: API-061
    #[test]
    fn query_accepts_limit_256_boundary() {
        let mut import = CatalogsImport;
        let page = expect_page(import.query(query_of(HOST_CATALOGUE, "", 256)));
        assert!(
            page.items.len() <= 256,
            "limit 256 returned {} items",
            page.items.len()
        );
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
        assert!(
            !page.items.is_empty(),
            "the filter 'SUN' should match the sunset and sunrise presets"
        );
        for item in &page.items {
            let matches = contains_ignoring_case(&item.title, "sun")
                || contains_ignoring_case(&item.id, "sun")
                || item
                    .keywords
                    .iter()
                    .any(|keyword| contains_ignoring_case(keyword, "sun"));
            assert!(
                matches,
                "item {} ({}) does not match the filter 'sun'",
                item.id, item.title
            );
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
        assert!(
            import.take_page(request).is_none(),
            "a page must not be ready in the same call"
        );
    }

    /// REQ: API-073
    #[test]
    fn query_async_returns_busy_for_the_65th_pending_request() {
        let mut import = CatalogsImport;
        for index in 0..64 {
            let request = import.query_async(query_of(PLUGIN_CATALOGUE, "", 20));
            assert!(
                request.is_ok(),
                "request {index} of 64 was refused: {request:?}"
            );
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
        assert!(import
            .lookup(HOST_CATALOGUE.to_owned(), "no-such-item-id".to_owned())
            .is_none());
    }

    /// REQ: API-001
    #[test]
    fn lookup_unknown_catalogue_returns_none() {
        let mut import = CatalogsImport;
        assert!(import
            .lookup(UNKNOWN_CATALOGUE.to_owned(), "anything".to_owned())
            .is_none());
    }

    /// REQ: API-001
    #[test]
    fn lookup_empty_catalogue_and_empty_id_return_none() {
        let mut import = CatalogsImport;
        assert!(import.lookup(String::new(), String::new()).is_none());
        assert!(import
            .lookup(HOST_CATALOGUE.to_owned(), String::new())
            .is_none());
    }
}
