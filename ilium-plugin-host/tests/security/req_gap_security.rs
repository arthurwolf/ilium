//! Gap-review requirements of lane security: SEC-000 and SEC-015 .. SEC-139. Feeds and their
//! staleness clock, the release installer signature, sandbox limits of the helper, policy file
//! trust and locking, the audit chain and its sinks, key grabs and the command origin. Each
//! requirement has an `a` test (the specified behaviour) and a `b` test (the refusal or edge case).
//! Where the API the requirement names does not exist yet, the test calls the closest existing
//! entry point and asserts the required result, so it fails at runtime rather than at compile time.

use std::fs;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::{ErrorCode, Side, Tier};
use ilium_plugin_host::cli::{run_cli, CliEnv, CliRunOutput};
use ilium_plugin_host::testing::{FakeNet, FakeResponse, HarnessHandle, HostHarness, ImportResult, Isolation};
use serde_json::{json, Value};

use crate::support::*;

const MIB: u64 = 1024 * 1024;
const FEED_URL: &str = "https://feeds.ilium.example/revocations.json";
const CHANNELS_URL: &str = "https://feeds.ilium.example/channels.json";

/// The audit file of a state directory (`audit/audit.jsonl` below the state root).
fn audit_file(state: &Path) -> PathBuf {
    state.join("audit").join("audit.jsonl")
}

/// A CLI environment whose state directory is `state` itself.
fn env_over(state: &Path) -> CliEnv {
    CliEnv::new(
        state.join("data"),
        state.to_path_buf(),
        state.join("config"),
        state.join("cwd"),
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
        false,
        Arc::new(NoNetwork),
    )
}

/// Runs `ilium plugin <words>` on the state directory `state`.
fn cli(state: &Path, words: &[&str]) -> CliRunOutput {
    run_cli(&env_over(state), &args(words))
}

/// A host on `state` with the default T3 server setup and no extra options.
fn host_on(state: &Path) -> HarnessHandle {
    host_with(state, None)
}

/// A host on `state`, with one harness option when given.
fn host_with(state: &Path, option: Option<(&str, Value)>) -> HarnessHandle {
    let mut harness = HostHarness::new()
        .tier(Tier::T3)
        .side(Side::Server)
        .grants(&[])
        .state_dir(state.to_path_buf())
        .net(FakeNet::new())
        .with_native_guest(Box::new(QuietGuest));
    if let Some((key, value)) = option {
        harness = harness.option(key, value);
    }
    harness
        .build()
        .expect("the harness builds on the given state directory")
}

/// The audit records of one event name, in file order.
fn events_named(lines: &[Value], event: &str) -> Vec<Value> {
    lines
        .iter()
        .filter(|line| line["event"] == event)
        .cloned()
        .collect()
}

