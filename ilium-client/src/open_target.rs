//! Detects a URL or filesystem path at a clicked position and decides
//! whether the OS's own "open externally" action should be offered for it.
//!
//! Detection itself is not reinvented here: [`crate::terminal_links::link_at`]
//! already expands a clicked cell into a URL or file-reference token (cell
//! widths, multi-byte glyph prefixes, wrapping punctuation, `path:line:col`
//! suffixes, `~/` and relative resolution all handled there). This module
//! only adds the policy layer needed to decide whether the OS opener should
//! be offered: an allowed URL scheme, or a path that actually exists on disk
//! (which also picks "Open file" vs "Open folder").
//!
//! The path side never calls `stat` on an arbitrary token: terminal and
//! editor text can be attacker- or agent-influenced, and a path pointing at
//! an unresponsive network mount would block the UI thread inside the mouse
//! handler that calls this. Only a path already confined to `cwd` or
//! `home_dir` is ever touched.

use std::path::{Path, PathBuf};

use crate::terminal_links::{link_at, TerminalLink};

/// A right-click target the OS's own "open externally" handler can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenTarget {
    Url(String),
    File(PathBuf),
    Directory(PathBuf),
}

impl OpenTarget {
    /// The menu label for this target's action.
    pub const fn menu_label(&self) -> &'static str {
        match self {
            Self::Url(_) => "Open in browser",
            Self::File(_) => "Open file",
            Self::Directory(_) => "Open folder",
        }
    }
}

/// Finds a clickable target at `column` in `line` and resolves it against the
/// filesystem. Returns `None` when the token is neither an allowed-scheme URL
/// nor a path that exists on disk -- callers must not offer an OS-open action
/// for text that merely looks like one.
pub fn resolve_at(
    line: &str,
    column: usize,
    cwd: &Path,
    home_dir: Option<&Path>,
) -> Option<OpenTarget> {
    match link_at(line, column, cwd, home_dir)? {
        TerminalLink::Url(url) => resolve_url(&url),
        TerminalLink::File { path, .. } => resolve_path(&path, cwd, home_dir),
    }
}

/// Validates a URL obtained independently of `link_at` (an OSC-8 hyperlink
/// label can be any string the foreground program chose to emit).
pub fn resolve_url(url: &str) -> Option<OpenTarget> {
    (url.starts_with("https://") || url.starts_with("http://"))
        .then(|| OpenTarget::Url(url.to_string()))
}

/// Classifies an already-resolved path candidate. Only returns `Some` for a
/// path that exists on disk right now -- the whole point of this action is
/// that it never advertises "Open file"/"Open folder" for a target that
/// doesn't exist. Refuses to touch the filesystem at all for a path outside
/// `cwd`/`home_dir`: the candidate came from clicked terminal or editor text,
/// so an absolute path elsewhere on the machine (an unresponsive network
/// mount, say) must never reach `stat` from this click handler.
fn resolve_path(path: &Path, cwd: &Path, home_dir: Option<&Path>) -> Option<OpenTarget> {
    let is_confined =
        path.starts_with(cwd) || home_dir.is_some_and(|home_dir| path.starts_with(home_dir));
    if !is_confined {
        return None;
    }
    if path.is_dir() {
        Some(OpenTarget::Directory(path.to_path_buf()))
    } else if path.is_file() {
        Some(OpenTarget::File(path.to_path_buf()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_an_allowed_url() {
        let target = resolve_at(
            "see https://example.test/docs.",
            10,
            Path::new("/tmp"),
            None,
        );
        assert_eq!(
            target,
            Some(OpenTarget::Url("https://example.test/docs".to_string()))
        );
    }

    #[test]
    fn refuses_a_disallowed_scheme_from_an_osc8_label() {
        assert_eq!(resolve_url("javascript:alert(1)"), None);
        assert_eq!(resolve_url("file:///etc/passwd"), None);
        assert_eq!(
            resolve_url("https://example.test"),
            Some(OpenTarget::Url("https://example.test".to_string()))
        );
    }

    #[test]
    fn resolves_an_existing_file_but_not_a_missing_one() {
        let project = tempfile::tempdir().unwrap();
        let file_path = project.path().join("src").join("main.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "fn main() {}").unwrap();

        let line = "see src/main.rs:42:7 for details";
        let target = resolve_at(line, 4, project.path(), None);
        assert_eq!(target, Some(OpenTarget::File(file_path)));

        assert_eq!(
            resolve_at("see src/missing.rs", 4, project.path(), None),
            None
        );
    }

    #[test]
    fn distinguishes_a_folder_from_a_file() {
        let project = tempfile::tempdir().unwrap();
        let dir_path = project.path().join("docs").join("guides");
        std::fs::create_dir_all(&dir_path).unwrap();

        let target = resolve_at("open docs/guides for reference", 5, project.path(), None);
        assert_eq!(target, Some(OpenTarget::Directory(dir_path)));
    }

    #[test]
    fn refuses_to_stat_a_path_outside_cwd_and_home_dir() {
        // This existing path is real (so a missing-vs-confined bug can't hide
        // behind "it returned None anyway because the path doesn't exist"),
        // but it sits outside both `cwd` and `home_dir` -- attacker- or
        // agent-influenced terminal text pointing at an absolute path
        // elsewhere on the machine must never reach `stat` from this
        // click handler.
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("real.txt"), b"hi").unwrap();
        let line = format!(
            "see {} for details",
            outside.path().join("real.txt").display()
        );
        let cwd = tempfile::tempdir().unwrap();

        assert_eq!(resolve_at(&line, 4, cwd.path(), None), None);
    }

    #[test]
    fn accounts_for_multi_byte_glyphs_when_resolving_a_click() {
        let project = tempfile::tempdir().unwrap();
        let file_path = project.path().join("src").join("main.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "fn main() {}").unwrap();

        // "⏺ " is 4 bytes but only 2 cells wide, mirroring the agent-pane
        // status-glyph prefixes `terminal_links::link_at` already handles.
        let target = resolve_at("⏺ src/main.rs", 2, project.path(), None);
        assert_eq!(target, Some(OpenTarget::File(file_path)));
    }
}
