//! Twins of ilium-client/src/cost_tracker.rs: per-pane cost derivation, overlay roll-ups and refresh planning.
//!
//! Ops `tracker.pane-cost` (`{stats, settings, now_ms}` -> pane cost), `tracker.overlay` (`{settings, panes,
//! agents, tree}` -> overlay), `tracker.settle` (a list of ticks over one tracker, answering one overlay per
//! tick) and `tracker.plan-refreshes`. Stats are written as `{usage:[{model,input,output}], spend:[{minute_ms,
//! model,input}], reported_usd, quota:[{ms,points,window}]}`.

use serde_json::{json, Value};

use crate::common::client;

const MINUTE: i64 = 60_000;

fn usage(model: &str, input: u64, output: u64) -> Value {
    json!({ "model": model, "input": input, "output": output })
}

fn pane_cost(stats: Value, settings: Value, now_ms: i64) -> Value {
    client(
        "tracker.pane-cost",
        json!({ "stats": stats, "settings": settings, "now_ms": now_ms }),
    )
}

fn quota_stats(points: &[(i64, f64)], window: &str) -> Value {
    let quota: Vec<Value> = points
        .iter()
        .map(|(ms, value)| json!({ "ms": ms, "points": value, "window": window }))
        .collect();
    json!({ "usage": [], "quota": quota })
}

fn quota_settings() -> Value {
    json!({ "metric": "quota", "quota_window": "primary" })
}

fn overlay(settings: Value, panes: Value) -> Value {
    client(
        "tracker.overlay",
        json!({ "settings": settings, "panes": panes, "agents": ["first", "second"], "tree": "project/[first,second]" }),
    )
}

