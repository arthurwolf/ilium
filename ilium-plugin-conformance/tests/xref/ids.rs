//! The cross-reference tests.

use std::collections::{BTreeMap, BTreeSet};

use crate::catalogue::{catalogue, constant_values, is_dotted_id, namespace_of};
use crate::manual::{
    backticked, codes_source, page, pages, prose_lines, sole_backticked, table_cells,
};

type Uses = BTreeMap<String, BTreeSet<String>>;

fn render(title: &str, uses: &Uses, missing: &[&String]) -> String {
    let mut text = format!("{title}: {} violation(s)\n", missing.len());
    for id in missing {
        let files: Vec<&str> = uses[*id].iter().take(4).map(String::as_str).collect();
        text.push_str(&format!("  {id}  (in {})\n", files.join(", ")));
    }
    text
}

fn note(uses: &mut Uses, id: &str, file: &str) {
    uses.entry(id.to_owned())
        .or_default()
        .insert(file.to_owned());
}

/// Last segments that make a backticked `a.b` a file name (`manifest.json`), not a code.
const FILE_EXTENSIONS: [&str; 7] = ["json", "toml", "sha256", "md", "wasm", "zip", "txt"];

/// Keys of the installed-state, session-selector and policy files. They look like
/// `ns.name` but are fields, not codes: `session.server`/`session.client` select a plugin
/// side in the test harness, `update.*` are fields of the installed record, `grants.*` are
/// policy-file keys (two of them also appear as retired spellings in a mapping table).
const FIELD_KEYS: [&str; 8] = [
    "session.client",
    "session.server",
    "update.attempts",
    "update.candidate",
    "update.state",
    "grants.deny",
    "grants.forbidden",
    "grants.preapproved",
];

/// Host function names of reference-wit.md: `interface NAME {` followed by indented
/// `fn-name: func(` lines, joined as `NAME.fn-name` (`settings.get-json`).
fn wit_function_names(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut interface = None;
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("interface ") {
            interface = rest.split_whitespace().next().map(str::to_owned);
        } else if let (Some(interface), Some(rest)) = (&interface, line.strip_prefix("  ")) {
            if let Some((name, tail)) = rest.split_once(": ") {
                let tail = tail.strip_prefix("static ").unwrap_or(tail);
                if tail.starts_with("func") && !name.contains(' ') {
                    names.insert(format!("{interface}.{name}"));
                }
            }
        }
    }
    names
}

/// Every backticked name in the first cell of a grants table row plus the "Always granted"
/// bullets, without `:argument` (a row may list several grants).
fn all_grant_tokens(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for cells in text.lines().filter_map(table_cells) {
        for (_, token, _) in backticked(cells[0]) {
            names.insert(token.split(':').next().unwrap_or(token).to_owned());
        }
    }
    let section = text
        .split("## Always granted")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .unwrap_or("");
    for line in section.lines().filter(|line| line.starts_with("- ")) {
        for (_, token, _) in backticked(line) {
            names.insert(token.split(':').next().unwrap_or(token).to_owned());
        }
    }
    names
}

/// The retired spellings (first cell of the "Retired and orphan codes" table) and the
/// invalid grant names shown as bad examples (first cell of a row whose result is
/// `manifest.grant-unknown`). Neither is meant to be in the catalogue.
fn deliberately_wrong_names(all: &[(String, String)]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let schema = page(all, "reference-manifest-schema.md");
    let retired = schema
        .split_once("\n## Retired and orphan codes")
        .map(|(_, rest)| rest.split("\n## ").next().unwrap_or(rest))
        .unwrap_or_else(|| panic!("reference-manifest-schema.md lost its retired codes heading"));
    for cells in retired.lines().filter_map(table_cells) {
        for (_, token, _) in backticked(cells[0]) {
            names.insert(token.to_owned());
        }
    }
    for (_, text) in all {
        for cells in text.lines().filter_map(table_cells) {
            if cells.len() >= 3 && cells[2].starts_with("`manifest.grant-unknown`") {
                for (_, token, _) in backticked(cells[0]) {
                    names.insert(token.to_owned());
                }
            }
        }
    }
    names
}

