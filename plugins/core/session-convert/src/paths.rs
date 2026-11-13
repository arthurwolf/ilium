//! Where Claude Code keeps its project transcripts, and where Codex keeps its home. The plugin never reads the
//! environment: the host passes `CODEX_HOME` in.

use std::path::{Path, PathBuf};

/// Claude Code's project directory name: every non `[A-Za-z0-9]` character of the canonical cwd becomes `-`.
pub fn claude_project_slug(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

pub fn claude_project_dir(home_dir: &Path, project_cwd: &Path) -> PathBuf {
    home_dir
        .join(".claude")
        .join("projects")
        .join(claude_project_slug(project_cwd))
}

/// `codex_home` when given, else the `CODEX_HOME` value the host read, else `<home>/.codex`.
pub fn resolve_codex_home(
    home_dir: &Path,
    requested: Option<&Path>,
    environment_codex_home: Option<&str>,
) -> PathBuf {
    if let Some(explicit) = requested {
        return explicit.to_path_buf();
    }
    match environment_codex_home {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => home_dir.join(".codex"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ: PORT:ilium-session-convert/src/paths.rs::slug_replaces_every_non_alphanumeric_character
    #[test]
    fn slug_replaces_every_non_alphanumeric_character() {
        assert_eq!(
            claude_project_slug(Path::new("/home/a.b/c_d e")),
            "-home-a-b-c-d-e"
        );
    }

    /// REQ: PORT:ilium-session-convert/src/paths.rs::explicit_codex_home_wins
    #[test]
    fn explicit_codex_home_wins() {
        assert_eq!(
            resolve_codex_home(Path::new("/h"), Some(Path::new("/x/codex")), Some("/env")),
            PathBuf::from("/x/codex")
        );
        assert_eq!(
            resolve_codex_home(Path::new("/h"), None, Some("/env")),
            PathBuf::from("/env")
        );
        assert_eq!(
            resolve_codex_home(Path::new("/h"), None, None),
            PathBuf::from("/h/.codex")
        );
    }
}
