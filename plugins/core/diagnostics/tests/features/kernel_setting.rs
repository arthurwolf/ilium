//! The read-only `debug` kernel setting (page sample "Read the Debug setting without writing it").

use ilium_plugin_sdk::types::{Event, KernelSettingsEvent};
use serde_json::json;

use crate::common::{LOG_SCOPE, PLUGIN_ID, log_line, session, warnings_panel, write_log};

fn changed(tab: &str, keys: &[&str]) -> Event {
    Event::KernelSettingsChanged(KernelSettingsEvent { tab: tab.to_string(), keys: keys.iter().map(|key| key.to_string()).collect() })
}

/// REQ: COR-050
#[test]
fn cor_050_the_debug_tab_is_read_with_the_read_only_grant_not_a_write_grant() {
    let manifest = crate::common::manifest_json();
    let server: Vec<&str> = manifest["capabilities"]["server"].as_array().unwrap().iter().map(|grant| grant.as_str().unwrap()).collect();
    assert!(server.contains(&"settings.read-kernel:debug"));
    assert!(server.iter().all(|grant| !grant.starts_with("settings.write")));
}

/// REQ: COR-050
#[test]
fn cor_050_a_change_of_file_logging_enabled_is_handled_without_any_write_intent() {
    let mut session = session();
    session.server(PLUGIN_ID).settings().set("debug.file_logging_enabled", json!(true)).expect("seed");
    let submitted = session.server(PLUGIN_ID).deliver_one(changed("debug", &["file_logging_enabled"]));
    assert!(submitted.iter().all(|submission| !format!("{:?}", submission.intent).to_lowercase().contains("setting")), "{submitted:?}");
    assert_eq!(session.server(PLUGIN_ID).settings().get("debug.file_logging_enabled"), Some(json!(true)));
}

/// REQ: COR-050
#[test]
fn cor_050_a_change_of_another_tab_is_ignored() {
    let mut session = session();
    let submitted = session.server(PLUGIN_ID).deliver_one(changed("sound", &["file_logging_enabled"]));
    assert!(submitted.is_empty(), "{submitted:?}");
}

/// REQ: COR-050
#[test]
fn cor_050_a_change_of_another_key_of_the_debug_tab_is_ignored() {
    let mut session = session();
    let submitted = session.server(PLUGIN_ID).deliver_one(changed("debug", &["something_else"]));
    assert!(submitted.is_empty(), "{submitted:?}");
}

/// REQ: COR-050
#[test]
fn cor_050_after_the_switch_changes_the_log_is_read_again_from_the_start() {
    let mut session = session();
    let name = "log-2026-10-12 09-14-03.txt";
    write_log(&mut session, name, log_line("WARN", "before switch").as_bytes());
    session.pump().expect("settles");
    write_log(&mut session, "log-2026-10-12 10-00-00.txt", log_line("WARN", "after switch").as_bytes());
    session.server(PLUGIN_ID).deliver_one(changed("debug", &["file_logging_enabled"]));
    session.pump().expect("settles");
    let panel = warnings_panel(&mut session, 100, 20);
    assert!(panel.contains("after switch"), "{panel}");
    let _ = LOG_SCOPE;
}
