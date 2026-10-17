//! Twins of ilium-client/src/cost_model.rs: price table, level scales, calibration, formatting, spend series.

use serde_json::{json, Value};

use crate::common::{near, server};

fn s(op: &str, request: Value) -> Value {
    server(&format!("model.{op}"), request)
}

fn usd(value: f64, lower_bound: bool) -> Value {
    s(
        "format-usd",
        json!({ "usd": value, "lower_bound": lower_bound }),
    )
}

fn amount(metric: &str, value: f64, lower_bound: bool) -> Value {
    s(
        "format-amount",
        json!({ "metric": metric, "value": value, "lower_bound": lower_bound }),
    )
}

fn tokens(input: u64, cache_read: u64, cache_write: u64, output: u64) -> Value {
    json!({ "input": input, "cache_read": cache_read, "cache_write": cache_write, "output": output, "reasoning": 0 })
}

fn calibrate(calibration: &str, peers: &[f64], history: Option<&[f64]>) -> Value {
    s(
        "calibrate",
        json!({
            "calibration": calibration,
            "fixed_cuts": [1.0, 5.0, 20.0, 50.0],
            "burn_cuts": [0.5, 2.0, 5.0, 15.0],
            "budget": 10.0,
            "peer_totals": peers,
            "history_sorted": history,
        }),
    )
}

fn level(cuts: &Value, value: f64) -> Value {
    s("scale-level", json!({ "cuts": cuts, "value": value }))
}

fn lookup(overrides: Value, model: &str) -> Value {
    s(
        "price-lookup",
        json!({ "overrides": overrides, "model": model }),
    )
}

fn points(items: &[(i64, f64)]) -> Value {
    json!(items
        .iter()
        .map(|(at_ms, usd)| json!({ "at_ms": at_ms, "usd": usd }))
        .collect::<Vec<_>>())
}

/// REQ: PORT:ilium-client/src/cost_model.rs::amounts_format_in_the_unit_of_their_metric
#[test]
fn port_cost_model_amounts_format_in_the_unit_of_their_metric() {
    assert_eq!(amount("dollars", 4.3, false), json!("$4.30"));
    assert_eq!(amount("dollars", 4.3, true), json!("~$4.30"));
    assert_eq!(amount("quota", 0.43, true), json!("0.4%"));
    assert_eq!(amount("quota", 12.4, false), json!("12%"));
    assert_eq!(
        s("format-rate", json!({ "metric": "dollars", "value": 3.0 })),
        json!("$3.00/h")
    );
    assert_eq!(
        s("format-rate", json!({ "metric": "quota", "value": 1.5 })),
        json!("1.5%/h")
    );
    assert_eq!(
        s("quota-window-key", json!({ "window": "primary" })),
        json!("primary")
    );
    assert_eq!(
        s("quota-window-key", json!({ "window": "secondary" })),
        json!("secondary")
    );
}

/// REQ: PORT:ilium-client/src/cost_model.rs::budget_and_burn_calibrations_use_their_own_basis
#[test]
fn port_cost_model_budget_and_burn_calibrations_use_their_own_basis() {
    let budget = calibrate("budget", &[], None);
    assert_eq!(budget["cuts"], json!([2.0, 4.0, 6.0, 8.0]));
    assert_eq!(budget["basis"], json!("total_usd"));
    let burn = calibrate("burn_rate", &[], None);
    assert_eq!(burn["basis"], json!("burn_usd_per_hour"));
    assert_eq!(burn["cuts"], json!([0.5, 2.0, 5.0, 15.0]));
    assert!(near(
        &s("budget-fill", json!({ "total": 12.5, "budget": 10.0 })),
        1.25
    ));
}

