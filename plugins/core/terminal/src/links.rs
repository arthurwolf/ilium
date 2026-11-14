//! Link discovery for rendered terminal text (moved from ilium-client/src/terminal_links.rs).
//! Detection is pure: it never opens anything, so output can never cause an external program to
//! launch by itself. Activation is the kernel's (`link-activated`) and the editor's (`open-link`).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

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
                matches!(character, '(' | '[' | '{' | ',' | ';' | '"' | '\'' | '`')
            })
            .trim_end_matches(|character: char| {
                matches!(
                    character,
                    ')' | ']' | '}' | ',' | '.' | ';' | '"' | '\'' | '`'
                )
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
pub fn cell_column_to_byte_offset(line: &str, column: usize) -> usize {
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
pub fn parse_file_reference(value: &str) -> Option<(PathBuf, Option<u32>, Option<u32>)> {
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
    // A bare token can still name a project-local file. Resolution and the
    // caller's final existence/confinement check decide whether it is
    // actionable, so this admits `STYLE.md` without opening arbitrary paths.
    (!path.is_empty()).then(|| (PathBuf::from(path), line, column))
}

/// One match of a manifest link detector inside a line of rendered text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkMatch {
    /// The detector id of the manifest (`path-line` or `url`).
    pub detector_id: &'static str,
    /// Byte offset of the first matched character.
    pub start: usize,
    /// Byte offset one past the last matched character.
    pub end: usize,
    /// The matched text.
    pub text: String,
}

/// Regex of the `path-line` detector, identical to `contributes.link_detectors[1].regex`.
pub const PATH_LINE_PATTERN: &str = "[./A-Za-z0-9_-]+\\.[a-z]+:[0-9]+";
/// Regex of the `url` detector, identical to `contributes.link_detectors[0].regex`: an
/// `http(s)` URL that does not end in wrapping or sentence punctuation.
pub const URL_PATTERN: &str = "https?://[^\\s]*[^\\s)\\]},.;\"'`]";
/// Regex of the `path` detector, identical to `contributes.link_detectors[2].regex`: any
/// whitespace-delimited token without wrapping punctuation at its ends, so `STYLE.md`, `~/.bashrc`,
/// `../src/main.rs`, `file:///tmp/a.log` and `src/main.rs:42:7` are one link each. A leading `.`
/// survives; a trailing one does not.
pub const PATH_PATTERN: &str = "[^\\s()\\[\\]{},;\"'`]*[^\\s()\\[\\]{},;\"'`.]";

fn detectors() -> &'static [(&'static str, Regex)] {
    static DETECTORS: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    DETECTORS.get_or_init(|| {
        [
            ("url", URL_PATTERN),
            ("path-line", PATH_LINE_PATTERN),
            ("path", PATH_PATTERN),
        ]
        .into_iter()
        .filter_map(|(id, pattern)| Regex::new(pattern).ok().map(|regex| (id, regex)))
        .collect()
    })
}

/// Every match of the `url` and `path-line` detectors in `line`, ordered by position. A
/// `path-line` match that lies inside a `url` match is dropped, so `https://host/a.rs:3` is one
/// link, not two. The catch-all `path` detector is not listed here: it matches every word, so
/// it only answers a click through [`detector_at`].
pub fn detect(line: &str) -> Vec<LinkMatch> {
    let mut found: Vec<LinkMatch> = Vec::new();
    for (detector_id, regex) in detectors().iter().filter(|(id, _)| *id != "path") {
        for matched in regex.find_iter(line) {
            found.push(LinkMatch {
                detector_id,
                start: matched.start(),
                end: matched.end(),
                text: matched.as_str().to_string(),
            });
        }
    }
    let urls: Vec<(usize, usize)> = found
        .iter()
        .filter(|link| link.detector_id == "url")
        .map(|link| (link.start, link.end))
        .collect();
    found.retain(|link| {
        link.detector_id == "url"
            || !urls
                .iter()
                .any(|(start, end)| link.start >= *start && link.end <= *end)
    });
    found.sort_by_key(|link| (link.start, link.end));
    found
}

/// The link under the byte offset `offset` of `line`, trying the detectors in manifest order
/// (`url`, `path-line`, then the catch-all `path`), like the kernel's click path does.
pub fn detector_at(line: &str, offset: usize) -> Option<LinkMatch> {
    detectors().iter().find_map(|(detector_id, regex)| {
        regex
            .find_iter(line)
            .find(|matched| (matched.start()..matched.end()).contains(&offset))
            .map(|matched| LinkMatch {
                detector_id,
                start: matched.start(),
                end: matched.end(),
                text: matched.as_str().to_string(),
            })
    })
}

/// Where an activated file link opens: a scope and a path the kernel confines to it, and the
/// zero-based editor line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTarget {
    /// `project` (relative to the project, or an absolute path inside it or inside the home
    /// directory) or `home` (relative to the home directory).
    pub scope: &'static str,
    /// The path inside the scope.
    pub path: String,
    /// The zero-based line to reveal.
    pub line: Option<u32>,
}

/// Collapses `.` and `..` segments without touching the filesystem.
fn normalize_segments(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Turns a file reference (`path`, one-based `line`) into what the editor opens. `~` is the
/// home scope, an absolute path stays absolute, and a relative path joins the pane's working
/// directory `cwd` when it is known (otherwise it stays relative to the project). `None` when
/// the path is empty or names the home directory itself.
pub fn file_target(path: &str, line: Option<u32>, cwd: Option<&str>) -> Option<FileTarget> {
    if path.is_empty() {
        return None;
    }
    let line = line.map(|value| value.saturating_sub(1));
    if path == "~" {
        return None;
    }
    if let Some(rest) = path.strip_prefix("~/") {
        let relative = normalize_segments(Path::new(rest));
        let text = relative.to_str()?;
        if text.is_empty() {
            return None;
        }
        return Some(FileTarget {
            scope: "home",
            path: text.to_string(),
            line,
        });
    }
    let joined = match cwd {
        Some(cwd) if !Path::new(path).is_absolute() => {
            normalize_segments(&Path::new(cwd).join(path))
        }
        Some(_) => normalize_segments(Path::new(path)),
        None => PathBuf::from(path),
    };
    Some(FileTarget {
        scope: "project",
        path: joined.to_str()?.to_string(),
        line,
    })
}

/// The file target of a `path` detector match: an optional `file://` prefix, then `path`,
/// `path:line` or `path:line:column`.
pub fn file_target_of_text(text: &str, cwd: Option<&str>) -> Option<FileTarget> {
    let value = text.strip_prefix("file://").unwrap_or(text);
    let (path, line, _column) = parse_file_reference(value)?;
    file_target(path.to_str()?, line, cwd)
}

/// Splits `path:line` text of the `path-line` detector; the line must be a `u32`.
pub fn parse_path_line(text: &str) -> Option<(&str, u32)> {
    let (path, line) = text.rsplit_once(':')?;
    if path.is_empty() || line.is_empty() || !line.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((path, line.parse().ok()?))
}
