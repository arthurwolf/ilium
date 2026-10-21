//! The append-only audit log (reference-security.md).

use crate::error::HostError;

/// One audit line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuditRecord {
    /// Time, milliseconds since the Unix epoch.
    pub at_unix_millis: u64,
    /// The plugin concerned.
    pub plugin: String,
    /// The action, for example `grant.approved`.
    pub action: String,
    /// Detail as JSON.
    pub detail_json: String,
}

/// The audit log writer.
#[derive(Debug)]
pub struct AuditLog;

impl AuditLog {
    /// Opens or creates the log.
    pub fn open(_path: std::path::PathBuf) -> Result<AuditLog, HostError> {
        unimplemented!("host::audit::AuditLog::open")
    }

    /// Appends one record as a JSON line.
    pub fn append(&mut self, _record: &AuditRecord) -> Result<(), HostError> {
        unimplemented!("host::audit::AuditLog::append")
    }
}

/// Every audit event name of the catalogue, in table order (reference-audit-log.md SEC-222).
pub const EVENT_NAMES: &[&str] = &[
    "install",
    "update",
    "downgrade",
    "uninstall",
    "enable",
    "disable",
    "failure-disable",
    "grant-given",
    "grant-revoked",
    "consent-given",
    "consent-withdrawn",
    "trust-add",
    "trust-remove",
    "replacement-on",
    "replacement-off",
    "policy-loaded",
    "policy-violation",
    "sandbox-unavailable",
    "sandbox-degraded",
    "signature-failure",
    "integrity-failure",
    "blocklist-hit",
    "process-run",
    "write-outside-data",
    "egress-host-first-use",
    "extension-disabled-after-breaches",
    "store-external-edit",
    "h-grant-refused",
    "project-trust",
    "safe-mode",
    "audit-reset",
    "audit-dropped",
];

#[cfg(test)]
mod tests {
    use super::{AuditLog, AuditRecord, EVENT_NAMES};
    use sha2::{Digest, Sha256};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    const MANUAL_AUDIT_LOG_PAGE: &str =
        include_str!("../../docs/plugins-manual/pages/reference-audit-log.md");

    fn sample_record(action: &str) -> AuditRecord {
        AuditRecord {
            at_unix_millis: 1_786_000_000_000,
            plugin: "example.notes".to_owned(),
            action: action.to_owned(),
            detail_json: "{}".to_owned(),
        }
    }

    fn audit_file_path(directory: &Path) -> PathBuf {
        directory.join("audit.jsonl")
    }

    fn open_log(log_path: &Path) -> AuditLog {
        AuditLog::open(log_path.to_path_buf()).expect("the audit log opens")
    }

    fn append_all(log_path: &Path, records: &[AuditRecord]) {
        let mut log = open_log(log_path);
        for record in records {
            log.append(record).expect("the record is appended");
        }
    }

