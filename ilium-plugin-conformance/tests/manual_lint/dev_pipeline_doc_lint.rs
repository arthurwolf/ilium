//! Documentation-lint rules of the dev-pipeline lane: owner table DEV-955 and DEV-973, release
//! record DEV-958, retired-code table DEV-957 and troubleshooting links DEV-959
//! (reference-documentation-lifecycle.md, reference-manifest-schema.md). The documentation rules read
//! the pages; the release-check behaviour is not in the repository yet and stays unimplemented.

use crate::dev_pipeline_manual::{
    heading_anchors, link_anchors, manual_page, split_cells, tables_with_header,
};

const OWNER_ROLES: [&str; 3] = ["Manual maintainer", "Host maintainer", "Support lead"];

const TROUBLESHOOTING_PAGES: [&str; 3] = [
    "guide-troubleshooting.md",
    "user-troubleshooting.md",
    "user-safe-mode-and-troubleshooting.md",
];

const CATALOGUE_PAGE: &str = "reference-manifest-schema.md";
const CATALOGUE_LINK_PREFIX: &str = "/pages/reference-manifest-schema/";

/// The owner table of the documentation lifecycle page: header cells and body rows.
fn owner_table() -> (Vec<String>, Vec<Vec<String>>) {
    let page = manual_page("reference-documentation-lifecycle.md");
    let table = tables_with_header(&page, "Page")
        .into_iter()
        .next()
        .expect("the lifecycle page has the owner table");
    (table.header, table.rows)
}

/// The error codes of the error catalogue: the first backticked cell of each of its table rows.
fn catalogue_codes() -> Vec<String> {
    let page = manual_page(CATALOGUE_PAGE);
    let start = page
        .find("## Error catalogue")
        .expect("the manifest schema has the error catalogue");
    let end = page
        .find("## Retired and orphan codes")
        .expect("the manifest schema has the retired-codes table");
    let section = &page[start..end];
    section
        .lines()
        .filter(|line| line.trim_start().starts_with("| `"))
        .filter_map(|line| split_cells(line).into_iter().next())
        .map(|cell| cell.trim_matches('`').to_owned())
        .filter(|code| code.contains('.'))
        .collect()
}

