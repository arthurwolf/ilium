//! Safe link discovery for rendered terminal text.  It intentionally keeps
//! detection (pure) separate from activation (owned by `App`), so output can
//! never cause an external program to launch by itself.

use std::path::{Path, PathBuf};

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

/// Finds a clickable target under a terminal column.  Trailing punctuation is
/// excluded so prose such as `see https://example.test/docs.` behaves as expected.
pub fn link_at(line: &str, column: usize, cwd: &Path) -> Option<TerminalLink> {
    let mut start = 0;
    for token in line.split_whitespace() {
        let byte_start = line[start..].find(token)? + start;
        let byte_end = byte_start + token.len();
        start = byte_end;
        if !(byte_start..byte_end).contains(&column) {
            continue;
        }
        let value = token.trim_matches(|character: char| {
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
        let path = if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        };
        return Some(TerminalLink::File { path, line, column });
    }
    None
}

fn parse_file_reference(value: &str) -> Option<(PathBuf, Option<u32>, Option<u32>)> {
    let mut parts = value.rsplitn(3, ':');
    let last = parts.next()?;
    let previous = parts.next();
    let before = parts.next();
    let (path, line, column) = match (
        before,
        previous.and_then(|part| part.parse::<u32>().ok()),
        last.parse::<u32>().ok(),
    ) {
        (Some(path), Some(line), Some(column)) => (path, Some(line), Some(column)),
        (_, _, Some(line)) => (previous?, Some(line), None),
        _ => (value, None, None),
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
            link_at("see https://example.test/a.", 10, Path::new("/tmp")),
            Some(TerminalLink::Url(_))
        ));
        assert!(matches!(
            link_at("src/main.rs:42:7", 4, Path::new("/repo")),
            Some(TerminalLink::File {
                line: Some(42),
                column: Some(7),
                ..
            })
        ));
    }
}
