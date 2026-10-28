//! Operator guide for upgrading the host: order of steps, rollback and no-downtime expectations
//! (DEV-992). Page: guide-operator-upgrades.

use ilium_plugin_host::testing::migration::{MigrationRig, PluginSpec};

use crate::manual_page;

/// REQ: DEV-992
#[test]
fn operator_upgrade_order_is_backup_then_host_then_plugins_and_rollback_restores_the_backup() {
    let page = manual_page("guide-operator-upgrades");
    let backup_at = page.find("backup").expect("the guide says to back up");
    let host_at = page
        .find("host")
        .expect("the guide says to upgrade the host");
    assert!(
        backup_at < host_at,
        "the backup step comes before the host upgrade"
    );

    let rig = MigrationRig::new();
    rig.install(&PluginSpec::new("example.agent-tools", "1.0.0").data_version(1));
    let before_upgrade = rig.data_dir_files("example.agent-tools");
    let upgraded = rig.update(
        &PluginSpec::new("example.agent-tools", "1.1.0").data_version(2),
        &[],
    );
    assert_eq!(upgraded.exit_code, 0, "{:?}", upgraded.stdout_jsonl);
    let backups = rig.backups("example.agent-tools");
    assert!(
        !backups.is_empty(),
        "the upgrade keeps a backup of the data before it runs"
    );

    let rolled_back = rig.rollback("example.agent-tools", &[]);
    assert_eq!(rolled_back.exit_code, 0, "{:?}", rolled_back.stdout_jsonl);
    assert_eq!(
        rig.data_dir_files("example.agent-tools"),
        before_upgrade,
        "rollback restores the data files byte for byte"
    );
}

/// REQ: DEV-992
#[test]
fn preflight_lists_every_installed_plugin_without_starting_any_plugin() {
    let rig = MigrationRig::new();
    rig.install(&PluginSpec::new("example.agent-tools", "1.0.0").data_version(1));
    let preflight = rig.cli(&["preflight", "--target", "2.0.0"]);
    assert_eq!(preflight.exit_code, 0, "{:?}", preflight.stdout_jsonl);
    assert!(
        preflight
            .stdout_jsonl
            .iter()
            .any(|row| row.to_string().contains("example.agent-tools")),
        "every installed plugin is listed: {:?}",
        preflight.stdout_jsonl
    );
    assert!(
        rig.log_codes().iter().all(|code| code != "plugin.started"),
        "preflight must not start a plugin"
    );
}
