//! Guide, data migration (guide-data-migration.md): LIF-100..LIF-116.

use std::collections::BTreeMap;

use ilium_plugin_api::manifest::parse_manifest;
use ilium_plugin_api::ErrorCode;
use proptest::prelude::*;

use super::common::*;

// ---------------------------------------------------------------------------------------------
// LIF-100: the manifest key data_version
// ---------------------------------------------------------------------------------------------

/// REQ: LIF-100
#[test]
fn lif_100_absent_data_version_defaults_to_one() {
    let manifest =
        parse_manifest(&manifest_text(None)).expect("a manifest without data_version parses");
    assert_eq!(manifest.data_version, 1);
}

/// REQ: LIF-100
#[test]
fn lif_100_data_version_one_is_accepted() {
    assert!(!is_refused_for_data_version("1"));
    let manifest = parse_manifest(&manifest_text(Some("1"))).expect("data_version 1 parses");
    assert_eq!(manifest.data_version, 1);
}

/// REQ: LIF-100
#[test]
fn lif_100_data_version_u32_maximum_is_accepted() {
    assert!(!is_refused_for_data_version("4294967295"));
    let manifest = parse_manifest(&manifest_text(Some("4294967295"))).expect("u32::MAX parses");
    assert_eq!(manifest.data_version, u32::MAX);
}

/// REQ: LIF-100
#[test]
fn lif_100_data_version_two_is_accepted() {
    assert!(!is_refused_for_data_version("2"));
}

/// REQ: LIF-100
#[test]
fn lif_100_data_version_zero_is_refused_manifest_data_version() {
    assert!(is_refused_for_data_version("0"));
}

/// REQ: LIF-100
#[test]
fn lif_100_data_version_string_is_refused_manifest_data_version() {
    assert!(is_refused_for_data_version("\"2\""));
}

/// REQ: LIF-100
#[test]
fn lif_100_data_version_above_u32_is_refused_manifest_data_version() {
    assert!(is_refused_for_data_version("4294967296"));
}

/// REQ: LIF-100
#[test]
fn lif_100_data_version_negative_is_refused_manifest_data_version() {
    assert!(is_refused_for_data_version("-1"));
}

/// REQ: LIF-100
#[test]
fn lif_100_data_version_fraction_is_refused_manifest_data_version() {
    assert!(is_refused_for_data_version("1.5"));
}

/// REQ: LIF-100
#[test]
fn lif_100_data_version_boolean_is_refused_manifest_data_version() {
    assert!(is_refused_for_data_version("true"));
}

/// REQ: LIF-100
#[test]
fn lif_100_data_version_refusal_names_pointer_and_value() {
    let found = diagnostics_of(&manifest_text(Some("0")));
    let diagnostic = found
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == "manifest.data-version")
        .expect("manifest.data-version reported");
    assert_eq!(diagnostic.pointer, "/data_version");
    assert_eq!(
        diagnostic.message,
        "data_version must be an integer from 1 to 4294967295. Found 0."
    );
}

/// REQ: LIF-100
#[test]
fn lif_100_install_with_zero_data_version_exits_one_manifest_data_version() {
    let rig = rig();
    let output = rig.install(&notes_v1().data_version_json("0"));
    assert_eq!(output.exit_code, 1);
    assert!(error_codes(&output).contains(&"manifest.data-version".to_owned()));
}

/// REQ: LIF-100
#[test]
fn lif_100_install_with_string_data_version_exits_one_manifest_data_version() {
    let rig = rig();
    let output = rig.install(&notes_v1().data_version_json("\"2\""));
    assert_eq!(output.exit_code, 1);
    assert!(error_codes(&output).contains(&"manifest.data-version".to_owned()));
}

/// REQ: LIF-100
#[test]
fn lif_100_install_with_maximum_data_version_succeeds() {
    let rig = rig();
    let output = rig.install(&notes_v1().data_version_json("4294967295"));
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
}

proptest! {
    /// REQ: LIF-100
    #[test]
    fn lif_100_every_u32_from_one_is_accepted(value in 1u32..=u32::MAX) {
        prop_assert!(!is_refused_for_data_version(&value.to_string()));
    }

    /// REQ: LIF-100
    #[test]
    fn lif_100_every_integer_above_u32_is_refused(value in (u32::MAX as u64 + 1)..=u64::MAX) {
        prop_assert!(is_refused_for_data_version(&value.to_string()));
    }
}

// ---------------------------------------------------------------------------------------------
// LIF-101: the stamp lives where the plugin cannot reach it
// ---------------------------------------------------------------------------------------------

/// REQ: LIF-101
#[test]
fn lif_101_stamp_is_new_data_version_after_successful_migration() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "count", b"7")
        .expect("put");
    update_ok(
        &rig,
        &notes_v2(vec![Step::Remove(Bucket::Global, "count".into())]),
        &[],
    );
    let report = server.upgrade(NOTES);
    assert_eq!(report.active_version, "2.0.0");
    assert_eq!(rig.stamp(NOTES, Bucket::Global), Some(2));
    assert_eq!(rig.data_dir_stamp(NOTES), Some(2));
    assert_eq!(rig.info_json(NOTES)["data_version"], 2);
}

/// REQ: LIF-101
#[test]
fn lif_101_stamp_stays_old_after_failed_migration() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "count", b"7")
        .expect("put");
    update_ok(
        &rig,
        &notes_v2(vec![Step::Fail(plugin_error(ErrorCode::Internal, "boom"))]),
        &[],
    );
    let report = server.upgrade(NOTES);
    assert!(report.rolled_back, "{report:?}");
    assert_eq!(rig.stamp(NOTES, Bucket::Global), Some(1));
    assert_eq!(rig.data_dir_stamp(NOTES), Some(1));
}

/// REQ: LIF-101
#[test]
fn lif_101_stamp_is_set_on_first_creation() {
    let rig = rig();
    install_ok(&rig, &notes_v3(Vec::new()));
    let server = rig.server("a", "/p/one", "default");
    server.activate(NOTES);
    assert_eq!(rig.stamp(NOTES, Bucket::Global), None, "nothing stored yet");
    server
        .storage(NOTES)
        .put(Bucket::Global, "k", b"v")
        .expect("put");
    assert_eq!(rig.stamp(NOTES, Bucket::Global), Some(3));
}

/// REQ: LIF-101
#[test]
fn lif_101_plugin_cannot_see_the_stamp_in_its_keys_or_files() {
    let rig = rig();
    install_ok(&rig, &notes_v2(Vec::new()));
    let server = rig.server("a", "/p/one", "default");
    server.activate(NOTES);
    server
        .storage(NOTES)
        .put(Bucket::Global, "k", b"v")
        .expect("put");
    assert_eq!(
        server.storage(NOTES).keys(Bucket::Global, ""),
        vec!["k".to_owned()]
    );
    assert!(
        rig.data_dir_files(NOTES).is_empty(),
        "the stamp is not a file of the data directory"
    );
    assert!(
        !rig.stamp_location(NOTES)
            .starts_with(rig.plugin_data_dir(NOTES)),
        "the stamp is outside the plugin's reach"
    );
}

