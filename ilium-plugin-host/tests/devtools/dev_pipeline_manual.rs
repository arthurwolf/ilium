//! Readers of the manual and the repository for the dev-pipeline tests (lane dev-pipeline): page
//! text and markdown tables. Documentation rules assert the concrete content of
//! these pages; they never copy it.

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

/// One markdown table: the non-empty line above it, its header cells and its body rows.
pub struct MarkdownTable {
    pub preceding_line: String,
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
        let preceding_line = lines[..index]
            .iter()
            .rev()
            .find(|candidate| !candidate.trim().is_empty())
            .map(|candidate| candidate.trim().to_owned())
            .unwrap_or_default();
        let rows = lines[index + 2..]
            .iter()
            .take_while(|row| row.trim().starts_with('|'))
            .map(|row| split_cells(row))
            .collect();
        tables.push(MarkdownTable {
            preceding_line,
            header: split_cells(trimmed),
            rows,
        });
    }
    tables
}
