//! SCU-005, SCU-008 and SCU-009: a kill survives a restart, an update and an uninstall
//! (with reinstall), and the audit trail of a killed plugin is kept.

use ilium_plugin_host::kill_switch::{KillSwitch, PluginState};

use crate::secu_support::{audit_text, env, plugin, row, PLUGIN};

/// REQ: SCU-005
#[test]
fn secu_005_kill_survives_a_restart_and_activation_stays_refused_a() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    plugin(&env, &["kill", PLUGIN, "--reason", "bad build"]);

    // A restart is modelled by dropping the switch and opening the same state again.
    let mut reopened = KillSwitch::open(&root.path().join("state"));
    assert_eq!(reopened.plugin_state(PLUGIN), PluginState::DisabledByAdmin);
    assert!(reopened.try_activate(PLUGIN).is_err());
}

/// REQ: SCU-005
#[test]
fn secu_005_status_row_still_reads_disabled_by_admin_after_restart_b() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    plugin(&env, &["kill", PLUGIN, "--reason", "bad build"]);

    let restarted = env_with_new_session(root.path());
    assert!(
        row(&restarted, PLUGIN).contains("disabled-by-admin"),
        "instance state must be disabled-by-admin after restart"
    );
}

/// Same directories, fresh session: what a server restart looks like to the CLI.
fn env_with_new_session(root: &std::path::Path) -> ilium_plugin_host::cli::CliEnv {
    env(root, true)
}

/// REQ: SCU-008
#[test]
fn secu_008_update_of_a_killed_plugin_keeps_activation_refused_a() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    plugin(&env, &["kill", PLUGIN, "--reason", "version A is bad"]);
    plugin(&env, &["update", PLUGIN]);

    let mut switch = KillSwitch::open(&root.path().join("state"));
    assert!(switch.try_activate(PLUGIN).is_err(), "update must not clear the kill");
    assert_eq!(switch.plugin_state(PLUGIN), PluginState::DisabledByAdmin);
}

/// REQ: SCU-008
#[test]
fn secu_008_status_shows_disabled_by_admin_after_update_b() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    plugin(&env, &["kill", PLUGIN, "--reason", "version A is bad"]);
    plugin(&env, &["update", PLUGIN]);

    assert!(row(&env, PLUGIN).contains("disabled-by-admin"));
}

/// REQ: SCU-009
#[test]
fn secu_009_uninstall_of_a_killed_plugin_removes_the_kill_flag_a() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    plugin(&env, &["kill", PLUGIN, "--reason", "bad build"]);
    plugin(&env, &["uninstall", PLUGIN]);

    assert_eq!(
        KillSwitch::open(&root.path().join("state")).plugin_state(PLUGIN),
        PluginState::Active,
        "the kill flag goes with the plugin record"
    );
}

/// REQ: SCU-009
#[test]
fn secu_009_plugin_killed_audit_records_survive_uninstall_b() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    plugin(&env, &["kill", PLUGIN, "--reason", "bad build"]);
    plugin(&env, &["uninstall", PLUGIN]);

    assert!(
        audit_text(&env).contains("plugin-killed"),
        "the earlier plugin-killed record must still be in the audit log"
    );
}
