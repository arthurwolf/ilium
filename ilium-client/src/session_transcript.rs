//! Locates the on-disk transcript file for a Claude Code/Codex session
//! whose ID is already known, so `session_naming::infer_pane_title` can
//! read its recent prompts. Pure `std::fs` path resolution -- no PTY, no
//! process-tree walk -- so it is genuinely client-local work: ilium-client
//! already owns "what local files does it read", and a session's
//! transcript is a plain file under `~/.claude`/`~/.codex`.
//!
//! This intentionally duplicates a handful of functions that also still
//! exist in the legacy `ilium` bin crate's `agent_detect.rs`
//! (`session_id_from_transcript_stem`, `looks_like_uuid`,
//! `slugify_claude_project_path`) -- those are shared there with the
//! bin's own 5-tier *session-ID discovery* (which walks `/proc` against a
//! real OS process this client no longer has direct access to, since
//! ilium-server now owns every pane's PTY), so they can't simply move
//! out from under it in this stage. The duplication is small (pure,
//! well-tested path/string logic) and transitional: the next stage
//! rewires the `ilium` bin to stop owning detection itself, at which
//! point `agent_detect.rs` and its copy of this logic are deleted,
//! leaving this module as the only copy.

use std::path::{Path, PathBuf};

use ilium_core::AgentClass;

/// Locates the on-disk transcript file for a session ID ilium already
/// knows with certainty. `None` when the agent class has no known
/// transcript format (`AgentClass::Other`) or no matching file is found.
pub fn transcript_path_for_session(
    class: &AgentClass,
    home: &Path,
    cwd: &Path,
    session_id: &str,
) -> Option<PathBuf> {
    let (dir, expected_extension) = transcript_dir_and_extension(class, home, cwd)?;
    transcript_files_under(&dir, class.clone(), expected_extension)
        .into_iter()
        .find(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| session_id_from_transcript_stem(class, stem))
                .as_deref()
                == Some(session_id)
        })
}

/// The directory to search and the file extension to expect for a given
/// agent's transcripts -- `None` for `AgentClass::Other`, which has no known
/// transcript format.
fn transcript_dir_and_extension(
    class: &AgentClass,
    home: &Path,
    cwd: &Path,
) -> Option<(PathBuf, &'static str)> {
    match class {
        AgentClass::Claude => Some((
            home.join(".claude")
                .join("projects")
                .join(slugify_claude_project_path(cwd)),
            "jsonl",
        )),
        AgentClass::Codex => Some((home.join(".codex").join("sessions"), "jsonl")),
        AgentClass::Other(_) => None,
    }
}

/// Returns transcript files in `directory` that match `expected_extension`.
/// Codex stores current rollouts under date directories while Claude keeps a
/// project's transcripts flat, so only Codex needs recursive traversal here.
fn transcript_files_under(
    directory: &Path,
    class: AgentClass,
    expected_extension: &str,
) -> Vec<PathBuf> {
    let mut directories = vec![directory.to_path_buf()];
    let mut transcript_paths = Vec::new();

    while let Some(current_directory) = directories.pop() {
        let Ok(entries) = std::fs::read_dir(current_directory) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                if class == AgentClass::Codex {
                    directories.push(path);
                }
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) == Some(expected_extension)
            {
                transcript_paths.push(path);
            }
        }
    }

    transcript_paths
}

/// Parses a transcript filename's stem (no directory, no extension) into a
/// session ID, given which CLI's naming convention to expect.
fn session_id_from_transcript_stem(class: &AgentClass, stem: &str) -> Option<String> {
    match class {
        // Claude Code: the whole stem is the session UUID.
        AgentClass::Claude => looks_like_uuid(stem).then(|| stem.to_string()),
        // Codex rollout filenames have changed from a date-only prefix and
        // `.json` extension to a timestamped, nested `.jsonl` layout. The
        // stable contract is the final UUID, so accept either layout rather
        // than coupling recovery to the timestamp representation.
        AgentClass::Codex => {
            let after_prefix = stem.strip_prefix("rollout-")?;
            let uuid_start = after_prefix.len().checked_sub(36)?;
            let candidate = after_prefix.get(uuid_start..)?;
            (uuid_start > 0
                && after_prefix.as_bytes().get(uuid_start - 1) == Some(&b'-')
                && looks_like_uuid(candidate))
            .then(|| candidate.to_string())
        }
        AgentClass::Other(_) => None,
    }
}