/// REQ: LIF-101
#[test]
fn lif_101_each_bucket_has_its_own_stamp() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    server
        .storage(NOTES)
        .put(Bucket::Project, "p", b"1")
        .expect("put");
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    server.upgrade(NOTES);
    assert_eq!(server.stamp(NOTES, Bucket::Global), Some(2));
    assert_eq!(server.stamp(NOTES, Bucket::Project), Some(2));
    assert_eq!(
        server.stamp(NOTES, Bucket::Session),
        None,
        "a session bucket never written has no stamp"
    );
}

// ---------------------------------------------------------------------------------------------
// LIF-102: unstamped data is never migrated; first write stamps
// ---------------------------------------------------------------------------------------------

/// REQ: LIF-102
#[test]
fn lif_102_first_put_on_new_plugin_stamps_current_data_version() {
    let rig = rig();
    install_ok(&rig, &notes_v2(Vec::new()));
    let server = rig.server("a", "/p/one", "default");
    server.activate(NOTES);
    server
        .storage(NOTES)
        .put(Bucket::Project, "k", b"v")
        .expect("put");
    assert_eq!(server.stamp(NOTES, Bucket::Project), Some(2));
}

/// REQ: LIF-102
#[test]
fn lif_102_bucket_without_stamp_is_not_migrated() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    update_ok(
        &rig,
        &notes_v3(vec![Step::Put(
            Bucket::Global,
            "touched".into(),
            bytes("yes"),
        )]),
        &[],
    );
    let report = server.upgrade(NOTES);
    assert_eq!(report.active_version, "3.0.0");
    assert!(
        global_calls(&rig, NOTES).is_empty(),
        "no stamp means no migrate call"
    );
    assert_eq!(server.storage(NOTES).get(Bucket::Global, "touched"), None);
}

/// REQ: LIF-102
#[test]
fn lif_102_unstamped_bucket_gets_new_version_at_first_write_after_update() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    update_ok(&rig, &notes_v3(Vec::new()), &[]);
    server.upgrade(NOTES);
    server
        .storage(NOTES)
        .put(Bucket::Global, "k", b"v")
        .expect("put");
    assert_eq!(rig.stamp(NOTES, Bucket::Global), Some(3));
}

// ---------------------------------------------------------------------------------------------
// LIF-103: restore bytes and the data version
// ---------------------------------------------------------------------------------------------

/// REQ: LIF-103
#[test]
fn lif_103_restore_bytes_dropped_when_data_version_differs() {
    let rig = rig();
    install_ok(&rig, &notes_v1().restore_bytes(bytes("state-v1")));
    let server = running_v1(&rig, "a", "/p/one", "default");
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    let report = server.upgrade(NOTES);
    assert_eq!(report.restored, None);
    assert!(rig
        .log_codes()
        .contains(&"restore.dropped-data-version".to_owned()));
}

/// REQ: LIF-103
#[test]
fn lif_103_restore_bytes_kept_when_data_version_is_equal() {
    let rig = rig();
    install_ok(&rig, &notes_v2(Vec::new()).restore_bytes(bytes("state-v2")));
    let server = rig.server("a", "/p/one", "default");
    server.activate(NOTES);
    update_ok(
        &rig,
        &PluginSpec::new(NOTES, "2.1.0")
            .data_version(2)
            .restore_bytes(bytes("state-v2")),
        &[],
    );
    let report = server.upgrade(NOTES);
    assert_eq!(report.active_version, "2.1.0");
    assert_eq!(report.restored, Some(bytes("state-v2")));
    assert!(!rig
        .log_codes()
        .contains(&"restore.dropped-data-version".to_owned()));
}

/// REQ: LIF-103
#[test]
fn lif_103_restore_bytes_dropped_without_migrator_too() {
    let rig = rig();
    install_ok(&rig, &notes_v1().restore_bytes(bytes("state-v1")));
    let server = running_v1(&rig, "a", "/p/one", "default");
    update_ok(&rig, &PluginSpec::new(NOTES, "2.0.0").data_version(2), &[]);
    let report = server.upgrade(NOTES);
    assert_eq!(report.restored, None);
    let log = rig.log_codes();
    assert!(log.contains(&"restore.dropped-data-version".to_owned()));
    assert!(log.contains(&"migration.skipped".to_owned()));
}

// ---------------------------------------------------------------------------------------------
// LIF-104: when migrate is called, how often, with which arguments
// ---------------------------------------------------------------------------------------------

/// REQ: LIF-104
#[test]
fn lif_104_update_one_to_three_calls_migrate_once_with_one_and_three() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &notes_v3(Vec::new()), &[]);
    server.upgrade(NOTES);
    let calls = global_calls(&rig, NOTES);
    assert_eq!(calls.len(), 1);
    assert_eq!((calls[0].from, calls[0].to), (1, 3));
}

/// REQ: LIF-104
#[test]
fn lif_104_two_sessions_give_one_global_call_and_one_call_per_session() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let first = running_v1(&rig, "a", "/p/one", "alpha");
    let second = running_v1(&rig, "b", "/p/two", "beta");
    for server in [&first, &second] {
        server
            .storage(NOTES)
            .put(Bucket::Global, "g", b"1")
            .expect("put");
        server
            .storage(NOTES)
            .put(Bucket::Session, "s", b"1")
            .expect("put");
    }
    update_ok(&rig, &notes_v3(Vec::new()), &[]);
    first.upgrade(NOTES);
    second.upgrade(NOTES);
    assert_eq!(global_calls(&rig, NOTES).len(), 1);
    let sessions = session_calls(&rig, NOTES);
    assert_eq!(sessions.len(), 2);
    assert!(sessions.iter().all(|call| (call.from, call.to) == (1, 3)));
    let scopes: Vec<MigrateScope> = sessions.into_iter().map(|call| call.scope).collect();
    assert!(scopes.contains(&MigrateScope::Session {
        project: "/p/one".into(),
        session: "alpha".into()
    }));
    assert!(scopes.contains(&MigrateScope::Session {
        project: "/p/two".into(),
        session: "beta".into()
    }));
}

/// REQ: LIF-104
#[test]
fn lif_104_from_is_always_less_than_to() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    server
        .storage(NOTES)
        .put(Bucket::Project, "p", b"1")
        .expect("put");
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    server.upgrade(NOTES);
    let calls = rig.migrate_calls(NOTES);
    assert!(
        !calls.is_empty(),
        "upgrade must record at least one migrate call"
    );
    for call in calls {
        assert!(call.from < call.to, "{call:?}");
    }
}

