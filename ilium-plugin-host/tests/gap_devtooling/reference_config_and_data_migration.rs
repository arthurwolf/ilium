//! Configuration and stored-data migration on upgrade (DEV-986). Page: reference-config-and-data-migration.

use ilium_plugin_host::testing::migration::{MigrationRig, PluginSpec};

use crate::manual_page;

const PLUGIN_ID: &str = "example.agent-tools";

/// REQ: DEV-986
#[test]
fn stored_settings_survive_an_upgrade_and_an_older_host_refuses_newer_data() {
    let page = manual_page("reference-config-and-data-migration");
    assert!(
        page.to_lowercase().contains("settings"),
        "the reference describes stored settings"
    );

    let rig = MigrationRig::new();
    rig.install(&PluginSpec::new(PLUGIN_ID, "1.0.0").data_version(1));
    let server = rig.server("upgrade", "/projects/a", "default");
    server.write_setting("settings", "example.agent-tools.retention_days", "30");
    let upgraded = rig.update(&PluginSpec::new(PLUGIN_ID, "1.1.0").data_version(2), &[]);
    assert_eq!(upgraded.exit_code, 0, "{:?}", upgraded.stdout_jsonl);
    assert!(
        server.is_running(),
        "the host keeps running after the upgrade"
    );
    let newer = PluginSpec::new(PLUGIN_ID, "1.2.0").data_version(9);
    rig.install(&newer);
    let downgrade = rig.rollback(PLUGIN_ID, &[]);
    assert_eq!(downgrade.exit_code, 0, "{:?}", downgrade.stdout_jsonl);
    assert_eq!(
        rig.data_dir_stamp(PLUGIN_ID),
        Some(9),
        "newer stored data is not rewritten by the older host"
    );
    assert!(
        rig.registry_json().to_string().contains("retention_days"),
        "the setting is still registered after the upgrade"
    );
}

/// REQ: DEV-986
#[test]
fn stored_setting_stays_readable_under_its_key_after_the_upgrade() {
    let rig = MigrationRig::new();
    rig.install(&PluginSpec::new(PLUGIN_ID, "1.0.0").data_version(1));
    let server = rig.server("upgrade", "/projects/a", "default");
    server.write_setting("settings", "example.agent-tools.retention_days", "30");
    let upgraded = rig.update(&PluginSpec::new(PLUGIN_ID, "1.1.0").data_version(2), &[]);
    assert_eq!(upgraded.exit_code, 0, "{:?}", upgraded.stdout_jsonl);
    assert_eq!(
        server.storage(PLUGIN_ID).get(
            ilium_plugin_host::testing::migration::Bucket::Global,
            "retention_days"
        ),
        Some(b"30".to_vec()),
        "the value survives under its key"
    );
}
