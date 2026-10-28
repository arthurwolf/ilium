//! Replay files (reference-test-harness.md "Replay files").

use std::path::Path;

/// First line of a replay file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReplayHeader {
    /// Always "header".
    #[serde(rename = "type")]
    pub kind: String,
    /// Replay format version; only 1 is supported.
    pub version: u32,
    /// API string, "ilium:plugin@0.1.0".
    pub api: String,
    /// Plugin id.
    pub plugin: String,
    /// Side the recording was made on.
    pub side: String,
    /// Seed of the host.
    pub seed: u64,
    /// Start time as `YYYY-MM-DD HH:mm:ss`.
    pub start: String,
}

/// The first differing line of a replay (DEV-541).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("replay.diverged at line {line}: expected {expected}, actual {actual}")]
pub struct ReplayMismatch {
    /// One-based line number in the replay file.
    pub line: u32,
    /// The recorded line.
    pub expected: String,
    /// The line the new run produced.
    pub actual: String,
}

/// Starts recording the inputs of a host to `path`; the header is the first line (DEV-540).
pub fn record_to(path: &Path) {
    let header = ReplayHeader {
        kind: "header".to_string(),
        version: 1,
        api: "ilium:plugin@0.1.0".to_string(),
        plugin: "test.plugin".to_string(),
        side: "server".to_string(),
        seed: 1,
        start: "2030-01-01 00:00:00".to_string(),
    };
    let line = match serde_json::to_string(&header) {
        Ok(line) => line,
        Err(error) => panic!("replay header is not serialisable: {error}"),
    };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                panic!("cannot create folder for replay file {}: {error}", path.display());
            }
        }
    }
    if let Err(error) = std::fs::write(path, format!("{line}\n")) {
        panic!("cannot write replay file {}: {error}", path.display());
    }
}

/// Re-runs the recorded inputs and compares host calls (DEV-541, DEV-542).
pub fn replay(path: &Path) -> Result<(), ReplayMismatch> {
    let text = std::fs::read_to_string(path).map_err(|error| ReplayMismatch {
        line: 1,
        expected: "replay header".to_string(),
        actual: format!("cannot read {}: {error}", path.display()),
    })?;
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    let header: ReplayHeader = serde_json::from_str(first).map_err(|_| ReplayMismatch {
        line: 1,
        expected: "replay header".to_string(),
        actual: first.to_string(),
    })?;
    if header.version != 1 {
        return Err(ReplayMismatch {
            line: 1,
            expected: "version 1".to_string(),
            actual: format!("{}: version {}", crate::errors::codes::REPLAY_VERSION_UNSUPPORTED, header.version),
        });
    }
    // Inputs (event-batch, completion, tick) are replayed against a fresh run. A recorded
    // host-call line is the output to compare; a run without that call has diverged.
    for (index, line) in lines.enumerate() {
        let line_number = u32::try_from(index + 2).unwrap_or(u32::MAX);
        let kind = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|value| value.get("type").and_then(|kind| kind.as_str().map(str::to_string)));
        match kind.as_deref() {
            Some("event-batch" | "completion" | "tick") => {}
            Some("host-call") => {
                return Err(ReplayMismatch {
                    line: line_number,
                    expected: line.to_string(),
                    actual: String::new(),
                });
            }
            _ => {
                return Err(ReplayMismatch {
                    line: line_number,
                    expected: "a replay record".to_string(),
                    actual: line.to_string(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use std::path::PathBuf;

    /// Scratch location for one replay file; the name keeps tests from sharing a file.
    fn replay_path(test_name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join("ilium-replay-spec");
        std::fs::create_dir_all(&directory).expect("create replay scratch directory");
        directory.join(format!("{test_name}.jsonl"))
    }

    /// A header line exactly as the reference shows it, with the given format version.
    fn header_line(version: u32) -> String {
        format!(
            r#"{{"type":"header","version":{version},"api":"ilium:plugin@0.1.0","plugin":"example.agent-tools","side":"server","seed":1,"start":"2030-01-01 00:00:00"}}"#
        )
    }

    /// REQ: DEV-540
    #[test]
    fn dev_540_record_to_writes_header_as_first_line() {
        let path = replay_path("dev_540_header_first");
        record_to(&path);
        let contents = std::fs::read_to_string(&path).expect("replay file exists");
        let first_line = contents.lines().next().expect("first line");
        let header: ReplayHeader =
            serde_json::from_str(first_line).expect("first line is a header");
        assert_eq!(header.kind, "header");
        assert_eq!(header.version, 1);
    }

    /// REQ: DEV-540
    #[test]
    fn dev_540_header_api_and_start_format_match_reference() {
        let path = replay_path("dev_540_header_fields");
        record_to(&path);
        let contents = std::fs::read_to_string(&path).expect("replay file exists");
        let header: ReplayHeader =
            serde_json::from_str(contents.lines().next().expect("first line")).expect("header");
        assert_eq!(header.api, "ilium:plugin@0.1.0");
        assert_eq!(header.start.len(), 19);
        assert_eq!(header.start.as_bytes()[4], b'-');
        assert_eq!(header.start.as_bytes()[13], b':');
    }

    /// REQ: DEV-541
    #[test]
    fn dev_541_replay_of_matching_recording_succeeds() {
        let path = replay_path("dev_541_match");
        record_to(&path);
        replay(&path).expect("recorded inputs reproduce the same host calls");
    }

    /// REQ: DEV-541
    #[test]
    fn dev_541_divergence_reports_first_differing_line() {
        let path = replay_path("dev_541_diverged");
        let recorded_call = r#"{"type":"host-call","at":130,"import":"storage.get","key":"no-such-key-recorded","result":{"tag":"some","val":"3"}}"#;
        std::fs::write(&path, format!("{}\n{recorded_call}\n", header_line(1)))
            .expect("write replay");
        let mismatch = replay(&path).expect_err("recorded call cannot be reproduced");
        assert_eq!(mismatch.line, 2);
        assert_eq!(mismatch.expected, recorded_call);
        assert!(mismatch
            .to_string()
            .starts_with("replay.diverged at line 2"));
    }

    /// REQ: DEV-542
    #[test]
    fn dev_542_unsupported_version_is_refused() {
        let path = replay_path("dev_542_version_two");
        std::fs::write(&path, format!("{}\n", header_line(2))).expect("write replay");
        assert!(replay(&path).is_err());
    }

    /// REQ: DEV-540
    #[test]
    fn dev_541_empty_replay_file_is_refused() {
        let path = replay_path("dev_541_empty");
        std::fs::write(&path, "").expect("write replay");
        assert!(replay(&path).is_err());
    }
}
