//! Twins of ilium-client/src/cost_settings.rs: the cost settings model, its rows, ladders and TOML form.
//!
//! `settings.apply` takes `{settings:{...overrides}, actions:[...]}` where an action is
//! `{adjust:{row,direction}}`, `{set_number:{row,text}}`, `{sanitize:true}` or `{set:{field:value}}`; the reply
//! carries the final `settings`, a `results` list (`true`/`false` for adjust, `{ok}`/`{err}` for set_number) and,
//! when `rows` is requested, the visible `rows` plus their `help_ids`.

use serde_json::{json, Value};

use crate::common::client;

fn apply(settings: Value, actions: Value) -> Value {
    client(
        "settings.apply",
        json!({ "settings": settings, "actions": actions }),
    )
}

fn adjust(row: &str, direction: i64) -> Value {
    json!({ "adjust": { "row": row, "direction": direction } })
}

fn set_number(row: &str, text: &str) -> Value {
    json!({ "set_number": { "row": row, "text": text } })
}

fn is_err(result: &Value) -> bool {
    result.get("err").is_some()
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::always_visible_options_show_without_hover
#[test]
fn port_cost_settings_always_visible_options_show_without_hover() {
    let reply = apply(
        json!({}),
        json!([adjust("visibility:meter", 0), { "probe": { "option": "meter", "hovered": false } }]),
    );
    assert_eq!(reply["probes"][0], json!(true));
    let reply = apply(
        json!({}),
        json!([adjust("visibility:meter", 0), adjust("display:meter", 0), { "probe": { "option": "meter", "hovered": true } }]),
    );
    assert_eq!(reply["probes"][0], json!(false));
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::bad_quota_values_are_repaired
#[test]
fn port_cost_settings_bad_quota_values_are_repaired() {
    let reply = apply(
        json!({ "quota_fixed_cuts": [5.0, 1.0, 2.0, 3.0], "quota_burn_cuts": [0.0, 1.0, 2.0, 3.0], "quota_budget_percent": null }),
        json!([{ "sanitize": true }]),
    );
    let defaults = apply(json!({}), json!([]));
    assert_eq!(
        reply["settings"]["quota_fixed_cuts"],
        defaults["settings"]["quota_fixed_cuts"]
    );
    assert_eq!(
        reply["settings"]["quota_burn_cuts"],
        defaults["settings"]["quota_burn_cuts"]
    );
    assert_eq!(reply["settings"]["quota_budget_percent"], json!(10.0));
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::defaults_show_only_the_meter_and_only_on_hover
#[test]
fn port_cost_settings_defaults_show_only_the_meter_and_only_on_hover() {
    let reply = apply(json!({}), json!([]));
    let settings = &reply["settings"];
    assert_eq!(settings["calibration"], json!("own_history"));
    assert_eq!(settings["sparkline_window_minutes"], json!(360));
    for display in reply["displays"].as_array().cloned().unwrap_or_default() {
        let name = display.as_str().unwrap_or("");
        let option = &settings[name];
        assert_eq!(option["enabled"], json!(name == "meter"), "{name}");
        assert_eq!(option["visibility"], json!("hover"), "{name}");
    }
    assert_eq!(reply["shown"]["meter"]["hovered"], json!(true));
    assert_eq!(reply["shown"]["meter"]["idle"], json!(false));
    assert_eq!(reply["shown"]["level_glyph"]["hovered"], json!(false));
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::direct_numbers_accept_valid_intermediates_without_ladder_quantization
#[test]
fn port_cost_settings_direct_numbers_accept_valid_intermediates_without_ladder_quantization() {
    let reply = apply(
        json!({}),
        json!([
            set_number("budget", "17.125"),
            set_number("history_days", "17"),
            set_number("sparkline_window", "101"),
            set_number("sparkline_cells", "13"),
            { "set": { "metric": "quota" } },
            set_number("budget", "125.5"),
        ]),
    );
    let settings = &reply["settings"];
    assert_eq!(settings["history_days"], json!(17));
    assert_eq!(settings["sparkline_window_minutes"], json!(101));
    assert_eq!(settings["sparkline_cells"], json!(13));
    assert_eq!(settings["quota_budget_percent"], json!(125.5));
    assert_eq!(settings["budget_usd"], json!(17.125));
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::direct_sparkline_window_preserves_native_duration_entry_and_rejects_invalid_drafts
#[test]
fn port_cost_settings_direct_sparkline_window_preserves_native_duration_entry_and_rejects_invalid_drafts(
) {
    for (text, minutes) in [("10h", 600), ("45m", 45), ("3d", 4320), ("1.5 h", 90)] {
        let reply = apply(json!({}), json!([set_number("sparkline_window", text)]));
        assert_eq!(
            reply["settings"]["sparkline_window_minutes"],
            json!(minutes),
            "{text}"
        );
    }
    for text in ["5x", "0m", "366d", "NaN", ""] {
        let reply = apply(
            json!({ "sparkline_window_minutes": 90 }),
            json!([set_number("sparkline_window", text)]),
        );
        assert!(is_err(&reply["results"][0]), "{text}");
        // An invalid duration must retain the prior value.
        assert_eq!(
            reply["settings"]["sparkline_window_minutes"],
            json!(90),
            "{text}"
        );
    }
}

fn all_row_help_ids() -> Vec<String> {
    let mut ids = std::collections::BTreeSet::new();
    for metric in ["dollars", "quota"] {
        for calibration in [
            "own_history",
            "fixed_bands",
            "budget",
            "burn_rate",
            "peer_relative",
        ] {
            let reply = client(
                "settings.rows",
                json!({ "settings": { "metric": metric, "calibration": calibration } }),
            );
            for id in reply["help_ids"].as_array().cloned().unwrap_or_default() {
                ids.insert(id.as_str().unwrap_or("").to_string());
            }
        }
    }
    ids.into_iter().collect()
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::every_row_has_a_distinct_catalogued_help_id
#[test]
fn port_cost_settings_every_row_has_a_distinct_catalogued_help_id() {
    // COST-01 through COST-23.
    assert_eq!(all_row_help_ids().len(), 23);
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::every_row_has_a_distinct_or_shared_help_id_in_range
#[test]
fn port_cost_settings_every_row_has_a_distinct_or_shared_help_id_in_range() {
    for id in all_row_help_ids() {
        let number: u32 = id
            .strip_prefix("COST-")
            .and_then(|digits| digits.parse().ok())
            .unwrap_or(0);
        assert!((1..=23).contains(&number), "{id}");
    }
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::invalid_direct_number_never_changes_settings
#[test]
fn port_cost_settings_invalid_direct_number_never_changes_settings() {
    for (row, text) in [
        ("budget", "NaN"),
        ("budget", "0"),
        ("budget", "1e400"),
        ("history_days", "3651"),
        ("history_days", "1.5"),
        ("sparkline_window", "525601"),
        ("sparkline_cells", "25"),
        ("sparkline_cells", "3"),
        ("sort_by_cost", "1"),
    ] {
        let before = apply(json!({}), json!([]));
        let reply = apply(json!({}), json!([set_number(row, text)]));
        assert!(is_err(&reply["results"][0]), "{row}: {text}");
        assert_eq!(reply["settings"], before["settings"], "{row}: {text}");
    }
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::numeric_ladders_preserve_endpoints_and_values_outside_presets
#[test]
fn port_cost_settings_numeric_ladders_preserve_endpoints_and_values_outside_presets() {
    let step = |ladder: &str, current: Value, direction: i64| {
        client(
            "settings.step-ladder",
            json!({ "ladder": ladder, "current": current, "direction": direction }),
        )
    };
    for (current, direction, expected) in [
        (730, 1, 730),
        (7, -1, 7),
        (1000, 1, 1000),
        (1, -1, 1),
        (17, 1, 30),
        (17, -1, 14),
    ] {
        assert_eq!(
            step("history_day_steps", json!(current), direction),
            json!(expected),
            "{current} {direction}"
        );
    }
    assert_eq!(step("budget_steps", json!(1500.5), 1), json!(1500.5));
    assert_eq!(step("quota_budget_steps", json!(0.1), -1), json!(0.1));
    assert_eq!(step("empty", json!(7), 1), json!(7));
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::presets_wrap_and_custom_cuts_snap
#[test]
fn port_cost_settings_presets_wrap_and_custom_cuts_snap() {
    let run = |settings: Value, direction: i64| {
        apply(settings, json!([adjust("fixed_preset", direction)]))
    };
    let presets = client("settings.presets", json!({}));
    let preset = |index: usize| presets["fixed_presets"][index].clone();
    let default = run(json!({}), 1);
    assert_eq!(
        apply(json!({}), json!([]))["settings"]["fixed_cuts"],
        preset(2)
    );
    assert_eq!(default["settings"]["fixed_cuts"], preset(3));
    assert_eq!(
        run(json!({ "fixed_cuts": [0.7, 3.0, 9.0, 30.0] }), 1)["settings"]["fixed_cuts"],
        preset(2)
    );
    assert_eq!(
        run(json!({ "fixed_cuts": preset(0) }), -1)["settings"]["fixed_cuts"],
        preset(4)
    );
    assert_eq!(
        client("settings.is-preset", json!({ "cuts": preset(1) })),
        json!(true)
    );
    assert_eq!(
        client(
            "settings.is-preset",
            json!({ "cuts": [1.0, 2.0, 3.0, 4.0] })
        ),
        json!(false)
    );
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::quota_metric_adds_its_window_row_and_keeps_dollar_values_apart
#[test]
fn port_cost_settings_quota_metric_adds_its_window_row_and_keeps_dollar_values_apart() {
    let dollars = client("settings.rows", json!({ "settings": {} }));
    assert_eq!(dollars["settings"]["metric"], json!("dollars"));
    assert!(!dollars["rows"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .contains(&json!("quota_window")));

    let reply = apply(json!({}), json!([adjust("metric:dollars", 1)]));
    assert_eq!(reply["results"][0], json!(true));
    let rows = client(
        "settings.rows",
        json!({ "settings": reply["settings"].clone() }),
    )["rows"]
        .clone();
    assert_eq!(rows[0], json!("metric:quota"));
    assert_eq!(rows[1], json!("quota_window"));
    assert_eq!(rows[2], json!("calibration:own_history"));

    // The parameter rows now edit the quota values, not the dollar ones.
    let dollar_bands = reply["settings"]["fixed_cuts"].clone();
    let presets = client("settings.presets", json!({}));
    let next = apply(
        reply["settings"].clone(),
        json!([
            adjust("fixed_preset", 1),
            adjust("budget", 1),
            adjust("burn_preset", -1)
        ]),
    );
    assert_eq!(next["results"], json!([true, true, true]));
    assert_eq!(next["settings"]["fixed_cuts"], dollar_bands);
    assert_eq!(
        next["settings"]["quota_fixed_cuts"],
        presets["quota_fixed_presets"][2]
    );
    assert_eq!(next["settings"]["budget_usd"], json!(10.0));
    assert_eq!(next["settings"]["quota_budget_percent"], json!(20.0));
    assert_eq!(
        next["settings"]["quota_burn_cuts"],
        presets["quota_burn_presets"][0]
    );
    assert_eq!(next["active_fixed_cuts"], presets["quota_fixed_presets"][2]);
    assert_eq!(next["active_budget"], json!(20.0));

    let window = apply(next["settings"].clone(), json!([adjust("quota_window", 0)]));
    assert_eq!(window["results"][0], json!(true));
    assert_eq!(window["settings"]["quota_window"], json!("secondary"));
    let window = apply(
        window["settings"].clone(),
        json!([adjust("quota_window", 1)]),
    );
    assert_eq!(window["settings"]["quota_window"], json!("primary"));

    let back = apply(
        window["settings"].clone(),
        json!([adjust("metric:dollars", 0)]),
    );
    assert_eq!(back["active_fixed_cuts"], dollar_bands);
    let rows = client(
        "settings.rows",
        json!({ "settings": back["settings"].clone() }),
    )["rows"]
        .clone();
    assert!(!rows
        .as_array()
        .cloned()
        .unwrap_or_default()
        .contains(&json!("quota_window")));
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::rows_follow_the_selected_calibration
#[test]
fn port_cost_settings_rows_follow_the_selected_calibration() {
    let rows_with = |calibration: &str| -> Vec<Value> {
        client(
            "settings.rows",
            json!({ "settings": { "calibration": calibration } }),
        )["rows"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    };
    assert!(rows_with("own_history").contains(&json!("history_days")));
    assert!(!rows_with("own_history").contains(&json!("budget")));
    assert!(rows_with("budget").contains(&json!("budget")));
    assert!(!rows_with("budget").contains(&json!("history_days")));
    assert!(rows_with("burn_rate").contains(&json!("burn_preset")));
    assert!(rows_with("fixed_bands").contains(&json!("fixed_preset")));
    // Metric selector, calibration selector, 16 options, 3 tail rows.
    assert_eq!(rows_with("peer_relative").len(), 1 + 1 + 16 + 3);
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::sanitizing_repairs_hand_edited_nonsense
#[test]
fn port_cost_settings_sanitizing_repairs_hand_edited_nonsense() {
    let reply = apply(
        json!({
            "fixed_cuts": [5.0, 1.0, 2.0, 3.0], "burn_cuts": [0.0, 1.0, 2.0, 3.0], "budget_usd": null,
            "history_days": 0, "sparkline_window_minutes": 0, "sparkline_cells": 200,
        }),
        json!([{ "sanitize": true }]),
    );
    let defaults = apply(json!({}), json!([]));
    let settings = &reply["settings"];
    assert_eq!(settings["fixed_cuts"], defaults["settings"]["fixed_cuts"]);
    assert_eq!(settings["burn_cuts"], defaults["settings"]["burn_cuts"]);
    assert_eq!(settings["budget_usd"], defaults["settings"]["budget_usd"]);
    assert_eq!(settings["history_days"], json!(1));
    assert_eq!(settings["sparkline_window_minutes"], json!(1));
    assert_eq!(
        settings["sparkline_cells"],
        defaults["sparkline_cell_range"][1]
    );
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::sparkline_cells_are_clamped
#[test]
fn port_cost_settings_sparkline_cells_are_clamped() {
    let range = apply(json!({}), json!([]))["sparkline_cell_range"].clone();
    let high = apply(
        json!({ "sparkline_cells": range[1] }),
        json!([adjust("sparkline_cells", 1)]),
    );
    assert_eq!(high["results"][0], json!(false));
    let low = apply(
        json!({ "sparkline_cells": range[0] }),
        json!([adjust("sparkline_cells", -1)]),
    );
    assert_eq!(low["results"][0], json!(false));
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::sparkline_window_steps_through_the_ladder_and_snaps_custom_values
#[test]
fn port_cost_settings_sparkline_window_steps_through_the_ladder_and_snaps_custom_values() {
    let up = apply(json!({}), json!([adjust("sparkline_window", 1)]));
    assert_eq!(up["results"][0], json!(true));
    assert_eq!(up["settings"]["sparkline_window_minutes"], json!(480));
    let down = apply(
        up["settings"].clone(),
        json!([adjust("sparkline_window", -1)]),
    );
    assert_eq!(down["settings"]["sparkline_window_minutes"], json!(360));
    let snap_up = apply(
        json!({ "sparkline_window_minutes": 100 }),
        json!([adjust("sparkline_window", 1)]),
    );
    assert_eq!(snap_up["settings"]["sparkline_window_minutes"], json!(120));
    let snap_down = apply(
        json!({ "sparkline_window_minutes": 100 }),
        json!([adjust("sparkline_window", -1)]),
    );
    assert_eq!(snap_down["settings"]["sparkline_window_minutes"], json!(90));
    let top = apply(
        json!({ "sparkline_window_minutes": 43_200 }),
        json!([adjust("sparkline_window", 1)]),
    );
    assert_eq!(top["results"][0], json!(false));
    assert_eq!(top["settings"]["sparkline_window_minutes"], json!(43_200));
}

/// REQ: PORT:ilium-client/src/cost_settings.rs::toml_round_trip_keeps_nested_options_and_accepts_partial_files
#[test]
fn port_cost_settings_toml_round_trip_keeps_nested_options_and_accepts_partial_files() {
    let reply = apply(
        json!({ "prices": { "gpt-reserve": { "input": 1.0, "output": 2.0, "cache_read": 0.1, "cache_write": 0.0 } } }),
        json!([
            adjust("display:sparkline", 0),
            adjust("visibility:sparkline", 0)
        ]),
    );
    let round = client(
        "settings.toml-round-trip",
        json!({ "settings": reply["settings"].clone() }),
    );
    assert_eq!(round["settings"], reply["settings"]);

    let partial = client(
        "settings.toml-parse",
        json!({ "toml": "calibration = \"budget\"\n[meter]\nvisibility = \"always\"\n" }),
    );
    assert_eq!(partial["settings"]["calibration"], json!("budget"));
    assert_eq!(partial["settings"]["meter"]["visibility"], json!("always"));
    assert_eq!(partial["settings"]["meter"]["enabled"], json!(false));
    assert_eq!(partial["settings"]["sparkline_window_minutes"], json!(360));
}