/// Prose mentions of `ns.name` where `ns` is a namespace of the catalogue or of `codes.rs`
/// must be in the catalogue.
#[test]
fn doc_xref_error_ids_in_prose_are_in_the_catalogue() {
    let all = pages();
    let known = catalogue(&all);
    let mut namespaces: BTreeSet<String> = known
        .iter()
        .map(|code| namespace_of(code).to_owned())
        .collect();
    for value in constant_values(&codes_source()) {
        namespaces.insert(namespace_of(&value).to_owned());
    }
    let mut not_codes = wit_function_names(page(&all, "reference-wit.md"));
    not_codes.extend(all_grant_tokens(page(&all, "reference-capabilities.md")));
    not_codes.extend(deliberately_wrong_names(&all));
    not_codes.extend(FIELD_KEYS.iter().map(|key| (*key).to_owned()));
    let is_file_name = |token: &str| {
        token
            .rsplit('.')
            .next()
            .is_some_and(|last| FILE_EXTENSIONS.contains(&last))
    };
    let mut uses = Uses::new();
    for (file, text) in &all {
        for line in prose_lines(text) {
            for (_, token, _) in backticked(line) {
                if is_dotted_id(token)
                    && namespaces.contains(namespace_of(token))
                    && !is_file_name(token)
                    && !not_codes.contains(token)
                {
                    note(&mut uses, token, file);
                }
            }
        }
    }
    let missing: Vec<&String> = uses.keys().filter(|id| !known.contains(*id)).collect();
    assert!(
        missing.is_empty(),
        "{}",
        render(
            "error ids in prose that no error table defines",
            &uses,
            &missing
        )
    );
}

/// `AAA-123`
fn is_requirement_id(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() == 7
        && bytes[..3].iter().all(u8::is_ascii_uppercase)
        && bytes[3] == b'-'
        && bytes[4..].iter().all(u8::is_ascii_digit)
}

/// The id of a defining line: a table row `| ID |` or `| [ID] |`, or a bullet `- **[ID]**`.
fn defined_id(line: &str) -> Option<&str> {
    if let Some(cells) = table_cells(line) {
        let cell = cells[0].trim_start_matches('[').trim_end_matches(']');
        return is_requirement_id(cell).then_some(cell);
    }
    let rest = line
        .trim_start()
        .strip_prefix("- **[")
        .or_else(|| line.trim_start().strip_prefix("* **["))?;
    let id = rest.get(..7)?;
    (is_requirement_id(id) && rest[7..].starts_with("]**")).then_some(id)
}

/// Every `[AAA-123]` in a line.
fn bracketed_ids(line: &str) -> Vec<&str> {
    let mut found = Vec::new();
    for (start, _) in line.match_indices('[') {
        if let Some(id) = line.get(start + 1..start + 8) {
            if is_requirement_id(id) && line[start + 8..].starts_with(']') {
                found.push(id);
            }
        }
    }
    found
}

/// Every `[AAA-123]` cited in prose must be defined by a table row or bullet somewhere.
#[test]
fn doc_xref_cited_requirement_ids_are_defined() {
    let all = pages();
    let mut defined = BTreeSet::new();
    for (_, text) in &all {
        for line in text.lines() {
            if let Some(id) = defined_id(line) {
                defined.insert(id.to_owned());
            }
        }
    }
    assert!(
        defined.len() > 500,
        "found only {} definitions",
        defined.len()
    );
    let mut uses = Uses::new();
    for (file, text) in &all {
        for line in prose_lines(text) {
            for id in bracketed_ids(line) {
                note(&mut uses, id, file);
            }
        }
    }
    let missing: Vec<&String> = uses.keys().filter(|id| !defined.contains(*id)).collect();
    assert!(
        missing.is_empty(),
        "{}",
        render("requirement ids cited but never defined", &uses, &missing)
    );
}

