//! `ilium plugin support-bundle` and what the bundle holds: DEV-831..833 of reference-plugin-cli.md,
//! USR-221..225 and USR-229 of reference-requirements-user-admin.md (core-plugin-diagnostics.md,
//! "Support bundle" and "Crash records"). Each test drives the real command.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ilium_plugin_api::Side;
use ilium_plugin_host::cli::{CliEnv, CliOutput};
use ilium_plugin_host::crash_records::{format_record, records_path, CrashReason, CrashRecord};

use crate::common::{
    env_with_session, error_code, log_line, records_of, run, scratch, write_log, FakeBackend,
};

const ID: &str = "example.notes";
const EIGHT_MIB: u64 = 8 * 1024 * 1024;

fn bundle_run(env: &CliEnv, out: &Path) -> CliOutput {
    let out_text = out.to_str().expect("utf8 path");
    run(env, &["support-bundle", ID, "--out", out_text, "--json"])
}

/// Every entry of the archive, by its name, as text.
fn bundle_entries(path: &Path) -> BTreeMap<String, String> {
    let file = std::fs::File::open(path).expect("open bundle");
    let mut archive = zip::ZipArchive::new(file).expect("read zip archive");
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("archive entry");
        let mut text = String::new();
        entry.read_to_string(&mut text).expect("utf8 entry");
        entries.insert(entry.name().to_owned(), text);
    }
    entries
}

/// Every file under a folder, with its bytes, so a test can prove nothing changed.
fn tree_snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("read dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.insert(path.clone(), std::fs::read(&path).expect("read file"));
            }
        }
    }
    files
}

/// Two hundred lines of 64 KiB each: 12.8 MB, which the 8 MiB limit must trim.
fn oversized_log(env: &CliEnv) {
    let padding = "x".repeat(64 * 1024);
    let lines: Vec<String> = (0..200)
        .map(|index| format!("LINE-{index:04} {padding}"))
        .collect();
    write_log(env, ID, "", &lines);
}

fn fixed_time() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_893_499_200)
}

/// REQ: DEV-831
#[test]
fn dev_831_writes_the_archive_and_prints_one_support_bundle_record() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    let out = scratch.path().join("bundle.zip");
    let output = bundle_run(&env, &out);
    assert_eq!(output.exit_code, 0);
    let records = records_of(&output, "support-bundle");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["path"].as_str(), out.to_str());
    let written = std::fs::metadata(&out).expect("bundle written").len();
    assert_eq!(records[0]["bytes"].as_u64(), Some(written));
}

/// REQ: DEV-832
#[test]
fn dev_832_an_existing_target_is_never_overwritten() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    let out = scratch.path().join("bundle.zip");
    std::fs::write(&out, b"keep").expect("write existing file");
    let output = bundle_run(&env, &out);
    assert_eq!(output.exit_code, 2);
    assert_eq!(
        error_code(&output).as_deref(),
        Some("support-bundle.exists")
    );
    assert_eq!(std::fs::read(&out).expect("read target"), b"keep");
}

/// REQ: DEV-833
#[test]
fn dev_833_an_oversized_archive_is_trimmed_and_the_record_says_so() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    oversized_log(&env);
    let out = scratch.path().join("bundle.zip");
    let output = bundle_run(&env, &out);
    assert!(std::fs::metadata(&out).expect("bundle written").len() <= EIGHT_MIB);
    let records = records_of(&output, "support-bundle");
    assert_eq!(records[0]["trimmed"].as_bool(), Some(true));
}

/// REQ: USR-221
#[test]
fn usr_221_the_archive_holds_exactly_the_four_named_files() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    let out = scratch.path().join("bundle.zip");
    bundle_run(&env, &out);
    let names: Vec<String> = bundle_entries(&out).into_keys().collect();
    assert_eq!(
        names,
        ["crashes.txt", "log.txt", "stats.txt", "summary.txt"]
    );
}

/// REQ: USR-222
#[test]
fn usr_222_credentials_url_parameters_home_paths_and_payloads_are_redacted() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    let home = std::env::var("HOME").expect("HOME is set");
    write_log(
        &env,
        ID,
        "",
        &[
            log_line(
                "2030-01-01T12:00:00Z",
                "INFO",
                "server",
                ID,
                "Authorization: Bearer SECRET_TOKEN_ABC",
            ),
            log_line(
                "2030-01-01T12:00:01Z",
                "INFO",
                "server",
                ID,
                "GET https://api.example.test/v1?api_key=SECRET_KEY_XYZ&q=1",
            ),
            log_line(
                "2030-01-01T12:00:02Z",
                "INFO",
                "server",
                ID,
                &format!("opened {home}/dev/notes.md"),
            ),
            log_line(
                "2030-01-01T12:00:03Z",
                "INFO",
                "server",
                ID,
                "payload {\"body\":\"PAYLOAD_SENTINEL\"} 1024 bytes",
            ),
        ],
    );
    let out = scratch.path().join("bundle.zip");
    bundle_run(&env, &out);
    let text = bundle_entries(&out)
        .into_values()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!text.contains("SECRET_TOKEN_ABC"));
    assert!(!text.contains("SECRET_KEY_XYZ"));
    assert!(!text.contains("PAYLOAD_SENTINEL"));
    assert!(!text.contains(&home));
    assert!(text.contains("~/dev/notes.md"));
}

/// REQ: USR-223
#[test]
fn usr_223_the_plugin_data_folder_stays_out_of_the_archive() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    let data = env.data_dir.join(ID);
    std::fs::create_dir_all(&data).expect("create data folder");
    std::fs::write(data.join("DATA_SENTINEL.bin"), "DATA_SENTINEL").expect("write sentinel");
    let out = scratch.path().join("bundle.zip");
    bundle_run(&env, &out);
    let entries = bundle_entries(&out);
    assert!(entries.keys().all(|name| !name.contains("DATA_SENTINEL")));
    assert!(entries.values().all(|text| !text.contains("DATA_SENTINEL")));
}

