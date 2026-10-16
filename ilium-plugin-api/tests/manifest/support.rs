//! Shared helpers of the manifest lane: base manifests, validation shortcuts, JSON editing and
//! readers for the manual pages (the pages are the spec; tables are parsed, never copied).
//!
//! Conventions. `errors(..)` validates a source manifest the way `ilium plugin check` does
//! (Check mode, tier T3) and returns the codes of the `error` diagnostics in order.
//! `install_errors(..)` validates a packed manifest the way the installer does (Install mode)
//! and needs a `files` list; `packed(..)` adds one.

#![allow(dead_code)]

use std::path::PathBuf;

use ilium_plugin_api::abi::UNSTABLE_FEATURES;
use ilium_plugin_api::error::Severity;
use ilium_plugin_api::manifest::{
    validate_json, CheckMode, CorePluginSummary, Diagnostic, ValidationContext,
};
use ilium_plugin_api::{Platform, Tier};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------------------------
// Validation context and shortcuts
// ---------------------------------------------------------------------------------------------

/// A validation context for Linux with two core plugins: `ilium.core.titles` (replaceable, no
/// surface items) and `ilium.core.sessions` (not replaceable, reference-manifest.md "Replacement").
pub fn context_with(tier: Tier, mode: CheckMode) -> ValidationContext {
    ValidationContext {
        tier,
        mode,
        platform: Platform::Linux,
        installed: Vec::new(),
        core_plugins: vec![
            CorePluginSummary {
                id: "ilium.core.titles".to_owned(),
                replaceable: true,
                surface: Vec::new(),
            },
            CorePluginSummary {
                id: "ilium.core.sessions".to_owned(),
                replaceable: false,
                surface: Vec::new(),
            },
        ],
        unstable_features: UNSTABLE_FEATURES
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
    }
}

/// All diagnostics of a manifest text under the given tier and mode.
pub fn diagnostics_of_text(text: &str, tier: Tier, mode: CheckMode) -> Vec<Diagnostic> {
    validate_json(text, &context_with(tier, mode))
}

/// All diagnostics of a manifest value under the given tier and mode.
pub fn diagnostics(value: &Value, tier: Tier, mode: CheckMode) -> Vec<Diagnostic> {
    diagnostics_of_text(&value.to_string(), tier, mode)
}

/// The codes of the `error` diagnostics, in order.
pub fn error_codes(found: &[Diagnostic]) -> Vec<String> {
    found
        .iter()
        .filter(|diagnostic| matches!(diagnostic.severity, Severity::Error))
        .map(|diagnostic| diagnostic.code.as_str().to_owned())
        .collect()
}

/// Error codes of a source manifest checked as `ilium plugin check` does, tier T3.
pub fn errors(value: &Value) -> Vec<String> {
    error_codes(&diagnostics(value, Tier::T3, CheckMode::Check))
}

/// Error codes of a manifest text checked as `ilium plugin check` does, tier T3.
pub fn text_errors(text: &str) -> Vec<String> {
    error_codes(&diagnostics_of_text(text, Tier::T3, CheckMode::Check))
}

/// Error codes of a packed manifest checked as the installer does.
pub fn install_errors(value: &Value, tier: Tier) -> Vec<String> {
    error_codes(&diagnostics(value, tier, CheckMode::Install))
}

/// Error diagnostics (not only codes) of a source manifest, tier T3, Check mode.
pub fn error_diagnostics(value: &Value) -> Vec<Diagnostic> {
    diagnostics(value, Tier::T3, CheckMode::Check)
        .into_iter()
        .filter(|diagnostic| matches!(diagnostic.severity, Severity::Error))
        .collect()
}

/// Asserts that `found` holds `code`.
pub fn assert_has(found: &[String], code: &str) {
    assert!(
        found.iter().any(|candidate| candidate == code),
        "expected {code} in {found:?}"
    );
}

/// Asserts that `found` does not hold `code`.
pub fn assert_lacks(found: &[String], code: &str) {
    assert!(
        !found.iter().any(|candidate| candidate == code),
        "did not expect {code} in {found:?}"
    );
}

/// Asserts that a source manifest produces no error at all.
pub fn assert_clean(value: &Value) {
    let found = errors(value);
    assert!(found.is_empty(), "expected no error, got {found:?}");
}

/// Asserts that the codes are exactly `expected`, in order.
pub fn assert_codes(found: &[String], expected: &[&str]) {
    let expected: Vec<String> = expected.iter().map(|code| (*code).to_owned()).collect();
    assert_eq!(found, expected.as_slice());
}

