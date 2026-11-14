//! Resolves file references embedded in physical editor source lines.
//!
//! The normalization is pure. The disk check is not done here: the plugin never touches the file
//! system directly, it asks the host (`fs` import) whether the resolved path is an ordinary file
//! and passes the answer to [`project_file_from_line`] as the `is_file` closure.
//!
//! Moved from `ilium-client/src/editor_line_path.rs`.

use std::path::{Component, Path, PathBuf};

/// Removes presentation punctuation around a candidate file reference while preserving its actual
/// path characters, including Unix and Windows separators. The first and last retained characters
/// must be a separator or alphanumeric, matching the source-line interaction contract.
///
/// The leading edge additionally retains `.`, unlike the trailing edge: a leading `.` is
/// path-meaningful (`../sibling.rs`, `./local.rs`, `.gitignore`) and anchors relative-vs-absolute
/// resolution, while a trailing `.` is ordinary prose punctuation (`see docs/plan.md.`). Trimming a
/// leading `..` would strip it down to the separator that followed, silently turning
/// `../src/lib.rs` into the absolute-looking `/src/lib.rs`.
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

/// Resolves a normalized source-line candidate relative to the project working directory and
/// returns it only when `is_file` (the host's answer) says it is an ordinary file.
pub fn project_file_from_line(
    line: &str,
    project_cwd: &Path,
    is_file: impl FnOnce(&Path) -> bool,
) -> Option<PathBuf> {
    let candidate = PathBuf::from(normalized_path_candidate(line)?);
    let mut components = candidate.components().peekable();
    // `is_absolute()` alone misses Windows paths that are anchored but not fully absolute:
    // root-without-prefix (`\foo.txt`) and prefix-without-root (`C:foo.txt`). Both still carry a
    // leading Prefix or RootDir component, and `PathBuf::extend` discards an existing buffer when
    // fed one of those, which would silently drop `project_cwd`. Any leading Prefix/RootDir
    // component is treated as anchored and used as-is.
    let is_anchored = matches!(
        components.peek(),
        Some(Component::Prefix(_) | Component::RootDir)
    );
    let path = if is_anchored {
        candidate
    } else {
        // Extended component by component rather than joined whole. A source line writes
        // `docs/guide.md` whatever the platform, and the components round-trip into the native
        // form instead of carrying a foreign separator through.
        let mut resolved = project_cwd.to_path_buf();
        resolved.extend(components);
        resolved
    };
    is_file(&path).then_some(path)
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
    fn resolves_only_what_the_host_says_is_a_file() {
        let project_cwd = Path::new("/work/project");
        let plan = project_cwd.join("docs").join("plan.md");
        let answer = |candidate: &Path| candidate == plan;
        assert_eq!(
            project_file_from_line("- `docs/plan.md`", project_cwd, answer),
            Some(plan.clone())
        );
        assert_eq!(project_file_from_line("docs", project_cwd, answer), None);
        assert_eq!(
            project_file_from_line("missing.md", project_cwd, answer),
            None
        );
    }

    #[test]
    fn resolves_a_leading_parent_dir_reference_relative_to_project_cwd() {
        let project_cwd = Path::new("/work/project");
        assert_eq!(
            project_file_from_line("- `../sibling.rs`", project_cwd, |_| true),
            Some(project_cwd.join("../sibling.rs"))
        );
    }

    #[test]
    fn a_forward_slash_reference_resolves_to_a_native_path() {
        let project_cwd = Path::new("/work/project");
        let file_path = project_cwd.join("docs").join("plan.md");
        let resolved = project_file_from_line("- `docs/plan.md`", project_cwd, |_| true)
            .expect("an accepted file resolves");
        assert_eq!(resolved, file_path);
        assert!(!resolved.to_string_lossy().contains(&format!(
            "docs{}plan.md",
            if cfg!(windows) { "/" } else { "\\" }
        )));
    }

    #[test]
    fn an_anchored_reference_ignores_the_project_directory() {
        let resolved = project_file_from_line("\"/tmp/report.txt\"", Path::new("/work"), |_| true)
            .expect("an accepted file resolves");
        assert_eq!(resolved, PathBuf::from("/tmp/report.txt"));
    }
}
