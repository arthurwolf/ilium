//! Reset forecasts (F-088) and the Reset planning tab (F-116).
//!
//! The plugin reads the public Claude and Codex reset feeds with the `http` import, one request per
//! provider and hour, and shows the announcement as the status segment `reset-planning`. The
//! fake host never fires a timer by itself, so the tests deliver the plugin's timer by hand.

use serde_json::{json, Value};

use crate::common::*;
use ilium_plugin_sdk::host::{Bucket, HttpResponse};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    Event, Intent, KernelSettingsEvent, RichSpan, SetStatusSegmentSpec, TimerId,
};

const CLAUDE_URL: &str = "https://claude-resets.com/api/resets";
const CODEX_URL: &str = "https://codex-resets.com/api/v1/status";
const SEGMENT: &str = "reset-planning";
const HOUR_MS: u64 = 3_600_000;

fn claude_feed() -> Vec<u8> {
    json!({
        "providers": {"claude": {"events": [
            {"kind": "reset", "resetType": "banked", "date": "2029-12-26T16:44:06Z"},
            {"kind": "policy", "date": "2029-12-14T00:00:00Z"}
        ]}},
        "meta": {"asOf": "2029-12-31T23:44:29Z", "detector": {
            "status": "fresh", "asOf": "2029-12-31T23:44:29Z",
            "lastSuccessfulCheckAt": "2029-12-31T23:43:05Z"
        }}
    })
    .to_string()
    .into_bytes()
}

fn codex_feed(scheduled: Value) -> Vec<u8> {
    json!({
        "data": {"scheduled_reset": scheduled},
        "meta": {"api_version": "v1", "generated_at": "2029-12-31T23:43:37.281Z"}
    })
    .to_string()
    .into_bytes()
}

fn announced_without_time() -> Value {
    json!({
        "id": "2103963215885701493", "status": "scheduled", "reset_type": "regular",
        "announced_at": "2029-12-30T21:41:35.000Z", "scheduled_for": null,
        "source": {"type": "x_post", "author": "thsottiaux",
            "url": "https://x.com/thsottiaux/status/2103963215885701493"}
    })
}

fn answer(status: u16, body: Vec<u8>) -> HttpResponse {
    HttpResponse {
        status,
        headers: Vec::new(),
        body,
    }
}

fn tab(claude: bool, codex: bool, style: &str) -> Value {
    json!({"monitor_claude": claude, "monitor_codex": codex, "time_style": style})
}

/// A host with the given feeds registered, activated at the fake clock start (2030-01-01T00:00:00Z).
fn monitored(settings: Value, feeds: Vec<(&str, HttpResponse)>) -> FakeHost {
    let mut host = server_host();
    set_kernel_tab(&mut host, "reset-planning", settings);
    for (url, response) in feeds {
        host.http().expect("GET", url, response);
    }
    host.activate::<ilium_plugin_core_cost::CostServer>();
    host.settle();
    host
}

fn claude_only(status: u16) -> FakeHost {
    monitored(
        tab(true, false, "exact"),
        vec![(CLAUDE_URL, answer(status, claude_feed()))],
    )
}

fn requests_to(host: &mut FakeHost, url: &str) -> usize {
    host.http()
        .calls()
        .iter()
        .filter(|call| call.url == url)
        .count()
}

/// Moves the clock on and fires the newest timer the plugin armed.
fn pass(host: &mut FakeHost, milliseconds: u64) {
    host.advance_clock(milliseconds);
    let timer = host
        .active_timers()
        .iter()
        .map(|(id, _)| *id)
        .max()
        .expect("the plugin keeps a timer armed while a monitor is on");
    host.deliver(vec![Event::TimerFired(TimerId(u64::from(timer)))]);
    host.settle();
}

