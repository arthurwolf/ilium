//! Twins of ilium-client/src/session_stats_ui.rs: the statistics popover layout, tabs, formatting helpers and
//! scale buttons. Scenario ops `stats-ui.<name-with-dashes>` render the popover for the case described by the
//! request and answer `{text, ...}` with the rendered text and the geometry facts the original asserted
//! (see ../API_GAPS_ports.md).

use serde_json::{json, Value};

use crate::common::client;

fn scenario(name: &str, request: Value) -> Value {
    client(&format!("stats-ui.{name}"), request)
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::activity_and_prompts_tabs_list_their_content
#[test]
fn port_session_stats_ui_activity_and_prompts_tabs_list_their_content() {
    let reply = scenario(
        "activity-and-prompts-tabs-list-their-content",
        json!({ "tabs": ["activity", "prompts"], "tool": "Bash", "prompt": "fix the login bug", "repeats": 3 }),
    );
    assert!(
        reply["text"].as_str().unwrap_or("").contains("Bash"),
        "{reply}"
    );
    assert!(
        reply["text"].as_str().unwrap_or("").contains("TURNS"),
        "{reply}"
    );
    assert!(
        reply["text"]
            .as_str()
            .unwrap_or("")
            .contains("fix the login bug"),
        "{reply}"
    );
    assert!(
        reply["text"]
            .as_str()
            .unwrap_or("")
            .contains("×3 sent in a row"),
        "{reply}"
    );
    assert_eq!(reply["newest_before_oldest"], json!(true), "newest first");
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::dump_tabs
#[test]
fn port_session_stats_ui_dump_tabs() {
    let reply = scenario(
        "dump-tabs",
        json!({ "tabs": "all", "width": 120, "height": 40 }),
    );
    assert!(
        reply["tabs"].as_array().is_some_and(|tabs| !tabs.is_empty()
            && tabs
                .iter()
                .all(|tab| !tab["text"].as_str().unwrap_or("").is_empty())),
        "{reply}"
    );
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::every_tab_renders_without_panicking_on_small_and_large_terminals
#[test]
fn port_session_stats_ui_every_tab_renders_without_panicking_on_small_and_large_terminals() {
    let reply = scenario(
        "every-tab-renders-without-panicking-on-small-and-large-terminals",
        json!({ "tabs": "all", "sizes": [[60, 18], [120, 40], [250, 70]] }),
    );
    assert_eq!(reply["rendered"], json!(true));
    assert_eq!(reply["every_tab_has_title"], json!("Costs & stats"));
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::failed_refresh_keeps_retained_metrics_and_explains_incomplete_statistics
#[test]
fn port_session_stats_ui_failed_refresh_keeps_retained_metrics_and_explains_incomplete_statistics()
{
    let reply = scenario(
        "failed-refresh-keeps-retained-metrics-and-explains-incomplete-statistics",
        json!({ "retained": { "usd": 12.5, "model": "claude-sonnet-5" }, "failure": "filesystem entries limit reached: used 4096, at least 1 over", "tabs": "all" }),
    );
    assert!(
        reply["text"]
            .as_str()
            .unwrap_or("")
            .contains("Statistics may be incomplete"),
        "{reply}"
    );
    assert!(
        reply["text"]
            .as_str()
            .unwrap_or("")
            .contains("filesystem entries limit reached"),
        "{reply}"
    );
    assert!(
        reply["text"].as_str().unwrap_or("").contains("used 4096"),
        "{reply}"
    );
    assert!(
        reply["text"]
            .as_str()
            .unwrap_or("")
            .contains("at least 1 over"),
        "{reply}"
    );
    assert!(
        reply["text"].as_str().unwrap_or("").contains("$12.50"),
        "{reply}"
    );
    assert!(
        reply["text"]
            .as_str()
            .unwrap_or("")
            .contains("claude-sonnet-5"),
        "{reply}"
    );
    assert!(
        !reply["text"]
            .as_str()
            .unwrap_or("")
            .contains("No statistics available"),
        "{reply}"
    );
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::formatting_helpers
#[test]
fn port_session_stats_ui_formatting_helpers() {
    let reply = scenario(
        "formatting-helpers",
        json!({ "compact_count": [999, 12345, 4560000], "grouped_count": [1234567, 12], "duration_label_ms": [42000, 312000, 8040000, 273600000], "percent_label": [0.0004, 0.734], "tab_step": [["prompts", 1], ["overview", -1]] }),
    );
    assert_eq!(reply["compact_count"], json!(["999", "12.3k", "4.56M"]));
    assert_eq!(reply["grouped_count"], json!(["1,234,567", "12"]));
    assert_eq!(
        reply["duration_label"],
        json!(["42s", "5m 12s", "2h 14m", "3d 4h"])
    );
    assert_eq!(reply["percent_label"], json!(["<0.1%", "73%"]));
    assert_eq!(reply["tab_step"], json!(["overview", "prompts"]));
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::geometry_is_stable_and_clickable_regions_do_not_overlap
#[test]
fn port_session_stats_ui_geometry_is_stable_and_clickable_regions_do_not_overlap() {
    let reply = scenario(
        "geometry-is-stable-and-clickable-regions-do-not-overlap",
        json!({ "screen": [0, 0, 120, 40], "anchor": [2, 0] }),
    );
    assert_eq!(reply["area_within_screen"], json!(true));
    assert_eq!(reply["body_above_footer"], json!(true));
    assert_eq!(reply["tabs_inside_area"], json!(true));
    assert_eq!(reply["tabs_overlap"], json!(false), "tabs must not overlap");
    assert_eq!(reply["hit_below_area"], Value::Null);
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::geometry_refuses_terminals_that_are_too_small
#[test]
fn port_session_stats_ui_geometry_refuses_terminals_that_are_too_small() {
    let reply = scenario(
        "geometry-refuses-terminals-that-are-too-small",
        json!({ "screens": [[0, 0, 50, 40], [0, 0, 120, 10]], "anchor": [2, 0] }),
    );
    assert_eq!(reply["layouts"], json!([null, null]));
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::geometry_stays_on_screen_when_the_anchor_is_far_right
#[test]
fn port_session_stats_ui_geometry_stays_on_screen_when_the_anchor_is_far_right() {
    let reply = scenario(
        "geometry-stays-on-screen-when-the-anchor-is-far-right",
        json!({ "screen": [0, 0, 120, 40], "anchor": [119, 0] }),
    );
    assert_eq!(reply["area_within_screen"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::hover_popover_has_no_close_control_and_says_how_to_pin
#[test]
fn port_session_stats_ui_hover_popover_has_no_close_control_and_says_how_to_pin() {
    let reply = scenario(
        "hover-popover-has-no-close-control-and-says-how-to-pin",
        json!({ "pinned": false }),
    );
    assert!(
        !reply["text"].as_str().unwrap_or("").contains("✕ close"),
        "{reply}"
    );
    assert!(
        reply["text"]
            .as_str()
            .unwrap_or("")
            .contains("click ● to pin"),
        "{reply}"
    );
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::nothing_is_drawn_outside_the_popover_on_the_narrowest_terminal
#[test]
fn port_session_stats_ui_nothing_is_drawn_outside_the_popover_on_the_narrowest_terminal() {
    let reply = scenario(
        "nothing-is-drawn-outside-the-popover-on-the-narrowest-terminal",
        json!({ "screen": [0, 0, 60, 18] }),
    );
    assert_eq!(reply["cells_drawn_outside_area"], json!(0));
    assert_eq!(reply["close_inside_area"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::output_chart_has_work_ribbon_outcome_icons_legend_and_scale_buttons
#[test]
fn port_session_stats_ui_output_chart_has_work_ribbon_outcome_icons_legend_and_scale_buttons() {
    let reply = scenario(
        "output-chart-has-work-ribbon-outcome-icons-legend-and-scale-buttons",
        json!({ "tab": "overview", "scales": "all" }),
    );
    assert_eq!(reply["heading_has_rule"], json!(true));
    assert_eq!(
        reply["row_after_heading_has_timescale_and_every_label"],
        json!(true)
    );
    assert_eq!(reply["chart_has_braille_ribbon"], json!(true));
    assert_eq!(reply["outcome_icons"], json!(["✓", "✗"]));
    assert!(
        reply["text"]
            .as_str()
            .unwrap_or("")
            .contains("progress bar running"),
        "{reply}"
    );
    assert!(
        reply["text"].as_str().unwrap_or("").contains("thinking"),
        "{reply}"
    );
    assert_eq!(reply["axis_row_has_time"], json!(true));
    assert!(
        reply["scale_buttons"].as_u64().unwrap_or(0)
            >= reply["scale_count"].as_u64().unwrap_or(u64::MAX)
    );
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::overview_capture_shows_section_rules_at_a_wide_terminal_size
#[test]
fn port_session_stats_ui_overview_capture_shows_section_rules_at_a_wide_terminal_size() {
    let reply = scenario(
        "overview-capture-shows-section-rules-at-a-wide-terminal-size",
        json!({ "tab": "overview", "screen": [0, 0, 250, 70] }),
    );
    assert_eq!(reply["output_over_time_row_has_rule"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::overview_renders_the_headline_facts
#[test]
fn port_session_stats_ui_overview_renders_the_headline_facts() {
    let reply = scenario(
        "overview-renders-the-headline-facts",
        json!({ "tab": "overview", "model": "claude-sonnet-5", "running_ms": 2700000, "usd": 12.5 }),
    );
    assert!(
        reply["text"]
            .as_str()
            .unwrap_or("")
            .contains("Costs & stats"),
        "{reply}"
    );
    assert!(
        reply["text"]
            .as_str()
            .unwrap_or("")
            .contains("claude-sonnet-5"),
        "{reply}"
    );
    assert!(
        reply["text"].as_str().unwrap_or("").contains("RUNNING FOR"),
        "{reply}"
    );
    assert!(
        reply["text"].as_str().unwrap_or("").contains("45m 0s"),
        "{reply}"
    );
    assert!(
        reply["text"].as_str().unwrap_or("").contains("$12.50"),
        "{reply}"
    );
    assert!(
        reply["text"].as_str().unwrap_or("").contains("close"),
        "{reply}"
    );
    assert!(reply["content_rows"].as_u64().unwrap_or(0) > 0);
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::placeholders_explain_missing_data
#[test]
fn port_session_stats_ui_placeholders_explain_missing_data() {
    let reply = scenario(
        "placeholders-explain-missing-data",
        json!({ "cases": "every missing-data placeholder" }),
    );
    assert_eq!(
        reply["missing_expected"],
        json!([]),
        "every placeholder text is shown"
    );
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::scale_buttons_hit_test_and_narrow_the_window
#[test]
fn port_session_stats_ui_scale_buttons_hit_test_and_narrow_the_window() {
    let reply = scenario(
        "scale-buttons-hit-test-and-narrow-the-window",
        json!({ "scales": "all", "five_minutes_window_ms": 300000 }),
    );
    assert_eq!(reply["hit_matches_scale"], json!(true));
    assert_eq!(reply["hit_at_origin"], Value::Null);
    assert_eq!(reply["five_minute_window_ms"], json!(5 * 60_000));
    assert_eq!(reply["selected_scale_narrows_chart"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::scrolling_clamps_to_the_content
#[test]
fn port_session_stats_ui_scrolling_clamps_to_the_content() {
    let reply = scenario(
        "scrolling-clamps-to-the-content",
        json!({ "window": "short", "scroll_down": 1000, "scroll_up": 1000 }),
    );
    assert!(
        reply["max_scroll"].as_u64().unwrap_or(0) > 0,
        "a short window needs scrolling"
    );
    assert_eq!(reply["scroll_after_down"], reply["max_scroll"]);
    assert_eq!(reply["scroll_after_up"], json!(0));
    assert_eq!(reply["scrolled_differs_from_top"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::segmented_bar_always_fills_the_requested_width
#[test]
fn port_session_stats_ui_segmented_bar_always_fills_the_requested_width() {
    let reply = scenario(
        "segmented-bar-always-fills-the-requested-width",
        json!({ "widths": [1, 2, 7, 20, 80], "segments": [0.5, 0.3, 0.2] }),
    );
    assert_eq!(reply["widths_match"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::the_popover_fills_the_right_panel_and_never_leaves_it
#[test]
fn port_session_stats_ui_the_popover_fills_the_right_panel_and_never_leaves_it() {
    let reply = scenario(
        "the-popover-fills-the-right-panel-and-never-leaves-it",
        json!({ "panel": [100, 0, 40, 50], "icon_row": 0, "wide_panel_width": 250 }),
    );
    assert_eq!(reply["area_x"], reply["panel_x_plus_one"]);
    assert_eq!(reply["area_right"], reply["panel_right_minus_one"]);
    assert_eq!(reply["area_bottom"], reply["panel_bottom_minus_one"]);
    assert_eq!(
        reply["area_y"],
        json!(1),
        "hangs directly below the icon row"
    );
    assert_eq!(reply["wide_area_width"], json!(248));
    assert_eq!(reply["wide_area_taller_than_36"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::tokens_tab_shows_breakdown_models_and_a_context_chart
#[test]
fn port_session_stats_ui_tokens_tab_shows_breakdown_models_and_a_context_chart() {
    let reply = scenario(
        "tokens-tab-shows-breakdown-models-and-a-context-chart",
        json!({ "tab": "tokens" }),
    );
    assert!(
        reply["text"].as_str().unwrap_or("").contains("BREAKDOWN"),
        "{reply}"
    );
    assert!(
        reply["text"].as_str().unwrap_or("").contains("cache read"),
        "{reply}"
    );
    assert!(
        reply["text"].as_str().unwrap_or("").contains("BY MODEL"),
        "{reply}"
    );
    assert!(
        reply["text"]
            .as_str()
            .unwrap_or("")
            .contains("CONTEXT WINDOW OVER TIME"),
        "{reply}"
    );
    assert!(
        reply["text"]
            .as_str()
            .unwrap_or("")
            .contains("model context window"),
        "{reply}"
    );
    assert_eq!(reply["chart_glyphs_present"], json!(["┤", "╭", "─"]));
    assert!(reply["content_rows"].as_u64().unwrap_or(0) > 20);
}

/// REQ: PORT:ilium-client/src/session_stats_ui.rs::unsupported_agents_are_told_why_there_are_no_statistics
#[test]
fn port_session_stats_ui_unsupported_agents_are_told_why_there_are_no_statistics() {
    let reply = scenario(
        "unsupported-agents-are-told-why-there-are-no-statistics",
        json!({ "agent": "unsupported" }),
    );
    assert!(
        reply["text"]
            .as_str()
            .unwrap_or("")
            .contains("No statistics for this agent"),
        "{reply}"
    );
}
