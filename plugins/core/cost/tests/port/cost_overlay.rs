//! Twins of ilium-client/src/cost_overlay.rs: the per-row cost segments, the detail card and the title suffix.
//!
//! `overlay.segments` takes `{row:{level,amount,...}, settings:{...overrides}, adjust:[{row,direction}],
//! hovered, is_group}` and answers `{text, texts}`. `overlay.draw-segments` draws segments into a one-row
//! buffer and answers `{start, row_text, bg:{col:color}, fg:{col:color}, symbol:{col:str}}`.

use serde_json::{json, Value};

use crate::common::client;

fn row(level: u64, amount: f64) -> Value {
    json!({
        "level": level, "amount": amount, "is_lower_bound": false, "burn_per_hour": 0.0, "spark": "",
        "is_spike": false, "is_over_budget": false, "is_loading": false, "is_unavailable": false,
    })
}

fn segments(request: Value) -> Value {
    client("overlay.segments", request)
}

fn display_adjust(display: &str) -> Value {
    json!({ "row": format!("display:{display}"), "direction": 0 })
}

fn visibility_adjust(display: &str) -> Value {
    json!({ "row": format!("visibility:{display}"), "direction": 0 })
}

/// REQ: PORT:ilium-client/src/cost_overlay.rs::default_settings_show_the_meter_only_while_hovered
#[test]
fn port_cost_overlay_default_settings_show_the_meter_only_while_hovered() {
    let idle = segments(
        json!({ "row": row(2, 6.0), "settings": {}, "hovered": false, "is_group": false }),
    );
    assert_eq!(idle["texts"], json!([]));
    let hovered =
        segments(json!({ "row": row(2, 6.0), "settings": {}, "hovered": true, "is_group": false }));
    assert_eq!(hovered["text"], json!("▰▰▰▱▱"));
}

/// REQ: PORT:ilium-client/src/cost_overlay.rs::detail_card_for_quota_shows_percent_and_explains_missing_data
#[test]
fn port_cost_overlay_detail_card_for_quota_shows_percent_and_explains_missing_data() {
    let settings = json!({ "metric": "quota", "calibration": "fixed_bands" });
    let mut cost = json!({
        "usd": 4.0, "is_lower_bound": false, "burn_usd_per_hour": 0.0, "is_spike": false, "spark_cells": [],
        "quota_points": 7.5, "quota_burn_per_hour": 3.0, "has_quota": true, "model_costs": [],
        "reported_usd": null, "quota": [["primary", 41.0]], "spend_points": [],
    });
    let page = client(
        "overlay.detail-card",
        json!({ "cost": cost.clone(), "row": row(3, 7.5), "settings": settings.clone() }),
    );
    let page = page["text"].as_str().unwrap_or("").to_string();
    assert!(page.contains("Quota  7.5%"), "{page}");
    assert!(page.contains("3.0%/h"), "{page}");
    assert!(page.contains("$4.00 (API-equivalent estimate)"), "{page}");
    assert!(
        page.contains("primary window 41% used account-wide"),
        "{page}"
    );
    cost["has_quota"] = json!(false);
    let page = client(
        "overlay.detail-card",
        json!({ "cost": cost, "row": row(0, 0.0), "settings": settings }),
    );
    let page = page["text"].as_str().unwrap_or("").to_string();
    assert!(page.contains("not recorded by this agent's CLI"), "{page}");
}

fn draw(
    width: u64,
    background: Option<&str>,
    title: Option<&str>,
    start_col: u64,
    end_col: u64,
    texts: &[(&str, &str)],
) -> Value {
    let segs: Vec<Value> = texts
        .iter()
        .map(|(text, fg)| json!({ "text": text, "fg": fg, "bold": false }))
        .collect();
    client(
        "overlay.draw-segments",
        json!({ "width": width, "background": background, "title": title, "row": 0, "start_col": start_col, "end_col": end_col, "segments": segs }),
    )
}

