//! Golden and snapshot files (reference-test-harness.md "Snapshot and golden files").

/// Environment variable that switches golden files to update mode.
pub const UPDATE_GOLDEN_ENV: &str = "ILIUM_PLUGIN_UPDATE_GOLDEN";

/// Compares `actual` to the golden file after converting CRLF to LF; fails with a unified diff of at most 40 lines.
pub fn assert_golden(_path: &str, _actual: &str) {
    unimplemented!("DEV-500")
}

/// Compares the value, as pretty JSON with sorted keys and a trailing newline, to `tests/snapshots/<name>.json`.
pub fn assert_snapshot(_name: &str, _value: &impl serde::Serialize) {
    unimplemented!("DEV-504")
}

#[cfg(test)]
mod spec_tests {
    use super::{assert_golden, assert_snapshot, UPDATE_GOLDEN_ENV};
    use crate::errors::codes;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard};

    /// Serialises the tests that change process environment variables, because the harness runs tests in parallel.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Sets the golden-update and `CI` variables for one test and restores their previous values on drop.
    struct GoldenEnv {
        _lock: MutexGuard<'static, ()>,
        previous: Vec<(&'static str, Option<OsString>)>,
    }

    impl GoldenEnv {
        fn set(update_mode: bool, ci: bool) -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let names: [&'static str; 2] = [UPDATE_GOLDEN_ENV, "CI"];
            let previous = names
                .iter()
                .map(|name| (*name, std::env::var_os(name)))
                .collect();
            set_flag(UPDATE_GOLDEN_ENV, update_mode);
            set_flag("CI", ci);
            Self { _lock: lock, previous }
        }
    }

    impl Drop for GoldenEnv {
        fn drop(&mut self) {
            for (name, value) in &self.previous {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    fn set_flag(name: &str, on: bool) {
        if on {
            std::env::set_var(name, "1");
        } else {
            std::env::remove_var(name);
        }
    }

    /// A golden file under the system temp directory, owned by one test and removed on drop.
    struct ScratchFile(PathBuf);

    impl ScratchFile {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("ilium-golden-spec-{}-{label}.txt", std::process::id()));
            let _ = std::fs::remove_file(&path);
            Self(path)
        }

        fn path_str(&self) -> &str {
            self.0.to_str().expect("temp path is UTF-8")
        }
    }

    impl Drop for ScratchFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// A snapshot file under the crate's `tests/snapshots` folder, removed on drop.
    struct SnapshotFile(PathBuf);

    impl SnapshotFile {
        fn write(name: &str, contents: &str) -> Self {
            let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("snapshots");
            std::fs::create_dir_all(&directory).expect("create snapshots folder");
            let path = directory.join(format!("{name}.json"));
            std::fs::write(&path, contents).expect("write snapshot file");
            Self(path)
        }
    }

    impl Drop for SnapshotFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            if let Some(parent) = self.0.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }
    }

    /// Runs `check` and returns its panic message, or `None` when it returns normally.
    fn panic_message(check: impl FnOnce() + std::panic::UnwindSafe) -> Option<String> {
        let payload = std::panic::catch_unwind(check).err()?;
        let message = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|text| (*text).to_string()))
            .unwrap_or_default();
        Some(message)
    }

    /// REQ: DEV-500 (assert_golden compares bytes after converting CRLF to LF)
    #[test]
    fn dev_500_crlf_in_actual_text_matches_lf_golden() {
        let _env = GoldenEnv::set(false, false);
        let golden = ScratchFile::new("crlf");
        std::fs::write(&golden.0, "first row\nsecond row\n").expect("write golden");
        assert_golden(golden.path_str(), "first row\r\nsecond row\r\n");
    }

    /// REQ: DEV-500 (empty actual text matches an empty golden file)
    #[test]
    fn dev_500_empty_actual_text_matches_empty_golden() {
        let _env = GoldenEnv::set(false, false);
        let golden = ScratchFile::new("empty");
        std::fs::write(&golden.0, "").expect("write golden");
        assert_golden(golden.path_str(), "");
    }

    /// REQ: DEV-500 (a mismatch fails with a unified diff of at most 40 changed lines)
    #[test]
    fn dev_500_mismatch_diff_shows_at_most_forty_changed_lines() {
        let _env = GoldenEnv::set(false, false);
        let golden = ScratchFile::new("diff-limit");
        let expected: String = (0..100).map(|row| format!("expected row {row}\n")).collect();
        let actual: String = (0..100).map(|row| format!("actual row {row}\n")).collect();
        std::fs::write(&golden.0, &expected).expect("write golden");
        let message = panic_message(|| assert_golden(golden.path_str(), &actual))
            .expect("a mismatch must fail");
        assert!(
            !message.starts_with("not implemented"),
            "assert_golden must report a mismatch, not an unwritten stub: {message}"
        );
        let changed_lines = message
            .lines()
            .filter(|line| {
                (line.starts_with('+') || line.starts_with('-'))
                    && !line.starts_with("+++")
                    && !line.starts_with("---")
            })
            .count();
        assert!(
            changed_lines > 0 && changed_lines <= 40,
            "changed lines in diff: {changed_lines}"
        );
    }

    /// REQ: DEV-501 (update mode writes the actual text to the golden file and passes)
    #[test]
    fn dev_501_update_mode_rewrites_golden_and_passes() {
        let _env = GoldenEnv::set(true, false);
        let golden = ScratchFile::new("update");
        std::fs::write(&golden.0, "stale\n").expect("write golden");
        assert_golden(golden.path_str(), "fresh row\nsecond row\n");
        let written = std::fs::read_to_string(&golden.0).expect("read golden");
        assert_eq!(written, "fresh row\nsecond row\n");
    }

    /// REQ: DEV-502 (a missing golden file fails with harness.golden-missing outside update mode)
    #[test]
    fn dev_502_missing_golden_fails_with_golden_missing_code() {
        let _env = GoldenEnv::set(false, false);
        let golden = ScratchFile::new("missing");
        let message = panic_message(|| assert_golden(golden.path_str(), "row\n"))
            .expect("a missing golden file must fail");
        assert!(
            message.contains(codes::GOLDEN_MISSING),
            "failure message was: {message}"
        );
    }

    /// REQ: DEV-502 (update mode creates a missing golden file and passes)
    #[test]
    fn dev_502_update_mode_creates_missing_golden_file() {
        let _env = GoldenEnv::set(true, false);
        let golden = ScratchFile::new("create");
        assert_golden(golden.path_str(), "created row\n");
        let written = std::fs::read_to_string(&golden.0).expect("golden file created");
        assert_eq!(written, "created row\n");
    }

    /// REQ: DEV-503 (update mode is refused when CI is set and non-empty, and the golden file is left unchanged)
    #[test]
    fn dev_503_update_in_ci_fails_and_leaves_golden_unchanged() {
        let _env = GoldenEnv::set(true, true);
        let golden = ScratchFile::new("ci");
        std::fs::write(&golden.0, "kept\n").expect("write golden");
        let message = panic_message(|| assert_golden(golden.path_str(), "changed\n"))
            .expect("update in CI must fail");
        assert!(
            message.contains(codes::GOLDEN_UPDATE_IN_CI),
            "failure message was: {message}"
        );
        let unchanged = std::fs::read_to_string(&golden.0).expect("read golden");
        assert_eq!(unchanged, "kept\n");
    }

    /// REQ: DEV-504 (a snapshot file is pretty JSON with sorted keys and a trailing newline)
    #[test]
    fn dev_504_snapshot_is_pretty_sorted_json_with_trailing_newline() {
        let _env = GoldenEnv::set(false, false);
        let _snapshot = SnapshotFile::write(
            "spec_dev_504_sorted",
            "{\n  \"alpha\": {\n    \"inner\": [\n      1,\n      2\n    ],\n    \"zeta\": true\n  },\n  \"beta\": \"text\"\n}\n",
        );
        let value = serde_json::json!({
            "beta": "text",
            "alpha": { "zeta": true, "inner": [1, 2] }
        });
        assert_snapshot("spec_dev_504_sorted", &value);
    }

    /// REQ: DEV-504 (an empty object is stored as `{}` followed by a trailing newline)
    #[test]
    fn dev_504_empty_object_snapshot_is_braces_with_trailing_newline() {
        let _env = GoldenEnv::set(false, false);
        let _snapshot = SnapshotFile::write("spec_dev_504_empty", "{}\n");
        assert_snapshot("spec_dev_504_empty", &serde_json::json!({}));
    }
}
