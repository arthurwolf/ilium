//! Resolves file references embedded in physical editor source lines.
//!
//! The editor-line context menu owns activation, while this module owns the
//! narrow normalization and disk check required to decide whether activation
//! is available. Keeping that policy pure except for the final metadata check
//! prevents menu visibility and action execution from drifting apart.

use std::path::{Path, PathBuf};

/// Removes presentation punctuation around a candidate file reference while
/// preserving its actual path characters, including Unix and Windows
/// separators. The first and last retained characters must be a separator or
/// alphanumeric, matching the source-line interaction contract.
pub fn normalized_path_candidate(line: &str) -> Option<&str> {
    let candidate = line.trim_matches(|character: char| {
        character != '/' && character != '\\' && !character.is_alphanumeric()
    });
    (!candidate.is_empty()).then_some(candidate)
}

/// Resolves a normalized source-line candidate relative to the project CWD
/// and returns it only when it is an ordinary file on disk.
pub fn project_file_from_line(line: &str, project_cwd: &Path) -> Option<PathBuf> {
    let candidate = PathBuf::from(normalized_path_candidate(line)?);
    let path = if candidate.is_absolute() {
        candidate
    } else {
        project_cwd.join(candidate)
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
}