/// REQ: LIF-104
#[test]
fn lif_104_no_second_call_when_the_same_session_activates_again() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    server.upgrade(NOTES);
    let after_first = rig.migrate_calls(NOTES).len();
    let restarted = rig.restart_server(&server);
    restarted.activate(NOTES);
    assert_eq!(
        rig.migrate_calls(NOTES).len(),
        after_first,
        "a migrated bucket is not migrated again"
    );
}

/// REQ: LIF-104
#[test]
fn lif_104_no_call_when_data_version_is_unchanged() {
    let rig = rig();
    install_ok(&rig, &notes_v2(Vec::new()));
    let server = rig.server("a", "/p/one", "default");
    server.activate(NOTES);
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(
        &rig,
        &PluginSpec::new(NOTES, "2.1.0")
            .data_version(2)
            .migrator(Vec::new()),
        &[],
    );
    server.upgrade(NOTES);
    assert!(rig.migrate_calls(NOTES).is_empty());
}

/// REQ: LIF-104
#[test]
fn lif_104_from_is_the_lowest_stamp_in_scope() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    server.upgrade(NOTES);
    server
        .storage(NOTES)
        .put(Bucket::Project, "p", b"1")
        .expect("put");
    update_ok(&rig, &notes_v3(Vec::new()), &[]);
    let calls_before = rig.migrate_calls(NOTES).len();
    server.upgrade(NOTES);
    let new_calls: Vec<MigrateCall> = rig
        .migrate_calls(NOTES)
        .into_iter()
        .skip(calls_before)
        .collect();
    assert_eq!(new_calls.len(), 1, "one call per update and scope");
    assert_eq!((new_calls[0].from, new_calls[0].to), (2, 3));
}

/// REQ: LIF-104
#[test]
fn lif_104_without_migrator_the_host_stamps_and_logs_skipped() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    server
        .storage(NOTES)
        .put(Bucket::Shared, "cache", b"x")
        .expect("put");
    update_ok(&rig, &PluginSpec::new(NOTES, "2.0.0").data_version(2), &[]);
    server.upgrade(NOTES);
    assert!(rig.migrate_calls(NOTES).is_empty());
    assert_eq!(rig.stamp(NOTES, Bucket::Global), Some(2));
    assert!(rig.log_codes().contains(&"migration.skipped".to_owned()));
    assert_eq!(
        server.storage(NOTES).get(Bucket::Shared, "cache"),
        None,
        "the shared bucket is cleared"
    );
    assert_eq!(
        server.storage(NOTES).get(Bucket::Global, "g"),
        Some(bytes("1"))
    );
}

/// REQ: LIF-104
#[test]
fn lif_104_disabled_plugin_migrates_at_first_activation_after_enable() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    assert_eq!(rig.cli(&["disable", NOTES]).exit_code, 0);
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    server.upgrade(NOTES);
    assert!(
        rig.migrate_calls(NOTES).is_empty(),
        "a disabled plugin does not activate"
    );
    assert_eq!(rig.cli(&["enable", NOTES]).exit_code, 0);
    server.activate(NOTES);
    assert_eq!(rig.migrate_calls(NOTES).len(), 1);
}

// ---------------------------------------------------------------------------------------------
// LIF-105: a migration failure is not a plugin failure
// ---------------------------------------------------------------------------------------------

fn failing_update(rig: &MigrationRig, steps: Vec<Step>) -> (MigServer, ActivateReport) {
    install_ok(rig, &notes_v1());
    let server = running_v1(rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(rig, &notes_v2(steps), &[]);
    let report = server.upgrade(NOTES);
    (server, report)
}

/// REQ: LIF-105
#[test]
fn lif_105_error_result_is_a_migration_failure_not_counted_in_failure_window() {
    let rig = rig();
    let (_server, report) = failing_update(
        &rig,
        vec![Step::Fail(plugin_error(
            ErrorCode::Internal,
            "corrupt prefs",
        ))],
    );
    assert!(report.rolled_back, "{report:?}");
    assert!(rig.log_codes().contains(&"migration.failed".to_owned()));
    assert_eq!(rig.failures_in_window(NOTES), 0);
}

/// REQ: LIF-105
#[test]
fn lif_105_trap_is_a_migration_failure_not_counted_in_failure_window() {
    let rig = rig();
    let (_server, report) = failing_update(&rig, vec![Step::Trap]);
    assert!(report.rolled_back, "{report:?}");
    assert!(rig.log_codes().contains(&"migration.failed".to_owned()));
    assert_eq!(rig.failures_in_window(NOTES), 0);
}

/// REQ: LIF-105
#[test]
fn lif_105_missed_deadline_is_a_migration_failure_not_counted_in_failure_window() {
    let rig = rig();
    let (_server, report) = failing_update(&rig, vec![Step::Hang]);
    assert!(report.rolled_back, "{report:?}");
    let calls = rig.migrate_calls(NOTES);
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0].elapsed_ms >= 30_000,
        "the default deadline is 30000 ms: {}",
        calls[0].elapsed_ms
    );
    assert!(
        calls[0].elapsed_ms <= 30_020,
        "interrupted within 20 ms of the deadline: {}",
        calls[0].elapsed_ms
    );
    assert_eq!(rig.failures_in_window(NOTES), 0);
}

/// REQ: LIF-105
#[test]
fn lif_105_declared_deadline_applies_to_migrate() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(
        &rig,
        &notes_v2(vec![Step::Hang]).migrate_deadline_ms(5_000),
        &[],
    );
    server.upgrade(NOTES);
    let calls = rig.migrate_calls(NOTES);
    assert!(
        calls[0].elapsed_ms >= 5_000 && calls[0].elapsed_ms <= 5_020,
        "{}",
        calls[0].elapsed_ms
    );
}

/// REQ: LIF-105
#[test]
fn lif_105_successful_migration_leaves_failure_window_empty() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    server.upgrade(NOTES);
    assert_eq!(rig.failures_in_window(NOTES), 0);
}

// ---------------------------------------------------------------------------------------------
// LIF-106: migrate is idempotent
// ---------------------------------------------------------------------------------------------

fn rename_steps() -> Vec<Step> {
    vec![
        Step::Put(Bucket::Global, "opened".into(), bytes("7")),
        Step::Remove(Bucket::Global, "count".into()),
        Step::WriteFile("layout.json".into(), bytes("{}")),
    ]
}

/// REQ: LIF-106
#[test]
fn lif_106_second_call_with_same_arguments_leaves_data_unchanged() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "count", b"7")
        .expect("put");
    update_ok(&rig, &notes_v2(rename_steps()), &[]);
    server.upgrade(NOTES);
    let after_one = dump(&server, Bucket::Global);
    let files_one = rig.data_dir_files(NOTES);
    server
        .call_migrate(NOTES, 1, 2)
        .expect("a repeated migrate call succeeds");
    assert_eq!(dump(&server, Bucket::Global), after_one);
    assert_eq!(rig.data_dir_files(NOTES), files_one);
}

