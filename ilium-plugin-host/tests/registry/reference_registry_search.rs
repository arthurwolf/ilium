//! reference-registry.md "Search": query grammar, scoring, sort orders, pagination, categories
//! (PKG-379..PKG-384).

use crate::common::*;
use proptest::prelude::*;

fn plain(id: &str) -> PluginSpec {
    PluginSpec::new(id)
        .named("Plain")
        .by("Anon")
        .described("")
        .in_categories(&["tools"])
}

fn sandbox_with(plugins: Vec<PluginSpec>) -> Sandbox {
    Sandbox::with_registry(&RegistryBuilder::with_plugins(plugins))
}

fn search(sandbox: &Sandbox, args: &[&str]) -> Vec<String> {
    let mut full = vec!["search"];
    full.extend_from_slice(args);
    let output = sandbox.run(&full);
    assert_ok(&output);
    plugin_ids(&output)
}

// ---- PKG-379 query grammar ----------------------------------------------------------------

/// REQ: PKG-379
#[test]
fn pkg_379_every_term_must_match_as_a_word_prefix() {
    let sandbox = sandbox_with(vec![
        plain("com.a.one").named("Red Panda").described("fast"),
        plain("com.a.two").named("Red Fox").described("slow"),
    ]);
    assert_eq!(search(&sandbox, &["red", "panda"]), vec!["com.a.one"]);
    assert_eq!(search(&sandbox, &["red"]).len(), 2);
    assert!(search(&sandbox, &["red", "zebra"]).is_empty());
}

/// REQ: PKG-379
#[test]
fn pkg_379_a_term_matches_only_at_the_start_of_a_word() {
    let sandbox = sandbox_with(vec![plain("com.a.one").named("Panda")]);
    assert_eq!(search(&sandbox, &["pan"]), vec!["com.a.one"]);
    assert!(search(&sandbox, &["anda"]).is_empty());
}

/// REQ: PKG-379
#[test]
fn pkg_379_case_and_accents_are_folded() {
    let sandbox = Sandbox::standard();
    for query in ["cafe", "CAFE", "Caf\u{e9}", "cAf\u{c9}", "cafe\u{301}"] {
        assert!(
            search(&sandbox, &[query]).contains(&"org.zeta.cafe-timer".to_owned()),
            "{query:?}"
        );
    }
}

/// REQ: PKG-379
#[test]
fn pkg_379_each_listed_field_is_searched() {
    let sandbox = sandbox_with(vec![
        plain("com.a.byid"),
        plain("com.a.n1").named("Byname"),
        plain("com.a.n2").keyworded(&["bykeyword"]),
        plain("com.a.n3").by("Bypublisher"),
        plain("com.a.n4").described("works bydescription well"),
    ]);
    for (query, expected) in [
        ("byid", "com.a.byid"),
        ("byname", "com.a.n1"),
        ("bykeyword", "com.a.n2"),
        ("bypublisher", "com.a.n3"),
        ("bydescription", "com.a.n4"),
    ] {
        assert_eq!(
            search(&sandbox, &[query]),
            vec![expected.to_owned()],
            "{query}"
        );
    }
}

/// REQ: PKG-379
#[test]
fn pkg_379_query_length_is_bounded_at_200_characters() {
    let sandbox = Sandbox::standard();
    let at_limit = "a".repeat(200);
    let over = "a".repeat(201);
    assert_ok(&sandbox.run(&["search", &at_limit]));
    let refused = sandbox.run(&["search", &over]);
    assert_ne!(refused.exit_code, 0, "{refused:?}");
    assert_no_plugin_records(&refused);
}

/// REQ: PKG-379
#[test]
fn pkg_379_empty_query_lists_everything() {
    let sandbox = Sandbox::standard();
    assert_eq!(search(&sandbox, &[]).len(), 3);
}

// ---- PKG-380 scoring ----------------------------------------------------------------------

/// REQ: PKG-380
#[test]
fn pkg_380_field_weights_are_8_6_4_2_1() {
    let sandbox = sandbox_with(vec![
        plain("com.q.desc").described("zed"),
        plain("com.q.pub").by("Zed"),
        plain("com.q.kw").keyworded(&["zed"]),
        plain("com.q.name").named("Zed"),
        plain("com.q.zed"),
    ]);
    assert_eq!(
        search(&sandbox, &["zed"]),
        vec![
            "com.q.zed",
            "com.q.name",
            "com.q.kw",
            "com.q.pub",
            "com.q.desc"
        ]
    );
}