/// Reproduces Claude Code's project-directory slug: every character that
/// isn't an ASCII letter or digit becomes `-` (e.g. `/home/developer/.claude-docs`
/// -> `-home-arthur--claude-docs`, `/tmp/e2e_project` -> `-tmp-e2e-project`).
/// Verified against real `~/.claude/projects/<slug>` directories: Claude Code
/// replaces every non-alphanumeric byte, not just `/` and `.` -- a version of
/// this function that replaced only those two left underscores (and any
/// other punctuation) untouched, so it silently computed a project directory
/// that never existed for any cwd containing one, and transcript lookups
/// permanently missed for those projects. Kept in sync with
/// `ilium-server`'s `session_id::slugify_claude_project_path`, which the
/// same duplication note at the top of this file applies to.
fn slugify_claude_project_path(cwd: &Path) -> String {
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

/// True for the canonical UUID layout used by Claude and Codex session IDs.
fn looks_like_uuid(value: &str) -> bool {
    const HYPHEN_INDICES: [usize; 4] = [8, 13, 18, 23];
    value.len() == 36
        && value.chars().enumerate().all(|(index, character)| {
            if HYPHEN_INDICES.contains(&index) {
                character == '-'
            } else {
                character.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("ilium-client-session-transcript-tests")
            .join(format!("{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn transcript_path_for_session_finds_the_matching_claude_file_among_several() {
        let home = scratch_dir();
        let cwd = Path::new("/home/developer/dev/ai/ilium");
        let project_dir = home
            .join(".claude")
            .join("projects")
            .join(slugify_claude_project_path(cwd));
        std::fs::create_dir_all(&project_dir).unwrap();

        let other_id = "44444444-4444-4444-8444-444444444444";
        let target_id = "55555555-5555-4555-8555-555555555555";
        std::fs::write(project_dir.join(format!("{other_id}.jsonl")), "{}").unwrap();
        std::fs::write(project_dir.join(format!("{target_id}.jsonl")), "{}").unwrap();

        let path = transcript_path_for_session(&AgentClass::Claude, &home, cwd, target_id)
            .expect("target transcript should be found");
        assert_eq!(path, project_dir.join(format!("{target_id}.jsonl")));
    }

    #[test]
    fn transcript_path_for_session_finds_the_matching_codex_rollout_in_nested_directories() {
        let home = scratch_dir();
        let cwd = Path::new("/some/other/project");
        let sessions_dir = home
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("07")
            .join("11");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        let id = "66666666-6666-4666-8666-666666666666";
        std::fs::write(
            sessions_dir.join(format!("rollout-2026-07-11T22-32-39-{id}.jsonl")),
            "{}",
        )
        .unwrap();

        let path = transcript_path_for_session(&AgentClass::Codex, &home, cwd, id)
            .expect("target rollout should be found");
        assert_eq!(
            path,
            sessions_dir.join(format!("rollout-2026-07-11T22-32-39-{id}.jsonl"))
        );
    }

    #[test]
    fn slugify_replaces_underscores_and_other_punctuation_too() {
        // Confirmed against real `~/.claude/projects/` directories: Claude
        // Code's own slug replaces every non-alphanumeric character, not
        // just `/` and `.` -- e.g. a real recorded cwd of
        // `/media/arthur/Big/Old_Disk_Extracted` is stored under
        // `-media-arthur-Big-Old-Disk-Extracted`. A version of this function
        // that only replaced `/` and `.` would instead produce
        // `-media-arthur-Big-Old_Disk-Extracted`, a directory that never
        // exists on disk.
        assert_eq!(
            slugify_claude_project_path(Path::new("/media/arthur/Big/Old_Disk_Extracted")),
            "-media-arthur-Big-Old-Disk-Extracted"
        );
    }

    #[test]
    fn transcript_path_for_session_returns_none_for_unknown_agent_class() {
        let home = scratch_dir();
        let cwd = Path::new("/home/developer/dev/ai/ilium");
        assert_eq!(
            transcript_path_for_session(
                &AgentClass::Other("opencode".to_string()),
                &home,
                cwd,
                "any-id"
            ),
            None
        );
    }
}
