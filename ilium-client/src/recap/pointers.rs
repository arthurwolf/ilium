//! Pointer validation. A pointer survives only when it is named in the
//! evidence the model saw, resolves to an existing file under a permitted
//! project root, and is not in a protected location.

use std::path::{Path, PathBuf};

use super::custody::{denied_path_reason, redact_secrets};
use super::model::RecapPointer;

/// Result of validating one text's pointers.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PointerOutcome {
    pub accepted: Vec<RecapPointer>,
    pub rejected: usize,
}

/// Validates `pointers` against `evidence_text` and the permitted `roots`.
/// Rejected pointers are counted, never repaired or guessed.
pub fn validate_pointers(
    pointers: Vec<RecapPointer>,
    evidence_text: &str,
    roots: &[PathBuf],
) -> PointerOutcome {
    let mut outcome = PointerOutcome::default();
    for pointer in pointers {
        if !pointer_is_valid(&pointer, evidence_text, roots) {
            outcome.rejected += 1;
            continue;
        }
        outcome.accepted.push(RecapPointer {
            path: pointer.path,
            note: redact_secrets(&pointer.note).text,
        });
    }
    outcome
}

fn pointer_is_valid(pointer: &RecapPointer, evidence_text: &str, roots: &[PathBuf]) -> bool {
    if pointer.path.is_empty() || !evidence_text.contains(&pointer.path) {
        return false;
    }
    let candidate = Path::new(&pointer.path);
    if denied_path_reason(candidate).is_some() {
        return false;
    }
    roots.iter().any(|root| resolves_under(candidate, root))
}

/// True when `candidate` (absolute, or relative to `root`) is an existing file
/// whose real location sits inside `root`. Symlinks are followed before the
/// check, so a link out of the project fails.
fn resolves_under(candidate: &Path, root: &Path) -> bool {
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let (Ok(real_root), Ok(real_file)) = (root.canonicalize(), joined.canonicalize()) else {
        return false;
    };
    real_file.is_file()
        && real_file.starts_with(&real_root)
        && denied_path_reason(&real_file).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_with_file(name: &str) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("tempdir");
        let file = directory.path().join(name);
        std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        std::fs::write(&file, "x").expect("write");
        let root = directory.path().to_path_buf();
        (directory, root)
    }

    fn pointer(path: &str) -> RecapPointer {
        RecapPointer {
            path: path.into(),
            note: "note".into(),
        }
    }

    #[test]
    fn accepts_existing_file_named_in_evidence() {
        let (_directory, root) = project_with_file("src/lib.rs");
        let outcome = validate_pointers(
            vec![pointer("src/lib.rs")],
            "edited src/lib.rs",
            std::slice::from_ref(&root),
        );
        assert_eq!(outcome.accepted.len(), 1);
        assert_eq!(outcome.rejected, 0);
    }

    #[test]
    fn rejects_pointer_not_mentioned_in_evidence() {
        let (_directory, root) = project_with_file("src/lib.rs");
        let outcome = validate_pointers(vec![pointer("src/lib.rs")], "nothing here", &[root]);
        assert!(outcome.accepted.is_empty());
        assert_eq!(outcome.rejected, 1);
    }

    #[test]
    fn rejects_missing_file_and_outside_root() {
        let (_directory, root) = project_with_file("src/lib.rs");
        let outside = tempfile::tempdir().expect("tempdir");
        let outside_file = outside.path().join("other.rs");
        std::fs::write(&outside_file, "x").expect("write");
        let outside_text = outside_file.display().to_string();
        let outcome = validate_pointers(
            vec![pointer("src/missing.rs"), pointer(&outside_text)],
            &format!("src/missing.rs {outside_text}"),
            &[root],
        );
        assert!(outcome.accepted.is_empty());
        assert_eq!(outcome.rejected, 2);
    }

    #[test]
    fn rejects_denied_paths_even_when_mentioned_and_present() {
        let (_directory, root) = project_with_file(".env");
        let outcome = validate_pointers(vec![pointer(".env")], "wrote .env", &[root]);
        assert!(outcome.accepted.is_empty());
        assert_eq!(outcome.rejected, 1);
    }

    #[test]
    fn redacts_secrets_in_pointer_notes() {
        let (_directory, root) = project_with_file("src/lib.rs");
        let note_pointer = RecapPointer {
            path: "src/lib.rs".into(),
            note: "set api_key=abcd1234efgh5678".into(),
        };
        let outcome = validate_pointers(vec![note_pointer], "src/lib.rs", &[root]);
        assert!(!outcome.accepted[0].note.contains("abcd1234efgh5678"));
    }
}