/// Every status segment submission of the reset label, as text (`None` is a clear).
fn labels(host: &FakeHost) -> Vec<Option<String>> {
    host.submitted()
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::SetStatusSegment(SetStatusSegmentSpec {
                segment, content, ..
            }) if segment == SEGMENT => Some(Some(
                content
                    .iter()
                    .map(|span: &RichSpan| span.text.as_str())
                    .collect(),
            )),
            Intent::ClearStatusSegment(segment) if segment == SEGMENT => Some(None),
            _ => None,
        })
        .collect()
}

/// REQ: F-088
#[test]
fn reset_088_monitoring_claude_requests_the_feed() {
    let mut host = claude_only(200);
    assert_eq!(requests_to(&mut host, CLAUDE_URL), 1);
    assert_eq!(requests_to(&mut host, CODEX_URL), 0);
}

/// REQ: F-088
#[test]
fn reset_088_request_uses_the_twelve_second_timeout() {
    let mut host = claude_only(200);
    let call = host
        .http()
        .calls()
        .into_iter()
        .find(|call| call.url == CLAUDE_URL)
        .expect("call");
    assert_eq!(call.timeout_millis, 12_000);
}

/// REQ: F-088
#[test]
fn reset_088_polling_is_hourly() {
    let mut host = claude_only(200);
    let first = requests_to(&mut host, CLAUDE_URL);
    pass(&mut host, HOUR_MS);
    let second = requests_to(&mut host, CLAUDE_URL);
    pass(&mut host, HOUR_MS);
    let third = requests_to(&mut host, CLAUDE_URL);
    assert_eq!(second - first, 1);
    assert_eq!(third - second, 1);
}

/// REQ: F-088
#[test]
fn reset_088_no_request_before_the_hour_is_over() {
    let mut host = claude_only(200);
    let first = host.http().calls().len();
    pass(&mut host, HOUR_MS / 2);
    assert_eq!(host.http().calls().len(), first);
}

/// REQ: F-088
#[test]
fn reset_088_a_rate_limit_delays_the_next_request_past_the_hour() {
    let mut host = monitored(
        tab(true, false, "exact"),
        vec![(
            CLAUDE_URL,
            HttpResponse {
                status: 429,
                headers: vec![ilium_plugin_sdk::types::Header {
                    name: "Retry-After".to_string(),
                    value: "7200".to_string(),
                }],
                body: Vec::new(),
            },
        )],
    );
    assert_eq!(requests_to(&mut host, CLAUDE_URL), 1);
    pass(&mut host, HOUR_MS + 1);
    assert_eq!(requests_to(&mut host, CLAUDE_URL), 1);
    pass(&mut host, HOUR_MS);
    assert_eq!(requests_to(&mut host, CLAUDE_URL), 2);
}

/// REQ: F-088
#[test]
fn reset_088_non_200_logs_a_warning_and_shows_nothing() {
    let host = claude_only(500);
    assert!(host.logs().iter().any(
        |line| line.message.contains("Claude reset feed check failed")
            && line.message.contains("HTTP 500")
    ));
    assert!(labels(&host).is_empty());
    assert!(host
        .storage()
        .get(Bucket::Global, "forecast/anthropic")
        .is_none());
}

/// REQ: F-088
#[test]
fn reset_088_a_healthy_claude_feed_has_no_countdown() {
    let host = claude_only(200);
    assert!(labels(&host).is_empty());
    assert!(!host
        .logs()
        .iter()
        .any(|line| line.message.contains("check failed")));
}

/// REQ: F-088
#[test]
fn reset_088_an_announcement_without_a_time_shows_time_tbd() {
    let host = monitored(
        tab(false, true, "exact"),
        vec![(CODEX_URL, answer(200, codex_feed(announced_without_time())))],
    );
    assert_eq!(
        labels(&host),
        vec![Some("Codex reset scheduled · time TBD".to_string())]
    );
}

