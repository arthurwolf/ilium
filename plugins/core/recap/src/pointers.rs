//! Pointer validation. A pointer survives only when it is named in the evidence the model saw,
//! resolves to an existing file under the pane's project directory, and is not in a protected
//! location. Existence is asked of the caller, so the check itself is pure.

use std::path::{Component, Path};

use crate::custody::{denied_path_reason, redact_secrets};
use crate::model::RecapPointer;

/// Result of validating one text's pointers.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PointerOutcome {
    pub accepted: Vec<RecapPointer>,
    pub rejected: usize,
}

/// Validates `pointers` against `evidence_text` and `project_dir`. `is_file` answers whether a
/// path relative to the project directory is an existing regular file. Rejected pointers are
/// counted, never repaired or guessed.
pub fn validate_pointers(
    pointers: Vec<RecapPointer>,
    evidence_text: &str,
    project_dir: &str,
    is_file: &mut dyn FnMut(&str) -> bool,
) -> PointerOutcome {
    let mut outcome = PointerOutcome::default();
    for pointer in pointers {
        if !pointer_is_valid(&pointer, evidence_text, project_dir, is_file) {
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

fn pointer_is_valid(
    pointer: &RecapPointer,
    evidence_text: &str,
    project_dir: &str,
    is_file: &mut dyn FnMut(&str) -> bool,
) -> bool {
    if pointer.path.is_empty() || !evidence_text.contains(&pointer.path) {
        return false;
    }
    let candidate = Path::new(&pointer.path);
    if denied_path_reason(candidate).is_some() {
        return false;
    }
    let Some(relative) = relative_to_project(candidate, Path::new(project_dir)) else {
        return false;
    };
    if denied_path_reason(Path::new(&relative)).is_some() {
        return false;
    }
    is_file(&relative)
}

/// `candidate` (absolute, or relative to `root`) as a normalized path below `root`, or `None`
/// when it leaves `root`, climbs with `..` or names `root` itself.
pub fn relative_to_project(candidate: &Path, root: &Path) -> Option<String> {
    let relative = if candidate.is_absolute() {
        candidate.strip_prefix(root).ok()?
    } else {
        candidate
    };
    let mut parts: Vec<String> = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?.to_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(path: &str) -> RecapPointer {
        RecapPointer {
            path: path.into(),
            note: "note".into(),
        }
    }

    fn only(existing: &'static str) -> impl FnMut(&str) -> bool {
        move |path| path == existing
    }

    #[test]
    fn accepts_existing_file_named_in_evidence() {
        let outcome = validate_pointers(
            vec![pointer("src/lib.rs"), pointer("/work/app/src/lib.rs")],
            "edited src/lib.rs and /work/app/src/lib.rs",
            "/work/app",
            &mut only("src/lib.rs"),
        );
        assert_eq!(outcome.accepted.len(), 2);
        assert_eq!(outcome.rejected, 0);
    }

    #[test]
    fn rejects_pointer_not_mentioned_in_evidence() {
        let outcome = validate_pointers(
            vec![pointer("src/lib.rs")],
            "nothing here",
            "/work/app",
            &mut only("src/lib.rs"),
        );
        assert!(outcome.accepted.is_empty());
        assert_eq!(outcome.rejected, 1);
    }

    #[test]
    fn rejects_missing_file_outside_root_and_climbing_paths() {
        let outcome = validate_pointers(
            vec![
                pointer("src/missing.rs"),
                pointer("/other/file.rs"),
                pointer("../escape.rs"),
                pointer("src/../../escape.rs"),
            ],
            "src/missing.rs /other/file.rs ../escape.rs src/../../escape.rs",
            "/work/app",
            &mut |_| true,
        );
        // The missing-file pointer passes the lexical checks here because the stub says it exists.
        assert_eq!(outcome.accepted.len(), 1);
        assert_eq!(outcome.rejected, 3);
        let outcome = validate_pointers(
            vec![pointer("src/missing.rs")],
            "src/missing.rs",
            "/work/app",
            &mut |_| false,
        );
        assert_eq!(outcome.rejected, 1);
    }

    #[test]
    fn rejects_denied_paths_even_when_mentioned_and_present() {
        let outcome = validate_pointers(
            vec![pointer(".env")],
            "wrote .env",
            "/work/app",
            &mut |_| true,
        );
        assert!(outcome.accepted.is_empty());
        assert_eq!(outcome.rejected, 1);
    }

    #[test]
    fn redacts_secrets_in_pointer_notes() {
        let note_pointer = RecapPointer {
            path: "src/lib.rs".into(),
            note: "set api_key=abcd1234efgh5678".into(),
        };
        let outcome =
            validate_pointers(vec![note_pointer], "src/lib.rs", "/work/app", &mut |_| true);
        assert!(!outcome.accepted[0].note.contains("abcd1234efgh5678"));
    }

    #[test]
    fn relative_paths_are_normalized_below_the_root() {
        let root = Path::new("/work/app");
        assert_eq!(
            relative_to_project(Path::new("./src//lib.rs"), root).as_deref(),
            Some("src/lib.rs")
        );
        assert_eq!(relative_to_project(Path::new("/work/app"), root), None);
        assert_eq!(relative_to_project(Path::new("/work/app2/x"), root), None);
    }
}
