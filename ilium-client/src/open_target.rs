//! Decides whether the OS's own "open externally" action should be offered for a URL or
//! filesystem path under a click.
//!
//! Finding the link is not done here: the link detectors an enabled plugin contributes
//! (`contributes.link_detectors`, the Terminal plugin's `url`, `path-line` and `path`) cover the
//! clicked cell, and [`crate::plugin_host::links::plugin_link_at`] returns the match. With no
//! such plugin enabled nothing is detected and nothing is offered. This module only adds the
//! policy layer needed to decide whether the OS opener should be offered for the matched text:
//! an allowed URL scheme, or a path that actually exists on disk (which also picks "Open file"
//! vs "Open folder"). A `path[:line[:column]]` reference resolves like a shell would: `~/`
//! against the home directory, a relative path against the working directory.
//!
//! The path side never calls `stat` on an arbitrary token: terminal and
//! editor text can be attacker- or agent-influenced, and a path pointing at
//! an unresponsive network mount would block the UI thread inside the mouse
//! handler that calls this. Only a path already confined to `cwd` or
//! `home_dir` is ever touched.

use std::path::{Component, Path, PathBuf};

/// What a click on terminal text resolved to, before the user confirms it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalLink {
    /// An OSC 8 hyperlink of the foreground program.
    Url(String),
    /// Text a plugin's link detector matched (contract G5). Opening it sends `link-activated`
    /// to `plugin` and runs `command` with the detector id and the text as arguments.
    Plugin {
        plugin: String,
        detector_id: String,
        command: String,
        text: String,
    },
}

impl TerminalLink {
    pub fn display(&self) -> String {
        match self {
            Self::Url(url) => url.clone(),
            Self::Plugin { text, .. } => text.clone(),
        }
    }
}

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

    /// Returns the locally verified regular file behind this target.
    pub fn file_path(&self) -> Option<&Path> {
        match self {
            Self::File(path) => Some(path),
            Self::Url(_) | Self::Directory(_) => None,
        }
    }
}

/// Finds a clickable target at `column` in `line` through the plugin link `detectors` and
/// resolves it against the filesystem. Returns `None` when no detector covers the cell, or when
/// the matched text is neither an allowed-scheme URL nor a path that exists on disk -- callers
/// must not offer an OS-open action for text that merely looks like one.
pub(crate) fn resolve_at(
    line: &str,
    column: usize,
    cwd: &Path,
    home_dir: Option<&Path>,
    detectors: &[crate::plugin_host::links::LinkDetector],
) -> Option<OpenTarget> {
    resolve_at_with_plugins(line, column, cwd, home_dir, detectors).0
}

/// What a right-click on a terminal cell can open, asking the link detectors of the enabled
/// plugins (contract G5), like a Ctrl+click does. Returns the target the menu's "Open"
/// entries use and the plugin match that covers the cell, if any. The text of a plugin match
/// is resolved as a URL, or as a path that exists.
pub(crate) fn resolve_at_with_plugins(
    line: &str,
    column: usize,
    cwd: &Path,
    home_dir: Option<&Path>,
    detectors: &[crate::plugin_host::links::LinkDetector],
) -> (
    Option<OpenTarget>,
    Option<crate::plugin_host::links::LinkMatch>,
) {
    let found = crate::plugin_host::links::plugin_link_at(detectors, line, column);
    let target = found
        .as_ref()
        .and_then(|found| resolve_text(&found.text, cwd, home_dir));
    (target, found)
}