/// REQ: PORT:ilium-client/src/cost_model.rs::burn_rate_and_spike_detection
#[test]
fn port_cost_model_burn_rate_and_spike_detection() {
    let minute: i64 = 60_000;
    let now = 1_000 * minute;
    let recent_ms = 10 * minute;
    let hot: Vec<(i64, f64)> = (0..10).map(|offset| (now - offset * minute, 0.5)).collect();
    let burn = s(
        "burn_usd_per_hour",
        json!({ "points": points(&hot), "now_ms": now, "window_ms": recent_ms }),
    );
    assert!(near(&burn, 30.0), "{burn}");
    let spike = |list: &[(i64, f64)]| {
        s(
            "is-burn-spike",
            json!({ "points": points(list), "now_ms": now, "baseline_window_ms": 360 * minute, "minimum_per_hour": 1.0 }),
        )
    };
    assert_eq!(spike(&hot), json!(true));
    let steady: Vec<(i64, f64)> = (0..360)
        .map(|offset| (now - offset * minute, 0.5))
        .collect();
    assert_eq!(spike(&steady), json!(false));
    assert_eq!(spike(&[(now, 0.01)]), json!(false));
}

/// REQ: PORT:ilium-client/src/cost_model.rs::cache_reads_cost_a_fraction_of_fresh_input
#[test]
fn port_cost_model_cache_reads_cost_a_fraction_of_fresh_input() {
    let cached = s(
        "price-cost",
        json!({ "model": "claude-sonnet-5-5", "tokens": tokens(0, 1_000_000, 0, 0) }),
    );
    let fresh = s(
        "price-cost",
        json!({ "model": "claude-sonnet-5-5", "tokens": tokens(1_000_000, 0, 0, 0) }),
    );
    assert!(near(&cached, 0.20), "{cached}");
    assert!(near(&fresh, 2.0), "{fresh}");
}

/// REQ: PORT:ilium-client/src/cost_model.rs::dollar_and_token_formatting_is_compact
#[test]
fn port_cost_model_dollar_and_token_formatting_is_compact() {
    assert_eq!(usd(0.0, false), json!("$0.00"));
    assert_eq!(usd(4.3, false), json!("$4.30"));
    assert_eq!(usd(41.04, false), json!("$41.0"));
    assert_eq!(usd(412.6, false), json!("$413"));
    assert_eq!(usd(1234.0, false), json!("$1.2k"));
    assert_eq!(usd(1.0, true), json!("~$1.00"));
    assert_eq!(
        s("format-usd-per-hour", json!({ "usd": 3.1 })),
        json!("$3.10/h")
    );
    let tok = |count: u64| s("format-tokens", json!({ "tokens": count }));
    assert_eq!(tok(842), json!("842"));
    assert_eq!(tok(12_400), json!("12.4k"));
    assert_eq!(tok(1_200_000), json!("1.2M"));
}

/// REQ: PORT:ilium-client/src/cost_model.rs::history_calibration_uses_percentiles_once_enough_sessions_exist
#[test]
fn port_cost_model_history_calibration_uses_percentiles_once_enough_sessions_exist() {
    let history: Vec<f64> = (1..=20).map(f64::from).collect();
    let calibrated = calibrate("own_history", &[], Some(&history));
    assert_eq!(calibrated["note"], json!("ready"));
    assert_eq!(calibrated["basis"], json!("total_usd"));
    assert_eq!(level(&calibrated["cuts"], 3.0), json!(0));
    assert_eq!(level(&calibrated["cuts"], 19.5), json!(4));
    let thin = calibrate("own_history", &[], Some(&history[..3]));
    assert_eq!(thin["note"], json!("history_unavailable"));
    assert_eq!(thin["cuts"], json!([1.0, 5.0, 20.0, 50.0]));
    let missing = calibrate("own_history", &[], None);
    assert_eq!(missing["note"], json!("history_unavailable"));
}

/// REQ: PORT:ilium-client/src/cost_model.rs::longest_prefix_wins_and_dated_snapshots_match_their_family
#[test]
fn port_cost_model_longest_prefix_wins_and_dated_snapshots_match_their_family() {
    let none = json!({});
    assert_eq!(lookup(none.clone(), "claude-opus-5-5")["input"], json!(4.0));
    assert_eq!(lookup(none.clone(), "claude-opus-5")["input"], json!(5.0));
    assert_eq!(
        lookup(none.clone(), "claude-haiku-4-5-20251001")["input"],
        json!(1.0)
    );
    assert_eq!(lookup(none.clone(), "gpt-reserve"), Value::Null);
    assert_eq!(lookup(none, "unknown"), Value::Null);
}