fn near(value: &Value, expected: f64) -> bool {
    value
        .as_f64()
        .is_some_and(|actual| (actual - expected).abs() < 1e-9)
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::a_reported_total_wins_when_larger_and_clears_the_lower_bound
#[test]
fn port_cost_tracker_a_reported_total_wins_when_larger_and_clears_the_lower_bound() {
    let stats = json!({ "usage": [usage("claude-haiku-4-5", 1_000_000, 0), usage("mystery", 5, 5)], "reported_usd": 3.5 });
    let cost = pane_cost(stats, json!({}), 0);
    assert_eq!(cost["usd"], json!(3.5));
    assert_eq!(cost["is_lower_bound"], json!(false));
    assert_eq!(cost["reported_usd"], json!(3.5));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::budget_calibration_flags_agents_over_budget
#[test]
fn port_cost_tracker_budget_calibration_flags_agents_over_budget() {
    let settings = json!({ "calibration": "budget", "budget_usd": 2.0 });
    let panes = json!({ "first": { "usage": [usage("claude-haiku-4-5", 3_000_000, 0)] } });
    let reply = overlay(settings, panes);
    assert_eq!(reply["rows"]["first"]["is_over_budget"], json!(true));
    // The highest level index (the maximum of the level scale).
    assert_eq!(reply["rows"]["first"]["level"], reply["max_level"]);
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::changing_the_metric_recomputes_panes_that_were_already_derived
#[test]
fn port_cost_tracker_changing_the_metric_recomputes_panes_that_were_already_derived() {
    let store = json!({ "first": quota_stats(&[(0, 9.0)], "primary") });
    let ticks = json!([
        { "settings": { "calibration": "fixed_bands" }, "store": store.clone() },
        { "settings": { "calibration": "fixed_bands", "metric": "quota" }, "store": store },
    ]);
    let reply = client(
        "tracker.settle",
        json!({ "ticks": ticks, "agents": ["first"] }),
    );
    let overlays = reply["overlays"].as_array().cloned().unwrap_or_default();
    assert_eq!(overlays[0]["changed"], json!(true));
    assert_eq!(
        overlays[0]["rows"]["first"]["amount"],
        json!(0.0),
        "no dollar spend"
    );
    assert_eq!(overlays[1]["changed"], json!(true));
    assert_eq!(overlays[1]["rows"]["first"]["amount"], json!(9.0));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::disabled_settings_track_nothing
#[test]
fn port_cost_tracker_disabled_settings_track_nothing() {
    let reply = client(
        "tracker.settle",
        json!({ "ticks": [{ "settings": { "meter": { "enabled": false } }, "store": {} }], "agents": ["first"] }),
    );
    assert_eq!(reply["overlays"][0]["changed"], json!(false));
    assert_eq!(reply["overlays"][0]["rows"], json!({}));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::invalidated_transcript_cache_removes_previously_derived_costs
#[test]
fn port_cost_tracker_invalidated_transcript_cache_removes_previously_derived_costs() {
    let settings =
        json!({ "calibration": "fixed_bands", "metric": "quota", "quota_window": "primary" });
    let ticks = json!([
        { "settings": settings.clone(), "store": { "first": quota_stats(&[(0, 9.0)], "primary") } },
        { "settings": settings, "store": {} },
    ]);
    let reply = client(
        "tracker.settle",
        json!({ "ticks": ticks, "agents": ["first"] }),
    );
    let overlays = reply["overlays"].as_array().cloned().unwrap_or_default();
    assert_eq!(overlays[0]["changed"], json!(true));
    assert!(!overlays[0]["pane_cost"]["first"].is_null());
    assert_eq!(overlays[1]["changed"], json!(true));
    // Old session cost cannot remain after transcript ownership disappears.
    assert!(overlays[1]["pane_cost"]["first"].is_null());
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::overlay_rolls_group_totals_up_and_keeps_loading_agents_unrated
#[test]
fn port_cost_tracker_overlay_rolls_group_totals_up_and_keeps_loading_agents_unrated() {
    let settings = json!({ "calibration": "fixed_bands", "group_totals": { "enabled": true } });
    let reply = overlay(
        settings,
        json!({ "first": { "usage": [usage("claude-haiku-4-5", 6_000_000, 0)] } }),
    );
    assert_eq!(reply["agent_count"], json!(2));
    assert!(near(&reply["total_amount"], 6.0));
    // $6 is in the $5..$20 band.
    assert_eq!(reply["rows"]["first"]["level"], json!(2));
    assert_eq!(reply["rows"]["second"]["is_loading"], json!(true));
    assert!(reply["rows"]["second"]["level"].is_null());
    assert!(near(&reply["rows"]["project"]["amount"], 6.0));
    // A loading child keeps the group unrated.
    assert_eq!(reply["rows"]["project"]["is_loading"], json!(true));
    assert_eq!(reply["ranks"]["first"], json!(6.0));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::prices_models_and_marks_unknown_ones_as_a_lower_bound
#[test]
fn port_cost_tracker_prices_models_and_marks_unknown_ones_as_a_lower_bound() {
    let stats =
        json!({ "usage": [usage("claude-haiku-4-5", 1_000_000, 0), usage("gpt-reserve", 10, 10)] });
    let cost = pane_cost(stats, json!({}), 0);
    assert!(near(&cost["usd"], 1.0));
    assert_eq!(cost["is_lower_bound"], json!(true));
    assert!(cost["model_costs"][1]["usd"].is_null());
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::quota_budget_is_a_share_of_the_window
#[test]
fn port_cost_tracker_quota_budget_is_a_share_of_the_window() {
    let mut settings = quota_settings();
    settings["calibration"] = json!("budget");
    settings["quota_budget_percent"] = json!(5.0);
    let reply = overlay(
        settings,
        json!({ "first": quota_stats(&[(0, 6.0)], "primary") }),
    );
    assert_eq!(reply["rows"]["first"]["is_over_budget"], json!(true));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::quota_metric_sums_the_chosen_window_and_drives_burn_and_sparkline
#[test]
fn port_cost_tracker_quota_metric_sums_the_chosen_window_and_drives_burn_and_sparkline() {
    let now = 1_000 * MINUTE;
    let stats = quota_stats(
        &[(now - 5 * MINUTE, 3.0), (now - 50 * MINUTE, 1.5)],
        "primary",
    );
    let mut settings = quota_settings();
    settings["sparkline_window_minutes"] = json!(60);
    settings["sparkline_cells"] = json!(4);
    let cost = pane_cost(stats.clone(), settings.clone(), now);
    assert_eq!(cost["has_quota"], json!(true));
    assert!(near(&cost["quota_points"], 4.5));
    // 3 points inside the last 15 minutes = 12 points an hour.
    assert!(near(&cost["quota_burn_per_hour"], 12.0));
    let sparkline_total: f64 = cost["spark_cells"]
        .as_array()
        .map(|cells| cells.iter().filter_map(Value::as_f64).sum())
        .unwrap_or(0.0);
    assert!((sparkline_total - 4.5).abs() < 1e-9);
    assert_eq!(cost["amount_quota"], cost["quota_points"]);
    assert_eq!(cost["amount_dollars"], cost["usd"]);

    settings["quota_window"] = json!("secondary");
    let other = pane_cost(stats, settings, now);
    assert_eq!(
        other["has_quota"],
        json!(false),
        "no secondary window was ever reported"
    );
    assert_eq!(other["quota_points"], json!(0.0));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::quota_overlay_rates_codex_agents_and_leaves_claude_agents_unavailable
#[test]
fn port_cost_tracker_quota_overlay_rates_codex_agents_and_leaves_claude_agents_unavailable() {
    let mut settings = quota_settings();
    settings["group_totals"] = json!({ "enabled": true });
    let panes = json!({
        "first": quota_stats(&[(0, 9.0)], "primary"),
        "second": { "usage": [usage("claude-haiku-4-5", 6_000_000, 0)] },
    });
    let reply = overlay(settings, panes);
    assert_eq!(reply["metric"], json!("quota"));
    assert_eq!(reply["rows"]["first"]["amount"], json!(9.0));
    assert_eq!(reply["rows"]["first"]["is_lower_bound"], json!(false));
    // 9 points sits in the 8..20 band of the default quota bands.
    assert_eq!(reply["rows"]["first"]["level"], json!(3));
    assert_eq!(reply["rows"]["second"]["is_unavailable"], json!(true));
    assert!(reply["rows"]["second"]["level"].is_null());
    assert_eq!(reply["rows"]["second"]["is_loading"], json!(false));
    assert_eq!(
        reply["agent_count"],
        json!(1),
        "only agents with a figure count"
    );
    assert!(near(&reply["total_amount"], 9.0));
    assert!(near(&reply["rows"]["project"]["amount"], 9.0));
    assert_eq!(reply["peer_totals"], json!([9.0]));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::quota_spike_needs_the_quota_floor_not_the_dollar_one
#[test]
fn port_cost_tracker_quota_spike_needs_the_quota_floor_not_the_dollar_one() {
    let now = 1_000 * MINUTE;
    // 1 point in ten minutes is 6 points an hour: a spike over a flat past.
    let stats = quota_stats(&[(now - 2 * MINUTE, 1.0)], "primary");
    let mut settings = quota_settings();
    settings["sparkline_window_minutes"] = json!(360);
    assert_eq!(
        pane_cost(stats.clone(), settings.clone(), now)["is_spike"],
        json!(true)
    );
    settings["metric"] = json!("dollars");
    assert_eq!(
        pane_cost(stats, settings, now)["is_spike"],
        json!(false),
        "no dollar spend, so nothing spikes"
    );
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::refresh_planning_respects_cadence_and_the_worker_cap
#[test]
fn port_cost_tracker_refresh_planning_respects_cadence_and_the_worker_cap() {
    let reply = client(
        "tracker.plan-refreshes",
        json!({ "panes": [10, 11, 12, 13, 14, 15], "script": [
        { "running_workers": 0, "at_secs": 0 },
        { "running_workers": "cap", "at_secs": 0 },
        { "running_workers": 0, "at_secs": 0 },
        { "panes": [10], "running_workers": 0, "at_secs": "interval_plus_one" },
    ] }),
    );
    let plans = reply["plans"].as_array().cloned().unwrap_or_default();
    let cap = reply["max_concurrent_stats_workers"].as_u64().unwrap_or(0) as usize;
    let first = plans[0].as_array().cloned().unwrap_or_default();
    assert_eq!(first.len(), cap);
    assert_eq!(plans[1], json!([]));
    let next = plans[2].as_array().cloned().unwrap_or_default();
    assert_eq!(next.len(), cap);
    assert!(next.iter().all(|pane| !first.contains(pane)));
    assert_eq!(plans[3], json!([10]));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::spend_points_feed_the_burn_rate_and_sparkline_window
#[test]
fn port_cost_tracker_spend_points_feed_the_burn_rate_and_sparkline_window() {
    let now = 1_000 * MINUTE;
    let stats = json!({
        "usage": [usage("claude-haiku-4-5", 1_500_000, 0)],
        "spend": [
            { "minute_ms": now - 5 * MINUTE, "model": "claude-haiku-4-5", "input": 1_000_000 },
            { "minute_ms": now - 50 * MINUTE, "model": "claude-haiku-4-5", "input": 500_000 },
        ],
    });
    let cost = pane_cost(
        stats,
        json!({ "sparkline_window_minutes": 60, "sparkline_cells": 4 }),
        now,
    );
    // $1 in the last 15 minutes = $4/h.
    assert!(cost["burn_usd_per_hour"]
        .as_f64()
        .is_some_and(|burn| (burn - 4.0).abs() < 1e-6));
    let cells = cost["spark_cells"].as_array().cloned().unwrap_or_default();
    assert_eq!(cells.len(), 4);
    assert!((cells.iter().filter_map(Value::as_f64).sum::<f64>() - 1.5).abs() < 1e-9);
    assert!(cells[0].as_f64().unwrap_or(0.0) > 0.0 && cells[3].as_f64().unwrap_or(0.0) > 0.0);
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::a_reported_total_wins_when_larger_and_clears_the_lower_bound
#[test]
fn port_cost_tracker_priced_total_wins_over_a_smaller_reported_total() {
    let stats = json!({ "usage": [usage("claude-haiku-4-5", 1_000_000, 0)], "reported_usd": 0.5 });
    let cost = pane_cost(stats, json!({}), 0);
    assert!(near(&cost["usd"], 1.0));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::budget_calibration_flags_agents_over_budget
#[test]
fn port_cost_tracker_budget_calibration_leaves_an_agent_under_budget_unflagged() {
    let settings = json!({ "calibration": "budget", "budget_usd": 2.0 });
    let panes = json!({ "first": { "usage": [usage("claude-haiku-4-5", 1_000_000, 0)] } });
    let reply = overlay(settings, panes);
    assert_eq!(reply["rows"]["first"]["is_over_budget"], json!(false));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::changing_the_metric_recomputes_panes_that_were_already_derived
#[test]
fn port_cost_tracker_switching_back_to_dollars_drops_the_quota_amount() {
    let store = json!({ "first": quota_stats(&[(0, 9.0)], "primary") });
    let ticks = json!([
        { "settings": { "calibration": "fixed_bands", "metric": "quota" }, "store": store.clone() },
        { "settings": { "calibration": "fixed_bands", "metric": "dollars" }, "store": store },
    ]);
    let reply = client(
        "tracker.settle",
        json!({ "ticks": ticks, "agents": ["first"] }),
    );
    let overlays = reply["overlays"].as_array().cloned().unwrap_or_default();
    assert_eq!(overlays[1]["changed"], json!(true));
    assert_eq!(overlays[1]["rows"]["first"]["amount"], json!(0.0));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::disabled_settings_track_nothing
#[test]
fn port_cost_tracker_disabled_meter_ignores_priced_usage() {
    let reply = client(
        "tracker.settle",
        json!({ "ticks": [{ "settings": { "meter": { "enabled": false } }, "store": { "first": { "usage": [usage("claude-haiku-4-5", 1_000_000, 0)] } } }], "agents": ["first"] }),
    );
    assert_eq!(reply["overlays"][0]["changed"], json!(false));
    assert!(reply["overlays"][0]["pane_cost"]["first"].is_null());
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::invalidated_transcript_cache_removes_previously_derived_costs
#[test]
fn port_cost_tracker_quota_pane_cost_reports_quota_availability() {
    let reply = client(
        "tracker.settle",
        json!({ "ticks": [{ "settings": quota_settings(), "store": { "first": quota_stats(&[(0, 9.0)], "primary") } }], "agents": ["first"] }),
    );
    assert_eq!(reply["overlays"][0]["pane_cost"]["first"]["has_quota"], json!(true));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::overlay_rolls_group_totals_up_and_keeps_loading_agents_unrated
#[test]
fn port_cost_tracker_group_row_is_absent_when_group_totals_are_off() {
    let reply = overlay(
        json!({ "calibration": "fixed_bands" }),
        json!({ "first": { "usage": [usage("claude-haiku-4-5", 6_000_000, 0)] } }),
    );
    assert!(reply["rows"]["project"].is_null());
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::prices_models_and_marks_unknown_ones_as_a_lower_bound
#[test]
fn port_cost_tracker_fully_priced_pane_is_not_a_lower_bound() {
    let stats = json!({ "usage": [usage("claude-haiku-4-5", 1_000_000, 0)] });
    let cost = pane_cost(stats, json!({}), 0);
    assert_eq!(cost["is_lower_bound"], json!(false));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::quota_budget_is_a_share_of_the_window
#[test]
fn port_cost_tracker_quota_under_the_window_share_is_not_over_budget() {
    let mut settings = quota_settings();
    settings["calibration"] = json!("budget");
    settings["quota_budget_percent"] = json!(5.0);
    let reply = overlay(
        settings,
        json!({ "first": quota_stats(&[(0, 4.0)], "primary") }),
    );
    assert_eq!(reply["rows"]["first"]["is_over_budget"], json!(false));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::quota_metric_sums_the_chosen_window_and_drives_burn_and_sparkline
#[test]
fn port_cost_tracker_quota_burn_is_zero_when_the_last_quarter_hour_is_empty() {
    let now = 1_000 * MINUTE;
    let stats = quota_stats(&[(now - 50 * MINUTE, 1.5)], "primary");
    let cost = pane_cost(stats, quota_settings(), now);
    assert!(near(&cost["quota_burn_per_hour"], 0.0));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::quota_overlay_rates_codex_agents_and_leaves_claude_agents_unavailable
#[test]
fn port_cost_tracker_codex_agent_with_quota_is_available() {
    let mut settings = quota_settings();
    settings["group_totals"] = json!({ "enabled": true });
    let panes = json!({ "first": quota_stats(&[(0, 9.0)], "primary") });
    let reply = overlay(settings, panes);
    assert_eq!(reply["rows"]["first"]["is_unavailable"], json!(false));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::quota_spike_needs_the_quota_floor_not_the_dollar_one
#[test]
fn port_cost_tracker_empty_quota_history_never_spikes() {
    let now = 1_000 * MINUTE;
    let stats = json!({ "usage": [], "quota": [] });
    let mut settings = quota_settings();
    settings["sparkline_window_minutes"] = json!(360);
    assert_eq!(pane_cost(stats, settings, now)["is_spike"], json!(false));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::refresh_planning_respects_cadence_and_the_worker_cap
#[test]
fn port_cost_tracker_no_panes_plans_no_refreshes() {
    let reply = client(
        "tracker.plan-refreshes",
        json!({ "panes": [], "script": [{ "running_workers": 0, "at_secs": 0 }] }),
    );
    assert_eq!(reply["plans"][0], json!([]));
}

/// REQ: PORT:ilium-client/src/cost_tracker.rs::spend_points_feed_the_burn_rate_and_sparkline_window
#[test]
fn port_cost_tracker_spend_older_than_the_sparkline_window_is_left_out() {
    let now = 1_000 * MINUTE;
    let stats = json!({
        "usage": [usage("claude-haiku-4-5", 1_500_000, 0)],
        "spend": [
            { "minute_ms": now - 5 * MINUTE, "model": "claude-haiku-4-5", "input": 1_000_000 },
            { "minute_ms": now - 50 * MINUTE, "model": "claude-haiku-4-5", "input": 500_000 },
        ],
    });
    let cost = pane_cost(
        stats,
        json!({ "sparkline_window_minutes": 10, "sparkline_cells": 4 }),
        now,
    );
    let cells = cost["spark_cells"].as_array().cloned().unwrap_or_default();
    assert!((cells.iter().filter_map(Value::as_f64).sum::<f64>() - 1.0).abs() < 1e-9);
}
