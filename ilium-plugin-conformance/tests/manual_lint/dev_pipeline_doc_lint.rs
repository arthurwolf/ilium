//! Documentation-lint rules of the dev-pipeline lane: owner table DEV-955 and DEV-973, release
//! record DEV-958, retired-code table DEV-957 and troubleshooting links DEV-959
//! (reference-documentation-lifecycle.md, reference-manifest-schema.md). The documentation rules read
//! the pages; the release checks run over page text handed to them, so each can be tried on a clean and a broken input.

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

/// The retired codes of the manifest schema: the first backticked cell of each row of its retired table.
fn retired_codes() -> Vec<String> {
    let page = manual_page(CATALOGUE_PAGE);
    let section = page
        .split("## Retired and orphan codes")
        .nth(1)
        .expect("the manifest schema has the retired and orphan codes section");
    section
        .lines()
        .filter(|line| line.trim_start().starts_with("| `"))
        .filter_map(|line| split_cells(line).into_iter().next())
        .map(|cell| cell.trim_matches('`').to_owned())
        .filter(|code| code.contains('.'))
        .collect()
}

/// Release check of DEV-957: a removed code may appear only in the retired table. `pages` holds
/// `(page file, text)`; the result names every page and code found outside that table.
fn check_removed_codes_only_in_retired_table(
    removed_codes: &[String],
    pages: &[(String, String)],
) -> Result<(), Vec<String>> {
    let mut findings = Vec::new();
    for (page_file, text) in pages {
        let mut in_retired_table = false;
        for line in text.lines() {
            if line.starts_with("## ") {
                in_retired_table = line.starts_with("## Retired and orphan codes");
            }
            // A row of the retired table is the one place a removed code belongs.
            if in_retired_table && line.trim_start().starts_with('|') {
                continue;
            }
            for token in backticked_tokens(line) {
                if removed_codes.contains(&token) {
                    findings.push(format!("{page_file}: removed code `{token}` outside the retired table"));
                }
            }
        }
    }
    if findings.is_empty() {
        Ok(())
    } else {
        Err(findings)
    }
}

/// Release check of DEV-958: the release record has one line per page of the owner table, naming
/// the owner role of its row, the reviewer and an ISO review date. The result lists every page that fails.
fn check_record_lines_cover_owner_table(
    owner_rows: &[Vec<String>],
    record: &str,
) -> Result<(), Vec<String>> {
    let mut findings = Vec::new();
    for row in owner_rows {
        let page = row.first().map(String::as_str).unwrap_or("");
        let owner = row.get(1).map(String::as_str).unwrap_or("");
        let Some(line) = record.lines().find(|line| line.contains(page)) else {
            findings.push(format!("{page}: no record line"));
            continue;
        };
        if owner.is_empty() || !line.contains(owner) {
            findings.push(format!("{page}: record line names no owner role"));
        }
        if !has_iso_date(line) {
            findings.push(format!("{page}: record line has no review date"));
        }
        if !line.to_lowercase().contains("reviewer") {
            findings.push(format!("{page}: record line names no reviewer"));
        }
    }
    if findings.is_empty() {
        Ok(())
    } else {
        Err(findings)
    }
}

/// True when the line holds a `YYYY-MM-DD` date.
fn has_iso_date(line: &str) -> bool {
    line.as_bytes().windows(10).any(|window| {
        window.iter().enumerate().all(|(position, byte)| match position {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        })
    })
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
    let removed = retired_codes();
    assert!(!removed.is_empty(), "the retired table lists codes");
    let schema = manual_page(CATALOGUE_PAGE);
    let clean_pages = vec![(CATALOGUE_PAGE.to_owned(), schema)];
    assert_eq!(
        check_removed_codes_only_in_retired_table(&removed, &clean_pages),
        Ok(()),
        "the retired table itself is the one place a removed code stays"
    );
    let leaked = format!("The error `{}` explains the failure.", removed[0]);
    let dirty_pages = vec![
        ("clean.md".to_owned(), "Nothing removed here.".to_owned()),
        ("leaky.md".to_owned(), leaked),
    ];
    let findings = check_removed_codes_only_in_retired_table(&removed, &dirty_pages)
        .expect_err("a removed code in prose must fail the release check");
    assert_eq!(findings.len(), 1, "only the leaking page is named: {findings:?}");
    assert!(findings[0].starts_with("leaky.md"), "{findings:?}");
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
    let (_, rows) = owner_table();
    assert!(rows.len() >= 2, "the owner table has at least two pages");
    let page_of = |row: &Vec<String>| row[0].clone();
    let full_record: String = rows
        .iter()
        .map(|row| {
            format!("{} | {} | reviewer: Ada | 2026-10-01\n", page_of(row), row[1])
        })
        .collect();
    assert_eq!(check_record_lines_cover_owner_table(&rows, &full_record), Ok(()));

    let missing_page = page_of(&rows[0]);
    let short_record: String = full_record
        .lines()
        .filter(|line| !line.contains(&missing_page))
        .map(|line| format!("{line}\n"))
        .collect();
    let findings = check_record_lines_cover_owner_table(&rows, &short_record)
        .expect_err("a page without a record line must block the release");
    assert_eq!(findings, vec![format!("{missing_page}: no record line")]);

    let undated_record = full_record.replace("2026-10-01", "soon");
    assert!(
        check_record_lines_cover_owner_table(&rows, &undated_record).is_err(),
        "a record line without a date must fail"
    );
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
