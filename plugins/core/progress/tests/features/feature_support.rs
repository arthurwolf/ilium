//! Helpers only the feature tests use: manual-page and document readers, JSON-text scanners,
//! extension and HTTP helpers. Mounted by `features/main.rs` alone, so the port target never
//! compiles an unused copy of them.

use std::path::PathBuf;

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::Value;

use crate::common::*;

pub fn workspace_root() -> PathBuf {
    crate_dir().join("../../..")
}

/// The text of a manual page by file stem, for example `core-plugin-progress`.
pub fn page(stem: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(format!("{stem}.md"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

pub fn page_text() -> String {
    page("core-plugin-progress")
}

/// A user document of the repository by file name, for example `cli-reference.md`.
pub fn user_doc(name: &str) -> String {
    let path = workspace_root().join("src/docs").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("user doc {path:?}: {error}"))
}

pub fn golden_path(name: &str) -> String {
    format!(
        "{}/tests/features/golden/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

pub fn manifest_value() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json is readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

pub fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string").to_string())
        .collect()
}

/// The first json block after the heading `heading`.
pub fn json_block_after(page: &str, heading: &str) -> Value {
    let start = page
        .find(heading)
        .unwrap_or_else(|| panic!("heading {heading} missing"));
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("json block") + "```json\n".len();
    let close = rest[open..].find("```").expect("block end");
    serde_json::from_str(&rest[open..open + close]).expect("page block is JSON")
}

/// The text of the `<pre class="term">` figures of a page with the markup removed, in order.
pub fn term_blocks(page: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("<pre class=\"term\">") {
        let after = &rest[start + "<pre class=\"term\">".len()..];
        let Some(end) = after.find("</pre>") else {
            break;
        };
        let mut text = String::new();
        let mut in_tag = false;
        for symbol in after[..end].chars() {
            match symbol {
                '<' => in_tag = true,
                '>' => in_tag = false,
                other if !in_tag => text.push(other),
                _ => {}
            }
        }
        let text = text
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">");
        blocks.push(text.trim_matches('\n').to_string());
        rest = &after[end + 6..];
    }
    blocks
}

/// The cells of the table row of `text` whose first cell is `first_cell`.
pub fn table_row(text: &str, first_cell: &str) -> Vec<String> {
    let line = text
        .lines()
        .find(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            cells.len() > 2 && cells[1] == first_cell
        })
        .unwrap_or_else(|| panic!("no table row starting with {first_cell}"));
    let mut cells: Vec<String> = line
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect();
    cells.remove(0);
    cells.pop();
    cells
}

/// The top-level keys of one JSON object text, in the order they are written.
pub fn top_level_keys(line: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut current = String::new();
    let characters: Vec<char> = line.chars().collect();
    let mut index = 0;
    while index < characters.len() {
        let symbol = characters[index];
        if in_string {
            if escaped {
                escaped = false;
                current.push(symbol);
            } else if symbol == '\\' {
                escaped = true;
                current.push(symbol);
            } else if symbol == '"' {
                in_string = false;
                if depth == 1 {
                    let mut look = index + 1;
                    while look < characters.len() && characters[look].is_whitespace() {
                        look += 1;
                    }
                    if look < characters.len() && characters[look] == ':' {
                        keys.push(current.clone());
                    }
                }
            } else {
                current.push(symbol);
            }
        } else {
            match symbol {
                '"' => {
                    in_string = true;
                    current.clear();
                }
                '{' | '[' => depth += 1,
                '}' | ']' => depth -= 1,
                _ => {}
            }
        }
        index += 1;
    }
    keys
}

/// The raw line of the record of `type` (to check key order).
pub fn line_of(stdout: &str, kind: &str) -> String {
    let needle = format!("\"type\":\"{kind}\"");
    stdout
        .lines()
        .find(|line| line.contains(&needle))
        .unwrap_or_else(|| panic!("no {kind} line in {stdout:?}"))
        .to_string()
}

pub const STATUS_SEGMENT: &str = "progress";
/// One reply the plugin gave to an `http-called` call: `(call, status, headers, body)`.
pub type HttpReply = (u64, u16, Vec<(String, String)>, Vec<u8>);

/// Every reply the plugin gave to an `http-called` call: `(call, status, headers, body)`.
pub fn http_replies(host: &FakeHost) -> Vec<HttpReply> {
    host.http_replies()
}

/// Contributes an entry to an extension point the plugin owns, as a package manifest would.
pub fn contribute(host: &mut FakeHost, point: &str, id: &str, priority: i32, entry: Value) {
    host.contribute_extension(point, id, priority, entry);
    host.settle();
}

/// Calls the service `ilium.core.progress/report` of the plugin; returns the reply text.
pub fn call_report(
    host: &mut FakeHost,
    request_json: &str,
) -> Result<String, ilium_plugin_sdk::types::PluginError> {
    host.call_own_service("ilium.core.progress/report", request_json)
}

/// `(topic, payload_json)` of every topic publication the plugin made.
pub fn published_topics(host: &FakeHost) -> Vec<(String, String)> {
    host.published_topics()
}

/// Runs `ilium progress <args>` in `pane` like [`progress`], also returning the standard error text.
pub fn progress_with_stderr(host: &mut FakeHost, pane: u64, args: &[&str]) -> (Run, String) {
    set_pane_env(host, Some(pane));
    let mut full = vec!["progress"];
    full.extend_from_slice(args);
    let invocation = host.cli().invoke(&full, b"");
    let stderr = invocation.stderr();
    let run = Run {
        stdout: invocation.stdout(),
        exit: invocation.exit_code(),
    };
    (run, stderr)
}
