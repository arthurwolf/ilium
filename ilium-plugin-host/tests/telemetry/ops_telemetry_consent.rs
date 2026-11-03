//! Requirements OPS-101..108: the telemetry consent of a plugin, its audit records,
//! the administrator policy and the install review (ops-telemetry-consent.md).

use std::fs;
use std::path::{Path, PathBuf};

use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};
use serde_json::json;

const PLUGIN: &str = "com.example.telemetry";

/// Every file below `root`, sorted.
fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Installs the telemetry fixture plugin through the CLI and returns the rig.
fn install_fixture(rig: &OpsRig) {
    let spec = PackageSpec::new(PLUGIN, "1.0.0");
    let path = rig.write_package(&spec);
    let _ = rig.cli(&["plugin", "install", path.to_str().unwrap_or("")]);
}

/// REQ: OPS-101
#[test]
fn ops_101_no_telemetry_before_user_consent_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.block_network();
    install_fixture(&rig);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "send"]);
    assert_eq!(rig.network_attempts(), 0, "no connection is opened before the user consents");
}

/// REQ: OPS-101
#[test]
fn ops_101_new_install_starts_not_asked_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_fixture(&rig);
    let grants = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    assert!(grants.contains("not asked"), "a new install starts in the state not-asked");
}

/// REQ: OPS-102
#[test]
fn ops_102_reinstall_starts_not_asked_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_fixture(&rig);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let _ = rig.cli(&["plugin", "uninstall", PLUGIN]);
    install_fixture(&rig);
    let grants = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    assert!(grants.contains("not asked"), "a removed and installed plugin starts in not-asked");
}

/// REQ: OPS-102
#[test]
fn ops_102_three_states_are_the_only_states_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_fixture(&rig);
    let initial = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let granted = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "decline"]);
    let declined = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    assert!(initial.contains("not asked"));
    assert!(granted.contains("granted"));
    assert!(declined.contains("declined"));
}

/// REQ: OPS-103
#[test]
fn ops_103_consent_is_in_user_consent_store_under_telemetry_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_fixture(&rig);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let store = rig.grants_json().to_string();
    assert!(store.contains(PLUGIN), "the consent is stored under the plugin id");
    assert!(store.contains("telemetry"), "the consent is stored under the key telemetry");
}

/// REQ: OPS-103
#[test]
fn ops_103_consent_is_not_in_plugin_data_folder_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_fixture(&rig);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let in_plugin_folder = files_under(&rig.data_dir(PLUGIN))
        .into_iter()
        .filter(|path| {
            fs::read_to_string(path)
                .map(|text| text.contains("\"telemetry\""))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(in_plugin_folder, 0, "the plugin's own data folder holds no consent");
}

/// REQ: OPS-104
#[test]
fn ops_104_each_change_writes_one_audit_record_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_fixture(&rig);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "withdraw"]);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let given = rig.audit_records("consent-given").len();
    let withdrawn = rig.audit_records("consent-withdrawn").len();
    assert_eq!(given + withdrawn, 3, "three changes write three audit records");
}

/// REQ: OPS-104
#[test]
fn ops_104_audit_record_names_plugin_state_actor_and_time_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_fixture(&rig);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let records = rig.audit_records("consent-given");
    let record = records.last().cloned().unwrap_or(json!(null));
    let text = record.to_string();
    assert!(text.contains(PLUGIN), "the record names the plugin id");
    assert!(text.contains("granted"), "the record names the new state");
    assert!(text.contains("actor"), "the record names the actor");
    assert!(text.contains("time"), "the record names the time");
}

/// REQ: OPS-105
#[test]
fn ops_105_withdrawn_consent_stops_next_send_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_fixture(&rig);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "send"]);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "withdraw"]);
    let before_second_send = rig.network_attempts();
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "send"]);
    assert_eq!(
        rig.network_attempts(),
        before_second_send,
        "the send after the withdrawal does not reach the endpoint"
    );
}

/// REQ: OPS-105
#[test]
fn ops_105_plugin_reads_new_state_after_withdrawal_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_fixture(&rig);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "withdraw"]);
    let state = rig.cli(&["plugin", "telemetry", PLUGIN, "state"]).stdout_raw;
    assert!(
        !state.contains("granted"),
        "the plugin reads the withdrawn state, never a stale granted value"
    );
}

/// REQ: OPS-106
#[test]
fn ops_106_policy_blocks_send_even_after_grant_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.write_policy("[telemetry]\nplugins = \"off\"\n");
    rig.reload_policy();
    install_fixture(&rig);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let before = rig.network_attempts();
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "send"]);
    assert_eq!(rig.network_attempts(), before, "the blocking policy stops the send");
}

/// REQ: OPS-106
#[test]
fn ops_106_policy_file_holds_no_consent_value_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.write_policy("[telemetry]\nplugins = \"off\"\n");
    install_fixture(&rig);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let policy = fs::read_to_string(rig.config_dir().join("policy.toml")).unwrap_or_default();
    assert!(!policy.contains("granted"), "the policy file holds no consent value");
    assert!(!policy.contains(PLUGIN), "the policy does not set a consent for the plugin");
}

/// REQ: OPS-107
#[test]
fn ops_107_accepting_review_keeps_telemetry_not_asked_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let spec = PackageSpec::new(PLUGIN, "1.0.0");
    let path = rig.write_package(&spec);
    let _ = rig.cli_tty(&["plugin", "install", path.to_str().unwrap_or("")], &["y", "y"]);
    let grants = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    assert!(grants.contains("not asked"), "accepting the review does not set the consent");
}

/// REQ: OPS-107
#[test]
fn ops_107_review_network_grant_leaves_store_untouched_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let spec = PackageSpec::new(PLUGIN, "1.0.0");
    let path = rig.write_package(&spec);
    let _ = rig.cli_tty(&["plugin", "install", path.to_str().unwrap_or("")], &["y", "y"]);
    let store = rig.grants_json().to_string();
    assert!(!store.contains("granted"), "the grant store holds no telemetry grant after the review");
}

/// REQ: OPS-108
#[test]
fn ops_108_settings_and_grants_show_not_asked_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_fixture(&rig);
    let grants = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    let settings = rig.cli(&["plugin", "settings", PLUGIN]).stdout_raw;
    assert!(grants.contains("not asked"), "ilium plugin grants shows not asked");
    assert!(settings.contains("not asked"), "the settings page shows not asked");
}

/// REQ: OPS-108
#[test]
fn ops_108_settings_and_grants_show_granted_and_declined_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_fixture(&rig);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let granted = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "decline"]);
    let declined = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    assert!(granted.contains("granted"));
    assert!(declined.contains("declined"));
}