/// REQ: PKG-380
#[test]
fn pkg_380_score_is_the_sum_over_terms() {
    // "zed tool": A = id(8)+name(6) = 14 for term zed? Each term adds its best-field weight.
    let sandbox = sandbox_with(vec![
        plain("com.q.zed").described("tool"), // 8 + 1 = 9
        plain("com.q.other").named("Zed").keyworded(&["tool"]), // 6 + 4 = 10
        plain("com.q.tool").by("Zed"),        // 2 + 8 = 10
    ]);
    let ranked = search(&sandbox, &["zed", "tool"]);
    assert_eq!(
        ranked.last().map(String::as_str),
        Some("com.q.zed"),
        "{ranked:?}"
    );
    assert_eq!(ranked.len(), 3);
}

/// REQ: PKG-380
#[test]
fn pkg_380_ties_break_by_updated_newest_first_then_by_id() {
    let sandbox = sandbox_with(vec![
        plain("com.q.b")
            .named("Zed")
            .updated_at("2029-01-01 00:00:00"),
        plain("com.q.a")
            .named("Zed")
            .updated_at("2029-01-01 00:00:00"),
        plain("com.q.c")
            .named("Zed")
            .updated_at("2029-06-01 00:00:00"),
        plain("com.q.d")
            .named("Zed")
            .updated_at("2028-01-01 00:00:00"),
    ]);
    assert_eq!(
        search(&sandbox, &["zed"]),
        vec!["com.q.c", "com.q.a", "com.q.b", "com.q.d"]
    );
}

/// REQ: PKG-381
#[test]
fn pkg_381_download_counts_and_ratings_do_not_change_the_ranking() {
    let mut loud = plain("com.q.loud").described("zed");
    loud.extra_index =
        serde_json::json!({ "downloads": 9_999_999, "rating": 5.0, "installs": 1_000_000 });
    let quiet = plain("com.q.quiet").named("Zed");
    let sandbox = sandbox_with(vec![loud, quiet]);
    assert_eq!(
        search(&sandbox, &["zed"]),
        vec!["com.q.quiet", "com.q.loud"]
    );
}

// ---- PKG-382 sort orders ------------------------------------------------------------------

fn sortable() -> Sandbox {
    sandbox_with(vec![
        plain("com.s.c")
            .named("charlie")
            .updated_at("2029-01-01 00:00:00")
            .described("zed"),
        plain("com.s.a")
            .named("Alpha")
            .updated_at("2029-03-01 00:00:00")
            .described("zed"),
        plain("com.s.b")
            .named("bravo")
            .updated_at("2029-02-01 00:00:00")
            .described("zed"),
        plain("com.s.d")
            .named("Zed")
            .updated_at("2028-01-01 00:00:00"),
    ])
}

/// REQ: PKG-382
#[test]
fn pkg_382_without_a_query_the_default_order_is_name_a_to_z() {
    assert_eq!(
        search(&sortable(), &[]),
        vec!["com.s.a", "com.s.b", "com.s.c", "com.s.d"]
    );
}

/// REQ: PKG-382
#[test]
fn pkg_382_with_a_query_the_default_order_is_relevance() {
    assert_eq!(search(&sortable(), &["zed"])[0], "com.s.d");
}

/// REQ: PKG-382
#[test]
fn pkg_382_updated_sorts_the_newest_first() {
    assert_eq!(
        search(&sortable(), &["--sort", "updated"]),
        vec!["com.s.a", "com.s.b", "com.s.c", "com.s.d"]
    );
}

/// REQ: PKG-382
#[test]
fn pkg_382_name_sort_can_be_asked_with_a_query() {
    assert_eq!(
        search(&sortable(), &["zed", "--sort", "name"]),
        vec!["com.s.a", "com.s.b", "com.s.c", "com.s.d"]
    );
}

/// REQ: PKG-382
#[test]
fn pkg_382_an_unknown_sort_order_is_a_usage_error() {
    let output = sortable().run(&["search", "--sort", "popularity"]);
    assert_failed(&output, "usage.invalid-value", 2);
}

// ---- PKG-383 pagination -------------------------------------------------------------------

fn many(count: usize) -> Sandbox {
    sandbox_with(
        (0..count)
            .map(|index| plain(&format!("com.p.n{index:03}")).named(&format!("Item {index:03}")))
            .collect(),
    )
}

/// REQ: PKG-383
#[test]
fn pkg_383_a_page_holds_20_results_by_default() {
    let sandbox = many(45);
    assert_eq!(search(&sandbox, &[]).len(), 20);
}

/// REQ: PKG-383
#[test]
fn pkg_383_limit_and_offset_select_a_window() {
    let sandbox = many(45);
    let all = search(&sandbox, &["--limit", "100"]);
    assert_eq!(all.len(), 45);
    assert_eq!(
        search(&sandbox, &["--limit", "5", "--offset", "10"]),
        all[10..15].to_vec()
    );
    assert_eq!(search(&sandbox, &["--offset", "40"]), all[40..].to_vec());
}