/// Resolves the text a link detector matched: an allowed URL, or a `path`, `path:line` or
/// `path:line:column` reference (optionally a `file://` URL) that exists on disk.
fn resolve_text(text: &str, cwd: &Path, home_dir: Option<&Path>) -> Option<OpenTarget> {
    if let Some(target) = resolve_url(text) {
        return Some(target);
    }
    let value = text.strip_prefix("file://").unwrap_or(text);
    let path = parse_file_reference(value)?;
    resolve_path(&resolve_reference_path(path, cwd, home_dir), cwd, home_dir)
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

/// The path of a `path`, `path:line` or `path:line:column` reference. The trailing numeric
/// segments are peeled off strictly from the right so that a colon embedded earlier in the
/// path (e.g. `a/b:c.txt:42`) keeps its non-numeric segment attached to the path.
fn parse_file_reference(value: &str) -> Option<PathBuf> {
    let path = match value.rsplit_once(':') {
        Some((rest, last)) if last.parse::<u32>().is_ok() => match rest.rsplit_once(':') {
            Some((before, middle)) if middle.parse::<u32>().is_ok() => before,
            _ => rest,
        },
        _ => value,
    };
    // A bare token can still name a project-local file. Resolution and the final existence and
    // confinement check decide whether it is actionable, so this admits `STYLE.md` without
    // opening arbitrary paths.
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Validates a URL obtained independently of the link detectors (an OSC-8 hyperlink
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
    // `Path::starts_with` compares components verbatim and never resolves
    // `..` itself, so a raw `/cwd/../etc/passwd`-shaped candidate (exactly
    // what `resolve_reference_path` produces for a `../`
    // reference) would satisfy `starts_with(cwd)` while actually pointing
    // outside it. Normalize lexically first so confinement is checked
    // against where the path really points, still without touching disk.
    let normalized = normalize_lexically(path);

    // The confinement roots must be normalized the same way as the candidate:
    // a `cwd` that itself contains a `..` component (e.g. a server launched
    // with `--cwd /a/b/../c`) would otherwise never prefix-match any
    // normalized candidate, silently disabling path opening for the whole
    // session.
    let normalized_cwd = normalize_lexically(cwd);
    let is_confined = normalized.starts_with(&normalized_cwd)
        || home_dir.is_some_and(|home_dir| normalized.starts_with(normalize_lexically(home_dir)));
    if !is_confined {
        return None;
    }
    if normalized.is_dir() {
        Some(OpenTarget::Directory(normalized))
    } else if normalized.is_file() {
        Some(OpenTarget::File(normalized))
    } else {
        None
    }
}

/// Collapses `.` and `..` components without touching the filesystem (no
/// `canonicalize`, since the whole point is deciding *before* any `stat`
/// whether a path is even allowed to be touched).
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match normalized.last() {
                Some(Component::Normal(_)) => {
                    normalized.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => normalized.push(component),
            },
            other => normalized.push(other),
        }
    }
    normalized.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The link detectors of the shipped Terminal plugin, the way an enabled plugin set offers
    /// them to a click.
    fn terminal_detectors() -> Vec<crate::plugin_host::links::LinkDetector> {
        crate::plugin_host::links::active_detectors(&[(
            "ilium.core.terminal".to_owned(),
            include_str!("../../plugins/core/terminal/manifest.json").to_owned(),
        )])
    }

    #[test]
    fn resolves_an_allowed_url() {
        let target = resolve_at(
            "see https://example.test/docs.",
            10,
            Path::new("/tmp"),
            None,
            &terminal_detectors(),
        );
        assert_eq!(
            target,
            Some(OpenTarget::Url("https://example.test/docs".to_string()))
        );
    }

    fn detectors(link_detectors: &str) -> Vec<crate::plugin_host::links::LinkDetector> {
        let manifest = format!(
            r#"{{"id":"x","contributes":{{"commands":[{{"id":"open"}}],"link_detectors":{link_detectors}}}}}"#
        );
        crate::plugin_host::links::active_detectors(&[("example.plugin".to_owned(), manifest)])
    }

    #[test]
    fn a_plugin_match_under_the_click_is_returned_and_its_text_resolved() {
        let project = tempfile::tempdir().unwrap();
        let detectors = detectors(r##"[{"id":"issue","regex":"#[0-9]+","command":"open"}]"##);
        assert_eq!(detectors.len(), 1);
        // `#42` is no URL and no existing path, and neither is anything else on the line, so
        // only the plugin match is offered.
        let (target, found) =
            resolve_at_with_plugins("fix #42 now", 5, project.path(), None, &detectors);
        let found = found.expect("the plugin detector covers the clicked cell");
        assert_eq!(found.detector_id, "issue");
        assert_eq!(found.command, "example.plugin/open");
        assert_eq!(found.text, "#42");
        assert_eq!(target, None);
        // A click outside every plugin match offers nothing: the kernel detects no links itself.
        let (target, found) = resolve_at_with_plugins(
            "see https://example.test/x",
            6,
            project.path(),
            None,
            &detectors,
        );
        assert_eq!(found, None);
        assert_eq!(target, None);
        // Without an enabled detector nothing is offered at all.
        assert_eq!(
            resolve_at("see https://example.test/x", 6, project.path(), None, &[]),
            None
        );
    }

    #[test]
    fn a_plugin_matched_url_or_existing_path_keeps_the_kernel_open_entries() {
        let project = tempfile::tempdir().unwrap();
        let file_path = project.path().join("src").join("lib.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "").unwrap();
        let detectors = detectors(
            r#"[{"id":"url","regex":"https?://[^\\s]+","command":"open"},
                {"id":"path","regex":"[./A-Za-z0-9_-]+\\.[a-z]+:[0-9]+","command":"open"}]"#,
        );
        let (target, found) = resolve_at_with_plugins(
            "at https://example.test:8080/a",
            14,
            project.path(),
            None,
            &detectors,
        );
        assert_eq!(found.map(|found| found.detector_id).as_deref(), Some("url"));
        assert_eq!(
            target,
            Some(OpenTarget::Url("https://example.test:8080/a".to_string()))
        );
        let (target, found) =
            resolve_at_with_plugins("at src/lib.rs:3", 5, project.path(), None, &detectors);
        assert_eq!(
            found.map(|found| found.detector_id).as_deref(),
            Some("path")
        );
        assert_eq!(target, Some(OpenTarget::File(file_path)));
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
        let target = resolve_at(line, 4, project.path(), None, &terminal_detectors());
        assert_eq!(target, Some(OpenTarget::File(file_path)));

        assert_eq!(
            resolve_at(
                "see src/missing.rs",
                4,
                project.path(),
                None,
                &terminal_detectors()
            ),
            None
        );
    }

    #[test]
    fn resolves_an_unprefixed_project_local_file_after_trailing_comma_trim() {
        let project = tempfile::tempdir().unwrap();
        let file_path = project.path().join("data").join("STYLE.md");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "# Style\n").unwrap();

        assert_eq!(
            resolve_at(
                "Read data/STYLE.md, before continuing",
                7,
                project.path(),
                None,
                &terminal_detectors()
            ),
            Some(OpenTarget::File(file_path))
        );
    }

    #[test]
    fn distinguishes_a_folder_from_a_file() {
        let project = tempfile::tempdir().unwrap();
        let dir_path = project.path().join("docs").join("guides");
        std::fs::create_dir_all(&dir_path).unwrap();

        let target = resolve_at(
            "open docs/guides for reference",
            5,
            project.path(),
            None,
            &terminal_detectors(),
        );
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

        assert_eq!(
            resolve_at(&line, 4, cwd.path(), None, &terminal_detectors()),
            None
        );
    }

    #[test]
    fn refuses_to_stat_a_dot_dot_path_that_escapes_cwd_via_component_prefix_matching() {
        // `resolve_reference_path` builds `../`-relative references as
        // `cwd.join("../...")`, which literally starts with `cwd` as a raw
        // component prefix even though it resolves outside it. The
        // confinement check must normalize `..` away before comparing,
        // not just prefix-match the unresolved path.
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("real.txt"), b"hi").unwrap();
        let cwd = outside.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();

        let line = "see ../real.txt for details";
        assert_eq!(resolve_at(line, 4, &cwd, None, &terminal_detectors()), None);
    }

    #[test]
    fn resolves_inside_a_cwd_that_itself_contains_a_dot_dot_component() {
        // A session cwd like `/a/b/../c` must confine (and resolve) exactly
        // like its normalized form `/a/c`: the candidate is normalized before
        // the prefix check, so the roots must be normalized too or nothing
        // in the project would ever match.
        let project = tempfile::tempdir().unwrap();
        let file_path = project.path().join("src").join("main.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "fn main() {}").unwrap();

        let unnormalized_cwd = project.path().join("src").join("..");
        let target = resolve_at(
            "see src/main.rs for details",
            4,
            &unnormalized_cwd,
            None,
            &terminal_detectors(),
        );
        assert_eq!(target, Some(OpenTarget::File(file_path)));
    }

    #[test]
    fn accounts_for_multi_byte_glyphs_when_resolving_a_click() {
        let project = tempfile::tempdir().unwrap();
        let file_path = project.path().join("src").join("main.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "fn main() {}").unwrap();

        // "⏺ " is 4 bytes but only 2 cells wide, mirroring the agent-pane
        // status-glyph prefixes the terminal plugin's link detectors cover.
        let target = resolve_at(
            "⏺ src/main.rs",
            2,
            project.path(),
            None,
            &terminal_detectors(),
        );
        assert_eq!(target, Some(OpenTarget::File(file_path)));
    }
}
