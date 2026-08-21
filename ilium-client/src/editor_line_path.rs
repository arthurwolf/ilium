//! Resolves file references embedded in physical editor source lines.
//!
//! The editor-line context menu owns activation, while this module owns the
//! narrow normalization and disk check required to decide whether activation
//! is available. Keeping that policy pure except for the final metadata check
//! prevents menu visibility and action execution from drifting apart.

use std::path::{Component, Path, PathBuf};

/// Removes presentation punctuation around a candidate file reference while
/// preserving its actual path characters, including Unix and Windows
/// separators. The first and last retained characters must be a separator or
/// alphanumeric, matching the source-line interaction contract.
///
/// The leading edge additionally retains `.`, unlike the trailing edge: a
/// leading `.` is path-meaningful (`../sibling.rs`, `./local.rs`,
/// `.gitignore`) and anchors relative-vs-absolute resolution, while a
/// trailing `.` is ordinary prose punctuation (`see docs/plan.md.`). Trimming
/// a leading `..` used to strip it down to the separator that followed,
/// silently turning `../src/lib.rs` into the absolute-looking `/src/lib.rs`.
pub fn normalized_path_candidate(line: &str) -> Option<&str> {
    let candidate = line
        .trim_start_matches(|character: char| {
            character != '/'
                && character != '\\'
                && character != '.'
                && !character.is_alphanumeric()
        })
        .trim_end_matches(|character: char| {
            character != '/' && character != '\\' && !character.is_alphanumeric()
        });
    (!candidate.is_empty()).then_some(candidate)
}

/// Resolves a normalized source-line candidate relative to the project CWD
/// and returns it only when it is an ordinary file on disk.
pub fn project_file_from_line(line: &str, project_cwd: &Path) -> Option<PathBuf> {
    let candidate = PathBuf::from(normalized_path_candidate(line)?);
    let mut components = candidate.components().peekable();
    // `is_absolute()` alone misses Windows paths that are anchored but not
    // fully absolute -- root-without-prefix (`\foo.txt`) and
    // prefix-without-root (`C:foo.txt`). Both still carry a leading Prefix or
    // RootDir component, and `PathBuf::extend`'s documented Windows push
    // semantics discard most or all of an existing buffer when fed one of
    // those, which would silently drop `project_cwd` instead of resolving
    // under it. Treat any leading Prefix/RootDir component as anchored so it
    // is used as-is rather than combined with `project_cwd`.
    let is_anchored = matches!(
        components.peek(),
        Some(Component::Prefix(_) | Component::RootDir)
    );
    let path = if is_anchored {
        candidate
    } else {
        // Extended component-by-component rather than joined whole. A source
        // line writes `docs/guide.md` whatever the platform, and joining that
        // verbatim on Windows yields `C:\project\docs/guide.md` -- which opens,
        // because Windows accepts either separator, but is then shown to the
        // user and stored with both. Windows treats `/` as a separator when
        // splitting too, so the components round-trip into the native form.
        let mut resolved = project_cwd.to_path_buf();
        resolved.extend(components);
        resolved
    };
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_quotes_markdown_bullets_and_whitespace() {
        assert_eq!(
            normalized_path_candidate("  - `docs/plan.md`  "),
            Some("docs/plan.md")
        );
        assert_eq!(
            normalized_path_candidate("\"/tmp/report.txt\""),
            Some("/tmp/report.txt")
        );
        assert_eq!(normalized_path_candidate("***"), None);
    }

    /// A leading `..`/`.` anchors relative-vs-absolute resolution and must
    /// survive normalization intact -- trimming it down to the separator
    /// that follows would turn `../src/lib.rs` into the absolute-looking
    /// `/src/lib.rs`, silently escaping `project_cwd`.
    #[test]
    fn preserves_leading_dot_segments_that_anchor_the_path() {
        assert_eq!(
            normalized_path_candidate("- `../src/lib.rs`"),
            Some("../src/lib.rs")
        );
        assert_eq!(
            normalized_path_candidate("`.gitignore`"),
            Some(".gitignore")
        );
    }

    #[test]
    fn resolves_only_existing_regular_files_relative_to_project_cwd() {
        let project = tempfile::tempdir().unwrap();
        let project_cwd = project.path();
        let file_path = project_cwd.join("docs").join("plan.md");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "# Plan\n").unwrap();

        assert_eq!(
            project_file_from_line("- `docs/plan.md`", project_cwd),
            Some(file_path.clone())
        );
        assert_eq!(project_file_from_line("docs", project_cwd), None);
        assert_eq!(project_file_from_line("missing.md", project_cwd), None);
    }

    #[test]
    fn resolves_a_leading_parent_dir_reference_relative_to_project_cwd() {
        let workspace = tempfile::tempdir().unwrap();
        let project_cwd = workspace.path().join("project");
        std::fs::create_dir_all(&project_cwd).unwrap();
        let sibling_file = workspace.path().join("sibling.rs");
        std::fs::write(&sibling_file, "fn main() {}\n").unwrap();

        assert_eq!(
            project_file_from_line("- `../sibling.rs`", &project_cwd),
            Some(project_cwd.join("../sibling.rs"))
        );
    }

    /// A source line writes `docs/plan.md` on every platform, but the resolved
    /// path is shown to the user and stored, so it must come back in the
    /// platform's own form rather than carrying a stray separator through.
    #[test]
    fn a_forward_slash_reference_resolves_to_a_native_path() {
        let project = tempfile::tempdir().unwrap();
        let project_cwd = project.path();
        let file_path = project_cwd.join("docs").join("plan.md");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "# Plan\n").unwrap();

        let resolved = project_file_from_line("- `docs/plan.md`", project_cwd)
            .expect("existing file should resolve");

        assert_eq!(
            resolved, file_path,
            "resolved path should be built from native components"
        );
        assert!(
            !resolved.to_string_lossy().contains(&format!(
                "docs{}plan.md",
                if cfg!(windows) { "/" } else { "\\" }
            )),
            "resolved path {resolved:?} kept a foreign separator"
        );
    }
}