/// `name` or `name.name...`, each segment `[a-z][a-z0-9-]*`.
fn is_grant_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|segment| {
            segment.starts_with(|c: char| c.is_ascii_lowercase())
                && segment
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

fn name_before_colon(token: &str) -> &str {
    token.split(':').next().unwrap_or(token)
}

/// The grant names of reference-capabilities.md: first-cell backticked rows of its tables
/// and the backticked names of its "Always granted" section.
fn grant_names(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for cells in text.lines().filter_map(table_cells) {
        if let Some(token) = sole_backticked(cells[0]) {
            names.insert(name_before_colon(token).to_owned());
        }
    }
    let section = text
        .split("## Always granted")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .unwrap_or("");
    for line in section.lines().filter(|line| line.starts_with("- ")) {
        for (_, token, _) in backticked(line) {
            names.insert(name_before_colon(token).to_owned());
        }
    }
    names
}

fn ends_with_word_after_space(before: &str, words: &[&str]) -> bool {
    let trimmed = before.trim_end();
    if trimmed.len() == before.len() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    words.iter().any(|word| {
        lower.ends_with(word)
            && !lower[..lower.len() - word.len()].ends_with(|c: char| c.is_alphanumeric())
    })
}

fn starts_with_word_after_space(after: &str, words: &[&str]) -> bool {
    let trimmed = after.trim_start();
    if trimmed.len() == after.len() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    words.iter().any(|word| {
        lower.starts_with(word)
            && !lower[word.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
    })
}

/// A backticked token written next to the word "grant" or "capability" is a grant name, so
/// it must be a row of the grants table (or an always-granted name) when its first segment
/// is a grant family.
#[test]
fn doc_xref_grant_names_in_prose_are_in_the_grants_table() {
    let all = pages();
    let known = grant_names(page(&all, "reference-capabilities.md"));
    assert!(known.len() > 60, "grants table parse found {}", known.len());
    let families: BTreeSet<&str> = known
        .iter()
        .map(|name| name.split('.').next().unwrap_or(name))
        .collect();
    let mut uses = Uses::new();
    for (file, text) in &all {
        if file == "reference-capabilities.md" {
            continue;
        }
        for line in prose_lines(text) {
            for (before, token, after) in backticked(line) {
                let named = ends_with_word_after_space(
                    before,
                    &["grant", "grants", "capability", "capabilities"],
                ) || starts_with_word_after_space(after, &["grant", "capability"]);
                let name = name_before_colon(token);
                let family = name.split('.').next().unwrap_or(name);
                if named && is_grant_name(name) && families.contains(family) {
                    note(&mut uses, name, file);
                }
            }
        }
    }
    let missing: Vec<&String> = uses.keys().filter(|name| !known.contains(*name)).collect();
    assert!(
        missing.is_empty(),
        "{}",
        render(
            "grant names in prose missing from the grants table",
            &uses,
            &missing
        )
    );
}

/// Every value of a `codes.rs` constant is a row of an error table.
#[test]
fn doc_xref_codes_rs_values_are_in_the_error_tables() {
    let known = catalogue(&pages());
    let missing: Vec<String> = constant_values(&codes_source())
        .into_iter()
        .filter(|value| !known.contains(value))
        .collect();
    assert!(
        missing.is_empty(),
        "codes.rs values with no error table row: {} violation(s)\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
}

/// Every error table row has a `codes.rs` constant.
#[test]
fn doc_xref_error_table_codes_have_codes_rs_constants() {
    let constants = constant_values(&codes_source());
    let missing: Vec<String> = catalogue(&pages())
        .into_iter()
        .filter(|code| !constants.contains(code))
        .collect();
    assert!(
        missing.is_empty(),
        "error table rows with no codes.rs constant: {} violation(s)\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
}