/// The first diagnostic with `code`.
pub fn first_with<'a>(found: &'a [Diagnostic], code: &str) -> &'a Diagnostic {
    found
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == code)
        .unwrap_or_else(|| panic!("no diagnostic with code {code}"))
}

// ---------------------------------------------------------------------------------------------
// Base manifests and JSON editing
// ---------------------------------------------------------------------------------------------

/// A valid source manifest with a client component.
pub fn client() -> Value {
    json!({
        "api": "ilium:plugin@0.1.0",
        "id": "example.demo",
        "name": "Example demo",
        "version": "0.1.0",
        "publisher": "Example Publisher",
        "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0" }
    })
}

/// A valid source manifest with a server component.
pub fn server() -> Value {
    json!({
        "api": "ilium:plugin@0.1.0",
        "id": "example.demo",
        "name": "Example demo",
        "version": "0.1.0",
        "publisher": "Example Publisher",
        "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0" }
    })
}

/// A valid source manifest with a scene component.
pub fn scene() -> Value {
    json!({
        "api": "ilium:plugin@0.1.0",
        "id": "example.demo",
        "name": "Example demo",
        "version": "0.1.0",
        "publisher": "Example Publisher",
        "scene": { "entry": "scene.wasm", "world": "ilium:anim/base@0.1.0" }
    })
}

/// 64 lower-case hex digits for an inventory entry.
pub const SAMPLE_HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// One `files` item.
pub fn file_entry(path: &str, length: u64) -> Value {
    json!({ "path": path, "length": length, "sha256": SAMPLE_HASH })
}

/// Adds a `files` inventory with every declared component plus `extra_paths`, sorted in archive
/// (bytewise) order, so the result is a packed manifest.
pub fn packed_with(mut value: Value, extra_paths: &[&str]) -> Value {
    let mut paths: Vec<String> = extra_paths.iter().map(|path| (*path).to_owned()).collect();
    for side in ["server", "client", "scene"] {
        if value.get(side).is_some() {
            paths.push(format!("{side}.wasm"));
        }
    }
    paths.sort();
    paths.dedup();
    let files: Vec<Value> = paths.iter().map(|path| file_entry(path, 1024)).collect();
    value["files"] = Value::Array(files);
    value
}

/// A packed manifest: `files` lists exactly the declared components.
pub fn packed(value: Value) -> Value {
    packed_with(value, &[])
}

/// Sets the value at a JSON pointer, creating missing objects on the way.
pub fn set(mut root: Value, pointer: &str, new_value: Value) -> Value {
    let tokens: Vec<&str> = pointer.split('/').skip(1).collect();
    let mut cursor = &mut root;
    for (position, token) in tokens.iter().enumerate() {
        let is_last = position + 1 == tokens.len();
        if cursor.is_array() {
            let index: usize = token.parse().expect("array index in pointer");
            let items = cursor.as_array_mut().expect("array");
            if is_last {
                items[index] = new_value;
                return root;
            }
            cursor = &mut items[index];
        } else {
            let map = cursor.as_object_mut().expect("object in pointer");
            if is_last {
                map.insert((*token).to_owned(), new_value);
                return root;
            }
            cursor = map
                .entry((*token).to_owned())
                .or_insert_with(|| json!({}));
        }
    }
    root
}

/// Removes the value at a JSON pointer, when it exists.
pub fn unset(mut root: Value, pointer: &str) -> Value {
    let tokens: Vec<&str> = pointer.split('/').skip(1).collect();
    let mut cursor = &mut root;
    for (position, token) in tokens.iter().enumerate() {
        let is_last = position + 1 == tokens.len();
        if cursor.is_array() {
            let index: usize = token.parse().expect("array index in pointer");
            let items = cursor.as_array_mut().expect("array");
            if is_last {
                items.remove(index);
                return root;
            }
            cursor = &mut items[index];
        } else {
            let map = match cursor.as_object_mut() {
                Some(map) => map,
                None => return root,
            };
            if is_last {
                map.remove(*token);
                return root;
            }
            cursor = match map.get_mut(*token) {
                Some(next) => next,
                None => return root,
            };
        }
    }
    root
}

/// The text of `value` with one raw member (for example `"limits":{"memory_mib":10.0}`) appended
/// to the top-level object, for numbers that serde_json would normalise.
pub fn with_raw_member(value: &Value, member: &str) -> String {
    let text = value.to_string();
    let trimmed = &text[..text.len() - 1];
    format!("{trimmed},{member}}}")
}

/// A client manifest with the given grants on the client side.
pub fn client_with_grants(grants: &[&str]) -> Value {
    set(client(), "/capabilities/client", json!(grants))
}