/// REQ: USR-224
#[test]
fn usr_224_oldest_log_lines_go_first_and_the_drop_count_is_recorded() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    oversized_log(&env);
    let out = scratch.path().join("bundle.zip");
    bundle_run(&env, &out);
    assert!(std::fs::metadata(&out).expect("bundle written").len() <= EIGHT_MIB);
    let entries = bundle_entries(&out);
    assert!(!entries["log.txt"].contains("LINE-0000 "));
    assert!(entries["log.txt"].contains("LINE-0199 "));
    assert!(entries["summary.txt"].contains("dropped"));
}

/// REQ: USR-225
#[test]
fn usr_225_writing_a_bundle_changes_no_plugin_state() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    std::fs::create_dir_all(&env.state_dir).expect("create state folder");
    std::fs::write(env.state_dir.join("grants.json"), "{}").expect("write grant store");
    let before = tree_snapshot(&env.state_dir);
    let out = scratch.path().join("bundle.zip");
    bundle_run(&env, &out);
    assert_eq!(tree_snapshot(&env.state_dir), before);
}

/// REQ: USR-225
#[test]
fn usr_225_a_failed_write_leaves_no_file_at_the_target() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    let out = scratch.path().join("missing-folder").join("bundle.zip");
    let output = bundle_run(&env, &out);
    assert_ne!(output.exit_code, 0);
    assert!(!out.exists());
}

/// REQ: USR-229
#[test]
fn usr_229_crash_records_are_listed_in_the_bundle_crashes_file() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    let record = CrashRecord {
        time_utc: fixed_time(),
        plugin_id: ID.to_owned(),
        plugin_version: "0.1.0".to_owned(),
        side: Side::Server,
        reason: CrashReason::Killed,
        exit_status: Some(9),
        export_name: Some("on-events".to_owned()),
    };
    std::fs::create_dir_all(&env.state_dir).expect("create state folder");
    std::fs::write(
        records_path(&env.state_dir),
        format!("{}\n", format_record(&record)),
    )
    .expect("seed crash records");
    let out = scratch.path().join("bundle.zip");
    bundle_run(&env, &out);
    let entries = bundle_entries(&out);
    assert!(entries["crashes.txt"].contains(&format_record(&record)));
}

/// REQ: DEV-831
#[test]
fn dev_831_successful_write_reports_no_error_record() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    let out = scratch.path().join("bundle-no-error.zip");
    let output = bundle_run(&env, &out);
    assert!(error_code(&output).is_none());
    assert!(records_of(&output, "error").is_empty());
}

/// REQ: DEV-832
#[test]
fn dev_832_refused_overwrite_prints_no_support_bundle_record() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    let out = scratch.path().join("bundle-refused.zip");
    std::fs::write(&out, b"keep").expect("write existing file");
    let output = bundle_run(&env, &out);
    assert!(records_of(&output, "support-bundle").is_empty());
}

/// REQ: DEV-833
#[test]
fn dev_833_trimmed_archive_still_holds_the_four_named_files() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    oversized_log(&env);
    let out = scratch.path().join("bundle-trimmed.zip");
    bundle_run(&env, &out);
    let names: Vec<String> = bundle_entries(&out).into_keys().collect();
    assert_eq!(
        names,
        ["crashes.txt", "log.txt", "stats.txt", "summary.txt"]
    );
}

/// REQ: USR-221
#[test]
fn usr_221_two_bundles_hold_the_same_entry_names() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    let first = scratch.path().join("first.zip");
    let second = scratch.path().join("second.zip");
    bundle_run(&env, &first);
    bundle_run(&env, &second);
    let first_names: Vec<String> = bundle_entries(&first).into_keys().collect();
    let second_names: Vec<String> = bundle_entries(&second).into_keys().collect();
    assert_eq!(first_names, second_names);
}

/// REQ: USR-222
#[test]
fn usr_222_a_second_token_never_reaches_the_archive_or_stdout() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    write_log(
        &env,
        ID,
        "",
        &[log_line(
            "2030-01-01T12:00:00Z",
            "INFO",
            "server",
            ID,
            "Authorization: Bearer SECRET_TOKEN_XYZ",
        )],
    );
    let out = scratch.path().join("bundle-token.zip");
    let output = bundle_run(&env, &out);
    assert!(!output.stdout_raw.contains("SECRET_TOKEN_XYZ"));
    assert!(!bundle_entries(&out)["log.txt"].contains("SECRET_TOKEN_XYZ"));
}

/// REQ: USR-223
#[test]
fn usr_223_no_archive_entry_is_named_after_data_or_trace() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    let out = scratch.path().join("bundle-names.zip");
    bundle_run(&env, &out);
    assert!(bundle_entries(&out)
        .keys()
        .all(|name| !name.contains("data") && !name.contains("trace")));
}

/// REQ: USR-224
#[test]
fn usr_224_small_log_is_kept_whole_and_not_trimmed() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    write_log(
        &env,
        ID,
        "",
        &[log_line("2030-01-01T12:00:00Z", "INFO", "server", ID, "LINE-FIRST")],
    );
    let out = scratch.path().join("bundle-small.zip");
    let output = bundle_run(&env, &out);
    let records = records_of(&output, "support-bundle");
    assert_eq!(records[0]["trimmed"].as_bool(), Some(false));
    assert!(bundle_entries(&out)["log.txt"].contains("LINE-FIRST"));
}
