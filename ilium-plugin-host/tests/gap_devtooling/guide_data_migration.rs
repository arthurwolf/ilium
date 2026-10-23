//! Data migration guide: a migration runs once per install, is retried when interrupted and
//! is idempotent on re-run (DEV-982, DEV-985). Page: guide-data-migration.

use ilium_plugin_host::testing::migration::{MigrationRig, PluginSpec, Step};

use crate::manual_page;

const PLUGIN_ID: &str = "example.agent-tools";

/// REQ: DEV-982
#[test]
fn migration_runs_once_per_install_and_a_retry_after_interruption_resumes() {
    let page = manual_page("guide-data-migration");
    assert!(
        page.to_lowercase().contains("checkpoint"),
        "the guide describes checkpoints"
    );

    let rig = MigrationRig::new();
    let version_one = PluginSpec::new(PLUGIN_ID, "1.0.0").data_version(1);
    let install = rig.install(&version_one);
    assert_eq!(install.exit_code, 0, "{:?}", install.stdout_jsonl);

    let interrupted_upgrade = PluginSpec::new(PLUGIN_ID, "1.1.0")
        .data_version(2)
        .migrator(vec![Step::KillHostFirstRun]);
    let _killed = rig.update(&interrupted_upgrade, &[]);
    let calls_after_kill = rig.migrate_calls(PLUGIN_ID);
    assert_eq!(
        calls_after_kill.len(),
        1,
        "the migration started once before the host was killed"
    );
    assert!(
        !calls_after_kill[0].returned_ok,
        "the first run did not finish"
    );
    assert_eq!(
        rig.data_dir_stamp(PLUGIN_ID),
        Some(1),
        "the stamp still reads the old version"
    );

    let retry = rig.update(&interrupted_upgrade, &[]);
    assert_eq!(retry.exit_code, 0, "{:?}", retry.stdout_jsonl);
    let calls_after_retry = rig.migrate_calls(PLUGIN_ID);
    assert_eq!(
        calls_after_retry.len(),
        2,
        "the retry runs the migration again"
    );
    assert_eq!(
        (calls_after_retry[1].from, calls_after_retry[1].to),
        (1, 2),
        "from version 1 to version 2"
    );
    assert!(calls_after_retry[1].returned_ok, "the retry completes");
    assert_eq!(
        rig.data_dir_stamp(PLUGIN_ID),
        Some(2),
        "the stamp moves to version 2 once"
    );
}

/// REQ: DEV-985
#[test]
fn migration_is_idempotent_and_a_second_update_on_the_same_data_changes_nothing() {
    let rig = MigrationRig::new();
    rig.install(&PluginSpec::new(PLUGIN_ID, "1.0.0").data_version(1));
    rig.write_data_file(PLUGIN_ID, "state.json", b"{\"theme\":\"dark\"}");
    let upgrade = PluginSpec::new(PLUGIN_ID, "1.1.0")
        .data_version(2)
        .migrator(vec![Step::WriteFile(
            "state.json".to_owned(),
            b"{\"theme\":\"dark\"}".to_vec(),
        )]);
    let first = rig.update(&upgrade, &[]);
    assert_eq!(first.exit_code, 0, "{:?}", first.stdout_jsonl);
    let files_after_first = rig.data_dir_files(PLUGIN_ID);
    let stamp_after_first = rig.data_dir_stamp(PLUGIN_ID);
    let calls_after_first = rig.migrate_calls(PLUGIN_ID).len();

    let second = rig.update(&upgrade, &[]);
    assert_eq!(second.exit_code, 0, "{:?}", second.stdout_jsonl);
    assert_eq!(
        rig.data_dir_files(PLUGIN_ID),
        files_after_first,
        "a second run leaves every data byte unchanged"
    );
    assert_eq!(
        rig.data_dir_stamp(PLUGIN_ID),
        stamp_after_first,
        "the stamp does not move"
    );
    assert_eq!(
        rig.migrate_calls(PLUGIN_ID).len(),
        calls_after_first,
        "an already-migrated install does not run the migration again"
    );
}