/// REQ: LIF-106 MAY
#[test]
fn lif_106_interrupted_run_may_repeat_with_identical_arguments() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "count", b"7")
        .expect("put");
    let mut steps = vec![
        Step::Put(Bucket::Global, "opened".into(), bytes("7")),
        Step::KillHostFirstRun,
    ];
    steps.push(Step::Remove(Bucket::Global, "count".into()));
    update_ok(&rig, &notes_v2(steps), &[]);
    let killed = server.upgrade(NOTES);
    assert!(killed.killed);
    let restarted = rig.restart_server(&server);
    let report = restarted.activate(NOTES);
    assert_eq!(report.active_version, "2.0.0", "{report:?}");
    let calls = rig.migrate_calls(NOTES);
    // Either the repeat happened with the same arguments (allowed), or it did not (also allowed).
    if calls.len() > 1 {
        assert_eq!((calls[0].from, calls[0].to), (calls[1].from, calls[1].to));
    }
    let data = dump(&restarted, Bucket::Global);
    assert_eq!(data.get("opened"), Some(&bytes("7")));
    assert_eq!(data.get("count"), None);
}

// ---------------------------------------------------------------------------------------------
// LIF-107: backup before migrate, space check, cap
// ---------------------------------------------------------------------------------------------

fn seeded_v1(rig: &MigrationRig, name: &str, megabytes: u64) -> MigServer {
    install_ok(rig, &notes_v1());
    let server = running_v1(rig, name, "/p/one", "default");
    for index in 0..megabytes {
        let value = vec![b'x'; (MIB as usize) - 16];
        server
            .storage(NOTES)
            .put(Bucket::Global, &format!("blob-{index}"), &value)
            .expect("put");
    }
    server
}

/// REQ: LIF-107
#[test]
fn lif_107_backup_directory_is_named_version_and_utc_timestamp() {
    let rig = rig();
    rig.set_clock_utc("2026-10-12T09:14:04Z");
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    server.upgrade(NOTES);
    assert_eq!(
        rig.backups(NOTES),
        vec!["1.0.0-20261012T091404Z".to_owned()]
    );
    assert!(is_backup_name(&rig.backups(NOTES)[0], "1.0.0"));
}

/// REQ: LIF-107
#[test]
fn lif_107_backup_holds_global_project_session_buckets_and_data_directory() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    let storage = server.storage(NOTES);
    storage.put(Bucket::Global, "g", b"global").expect("put");
    storage.put(Bucket::Project, "p", b"project").expect("put");
    storage.put(Bucket::Session, "s", b"session").expect("put");
    rig.write_data_file(NOTES, "layout.json", b"{\"a\":1}");
    update_ok(
        &rig,
        &notes_v2(vec![Step::Remove(Bucket::Global, "g".into())]),
        &[],
    );
    server.upgrade(NOTES);
    let backup = rig.backups(NOTES).remove(0);
    let global: BTreeMap<String, Vec<u8>> = [("g".to_owned(), bytes("global"))].into();
    assert_eq!(rig.backup_dump(NOTES, &backup, Bucket::Global), global);
    assert_eq!(
        rig.backup_dump(NOTES, &backup, Bucket::Project).get("p"),
        Some(&bytes("project"))
    );
    assert_eq!(
        rig.backup_dump(NOTES, &backup, Bucket::Session).get("s"),
        Some(&bytes("session"))
    );
    assert_eq!(
        rig.backup_data_files(NOTES, &backup).get("layout.json"),
        Some(&bytes("{\"a\":1}"))
    );
}

/// REQ: LIF-107
#[test]
fn lif_107_backup_exists_before_migrate_is_called() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    server.upgrade(NOTES);
    let calls = rig.migrate_calls(NOTES);
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0].backup_name.is_some(),
        "a backup was taken before the call: {:?}",
        calls[0]
    );
}

/// REQ: LIF-107
#[test]
fn lif_107_no_space_refused_before_touching_data() {
    let rig = rig();
    let server = seeded_v1(&rig, "a", 2);
    let before = dump(&server, Bucket::Global);
    rig.set_free_bytes(100 * MIB);
    let output = rig.update(
        &notes_v2(vec![Step::Remove(Bucket::Global, "blob-0".into())]),
        &[],
    );
    assert_eq!(output.exit_code, 1);
    assert!(error_codes(&output).contains(&"install.no-space".to_owned()));
    assert!(rig.backups(NOTES).is_empty());
    assert!(rig.migrate_calls(NOTES).is_empty());
    assert_eq!(dump(&server, Bucket::Global), before);
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "1.0.0");
}

/// REQ: LIF-107
#[test]
fn lif_107_free_space_threshold_is_twice_data_plus_100_mib() {
    let rig = rig();
    let server = seeded_v1(&rig, "a", 2);
    let size = dump(&server, Bucket::Global)
        .iter()
        .map(|(key, value)| (key.len() + value.len()) as u64)
        .sum::<u64>();
    let needed = 2 * size + 100 * MIB;
    rig.set_free_bytes(needed - MIB);
    let refused = rig.update(&notes_v2(Vec::new()), &[]);
    assert_eq!(
        refused.exit_code, 1,
        "one mebibyte below the threshold is refused"
    );
    assert!(error_codes(&refused).contains(&"install.no-space".to_owned()));
    rig.set_free_bytes(needed + MIB);
    let accepted = rig.update(&notes_v2(Vec::new()), &[]);
    assert_eq!(accepted.exit_code, 0, "{:?}", accepted.stdout_jsonl);
}

/// REQ: LIF-107
#[test]
fn lif_107_cap_below_data_size_refuses_backup_too_large() {
    let rig = rig();
    rig.config("[plugins]\nmigration_backup_mib = 1\n");
    let server = seeded_v1(&rig, "a", 2);
    let before = dump(&server, Bucket::Global);
    let output = rig.update(&notes_v2(Vec::new()), &[]);
    assert_eq!(output.exit_code, 1);
    assert!(error_codes(&output).contains(&"migration.backup-too-large".to_owned()));
    assert_eq!(dump(&server, Bucket::Global), before);
    assert!(rig.migrate_calls(NOTES).is_empty());
}

/// REQ: LIF-107
#[test]
fn lif_107_no_backup_flag_lets_over_cap_update_proceed_without_backup() {
    let rig = rig();
    rig.config("[plugins]\nmigration_backup_mib = 1\n");
    let server = seeded_v1(&rig, "a", 2);
    let output = rig.update(&notes_v2(Vec::new()), &["--no-backup"]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    server.upgrade(NOTES);
    assert!(rig.backups(NOTES).is_empty(), "--no-backup takes no backup");
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "2.0.0");
}

/// REQ: LIF-107
#[test]
fn lif_107_cap_above_data_size_takes_the_backup() {
    let rig = rig();
    rig.config("[plugins]\nmigration_backup_mib = 3\n");
    let server = seeded_v1(&rig, "a", 2);
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    server.upgrade(NOTES);
    assert_eq!(rig.backups(NOTES).len(), 1);
}