/// Every backticked token of a page, without the backticks.
fn backticked_tokens(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// REQ: DEV-955
#[test]
fn dev_955_every_owner_table_row_names_a_non_empty_owner_role() {
    let (header, rows) = owner_table();
    let owner_column = header
        .iter()
        .position(|cell| cell.starts_with("Owner"))
        .expect("the owner table has an Owner column");
    assert!(!rows.is_empty(), "the owner table has rows");
    for row in &rows {
        let owner = row.get(owner_column).map(String::as_str).unwrap_or("");
        assert!(
            OWNER_ROLES.iter().any(|role| owner.contains(*role)),
            "a row without an owner role: {row:?}"
        );
    }
}

/// REQ: DEV-955
#[test]
fn dev_955_each_owner_cell_names_exactly_one_role() {
    let (header, rows) = owner_table();
    let owner_column = header
        .iter()
        .position(|cell| cell.starts_with("Owner"))
        .expect("the owner table has an Owner column");
    for row in &rows {
        let owner = row.get(owner_column).map(String::as_str).unwrap_or("");
        let named_roles = OWNER_ROLES.iter().filter(|role| owner.contains(*role)).count();
        assert_eq!(named_roles, 1, "a page has one owner role, found {named_roles} in {owner:?}");
    }
}

/// REQ: DEV-957
#[test]
fn dev_957_a_removed_code_moves_to_the_retired_table_of_the_manifest_schema() {
    let lifecycle = manual_page("reference-documentation-lifecycle.md");
    assert!(
        lifecycle.contains("MUST update every page of the table that names it in the same change"),
        "a release updates every table page that names a changed item"
    );
    assert!(
        lifecycle.contains("instead of being deleted"),
        "a removed error code moves to the retired table instead of being deleted"
    );
    let schema = manual_page(CATALOGUE_PAGE);
    let retired = schema
        .split("## Retired and orphan codes")
        .nth(1)
        .expect("the manifest schema has the retired and orphan codes section");
    assert!(
        retired.contains("| Code |") || retired.contains("| `"),
        "the retired section holds a table of codes"
    );
}

/// REQ: DEV-957
#[test]
fn dev_957_the_release_check_rejects_a_removed_code_left_in_any_other_page() {
    unimplemented!("ilium-plugin-conformance::release::check_removed_codes_only_in_retired_table");
}

/// REQ: DEV-958
#[test]
fn dev_958_the_release_record_names_owner_reviewer_and_date_for_each_page() {
    let lifecycle = manual_page("reference-documentation-lifecycle.md");
    assert!(
        lifecycle.contains("name, for each page of the table, the owner role, the reviewer and the date of the review"),
        "the release record names owner, reviewer and review date of each page"
    );
    assert!(
        lifecycle.contains("A page without a record MUST block the release."),
        "a page without a record blocks the release"
    );
}

/// REQ: DEV-958
#[test]
fn dev_958_a_release_missing_one_page_record_is_blocked_by_the_release_check() {
    unimplemented!("ilium-plugin-conformance::release::check_record_lines_cover_owner_table");
}

/// REQ: DEV-959
#[test]
fn dev_959_each_error_code_named_on_a_troubleshooting_page_links_to_the_catalogue() {
    let codes = catalogue_codes();
    assert!(!codes.is_empty(), "the error catalogue lists codes");
    for page_file in TROUBLESHOOTING_PAGES {
        let text = manual_page(page_file);
        for token in backticked_tokens(&text) {
            if !codes.contains(&token) {
                continue;
            }
            let linked = format!("[`{token}`]({CATALOGUE_LINK_PREFIX}#");
            assert!(
                text.contains(&linked),
                "{page_file} names the error code `{token}` without a link to its catalogue entry"
            );
        }
    }
}

/// REQ: DEV-959
#[test]
fn dev_959_every_catalogue_link_on_a_troubleshooting_page_resolves_to_a_catalogue_anchor() {
    let catalogue_anchors = heading_anchors(&manual_page(CATALOGUE_PAGE));
    assert!(
        catalogue_anchors.iter().any(|anchor| anchor == "error-catalogue"),
        "the catalogue page has the error-catalogue anchor"
    );
    for page_file in TROUBLESHOOTING_PAGES {
        let text = manual_page(page_file);
        for anchor in link_anchors(&text, CATALOGUE_LINK_PREFIX) {
            assert!(
                catalogue_anchors.contains(&anchor),
                "{page_file} links to #{anchor}, which is not an anchor of the catalogue page"
            );
        }
    }
}

/// REQ: DEV-973
#[test]
fn dev_973_every_owner_table_page_is_reviewed_before_each_release_tag() {
    let (header, rows) = owner_table();
    let reviewed_column = header
        .iter()
        .position(|cell| cell.starts_with("Reviewed"))
        .expect("the owner table has a Reviewed column");
    for row in &rows {
        let reviewed = row.get(reviewed_column).map(String::as_str).unwrap_or("");
        assert!(
            reviewed.starts_with("Before each release tag"),
            "a page is reviewed before each release tag, found {reviewed:?}"
        );
    }
}

/// REQ: DEV-973
#[test]
fn dev_973_the_review_corrects_every_term_code_command_limit_or_feature_that_no_longer_matches() {
    let lifecycle = manual_page("reference-documentation-lifecycle.md");
    assert!(
        lifecycle.contains("review that page against the code of the release"),
        "the owner reviews the page against the code of the release"
    );
    assert!(
        lifecycle.contains("MUST correct every term, code, command, limit or feature that no longer matches"),
        "every term, code, command, limit or feature that no longer matches is corrected"
    );
}