/// REQ: PKG-383
#[test]
fn pkg_383_limit_bounds_are_1_and_100() {
    let sandbox = many(120);
    assert_eq!(search(&sandbox, &["--limit", "1"]).len(), 1);
    assert_eq!(search(&sandbox, &["--limit", "100"]).len(), 100);
    for bad in ["0", "101", "-1", "abc"] {
        let output = sandbox.run(&["search", "--limit", bad]);
        assert_failed(&output, "usage.invalid-value", 2);
    }
}

/// REQ: PKG-383
#[test]
fn pkg_383_an_offset_past_the_end_gives_an_empty_page() {
    let sandbox = many(5);
    assert!(search(&sandbox, &["--offset", "5"]).is_empty());
    assert!(search(&sandbox, &["--offset", "500"]).is_empty());
}

// ---- PKG-384 categories -------------------------------------------------------------------

/// REQ: PKG-384
#[test]
fn pkg_384_category_filter_keeps_only_that_category() {
    let sandbox = Sandbox::standard();
    assert_eq!(
        search(&sandbox, &["--category", "editors"]),
        vec!["com.acme.notes"]
    );
    assert_eq!(
        search(&sandbox, &["--category", "git"]),
        vec!["com.acme.git-tools"]
    );
}

/// REQ: PKG-384
#[test]
fn pkg_384_a_plugin_in_several_categories_matches_each() {
    let sandbox = sandbox_with(vec![
        plain("com.m.multi").in_categories(&["git", "editors", "search"])
    ]);
    for category in ["git", "editors", "search"] {
        assert_eq!(
            search(&sandbox, &["--category", category]),
            vec!["com.m.multi"],
            "{category}"
        );
    }
    assert!(search(&sandbox, &["--category", "voice"]).is_empty());
}

/// REQ: PKG-384
#[test]
fn pkg_384_a_category_outside_the_list_is_a_usage_error() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["search", "--category", "games"]);
    assert_failed(&output, "usage.invalid-value", 2);
    assert_no_plugin_records(&output);
}

/// REQ: PKG-384
#[test]
fn pkg_384_every_listed_category_is_accepted() {
    let sandbox = Sandbox::standard();
    for category in [
        "agents",
        "appearance",
        "scenes",
        "editors",
        "git",
        "inference",
        "notifications",
        "search",
        "terminal",
        "tools",
        "voice",
    ] {
        assert_ok(&sandbox.run(&["search", "--category", category]));
    }
}

/// REQ: PKG-384
#[test]
fn pkg_384_publishing_with_a_category_outside_the_list_is_category_unknown() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file("cat.iliumpkg", &signed_package("com.acme.fresh", "1.0.0"));
    let output = sandbox.run(&[
        "publish",
        path.to_str().unwrap(),
        "--registry",
        BASE,
        "--category",
        "games",
    ]);
    assert_code(&output, "registry.category-unknown");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// REQ: PKG-383
    #[test]
    fn pkg_383_every_window_equals_the_slice_of_the_full_list(limit in 1usize..=30, offset in 0usize..=40) {
        let sandbox = many(30);
        let all = search(&sandbox, &["--limit", "100"]);
        let limit_text = limit.to_string();
        let offset_text = offset.to_string();
        let window = search(&sandbox, &["--limit", &limit_text, "--offset", &offset_text]);
        let end = (offset + limit).min(all.len());
        let expected: Vec<String> = if offset >= all.len() { Vec::new() } else { all[offset..end].to_vec() };
        prop_assert_eq!(window, expected);
    }

    /// REQ: PKG-379
    #[test]
    fn pkg_379_search_never_matches_a_term_absent_from_every_field(term in "[qxz]{6,12}") {
        let sandbox = Sandbox::standard();
        let output = sandbox.run(&["search", &term]);
        prop_assert_eq!(output.exit_code, 0);
        prop_assert!(plugin_ids(&output).is_empty());
    }
}

/// REQ: PKG-381
#[test]
fn pkg_381_install_counts_do_not_lift_an_equal_text_match() {
    let mut popular = plain("com.q.b").described("zed");
    popular.extra_index = serde_json::json!({ "installs": 5_000_000 });
    let quiet = plain("com.q.a").described("zed");
    let sandbox = sandbox_with(vec![popular, quiet]);
    assert_eq!(
        search(&sandbox, &["zed"]),
        vec!["com.q.a".to_owned(), "com.q.b".to_owned()]
    );
}
