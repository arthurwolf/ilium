//! Core-plugin gap tests for `core-plugin-settings`: while the settings plugin is down, other
//! plugins read the last committed values, writes are refused, and the recovery page opens.

use ilium_plugin_host::testing::ops::OpsRig;

const SETTINGS: &str = "ilium.core.settings";
const READER: &str = "com.example.reader";
const KEY: &str = "font-size";

/// REQ: COR-215
#[test]
fn cor_215_while_settings_is_down_plugins_read_the_last_committed_value_and_writes_are_refused() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    server.write_setting(SETTINGS, KEY, "14");
    server.disable(SETTINGS);
    assert_eq!(
        server.read_setting_value(READER, KEY).as_deref(),
        Some("14"),
        "the last committed value must be served read-only"
    );
    let refusal = server
        .write_setting_checked(SETTINGS, KEY, "16")
        .expect_err("writes must be rejected while settings is down");
    assert!(format!("{refusal:?}").contains("settings.unavailable"));
}

/// REQ: COR-215
#[test]
fn cor_215_while_settings_is_down_the_recovery_page_opens_on_its_bound_key() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    server.disable(SETTINGS);
    server.press_recovery_enable(SETTINGS);
    assert!(server.recovery_page_open(), "the recovery page must open");
}
