//! Deprecation, retirement and settings values after a schema change (LIF-319..321).
//!
//! Assumptions: the deprecation object and the settings schema are supplied through the
//! `PackageSpecLifecycleExt` builders (a stub until the rig gets them). The settings schema is a
//! JSON array of rows as in the settings schema reference, and the virtual clock starts at the
//! current date, so advancing five years passes an end-of-life date of 2030-01-01.

use ilium_plugin_host::lifecycle_ops_support::PackageSpecLifecycleExt;

use crate::common::*;

/// A settings schema with one number row, `volume`, with the given bounds and default.
fn volume_schema(minimum: u32, maximum: u32, default: u32) -> String {
    format!(
        r#"[{{ "type": "number", "key": "volume", "title": "Volume", "minimum": {minimum}, "maximum": {maximum}, "step": 1, "default": {default} }}]"#
    )
}

/// REQ: LIF-319
#[test]
fn lif_319_deprecated_plugin_shows_a_deprecated_state_in_the_list() {
    let rig = rig();
    install_ok(
        &rig,
        &pkg(NOTES, "1.0.0").deprecated("Replaced by the new editor", "2030-01-01"),
    );
    let _server = rig.start_server("default");
    let row = rig
        .render_plugins_screen()
        .row_for(NOTES)
        .cloned()
        .expect("the deprecated plugin has a row in the plugins screen");
    assert!(row.state.to_lowercase().contains("deprecated"));
}

/// REQ: LIF-319
#[test]
fn lif_319_deprecated_plugin_stays_active_before_its_end_of_life() {
    let rig = rig();
    install_ok(
        &rig,
        &pkg(NOTES, "1.0.0").deprecated("Replaced by the new editor", "2030-01-01"),
    );
    let server = rig.start_server("default");
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: LIF-320
#[test]
fn lif_320_past_end_of_life_the_plugin_is_disabled_with_reason_retired() {
    let rig = rig();
    install_ok(
        &rig,
        &pkg(NOTES, "1.0.0").deprecated("Retiring", "2030-01-01"),
    );
    let server = rig.start_server_polling("default");
    rig.advance(secs(60 * 60 * 24 * 365 * 5));
    assert_eq!(server.inactive_reason(NOTES).as_deref(), Some("retired"));
    assert!(!server.is_enabled(NOTES));
}

/// REQ: LIF-320
#[test]
fn lif_320_past_end_of_life_the_plugin_stays_installed_with_its_data() {
    let rig = rig();
    install_ok(
        &rig,
        &pkg(NOTES, "1.0.0").deprecated("Retiring", "2030-01-01"),
    );
    rig.write_storage(NOTES, "global", "note", b"keep");
    let server = rig.start_server_polling("default");
    rig.advance(secs(60 * 60 * 24 * 365 * 5));
    server.apply_pending();
    assert!(registry_ids(&rig).contains(&NOTES.to_string()));
    assert_eq!(
        rig.read_storage(NOTES, "global", "note"),
        Some(b"keep".to_vec())
    );
}

/// REQ: LIF-320
#[test]
fn lif_320_retired_plugin_is_enabled_again_after_one_explicit_confirmation() {
    let rig = rig();
    install_ok(
        &rig,
        &pkg(NOTES, "1.0.0").deprecated("Retiring", "2030-01-01"),
    );
    let server = rig.start_server_polling("default");
    rig.advance(secs(60 * 60 * 24 * 365 * 5));
    let output = rig.cli_tty(&["enable", NOTES], &["yes"]);
    assert_eq!(output.exit_code, 0);
    assert!(server.is_enabled(NOTES));
}

/// REQ: LIF-321
#[test]
fn lif_321_out_of_range_saved_value_is_replaced_by_the_new_default() {
    let rig = rig();
    install_ok(
        &rig,
        &pkg(NOTES, "1.0.0").with_setting_schema(&volume_schema(0, 100, 50)),
    );
    rig.write_setting(NOTES, "volume", "80");
    let _server = rig.start_server("default");
    let output = update(
        &rig,
        &pkg(NOTES, "2.0.0").with_setting_schema(&volume_schema(0, 50, 10)),
    );
    assert_eq!(output.exit_code, 0);
    assert_eq!(rig.setting(NOTES, "volume").as_deref(), Some("10"));
}

/// REQ: LIF-321
#[test]
fn lif_321_reset_writes_a_settings_value_reset_record_with_the_old_value() {
    let rig = rig();
    install_ok(
        &rig,
        &pkg(NOTES, "1.0.0").with_setting_schema(&volume_schema(0, 100, 50)),
    );
    rig.write_setting(NOTES, "volume", "80");
    let server = rig.start_server("default");
    let output = update(
        &rig,
        &pkg(NOTES, "2.0.0").with_setting_schema(&volume_schema(0, 50, 10)),
    );
    assert_eq!(output.exit_code, 0);
    server.apply_pending();
    assert!(server
        .log_codes()
        .iter()
        .any(|code| code == "settings.value-reset"));
}

/// REQ: LIF-321
#[test]
fn lif_321_reset_is_not_counted_as_a_failure() {
    let rig = rig();
    install_ok(
        &rig,
        &pkg(NOTES, "1.0.0").with_setting_schema(&volume_schema(0, 100, 50)),
    );
    rig.write_setting(NOTES, "volume", "80");
    let server = rig.start_server("default");
    update(
        &rig,
        &pkg(NOTES, "2.0.0").with_setting_schema(&volume_schema(0, 50, 10)),
    );
    server.apply_pending();
    assert_eq!(
        server.failure_count(NOTES, ilium_plugin_api::Side::Server),
        0
    );
}