/// REQ: PORT:ilium-client/src/cost_overlay.rs::drawing_keeps_the_row_background_and_drops_leading_segments_to_fit
#[test]
fn port_cost_overlay_drawing_keeps_the_row_background_and_drops_leading_segments_to_fit() {
    // Room for 12 - 2 = 10 cells: sparkline + meter + dollars need 17 and meter + dollars need 11,
    // so only the dollars remain.
    let reply = draw(
        12,
        Some("magenta"),
        None,
        2,
        12,
        &[("▁▃█▅▂", "red"), ("▰▰▱▱▱", "red"), ("$9.99", "red")],
    );
    assert_eq!(reply["start"], json!(7));
    let rendered = reply["row_text"].as_str().unwrap_or("");
    assert!(
        !rendered.contains('▁') && !rendered.contains('▰'),
        "{rendered}"
    );
    assert!(rendered.contains("$9.99"));
    assert_eq!(reply["bg"]["11"], json!("magenta"));
    assert_eq!(reply["bg"]["7"], json!("magenta"));
}

/// REQ: PORT:ilium-client/src/cost_overlay.rs::drawing_right_aligns_before_the_edge_and_overwrites_title_text
#[test]
fn port_cost_overlay_drawing_right_aligns_before_the_edge_and_overwrites_title_text() {
    let reply = draw(
        30,
        None,
        Some("a-long-agent-title-that-runs-on"),
        4,
        26,
        &[("▰▰▱▱▱", "green")],
    );
    assert_eq!(reply["start"], json!(21));
    let rendered = reply["row_text"].as_str().unwrap_or("");
    assert_eq!(&rendered[..21], "a-long-agent-title-th");
    assert!(rendered.contains("▰▰▱▱▱"));
    assert_eq!(reply["fg"]["21"], json!("green"));
    // Cells right of the edge are untouched.
    assert_eq!(reply["symbol"]["26"], json!("n"));
}

/// REQ: PORT:ilium-client/src/cost_overlay.rs::group_rows_show_only_their_total_and_only_when_enabled
#[test]
fn port_cost_overlay_group_rows_show_only_their_total_and_only_when_enabled() {
    let off =
        segments(json!({ "row": row(2, 6.0), "settings": {}, "hovered": true, "is_group": true }));
    assert_eq!(off["texts"], json!([]));
    let on = segments(json!({
        "row": row(2, 6.0), "settings": {}, "adjust": [display_adjust("group_totals")], "hovered": true, "is_group": true,
    }));
    assert_eq!(on["text"], json!("▄ $6.00"));
}

/// REQ: PORT:ilium-client/src/cost_overlay.rs::loading_rows_show_an_empty_track_and_an_ellipsis
#[test]
fn port_cost_overlay_loading_rows_show_an_empty_track_and_an_ellipsis() {
    let mut loading = row(0, 0.0);
    loading["level"] = Value::Null;
    loading["is_loading"] = json!(true);
    let reply = segments(json!({
        "row": loading, "settings": {}, "adjust": [display_adjust("level_dollars")], "hovered": true, "is_group": false,
    }));
    assert_eq!(reply["text"], json!("▱▱▱▱▱ · …"));
}

/// REQ: PORT:ilium-client/src/cost_overlay.rs::lower_bounds_and_budget_overruns_are_marked
#[test]
fn port_cost_overlay_lower_bounds_and_budget_overruns_are_marked() {
    let mut over = row(4, 25.0);
    over["is_lower_bound"] = json!(true);
    over["is_over_budget"] = json!(true);
    let reply = segments(json!({
        "row": over, "settings": {}, "adjust": [display_adjust("level_dollars")], "hovered": true, "is_group": false,
    }));
    assert_eq!(reply["text"], json!("▰▰▰▰▰ █ ~$25.0 !"));
}

/// REQ: PORT:ilium-client/src/cost_overlay.rs::nothing_is_drawn_when_not_even_one_segment_fits
#[test]
fn port_cost_overlay_nothing_is_drawn_when_not_even_one_segment_fits() {
    let reply = draw(6, None, None, 3, 6, &[("▰▰▱▱▱", "red")]);
    assert_eq!(reply["start"], Value::Null);
    assert_eq!(reply["row_text"].as_str().unwrap_or("x").trim(), "");
}

