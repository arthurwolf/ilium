//! Twin of the cost Budget `*` dialog test of ilium-client/src/keys.rs: a failed durable save keeps the exact
//! number, and a retry after the repair writes it. The dialog becomes the `budget` row of the Cost settings
//! page (assumed action key `budget`, Changed with a number) and the failure an injected `kernel-settings.write`.

use std::path::PathBuf;

use ilium_plugin_core_cost::CostClient;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ErrorCode, PluginError, ViewTarget};
use serde_json::json;

/// REQ: F-040, PORT:ilium-client/src/keys.rs::cost_star_retains_exact_number_after_disk_failure_then_retries_durably
#[test]
fn port_value_cost_star_retains_the_exact_number_after_a_disk_failure_then_retries_durably() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest)
        .expect("manifest loads")
        .side("client")
        .build();
    host.kernel_settings()
        .set_tab("cost", json!({ "calibration": "budget" }));
    host.activate::<CostClient>();
    let page = ViewTarget::SettingsPage("cost".to_string());
    host.inject_failure(
        "kernel-settings.write",
        PluginError {
            code: ErrorCode::Io,
            message: "disk full".to_string(),
            retry_after_ms: None,
        },
    );
    host.send_action(
        page.clone(),
        "budget",
        ActionKind::Changed,
        ActionValue::Number(17.125),
    );
    host.settle();
    assert_eq!(
        host.kernel_settings().tab("cost"),
        Some(json!({ "calibration": "budget" }))
    );
    let shown = host.render(page.clone(), 80, 30).text();
    assert!(
        shown.contains("17.125"),
        "the exact number stays visible: {shown}"
    );
    assert!(host.last_notice().is_some(), "the failure is reported");
    host.send_action(
        page,
        "budget",
        ActionKind::Changed,
        ActionValue::Number(17.125),
    );
    host.settle();
    assert_eq!(
        host.kernel_settings().tab("cost"),
        Some(json!({ "calibration": "budget", "budget_usd": 17.125 }))
    );
}