/// REQ: LIF-107
#[test]
fn lif_107_default_cap_is_2048_mib() {
    let rig = rig();
    let server = seeded_v1(&rig, "a", 2);
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    server.upgrade(NOTES);
    assert_eq!(
        rig.backups(NOTES).len(),
        1,
        "2 MiB is far below the default 2048 MiB cap"
    );
}

/// REQ: LIF-107
#[test]
fn lif_107_cap_zero_disables_backups() {
    let rig = rig();
    rig.config("[plugins]\nmigration_backup_mib = 0\n");
    let server = seeded_v1(&rig, "a", 2);
    let _ = rig.update(&notes_v2(Vec::new()), &["--no-backup"]);
    server.upgrade(NOTES);
    assert!(rig.backups(NOTES).is_empty());
}

/// REQ: LIF-107
#[test]
fn lif_107_backup_too_large_message_names_size_and_limit() {
    let rig = rig();
    rig.config("[plugins]\nmigration_backup_mib = 1\n");
    seeded_v1(&rig, "a", 2);
    let output = rig.update(&notes_v2(Vec::new()), &[]);
    let message = output
        .stdout_jsonl
        .iter()
        .filter(|line| line["code"] == "migration.backup-too-large")
        .filter_map(|line| line["message"].as_str())
        .next()
        .expect("an error line with the message")
        .to_owned();
    assert!(
        message.starts_with("The data of com.example.notes is "),
        "{message}"
    );
    assert!(message.ends_with("Pass --no-backup."), "{message}");
}

// ---------------------------------------------------------------------------------------------
// LIF-108: failure restores the backup and rolls the update back
// ---------------------------------------------------------------------------------------------

fn half_written_failure() -> Vec<Step> {
    vec![
        Step::Put(Bucket::Global, "opened".into(), bytes("7")),
        Step::Remove(Bucket::Global, "count".into()),
        Step::Fail(plugin_error(
            ErrorCode::InvalidArgument,
            "prefs: expected value at line 1",
        )),
    ]
}

/// REQ: LIF-108
#[test]
fn lif_108_failed_migration_restores_buckets_to_the_backup() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "count", b"7")
        .expect("put");
    server
        .storage(NOTES)
        .put(Bucket::Global, "prefs", b"{bad")
        .expect("put");
    let before = dump(&server, Bucket::Global);
    update_ok(&rig, &notes_v2(half_written_failure()), &[]);
    let report = server.upgrade(NOTES);
    assert!(report.rolled_back, "{report:?}");
    assert_eq!(
        dump(&server, Bucket::Global),
        before,
        "half-written keys are undone"
    );
}

/// REQ: LIF-108
#[test]
fn lif_108_previous_version_stays_active_and_candidate_is_skipped() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "count", b"7")
        .expect("put");
    update_ok(&rig, &notes_v2(half_written_failure()), &[]);
    let report = server.upgrade(NOTES);
    assert_eq!(report.active_version, "1.0.0");
    let entry = registry_entry(&rig, NOTES);
    assert_eq!(entry["active_version"], "1.0.0");
    assert!(entry["update"].is_null(), "{entry}");
    assert!(entry["skip"]
        .as_array()
        .expect("skip list")
        .iter()
        .any(|version| version == "2.0.0"));
}

/// REQ: LIF-108
#[test]
fn lif_108_log_carries_the_plugin_error_code_and_message() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "count", b"7")
        .expect("put");
    update_ok(&rig, &notes_v2(half_written_failure()), &[]);
    server.upgrade(NOTES);
    let line = rig
        .log_line("migration.failed")
        .expect("migration.failed logged");
    let text = line.to_string();
    assert!(text.contains("invalid-argument"), "{text}");
    assert!(text.contains("prefs: expected value at line 1"), "{text}");
    assert!(rig.log_codes().contains(&"update.rolled-back".to_owned()));
}

/// REQ: LIF-108
#[test]
fn lif_108_stamp_and_data_directory_are_restored() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "count", b"7")
        .expect("put");
    rig.write_data_file(NOTES, "layout.json", b"old");
    let mut steps = vec![
        Step::WriteFile("layout.json".into(), bytes("new")),
        Step::WriteFile("extra.json".into(), bytes("x")),
    ];
    steps.push(Step::Fail(plugin_error(ErrorCode::Io, "disk")));
    update_ok(&rig, &notes_v2(steps), &[]);
    server.upgrade(NOTES);
    let files = rig.data_dir_files(NOTES);
    assert_eq!(files.get("layout.json"), Some(&bytes("old")));
    assert!(!files.contains_key("extra.json"));
    assert_eq!(rig.stamp(NOTES, Bucket::Global), Some(1));
}

/// REQ: LIF-108
#[test]
fn lif_108_no_backup_failure_keeps_old_version_and_logs_no_backup_failed() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "count", b"7")
        .expect("put");
    update_ok(&rig, &notes_v2(half_written_failure()), &["--no-backup"]);
    let report = server.upgrade(NOTES);
    assert_eq!(report.active_version, "1.0.0");
    let line = rig
        .log_line("migration.no-backup-failed")
        .expect("migration.no-backup-failed logged");
    assert_eq!(line["level"], "error");
    assert_eq!(
        dump(&server, Bucket::Global).get("opened"),
        Some(&bytes("7")),
        "no backup, so the half write stays"
    );
}

/// REQ: LIF-108
#[test]
fn lif_108_trap_in_migrate_also_restores_the_backup() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "count", b"7")
        .expect("put");
    let before = dump(&server, Bucket::Global);
    update_ok(
        &rig,
        &notes_v2(vec![
            Step::Put(Bucket::Global, "half".into(), bytes("1")),
            Step::Trap,
        ]),
        &[],
    );
    let report = server.upgrade(NOTES);
    assert!(report.rolled_back);
    assert_eq!(dump(&server, Bucket::Global), before);
}

// ---------------------------------------------------------------------------------------------
// LIF-109: only the newest backup is kept
// ---------------------------------------------------------------------------------------------

/// REQ: LIF-109
#[test]
fn lif_109_two_successful_updates_keep_only_the_newest_backup() {
    let rig = rig();
    rig.set_clock_utc("2026-10-12T09:00:00Z");
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    server.upgrade(NOTES);
    assert_eq!(
        rig.backups(NOTES),
        vec!["1.0.0-20261012T090000Z".to_owned()]
    );
    rig.advance_clock(60_000);
    update_ok(&rig, &notes_v3(Vec::new()), &[]);
    server.upgrade(NOTES);
    assert_eq!(
        rig.backups(NOTES),
        vec!["2.0.0-20261012T090100Z".to_owned()]
    );
}