/// REQ: PORT:ilium-client/src/cost_model.rs::meter_fills_one_cell_per_level_and_empty_without_data
#[test]
fn port_cost_model_meter_fills_one_cell_per_level_and_empty_without_data() {
    let meter = |level: u64| s("level-meter", json!({ "level": level }));
    assert_eq!(meter(0), json!("▰▱▱▱▱"));
    assert_eq!(meter(2), json!("▰▰▰▱▱"));
    assert_eq!(meter(99), json!("▰▰▰▰▰"));
    assert_eq!(s("meter-string", json!({ "level": null })), json!("▱▱▱▱▱"));
    assert_eq!(s("level-glyph", json!({ "level": 0 })), json!("▁"));
    assert_eq!(s("level-glyph", json!({ "level": 4 })), json!("█"));
}

/// REQ: PORT:ilium-client/src/cost_model.rs::overrides_beat_built_in_prices
#[test]
fn port_cost_model_overrides_beat_built_in_prices() {
    let price = |input: f64, output: f64, cache_read: f64, cache_write: f64| json!({ "input": input, "output": output, "cache_read": cache_read, "cache_write": cache_write });
    let overrides = json!({
        "GPT-Reserve": price(1.0, 2.0, 0.1, 0.0),
        "claude-sonnet-5": price(9.0, 9.0, 9.0, 9.0),
        "claude-haiku-4-5": price(7.0, 7.0, 7.0, 7.0),
    });
    assert_eq!(
        lookup(overrides.clone(), "gpt-reserve")["output"],
        json!(2.0)
    );
    // A longer built-in prefix beats a shorter override; equal length goes to the override.
    assert_eq!(
        lookup(overrides.clone(), "claude-sonnet-5-5")["input"],
        json!(2.0)
    );
    assert_eq!(
        lookup(overrides.clone(), "claude-sonnet-5")["input"],
        json!(9.0)
    );
    assert_eq!(
        lookup(overrides, "claude-haiku-4-5-20251001")["input"],
        json!(7.0)
    );
}

/// REQ: PORT:ilium-client/src/cost_model.rs::peer_calibration_scales_with_the_median_open_agent
#[test]
fn port_cost_model_peer_calibration_scales_with_the_median_open_agent() {
    let calibrated = calibrate("peer_relative", &[2.0, 4.0, 6.0, 0.0], None);
    assert_eq!(calibrated["cuts"], json!([1.0, 3.0, 6.0, 12.0]));
    let alone = calibrate("peer_relative", &[0.0], None);
    assert_eq!(alone["note"], json!("no_peers"));
}

/// REQ: PORT:ilium-client/src/cost_model.rs::percent_rank_counts_sessions_at_or_below
#[test]
fn port_cost_model_percent_rank_counts_sessions_at_or_below() {
    let rank =
        |sorted: &[f64], value: f64| s("percent-rank", json!({ "sorted": sorted, "value": value }));
    let sorted = [1.0, 2.0, 3.0, 4.0];
    assert_eq!(rank(&sorted, 0.5), json!(0.0));
    assert_eq!(rank(&sorted, 2.0), json!(50.0));
    assert_eq!(rank(&sorted, 99.0), json!(100.0));
    assert_eq!(rank(&[], 1.0), json!(0.0));
}

/// REQ: PORT:ilium-client/src/cost_model.rs::percentile_interpolates
#[test]
fn port_cost_model_percentile_interpolates() {
    let pct = |sorted: &[f64], fraction: f64| {
        s(
            "percentile",
            json!({ "sorted": sorted, "fraction": fraction }),
        )
    };
    let sorted = [1.0, 2.0, 3.0, 4.0, 5.0];
    assert_eq!(pct(&sorted, 0.0), json!(1.0));
    assert_eq!(pct(&sorted, 0.5), json!(3.0));
    assert!(near(&pct(&sorted, 0.25), 2.0));
    assert_eq!(pct(&sorted, 1.0), json!(5.0));
    assert_eq!(pct(&[], 0.5), json!(0.0));
    assert_eq!(pct(&[7.0], 0.9), json!(7.0));
}