/// The rotated audit files `audit-<first seq>.jsonl` of a state directory, in name order.
fn rotated_files(state: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(state.join("audit"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    let name = path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or_default();
                    name.starts_with("audit-") && name.ends_with(".jsonl")
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// Fills the live audit file up to its size limit and appends one record, which rotates it.
fn force_rotation(host: &HarnessHandle) {
    let (limit_mib, _days) = host.audit_limits();
    host.audit_fill_to(limit_mib * MIB);
    host.audit_append("rotation-probe", "{}");
}

/// A revocation or blocklist feed document issued at `issued` (unix seconds), signed by the feed key.
fn signed_feed(issued: u64, entry: &str) -> Vec<u8> {
    json!({ "issued": issued, "entries": [entry], "signed_by": "feed-key" })
        .to_string()
        .into_bytes()
}

/// Asks the host to refresh the feed at `url` from the fake network.
fn refresh_feed(rig: &Rig, url: &str) -> ImportResult {
    rig.call("ilium:host/feeds.refresh", json!({ "url": url }))
}

/// A `channels.json` document issued at `issued`, naming `latest` and signed by `signed_by`.
fn channels_doc(issued: u64, latest: &str, signed_by: &str) -> Vec<u8> {
    json!({ "issued": issued, "latest": latest, "signed_by": signed_by })
        .to_string()
        .into_bytes()
}

/// Runs the release installer with `sh` in `dir`.
fn run_installer(dir: &Path, extra: &[&str]) -> std::process::Output {
    std::process::Command::new("sh")
        .arg(workspace_root().join("release").join("install.sh"))
        .args(extra)
        .current_dir(dir)
        .output()
        .expect("sh runs the release installer")
}

/// A release fixture: a `SHA256SUMS` file and a signature made with some other key.
fn release_fixture(dir: &Path, sums: &str) {
    fs::write(dir.join("SHA256SUMS"), sums).expect("write SHA256SUMS");
    fs::write(dir.join("SHA256SUMS.sig"), b"signed-with-another-key").expect("write the signature");
}

/// Sorted file names of a folder.
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read the folder")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The value of `/proc/<pid>/oom_score_adj`, trimmed; empty when unreadable.
fn oom_score(pid: u32) -> String {
    fs::read_to_string(format!("/proc/{pid}/oom_score_adj"))
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// The cgroup v2 directory of a process, below `/sys/fs/cgroup`.
fn cgroup_dir_of(pid: u32) -> Option<PathBuf> {
    let text = fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    let relative = text.lines().find_map(|line| line.strip_prefix("0::"))?;
    Some(Path::new("/sys/fs/cgroup").join(relative.trim_start_matches('/')))
}

/// Writes a forged host catalogue into the config and data folders of `state`.
fn plant_forged_catalogue(state: &Path, folders: &[&str]) {
    for folder in folders {
        let dir = state.join(folder);
        fs::create_dir_all(&dir).expect("create the folder");
        fs::write(
            dir.join("host-catalogue.json"),
            r#"{"clipboard.read":"Read anything, no prompt"}"#,
        )
        .expect("write the forged catalogue");
    }
}

/// Environment that points every per-user location and the trust store at a permissive copy.
fn permissive_env(state: &Path) -> CliEnv {
    fs::write(
        state.join("policy.toml"),
        "version = 1\n[grants]\nallow = [\"*\"]\n",
    )
    .expect("write the permissive policy");
    fs::write(state.join("trust.json"), "{\"keys\":[{\"id\":\"temp-key\"}]}")
        .expect("write the temporary trust store");
    let home = state.display().to_string();
    let trust = state.join("trust.json").display().to_string();
    env_over(state)
        .with_var("ILIUM_CONFIG_DIR", &home)
        .with_var("ILIUM_TRUST_STORE", &trust)
        .with_var("XDG_CONFIG_HOME", &home)
        .with_var("HOME", &home)
}

/// The text of a small valid policy file.
fn policy_text() -> &'static str {
    "version = 1\n[grants]\ndeny = [\"clipboard.read\"]\n"
}

/// The `policy` lines of `policy show`, without their file hash.
fn policy_lines(rig: &Rig) -> Vec<Value> {
    rig.policy_show()
        .into_iter()
        .filter(|line| line["type"] == "policy")
        .map(|mut line| {
            if let Some(object) = line.as_object_mut() {
                object.remove("sha256");
            }
            line
        })
        .collect()
}

/// Holds the audit writer lock of the state directory `state` until the returned file is dropped.
fn hold_audit_lock(state: &Path) -> fs::File {
    fs::create_dir_all(state.join("audit")).expect("create the audit folder");
    let holder = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join("audit").join("audit.lock"))
        .expect("open the audit lock file");
    holder.lock().expect("the test takes the audit lock");
    holder
}

/// A host whose `[audit] sink` is syslog.
fn sink_host(state: &Path) -> HarnessHandle {
    host_with(state, Some(("audit-sink", json!("syslog"))))
}

/// REQ: SEC-015
#[test]
fn req_sec_015_older_feed_refused_keeps_newer_entries_a() {
    let rig = t3(&[]);
    rig.net
        .serve(FEED_URL, FakeResponse::ok(signed_feed(1_800_000_200, "T2-entry")));
    let _ = refresh_feed(&rig, FEED_URL);
    rig.net
        .serve(FEED_URL, FakeResponse::ok(signed_feed(1_800_000_100, "T1-entry")));
    let _ = refresh_feed(&rig, FEED_URL);
    let refusals = rig.audit_events("signature-failure");
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    let detail = refusals[0]["detail"].to_string();
    assert!(
        detail.contains("1800000100") && detail.contains("1800000200"),
        "{detail}"
    );
}

/// REQ: SEC-015
#[test]
fn req_sec_015_refused_feed_does_not_reset_seven_day_clock_b() {
    let rig = t3(&[]);
    rig.net
        .serve(FEED_URL, FakeResponse::ok(signed_feed(1_800_000_200, "T2-entry")));
    let _ = refresh_feed(&rig, FEED_URL);
    rig.host.advance_clock(Duration::from_secs(6 * 24 * 3600));
    rig.net
        .serve(FEED_URL, FakeResponse::ok(signed_feed(1_800_000_100, "T1-entry")));
    let _ = refresh_feed(&rig, FEED_URL);
    rig.host.advance_clock(Duration::from_secs(2 * 24 * 3600));
    assert!(
        rig.notices()
            .iter()
            .any(|notice| notice.contains("Revocation data is stale")),
        "{:?}",
        rig.notices()
    );
}

/// REQ: SEC-016
#[test]
fn req_sec_016_wrong_release_key_stops_installer_before_unpacking_a() {
    let dir = scratch();
    release_fixture(dir.path(), "0000  example.tar.gz\n");
    let before = names_in(dir.path());
    let output = run_installer(dir.path(), &["--release-key", "release-archive-2026"]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("SHA256SUMS signature mismatch"), "{text}");
    assert_eq!(names_in(dir.path()), before);
}

/// REQ: SEC-016
#[test]
fn req_sec_016_signature_is_checked_before_the_hash_list_b() {
    let dir = scratch();
    release_fixture(dir.path(), "ffff  example.tar.gz\n");
    let output = run_installer(dir.path(), &["--release-key", "release-archive-2026"]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let signature_at = text
        .find("SHA256SUMS signature mismatch")
        .unwrap_or(usize::MAX);
    assert!(
        text.find("hash").map_or(true, |hash_at| signature_at < hash_at),
        "{text}"
    );
}

/// REQ: SEC-017
#[test]
fn req_sec_017_older_channels_document_refused_keeps_newer_release_a() {
    let rig = t3(&[]);
    rig.net.serve(
        CHANNELS_URL,
        FakeResponse::ok(channels_doc(1_800_000_200, "1.4.0", "release-archive")),
    );
    let _ = refresh_feed(&rig, CHANNELS_URL);
    rig.net.serve(
        CHANNELS_URL,
        FakeResponse::ok(channels_doc(1_800_000_100, "1.2.0", "release-archive")),
    );
    let _ = refresh_feed(&rig, CHANNELS_URL);
    let out = cli(rig.state.path(), &["version", "--json"]);
    assert!(out.stdout.contains("1.4.0"), "{}", out.stdout);
    assert!(!out.stdout.contains("1.2.0"), "{}", out.stdout);
    let logged = rig
        .log_messages()
        .iter()
        .filter(|line| line.contains("channels.json"))
        .count();
    assert_eq!(logged, 1, "{:?}", rig.log_messages());
}

/// REQ: SEC-017
#[test]
fn req_sec_017_channels_signed_with_blocklist_key_is_ignored_silently_b() {
    let rig = t3(&[]);
    rig.net.serve(
        CHANNELS_URL,
        FakeResponse::ok(channels_doc(1_800_000_000, "1.4.0", "blocklist-release")),
    );
    let _ = refresh_feed(&rig, CHANNELS_URL);
    let out = cli(rig.state.path(), &["version", "--json"]);
    assert!(!out.stdout.contains("update_available"), "{}", out.stdout);
    assert!(
        rig.log_messages()
            .iter()
            .any(|line| line.contains("channels.json")),
        "{:?}",
        rig.log_messages()
    );
}

/// REQ: SEC-018
#[test]
fn req_sec_018_helper_oom_score_is_500_and_server_unchanged_a() {
    let server_before = oom_score(std::process::id());
    let rig = t3(&[]);
    let helper_pid = rig.host.guest_process_id();
    assert_eq!(oom_score(helper_pid), "500");
    assert_eq!(oom_score(std::process::id()), server_before);
}

/// REQ: SEC-018
#[test]
fn req_sec_018_guest_cannot_lower_its_oom_score_b() {
    let rig = setup(&["process.run:sh"]).start();
    let helper_pid = rig.host.guest_process_id();
    let before = oom_score(helper_pid);
    let attempt = rig.call(
        "ilium:host/process.run",
        json!({ "program": "sh", "args": ["-c", "echo 0 > /proc/self/oom_score_adj"] }),
    );
    assert!(!is_ok(&attempt), "lowering oom_score_adj must fail: {attempt:?}");
    assert_eq!(oom_score(helper_pid), before);
    assert_eq!(before, "500");
}

/// REQ: SEC-019
#[test]
fn req_sec_019_helper_cgroup_swap_max_is_zero_a() {
    let rig = t3(&[]);
    let dir = cgroup_dir_of(rig.host.guest_process_id())
        .expect("the helper runs in a cgroup v2 leaf");
    let swap = fs::read_to_string(dir.join("memory.swap.max")).expect("memory.swap.max exists");
    assert_eq!(swap.trim(), "0");
}

/// REQ: SEC-019
#[test]
fn req_sec_019_missing_swap_controller_is_named_in_degraded_line_b() {
    let rig = setup(&[]).option("swap-controller", json!(false)).start();
    let degraded = rig.audit_events("sandbox-degraded");
    assert!(
        degraded
            .iter()
            .any(|record| record.to_string().contains("swap")),
        "{degraded:?}"
    );
}

/// REQ: SEC-068
#[test]
fn req_sec_068_planted_catalogues_are_ignored_wording_unchanged_a() {
    let clean = t3(&["clipboard.read"]);
    let expected = clean.host.review_text_for(&["clipboard.read"]);
    plant_forged_catalogue(clean.state.path(), &["config", "data"]);
    let rig = clean.restart();
    assert_eq!(rig.host.review_text_for(&["clipboard.read"]), expected);
}

/// REQ: SEC-068
#[test]
fn req_sec_068_planted_catalogue_in_data_folder_is_logged_once_b() {
    let clean = t3(&["clipboard.read"]);
    let expected = clean.host.review_text_for(&["clipboard.read"]);
    plant_forged_catalogue(clean.state.path(), &["data"]);
    let rig = clean.restart();
    assert_eq!(rig.host.review_text_for(&["clipboard.read"]), expected);
    let logged = rig
        .log_messages()
        .iter()
        .filter(|line| line.contains("host-catalogue"))
        .count();
    assert_eq!(logged, 1, "{:?}", rig.log_messages());
}

/// REQ: SEC-069
#[test]
fn req_sec_069_writable_system_policy_is_ignored_with_notice_a() {
    let rig = setup(&[]).system_policy("version = 1\n", false).start();
    let shown = rig.policy_show();
    assert!(
        shown.iter().any(|line| line.to_string().contains("ignored")),
        "{shown:?}"
    );
    let violations = rig.audit_events("policy-violation");
    assert!(
        violations
            .iter()
            .any(|record| record.to_string().contains("policy-file-writable")),
        "{violations:?}"
    );
    assert!(
        rig.notices().iter().any(|notice| notice.contains("policy")),
        "{:?}",
        rig.notices()
    );
}

/// REQ: SEC-069
#[test]
fn req_sec_069_symlinked_system_policy_is_ignored_b() {
    let dir = scratch();
    let real = dir.path().join("user-owned-policy.toml");
    fs::write(&real, "version = 1\n").expect("write the user file");
    let link = dir.path().join("system-policy.toml");
    symlink(&real, &link).expect("create the symbolic link");
    let rig = setup(&[])
        .option(
            "system-policy-path",
            json!(link.to_string_lossy().into_owned()),
        )
        .start();
    let shown = rig.policy_show();
    assert!(
        shown.iter().any(|line| line.to_string().contains("ignored")),
        "{shown:?}"
    );
    assert!(
        rig.audit_events("policy-violation")
            .iter()
            .any(|record| record.to_string().contains("policy-file-writable")),
    );
}

/// REQ: SEC-088
#[test]
fn req_sec_088_environment_does_not_move_system_policy_path_a() {
    let root = scratch();
    let env = permissive_env(root.path());
    let out = run_cli(&env, &args(&["plugin", "policy", "show"]));
    assert!(
        out.stdout.contains("/etc/ilium/policy.toml"),
        "{}",
        out.stdout
    );
    assert!(!out.stdout.contains("\"*\""), "{}", out.stdout);
}

/// REQ: SEC-088
#[test]
fn req_sec_088_key_only_in_temporary_trust_store_is_not_trusted_b() {
    let root = scratch();
    let env = permissive_env(root.path());
    let out = run_cli(&env, &args(&["plugin", "policy", "show"]));
    assert!(!out.stdout.contains("temp-key"), "{}", out.stdout);
}

/// REQ: SEC-089
#[test]
fn req_sec_089_policy_loaded_sources_match_policy_show_a() {
    let rig = setup(&[])
        .system_policy("version = 1\n", true)
        .user_policy("version = 1\n")
        .start();
    let loaded = rig.audit_events("policy-loaded");
    let first = loaded.first().expect("one policy-loaded record");
    let sources = first["sources"].as_array().expect("sources is an array");
    let shown: Vec<Value> = rig
        .policy_show()
        .into_iter()
        .filter(|line| line["type"] == "policy-source")
        .collect();
    assert_eq!(sources.len(), shown.len(), "{sources:?} vs {shown:?}");
    for (source, line) in sources.iter().zip(shown.iter()) {
        assert_eq!(source["path"], line["path"]);
        assert_eq!(source["sha256"], line["sha256"]);
    }
    assert!(first["effective_sha256"].is_string(), "{first}");
}

/// REQ: SEC-089
#[test]
fn req_sec_089_unused_source_has_used_false_and_reason_b() {
    let rig = setup(&[])
        .system_policy("version = 1\n", false)
        .user_policy("version = 1\n")
        .start();
    let loaded = rig.audit_events("policy-loaded");
    let first = loaded.first().expect("one policy-loaded record");
    let sources = first["sources"].as_array().expect("sources is an array");
    let unused = sources
        .iter()
        .find(|source| source["used"] == json!(false))
        .expect("the writable system file is listed as not used");
    assert!(unused["reason"].is_string(), "{unused}");
}

/// REQ: SEC-090
#[test]
fn req_sec_090_locked_sync_records_bundle_source_and_lockfile_hash_a() {
    let root = scratch();
    let _ = cli(
        root.path(),
        &["plugin", "sync", "--locked", "--from-bundle", "missing.bundle"],
    );
    let lines = read_jsonl(&audit_file(root.path()));
    let installs = events_named(&lines, "install");
    assert!(!installs.is_empty(), "{lines:?}");
    for install in &installs {
        assert!(
            install["source"]
                .as_str()
                .unwrap_or_default()
                .starts_with("bundle:"),
            "{install}"
        );
        assert!(install["lockfile_sha256"].is_string(), "{install}");
    }
    let syncs = events_named(&lines, "sync");
    assert_eq!(syncs.len(), 1, "{lines:?}");
}

/// REQ: SEC-090
#[test]
fn req_sec_090_update_without_lockfile_has_no_lockfile_hash_b() {
    let root = scratch();
    let _ = cli(root.path(), &["plugin", "update", "example.demo"]);
    let lines = read_jsonl(&audit_file(root.path()));
    let updates = events_named(&lines, "update");
    let update = updates.first().expect("an update record");
    assert!(update["lockfile_sha256"].is_null(), "{update}");
}

/// REQ: SEC-091
#[test]
fn req_sec_091_policy_show_exits_zero_when_system_policy_is_valid_a() {
    let root = scratch();
    let out = cli(root.path(), &["plugin", "policy", "show"]);
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
}

/// REQ: SEC-091
#[test]
fn req_sec_091_malformed_policy_exits_one_with_line_number_b() {
    let root = scratch();
    let bad = root.path().join("bad-policy.toml");
    fs::write(&bad, "version = 1\n[grants\n").expect("write the malformed policy");
    let env = env_over(root.path()).with_var(
        "ILIUM_SYSTEM_POLICY_FILE",
        bad.to_str().expect("a UTF-8 scratch path"),
    );
    let out = run_cli(&env, &args(&["plugin", "policy", "show"]));
    assert_eq!(out.exit_code, 1, "{}", out.stdout);
    assert!(out.stdout.contains("line 2"), "{}", out.stdout);
}

/// REQ: SEC-092
#[test]
fn req_sec_092_lf_crlf_and_bom_policies_give_identical_policy_lines_a() {
    let lf = setup(&[]).system_policy(policy_text(), true).start();
    let crlf = setup(&[])
        .system_policy(&policy_text().replace('\n', "\r\n"), true)
        .start();
    let bom = setup(&[])
        .system_policy(
            &format!("\u{feff}{}", policy_text().replace('\n', "\r\n")),
            true,
        )
        .start();
    let reference = policy_lines(&lf);
    assert!(!reference.is_empty(), "{:?}", lf.policy_show());
    assert_eq!(policy_lines(&crlf), reference);
    assert_eq!(policy_lines(&bom), reference);
}

/// REQ: SEC-092
#[test]
fn req_sec_092_utf16_policy_is_policy_invalid_b() {
    let utf16 = "version = 1\n"
        .chars()
        .flat_map(|character| [character, '\0'])
        .collect::<String>();
    let rig = setup(&[]).system_policy(&utf16, true).start();
    assert!(
        rig.policy_show()
            .iter()
            .any(|line| line.to_string().contains("policy.invalid")),
        "{:?}",
        rig.policy_show()
    );
}

/// REQ: SEC-093
#[test]
fn req_sec_093_locked_registry_setting_refuses_user_change_a() {
    let rig = setup(&[])
        .system_policy(
            "version = 1\n[settings.locked]\n\"plugins.registry.urls\" = true\n\"plugins.registry.root\" = true\n",
            true,
        )
        .start();
    let result = rig
        .host
        .user_set_setting("plugins.registry.urls", json!(["https://registry.example"]));
    assert!(
        matches!(&result, Err(error) if error.message.contains("settings.locked")),
        "{result:?}"
    );
}

/// REQ: SEC-093
#[test]
fn req_sec_093_unlocked_offline_setting_can_still_be_changed_b() {
    let rig = setup(&[])
        .system_policy(
            "version = 1\n[settings.locked]\n\"plugins.registry.urls\" = true\n\"plugins.registry.root\" = true\n",
            true,
        )
        .start();
    let result = rig
        .host
        .user_set_setting("plugins.registry.offline", json!(true));
    assert!(result.is_ok(), "{result:?}");
}

/// REQ: SEC-094
#[test]
fn req_sec_094_grant_record_exists_when_command_exits_a() {
    let root = scratch();
    let out = cli(
        root.path(),
        &["plugin", "grant", "test.plugin", "clipboard.read"],
    );
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    let lines = read_jsonl(&audit_file(root.path()));
    assert_eq!(events_named(&lines, "grant-given").len(), 1, "{lines:?}");
}

/// REQ: SEC-094
#[test]
fn req_sec_094_each_state_change_is_audited_before_its_command_exits_b() {
    let root = scratch();
    let steps: Vec<(Vec<&str>, &str)> = vec![
        (vec!["plugin", "install", "test.plugin"], "install"),
        (vec!["plugin", "enable", "test.plugin"], "enable"),
        (vec!["plugin", "disable", "test.plugin"], "disable"),
        (vec!["plugin", "consent", "test.plugin"], "consent-given"),
    ];
    for (words, event) in steps {
        let out = cli(root.path(), &words);
        let lines = read_jsonl(&audit_file(root.path()));
        assert!(
            !events_named(&lines, event).is_empty(),
            "{event} exited {} with {}",
            out.exit_code,
            out.stderr
        );
    }
}

/// REQ: SEC-095
#[test]
fn req_sec_095_symlinked_audit_file_is_not_written_through_a_notice_names_it_a() {
    let state = scratch();
    let elsewhere = scratch();
    let target = elsewhere.path().join("elsewhere.txt");
    fs::write(&target, "before").expect("write the scratch file");
    fs::create_dir_all(state.path().join("audit")).expect("create the audit folder");
    let audit = audit_file(state.path());
    symlink(&target, &audit).expect("replace audit.jsonl with a link");
    let host = host_on(state.path());
    host.audit_append("grant-given", "{}");
    assert_eq!(fs::read_to_string(&target).unwrap_or_default(), "before");
    let logged = host.logs();
    assert!(
        logged
            .iter()
            .any(|line| line.message.contains(&*audit.to_string_lossy())),
        "{logged:?}"
    );
    fs::remove_file(&audit).expect("remove the link");
    host.audit_append("grant-given", "{}");
    drop(host);
    let lines = read_jsonl(&audit);
    assert!(!events_named(&lines, "grant-given").is_empty(), "{lines:?}");
}

/// REQ: SEC-095
#[test]
fn req_sec_095_symlinked_crashes_folder_gets_no_writes_b() {
    let state = scratch();
    let elsewhere = scratch();
    symlink(elsewhere.path(), state.path().join("crashes")).expect("link the crashes folder");
    let host = host_on(state.path());
    host.audit_append("crash-report", "{}");
    let written = fs::read_dir(elsewhere.path())
        .expect("read the link target")
        .count();
    assert_eq!(written, 0);
}

/// REQ: SEC-096
#[test]
fn req_sec_096_manual_catalogue_lists_kernel_events_a() {
    assert_page_has(
        "reference-audit-log.md",
        &[
            "plugin-state-changed",
            "crash-report",
            "audit-torn",
            "audit-retention-deleted",
        ],
    );
}

/// REQ: SEC-096
#[test]
fn req_sec_096_every_written_event_is_catalogued_b() {
    let state = scratch();
    let host = host_on(state.path());
    host.audit_append("plugin-state-changed", "{}");
    drop(host);
    let page = manual_page("reference-audit-log.md");
    let lines = read_jsonl(&audit_file(state.path()));
    assert!(!lines.is_empty(), "the kernel wrote no record");
    for line in &lines {
        let event = line["event"].as_str().unwrap_or_default();
        assert!(page.contains(&format!("`{event}`")), "{event} is not catalogued");
    }
}

/// REQ: SEC-125
#[test]
fn req_sec_125_verify_reports_missing_middle_rotation_with_exit_three_a() {
    let state = scratch();
    let host = host_on(state.path());
    force_rotation(&host);
    force_rotation(&host);
    force_rotation(&host);
    drop(host);
    let files = rotated_files(state.path());
    assert!(files.len() >= 3, "{files:?}");
    fs::remove_file(&files[1]).expect("delete the middle rotation");
    let out = cli(state.path(), &["plugin", "audit", "--verify"]);
    assert_eq!(out.exit_code, 3, "{}", out.stdout);
    assert!(out.stdout.contains("missing-file"), "{}", out.stdout);
}

/// REQ: SEC-125
#[test]
fn req_sec_125_recreated_live_file_is_reported_as_reset_not_ok_b() {
    let state = scratch();
    let host = host_on(state.path());
    host.audit_append("grant-given", "{}");
    drop(host);
    fs::remove_file(audit_file(state.path())).expect("remove the live file");
    let host = host_on(state.path());
    host.audit_append("grant-given", "{}");
    drop(host);
    let out = cli(state.path(), &["plugin", "audit", "--verify"]);
    assert!(out.stdout.contains("reset-at"), "{}", out.stdout);
    assert!(
        !out.stdout.lines().any(|line| line.trim() == "ok"),
        "{}",
        out.stdout
    );
}

/// REQ: SEC-126
#[test]
fn req_sec_126_failed_syslog_forward_keeps_every_record_and_logs_once_a() {
    let state = scratch();
    let host = sink_host(state.path());
    for number in 0..50 {
        host.audit_append("grant-given", &format!("{{\"n\":{number}}}"));
    }
    let lines = read_jsonl(&audit_file(state.path()));
    assert_eq!(events_named(&lines, "grant-given").len(), 50);
    let warnings = host
        .system_log_messages()
        .iter()
        .filter(|message| message.text.contains("host.audit.sink-failed"))
        .count();
    assert_eq!(warnings, 1);
}

/// REQ: SEC-126
#[test]
fn req_sec_126_audited_actions_do_not_wait_for_the_failing_sink_b() {
    let state = scratch();
    let host = sink_host(state.path());
    let started = Instant::now();
    for number in 0..50 {
        host.audit_append("grant-given", &format!("{{\"n\":{number}}}"));
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "50 records took {:?}",
        started.elapsed()
    );
}

/// REQ: SEC-127
#[test]
fn req_sec_127_append_does_not_block_while_another_server_holds_the_lock_a() {
    let state = scratch();
    let host = host_on(state.path());
    let holder = hold_audit_lock(state.path());
    let started = Instant::now();
    host.audit_append("grant-given", "{}");
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "the append blocked for {:?}",
        started.elapsed()
    );
    assert!(host.audit_queue_len() >= 1, "the record must be queued");
    drop(holder);
}

/// REQ: SEC-127
#[test]
fn req_sec_127_queued_record_is_written_once_the_lock_is_released_b() {
    let state = scratch();
    let host = host_on(state.path());
    let holder = hold_audit_lock(state.path());
    host.audit_append("grant-given", "{}");
    drop(holder);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && host.audit_queue_len() > 0 {
        std::thread::sleep(Duration::from_millis(100));
    }
    host.flush_audit_summaries();
    assert_eq!(host.audit_queue_len(), 0);
    let lines = read_jsonl(&audit_file(state.path()));
    assert_eq!(events_named(&lines, "grant-given").len(), 1, "{lines:?}");
}

/// REQ: SEC-128
#[test]
fn req_sec_128_rotated_audit_file_is_mode_0400_a() {
    let state = scratch();
    let host = host_on(state.path());
    force_rotation(&host);
    drop(host);
    let files = rotated_files(state.path());
    let rotated = files.first().expect("a rotated file exists");
    let mode = fs::metadata(rotated)
        .expect("stat the rotated file")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o400);
}

/// REQ: SEC-128
#[test]
fn req_sec_128_rotated_file_is_never_appended_or_rewritten_b() {
    let state = scratch();
    let host = host_on(state.path());
    force_rotation(&host);
    drop(host);
    let rotated = rotated_files(state.path())
        .first()
        .cloned()
        .expect("a rotated file exists");
    let before_meta = fs::metadata(&rotated).expect("stat before");
    let before = (
        before_meta.len(),
        before_meta.ino(),
        sha256_hex(&fs::read(&rotated).expect("read before")),
    );
    let host = host_on(state.path());
    for number in 0..100 {
        host.audit_append("grant-given", &format!("{{\"n\":{number}}}"));
    }
    drop(host);
    let after_meta = fs::metadata(&rotated).expect("the rotated file is still there");
    let after = (
        after_meta.len(),
        after_meta.ino(),
        sha256_hex(&fs::read(&rotated).expect("read after")),
    );
    assert_eq!(after, before);
}

/// REQ: SEC-129
#[test]
fn req_sec_129_update_and_downgrade_carry_previous_version_a() {
    let root = scratch();
    let _ = cli(
        root.path(),
        &["plugin", "install", "example.demo", "--version", "1.0.0"],
    );
    let _ = cli(
        root.path(),
        &["plugin", "update", "example.demo", "--to", "1.1.0"],
    );
    let _ = cli(
        root.path(),
        &["plugin", "downgrade", "example.demo", "--to", "1.0.0"],
    );
    let lines = read_jsonl(&audit_file(root.path()));
    let mut changes = events_named(&lines, "update");
    changes.extend(events_named(&lines, "downgrade"));
    let update = changes
        .iter()
        .find(|record| record["version"] == "1.1.0")
        .expect("an update record for 1.1.0");
    assert!(update["detail"].to_string().contains("from=1.0.0"), "{update}");
    let downgrade = changes
        .iter()
        .find(|record| record["version"] == "1.0.0")
        .expect("a downgrade record for 1.0.0");
    assert!(downgrade["detail"].to_string().contains("from=1.1.0"), "{downgrade}");
}

/// REQ: SEC-129
#[test]
fn req_sec_129_refused_update_records_failed_result_with_previous_version_b() {
    let root = scratch();
    let _ = cli(
        root.path(),
        &["plugin", "install", "example.demo", "--version", "1.0.0"],
    );
    let _ = cli(
        root.path(),
        &["plugin", "update", "example.demo", "--to", "9.9.9"],
    );
    let lines = read_jsonl(&audit_file(root.path()));
    let failed = events_named(&lines, "update")
        .into_iter()
        .find(|record| record["result"] == "failed")
        .expect("a failed update record");
    assert!(failed["detail"].to_string().contains("from=1.0.0"), "{failed}");
}

/// REQ: SEC-130
#[test]
fn req_sec_130_policy_loaded_detail_has_path_hash_and_used_a() {
    let rig = setup(&[]).system_policy("version = 1\n", true).start();
    let loaded = rig.audit_events("policy-loaded");
    let first = loaded.first().expect("one policy-loaded record");
    let detail = first["detail"].as_str().unwrap_or_default();
    assert!(detail.contains("path="), "{detail}");
    assert!(detail.contains("source=used"), "{detail}");
    assert!(
        detail.contains(&sha256_hex("version = 1\n".as_bytes())),
        "{detail}"
    );
}

/// REQ: SEC-130
#[test]
fn req_sec_130_policy_loaded_detail_holds_no_policy_values_b() {
    let rig = setup(&[])
        .system_policy("version = 1\n[grants]\ndeny = [\"marker-grant-xyz\"]\n", true)
        .start();
    let loaded = rig.audit_events("policy-loaded");
    let first = loaded.first().expect("one policy-loaded record");
    assert!(
        !first.to_string().contains("marker-grant-xyz"),
        "{first}"
    );
}

/// REQ: SEC-131
#[test]
fn req_sec_131_orderly_restart_records_clean_previous_run_a() {
    let rig = t3(&[]);
    let rig = rig.restart();
    let stops = rig.audit_events("server-stop");
    assert!(
        stops
            .iter()
            .any(|record| record.to_string().contains("shutdown")),
        "{stops:?}"
    );
    let starts = rig.audit_events("server-start");
    let last = starts.last().expect("a server-start record");
    assert!(last.to_string().contains("prev=clean"), "{last}");
}

/// REQ: SEC-131
#[test]
fn req_sec_131_start_after_missing_server_stop_records_unclean_b() {
    let state = scratch();
    let host = host_on(state.path());
    drop(host);
    let path = audit_file(state.path());
    let kept: String = fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.contains("\"server-stop\""))
        .map(|line| format!("{line}\n"))
        .collect();
    fs::write(&path, kept).expect("simulate a kill before server-stop");
    let _host = host_on(state.path());
    let starts = events_named(&read_jsonl(&path), "server-start");
    let last = starts.last().expect("a server-start record");
    assert!(last.to_string().contains("prev=unclean"), "{last}");
}