/// REQ: LIF-109
#[test]
fn lif_109_failed_update_keeps_the_older_backup() {
    let rig = rig();
    rig.set_clock_utc("2026-10-12T09:00:00Z");
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    server.upgrade(NOTES);
    let first = rig.backups(NOTES);
    rig.advance_clock(60_000);
    update_ok(
        &rig,
        &notes_v3(vec![Step::Fail(plugin_error(ErrorCode::Internal, "x"))]),
        &[],
    );
    server.upgrade(NOTES);
    assert!(
        rig.backups(NOTES).iter().any(|name| first.contains(name)),
        "the older backup survives a failed update"
    );
}

/// REQ: LIF-109
#[test]
fn lif_109_older_backups_are_deleted_after_activation() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    for (version, data_version) in [("2.0.0", 2u32), ("3.0.0", 3), ("4.0.0", 4)] {
        rig.advance_clock(1_000);
        update_ok(
            &rig,
            &PluginSpec::new(NOTES, version)
                .data_version(data_version)
                .migrator(Vec::new()),
            &[],
        );
        server.upgrade(NOTES);
        assert_eq!(rig.backups(NOTES).len(), 1, "after {version}");
    }
}

// ---------------------------------------------------------------------------------------------
// LIF-110: interrupted migration
// ---------------------------------------------------------------------------------------------

fn kill_in_middle() -> Vec<Step> {
    vec![
        Step::Put(Bucket::Global, "half".into(), bytes("1")),
        Step::KillHost,
    ]
}

/// REQ: LIF-110
#[test]
fn lif_110_migrating_state_is_on_disk_before_migrate_runs() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &notes_v2(kill_in_middle()), &[]);
    let report = server.upgrade(NOTES);
    assert!(report.killed);
    rig.power_loss();
    let update = registry_entry(&rig, NOTES)["update"].clone();
    assert_eq!(update["state"], "migrating");
    assert_eq!(update["attempts"], 0);
}

/// REQ: LIF-110
#[test]
fn lif_110_next_start_restores_backup_increments_attempts_and_runs_again() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    let steps = vec![
        Step::Put(Bucket::Global, "half".into(), bytes("1")),
        Step::KillHostFirstRun,
        Step::Remove(Bucket::Global, "g".into()),
    ];
    update_ok(&rig, &notes_v2(steps), &[]);
    assert!(server.upgrade(NOTES).killed);
    let restarted = rig.restart_server(&server);
    let report = restarted.activate(NOTES);
    assert_eq!(report.active_version, "2.0.0", "{report:?}");
    assert_eq!(
        rig.migrate_calls(NOTES).len(),
        2,
        "the update ran again from the beginning"
    );
    let data = dump(&restarted, Bucket::Global);
    assert_eq!(data.get("g"), None);
    assert_eq!(data.get("half"), Some(&bytes("1")));
}

/// REQ: LIF-110
#[test]
fn lif_110_attempts_is_one_after_the_first_restart() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &notes_v2(kill_in_middle()), &[]);
    assert!(server.upgrade(NOTES).killed);
    let restarted = rig.restart_server(&server);
    assert!(
        restarted.activate(NOTES).killed,
        "the script kills every run"
    );
    rig.power_loss();
    let update = registry_entry(&rig, NOTES)["update"].clone();
    assert_eq!(update["state"], "migrating");
    assert_eq!(update["attempts"], 1);
}

/// REQ: LIF-110
#[test]
fn lif_110_second_interruption_rolls_back_with_migration_interrupted_and_skip() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    let before = dump(&server, Bucket::Global);
    update_ok(&rig, &notes_v2(kill_in_middle()), &[]);
    assert!(server.upgrade(NOTES).killed);
    let second = rig.restart_server(&server);
    assert!(second.activate(NOTES).killed);
    let third = rig.restart_server(&second);
    let report = third.activate(NOTES);
    assert_eq!(report.active_version, "1.0.0", "{report:?}");
    let entry = registry_entry(&rig, NOTES);
    assert!(entry["update"].is_null(), "{entry}");
    assert!(entry["skip"]
        .as_array()
        .expect("skip list")
        .iter()
        .any(|version| version == "2.0.0"));
    assert!(rig
        .log_codes()
        .contains(&"migration.interrupted".to_owned()));
    assert_eq!(
        dump(&third, Bucket::Global),
        before,
        "the backup was restored"
    );
    assert_eq!(rig.migrate_calls(NOTES).len(), 2, "no third attempt runs");
}

/// REQ: LIF-110
#[test]
fn lif_110_interrupted_message_names_plugin_and_attempt_count() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &notes_v2(kill_in_middle()), &[]);
    server.upgrade(NOTES);
    let second = rig.restart_server(&server);
    second.activate(NOTES);
    rig.restart_server(&second).activate(NOTES);
    let line = rig
        .log_line("migration.interrupted")
        .expect("migration.interrupted logged");
    assert!(
        line.to_string().contains("The migration of com.example.notes was interrupted 2 times. The update was rolled back."),
        "{line}"
    );
}

// ---------------------------------------------------------------------------------------------
// LIF-111: downgrade over newer data
// ---------------------------------------------------------------------------------------------

fn migrated_to_v2(rig: &MigrationRig) -> MigServer {
    install_ok(rig, &notes_v1());
    let server = running_v1(rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "count", b"7")
        .expect("put");
    update_ok(
        rig,
        &notes_v2(vec![
            Step::Put(Bucket::Global, "opened".into(), bytes("7")),
            Step::Remove(Bucket::Global, "count".into()),
        ]),
        &[],
    );
    server.upgrade(NOTES);
    server
}

/// REQ: LIF-111
#[test]
fn lif_111_rollback_after_migration_is_refused_migration_data_newer() {
    let rig = rig();
    migrated_to_v2(&rig);
    let output = rig.rollback(NOTES, &[]);
    assert_eq!(output.exit_code, 1);
    assert!(error_codes(&output).contains(&"migration.data-newer".to_owned()));
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "2.0.0");
}

/// REQ: LIF-111
#[test]
fn lif_111_refusal_message_names_supported_version_and_stamp() {
    let rig = rig();
    migrated_to_v2(&rig);
    let output = rig.rollback(NOTES, &[]);
    let message = output
        .stdout_jsonl
        .iter()
        .filter(|line| line["code"] == "migration.data-newer")
        .filter_map(|line| line["message"].as_str())
        .next()
        .expect("an error line with the message")
        .to_owned();
    assert_eq!(
        message,
        "com.example.notes 1.0.0 supports data version 1. The stored data is at 2. Use --restore-backup or --discard-data."
    );
}

/// REQ: LIF-111
#[test]
fn lif_111_restore_backup_flag_restores_the_pre_update_data() {
    let rig = rig();
    let server = migrated_to_v2(&rig);
    let output = rig.rollback(NOTES, &["--restore-backup"]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "1.0.0");
    let restarted = rig.restart_server(&server);
    restarted.activate(NOTES);
    let data = dump(&restarted, Bucket::Global);
    assert_eq!(data.get("count"), Some(&bytes("7")));
    assert_eq!(data.get("opened"), None);
    assert_eq!(rig.stamp(NOTES, Bucket::Global), Some(1));
}