/// REQ: PORT:ilium-client/src/cost_model.rs::scale_counts_reached_cuts
#[test]
fn port_cost_model_scale_counts_reached_cuts() {
    let cuts = json!([1.0, 5.0, 20.0, 50.0]);
    assert_eq!(level(&cuts, 0.0), json!(0));
    assert_eq!(level(&cuts, 0.99), json!(0));
    assert_eq!(level(&cuts, 1.0), json!(1));
    assert_eq!(level(&cuts, 19.9), json!(2));
    assert_eq!(level(&cuts, 49.0), json!(3));
    assert_eq!(level(&cuts, 10_000.0), json!(4));
}

/// REQ: PORT:ilium-client/src/cost_model.rs::sparkline_scales_to_the_series_peak
#[test]
fn port_cost_model_sparkline_scales_to_the_series_peak() {
    let glyphs = |values: &[f64]| s("sparkline-glyphs", json!({ "values": values }));
    assert_eq!(glyphs(&[0.0, 1.0, 2.0, 4.0, 8.0]), json!("▁▂▃▅█"));
    assert_eq!(glyphs(&[0.0, 0.0]), json!("▁▁"));
    assert_eq!(glyphs(&[]), json!(""));
    assert_eq!(glyphs(&[0.001, 100.0]), json!("▂█"));
}

/// REQ: PORT:ilium-client/src/cost_model.rs::spend_cells_bucket_the_window_and_ignore_outside_points
#[test]
fn port_cost_model_spend_cells_bucket_the_window_and_ignore_outside_points() {
    let hour: i64 = 3_600_000;
    let now = 10 * hour;
    let list = points(&[
        (now - 30 * 60_000, 1.0),
        (now - 5 * 60_000, 2.0),
        (now - 5 * hour, 99.0),
    ]);
    let cells = |window_ms: i64, count: u64| {
        s(
            "spend-cells",
            json!({ "points": list, "now_ms": now, "window_ms": window_ms, "cells": count }),
        )
    };
    assert_eq!(cells(hour, 4), json!([0.0, 0.0, 1.0, 2.0]));
    assert_eq!(cells(0, 4), json!([0.0, 0.0, 0.0, 0.0]));
    assert_eq!(cells(hour, 0), json!([]));
}

/// REQ: PORT:ilium-client/src/cost_model.rs::typed_windows_parse_units_and_reject_nonsense
#[test]
fn port_cost_model_typed_windows_parse_units_and_reject_nonsense() {
    let parse = |text: &str| s("parse-window-minutes", json!({ "text": text }));
    assert_eq!(parse("90"), json!(90));
    assert_eq!(parse(" 90m "), json!(90));
    assert_eq!(parse("6h"), json!(360));
    assert_eq!(parse("1.5 h"), json!(90));
    assert_eq!(parse("2 days"), json!(2880));
    assert_eq!(parse("0"), Value::Null);
    assert_eq!(parse("400d"), Value::Null);
    assert_eq!(parse("six"), Value::Null);
    assert_eq!(parse("5x"), Value::Null);
    assert_eq!(parse(""), Value::Null);
}

/// REQ: PORT:ilium-client/src/cost_model.rs::unpriced_models_make_the_estimate_a_lower_bound
#[test]
fn port_cost_model_unpriced_models_make_the_estimate_a_lower_bound() {
    let models = json!([
        { "model": "claude-haiku-4-5", "calls": 1, "tokens": tokens(1_000_000, 0, 0, 0) },
        { "model": "gpt-reserve", "calls": 1, "tokens": tokens(500, 0, 0, 500) },
    ]);
    let estimate = s("price-estimate", json!({ "models": models }));
    assert!(near(&estimate["usd"], 1.0), "{estimate}");
    assert_eq!(estimate["unpriced_tokens"], json!(1_000));
    assert_eq!(estimate["is_lower_bound"], json!(true));
}

/// REQ: PORT:ilium-client/src/cost_model.rs::window_formatting_is_compact
#[test]
fn port_cost_model_window_formatting_is_compact() {
    let window = |minutes: u64| s("format-window", json!({ "minutes": minutes }));
    assert_eq!(window(5), json!("5 min"));
    assert_eq!(window(90), json!("1 h 30 min"));
    assert_eq!(window(360), json!("6 h"));
    assert_eq!(window(1440), json!("1 d"));
    assert_eq!(window(2160), json!("1 d 12 h"));
    assert_eq!(window(43_200), json!("30 d"));
}
