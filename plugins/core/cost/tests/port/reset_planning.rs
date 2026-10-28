//! Twins of ilium-client/src/reset_planning.rs: provider reset-window feeds, countdown texts, the monitor
//! state machine and the bounded fetch.
//!
//! Feeds travel as JSON strings. `reset.parse-codex-status` / `reset.parse-claude-feed` take
//! `{feed, now, max_age_secs}` and answer `{ok:{scheduled, active_watch}}` or `{err}`; `reset.countdown-text`
//! takes `{provider, scheduled, style, now}`; `reset.monitor-apply` runs a list of monitor events through the
//! state machine and answers the final provider state plus one snapshot per event. `now` is
//! 2026-09-27T04:00:00Z unless stated (the fixture clock of the original tests).

use serde_json::{json, Value};

use crate::common::client;

const NOW: &str = "2026-09-27T04:00:00Z";
const CODEX_STATUS_URL: &str = "https://codexstatus.example/api/v1/status";
const CODEX_LEGACY_URL: &str = "https://codexstatus.example/api/legacy";

fn ask(op: &str, request: Value) -> Value {
    client(&format!("reset.{op}"), request)
}

fn pending() -> Value {
    json!({
        "reset_type": "scheduled", "state": "pending", "announced_at": "2026-09-26T10:00:00Z",
        "source": { "type": "tweet", "url": "https://x.com/thsottiaux/status/1" },
    })
}

fn v1(event: Value) -> Value {
    json!({ "schema": "v1", "generated_at": NOW, "events": if event.is_null() { json!([]) } else { json!([event]) } })
}

fn v1_with_watch(event: Value, watch: Value) -> Value {
    let mut feed = v1(event);
    feed["active_watch"] = watch;
    feed
}

fn parse_codex(feed: Value, now: &str, max_age_secs: u64) -> Value {
    ask(
        "parse-codex-status",
        json!({ "feed": feed.to_string(), "now": now, "max_age_secs": max_age_secs }),
    )
}

fn parse_scheduled(feed: Value) -> Value {
    ask(
        "parse-scheduled",
        json!({ "feed": feed.to_string(), "now": NOW }),
    )
}

fn countdown(scheduled: Value, style: &str, now: &str) -> Value {
    ask(
        "countdown-text",
        json!({ "provider": "codex", "scheduled": scheduled, "style": style, "now": now }),
    )
}

fn claude_feed() -> Value {
    json!({
        "meta": { "detector": { "lastSuccessfulCheckAt": "2026-09-27T03:30:00Z" } },
        "providers": { "claude": { "scheduled_reset": null, "events": [{ "kind": "history" }, { "kind": "banked_grant" }] } },
    })
}

fn parse_claude(feed: Value) -> Value {
    ask(
        "parse-claude-feed",
        json!({ "feed": feed.to_string(), "now": NOW, "max_age_secs": 0 }),
    )
}

fn is_err(reply: &Value) -> bool {
    reply.get("err").is_some()
}