/// REQ: LIF-111
#[test]
fn lif_111_discard_data_flag_deletes_buckets_and_data_directory() {
    let rig = rig();
    let server = migrated_to_v2(&rig);
    rig.write_data_file(NOTES, "layout.json", b"{}");
    let output = rig.rollback(NOTES, &["--discard-data"]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "1.0.0");
    let restarted = rig.restart_server(&server);
    restarted.activate(NOTES);
    assert!(dump(&restarted, Bucket::Global).is_empty());
    assert!(rig.data_dir_files(NOTES).is_empty());
}

/// REQ: LIF-111
#[test]
fn lif_111_equal_data_version_needs_no_flag() {
    let rig = rig();
    install_ok(&rig, &notes_v2(Vec::new()));
    let server = rig.server("a", "/p/one", "default");
    server.activate(NOTES);
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &PluginSpec::new(NOTES, "2.1.0").data_version(2), &[]);
    server.upgrade(NOTES);
    let output = rig.rollback(NOTES, &[]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
}

/// REQ: LIF-111
#[test]
fn lif_111_downgrade_install_over_newer_data_is_refused() {
    let rig = rig();
    migrated_to_v2(&rig);
    let output = rig.cli_install_file(&notes_v1(), &["--allow-downgrade"]);
    assert_eq!(output.exit_code, 1);
    assert!(error_codes(&output).contains(&"migration.data-newer".to_owned()));
}

// ---------------------------------------------------------------------------------------------
// LIF-112: the cross-process migrate lock
// ---------------------------------------------------------------------------------------------

fn two_servers_on_v1(rig: &MigrationRig) -> (MigServer, MigServer) {
    install_ok(rig, &notes_v1());
    let first = running_v1(rig, "a", "/p/one", "alpha");
    let second = running_v1(rig, "b", "/p/two", "beta");
    first
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    (first, second)
}

/// REQ: LIF-112
#[test]
fn lif_112_global_migration_holds_the_lock_file() {
    let rig = rig();
    let (first, _second) = two_servers_on_v1(&rig);
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    first.upgrade(NOTES);
    let calls = global_calls(&rig, NOTES);
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0].lock_held,
        "plugins/locks/{NOTES}.migrate.lock held during migrate"
    );
    assert!(rig
        .lock_path(NOTES)
        .ends_with(format!("plugins/locks/{NOTES}.migrate.lock")));
}

/// REQ: LIF-112
#[test]
fn lif_112_two_servers_call_migrate_once_for_the_global_bucket() {
    let rig = rig();
    let (first, second) = two_servers_on_v1(&rig);
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    first.upgrade(NOTES);
    second.upgrade(NOTES);
    assert_eq!(global_calls(&rig, NOTES).len(), 1);
}

/// REQ: LIF-112
#[test]
fn lif_112_other_server_deactivates_with_upgrade_before_waiting() {
    let rig = rig();
    let (first, second) = two_servers_on_v1(&rig);
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    first.upgrade(NOTES);
    let report = second.upgrade(NOTES);
    assert_eq!(report.deactivation_reason.as_deref(), Some("upgrade"));
    assert_eq!(report.active_version, "2.0.0");
}

/// REQ: LIF-112
#[test]
fn lif_112_wait_for_the_lock_is_bounded_by_deadline_plus_five_seconds() {
    let rig = rig();
    let (first, second) = two_servers_on_v1(&rig);
    update_ok(
        &rig,
        &notes_v2(vec![Step::Hang]).migrate_deadline_ms(10_000),
        &[],
    );
    first.upgrade(NOTES);
    let report = second.upgrade(NOTES);
    assert!(
        report.lock_wait_ms <= 15_000,
        "waited {} ms",
        report.lock_wait_ms
    );
}

/// REQ: LIF-112
#[test]
fn lif_112_second_server_skips_when_the_stamp_is_already_new() {
    let rig = rig();
    let (first, second) = two_servers_on_v1(&rig);
    update_ok(
        &rig,
        &notes_v2(vec![Step::Put(Bucket::Global, "marker".into(), bytes("m"))]),
        &[],
    );
    first.upgrade(NOTES);
    first
        .storage(NOTES)
        .put(Bucket::Global, "after", b"kept")
        .expect("put");
    second.upgrade(NOTES);
    assert_eq!(
        first.storage(NOTES).get(Bucket::Global, "after"),
        Some(bytes("kept"))
    );
    assert_eq!(
        global_calls(&rig, NOTES).len(),
        1,
        "the second server did not migrate again"
    );
}

// ---------------------------------------------------------------------------------------------
// LIF-113: a stale instance cannot write the global bucket
// ---------------------------------------------------------------------------------------------

fn one_server_stale(rig: &MigrationRig) -> (MigServer, MigServer) {
    let (first, second) = two_servers_on_v1(rig);
    update_ok(rig, &notes_v2(Vec::new()), &[]);
    first.upgrade(NOTES);
    (first, second)
}

/// REQ: LIF-113
#[test]
fn lif_113_put_from_a_stale_instance_is_conflict_stale_data_version() {
    let rig = rig();
    let (_first, stale) = one_server_stale(&rig);
    let error = stale
        .storage(NOTES)
        .put(Bucket::Global, "g", b"old")
        .expect_err("stale write refused");
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(error.message, "stale-data-version");
}

/// REQ: LIF-113
#[test]
fn lif_113_write_batch_from_a_stale_instance_is_conflict_stale_data_version() {
    let rig = rig();
    let (_first, stale) = one_server_stale(&rig);
    let error = stale
        .storage(NOTES)
        .write_batch(Bucket::Global, &[("g".to_owned(), Some(bytes("old")))])
        .expect_err("stale batch refused");
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(error.message, "stale-data-version");
}

/// REQ: LIF-113
#[test]
fn lif_113_remove_from_a_stale_instance_is_conflict_stale_data_version() {
    let rig = rig();
    let (first, stale) = one_server_stale(&rig);
    let error = stale
        .storage(NOTES)
        .remove(Bucket::Global, "g")
        .expect_err("stale remove refused");
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(error.message, "stale-data-version");
    assert_eq!(
        first.storage(NOTES).get(Bucket::Global, "g"),
        Some(bytes("1")),
        "the value survived"
    );
}

/// REQ: LIF-113
#[test]
fn lif_113_reads_from_a_stale_instance_still_work() {
    let rig = rig();
    let (_first, stale) = one_server_stale(&rig);
    assert_eq!(
        stale.storage(NOTES).get(Bucket::Global, "g"),
        Some(bytes("1"))
    );
    assert_eq!(
        stale.storage(NOTES).keys(Bucket::Global, ""),
        vec!["g".to_owned()]
    );
}