/// REQ: SEC-132
#[test]
fn req_sec_132_bundle_record_carries_scope_and_hash_a() {
    let root = scratch();
    let bundle = root.path().join("example-demo.zip");
    let bundle_text = bundle.to_str().expect("a UTF-8 scratch path");
    let out = cli(
        root.path(),
        &["plugin", "support-bundle", "example.demo", "--output", bundle_text],
    );
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    let bytes = fs::read(&bundle).expect("the bundle is written");
    let records = events_named(
        &read_jsonl(&audit_file(root.path())),
        "support-bundle",
    );
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0]["target"], json!(bundle_text));
    assert!(
        records[0]["detail"]
            .to_string()
            .contains(&format!("sha256={}", sha256_hex(&bytes))),
        "{}",
        records[0]
    );
}

/// REQ: SEC-132
#[test]
fn req_sec_132_dry_run_writes_no_support_bundle_record_b() {
    let root = scratch();
    let before = events_named(
        &read_jsonl(&audit_file(root.path())),
        "support-bundle",
    )
    .len();
    let _ = cli(
        root.path(),
        &["plugin", "support-bundle", "example.demo", "--dry-run"],
    );
    let after = events_named(
        &read_jsonl(&audit_file(root.path())),
        "support-bundle",
    )
    .len();
    assert_eq!(after, before);
}

