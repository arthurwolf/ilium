//! ABI-067: the per-field size table of reference-wit.md covers every WIT string, list and blob field.

use crate::support::{manual_page, wit_files};

/// The text of one `## ` section of a manual page, without its heading line.
fn section(page: &str, heading: &str) -> String {
    page.split(heading)
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .unwrap_or_else(|| panic!("missing section {heading}"))
        .to_owned()
}

/// Whether a WIT field type is a string, a list or a blob (ABI-067).
fn is_sized_type(ty: &str) -> bool {
    ty == "string" || ty == "blob" || ty.starts_with("list<")
}

/// REQ: ABI-067
#[test]
fn doc_abi_067_size_table_has_a_row_for_every_string_list_or_blob_field() {
    let page = manual_page("reference-wit.md");
    let table = section(&page, "## Per-field size limits");
    let rows: Vec<&str> = table
        .lines()
        .filter(|line| line.trim_start().starts_with('|'))
        .collect();

    let mut missing = Vec::new();
    for (path, text) in wit_files() {
        for line in text.lines() {
            let Some((name, ty)) = line.trim().split_once(':') else {
                continue;
            };
            let name = name.trim();
            let ty = ty.trim().trim_end_matches(',').trim_end_matches(';').trim();
            let is_field = !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if !is_field || !is_sized_type(ty) {
                continue;
            }
            let has_row = rows.iter().any(|row| row.contains(&format!("`{name}`")));
            if !has_row {
                missing.push(format!("{path}: {name}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "fields without a size row: {missing:#?}"
    );
}

/// REQ: ABI-067
#[test]
fn doc_abi_067_every_size_row_names_a_refusal_code_in_backticks() {
    let page = manual_page("reference-wit.md");
    let table = section(&page, "## Per-field size limits");
    let rows: Vec<&str> = table
        .lines()
        .filter(|line| line.trim_start().starts_with('|'))
        .collect();
    // The first two table lines are the header and its separator.
    let data_rows = rows.get(2..).unwrap_or(&[]);
    assert!(!data_rows.is_empty(), "the size table has no data rows");
    let rows_without_code: Vec<&str> = data_rows
        .iter()
        .copied()
        .filter(|row| row.matches('`').count() < 2)
        .collect();
    assert!(
        rows_without_code.is_empty(),
        "size rows without a refusal code: {rows_without_code:#?}"
    );
}
