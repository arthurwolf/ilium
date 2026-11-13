//! Locating and reading the manual pages and `codes.rs`, plus small text scanners.

use std::path::PathBuf;

/// `ILIUM_PLUGIN_MANUAL_DIR`, else `<repo>/docs/plugins-manual/pages`.
pub fn manual_dir() -> PathBuf {
    if let Some(directory) = std::env::var_os("ILIUM_PLUGIN_MANUAL_DIR") {
        return PathBuf::from(directory);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/plugins-manual/pages")
}

/// Every manual page as (file name, text), sorted by name.
pub fn pages() -> Vec<(String, String)> {
    let directory = manual_dir();
    let mut found: Vec<(String, String)> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("list {}: {error}", directory.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .map(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            (name, text)
        })
        .collect();
    found.sort();
    assert!(!found.is_empty(), "no pages in {}", directory.display());
    found
}

/// One page by file name.
pub fn page<'a>(all: &'a [(String, String)], name: &str) -> &'a str {
    all.iter()
        .find(|(file, _)| file == name)
        .map(|(_, text)| text.as_str())
        .unwrap_or_else(|| panic!("manual page {name} is missing"))
}

/// The text of `ilium-plugin-api/src/codes.rs`.
pub fn codes_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ilium-plugin-api/src/codes.rs");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The lines of a page outside fenced code blocks.
pub fn prose_lines(text: &str) -> Vec<&str> {
    let mut inside_fence = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            inside_fence = !inside_fence;
            continue;
        }
        if !inside_fence {
            lines.push(line);
        }
    }
    lines
}

/// The spans between backticks of one line, with the text before and after each span.
pub fn backticked(line: &str) -> Vec<(&str, &str, &str)> {
    let parts: Vec<&str> = line.split('`').collect();
    let mut spans = Vec::new();
    let mut index = 1;
    while index + 1 < parts.len() {
        spans.push((parts[index - 1], parts[index], parts[index + 1]));
        index += 2;
    }
    spans
}

/// The cells of a table row after the leading bar, trimmed; `None` for a non-table line.
pub fn table_cells(line: &str) -> Option<Vec<&str>> {
    if !line.starts_with('|') {
        return None;
    }
    let cells: Vec<&str> = line.split('|').map(str::trim).collect();
    (cells.len() >= 3).then(|| cells[1..cells.len() - 1].to_vec())
}

/// The inside of a cell that is exactly one backticked span.
pub fn sole_backticked(cell: &str) -> Option<&str> {
    let inner = cell.strip_prefix('`')?.strip_suffix('`')?;
    (!inner.is_empty() && !inner.contains('`')).then_some(inner)
}