/// REQ: LIF-113
#[test]
fn lif_113_writes_work_again_after_the_stale_server_upgrades() {
    let rig = rig();
    let (_first, stale) = one_server_stale(&rig);
    stale.upgrade(NOTES);
    stale
        .storage(NOTES)
        .put(Bucket::Global, "g", b"new")
        .expect("write after upgrade");
}

// ---------------------------------------------------------------------------------------------
// LIF-114: project and session buckets migrate with the session that opens them
// ---------------------------------------------------------------------------------------------

/// REQ: LIF-114
#[test]
fn lif_114_two_projects_migrate_in_their_own_sessions_each_with_its_own_backup() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let one = running_v1(&rig, "a", "/p/one", "default");
    let two = running_v1(&rig, "b", "/p/two", "default");
    one.storage(NOTES)
        .put(Bucket::Project, "who", b"one")
        .expect("put");
    two.storage(NOTES)
        .put(Bucket::Project, "who", b"two")
        .expect("put");
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    rig.set_clock_utc("2026-10-12T10:00:00Z");
    one.upgrade(NOTES);
    rig.set_clock_utc("2026-10-12T10:00:05Z");
    two.upgrade(NOTES);
    let sessions = session_calls(&rig, NOTES);
    assert_eq!(sessions.len(), 2);
    let names: Vec<String> = sessions
        .iter()
        .filter_map(|call| call.backup_name.clone())
        .collect();
    assert_eq!(names.len(), 2, "each call had a backup");
    assert_ne!(names[0], names[1], "each session has its own backup");
}

/// REQ: LIF-114
#[test]
fn lif_114_a_project_is_not_migrated_until_its_session_opens_it() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let one = running_v1(&rig, "a", "/p/one", "default");
    let two = running_v1(&rig, "b", "/p/two", "default");
    one.storage(NOTES)
        .put(Bucket::Project, "who", b"one")
        .expect("put");
    two.storage(NOTES)
        .put(Bucket::Project, "who", b"two")
        .expect("put");
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    one.upgrade(NOTES);
    assert_eq!(one.stamp(NOTES, Bucket::Project), Some(2));
    assert_eq!(
        two.stamp(NOTES, Bucket::Project),
        Some(1),
        "the other project is untouched"
    );
}

/// REQ: LIF-114
#[test]
fn lif_114_session_bucket_migrates_in_its_own_session() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let alpha = running_v1(&rig, "a", "/p/one", "alpha");
    let beta = running_v1(&rig, "b", "/p/one", "beta");
    alpha
        .storage(NOTES)
        .put(Bucket::Session, "s", b"a")
        .expect("put");
    beta.storage(NOTES)
        .put(Bucket::Session, "s", b"b")
        .expect("put");
    update_ok(
        &rig,
        &notes_v2(vec![Step::Put(
            Bucket::Session,
            "migrated".into(),
            bytes("yes"),
        )]),
        &[],
    );
    alpha.upgrade(NOTES);
    assert_eq!(
        alpha.storage(NOTES).get(Bucket::Session, "migrated"),
        Some(bytes("yes"))
    );
    beta.upgrade(NOTES);
    assert_eq!(
        beta.storage(NOTES).get(Bucket::Session, "migrated"),
        Some(bytes("yes"))
    );
    assert_eq!(
        alpha.storage(NOTES).get(Bucket::Session, "s"),
        Some(bytes("a")),
        "no cross-session writes"
    );
}

// ---------------------------------------------------------------------------------------------
// LIF-115: put and write-batch are durable
// ---------------------------------------------------------------------------------------------

/// REQ: LIF-115
#[test]
fn lif_115_put_survives_power_loss_after_return() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "k", b"durable")
        .expect("put");
    rig.power_loss();
    let restarted = rig.restart_server(&server);
    restarted.activate(NOTES);
    assert_eq!(
        restarted.storage(NOTES).get(Bucket::Global, "k"),
        Some(bytes("durable"))
    );
}

/// REQ: LIF-115
#[test]
fn lif_115_write_batch_survives_power_loss_after_return() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .write_batch(
            Bucket::Project,
            &[
                ("a".to_owned(), Some(bytes("1"))),
                ("b".to_owned(), Some(bytes("2"))),
            ],
        )
        .expect("batch");
    rig.power_loss();
    let restarted = rig.restart_server(&server);
    restarted.activate(NOTES);
    let data = dump(&restarted, Bucket::Project);
    assert_eq!(data.get("a"), Some(&bytes("1")));
    assert_eq!(data.get("b"), Some(&bytes("2")));
}

/// REQ: LIF-115
#[test]
fn lif_115_put_survives_a_killed_process() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Session, "k", b"v")
        .expect("put");
    server.kill();
    let restarted = rig.restart_server(&server);
    restarted.activate(NOTES);
    assert_eq!(
        restarted.storage(NOTES).get(Bucket::Session, "k"),
        Some(bytes("v"))
    );
}

// ---------------------------------------------------------------------------------------------
// LIF-116: file sets with a version marker written last
// ---------------------------------------------------------------------------------------------

/// REQ: LIF-116 WARN
#[test]
fn lif_116_crash_between_set_and_marker_leaves_the_old_set() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    rig.write_data_file(NOTES, "set/a.json", b"old-a");
    rig.write_data_file(NOTES, "set/b.json", b"old-b");
    rig.write_data_file(NOTES, "VERSION", b"1");
    let old = rig.data_dir_files(NOTES);
    let steps = vec![
        Step::WriteFile("set-v2/a.json".into(), bytes("new-a")),
        Step::WriteFile("set-v2/b.json".into(), bytes("new-b")),
        Step::KillHost,
        Step::WriteFile("VERSION".into(), bytes("2")),
    ];
    update_ok(&rig, &notes_v2(steps), &[]);
    assert!(server.upgrade(NOTES).killed);
    let second = rig.restart_server(&server);
    second.activate(NOTES);
    let third = rig.restart_server(&second);
    third.activate(NOTES);
    let files = rig.data_dir_files(NOTES);
    assert_eq!(
        files.get("VERSION"),
        old.get("VERSION"),
        "the marker still names the old set"
    );
    assert_eq!(files.get("set/a.json"), old.get("set/a.json"));
    assert_eq!(files.get("set/b.json"), old.get("set/b.json"));
}

/// REQ: LIF-116 WARN
#[test]
fn lif_116_complete_run_leaves_the_new_set_and_the_marker() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    rig.write_data_file(NOTES, "VERSION", b"1");
    let steps = vec![
        Step::WriteFile("set-v2/a.json".into(), bytes("new-a")),
        Step::WriteFile("VERSION".into(), bytes("2")),
    ];
    update_ok(&rig, &notes_v2(steps), &[]);
    server.upgrade(NOTES);
    let files = rig.data_dir_files(NOTES);
    assert_eq!(files.get("VERSION"), Some(&bytes("2")));
    assert_eq!(files.get("set-v2/a.json"), Some(&bytes("new-a")));
}