/// REQ: F-088
#[test]
fn reset_088_a_timed_announcement_counts_down_every_second() {
    let mut event = announced_without_time();
    event["scheduled_for"] = json!("2030-01-01T01:00:00Z");
    let mut host = monitored(
        tab(false, true, "exact"),
        vec![(CODEX_URL, answer(200, codex_feed(event)))],
    );
    assert_eq!(
        labels(&host).last().cloned().flatten().as_deref(),
        Some("Codex reset in 1h 0m 0s")
    );
    pass(&mut host, 1_000);
    assert_eq!(
        labels(&host).last().cloned().flatten().as_deref(),
        Some("Codex reset in 59m 59s")
    );
}

/// REQ: F-116
#[test]
fn reset_116_monitors_off_make_no_request() {
    let mut host = monitored(tab(false, false, "human"), Vec::new());
    host.advance_clock(HOUR_MS);
    host.settle();
    assert!(host.http().calls().is_empty());
    assert!(host.active_timers().is_empty());
}

/// REQ: F-116
#[test]
fn reset_116_turning_the_monitor_off_stops_further_requests_and_clears_the_label() {
    let mut host = monitored(
        tab(false, true, "exact"),
        vec![(CODEX_URL, answer(200, codex_feed(announced_without_time())))],
    );
    assert_eq!(labels(&host).len(), 1);
    set_kernel_tab(&mut host, "reset-planning", tab(false, false, "exact"));
    host.deliver(vec![Event::KernelSettingsChanged(KernelSettingsEvent {
        tab: "reset-planning".to_string(),
        keys: vec!["monitor_codex".to_string()],
    })]);
    host.settle();
    assert_eq!(labels(&host).last(), Some(&None));
    let seen = host.http().calls().len();
    host.advance_clock(2 * HOUR_MS);
    host.settle();
    assert_eq!(host.http().calls().len(), seen);
    assert!(host.active_timers().is_empty());
}

/// REQ: F-116
#[test]
fn reset_116_switching_a_monitor_on_reads_its_feed_at_once() {
    let mut host = monitored(tab(false, false, "exact"), Vec::new());
    host.http()
        .expect("GET", CLAUDE_URL, answer(200, claude_feed()));
    set_kernel_tab(&mut host, "reset-planning", tab(true, false, "exact"));
    host.deliver(vec![Event::KernelSettingsChanged(KernelSettingsEvent {
        tab: "reset-planning".to_string(),
        keys: vec!["monitor_claude".to_string()],
    })]);
    host.settle();
    assert_eq!(requests_to(&mut host, CLAUDE_URL), 1);
}

/// REQ: F-116
#[test]
fn reset_116_the_tab_has_the_two_monitor_toggles_and_the_time_choice() {
    let manifest = manifest_value();
    let tab = manifest["contributes"]["settings"]
        .as_array()
        .expect("settings")
        .iter()
        .find(|item| item["id"] == "reset-planning")
        .expect("reset-planning tab");
    assert_eq!(tab["kind"], "tab");
    assert_eq!(tab["title"], "Reset planning");
    let rows = tab["sections"][0]["rows"].as_array().expect("rows");
    let keys: Vec<&str> = rows
        .iter()
        .map(|row| row["kernel_key"].as_str().expect("kernel key"))
        .collect();
    assert_eq!(keys, ["monitor_claude", "monitor_codex", "time_style"]);
    assert!(rows.iter().all(|row| row["kernel_tab"] == "reset-planning"));
}

/// REQ: F-116
#[test]
fn doc_f_116_user_doc_names_the_labels() {
    let doc = user_doc("automation.md");
    for text in [
        "time TBD",
        "awaiting confirmation",
        "Possible Codex reset watch window ends in",
    ] {
        assert!(doc.contains(text), "{text}");
    }
}

/// REQ: F-116
#[test]
fn doc_f_116_user_doc_names_the_three_keys() {
    let doc = user_doc("automation.md");
    assert!(doc.contains("Reset forecasts"));
    for key in ["monitor_claude", "monitor_codex", "time_style"] {
        assert!(doc.contains(key), "{key}");
    }
}
