//! Sample 3 of the page: the model choice row reads its options from the `models` catalogue the
//! plugin serves through `catalog-provider`.

use ilium_plugin_core_inference::InferenceServer;
use ilium_plugin_sdk::host::{CatalogPage, CatalogQuery, HttpResponse};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::ErrorCode;
use ilium_plugin_sdk::CatalogProvider;
use proptest::prelude::*;
use serde_json::json;

use crate::common::{row_command, server_host};

fn query(filter: &str) -> CatalogQuery {
    CatalogQuery {
        catalog: "models".to_string(),
        filter: filter.to_string(),
        cursor: None,
        limit: 100,
    }
}

fn loaded_host(ids: &[&str]) -> FakeHost {
    let mut host = server_host();
    let data: Vec<_> = ids.iter().map(|id| json!({"id": id})).collect();
    host.http().expect(
        "GET",
        "https://api.openai.com/v1/models",
        HttpResponse {
            status: 200,
            headers: vec![],
            body: json!({"data": data}).to_string().into_bytes(),
        },
    );
    host.deliver_one(row_command("ilium.core.inference/load-models"));
    host.settle();
    host
}

fn ask(host: &mut FakeHost, filter: &str) -> CatalogPage {
    host.instance_mut::<InferenceServer>()
        .query_catalog(query(filter))
        .expect("catalogue answers")
}

/// REQ: F-084
#[test]
fn catalog_084_an_empty_filter_lists_every_model() {
    let mut host = loaded_host(&["gpt-5.6-terra", "gpt-5.6-luna"]);
    let page = ask(&mut host, "");
    let ids: Vec<&str> = page.items.iter().map(|item| item.id.as_str()).collect();
    assert!(ids.contains(&"gpt-5.6-terra") && ids.contains(&"gpt-5.6-luna"));
}

/// REQ: F-084
#[test]
fn catalog_084_each_item_uses_the_model_name_as_id_and_title() {
    let mut host = loaded_host(&["gpt-5.6-terra"]);
    let page = ask(&mut host, "");
    for item in &page.items {
        assert_eq!(item.id, item.title);
    }
}

/// REQ: F-084
#[test]
fn catalog_084_the_filter_keeps_only_names_that_contain_it() {
    let mut host = loaded_host(&["gpt-5.6-terra", "gpt-5.6-luna"]);
    let page = ask(&mut host, "luna");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, "gpt-5.6-luna");
}

/// REQ: F-084
#[test]
fn catalog_084_a_filter_that_matches_nothing_gives_an_empty_page() {
    let mut host = loaded_host(&["gpt-5.6-terra"]);
    let page = ask(&mut host, "no-such-model");
    assert!(page.items.is_empty());
}

/// REQ: F-084
#[test]
fn catalog_084_the_total_is_the_size_of_the_whole_list_even_when_filtered() {
    let mut host = loaded_host(&["a-one", "a-two", "b-one"]);
    let all = ask(&mut host, "").total;
    let filtered = ask(&mut host, "b-").total;
    assert_eq!(all, filtered);
    assert!(all.expect("total") >= 3);
}

/// REQ: F-084
#[test]
fn catalog_084_the_page_has_no_next_cursor() {
    let mut host = loaded_host(&["a", "b"]);
    assert_eq!(ask(&mut host, "").next, None);
}

/// REQ: F-084
#[test]
fn catalog_084_the_saved_model_is_listed_even_before_any_load() {
    let mut host = server_host();
    let page = ask(&mut host, "");
    assert!(page.items.iter().any(|item| item.id == "gpt-5.6-terra"));
}

/// REQ: F-084
#[test]
fn catalog_084_the_catalogue_and_the_models_export_agree() {
    use ilium_plugin_sdk::InferenceProvider;
    let mut host = loaded_host(&["m-1", "m-2"]);
    let mut from_catalogue: Vec<String> = ask(&mut host, "")
        .items
        .into_iter()
        .map(|item| item.id)
        .collect();
    let mut from_export = host.instance_mut::<InferenceServer>().models();
    from_catalogue.sort();
    from_export.sort();
    assert_eq!(from_catalogue, from_export);
}

