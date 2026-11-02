//! Conservative process-descriptor check shared by provider goal readers.

use std::path::Path;

/// Other open JSONL files may be older or different sessions. Their presence
/// makes the current session ambiguous until stronger ownership is available.
pub(crate) fn only_transcript_open(process_id: u32, expected: &Path) -> bool {
    let Ok(paths) = ilium_platform::process_info::open_file_paths_strict(
        process_id,
        ilium_platform::process_info::OpenFilePathLimits {
            descriptors: 4096,
            retained_bytes: 1024 * 1024,
        },
    ) else {
        return false;
    };
    paths_are_unambiguous(&paths, expected)
}

fn paths_are_unambiguous(paths: &[std::path::PathBuf], expected: &Path) -> bool {
    paths.iter().any(|path| path == expected)
        && !paths.iter().any(|path| {
            path != expected
                && (path
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
                    || path
                        .as_os_str()
                        .to_string_lossy()
                        .ends_with(".jsonl (deleted)"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_ownership_rejects_missing_or_ambiguous_paths() {
        let expected = Path::new("/sessions/current.jsonl");
        assert!(!paths_are_unambiguous(&[], expected));
        assert!(paths_are_unambiguous(
            &[expected.into(), "/other/log.txt".into()],
            expected
        ));
        assert!(!paths_are_unambiguous(
            &[expected.into(), "/sessions/old.jsonl".into()],
            expected
        ));
        assert!(!paths_are_unambiguous(
            &[expected.into(), "/sessions/old.jsonl (deleted)".into()],
            expected
        ));
    }
}
