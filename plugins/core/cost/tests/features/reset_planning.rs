//! Reset forecasts (F-088) and the Reset planning tab (F-116).

use serde_json::json;

use crate::common::*;
use ilium_plugin_sdk::host::{Bucket, HttpResponse};

const CLAUDE_URL: &str = "https://status.anthropic.com/api/v2/summary.json";
const HOUR_MS: u64 = 3_600_000;

fn monitored_host(status: u16) -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = server_host();
    set_kernel_tab(&mut host, "reset-planning", json!({"monitor_claude": true, "monitor_codex": false, "time_style": "exact"}));
    host.http().expect("GET", CLAUDE_URL, HttpResponse { status, headers: Vec::new(), body: b"{}".to_vec() });
    host.activate::<ilium_plugin_core_cost::CostServer>();
    host.settle();
    host
}

/// REQ: F-088
#[test]
fn reset_088_monitoring_claude_requests_the_status_feed() {
    let mut host = monitored_host(200);
    assert!(host.http().calls().iter().any(|call| call.url == CLAUDE_URL));
}

/// REQ: F-088
#[test]
fn reset_088_request_uses_the_twelve_second_timeout() {
    let mut host = monitored_host(200);
    let call = host.http().calls().into_iter().find(|call| call.url == CLAUDE_URL).expect("call");
    assert_eq!(call.timeout_millis, 12_000);
}

/// REQ: F-088
#[test]
fn reset_088_polling_is_hourly() {
    let mut host = monitored_host(200);
    let count = |host: &mut ilium_plugin_sdk::testing::FakeHost| host.http().calls().iter().filter(|c| c.url == CLAUDE_URL).count();
    let first = count(&mut host);
    host.advance_clock(HOUR_MS);
    host.settle();
    let second = count(&mut host);
    host.advance_clock(HOUR_MS);
    host.settle();
    let third = count(&mut host);
    assert_eq!(second - first, 1);
    assert_eq!(third - second, 1);
}

/// REQ: F-088
#[test]
fn reset_088_no_request_before_the_hour_is_over() {
    let mut host = monitored_host(200);
    let first = host.http().calls().len();
    host.advance_clock(HOUR_MS / 2);
    host.settle();
    assert_eq!(host.http().calls().len(), first);
}

/// REQ: F-088
#[test]
fn reset_088_non_200_logs_a_warning_and_stores_no_forecast() {
    let host = monitored_host(500);
    assert!(host.logs().iter().any(|line| line.message.contains("status feed returned 500")));
    assert!(host.storage().get(Bucket::Global, "forecast/anthropic").is_none());
}

/// REQ: F-088
#[test]
fn reset_088_successful_poll_stores_forecast_anthropic() {
    let mut host = monitored_host(200);
    let stored = host.storage().get(Bucket::Global, "forecast/anthropic");
    assert!(stored.map(|bytes| bytes.len() == 8).unwrap_or(false));
}

/// REQ: F-116
#[test]
fn reset_116_monitors_off_make_no_request() {
    let mut host = server_host();
    set_kernel_tab(&mut host, "reset-planning", json!({"monitor_claude": false, "monitor_codex": false, "time_style": "human"}));
    host.activate::<ilium_plugin_core_cost::CostServer>();
    host.advance_clock(HOUR_MS);
    host.settle();
    assert!(host.http().calls().is_empty());
}

/// REQ: F-116
#[test]
fn reset_116_turning_the_monitor_off_stops_further_requests() {
    let mut host = monitored_host(200);
    set_kernel_tab(&mut host, "reset-planning", json!({"monitor_claude": false, "monitor_codex": false, "time_style": "exact"}));
    host.deliver(vec![ilium_plugin_sdk::types::Event::KernelSettingsChanged(ilium_plugin_sdk::types::KernelSettingsEvent {
        tab: "reset-planning".to_string(),
        keys: vec!["monitor_claude".to_string()],
    })]);
    host.settle();
    let seen = host.http().calls().len();
    host.advance_clock(2 * HOUR_MS);
    host.settle();
    assert_eq!(host.http().calls().len(), seen);
}

/// REQ: F-116
#[test]
fn doc_f_116_user_doc_lists_the_status_strings() {
    let doc = user_doc("automation.md");
    for text in ["On - checking...", "On - reset announced", "On - history only", "On - no scheduled reset"] {
        assert!(doc.contains(text), "{text}");
    }
}

/// REQ: F-116
#[test]
fn doc_f_116_user_doc_names_the_three_keys() {
    let doc = user_doc("automation.md");
    assert!(doc.contains("Reset forecasts"));
}
