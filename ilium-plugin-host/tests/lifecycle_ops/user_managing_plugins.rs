//! Managing plugins (LIF-190..193).

use ilium_plugin_api::Side;

use crate::common::*;

/// REQ: LIF-190
#[test]
fn lif_190_plugins_screen_shows_state_and_health_for_each_installed_plugin() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    server.fail(NOTES, Side::Server);
    let screen = server.render_plugins_screen();
    let notes_row = screen.row_for(NOTES).expect("row for notes");
    let other_row = screen.row_for(OTHER).expect("row for other");
    assert_eq!(notes_row.state, "active");
    assert_ne!(notes_row.health, other_row.health);
}

/// REQ: LIF-190
#[test]
fn lif_190_detail_view_has_the_five_tabs() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    let detail = server.render_plugin_detail(NOTES);
    assert_eq!(detail.tabs().len(), 5);
}

/// REQ: LIF-190
#[test]
fn lif_190_every_listed_plugin_has_a_row() {
    let rig = rig();
    for index in 0..5 {
        install_ok(&rig, &pkg(&format!("com.example.p{index}"), "1.0.0"));
    }
    let server = rig.start_server("s1");
    assert_eq!(server.render_plugins_screen().rows().len(), 5);
}

/// REQ: LIF-191
#[test]
fn lif_191_enable_on_a_failed_plugin_clears_the_failure_window() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    server.fail(NOTES, Side::Server);
    rig.advance(secs(2));
    server.fail(NOTES, Side::Server);
    rig.advance(secs(2));
    rig.cli(&["enable", NOTES]);
    rig.advance(secs(3));
    server.fail(NOTES, Side::Server);
    rig.advance(secs(6));
    server.fail(NOTES, Side::Server);
    rig.advance(secs(6));
    assert!(!server.is_disabled_by_failure(NOTES));
    assert_eq!(server.failure_count(NOTES, Side::Server), 2);
}

/// REQ: LIF-191
#[test]
fn lif_191_without_the_enable_the_third_failure_would_disable_it() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    for _ in 0..3 {
        server.fail(NOTES, Side::Server);
        rig.advance(secs(6));
    }
    assert!(server.is_disabled_by_failure(NOTES));
}

/// REQ: LIF-191
#[test]
fn lif_191_enable_on_a_disabled_by_failure_plugin_activates_it_again() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    for _ in 0..3 {
        server.fail(NOTES, Side::Server);
        rig.advance(secs(6));
    }
    rig.cli(&["enable", NOTES]);
    rig.advance(secs(3));
    assert!(!server.is_disabled_by_failure(NOTES));
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
}

/// REQ: LIF-192
#[test]
fn lif_192_replacement_disabled_by_failure_re_enables_the_built_in_with_a_notice() {
    let rig = rig();
    let server = rig.start_server("s1");
    rig.install_replacement_of_core(&pkg("com.example.replace", "1.0.0"), "ilium.core.tree");
    rig.advance(secs(3));
    for _ in 0..3 {
        server.fail("com.example.replace", Side::Server);
        rig.advance(secs(6));
    }
    assert!(server.is_disabled_by_failure("com.example.replace"));
    assert!(server.is_enabled("ilium.core.tree"));
    assert!(server
        .notices()
        .iter()
        .any(|text| text.contains("ilium.core.tree")));
}

/// REQ: LIF-192
#[test]
fn lif_192_replacement_failing_twice_keeps_the_replacement() {
    let rig = rig();
    let server = rig.start_server("s1");
    rig.install_replacement_of_core(&pkg("com.example.replace", "1.0.0"), "ilium.core.tree");
    rig.advance(secs(3));
    for _ in 0..2 {
        server.fail("com.example.replace", Side::Server);
        rig.advance(secs(6));
    }
    assert!(!server.is_disabled_by_failure("com.example.replace"));
    assert!(server.replaces("com.example.replace", "ilium.core.tree"));
}

/// REQ: LIF-193
#[test]
fn lif_193_plugin_blocked_by_policy_shows_blocked_by_policy_with_the_code_and_keeps_data() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_data(NOTES, 1);
    rig.write_policy(&format!("[plugins]\ndeny_ids = [\"{NOTES}\"]\n"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let plugins_screen = server.render_plugins_screen();
    let row = plugins_screen.row_for(NOTES).expect("row");
    assert!(row.text.contains("Blocked by policy"));
    assert!(row.text.contains("policy.denied"));
    assert!(rig.data_dir(NOTES).exists());
    assert_eq!(server.active_version(NOTES), None);
}

/// REQ: LIF-193
#[test]
fn lif_193_removing_the_id_from_the_policy_lets_the_plugin_run_again() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.write_policy(&format!("[plugins]\ndeny_ids = [\"{NOTES}\"]\n"));
    let server = rig.start_server("s1");
    rig.write_policy("[plugins]\ndeny_ids = []\n");
    server.reload_policy();
    rig.advance(secs(3));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: LIF-193
#[test]
fn lif_193_policy_block_keeps_grants_and_settings() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read:project"));
    rig.plant_settings(NOTES, "theme", "dark");
    let grants = rig.grants_json();
    rig.write_policy(&format!("[plugins]\ndeny_ids = [\"{NOTES}\"]\n"));
    let _server = rig.start_server("s1");
    rig.advance(secs(3));
    assert_eq!(rig.grants_json(), grants);
    assert_eq!(rig.setting(NOTES, "theme").as_deref(), Some("dark"));
}
