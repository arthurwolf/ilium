//! Readers of the manual and the repository for the documentation-lint tests (lane dev-pipeline):
//! page text, markdown tables and heading anchors. Copied from the devtools lane helper of the same
//! name; documentation rules assert the concrete content of these pages and never copy it.

use std::path::Path;

/// A file of the repository, read relative to the workspace root.
pub fn repository_file(relative_path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(relative_path);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} must be readable: {error}", path.display()))
}

/// A page of the plugin manual, by its file name.
pub fn manual_page(page_file: &str) -> String {
    repository_file(&format!("docs/plugins-manual/pages/{page_file}"))
}

/// One markdown table: its header cells and its body rows.
pub struct MarkdownTable {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// The cells of one table row, without the outer pipes.
pub fn split_cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_owned())
        .collect()
}

/// Every table of `text` whose header starts with the cell `first_header`, in page order.
pub fn tables_with_header(text: &str, first_header: &str) -> Vec<MarkdownTable> {
    let lines: Vec<&str> = text.lines().collect();
    let mut tables = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let is_header = trimmed.starts_with('|')
            && split_cells(trimmed).first().map(String::as_str) == Some(first_header);
        let separator = lines.get(index + 1).map(|next| next.trim()).unwrap_or("");
        let has_separator = separator.starts_with("|-") || separator.starts_with("| -");
        if !is_header || !has_separator {
            continue;
        }
        let rows = lines[index + 2..]
            .iter()
            .take_while(|row| row.trim().starts_with('|'))
            .map(|row| split_cells(row))
            .collect();
        tables.push(MarkdownTable {
            header: split_cells(trimmed),
            rows,
        });
    }
    tables
}

/// The GitHub-style anchor of a heading: lower case, spaces to hyphens, other punctuation dropped.
pub fn heading_anchor(heading_text: &str) -> String {
    heading_text
        .trim()
        .chars()
        .filter_map(|character| {
            if character.is_alphanumeric() {
                Some(character.to_ascii_lowercase())
            } else if character == ' ' || character == '-' {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

/// The anchors of every heading of a page, in page order.
pub fn heading_anchors(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.starts_with('#'))
        .map(|line| heading_anchor(line.trim_start_matches('#')))
        .collect()
}

/// The text of every `](<prefix>#<anchor>)` link target of `text`, in page order.
pub fn link_anchors(text: &str, path_prefix: &str) -> Vec<String> {
    let marker = format!("]({path_prefix}#");
    text.split(marker.as_str())
        .skip(1)
        .filter_map(|rest| rest.split(')').next())
        .map(str::to_owned)
        .collect()
}
