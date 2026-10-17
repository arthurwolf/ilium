//! Twins of the debug logging switch tests (`ilium-client/src/{app,debug_logging}.rs`): the plugin
//! only reads the `debug` kernel tab, so the observable rule is that changes never make it write.

use ilium_plugin_sdk::types::{Event, KernelSettingsEvent};
use serde_json::json;

use crate::common::server_host;

fn changed() -> Event {
    Event::KernelSettingsChanged(KernelSettingsEvent { tab: "debug".to_string(), keys: vec!["file_logging_enabled".to_string()] })
}

/// REQ: PORT:ilium-client/src/app.rs::debug_logging_toggle_persists_and_requests_live_reconciliation
#[test]
fn port_002_rapid_toggles_end_on_the_last_value_and_write_nothing() {
    let mut host = server_host();
    for value in [true, false, true, false] {
        host.kernel_settings().set("debug", "file_logging_enabled", json!(value)).expect("seed");
        let submitted = host.deliver_one(changed());
        assert_eq!(submitted.len(), 0, "{submitted:?}");
    }
    assert_eq!(host.kernel_settings().get("debug", "file_logging_enabled"), Some(json!(false)));
}

/// REQ: PORT:ilium-client/src/debug_logging.rs::ordered_transitions_are_bounded_and_server_sync_never_becomes_client_origin
#[test]
fn port_028_a_server_sync_of_the_switch_is_handled_without_echoing_a_change_back() {
    let mut host = server_host();
    host.kernel_settings().set("debug", "file_logging_enabled", json!(true)).expect("seed");
    for _ in 0..64 {
        let submitted = host.deliver_one(changed());
        assert_eq!(submitted.len(), 0, "{submitted:?}");
    }
    assert_eq!(host.kernel_settings().get("debug", "file_logging_enabled"), Some(json!(true)));
}
