//! The authoritative error catalogue and the constants of `codes.rs`.

use std::collections::BTreeSet;

use crate::manual::{page, sole_backticked, table_cells};

/// Namespaces whose first-cell rows on the errors page are error codes. The same page also
/// lists host functions in the first cell (`fs.tail`), which are not codes.
const ERROR_NAMESPACES: [&str; 14] = [
    "trap",
    "manifest",
    "settings",
    "config",
    "snapshot",
    "extension",
    "component",
    "host",
    "grants",
    "trust",
    "install",
    "migration",
    "plugin",
    "registry",
];

/// `namespace.kebab-name`: a lowercase namespace and one kebab-case name.
pub fn is_dotted_id(token: &str) -> bool {
    let Some((namespace, name)) = token.split_once('.') else {
        return false;
    };
    let namespace_ok = namespace.starts_with(|c: char| c.is_ascii_lowercase())
        && namespace
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let name_ok = name.starts_with(|c: char| c.is_ascii_lowercase())
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    namespace_ok && name_ok
}

pub fn namespace_of(code: &str) -> &str {
    code.split('.').next().unwrap_or(code)
}

/// Error codes in the first cell of the errors page tables, restricted to error namespaces.
fn errors_page_codes(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(table_cells)
        .filter_map(|cells| sole_backticked(cells[0]).map(str::to_owned))
        .filter(|code| is_dotted_id(code) && ERROR_NAMESPACES.contains(&namespace_of(code)))
        .collect()
}

/// Rows of the "Error catalogue" section of reference-manifest-schema.md: a backticked code
/// in the first cell of any table. The stage column is a number or a letter (`I`, `C`, `R`,
/// `T`, `L`) and some tables add a type column, so the section is read by heading. The table
/// of retired codes that follows lists names that no host prints, so it is not part of it.
fn schema_catalogue_codes(text: &str) -> BTreeSet<String> {
    let section = text
        .split_once("\n## Error catalogue")
        .and_then(|(_, rest)| rest.split_once("\n## Retired and orphan codes"))
        .map(|(section, _)| section)
        .unwrap_or_else(|| panic!("reference-manifest-schema.md lost its catalogue headings"));
    section
        .lines()
        .filter_map(table_cells)
        .filter_map(|cells| sole_backticked(cells[0]).map(str::to_owned))
        .filter(|code| is_dotted_id(code))
        .collect()
}

/// The authoritative set: the errors page tables plus the manifest-schema catalogue, which
/// is where `codes.rs` says the install and package codes are defined.
pub fn catalogue(all: &[(String, String)]) -> BTreeSet<String> {
    let mut codes = errors_page_codes(page(all, "reference-errors-and-limits.md"));
    codes.extend(schema_catalogue_codes(page(
        all,
        "reference-manifest-schema.md",
    )));
    assert!(
        codes.len() > 50,
        "catalogue parse found only {}",
        codes.len()
    );
    codes
}

/// Every string value of a `pub const NAME: &str = "value";` line in `codes.rs`.
pub fn constant_values(source: &str) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    for line in source.lines() {
        let Some((head, tail)) = line.split_once(": &str = \"") else {
            continue;
        };
        if !head.trim_start().starts_with("pub const ") {
            continue;
        }
        if let Some((value, _)) = tail.split_once('"') {
            values.insert(value.to_owned());
        }
    }
    assert!(!values.is_empty(), "no constants found in codes.rs");
    values
}