fn apply(events: Value, settings: Value, initial: Value) -> Value {
    ask(
        "monitor-apply",
        json!({ "settings": settings, "initial": initial, "events": events }),
    )
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::active_watch_survives_legacy_fallback_and_clears_on_primary_null
#[test]
fn port_reset_planning_active_watch_survives_legacy_fallback_and_clears_on_primary_null() {
    let watch = json!({ "expires_at": "2026-09-29T04:00:00Z" });
    let events = json!([
        { "provider": "codex", "ok": { "scheduled": null, "active_watch": watch.clone() }, "source_warning": null },
        { "provider": "codex", "ok": { "scheduled": null, "active_watch": null }, "source_warning": "primary failed; legacy used" },
        { "provider": "codex", "ok": { "scheduled": null, "active_watch": null }, "source_warning": null },
    ]);
    let reply = apply(events, json!({}), json!({}));
    let steps = reply["steps"].as_array().cloned().unwrap_or_default();
    assert_eq!(steps[0]["codex"]["active_watch"], watch);
    assert!(steps[0]["display_tick_interval_secs"]
        .as_u64()
        .is_some_and(|secs| secs <= 60));
    assert_eq!(steps[1]["codex"]["active_watch"], watch);
    assert_eq!(steps[2]["codex"]["active_watch"], Value::Null);
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::claude_history_and_banked_grant_have_no_schedule
#[test]
fn port_reset_planning_claude_history_and_banked_grant_have_no_schedule() {
    assert_eq!(parse_claude(claude_feed()), json!({ "ok": null }));
    let mut changed = claude_feed();
    changed["providers"]["claude"]["events"][0]["kind"] = json!("scheduled");
    assert!(is_err(&parse_claude(changed)));
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::claude_stale_detector_or_new_schedule_contract_is_not_healthy_none
#[test]
fn port_reset_planning_claude_stale_detector_or_new_schedule_contract_is_not_healthy_none() {
    let mut changed = claude_feed();
    changed["meta"]["detector"]["lastSuccessfulCheckAt"] = json!("2026-09-26T00:00:00Z");
    assert!(is_err(&parse_claude(changed)));
    let mut changed = claude_feed();
    changed["providers"]["claude"]["scheduled_reset"] = Value::Null;
    assert!(is_err(&parse_claude(changed)));
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::codex_active_watch_is_a_separate_possible_reset_window
#[test]
fn port_reset_planning_codex_active_watch_is_a_separate_possible_reset_window() {
    let expires_at = "2026-09-29T04:00:00Z";
    let status = parse_codex(
        v1_with_watch(Value::Null, json!({ "expires_at": expires_at })),
        NOW,
        0,
    );
    assert_eq!(status["ok"]["scheduled"], Value::Null);
    assert_eq!(
        status["ok"]["active_watch"]["expires_at"],
        json!(expires_at)
    );
    let label = ask(
        "active-watch-text",
        json!({ "provider": "codex", "watch": { "expires_at": expires_at }, "style": "human", "now": NOW }),
    );
    let label = label.as_str().unwrap_or("");
    assert!(label.contains("Possible Codex reset"), "{label}");
    assert!(label.contains("watch window ends"), "{label}");
    assert!(label.contains("2 days"), "{label}");
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::codex_active_watch_may_be_missing_or_null
#[test]
fn port_reset_planning_codex_active_watch_may_be_missing_or_null() {
    let missing = parse_codex(v1(Value::Null), NOW, 0);
    assert_eq!(missing["ok"]["scheduled"], Value::Null);
    assert_eq!(missing["ok"]["active_watch"], Value::Null);
    let explicit_null = parse_codex(v1_with_watch(Value::Null, Value::Null), NOW, 0);
    assert_eq!(explicit_null["ok"]["scheduled"], Value::Null);
    assert_eq!(explicit_null["ok"]["active_watch"], Value::Null);
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::codex_expired_active_watch_is_not_reported_as_a_future_watch
#[test]
fn port_reset_planning_codex_expired_active_watch_is_not_reported_as_a_future_watch() {
    let status = parse_codex(
        v1_with_watch(Value::Null, json!({ "expires_at": "2026-09-27T03:59:59Z" })),
        NOW,
        0,
    );
    assert_eq!(status["ok"]["scheduled"], Value::Null);
    assert_eq!(status["ok"]["active_watch"], Value::Null);
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::codex_explicit_target_offset_and_due_are_not_completion
#[test]
fn port_reset_planning_codex_explicit_target_offset_and_due_are_not_completion() {
    let mut event = pending();
    event["scheduled_for"] = json!("2026-09-28T02:00:00+02:00");
    let scheduled = parse_scheduled(v1(event));
    assert_eq!(
        scheduled["ok"]["scheduled_for"],
        json!("2026-09-28T00:00:00Z")
    );
    assert_eq!(
        countdown(scheduled["ok"].clone(), "exact", "2026-09-28T00:00:00Z"),
        json!("Codex reset due · awaiting confirmation")
    );
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::codex_observed_source_uses_status_page_without_url
#[test]
fn port_reset_planning_codex_observed_source_uses_status_page_without_url() {
    let mut event = pending();
    event["source"] = json!({ "type": "observed" });
    let scheduled = parse_scheduled(v1(event));
    assert_eq!(scheduled["ok"]["source_url"], json!(CODEX_STATUS_URL));
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::codex_scheduled_without_time_is_not_a_countdown
#[test]
fn port_reset_planning_codex_scheduled_without_time_is_not_a_countdown() {
    let scheduled = parse_scheduled(v1(pending()));
    assert_eq!(scheduled["ok"]["scheduled_for"], Value::Null);
    assert_eq!(
        countdown(scheduled["ok"].clone(), "exact", NOW),
        json!("Codex reset scheduled · time TBD")
    );
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::codex_v1_rejects_unknown_type_missing_target_and_stale_metadata
#[test]
fn port_reset_planning_codex_v1_rejects_unknown_type_missing_target_and_stale_metadata() {
    let mut event = pending();
    event["reset_type"] = json!("unknown");
    assert!(is_err(&parse_scheduled(v1(event))));
    let mut event = pending();
    event
        .as_object_mut()
        .map(|object| object.remove("scheduled_for"));
    assert!(is_err(&parse_scheduled(v1(event))));
    assert!(is_err(&parse_codex(
        v1(Value::Null),
        "2026-09-28T04:00:00Z",
        0
    )));
    assert!(is_err(&parse_codex(v1(Value::Null), NOW, 7 * 3600)));
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::compact_watch_text_keeps_hours_when_the_window_is_more_than_a_day_away
#[test]
fn port_reset_planning_compact_watch_text_keeps_hours_when_the_window_is_more_than_a_day_away() {
    let reply = ask(
        "compact-active-watch-text",
        json!({ "provider": "codex", "watch": { "expires_at": "2026-09-29T12:00:00Z" }, "now": NOW }),
    );
    assert_eq!(reply, json!("Possible Codex reset watch ends in 2d8h"));
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::degraded_legacy_cannot_retract_or_roll_back_retained_announcement
#[test]
fn port_reset_planning_degraded_legacy_cannot_retract_or_roll_back_retained_announcement() {
    let retained = parse_scheduled(v1(pending()))["ok"].clone();
    let older = json!({
        "announced_at": "2026-09-25T10:00:00Z", "scheduled_for": null, "source_url": CODEX_LEGACY_URL, "is_banked": true,
    });
    let events = json!([
        { "provider": "codex", "ok": { "scheduled": null, "active_watch": null }, "source_warning": "primary failed; legacy used" },
        { "provider": "codex", "ok": { "scheduled": older, "active_watch": null }, "source_warning": "legacy used" },
        { "provider": "codex", "ok": { "scheduled": null, "active_watch": null }, "source_warning": null },
    ]);
    let reply = apply(
        events,
        json!({}),
        json!({ "codex": { "scheduled": retained.clone() } }),
    );
    let steps = reply["steps"].as_array().cloned().unwrap_or_default();
    assert_eq!(steps[0]["codex"]["scheduled"], retained);
    assert_eq!(steps[1]["codex"]["scheduled"], retained);
    assert!(!steps[1]["codex"]["last_error"].is_null());
    assert_eq!(steps[2]["codex"]["scheduled"], Value::Null);
    assert_eq!(steps[2]["codex"]["last_error"], Value::Null);
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::disabled_provider_ignores_late_result
#[test]
fn port_reset_planning_disabled_provider_ignores_late_result() {
    let events = json!([{ "provider": "codex", "err": "late failure", "source_warning": null }]);
    let reply = apply(events, json!({ "monitor_codex": false }), json!({}));
    let state = &reply["steps"][0]["codex"];
    assert_eq!(state["last_checked"], Value::Null);
    assert_eq!(state["last_error"], Value::Null);
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::exact_countdown_has_seconds_and_due_waits_for_confirmation
#[test]
fn port_reset_planning_exact_countdown_has_seconds_and_due_waits_for_confirmation() {
    let scheduled = json!({
        "announced_at": "2026-09-27T00:00:00Z", "scheduled_for": "2026-09-28T04:07:32Z",
        "source_url": "https://x.com/thsottiaux/status/1", "is_banked": false,
    });
    assert_eq!(
        countdown(scheduled.clone(), "exact", "2026-09-27T00:00:00Z"),
        json!("Codex reset in 1d 4h 7m 32s")
    );
    assert_eq!(
        countdown(scheduled, "exact", "2026-09-28T04:07:32Z"),
        json!("Codex reset due · awaiting confirmation")
    );
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::legacy_watch_does_not_schedule_and_banked_pending_has_unknown_time
#[test]
fn port_reset_planning_legacy_watch_does_not_schedule_and_banked_pending_has_unknown_time() {
    let legacy = |text: &str| ask("parse-codex-legacy", json!({ "feed": text }));
    assert_eq!(
        legacy(r#"{"scheduled":null,"watch":{"reset_chance_percent":60}}"#),
        json!({ "ok": null })
    );
    assert!(is_err(&legacy(r#"{"events":[],"watch":null}"#)));
    let scheduled = legacy(
        r#"{"scheduled":{
            "tweet_id":"2101352781219258527","text":"Tuesday",
            "announced_at":"2026-09-19T16:48:38Z","scheduled_for":null,
            "reset_type":"banked","tweet_url":"https://x.com/thsottiaux/status/2101352781219258527"
        }}"#,
    );
    assert_eq!(scheduled["ok"]["is_banked"], json!(true));
    assert_eq!(scheduled["ok"]["scheduled_for"], Value::Null);
    assert_eq!(
        countdown(scheduled["ok"].clone(), "exact", NOW),
        json!("Codex banked reset scheduled · time TBD")
    );
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::primary_success_avoids_fallback_and_malformed_primary_uses_one_legacy_request
#[test]
fn port_reset_planning_primary_success_avoids_fallback_and_malformed_primary_uses_one_legacy_request(
) {
    let healthy = ask(
        "fetch-provider",
        json!({ "provider": "codex", "now": NOW, "responses": { CODEX_STATUS_URL: v1(Value::Null).to_string() } }),
    );
    assert_eq!(healthy["ok"]["scheduled"], Value::Null);
    assert_eq!(healthy["urls"], json!([CODEX_STATUS_URL]));
    let degraded = ask(
        "fetch-provider",
        json!({
            "provider": "codex", "now": NOW,
            "responses": { CODEX_STATUS_URL: "{", CODEX_LEGACY_URL: r#"{"scheduled":null,"watch":{"reset_chance_percent":60}}"# },
        }),
    );
    assert_eq!(degraded["ok"]["scheduled"], Value::Null);
    assert!(degraded["source_warning"]
        .as_str()
        .unwrap_or("")
        .contains("unknown upstream freshness"));
    assert_eq!(
        degraded["urls"],
        json!([CODEX_STATUS_URL, CODEX_LEGACY_URL])
    );
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::rate_limit_and_transport_failure_do_not_cascade
#[test]
fn port_reset_planning_rate_limit_and_transport_failure_do_not_cascade() {
    let limited = ask(
        "fetch-provider",
        json!({ "provider": "codex", "now": NOW, "failure": { "http": { "url": CODEX_STATUS_URL, "status": 429, "retry_after_secs": 7200 } } }),
    );
    assert!(is_err(&limited));
    assert_eq!(limited["retry_after_secs"], json!(7200));
    assert_eq!(limited["requests"], json!(1));
    let unavailable = ask(
        "fetch-provider",
        json!({ "provider": "codex", "now": NOW, "failure": { "transport": "network unavailable" } }),
    );
    assert!(is_err(&unavailable));
    assert_eq!(unavailable["requests"], json!(1));
    let may_use_legacy = |retry_after: Value| {
        ask(
            "failure-may-use-legacy",
            json!({ "http": { "url": CODEX_STATUS_URL, "status": 503, "retry_after_secs": retry_after } }),
        )
    };
    assert_eq!(may_use_legacy(json!(0)), json!(false));
    assert_eq!(may_use_legacy(Value::Null), json!(true));
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::reset_fetch_runs_on_bounded_io_worker_and_drop_requests_cancellation
#[test]
fn port_reset_planning_reset_fetch_runs_on_bounded_io_worker_and_drop_requests_cancellation() {
    let reply = ask(
        "fetch-on-io-worker",
        json!({ "io_threads": 1, "queue_slots": 1, "abort_after_start": true, "join_deadline_secs": 5 }),
    );
    // The blocking fetch callback runs on a dedicated OS thread, not the event loop.
    assert_eq!(reply["started"], json!(true));
    assert_eq!(reply["ran_on_event_loop_thread"], json!(false));
    assert_eq!(reply["future_cancelled"], json!(true));
    // Dropping the request cancels the job and the worker physically exits.
    assert_eq!(reply["remaining_workers"], json!(0));
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::retry_after_and_poll_deadline_preserve_hourly_floor
#[test]
fn port_reset_planning_retry_after_and_poll_deadline_preserve_hourly_floor() {
    let then = "1994-11-06T08:48:37Z";
    let retry = |text: &str| ask("parse-retry-after", json!({ "text": text, "now": then }));
    assert_eq!(retry("90"), json!(90));
    assert_eq!(retry("Sun, 06 Nov 1994 08:49:37 GMT"), json!(60));
    assert_eq!(retry("Sun, 06 Nov 1994 08:47:37 GMT"), json!(0));
    assert_eq!(retry("invalid"), Value::Null);
    let deadline = |retry_after: Value| {
        ask(
            "next-poll-deadline",
            json!({ "retry_after_secs": retry_after }),
        )
    };
    assert_eq!(
        deadline(Value::Null)["delay_secs"],
        deadline(Value::Null)["poll_interval_secs"]
    );
    assert_eq!(deadline(json!(7200))["delay_secs"], json!(7200));
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::active_watch_survives_legacy_fallback_and_clears_on_primary_null
#[test]
fn port_reset_planning_active_watch_is_never_reported_as_a_scheduled_reset() {
    let watch = json!({ "expires_at": "2026-09-29T04:00:00Z" });
    let events = json!([
        { "provider": "codex", "ok": { "scheduled": null, "active_watch": watch.clone() }, "source_warning": null },
    ]);
    let reply = apply(events, json!({}), json!({}));
    let steps = reply["steps"].as_array().cloned().unwrap_or_default();
    assert_eq!(steps[0]["codex"]["scheduled"], Value::Null);
    assert_eq!(steps[0]["codex"]["active_watch"], watch);
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::claude_history_and_banked_grant_have_no_schedule
#[test]
fn port_reset_planning_claude_banked_grant_alone_yields_no_schedule() {
    let mut feed = claude_feed();
    feed["providers"]["claude"]["events"] = json!([{ "kind": "banked_grant" }]);
    assert_eq!(parse_claude(feed), json!({ "ok": null }));
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::claude_stale_detector_or_new_schedule_contract_is_not_healthy_none
#[test]
fn port_reset_planning_claude_feed_without_detector_metadata_is_rejected() {
    let mut feed = claude_feed();
    if let Some(object) = feed.as_object_mut() {
        object.remove("meta");
    }
    assert!(is_err(&parse_claude(feed)));
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::codex_active_watch_is_a_separate_possible_reset_window
#[test]
fn port_reset_planning_codex_scheduled_reset_and_active_watch_coexist_as_separate_fields() {
    let expires_at = "2026-09-29T04:00:00Z";
    let status = parse_codex(
        v1_with_watch(pending(), json!({ "expires_at": expires_at })),
        NOW,
        0,
    );
    assert!(!status["ok"]["scheduled"].is_null());
    assert_eq!(
        status["ok"]["active_watch"]["expires_at"],
        json!(expires_at)
    );
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::codex_active_watch_may_be_missing_or_null
#[test]
fn port_reset_planning_codex_pending_event_without_watch_still_schedules() {
    let status = parse_codex(v1(pending()), NOW, 0);
    assert!(!status["ok"]["scheduled"].is_null());
    assert_eq!(status["ok"]["active_watch"], Value::Null);
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::codex_expired_active_watch_is_not_reported_as_a_future_watch
#[test]
fn port_reset_planning_codex_watch_one_second_ahead_is_still_reported() {
    let expires_at = "2026-09-27T04:00:01Z";
    let status = parse_codex(
        v1_with_watch(Value::Null, json!({ "expires_at": expires_at })),
        NOW,
        0,
    );
    assert_eq!(
        status["ok"]["active_watch"]["expires_at"],
        json!(expires_at)
    );
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::codex_explicit_target_offset_and_due_are_not_completion
#[test]
fn port_reset_planning_codex_countdown_before_explicit_target_is_not_yet_due() {
    let mut event = pending();
    event["scheduled_for"] = json!("2026-09-28T00:00:00Z");
    let scheduled = parse_scheduled(v1(event));
    let text = countdown(scheduled["ok"].clone(), "exact", "2026-09-27T22:00:00Z");
    let text = text.as_str().unwrap_or("");
    assert!(text.starts_with("Codex reset in "), "{text}");
    assert!(!text.contains("awaiting"), "{text}");
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::codex_observed_source_uses_status_page_without_url
#[test]
fn port_reset_planning_codex_tweet_source_keeps_its_own_url() {
    let scheduled = parse_scheduled(v1(pending()));
    assert_eq!(
        scheduled["ok"]["source_url"],
        json!("https://x.com/thsottiaux/status/1")
    );
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::codex_scheduled_without_time_is_not_a_countdown
#[test]
fn port_reset_planning_codex_scheduled_without_time_stays_tbd_in_compact_style() {
    let scheduled = parse_scheduled(v1(pending()));
    let text = countdown(scheduled["ok"].clone(), "compact", NOW);
    let text = text.as_str().unwrap_or("");
    assert!(text.contains("TBD"), "{text}");
    assert!(!text.contains("reset in"), "{text}");
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::codex_v1_rejects_unknown_type_missing_target_and_stale_metadata
#[test]
fn port_reset_planning_codex_v2_schema_is_rejected_before_any_event() {
    let mut feed = v1(Value::Null);
    feed["schema"] = json!("v2");
    assert!(is_err(&parse_codex(feed, NOW, 0)));
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::compact_watch_text_keeps_hours_when_the_window_is_more_than_a_day_away
#[test]
fn port_reset_planning_compact_watch_text_drops_days_inside_one_day() {
    let reply = ask(
        "compact-active-watch-text",
        json!({ "provider": "codex", "watch": { "expires_at": "2026-09-27T20:00:00Z" }, "now": NOW }),
    );
    let text = reply.as_str().unwrap_or("");
    assert!(text.contains("16h"), "{text}");
    assert!(!text.contains('d'), "{text}");
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::degraded_legacy_cannot_retract_or_roll_back_retained_announcement
#[test]
fn port_reset_planning_degraded_legacy_keeps_retained_announcement_time() {
    let retained = parse_scheduled(v1(pending()))["ok"].clone();
    let events = json!([
        { "provider": "codex", "ok": { "scheduled": null, "active_watch": null }, "source_warning": "primary failed; legacy used" },
    ]);
    let reply = apply(
        events,
        json!({}),
        json!({ "codex": { "scheduled": retained } }),
    );
    assert_eq!(
        reply["steps"][0]["codex"]["scheduled"]["announced_at"],
        json!("2026-09-26T10:00:00Z")
    );
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::disabled_provider_ignores_late_result
#[test]
fn port_reset_planning_disabled_provider_ignores_late_successful_watch() {
    let events = json!([
        { "provider": "codex", "ok": { "scheduled": null, "active_watch": { "expires_at": "2026-09-29T04:00:00Z" } }, "source_warning": null },
    ]);
    let reply = apply(events, json!({ "monitor_codex": false }), json!({}));
    assert_eq!(reply["steps"][0]["codex"]["active_watch"], Value::Null);
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::exact_countdown_has_seconds_and_due_waits_for_confirmation
#[test]
fn port_reset_planning_exact_countdown_one_second_before_due_shows_seconds() {
    let scheduled = json!({
        "announced_at": "2026-09-27T00:00:00Z", "scheduled_for": "2026-09-28T04:07:32Z",
        "source_url": "https://x.com/thsottiaux/status/1", "is_banked": false,
    });
    let text = countdown(scheduled, "exact", "2026-09-28T04:07:31Z");
    let text = text.as_str().unwrap_or("");
    assert!(text.starts_with("Codex reset in "), "{text}");
    assert!(text.contains("1s"), "{text}");
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::legacy_watch_does_not_schedule_and_banked_pending_has_unknown_time
#[test]
fn port_reset_planning_legacy_scheduled_reset_without_banked_type_is_not_banked() {
    let legacy = |text: &str| ask("parse-codex-legacy", json!({ "feed": text }));
    let scheduled = legacy(
        r#"{"scheduled":{
            "tweet_id":"2101352781219258527","text":"Friday",
            "announced_at":"2026-09-19T16:48:38Z","scheduled_for":null,
            "reset_type":"scheduled","tweet_url":"https://x.com/thsottiaux/status/2101352781219258527"
        }}"#,
    );
    assert_eq!(scheduled["ok"]["is_banked"], json!(false));
    assert_eq!(
        countdown(scheduled["ok"].clone(), "exact", NOW),
        json!("Codex reset scheduled · time TBD")
    );
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::primary_success_avoids_fallback_and_malformed_primary_uses_one_legacy_request
#[test]
fn port_reset_planning_healthy_primary_reports_no_source_warning() {
    let healthy = ask(
        "fetch-provider",
        json!({ "provider": "codex", "now": NOW, "responses": { CODEX_STATUS_URL: v1(Value::Null).to_string() } }),
    );
    assert_eq!(healthy["source_warning"], Value::Null);
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::rate_limit_and_transport_failure_do_not_cascade
#[test]
fn port_reset_planning_server_error_without_retry_after_makes_one_request() {
    let failed = ask(
        "fetch-provider",
        json!({ "provider": "codex", "now": NOW, "failure": { "http": { "url": CODEX_STATUS_URL, "status": 500, "retry_after_secs": null } } }),
    );
    assert!(is_err(&failed));
    assert_eq!(failed["requests"], json!(1));
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::reset_fetch_runs_on_bounded_io_worker_and_drop_requests_cancellation
#[test]
fn port_reset_planning_reset_fetch_without_abort_completes_off_the_event_loop() {
    let reply = ask(
        "fetch-on-io-worker",
        json!({ "io_threads": 1, "queue_slots": 1, "abort_after_start": false, "join_deadline_secs": 5 }),
    );
    assert_eq!(reply["started"], json!(true));
    assert_eq!(reply["ran_on_event_loop_thread"], json!(false));
    assert_eq!(reply["future_cancelled"], json!(false));
}

/// REQ: PORT:ilium-client/src/reset_planning.rs::retry_after_and_poll_deadline_preserve_hourly_floor
#[test]
fn port_reset_planning_short_retry_after_cannot_shorten_the_hourly_floor() {
    let deadline = |retry_after: Value| {
        ask(
            "next-poll-deadline",
            json!({ "retry_after_secs": retry_after }),
        )
    };
    let floor = deadline(Value::Null)["poll_interval_secs"].as_u64();
    let short = deadline(json!(60))["delay_secs"].as_u64();
    assert!(
        short.is_some() && short >= floor,
        "short={short:?} floor={floor:?}"
    );
}
