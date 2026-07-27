//! Safe link discovery for rendered terminal text.  It intentionally keeps
//! detection (pure) separate from activation (owned by `App`), so output can
//! never cause an external program to launch by itself.

use std::path::{Path, PathBuf};

use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalLink {
    Url(String),
    File {
        path: PathBuf,
        line: Option<u32>,
        column: Option<u32>,
    },
}

impl TerminalLink {
    pub fn display(&self) -> String {
        match self {
            Self::Url(url) => url.clone(),
            Self::File { path, line, column } => format!(
                "{}{}{}",
                path.display(),
                line.map(|value| format!(":{value}")).unwrap_or_default(),
                column.map(|value| format!(":{value}")).unwrap_or_default()
            ),
        }
    }
}

/// Finds a clickable target under a terminal column.  `column` is a
/// terminal *cell* index (as reported by the mouse event), not a byte
/// offset, so it is converted before being compared against byte ranges --
/// agent CLI panes routinely prefix a path with a multi-byte status glyph
/// (`⏺`, `⎿`, `→`) and a raw byte comparison would land inside that glyph's
/// encoding instead of the intended cell. Wrapping punctuation is stripped
/// from both ends of the clicked token (e.g. `see https://example.test/docs.`
/// or a quoted `"https://example.test"`), except a leading `.` is preserved
/// so relative references (`./`, `../`) and dotfiles (`.bashrc`) survive.
pub fn link_at(
    line: &str,
    column: usize,
    cwd: &Path,
    home_dir: Option<&Path>,
) -> Option<TerminalLink> {
    let byte_column = cell_column_to_byte_offset(line, column);
    let mut start = 0;
    for token in line.split_whitespace() {
        let byte_start = line[start..].find(token)? + start;
        let byte_end = byte_start + token.len();
        start = byte_end;
        if !(byte_start..byte_end).contains(&byte_column) {
            continue;
        }
        let value = token
            .trim_start_matches(|character: char| {
                matches!(character, ')' | ']' | '}' | ',' | ';' | '"' | '\'')
            })
            .trim_end_matches(|character: char| {
                matches!(character, ')' | ']' | '}' | ',' | '.' | ';' | '"' | '\'')
            });
        if value.starts_with("https://") || value.starts_with("http://") {
            return Some(TerminalLink::Url(value.to_string()));
        }
        if let Some(path) = value.strip_prefix("file://") {
            return Some(TerminalLink::File {
                path: PathBuf::from(path),
                line: None,
                column: None,
            });
        }
        let (path, line, column) = parse_file_reference(value)?;
        let path = resolve_reference_path(path, cwd, home_dir);
        return Some(TerminalLink::File { path, line, column });
    }
    None
}

/// Converts a terminal cell column into the byte offset of the character
/// occupying that cell, walking `line` cell-by-cell so multi-byte and
/// double-width characters (box-drawing glyphs, agent status icons, emoji)
/// preceding the click point are accounted for rather than counted as a
/// single byte each. Clicking either half of a double-width character
/// resolves to that character's own start, matching the token it belongs to.
fn cell_column_to_byte_offset(line: &str, column: usize) -> usize {
    let mut cell = 0;
    for (byte_offset, character) in line.char_indices() {
        let width = UnicodeWidthChar::width(character).unwrap_or(1);
        if column < cell + width {
            return byte_offset;
        }
        cell += width;
    }
    line.len()
}

/// Resolves a detected file reference against the click context: a leading
/// `~/` expands against the caller's home directory, an already-absolute
/// path is used as-is, and everything else is resolved relative to the
/// pane's working directory. When the home directory could not be
/// determined, a `~/`-prefixed reference is left relative to `cwd` instead
/// of being silently dropped.
fn resolve_reference_path(path: PathBuf, cwd: &Path, home_dir: Option<&Path>) -> PathBuf {
    if let (Ok(home_relative), Some(home_dir)) = (path.strip_prefix("~"), home_dir) {
        return home_dir.join(home_relative);
    }
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

/// Parses a `path`, `path:line`, or `path:line:column` reference out of a
/// single whitespace-delimited token. The trailing numeric segments are
/// peeled off strictly from the right so that a colon embedded earlier in
/// the path (e.g. `a/b:c.txt:42`) keeps its non-numeric segment attached to
/// the path instead of being silently discarded.
fn parse_file_reference(value: &str) -> Option<(PathBuf, Option<u32>, Option<u32>)> {
    let (path, line, column) = match value.rsplit_once(':') {
        Some((rest, last)) => match last.parse::<u32>() {
            Ok(last_number) => match rest.rsplit_once(':') {
                Some((before, middle)) => match middle.parse::<u32>() {
                    Ok(middle_number) => (before, Some(middle_number), Some(last_number)),
                    Err(_) => (rest, Some(last_number), None),
                },
                None => (rest, Some(last_number), None),
            },
            Err(_) => (value, None, None),
        },
        None => (value, None, None),
    };
    (path.starts_with('/')
        || path.starts_with("./")
        || path.starts_with("../")
        || path.starts_with("~/")
        || path.contains('/'))
    .then(|| (PathBuf::from(path), line, column))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_url_and_file_line() {
        assert!(matches!(
            link_at("see https://example.test/a.", 10, Path::new("/tmp"), None),
            Some(TerminalLink::Url(_))
        ));
        assert!(matches!(
            link_at("src/main.rs:42:7", 4, Path::new("/repo"), None),
            Some(TerminalLink::File {
                line: Some(42),
                column: Some(7),
                ..
            })
        ));
    }

    #[test]
    fn keeps_relative_references_relative_instead_of_becoming_absolute() {
        // A leading '.' must survive trimming: "../src/main.rs" collapsing
        // to "/src/main.rs" would silently turn a relative reference into
        // an absolute path to a completely different file.
        let link = link_at(
            "see ../src/main.rs for details",
            4,
            Path::new("/repo"),
            None,
        );
        assert_eq!(
            link,
            Some(TerminalLink::File {
                path: PathBuf::from("/repo/../src/main.rs"),
                line: None,
                column: None,
            })
        );
    }

    #[test]
    fn expands_home_relative_references_against_the_injected_home_dir() {
        let link = link_at(
            "~/.bashrc",
            0,
            Path::new("/repo"),
            Some(Path::new("/home/user")),
        );
        assert_eq!(
            link,
            Some(TerminalLink::File {
                path: PathBuf::from("/home/user/.bashrc"),
                line: None,
                column: None,
            })
        );
    }

    #[test]
    fn keeps_a_non_numeric_middle_segment_attached_to_the_path() {
        // "a/b:c.txt:42": the middle segment isn't a line number, so the
        // whole "a/b:c.txt" must remain the path instead of losing "a/b".
        let link = link_at("a/b:c.txt:42", 0, Path::new("/repo"), None);
        assert_eq!(
            link,
            Some(TerminalLink::File {
                path: PathBuf::from("/repo/a/b:c.txt"),
                line: Some(42),
                column: None,
            })
        );
    }

    #[test]
    fn accounts_for_multi_byte_glyphs_when_mapping_a_cell_column() {
        // "→ " is 4 bytes (a 3-byte arrow plus a space) but only 2 cells,
        // so cell column 2 (the 's' of the path) must not be treated as
        // byte offset 2, which would land inside the arrow's own encoding.
        let link = link_at("→ src/main.rs:42", 2, Path::new("/repo"), None);
        assert_eq!(
            link,
            Some(TerminalLink::File {
                path: PathBuf::from("/repo/src/main.rs"),
                line: Some(42),
                column: None,
            })
        );
    }
}