    fn read_lines(log_path: &Path) -> Vec<String> {
        std::fs::read_to_string(log_path)
            .expect("the audit file exists and is readable")
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn parse_line_as_json(line: &str) -> serde_json::Value {
        serde_json::from_str(line).expect("each audit line is one JSON object")
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Reads the event names from the "| Event | When |" table of the manual page.
    fn catalogue_names_in_manual() -> Vec<String> {
        MANUAL_AUDIT_LOG_PAGE
            .lines()
            .skip_while(|line| !line.starts_with("| Event | When |"))
            .skip(2)
            .take_while(|line| line.starts_with('|'))
            .filter_map(|line| line.split('|').nth(1))
            .flat_map(|cell| {
                cell.split(',')
                    .map(|name| name.trim().trim_matches('`').to_owned())
                    .collect::<Vec<String>>()
            })
            .filter(|name| !name.is_empty())
            .collect()
    }

    /// REQ: SEC-222
    #[test]
    fn sec_222_event_names_match_the_manual_catalogue_in_table_order() {
        let manual_names = catalogue_names_in_manual();
        assert!(
            !manual_names.is_empty(),
            "the manual's event catalogue table is found and parsed"
        );
        let event_names: Vec<String> = EVENT_NAMES.iter().map(|name| (*name).to_owned()).collect();
        assert_eq!(event_names, manual_names);
    }

    /// REQ: SEC-222
    #[test]
    fn sec_222_event_names_are_unique() {
        let mut seen_names = HashSet::new();
        for name in EVENT_NAMES {
            assert!(seen_names.insert(*name), "duplicate event name {name}");
        }
    }

    /// REQ: SEC-222
    #[test]
    fn sec_222_event_names_are_nonempty_lowercase_kebab_case() {
        for name in EVENT_NAMES {
            assert!(!name.is_empty(), "an event name is empty");
            assert!(
                name.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'-'),
                "event name {name} is not lowercase kebab-case"
            );
            assert!(
                !name.starts_with('-') && !name.ends_with('-'),
                "event name {name} starts or ends with a hyphen"
            );
        }
    }

    /// REQ: SEC-220
    #[test]
    fn sec_220_append_writes_the_audit_file_at_the_given_path() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        append_all(&log_path, &[sample_record("install")]);
        assert!(log_path.is_file(), "audit.jsonl exists after the first append");
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_append_writes_one_json_object_per_line_with_trailing_newline() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        append_all(&log_path, &[sample_record("install")]);
        let raw_content = std::fs::read_to_string(&log_path).expect("the audit file is readable");
        assert!(raw_content.ends_with('\n'), "the line ends with a newline");
        assert_eq!(raw_content.lines().count(), 1, "one record is one line");
        assert!(parse_line_as_json(raw_content.trim_end_matches('\n')).is_object());
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_appended_records_read_back_unchanged_and_in_order() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        let records = vec![
            sample_record("install"),
            sample_record("grant-given"),
            sample_record("enable"),
        ];
        append_all(&log_path, &records);
        let lines = read_lines(&log_path);
        assert_eq!(lines.len(), records.len());
        for (line, record) in lines.iter().zip(records.iter()) {
            let read_back: AuditRecord =
                serde_json::from_str(line).expect("each line deserializes as an audit record");
            assert_eq!(&read_back, record);
        }
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_reopened_log_keeps_existing_bytes_and_appends_after_them() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        append_all(&log_path, &[sample_record("install")]);
        let bytes_before_reopen = std::fs::read(&log_path).expect("the audit file is readable");
        append_all(&log_path, &[sample_record("enable")]);
        let bytes_after_reopen = std::fs::read(&log_path).expect("the audit file is readable");
        assert!(
            bytes_after_reopen.starts_with(&bytes_before_reopen),
            "earlier bytes are never rewritten"
        );
        assert_eq!(read_lines(&log_path).len(), 2);
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_detail_containing_newlines_stays_on_one_line() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        let record = AuditRecord {
            at_unix_millis: 1,
            plugin: "example.notes".to_owned(),
            action: "process-run".to_owned(),
            detail_json: "first line\nsecond line".to_owned(),
        };
        append_all(&log_path, std::slice::from_ref(&record));
        let lines = read_lines(&log_path);
        assert_eq!(lines.len(), 1, "an embedded newline is escaped inside the JSON line");
        let read_back: AuditRecord =
            serde_json::from_str(&lines[0]).expect("the line deserializes as an audit record");
        assert_eq!(read_back, record);
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_empty_and_extreme_field_values_round_trip() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        let empty_record = AuditRecord {
            at_unix_millis: 0,
            plugin: String::new(),
            action: String::new(),
            detail_json: String::new(),
        };
        let extreme_record = AuditRecord {
            at_unix_millis: u64::MAX,
            plugin: "x".repeat(4096),
            action: "audit-dropped".to_owned(),
            detail_json: "{\"count\":18446744073709551615}".to_owned(),
        };
        append_all(&log_path, &[empty_record.clone(), extreme_record.clone()]);
        let lines = read_lines(&log_path);
        assert_eq!(lines.len(), 2);
        let first: AuditRecord = serde_json::from_str(&lines[0]).expect("first line parses");
        let second: AuditRecord = serde_json::from_str(&lines[1]).expect("second line parses");
        assert_eq!(first, empty_record);
        assert_eq!(second, extreme_record);
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_seq_starts_at_one_and_increases_by_one_across_reopen() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        append_all(&log_path, &[sample_record("install"), sample_record("enable")]);
        append_all(&log_path, &[sample_record("disable")]);
        let lines = read_lines(&log_path);
        assert_eq!(lines.len(), 3);
        for (index, line) in lines.iter().enumerate() {
            let value = parse_line_as_json(line);
            assert_eq!(value["seq"].as_u64(), Some(index as u64 + 1), "line {}", index + 1);
        }
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_first_record_prev_hash_is_sixty_four_zeros() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        append_all(&log_path, &[sample_record("install")]);
        let lines = read_lines(&log_path);
        let first_line = lines.first().expect("the first record is written");
        let zero_hash = "0".repeat(64);
        assert_eq!(
            parse_line_as_json(first_line)["prev_hash"].as_str(),
            Some(zero_hash.as_str())
        );
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_each_prev_hash_is_sha256_of_the_previous_line_without_newline() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        append_all(&log_path, &[sample_record("install"), sample_record("enable")]);
        append_all(&log_path, &[sample_record("disable")]);
        let lines = read_lines(&log_path);
        assert_eq!(lines.len(), 3);
        for index in 1..lines.len() {
            let expected_hash = sha256_hex(lines[index - 1].as_bytes());
            assert_eq!(
                parse_line_as_json(&lines[index])["prev_hash"].as_str(),
                Some(expected_hash.as_str()),
                "record {} links to record {}",
                index + 1,
                index
            );
        }
    }

    /// REQ: SEC-224
    #[test]
    fn sec_224_deleted_log_file_is_recreated_with_audit_reset_and_zero_prev_hash() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        let mut log = open_log(&log_path);
        log.append(&sample_record("install")).expect("the first record is appended");
        std::fs::remove_file(&log_path).expect("the audit file is removed");
        log.append(&sample_record("enable")).expect("the record after deletion is appended");
        let lines = read_lines(&log_path);
        assert!(
            lines.iter().any(|line| line.contains("audit-reset")),
            "a recreated file starts with an audit-reset record"
        );
        let first_line = lines.first().expect("the recreated file has a first line");
        let zero_hash = "0".repeat(64);
        assert_eq!(
            parse_line_as_json(first_line)["prev_hash"].as_str(),
            Some(zero_hash.as_str())
        );
    }
}
