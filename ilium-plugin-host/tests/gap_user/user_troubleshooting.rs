//! Incompatible plugins, update offers and bisect of the page `user-troubleshooting`
//! (USR-254, USR-255, USR-256, USR-346).

use crate::common::*;

/// REQ: USR-254
#[test]
fn usr_254_api_incompatible_plugin_stays_installed_as_incompatible_and_is_not_loaded() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").api_version("2.0"));
    rig.set_host_api_version("1.0");
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let row = server
        .render_plugins_screen()
        .row_for(NOTES)
        .expect("row for notes")
        .clone();
    assert_eq!(row.state, "Incompatible");
    assert_eq!(
        server.active_version(NOTES),
        None,
        "an incompatible plugin must not run"
    );
    assert!(
        rig.cli(&["list"]).stdout_raw.contains(NOTES),
        "the plugin must stay installed"
    );
}

/// REQ: USR-255
#[test]
fn usr_255_incompatible_notice_names_api_range_and_three_actions_with_one_state_word() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").api_version("2.0"));
    rig.set_host_api_version("1.0");
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let notice = server.notices().join("\n");
    assert!(
        notice.contains(NOTES),
        "notice must name the plugin: {notice}"
    );
    assert!(
        notice.contains("2.0"),
        "notice must name the plugin API version: {notice}"
    );
    assert!(
        notice.contains("1.0"),
        "notice must name the host range: {notice}"
    );
    for action in ["Check for update", "Keep disabled", "Uninstall"] {
        assert!(notice.contains(action), "missing action {action}: {notice}");
    }
    let word = server
        .render_plugins_screen()
        .row_for(NOTES)
        .expect("row")
        .state
        .to_string();
    assert!(
        rig.cli(&["list"]).stdout_raw.contains(&word),
        "CLI must show {word}"
    );
    assert!(
        server.render_recovery_page().contains(&word),
        "recovery page must show {word}"
    );
}

/// REQ: USR-256
#[test]
fn usr_256_compatible_update_is_offered_first_and_keeps_settings_and_grants() {
    let rig = rig();
    install_ok(
        &rig,
        &pkg(NOTES, "1.0.0").api_version("2.0").grant("fs.read"),
    );
    rig.set_host_api_version("1.0");
    rig.plant_settings(NOTES, "theme", "dark");
    let grants_before = rig.grants_json();
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    rig.publish_to_registry(&pkg(NOTES, "1.1.0").api_version("1.0"));
    let offered = server.offered_updates(NOTES);
    assert_eq!(
        offered.first().map(String::as_str),
        Some("1.1.0"),
        "update not offered first"
    );
    let accepted = rig.cli(&["update", NOTES, "--yes"]);
    assert_eq!(
        accepted.exit_code, 0,
        "accepting the update failed: {}",
        accepted.stderr
    );
    rig.advance(secs(3));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.1.0"));
    assert_eq!(rig.setting(NOTES, "theme").as_deref(), Some("dark"));
    assert_eq!(
        rig.grants_json(),
        grants_before,
        "grants changed through the update"
    );
}

/// REQ: USR-346
#[test]
fn usr_346_bisect_narrows_to_one_plugin_in_log_rounds_and_restores_the_enabled_set() {
    let rig = rig();
    let ids: Vec<String> = (0..8)
        .map(|index| format!("com.example.p{index}"))
        .collect();
    for id in &ids {
        install_ok(&rig, &pkg(id, "1.0.0"));
    }
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let output = rig.cli_tty(&["bisect"], &["no", "no", "no", "no", "no"]);
    let asked_gone = output.stdout_raw.contains("Is the problem gone?")
        || output.stderr.contains("Is the problem gone?");
    assert!(asked_gone, "bisect must ask 'Is the problem gone?'");
    assert!(
        rig.prompt_count() <= 4,
        "ceil(log2 8)+1 = 4 rounds at most, got {}",
        rig.prompt_count()
    );
    for id in &ids {
        assert!(server.is_enabled(id), "{id} was not restored after bisect");
    }
}
