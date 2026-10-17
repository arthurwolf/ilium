//! `ilium:host/catalogs` (api-catalogs.md).

use ilium_plugin_api::generated::host::catalogs::{CatalogQuery, HostCatalogs};
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::ui::CatalogItem;
use ilium_plugin_api::ErrorCode;
use std::time::Duration;

use crate::common::*;

const GRANTS: &[&str] = &["services.use"];

fn item(id: &str) -> CatalogItem {
    CatalogItem {
        id: id.to_owned(),
        title: format!("title {id}"),
        description: None,
        preview: None,
        group: None,
        keywords: Vec::new(),
        disabled_reason: None,
    }
}

fn query(catalog: &str, limit: u16) -> CatalogQuery {
    CatalogQuery {
        catalog: catalog.to_owned(),
        filter: String::new(),
        cursor: None,
        limit,
    }
}

fn seeded(count: usize) -> Rig {
    let rig = rig(GRANTS);
    rig.kernel.set_catalog(
        "ilium.style-presets",
        (0..count).map(|n| item(&format!("i{n}"))).collect(),
    );
    rig
}

/// REQ: WIT:ilium:host/catalogs.query
#[test]
fn wit_catalogs_query_returns_the_first_page() {
    let mut rig = seeded(10);
    let page = rig
        .host
        .catalogs()
        .query(query("ilium.style-presets", 4))
        .expect("page");
    assert_eq!(page.items.len(), 4);
    assert!(page.next.is_some());
}

/// REQ: WIT:ilium:host/catalogs.query
#[test]
fn wit_catalogs_query_cursor_walks_to_the_end() {
    let mut rig = seeded(5);
    let first = rig
        .host
        .catalogs()
        .query(query("ilium.style-presets", 3))
        .expect("page 1");
    let mut next = query("ilium.style-presets", 3);
    next.cursor = first.next;
    let second = rig.host.catalogs().query(next).expect("page 2");
    assert_eq!(second.items.len(), 2);
    assert!(second.next.is_none());
}

/// REQ: WIT:ilium:host/catalogs.query
#[test]
fn wit_catalogs_query_limit_boundaries_invalid_argument() {
    let mut rig = seeded(300);
    assert_eq!(
        code_of(rig.host.catalogs().query(query("ilium.style-presets", 0))),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        rig.host
            .catalogs()
            .query(query("ilium.style-presets", 1))
            .expect("1")
            .items
            .len(),
        1
    );
    assert_eq!(
        rig.host
            .catalogs()
            .query(query("ilium.style-presets", 256))
            .expect("256")
            .items
            .len(),
        256
    );
    assert_eq!(
        code_of(rig.host.catalogs().query(query("ilium.style-presets", 257))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/catalogs.query
#[test]
fn wit_catalogs_query_unknown_catalog_not_found() {
    let mut rig = rig(GRANTS);
    assert_eq!(
        code_of(rig.host.catalogs().query(query("no.such", 5))),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/catalogs.query
#[test]
fn wit_catalogs_query_models_without_the_inference_grant_not_permitted() {
    let mut rig = rig(GRANTS);
    assert_eq!(
        code_of(rig.host.catalogs().query(query("ilium.models", 5))),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/catalogs.query-async
#[test]
fn wit_catalogs_query_async_returns_a_request_id_that_completes() {
    let mut rig = seeded(3);
    let id = rig
        .host
        .catalogs()
        .query_async(query("ilium.style-presets", 3))
        .expect("accepted");
    rig.host.pump(8);
    assert!(rig.host.request_completed(id));
}

/// REQ: WIT:ilium:host/catalogs.query-async
#[test]
fn wit_catalogs_query_async_limit_zero_invalid_argument() {
    let mut rig = seeded(3);
    assert_eq!(
        code_of(
            rig.host
                .catalogs()
                .query_async(query("ilium.style-presets", 0))
        ),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/catalogs.query-async
#[test]
fn wit_catalogs_query_async_unknown_catalog_not_found() {
    let mut rig = rig(GRANTS);
    assert_eq!(
        code_of(rig.host.catalogs().query_async(query("no.such", 1))),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/catalogs.query-async
#[test]
fn wit_catalogs_query_async_slow_provider_times_out() {
    let mut rig = seeded(1);
    rig.kernel
        .set_catalog_provider_delay("ilium.style-presets", Duration::from_millis(100));
    let id = rig
        .host
        .catalogs()
        .query_async(query("ilium.style-presets", 1))
        .expect("accepted");
    rig.host.advance_clock(Duration::from_millis(30));
    rig.host.pump(8);
    assert_eq!(rig.host.request_error(id), Some(ErrorCode::TimedOut));
}

/// REQ: WIT:ilium:host/catalogs.take-page
#[test]
fn wit_catalogs_take_page_returns_the_page_once() {
    let mut rig = seeded(3);
    let id = rig
        .host
        .catalogs()
        .query_async(query("ilium.style-presets", 2))
        .expect("accepted");
    rig.host.pump(8);
    let page = rig.host.catalogs().take_page(id).expect("page");
    assert_eq!(page.items.len(), 2);
    assert!(rig.host.catalogs().take_page(id).is_none());
}

/// REQ: WIT:ilium:host/catalogs.take-page
#[test]
fn wit_catalogs_take_page_unknown_request_is_none() {
    let mut rig = seeded(1);
    assert!(rig.host.catalogs().take_page(RequestId(77_777)).is_none());
}

/// REQ: WIT:ilium:host/catalogs.lookup
#[test]
fn wit_catalogs_lookup_finds_an_item_by_id() {
    let mut rig = seeded(3);
    let found = rig
        .host
        .catalogs()
        .lookup("ilium.style-presets".to_owned(), "i1".to_owned())
        .expect("found");
    assert_eq!(found.id, "i1");
}

/// REQ: WIT:ilium:host/catalogs.lookup
#[test]
fn wit_catalogs_lookup_unknown_id_is_none() {
    let mut rig = seeded(3);
    assert!(rig
        .host
        .catalogs()
        .lookup("ilium.style-presets".to_owned(), "zzz".to_owned())
        .is_none());
}

/// REQ: WIT:ilium:host/catalogs.lookup
#[test]
fn wit_catalogs_lookup_unknown_catalog_is_none() {
    let mut rig = seeded(3);
    assert!(rig
        .host
        .catalogs()
        .lookup("no.such".to_owned(), "i1".to_owned())
        .is_none());
}