/// A server manifest with the given grants on the server side.
pub fn server_with_grants(grants: &[&str]) -> Value {
    set(server(), "/capabilities/server", json!(grants))
}

/// A string of `count` copies of `character`.
pub fn repeat(character: char, count: usize) -> String {
    std::iter::repeat(character).take(count).collect()
}

// ---------------------------------------------------------------------------------------------
// The manual
// ---------------------------------------------------------------------------------------------

/// The canonical manual pages: `ILIUM_PLUGIN_MANUAL_DIR`, else `<repo>/docs/plugins-manual/pages`.
pub fn manual_dir() -> PathBuf {
    if let Some(directory) = std::env::var_os("ILIUM_PLUGIN_MANUAL_DIR") {
        return PathBuf::from(directory);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/plugins-manual/pages")
}

/// The repository root (parent of the crate directory).
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Reads one manual page by file name.
pub fn page(name: &str) -> String {
    let path = manual_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The text between a heading line and the next heading of the same or a higher level.
pub fn section(text: &str, heading: &str) -> String {
    let level = heading.chars().take_while(|character| *character == '#').count();
    let mut inside = false;
    let mut collected = Vec::new();
    for line in text.lines() {
        if line.trim_end() == heading {
            inside = true;
            continue;
        }
        if inside {
            let line_level = line.chars().take_while(|character| *character == '#').count();
            if line_level > 0 && line_level <= level && line.starts_with('#') {
                break;
            }
            collected.push(line);
        }
    }
    assert!(inside, "heading {heading:?} not found");
    collected.join("\n")
}

/// Splits one table line into trimmed cells; `None` for a separator or a non-table line.
pub fn table_cells(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return None;
    }
    let inner = trimmed.trim_start_matches('|').trim_end_matches('|');
    let cells: Vec<String> = inner.split('|').map(|cell| cell.trim().to_owned()).collect();
    if cells
        .iter()
        .all(|cell| !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':'))
    {
        return None;
    }
    Some(cells)
}

/// Every table row of a text as cells (header rows included).
pub fn rows_of(text: &str) -> Vec<Vec<String>> {
    text.lines().filter_map(table_cells).collect()
}

/// Removes surrounding backticks of a cell.
pub fn unquote(cell: &str) -> String {
    cell.trim().trim_matches('`').to_owned()
}

/// One row of the error catalogue of reference-manifest-schema.md.
#[derive(Debug, Clone)]
pub struct CatalogueRow {
    /// The code.
    pub code: String,
    /// The stage cell: `1` to `11`, or a letter.
    pub stage: String,
    /// The exit code cell.
    pub exit: String,
    /// The message template.
    pub template: String,
}

/// The rows of the error catalogue (between "Error catalogue" and "Retired and orphan codes").
pub fn catalogue() -> Vec<CatalogueRow> {
    let text = page("reference-manifest-schema.md");
    let start = text.find("## Error catalogue").expect("catalogue heading");
    let end = text.find("## Retired and orphan codes").expect("retired heading");
    rows_of(&text[start..end])
        .into_iter()
        .filter(|cells| cells.len() >= 4 && cells[0].starts_with('`'))
        .map(|cells| CatalogueRow {
            code: unquote(&cells[0]),
            stage: cells[1].clone(),
            exit: cells[2].clone(),
            template: unquote(&cells[3]),
        })
        .collect()
}

/// The retired codes and their replacement cell.
pub fn retired_codes() -> Vec<(String, String)> {
    let text = page("reference-manifest-schema.md");
    let start = text.find("## Retired and orphan codes").expect("retired heading");
    let end = text.find("## Edge cases").expect("edge cases heading");
    rows_of(&text[start..end])
        .into_iter()
        .filter(|cells| cells.len() >= 2 && cells[0].starts_with('`'))
        .map(|cells| (unquote(&cells[0]), cells[1].clone()))
        .collect()
}

/// The JSON Schema document printed on reference-manifest-schema.md.
pub fn page_schema() -> Value {
    let text = page("reference-manifest-schema.md");
    let marker = "```json\n{\n  \"$schema\"";
    let start = text.find(marker).expect("schema block") + "```json\n".len();
    let rest = &text[start..];
    let end = rest.find("\n```").expect("end of schema block");
    serde_json::from_str(&rest[..end]).expect("schema block is JSON")
}

/// Stage number of a catalogue stage cell, `None` for a letter stage.
pub fn stage_number(cell: &str) -> Option<u8> {
    cell.trim().parse::<u8>().ok()
}