/// REQ: F-084
#[test]
fn catalog_084_a_query_for_another_catalogue_is_refused_with_not_found() {
    let mut host = server_host();
    let other = CatalogQuery {
        catalog: "palettes".to_string(),
        ..query("")
    };
    let answer = host.instance_mut::<InferenceServer>().query_catalog(other);
    assert_eq!(answer.expect_err("refused").code, ErrorCode::NotFound);
}

/// REQ: F-084
#[test]
fn catalog_084_limit_zero_is_refused_with_invalid_argument() {
    let mut host = server_host();
    let answer = host
        .instance_mut::<InferenceServer>()
        .query_catalog(CatalogQuery {
            limit: 0,
            ..query("")
        });
    assert_eq!(
        answer.expect_err("refused").code,
        ErrorCode::InvalidArgument
    );
}

/// REQ: F-084
#[test]
fn catalog_084_limit_257_is_refused_with_invalid_argument() {
    let mut host = server_host();
    let answer = host
        .instance_mut::<InferenceServer>()
        .query_catalog(CatalogQuery {
            limit: 257,
            ..query("")
        });
    assert_eq!(
        answer.expect_err("refused").code,
        ErrorCode::InvalidArgument
    );
}

/// REQ: F-084
#[test]
fn catalog_084_limits_one_and_256_are_accepted() {
    let mut host = loaded_host(&["m-1", "m-2"]);
    for (limit, expected_items) in [(1u16, 1usize), (256, 2)] {
        let page = host
            .instance_mut::<InferenceServer>()
            .query_catalog(CatalogQuery { limit, ..query("") })
            .unwrap_or_else(|error| panic!("limit {limit}: {error:?}"));
        assert_eq!(page.items.len(), expected_items, "limit {limit}");
        assert_eq!(page.next.is_some(), expected_items < 2, "limit {limit}");
    }
}

/// REQ: F-084
#[test]
fn catalog_084_a_page_never_holds_more_items_than_the_limit_and_offers_a_cursor() {
    let mut host = loaded_host(&["m-1", "m-2", "m-3"]);
    let page = host
        .instance_mut::<InferenceServer>()
        .query_catalog(CatalogQuery {
            limit: 1,
            ..query("")
        })
        .expect("ok");
    assert_eq!(page.items.len(), 1);
    assert!(page.next.is_some());
}

/// REQ: F-084
#[test]
fn catalog_084_following_the_cursor_visits_every_model_once() {
    let mut host = loaded_host(&["m-1", "m-2", "m-3"]);
    let mut seen = Vec::new();
    let mut cursor = None;
    for _ in 0..10 {
        let page = host
            .instance_mut::<InferenceServer>()
            .query_catalog(CatalogQuery {
                limit: 1,
                cursor: cursor.clone(),
                ..query("")
            })
            .expect("ok");
        seen.extend(page.items.into_iter().map(|item| item.id));
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), seen.len());
    for id in ["m-1", "m-2", "m-3"] {
        assert!(seen.iter().any(|name| name == id), "{id}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    // REQ: F-084
    #[test]
    fn catalog_084_filtered_items_are_a_subset_that_all_contain_the_filter(filter in "[a-z0-9.-]{0,4}") {
        let mut host = loaded_host(&["gpt-5.6-terra", "gpt-5.6-luna", "o4-mini", "llama3"]);
        let all: Vec<String> = ask(&mut host, "").items.into_iter().map(|item| item.id).collect();
        let page = ask(&mut host, &filter);
        for item in &page.items {
            prop_assert!(all.contains(&item.id));
            prop_assert!(item.id.contains(&filter));
        }
        let expected = all.iter().filter(|name| name.contains(&filter)).count();
        prop_assert_eq!(page.items.len(), expected);
    }
}