/// REQ: SEC-133
#[test]
fn req_sec_133_each_diagnostics_switch_writes_one_capture_record_a() {
    let root = scratch();
    let _ = cli(root.path(), &["plugin", "diagnostics", "file-log", "on"]);
    let _ = cli(root.path(), &["plugin", "diagnostics", "file-log", "off"]);
    let _ = run_cli(
        &env_over(root.path()).with_var("ILIUM_PLUGIN_TRACE", "1"),
        &args(&["plugin", "list"]),
    );
    let _ = cli(root.path(), &["plugin", "log-level", "example.demo", "debug"]);
    let records = events_named(
        &read_jsonl(&audit_file(root.path())),
        "diagnostics-capture",
    );
    let seen: Vec<(String, String)> = records
        .iter()
        .map(|record| {
            (
                record["target"].as_str().unwrap_or_default().to_owned(),
                record["reason"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    for (target, reason) in [
        ("file-log", "on"),
        ("file-log", "off"),
        ("trace", "on"),
        ("log-level:example.demo", "on"),
    ] {
        assert!(
            seen.iter()
                .any(|(seen_target, seen_reason)| seen_target.as_str() == target
                    && seen_reason.as_str() == reason),
            "{target} {reason} missing from {seen:?}"
        );
    }
}

/// REQ: SEC-133
#[test]
fn req_sec_133_capture_records_name_their_actor_b() {
    let root = scratch();
    let _ = cli(root.path(), &["plugin", "diagnostics", "file-log", "on"]);
    let records = events_named(
        &read_jsonl(&audit_file(root.path())),
        "diagnostics-capture",
    );
    assert!(!records.is_empty(), "no diagnostics-capture record");
    assert!(records.iter().all(|record| !record["actor"].is_null()), "{records:?}");
}

/// REQ: SEC-134
#[test]
fn req_sec_134_every_written_record_has_schema_one_a() {
    let state = scratch();
    let host = host_on(state.path());
    host.audit_append("grant-given", "{}");
    drop(host);
    let lines = read_jsonl(&audit_file(state.path()));
    assert!(!lines.is_empty(), "the kernel wrote no record");
    for line in &lines {
        assert_eq!(line["schema"], json!(1), "{line}");
    }
}

/// REQ: SEC-134
#[test]
fn req_sec_134_unknown_schema_is_reported_with_exit_three_b() {
    let state = scratch();
    let host = host_on(state.path());
    host.audit_append("grant-given", "{}");
    drop(host);
    let path = audit_file(state.path());
    let mut text = fs::read_to_string(&path).unwrap_or_default();
    text.push_str("{\"seq\":999,\"schema\":99,\"event\":\"future-event\"}\n");
    fs::write(&path, text).expect("append a record with schema 99");
    let out = cli(state.path(), &["plugin", "audit", "--verify"]);
    assert_eq!(out.exit_code, 3, "{}", out.stdout);
    assert!(out.stdout.contains("unknown-schema 999"), "{}", out.stdout);
}

/// REQ: SEC-135
#[test]
fn req_sec_135_no_key_events_reach_grab_while_echo_is_off_a() {
    let rig = t3(&["keys.intercept"]);
    rig.host.set_pane_echo(false);
    let granted = rig.call(
        "ilium:host/keys.intercept",
        json!({ "chords": ["a", "b", "enter"], "pane": "none" }),
    );
    assert!(is_ok(&granted), "{granted:?}");
    let _ = rig.host.pump(10);
    let delivered = rig
        .host
        .kernel()
        .take_events()
        .into_iter()
        .filter(|event| matches!(event, Event::KeyPressed(_)))
        .count();
    assert_eq!(delivered, 0);
}

/// REQ: SEC-135
#[test]
fn req_sec_135_grab_survives_echo_toggling_b() {
    let rig = t3(&["keys.intercept"]);
    let granted = rig.call(
        "ilium:host/keys.intercept",
        json!({ "chords": ["a", "b", "enter"], "pane": "none" }),
    );
    assert!(is_ok(&granted), "{granted:?}");
    rig.host.set_pane_echo(false);
    rig.host.set_pane_echo(true);
    assert!(!rig.host.kernel().active_grabs().is_empty());
}

/// REQ: SEC-136
#[test]
fn req_sec_136_global_grab_writes_start_and_end_records_a() {
    let rig = t3(&["keys.intercept"]);
    let granted = rig.call(
        "ilium:host/keys.intercept",
        json!({ "chords": ["ctrl+g"], "pane": "none" }),
    );
    assert!(is_ok(&granted), "{granted:?}");
    let _ = rig.call(
        "ilium:host/keys.release",
        json!({ "chords": ["ctrl+g"], "pane": "none" }),
    );
    let starts = rig.audit_events("key-grab-start");
    let ends = rig.audit_events("key-grab-end");
    assert_eq!(starts.len(), 1, "{starts:?}");
    assert_eq!(ends.len(), 1, "{ends:?}");
    assert!(starts[0].to_string().contains("ctrl+g"), "{}", starts[0]);
}

/// REQ: SEC-136
#[test]
fn req_sec_136_pane_scoped_grab_writes_no_grab_record_b() {
    let rig = t3(&["keys.intercept"]);
    let granted = rig.call(
        "ilium:host/keys.intercept",
        json!({ "chords": ["ctrl+g"], "pane": "pane-1" }),
    );
    assert!(is_ok(&granted), "{granted:?}");
    assert!(rig.audit_events("key-grab-start").is_empty());
}

/// REQ: SEC-137
#[test]
fn req_sec_137_command_origin_enum_has_plugin_case_a() {
    let source = fs::read_to_string(
        workspace_root().join("ilium-plugin-api/src/generated/types.rs"),
    )
    .expect("the generated types exist");
    let start = source
        .find("pub enum CommandOrigin")
        .expect("CommandOrigin is generated");
    let block = &source[start..];
    let end = block.find('}').unwrap_or(block.len());
    assert!(
        block[..end].lines().any(|line| line.trim() == "Plugin,"),
        "CommandOrigin has no Plugin case"
    );
}

/// REQ: SEC-137
#[test]
fn req_sec_137_palette_invocation_has_no_invoker_b() {
    let rig = t3(&["commands.invoke:*"]);
    let _ = rig.call(
        "ilium:host/commands.invoke",
        json!({ "command": "example.demo/save", "origin": "palette" }),
    );
    let invoked = rig.audit_events("command-invoked");
    assert!(!invoked.is_empty(), "no command-invoked record");
    assert!(
        invoked
            .iter()
            .all(|record| !record.to_string().contains("invoker")),
        "{invoked:?}"
    );
}

/// REQ: SEC-138
#[test]
fn req_sec_138_plugin_origin_core_command_refused_with_tier_a() {
    let rig = setup(&["commands.invoke:*"]).start();
    let result = rig.value(
        "ilium:host/commands.invoke",
        json!({ "command": "settings.disable-clipboard", "origin": "plugin" }),
    );
    assert_refused(&result, ErrorCode::NotPermitted, "tier");
}

/// REQ: SEC-138
#[test]
fn req_sec_138_refused_core_command_leaves_target_plugin_enabled_b() {
    let rig = setup(&["commands.invoke:*"]).start();
    let _ = rig.value(
        "ilium:host/commands.invoke",
        json!({ "command": "settings.disable-clipboard", "origin": "plugin" }),
    );
    assert!(
        !rig.host.plugin_disabled("clipboard"),
        "the target plugin must stay enabled"
    );
}

/// REQ: SEC-139
#[test]
fn req_sec_139_decoder_is_confined_and_listed_as_fuzz_target_a() {
    let rig = t3(&[]);
    assert!(
        matches!(rig.host.isolation(), Isolation::HelperProcess),
        "the decoder must run in a confined helper"
    );
    let page = manual_page("reference-security-testing.md");
    assert!(page.contains("decoder"), "the fuzz target list names the decoder");
}

/// REQ: SEC-139
#[test]
fn req_sec_139_crashing_decoder_fails_only_the_open_call_b() {
    let rig = t3(&[]);
    let dir = scratch();
    let crash = dir.path().join("crash.mp4");
    fs::write(&crash, b"CRASH-DECODER-MARKER").expect("write the crafted file");
    let result = rig.value(
        "ilium:host/media.open-video",
        json!({ "path": crash.to_string_lossy().into_owned() }),
    );
    assert!(
        matches!(&result, Err(error) if error.message.contains("failed(decoder)")),
        "{result:?}"
    );
}

/// REQ: SEC-000
#[test]
fn req_sec_000_kept_data_is_read_back_for_the_same_publisher_key_a() {
    let rig = setup(&[]).start();
    let first = rig.host.install_check_signed_by("test.plugin", "key-a");
    assert!(first.is_ok(), "{first:?}");
    rig.host.write_plugin_data("test.plugin", b"kept-data");
    let removed = cli(rig.state.path(), &["plugin", "uninstall", "test.plugin"]);
    assert_eq!(removed.exit_code, 0, "{}", removed.stderr);
    let owner = fs::read_to_string(
        rig.state
            .path()
            .join("storage")
            .join("test.plugin")
            .join("owner.json"),
    )
    .unwrap_or_default();
    assert!(owner.contains("key-a"), "{owner}");
    let reinstall = rig.host.install_check_signed_by("test.plugin", "key-a");
    assert!(reinstall.is_ok(), "{reinstall:?}");
    assert_eq!(
        rig.host.read_plugin_data("test.plugin"),
        Some(b"kept-data".to_vec())
    );
}

/// REQ: SEC-000
#[test]
fn req_sec_000_unrelated_key_is_refused_and_data_untouched_b() {
    let rig = setup(&[]).start();
    let _ = rig.host.install_check_signed_by("test.plugin", "key-a");
    rig.host.write_plugin_data("test.plugin", b"kept-data");
    let _ = cli(rig.state.path(), &["plugin", "uninstall", "test.plugin"]);
    let reinstall = rig.host.install_check_signed_by("test.plugin", "key-b");
    assert!(
        matches!(&reinstall, Err(message) if message.contains("package.publisher-key-changed")),
        "{reinstall:?}"
    );
    assert_eq!(
        rig.host.read_plugin_data("test.plugin"),
        Some(b"kept-data".to_vec())
    );
    let unsigned = rig.host.install_check("test.plugin", Tier::T3);
    assert!(
        matches!(&unsigned, Err(message) if message.contains("package.publisher-key-changed")),
        "{unsigned:?}"
    );
}
