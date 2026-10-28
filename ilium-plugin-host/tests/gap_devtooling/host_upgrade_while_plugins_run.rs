//! Host upgrade while plugins are running: the running plugin keeps its data and the server
//! reports the restart it needs (DEV-993). Page: reference-plugin-lifecycle-upgrade.

use ilium_plugin_host::testing::migration::{MigrationRig, PluginSpec};

use crate::manual_page;

/// REQ: DEV-993
#[test]
fn upgrading_the_host_under_a_running_plugin_keeps_its_data_and_reports_pending_restart() {
    let page = manual_page("reference-plugin-lifecycle-upgrade");
    assert!(
        page.to_lowercase().contains("restart"),
        "the page describes the restart"
    );

    let rig = MigrationRig::new();
    rig.install(&PluginSpec::new("example.agent-tools", "1.0.0").data_version(1));
    let running = rig.server("main", "/projects/a", "default");
    assert!(
        running.is_running(),
        "the host runs the plugin before the upgrade"
    );
    running.write_setting("settings", "example.agent-tools.retention_days", "14");
    let data_before = rig.data_dir_files("example.agent-tools");

    let upgraded = rig.update(
        &PluginSpec::new("example.agent-tools", "1.1.0").data_version(2),
        &[],
    );
    assert_eq!(upgraded.exit_code, 0, "{:?}", upgraded.stdout_jsonl);
    assert!(
        upgraded
            .stdout_jsonl
            .iter()
            .any(|record| record["code"] == "host.restart-pending"),
        "the upgrade reports host.restart-pending: {:?}",
        upgraded.stdout_jsonl
    );
    assert!(
        running.is_running(),
        "the running host is not stopped by the upgrade"
    );
    assert_eq!(
        rig.data_dir_files("example.agent-tools").len(),
        data_before.len(),
        "the plugin data files are kept"
    );
    assert_eq!(
        running.storage("example.agent-tools").get(
            ilium_plugin_host::testing::migration::Bucket::Global,
            "retention_days"
        ),
        Some(b"14".to_vec()),
        "the stored value is still readable after the upgrade"
    );
}

/// REQ: DEV-993
#[test]
fn direct_upgrade_from_one_minor_to_the_next_major_stamps_the_new_data_version() {
    let rig = MigrationRig::new();
    rig.install(&PluginSpec::new("example.agent-tools", "1.0.0").data_version(1));
    let upgraded = rig.update(
        &PluginSpec::new("example.agent-tools", "2.0.0").data_version(2),
        &[],
    );
    assert_eq!(upgraded.exit_code, 0, "{:?}", upgraded.stdout_jsonl);
    assert_eq!(
        rig.data_dir_stamp("example.agent-tools"),
        Some(2),
        "the stored data moves to version 2 in one step"
    );
}