/// REQ: PORT:ilium-client/src/cost_overlay.rs::options_compose_in_a_fixed_order_and_dollars_supersede_the_bare_glyph
#[test]
fn port_cost_overlay_options_compose_in_a_fixed_order_and_dollars_supersede_the_bare_glyph() {
    let mut spiking = row(3, 12.0);
    spiking["is_spike"] = json!(true);
    spiking["spark"] = json!("▁▃█");
    let adjust: Vec<Value> = ["sparkline", "level_glyph", "level_dollars", "burn_marker"]
        .iter()
        .map(|d| display_adjust(d))
        .collect();
    let reply = segments(
        json!({ "row": spiking.clone(), "settings": {}, "adjust": adjust.clone(), "hovered": true, "is_group": false }),
    );
    assert_eq!(reply["text"], json!("▁▃█ ▰▰▰▰▱ ▆ $12.0 ↑"));
    let reply = segments(json!({
        "row": spiking, "settings": { "level_dollars": { "enabled": false } }, "adjust": adjust,
        "disable_after_adjust": ["level_dollars"], "hovered": true, "is_group": false,
    }));
    assert_eq!(reply["text"], json!("▁▃█ ▰▰▰▰▱ ▆ ↑"));
}

/// REQ: PORT:ilium-client/src/cost_overlay.rs::quota_rows_draw_percent_and_unavailable_rows_draw_nothing
#[test]
fn port_cost_overlay_quota_rows_draw_percent_and_unavailable_rows_draw_nothing() {
    let settings = json!({ "metric": "quota" });
    let adjust = json!([
        display_adjust("level_dollars"),
        visibility_adjust("level_dollars")
    ]);
    let reply = segments(
        json!({ "row": row(2, 6.2), "settings": settings.clone(), "adjust": adjust.clone(), "hovered": false, "is_group": false }),
    );
    let texts = reply["texts"].as_array().cloned().unwrap_or_default();
    assert!(texts.contains(&json!("6.2%")), "{texts:?}");
    assert!(!texts
        .iter()
        .any(|text| text.as_str().unwrap_or("").contains('$')));
    let mut unavailable = row(0, 0.0);
    unavailable["level"] = Value::Null;
    unavailable["is_unavailable"] = json!(true);
    let reply = segments(
        json!({ "row": unavailable, "settings": settings, "adjust": adjust, "hovered": true, "is_group": false }),
    );
    assert_eq!(reply["texts"], json!([]));
}

/// REQ: PORT:ilium-client/src/cost_overlay.rs::title_suffix_follows_its_visibility_and_needs_agents
#[test]
fn port_cost_overlay_title_suffix_follows_its_visibility_and_needs_agents() {
    let suffix = |overlay: Value, hovered: bool| {
        client(
            "overlay.title-suffix",
            json!({ "overlay": overlay, "hovered": hovered }),
        )
    };
    let base = json!({ "agent_count": 2, "total_amount": 52.34, "settings": {} });
    // The header total is off by default.
    assert_eq!(suffix(base.clone(), true), Value::Null);
    let enabled = json!({ "agent_count": 2, "total_amount": 52.34, "settings": { "header_total": { "enabled": true } } });
    // Hover-only by default.
    assert_eq!(suffix(enabled.clone(), false), Value::Null);
    assert_eq!(suffix(enabled, true), json!(" · Σ $52.3"));
    let always = json!({ "agent_count": 2, "total_amount": 52.34, "settings": { "header_total": { "enabled": true, "visibility": "always" } } });
    assert_eq!(suffix(always.clone(), false), json!(" · Σ $52.3"));
    let mut lower = always.clone();
    lower["total_is_lower_bound"] = json!(true);
    assert_eq!(suffix(lower, false), json!(" · Σ ~$52.3"));
    let mut none = always;
    none["agent_count"] = json!(0);
    assert_eq!(suffix(none, false), Value::Null);
}

/// REQ: PORT:ilium-client/src/cost_overlay.rs::visibility_is_decided_per_option
#[test]
fn port_cost_overlay_visibility_is_decided_per_option() {
    let adjust = json!([
        display_adjust("level_dollars"),
        visibility_adjust("level_dollars")
    ]);
    let idle = segments(
        json!({ "row": row(1, 2.0), "settings": {}, "adjust": adjust.clone(), "hovered": false, "is_group": false }),
    );
    assert_eq!(idle["level_dollars_visibility"], json!("always"));
    // The always-visible option shows without hover.
    assert_eq!(idle["text"], json!("▂ $2.00"));
    let hovered = segments(
        json!({ "row": row(1, 2.0), "settings": {}, "adjust": adjust, "hovered": true, "is_group": false }),
    );
    assert_eq!(hovered["text"], json!("▰▰▱▱▱ ▂ $2.00"));
}
