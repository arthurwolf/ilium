//! Troubleshooting plugins (user-troubleshooting.md): USR-030, USR-033, USR-100, USR-101.

use crate::support::*;

fn rig_with_core_and_two_third_party() -> UserRig {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.notes", "1.0.0", &["tree.read"]),
    );
    install_ok(&mut rig, &spec("com.example.old", "1.0.0", &[]));
    rig
}

// ---- USR-030 ----

/// REQ: USR-030
#[test]
fn usr_030_safe_mode_turns_third_party_off_and_keeps_core_on() {
    let mut rig = rig_with_core_and_two_third_party();
    rig.start_server_mode(StartMode::SafePlugins);
    for row in rig.list() {
        if row.id.starts_with("ilium.core.") {
            assert_eq!(row.state, "On", "{}", row.id);
        } else {
            assert_eq!(row.state, "Off, safe mode", "{}", row.id);
            assert_eq!(row.reason.as_deref(), Some("safe-mode"));
        }
    }
}

/// REQ: USR-030
#[test]
fn usr_030_safe_mode_run_leaves_the_registry_byte_identical() {
    let mut rig = rig_with_core_and_two_third_party();
    let registry = rig.data_dir().join("plugins/installed.json");
    let before = std::fs::read(&registry).expect("registry");
    rig.start_server_mode(StartMode::SafePlugins);
    rig.stop_server();
    assert_eq!(std::fs::read(&registry).expect("registry"), before);
}

/// REQ: USR-030
#[test]
fn usr_030_next_normal_start_has_every_plugin_as_before() {
    let mut rig = rig_with_core_and_two_third_party();
    rig.start_server();
    let before = rig.list();
    rig.stop_server();
    rig.start_server_mode(StartMode::SafePlugins);
    rig.stop_server();
    rig.start_server();
    assert_eq!(rig.list(), before);
}

/// REQ: USR-030
#[test]
fn usr_030_a_user_disabled_plugin_stays_disabled_by_user_after_safe_mode() {
    let mut rig = rig_with_core_and_two_third_party();
    let _ = rig.cli(&["disable", "com.example.old"]);
    rig.start_server_mode(StartMode::SafePlugins);
    rig.stop_server();
    rig.start_server();
    assert_eq!(list_row(&mut rig, "com.example.old").state, "Off");
}

// ---- USR-033 ----

/// REQ: USR-033
#[test]
fn usr_033_forget_settings_deletes_only_the_settings_values() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.keeper", "1.0.0", &["tree.read"]).start_writes_file("kept.txt"),
    );
    rig.start_server();
    rig.advance_ms(1_000);
    rig.set_plugin_setting("com.example.keeper", "density", "compact");
    let grants_before = std::fs::read(rig.grants_path()).expect("grants");
    let data_before = tree_snapshot(&rig.data_dir().join("plugins/data/com.example.keeper"));
    let output = rig.cli(&["forget-settings", "com.example.keeper", "--yes"]);
    assert_eq!(result_exit_code(&output), 0);
    assert_eq!(rig.plugin_setting("com.example.keeper", "density"), None);
    assert_eq!(
        std::fs::read(rig.grants_path()).expect("grants"),
        grants_before
    );
    assert_eq!(
        tree_snapshot(&rig.data_dir().join("plugins/data/com.example.keeper")),
        data_before
    );
}

/// REQ: USR-033
#[test]
fn usr_033_forget_settings_leaves_another_plugins_settings() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.keeper", "1.0.0", &[]));
    install_ok(&mut rig, &spec("com.example.other", "1.0.0", &[]));
    rig.set_plugin_setting("com.example.keeper", "density", "compact");
    rig.set_plugin_setting("com.example.other", "density", "wide");
    let _ = rig.cli(&["forget-settings", "com.example.keeper", "--yes"]);
    assert_eq!(
        rig.plugin_setting("com.example.other", "density")
            .as_deref(),
        Some("wide")
    );
}

// ---- USR-100 ----

fn failing_rig() -> UserRig {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.crash", "1.0.0", &[]).activation_traps(),
    );
    rig
}

/// REQ: USR-100
#[test]
fn usr_100_first_restart_after_one_second() {
    let mut rig = failing_rig();
    rig.start_server();
    let starts = rig.instance_starts("com.example.crash");
    rig.advance_ms(999);
    assert_eq!(rig.instance_starts("com.example.crash"), starts, "n-1");
    rig.advance_ms(2);
    assert_eq!(rig.instance_starts("com.example.crash"), starts + 1, "n+1");
}

/// REQ: USR-100
#[test]
fn usr_100_second_restart_after_five_seconds() {
    let mut rig = failing_rig();
    rig.start_server();
    rig.advance_ms(1_001);
    let starts = rig.instance_starts("com.example.crash");
    rig.advance_ms(4_999);
    assert_eq!(rig.instance_starts("com.example.crash"), starts, "n-1");
    rig.advance_ms(2);
    assert_eq!(rig.instance_starts("com.example.crash"), starts + 1, "n+1");
}

/// REQ: USR-100
#[test]
fn usr_100_third_failure_inside_ten_minutes_stays_off() {
    let mut rig = failing_rig();
    rig.start_server();
    rig.advance_ms(1_001);
    rig.advance_ms(5_001);
    let starts = rig.instance_starts("com.example.crash");
    rig.advance_ms(600_000);
    assert_eq!(rig.instance_starts("com.example.crash"), starts);
    assert_eq!(list_row(&mut rig, "com.example.crash").state, "Failed");
}

/// REQ: USR-100
#[test]
fn usr_100_failures_spread_over_more_than_ten_minutes_do_not_stop_the_plugin() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.slowcrash", "1.0.0", &[]));
    rig.start_server();
    for _ in 0..3 {
        rig.inject_trap("com.example.slowcrash");
        rig.advance_ms(600_001);
    }
    assert_ne!(list_row(&mut rig, "com.example.slowcrash").state, "Failed");
}

// ---- USR-101 ----

/// REQ: USR-101
#[test]
fn usr_101_turning_on_after_the_stop_clears_the_count() {
    let mut rig = failing_rig();
    rig.start_server();
    rig.advance_ms(7_000);
    assert_eq!(list_row(&mut rig, "com.example.crash").state, "Failed");
    let _ = rig.cli(&["enable", "com.example.crash"]);
    assert_eq!(rig.failure_count("com.example.crash"), 0);
    rig.advance_ms(10);
    let starts = rig.instance_starts("com.example.crash");
    rig.advance_ms(1_001);
    assert_eq!(
        rig.instance_starts("com.example.crash"),
        starts + 1,
        "first failure restarts after 1 s again"
    );
}

/// REQ: USR-101
#[test]
fn usr_101_a_restart_of_the_server_also_starts_with_a_clear_window() {
    let mut rig = failing_rig();
    rig.start_server();
    rig.advance_ms(7_000);
    rig.restart_server();
    assert_eq!(rig.failure_count("com.example.crash"), 0);
}
